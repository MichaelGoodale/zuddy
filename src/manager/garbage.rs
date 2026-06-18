use crate::{
    ZddHolder,
    manager::{RawZddData, ZddIndex},
};
use dashmap::DashSet;
use rayon::prelude::*;
use std::hash::Hash;

impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&self) {
        if let Some(x) = self.uniq_table.start_gc() {
            self.cache.clear();
            self.sum_cache.clear();

            let marked = DashSet::new();
            self.used_variables().for_each(|g| mark(g, &marked, self));
            let marked = marked.into_iter().map(usize::from).collect::<Vec<_>>();
            self.uniq_table.clear(marked);
            self.uniq_table.end_gc(x);
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
