use crate::{
    ZddHolder,
    manager::{RawZddData, ZddIndex},
};
use rayon::prelude::*;
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
            .into_par_iter()
            .enumerate()
            .flat_map_iter(|(i, word)| {
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
    ///Clean up unused nodes!
    pub fn gc(&self, force_resize: bool) {
        if self.uniq_table.start_gc() {
            self.generation.fetch_add(1, Ordering::Relaxed);
            let resize_to = if force_resize || self.uniq_table.usage() > 0.5 {
                Some(self.uniq_table.capacity() * 2)
            } else {
                None
            };
            self.cache.clear();
            self.size_caches.clear();

            let marked = MarkedNodes::new(self.uniq_table.len());
            self.used_variables().for_each(|g| mark(g, &marked, self));
            let marked = marked.into_marked();
            self.uniq_table.clear(&marked, resize_to);
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
    mark_inner(to_mark, marked, holder, 0);
}
const PAR_DEPTH_LIMIT: usize = 12;

fn mark_inner<V: Send + Sync + Eq + Hash + Clone>(
    to_mark: ZddIndex<V>,
    marked: &MarkedNodes,
    holder: &ZddHolder<V>,
    depth: usize,
) {
    if marked.try_mark(usize::from(to_mark))
        && let Some((hi, lo)) = to_mark.raw_children(holder)
    {
        if depth < PAR_DEPTH_LIMIT {
            rayon::join(
                || mark_inner(hi, marked, holder, depth + 1),
                || mark_inner(lo, marked, holder, depth + 1),
            );
        } else {
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
}
