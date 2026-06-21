use std::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicU64,
        Ordering::{AcqRel, Acquire, Relaxed, Release},
    },
};

use rayon::{ThreadPool, ThreadPoolBuilder};

use super::UsageFlag;

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

fn generate_next_free(size: usize) -> impl Iterator<Item = UsageFlag> {
    (1..=size).map(move |i| {
        if i <= 2 {
            //Mark 0 and 1 as used.
            UsageFlag::Used
        } else if i == size {
            UsageFlag::NextStored(2)
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
        let current_position = UnsafeCell::new(
            generate_current_position(size, n_pools)
                .map(|x| AtomicU64::from(u64::try_from(x).unwrap()))
                .collect(),
        );

        let pools = ThreadPoolBuilder::new()
            .num_threads(n_pools)
            .build()
            .unwrap();
        SharedLinkedList {
            next_free,
            current_position,
            pools,
            n_pools,
        }
    }

    fn thread_id(&self) -> usize {
        self.pools
            .current_thread_index()
            .unwrap_or_else(|| rayon::current_thread_index().unwrap_or(0) % self.n_pools)
    }

    pub(super) fn reserve_bucket(&self) -> Option<usize> {
        let thread_id = self.thread_id();
        loop {
            let UsageFlag::NextStored(id) = self.current_position()[thread_id].load(Acquire).into()
            else {
                return None;
            };

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

        let next_free = next_free
            .into_iter()
            .map(|x| AtomicU64::from(u64::from(x)))
            .collect::<Vec<_>>();

        unsafe {
            *self.next_free.get() = next_free;
            *self.current_position.get() = positions;
        }
    }

    pub(super) fn free_bucket(&self, index: usize) {
        let thread_id = self.thread_id();
        loop {
            let current_pos = self.current_position()[thread_id].load(Acquire);
            self.next_free()[index].store(current_pos, Release);
            if self.current_position()[thread_id]
                .compare_exchange(current_pos, u64::try_from(index).unwrap(), AcqRel, Acquire)
                .is_ok()
            {
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
