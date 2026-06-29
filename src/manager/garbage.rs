use crate::{
    ZddHolder,
    manager::{RawZddData, ZddIndex},
};
use dashmap::DashSet;
use rayon::prelude::*;
use std::hash::Hash;

impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&self, force_resize: bool) {
        if self.uniq_table.start_gc() {
            let resize_to = if force_resize || self.uniq_table.usage() > 0.5 {
                Some(self.uniq_table.capacity() * 2)
            } else {
                None
            };
            self.cache.clear();
            self.sum_cache.clear();

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
    if !marked.contains(&to_mark) {
        marked.insert(to_mark);
        if let Some((lo, hi)) = to_mark.raw_children(holder) {
            rayon::join(|| mark(lo, marked, holder), || mark(hi, marked, holder));
        }
    }
}
