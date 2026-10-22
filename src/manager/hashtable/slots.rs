use std::{
    cell::{Cell, UnsafeCell},
    sync::atomic::{
        AtomicBool, AtomicU64, AtomicUsize,
        Ordering::{Acquire, Relaxed},
    },
};

thread_local! {
    static THREAD_ID: Cell<usize> = const { Cell::new(usize::MAX) };
}

static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(0);

///For each thread, it assigns it an id, which gradually grows.
///
///This is modulod by the `n_splits_of_regions` so that all threads are offset to an extent
fn assign_thread_id(n_splits_of_regions: usize) -> usize {
    THREAD_ID.with(|id| {
        if id.get() == usize::MAX {
            id.set(NEXT_THREAD_ID.fetch_add(1, Relaxed));
        }
        id.get() % n_splits_of_regions
    })
}

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
    n_splits_of_regions: usize,
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
    pub(super) fn new(size: usize, n_splits_of_regions: usize) -> Self {
        let current_region =
            generate_current_position(size.div_ceil(REGION_SIZE), n_splits_of_regions)
                .map(AtomicUsize::from)
                .collect::<Vec<_>>();

        let claimed_regions = (0..size.div_ceil(REGION_SIZE))
            .map(|_| AtomicBool::from(false))
            .collect::<Vec<_>>();

        for x in &current_region {
            let i = x.load(Relaxed);
            claimed_regions[i].store(true, Relaxed);
        }

        Slots {
            claimed_regions: UnsafeCell::new(claimed_regions),
            current_region: UnsafeCell::new(current_region),
            used_data: DataTakenRecord::new(size),
            n_splits_of_regions,
        }
    }

    pub(super) fn thread_id(&self) -> usize {
        assign_thread_id(self.n_splits_of_regions)
    }

    pub(crate) fn n_claimed_regions(&self) -> usize {
        let x = unsafe { self.claimed_regions.get().as_ref().unwrap() };
        x.iter().map(|x| usize::from(x.load(Relaxed))).sum()
    }

    pub(crate) fn n_regions(&self) -> usize {
        unsafe { self.claimed_regions.get().as_ref() }
            .unwrap()
            .len()
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
            generate_current_position(n_elements.div_ceil(REGION_SIZE), self.n_splits_of_regions)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn freeing_and_using_slots() {
        //Fill up the table
        let size = 4 * REGION_SIZE;
        let slots = Slots::new(size, 1);

        let mut reserved = std::collections::HashSet::new();
        for _ in 0..size - 2 {
            let i = slots.reserve_bucket().expect("table is not yet full");
            assert!(reserved.insert(i), "slot {i} reserved twice");
        }
        //Indexes 0 and 1 are reserved for the terminals and never handed out.
        assert!(!reserved.contains(&0) && !reserved.contains(&1));
        assert_eq!(slots.n_used(), reserved.len() + 2);
        assert_eq!(
            slots.reserve_bucket(),
            None,
            "all buckets but the terminals are taken"
        );

        //Freed slots get recycled for new insertions
        let thread_id = slots.thread_id();
        let current_region = slots.current_region()[thread_id].load(Relaxed);
        let mut freed_in_current_region = vec![];
        for i in (current_region * REGION_SIZE..(current_region + 1) * REGION_SIZE).step_by(7) {
            slots.free_bucket(i);
            assert!(reserved.remove(&i));
            freed_in_current_region.push(i);
        }
        for expected in freed_in_current_region {
            let i = slots
                .reserve_bucket()
                .expect("freed slots in the current region are reused");
            assert_eq!(i, expected, "freed slots are reused first");
            assert!(reserved.insert(i));
        }

        //Clear keeps only the marked (plus terminals)
        let marked: Vec<usize> = (13..100).step_by(3).collect();
        slots.clear(&marked, Some(2 * size));
        assert_eq!(slots.n_used(), marked.len() + 2);

        //Any new insertions are unmarked.
        let mut drained = std::collections::HashSet::new();
        while let Some(i) = slots.reserve_bucket() {
            assert!(
                !marked.contains(&i),
                "marked slot {i} handed out after clear"
            );
            assert!(drained.insert(i), "slot {i} reserved twice after clear");
        }
        let expected_free: std::collections::HashSet<usize> = (0..2 * size)
            .filter(|i| !marked.contains(i) && !matches!(i, 0 | 1))
            .collect();
        assert_eq!(
            drained, expected_free,
            "after clear every unmarked slot is handed out"
        );
    }

    #[test]
    fn use_every_possible_slot() {
        let size = 3 * REGION_SIZE;
        let slots = Slots::new(size, 1);
        let mut seen = std::collections::HashSet::new();
        while let Some(i) = slots.reserve_bucket() {
            assert!(i < size, "reserved index {i} out of bounds");
            assert!(seen.insert(i), "index {i} handed out twice");
        }
        assert_eq!(
            seen.len(),
            size - 2,
            "every index but the terminals handed out"
        );
        assert!(!seen.contains(&0) && !seen.contains(&1));
        assert_eq!(slots.n_used(), size);
    }
}
