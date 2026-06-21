use std::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU64, AtomicUsize,
        Ordering::{AcqRel, Acquire, Relaxed, Release},
    },
};

use rayon::{ThreadPool, ThreadPoolBuilder};

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

pub const REGION_SIZE: usize = 512;

///A struct for managing which slots are free in the concurrent data structure.
#[derive(Debug)]
pub(super) struct SharedLinkedList {
    current_region: UnsafeCell<Vec<AtomicUsize>>,
    claimed_regions: UnsafeCell<Vec<AtomicBool>>,
    used_data: DataTakenRecord,
    pub(super) pools: ThreadPool,
    n_pools: usize,
}

#[derive(Debug)]
struct DataTakenRecord(UnsafeCell<Vec<AtomicU64>>);

pub fn find_zero_bit(v: u64) -> Option<usize> {
    let idx = (!v).trailing_zeros();
    if idx < u64::BITS {
        Some(idx as usize)
    } else {
        None
    }
}

impl DataTakenRecord {
    fn new(size: usize) -> Self {
        let n = u64::try_from(size / 64).unwrap();
        let v = (0_u64..n).map(AtomicU64::from).collect::<Vec<_>>();
        v[0].store(0b11, Relaxed);
        DataTakenRecord(UnsafeCell::new(v))
    }

    fn len(&self) -> usize {
        unsafe { &*self.0.get() }.len()
    }

    fn free_slot(&self, index: usize) {
        let i = index / 64;
        let j = index % 64;
        unsafe { (&*self.0.get())[i].fetch_and(!(1 << j), Release) };
    }

    fn reserve_slot(&self, region: usize) -> Option<usize> {
        let d = unsafe { &*self.0.get() };
        for i in 0..8 {
            let mut region_usage = d[region * 8 + i].load(Relaxed);
            while let Some(j) = find_zero_bit(region_usage) {
                let set_with_mark = region_usage | 1 << j;
                match d[region * 8 + i].compare_exchange(
                    region_usage,
                    set_with_mark,
                    Acquire,
                    Relaxed,
                ) {
                    Ok(_) => return Some((region * 8 + i) * 64 + j),
                    Err(actual) => region_usage = actual,
                }
            }
        }
        None
    }
}

impl SharedLinkedList {
    pub(super) fn new(size: usize, n_pools: usize) -> Self {
        let current_region = generate_current_position(size / REGION_SIZE, n_pools)
            .map(AtomicUsize::from)
            .collect::<Vec<_>>();

        let claimed_regions = (0..size.div_ceil(n_pools))
            .map(|_| AtomicBool::from(false))
            .collect::<Vec<_>>();

        for x in &current_region {
            let i = x.load(Relaxed);
            claimed_regions[i].store(true, Relaxed);
        }

        let pools = ThreadPoolBuilder::new()
            .num_threads(n_pools)
            .build()
            .unwrap();

        SharedLinkedList {
            claimed_regions: UnsafeCell::new(claimed_regions),
            current_region: UnsafeCell::new(current_region),
            used_data: DataTakenRecord::new(size),
            pools,
            n_pools,
        }
    }

    fn thread_id(&self) -> usize {
        self.pools
            .current_thread_index()
            .unwrap_or_else(|| rayon::current_thread_index().unwrap_or(0) % self.n_pools)
    }

    fn claim_region(&self, thread_id: usize) -> bool {
        let current_region: usize = self.current_region()[thread_id].load(Relaxed);
        let mut new_region = (current_region + 1) % (self.used_data.len() / REGION_SIZE);
        let claimed_regions = unsafe { &*self.claimed_regions.get() };

        while new_region != current_region {
            if claimed_regions[new_region]
                .compare_exchange(false, true, Relaxed, Relaxed)
                .is_ok()
            {
                return true;
            }
            new_region = (new_region + 1) % (self.used_data.len() / REGION_SIZE);
        }
        false
    }

    pub(super) fn reserve_bucket(&self) -> Option<usize> {
        let thread_id = self.thread_id();
        loop {
            let current_region: usize = self.current_region()[thread_id].load(Relaxed);
            if let Some(x) = self.used_data.reserve_slot(current_region) {
                return Some(x);
            }
            if !self.claim_region(thread_id) {
                return None;
            }
        }
    }

    pub(super) fn clear(&self, marked: &[usize]) {
        /*
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
            *self.current_region.get() = positions;
            *self.claimed_regions.get() = claimed_regions;
        }*/
    }

    pub(super) fn free_bucket(&self, index: usize) {
        self.used_data.free_slot(index);
    }

    fn current_region(&self) -> &Vec<AtomicUsize> {
        unsafe { &*self.current_region.get() }
    }
}
