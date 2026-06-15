use std::{collections::HashSet, fmt::Debug, hash::Hash, marker::PhantomData};

use crate::{SetFamily, ZddHolder};

impl<V: Eq + Hash + Clone + Debug> ZddHolder<V> {
    ///Clean up unused nodes!
    pub fn gc(&mut self) {
        let mut marked = HashSet::default();
        for g in self.protected.iter().copied() {
            mark(g, &mut marked, self);
        }
        self.cache.clear();
        self.sum_cache.clear();
        self.uniq_table.clear();
        self.free.extend((2..self.data.len()).filter(|&x| {
            if marked.contains(&SetFamily(x, PhantomData)) {
                false
            } else {
                self.data[x] = None;
                true
            }
        }));

        self.uniq_table.extend(
            self.data
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().map(|x| (x.clone(), SetFamily(i, PhantomData)))),
        );
    }
}

fn mark<V>(to_mark: SetFamily<V>, marked: &mut HashSet<SetFamily<V>>, holder: &ZddHolder<V>) {
    if !marked.contains(&to_mark) {
        marked.insert(to_mark);
        if let Some((lo, hi)) = to_mark.children(holder) {
            mark(lo, marked, holder);
            mark(hi, marked, holder);
        }
    }
}

impl<V> SetFamily<V> {
    ///Mark a node as protected from garbage collection.
    pub fn protect(&self, holder: &mut ZddHolder<V>) {
        holder.protected.insert(*self);
    }

    ///Unmark a node as protected from garbage collection.
    pub fn unprotect(&self, holder: &mut ZddHolder<V>) {
        holder.protected.remove(self);
    }
}
