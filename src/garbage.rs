use crate::{SetFamily, ZddHolder};
use dashmap::DashSet;
use rayon::prelude::*;
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

impl<V: Eq + Hash + Clone + Debug + Send + Sync> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&mut self) {
        self.pools().install(|| self.inner_gc());
    }

    fn inner_gc(&mut self) {
        let marked = DashSet::new();
        self.protected
            .par_iter()
            .copied()
            .for_each(|g| mark(g, &marked, self));

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
                    if marked.contains(&SetFamily(i, PhantomData)) {
                        None
                    } else {
                        *x = None;
                        Some(i)
                    }
                }),
        );

        self.uniq_table.par_extend(
            self.data
                .read()
                .unwrap()
                .par_iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|x| (x.clone(), SetFamily(i, PhantomData)))),
        );
    }
}

fn mark<V: Send + Sync + Eq + Hash>(
    to_mark: SetFamily<V>,
    marked: &DashSet<SetFamily<V>>,
    holder: &ZddHolder<V>,
) {
    if !marked.contains(&to_mark) {
        marked.insert(to_mark);
        if let Some((lo, hi)) = to_mark.children(holder) {
            rayon::join(|| mark(lo, marked, holder), || mark(hi, marked, holder));
        }
    }
}

impl<V: Eq + Hash> SetFamily<V> {
    ///Mark a node as protected from garbage collection.
    pub fn protect(&self, holder: &mut ZddHolder<V>) {
        holder.protected.insert(*self);
    }

    ///Unmark a node as protected from garbage collection.
    pub fn unprotect(&self, holder: &mut ZddHolder<V>) {
        holder.protected.remove(self);
    }
}
