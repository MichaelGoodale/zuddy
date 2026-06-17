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
        let marked = DashSet::new();
        self.protected_values().for_each(|g| mark(g, &marked, self));

        self.cache.clear();
        self.sum_cache.clear();
        self.uniq_table.clear();

        self.free.lock().unwrap().par_extend(
            self.data
                .write()
                .unwrap()
                .par_iter_mut()
                .enumerate()
                .skip(2)
                .filter_map(|(i, x)| {
                    if marked.contains(&ZddIndex::from(i)) {
                        None
                    } else {
                        *x = None;
                        Some(i)
                    }
                }),
        );

        self.data
            .read()
            .unwrap()
            .par_iter()
            .enumerate()
            .for_each(|(i, x)| {
                if let Some(k) = x {
                    self.uniq_table.insert(k.clone(), ZddIndex::from(i));
                }
            });
    }
}

fn mark<V: Send + Sync + Eq + Hash>(
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
