use std::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU64,
        Ordering::{AcqRel, Acquire, Relaxed, Release},
    },
};

use rayon::{ThreadPool, ThreadPoolBuilder};

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum UsageFlag {
    Used,
    RegionEnd,
    NextStored(usize),
}

impl From<u64> for UsageFlag {
    fn from(value: u64) -> Self {
        if value == u64::MAX {
            UsageFlag::Used
        } else if value == u64::MAX - 1 {
            UsageFlag::RegionEnd
        } else {
            UsageFlag::NextStored(usize::try_from(value).unwrap())
        }
    }
}

impl From<UsageFlag> for u64 {
    fn from(value: UsageFlag) -> Self {
        match value {
            UsageFlag::Used => u64::MAX,
            UsageFlag::RegionEnd => u64::MAX - 1,
            UsageFlag::NextStored(x) => u64::try_from(x).unwrap(),
        }
    }
}
fn generate_current_position(size: usize, n_pools: usize) -> impl Iterator<Item = usize> {
    const EXTRA_SPACE: usize = 4;
    let gap = size / (n_pools + EXTRA_SPACE);
    (0..n_pools).map(move |x| {
        if x == 0 {
            //skip the first two
            2
        } else {
            //We give the first pos a lot of extra space since the main thread is used more.
            (x + EXTRA_SPACE) * gap
        }
    })
}

pub const REGION_SIZE: usize = 12;

fn generate_next_free(size: usize) -> impl Iterator<Item = UsageFlag> {
    (1..=size).map(move |i| {
        if i <= 2 || i == size {
            //Mark 0 and 1 as used.
            UsageFlag::Used
        } else if i % REGION_SIZE == 0 {
            UsageFlag::RegionEnd
        } else {
            UsageFlag::NextStored(i)
        }
    })
}

///A struct for managing which slots are free in the concurrent data structure.
#[derive(Debug)]
pub(super) struct SharedLinkedList {
    next_free: UnsafeCell<Vec<AtomicU64>>,
    current_position: UnsafeCell<Vec<AtomicU64>>,
    claimed_regions: UnsafeCell<Vec<AtomicBool>>,
    pub(super) pools: ThreadPool,
    n_pools: usize,
}

impl SharedLinkedList {
    pub(super) fn new(size: usize, n_pools: usize) -> Self {
        let next_free = UnsafeCell::new(
            generate_next_free(size)
                .map(|x| AtomicU64::from(u64::from(x)))
                .collect(),
        );
        let current_position = generate_current_position(size, n_pools)
            .map(|x| AtomicU64::from(u64::try_from(x).unwrap()))
            .collect::<Vec<_>>();

        let claimed_regions = (0..size.div_ceil(n_pools))
            .map(|_| AtomicBool::from(false))
            .collect::<Vec<_>>();

        for x in &current_position {
            let i = usize::try_from(x.load(Relaxed)).unwrap();
            claimed_regions[i / REGION_SIZE].store(true, Relaxed);
        }

        let pools = ThreadPoolBuilder::new()
            .num_threads(n_pools)
            .build()
            .unwrap();

        SharedLinkedList {
            next_free,
            current_position: UnsafeCell::new(current_position),
            claimed_regions: UnsafeCell::new(claimed_regions),
            pools,
            n_pools,
        }
    }

    fn thread_id(&self) -> usize {
        self.pools
            .current_thread_index()
            .unwrap_or_else(|| rayon::current_thread_index().unwrap_or(0) % self.n_pools)
    }

    //Given the current position, find and claim a new region, returning the head.
    fn next_id(&self, thread_id: usize) -> Option<usize> {
        let current_position: UsageFlag = self.current_position()[thread_id].load(Acquire).into();
        match current_position {
            UsageFlag::NextStored(id) => return Some(id),
            UsageFlag::Used => {
                return None;
            }
            UsageFlag::RegionEnd => (),
        }

        let new_region = unsafe {
            (&*self.claimed_regions.get())
                .iter()
                .enumerate()
                .find_map(|(i, x)| {
                    if x.compare_exchange(false, true, Acquire, Relaxed).is_ok() {
                        Some(i)
                    } else {
                        None
                    }
                })
                .unwrap()
        };

        let new_position = new_region * REGION_SIZE;
        if self.current_position()[thread_id]
            .compare_exchange(
                current_position.into(),
                u64::from(UsageFlag::NextStored(new_position)),
                AcqRel,
                Relaxed,
            )
            .is_ok()
        {
            Some(new_position)
        } else {
            //other thread already claimed a new region if it errored, so we need to unclaim
            unsafe {
                (&*self.claimed_regions.get())[new_region].store(false, Relaxed);
            }
            //try again to get a thread_id
            self.next_id(thread_id)
        }
    }

    pub(super) fn reserve_bucket(&self) -> Option<usize> {
        let thread_id = self.thread_id();
        loop {
            let id = self.next_id(thread_id)?;

            let next = UsageFlag::from(self.next_free()[id].load(Relaxed));

            if self.current_position()[thread_id]
                .compare_exchange(
                    UsageFlag::NextStored(id).into(),
                    next.into(),
                    AcqRel,
                    Acquire,
                )
                .is_ok()
            {
                if self.next_free()[id]
                    .compare_exchange(next.into(), UsageFlag::Used.into(), Acquire, Relaxed)
                    .is_ok()
                {
                    return Some(id);
                }
                panic!(
                    "Mismatch between free list and usage! 
                    thread_id: {thread_id}, id: {id}, next: {next:?}"
                );
            }
        }
    }

    pub(super) fn clear(&self, marked: &[usize]) {
        let size = unsafe { (&*self.next_free.get()).len() };
        let mut next_free = generate_next_free(size).collect::<Vec<_>>();

        for index in marked {
            //check if index happened already.
            if !matches!(next_free[*index], UsageFlag::Used) {
                let old = std::mem::replace(&mut next_free[*index], UsageFlag::Used);
                if let Some(prev) = next_free[..*index]
                    .iter_mut()
                    .rev()
                    .find(|x| matches!(x, UsageFlag::NextStored(_)))
                {
                    *prev = old;
                }
            }
        }

        let free_slots = next_free
            .iter()
            .enumerate()
            .filter_map(|(i, x)| {
                if matches!(x, UsageFlag::NextStored(_)) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let n_free = free_slots.len();
        let positions = generate_current_position(n_free, self.n_pools)
            .map(|i| AtomicU64::from(u64::try_from(free_slots[i]).unwrap()))
            .collect::<Vec<_>>();

        println!("{next_free:?}");

        let next_free = next_free
            .into_iter()
            .map(|x| AtomicU64::from(u64::from(x)))
            .collect::<Vec<_>>();

        let claimed_regions = (0..size.div_ceil(self.n_pools))
            .map(|_| AtomicBool::from(false))
            .collect::<Vec<_>>();

        for x in &positions {
            let i = usize::try_from(x.load(Relaxed)).unwrap();
            claimed_regions[i / REGION_SIZE].store(true, Relaxed);
        }

        unsafe {
            *self.next_free.get() = next_free;
            *self.current_position.get() = positions;
            *self.claimed_regions.get() = claimed_regions;
        }
    }

    pub(super) fn free_bucket(&self, index: usize) {
        let thread_id = self.thread_id();
        loop {
            let current_pos = self.current_position()[thread_id].load(Acquire);
            if self.current_position()[thread_id]
                .compare_exchange(current_pos, u64::try_from(index).unwrap(), AcqRel, Acquire)
                .is_ok()
            {
                self.next_free()[index].store(current_pos, Release);
                break;
            }
        }
    }

    fn current_position(&self) -> &Vec<AtomicU64> {
        unsafe { &*self.current_position.get() }
    }

    fn next_free(&self) -> &Vec<AtomicU64> {
        unsafe { &*self.next_free.get() }
    }
}
