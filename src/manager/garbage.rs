use crate::{ZddHolder, manager::ZddIndex};
use dashmap::DashSet;
use rayon::prelude::*;
use std::{fmt::Debug, hash::Hash};

impl<V: Eq + Hash + Clone + Debug + Send + Sync> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&self) {
        self.pools().install(|| self.inner_gc());
    }

    fn inner_gc(&self) {
        todo!();
        /*
        let marked = DashSet::new();
        self.protected_values().for_each(|g| mark(g, &marked, self));

        self.cache.clear();
        self.sum_cache.clear();
        self.uniq_table.clear();

        marked.par_iter().for_each(|i| {
            let i = *i.key();
            if let Some(k) = self.data[usize::from(i)].read().unwrap().as_ref() {
                self.uniq_table.insert(k.clone(), i);
            }
        });

        let c = self.data.len() - 2 - marked.len();
        self.distribute_free_index_count(
            (2..self.data.len()).filter(|x| !marked.contains(&ZddIndex::from(*x))),
            c,
        );*/
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
