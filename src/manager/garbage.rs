use crate::{ZddHolder, manager::ZddIndex};
use dashmap::DashSet;
use rayon::prelude::*;
use std::{fmt::Debug, hash::Hash};

impl<V: Eq + Hash + Clone + Debug + Send + Sync> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&self) {
        self.cache.clear();
        self.sum_cache.clear();

        let marked = DashSet::new();
        self.used_variables().for_each(|g| mark(g, &marked, self));
        let marked = marked.into_iter().map(usize::from).collect::<Vec<_>>();
        self.uniq_table.clear(marked);
    }
}

fn mark<V: Send + Sync + Eq + Hash + Clone>(
    to_mark: ZddIndex<V>,
    marked: &DashSet<ZddIndex<V>>,
    holder: &ZddHolder<V>,
) {
    if !marked.contains(&to_mark) {
        marked.insert(to_mark);
        if let Some((lo, hi)) = to_mark.children(holder) {
            rayon::join(|| mark(lo, marked, holder), || mark(hi, marked, holder));
        }
    }
}
