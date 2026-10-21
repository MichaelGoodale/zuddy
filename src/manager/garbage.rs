use crate::{
    ZddHolder,
    manager::{RawZddData, ZddIndex},
};
use std::{
    hash::Hash,
    sync::atomic::{AtomicU64, Ordering},
};

///Keeps track if a node is being used somewhere.
struct MarkedNodes {
    bits: Vec<AtomicU64>,
}

impl MarkedNodes {
    fn new(len: usize) -> Self {
        Self {
            bits: (0..len.div_ceil(u64::BITS as usize))
                .map(|_| AtomicU64::new(0))
                .collect(),
        }
    }

    ///Marks `i` and returns true if it wasn't marked yet.
    fn try_mark(&self, i: usize) -> bool {
        let word = &self.bits[i / 64];
        let mask = 1_u64 << (i % 64);
        word.fetch_or(mask, Ordering::Relaxed) & mask == 0
    }

    fn into_marked(self) -> Vec<usize> {
        self.bits
            .into_iter()
            .enumerate()
            .flat_map(|(i, word)| {
                let mut word = word.into_inner();
                let base = i * 64;
                (0..64).filter_map(move |b| {
                    let found = word & 1 != 0;
                    word >>= 1;
                    found.then_some(base + b)
                })
            })
            .collect()
    }
}

impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
    fn used_variables(&self) -> impl Iterator<Item = ZddIndex<V>> {
        (0..self.uniq_table.len())
            .filter(|&i| self.uniq_table.is_held(i))
            .map(ZddIndex::from)
    }

    ///Clean up unused nodes!
    pub fn gc(&self, force_resize: bool) {
        if self.uniq_table.start_gc() {
            self.generation.fetch_add(1, Ordering::Relaxed);
            let double = self.uniq_table.capacity() * 2;
            let resize_to = if force_resize || self.uniq_table.usage() > 0.5 {
                match self.max_table_size {
                    Some(max) if double > max => {
                        if self.uniq_table.capacity() >= max {
                            None
                        } else {
                            Some(max)
                        }
                    }
                    Some(_) | None => Some(double),
                }
            } else {
                None
            };
            self.cache.clear();
            self.size_caches.clear();

            let marked = MarkedNodes::new(self.uniq_table.len());
            for g in self.used_variables() {
                mark(g, &marked, self);
            }
            let marked = marked.into_marked();
            self.uniq_table.clear(&marked, resize_to);
            let n_live = marked.len();
            self.cache.resize(n_live.saturating_mul(4));
            self.size_caches.resize(n_live.saturating_mul(2));
            self.uniq_table.end_gc();
        } else {
            //Someone else is doing GC so let's wait til that's done.
            self.uniq_table.wait_until_unpaused();
        }
    }
}

impl<V: Eq + Hash + Hash + Clone> ZddIndex<V> {
    fn raw_children(self, holder: &ZddHolder<V>) -> Option<(ZddIndex<V>, ZddIndex<V>)> {
        unsafe {
            holder
                .uniq_table
                .get_unchecked(usize::from(self))
                .map(|RawZddData { lo, hi, .. }| (lo, hi))
        }
    }
}

fn mark<V: Send + Sync + Eq + Hash + Clone>(
    to_mark: ZddIndex<V>,
    marked: &MarkedNodes,
    holder: &ZddHolder<V>,
) {
    if marked.try_mark(usize::from(to_mark))
        && let Some((hi, lo)) = to_mark.raw_children(holder)
    {
        let mut stack = vec![hi, lo];
        while let Some(x) = stack.pop() {
            if marked.try_mark(usize::from(x))
                && let Some((lo, hi)) = x.raw_children(holder)
            {
                stack.extend([lo, hi]);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::SetFamily;
    use rayon::prelude::*;

    ///The unique table should never grow past `max_table_size` nodes, and should panic
    ///once it is full rather than growing or looping forever.
    #[test]
    #[should_panic(expected = "The unique table is full at its maximum size")]
    fn gc_respects_max_table_size() {
        use crate::manager::hashtable::slots::REGION_SIZE;
        let max = 8 * REGION_SIZE;
        let holder =
            ZddHolder::<usize>::with_capacity_and_pools(REGION_SIZE, 1).with_max_table_size(max);

        let mut acc = holder.one();
        for v in 0..(REGION_SIZE * 4) {
            acc = acc.change(v);
            holder.gc(false);
        }
        assert!(holder.stats().table_size <= max);
    }

    ///Regression test for a dead-lock caused by using the same pool for GC as workers, could
    ///dead-lock if all workers were waiting for gc to finish yet were needed for gc.
    #[test]
    fn gc_during_parallel_always_completes() {
        for _ in 0..20 {
            let holder = ZddHolder::<usize>::with_capacity_and_pools(600, 4);
            let pool = holder.pools();

            pool.install(|| {
                (0..4).into_par_iter().for_each(|t| {
                    let mut acc = holder.zero();
                    for v in 0..26 {
                        let value = v + t;
                        acc = acc.union(SetFamily::singleton(value, &holder));
                        acc = acc.change(value);
                    }
                });
            });
        }
    }
}
