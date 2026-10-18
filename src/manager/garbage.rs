use crate::{
    ZddHolder,
    manager::{RawZddData, ZddIndex},
};
use dashmap::DashSet;
use rayon::prelude::*;
use std::{hash::Hash, sync::atomic::Ordering};

impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
    ///Ensure the unique table has room for at least `additional` more nodes
    ///without needing to resize. Resizing mid-construction is expensive since
    ///every live node must be marked and rehashed, so calling this before
    ///building large ZDDs (e.g. via [`SetFamily::from_sets`](crate::SetFamily::from_sets))
    /// avoids repeated stop-the-world grows.
    pub fn reserve(&self, additional: usize) {
        if self.uniq_table.capacity() - self.uniq_table.n_used() < additional {
            self.gc_with_resize((self.uniq_table.n_used() + additional).next_power_of_two());
        }
    }

    ///Clean up unused nodes!
    pub fn gc(&self, force_resize: bool) {
        if force_resize || self.uniq_table.usage() > 0.5 {
            self.gc_with_resize(self.uniq_table.capacity() * 2);
        } else {
            self.gc_inner(None);
        }
    }

    fn gc_with_resize(&self, resize_to: usize) {
        self.gc_inner(Some(resize_to.max(self.uniq_table.capacity())));
    }

    fn gc_inner(&self, resize_to: Option<usize>) {
        if self.uniq_table.start_gc() {
            self.generation.fetch_add(1, Ordering::Relaxed);
            self.cache.clear();
            self.size_caches.clear();

            let marked = DashSet::new();
            self.used_variables().for_each(|g| mark(g, &marked, self));
            let marked = marked.into_iter().map(usize::from).collect::<Vec<_>>();
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
    marked: &DashSet<ZddIndex<V>>,
    holder: &ZddHolder<V>,
) {
    mark_inner(to_mark, marked, holder, 0);
}
const PAR_DEPTH_LIMIT: usize = 12;

fn mark_inner<V: Send + Sync + Eq + Hash + Clone>(
    to_mark: ZddIndex<V>,
    marked: &DashSet<ZddIndex<V>>,
    holder: &ZddHolder<V>,
    depth: usize,
) {
    if !marked.contains(&to_mark) {
        marked.insert(to_mark);
        if let Some((hi, lo)) = to_mark.raw_children(holder) {
            if depth < PAR_DEPTH_LIMIT {
                rayon::join(
                    || mark_inner(hi, marked, holder, depth + 1),
                    || mark_inner(lo, marked, holder, depth + 1),
                );
            } else {
                let mut stack = vec![hi, lo];
                while let Some(x) = stack.pop() {
                    if !marked.contains(&x) {
                        marked.insert(x);
                        if let Some((lo, hi)) = x.raw_children(holder) {
                            stack.extend([lo, hi]);
                        }
                    }
                }
            }
        }
    }
}
