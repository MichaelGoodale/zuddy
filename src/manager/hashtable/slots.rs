use std::{
    cell::UnsafeCell,
    sync::atomic::{
        AtomicBool, AtomicU64, AtomicUsize,
        Ordering::{Acquire, Relaxed},
    },
};

use rayon::{ThreadPool, ThreadPoolBuilder};

fn generate_current_position(size: usize, n_pools: usize) -> impl Iterator<Item = usize> {
    let mut gap = size / n_pools;
    if gap == 0 {
        gap = 1;
    }
    (0..n_pools).map(move |x| x * gap)
}

pub const REGION_SIZE: usize = 512;

///A struct for managing which slots are free in the concurrent data structure.
#[derive(Debug)]
pub(super) struct Slots {
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
        let v = std::iter::repeat_n(0_u64, size / 64)
            .map(AtomicU64::from)
            .collect::<Vec<_>>();
        v[0].store(0b11, Relaxed);
        DataTakenRecord(UnsafeCell::new(v))
    }

    fn len(&self) -> usize {
        unsafe { &*self.0.get() }.len()
    }

    fn free_slot(&self, index: usize) {
        let i = index / 64_usize;
        let j = u64::try_from(index).unwrap() % 64_u64;
        unsafe {
            let data_vec = &*self.0.get();
            data_vec[i].fetch_and(!(1_u64 << j), Relaxed);
        };
    }

    fn clear_and_mark_slots(&self, to_mark: &[usize], resize_to: Option<usize>) {
        unsafe {
            let this = &mut *self.0.get();
            let n_bit_arrays = resize_to.map_or(this.len(), |x| x / 64);
            let v = std::iter::repeat_n(0_u64, n_bit_arrays)
                .map(AtomicU64::from)
                .collect::<Vec<_>>();
            *this = v;
            this[0].store(0b11, Relaxed);
            for index in to_mark {
                let i = index / 64_usize;
                let j = u64::try_from(*index).unwrap() % 64_u64;
                this[i].fetch_or(1_u64 << j, Relaxed);
            }
        }
    }

    fn reserve_slot(&self, region: usize) -> Option<usize> {
        let d = unsafe { &*self.0.get() };
        for i in 0..8 {
            let mut region_usage = d[region * 8 + i].load(Relaxed);
            while let Some(j) = find_zero_bit(region_usage) {
                let set_with_mark = region_usage | (1 << j);
                match d[region * 8 + i].compare_exchange(
                    region_usage,
                    set_with_mark,
                    Acquire,
                    Relaxed,
                ) {
                    Ok(_) => {
                        return Some((region * 8 + i) * 64 + j);
                    }
                    Err(actual) => region_usage = actual,
                }
            }
        }
        None
    }
}

impl Slots {
    pub(super) fn new(size: usize, n_pools: usize) -> Self {
        let current_region = generate_current_position(size.div_ceil(REGION_SIZE), n_pools)
            .map(AtomicUsize::from)
            .collect::<Vec<_>>();

        let claimed_regions = (0..size.div_ceil(REGION_SIZE))
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

        Slots {
            claimed_regions: UnsafeCell::new(claimed_regions),
            current_region: UnsafeCell::new(current_region),
            used_data: DataTakenRecord::new(size),
            pools,
            n_pools,
        }
    }

    pub(super) fn thread_id(&self) -> usize {
        self.pools
            .current_thread_index()
            .unwrap_or_else(|| rayon::current_thread_index().unwrap_or(0) % self.n_pools)
    }

    fn claim_region(&self, thread_id: usize) -> bool {
        let current_region: usize = self.current_region()[thread_id].load(Relaxed);
        let n_regions = unsafe { (&*self.claimed_regions.get()).len() };
        let mut new_region = (current_region + 1) % n_regions;
        let claimed_regions = unsafe { &*self.claimed_regions.get() };

        while new_region != current_region {
            if claimed_regions[new_region]
                .compare_exchange(false, true, Relaxed, Relaxed)
                .is_ok()
            {
                //TODO: Maybe some checks to see if the region was already switched?;
                self.current_region()[thread_id].store(new_region, Relaxed);
                return true;
            }
            new_region = (new_region + 1) % n_regions;
        }
        false
    }

    pub(super) fn reserve_bucket(&self) -> Option<usize> {
        let thread_id = self.thread_id();
        loop {
            let current_region: usize = self.current_region()[thread_id].load(Acquire);
            if let Some(x) = self.used_data.reserve_slot(current_region) {
                return Some(x);
            }
            if !self.claim_region(thread_id) {
                return None;
            }
        }
    }
    pub(super) fn n_used(&self) -> usize {
        unsafe { &*self.used_data.0.get() }
            .iter()
            .map(|x| usize::try_from(x.load(Relaxed).count_ones()).unwrap())
            .sum()
    }

    pub(super) fn clear(&self, marked: &[usize], resize_to: Option<usize>) {
        let n_elements = resize_to.unwrap_or(self.used_data.len() * 64);
        let current_region =
            generate_current_position(n_elements.div_ceil(REGION_SIZE), self.n_pools)
                .map(AtomicUsize::from)
                .collect::<Vec<_>>();

        let claimed_regions = (0..n_elements.div_ceil(REGION_SIZE))
            .map(|_| AtomicBool::from(false))
            .collect::<Vec<_>>();

        for x in &current_region {
            let i = x.load(Relaxed);
            claimed_regions[i].store(true, Relaxed);
        }

        self.used_data.clear_and_mark_slots(marked, resize_to);

        unsafe {
            *self.current_region.get() = current_region;
            *self.claimed_regions.get() = claimed_regions;
        }
    }

    pub(super) fn free_bucket(&self, index: usize) {
        self.used_data.free_slot(index);
    }

    fn current_region(&self) -> &Vec<AtomicUsize> {
        unsafe { &*self.current_region.get() }
    }
}
