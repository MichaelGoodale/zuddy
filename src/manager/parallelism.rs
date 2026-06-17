//! Tools for working with ZDD algorithms in parallel.
use dashmap::{
    DashMap,
    Entry::{Occupied, Vacant},
};
use rayon::{ThreadPool, ThreadPoolBuilder, iter::ParallelIterator};
use rayon::{current_thread_index, prelude::*};
use std::{fmt::Debug, hash::Hash, marker::PhantomData, sync::Mutex};

use crate::manager::{RawZddData, ZddIndex};
use crate::{SetFamily, ZddHolder};

impl<'a, V: Eq + Hash> SetFamily<'a, V> {
    #[expect(dead_code)]
    pub(crate) fn children(&self) -> Option<(SetFamily<'a, V>, SetFamily<'a, V>)> {
        self.manager.data[self.id]
            .read()
            .unwrap()
            .as_ref()
            .map(|x| {
                (
                    SetFamily::from_set_family(x.lo, self.manager),
                    SetFamily::from_set_family(x.hi, self.manager),
                )
            })
    }

    pub(crate) fn lo(self) -> Option<SetFamily<'a, V>> {
        self.manager.data[self.id]
            .read()
            .unwrap()
            .as_ref()
            .map(|x| SetFamily::from_set_family(x.lo, self.manager))
    }

    #[expect(dead_code)]
    pub(crate) fn hi(self) -> Option<SetFamily<'a, V>> {
        self.manager.data[self.id]
            .read()
            .unwrap()
            .as_ref()
            .map(|x| SetFamily::from_set_family(x.lo, self.manager))
    }

    pub(crate) fn from_set_family(s: ZddIndex<V>, manager: &'a ZddHolder<V>) -> SetFamily<'a, V> {
        let id = usize::from(s);
        match manager.pools.referenced_variables.entry(id) {
            Occupied(mut oc) => *oc.get_mut() += 1,
            Vacant(vac) => {
                vac.insert(1);
            }
        }

        SetFamily {
            id,
            manager,
            phantom: PhantomData,
        }
    }

    pub(crate) fn as_raw(&self) -> ZddIndex<V> {
        ZddIndex::from(self.id)
    }
}
impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    pub(crate) fn get(&self) -> Option<(V, SetFamily<'a, V>, SetFamily<'a, V>)> {
        (self.manager.data[self.id].read().unwrap())
            .clone()
            .map(|x| {
                (
                    x.value,
                    SetFamily::from_set_family(x.lo, self.manager),
                    SetFamily::from_set_family(x.hi, self.manager),
                )
            })
    }
}

impl<V: Eq + Hash> Clone for SetFamily<'_, V> {
    fn clone(&self) -> Self {
        if !self.is_zero() && !self.is_one() {
            let mut count = self
                .manager
                .pools
                .referenced_variables
                .entry(self.id)
                .or_insert(1);
            *count += 1;
        }
        Self {
            id: self.id,
            phantom: PhantomData,
            manager: self.manager,
        }
    }
}

impl<V: Eq + Hash> Drop for SetFamily<'_, V> {
    fn drop(&mut self) {
        if !self.is_zero() && !self.is_one() {
            //We have to use this scope so that count is dropped before trying to remove.
            //Otherwise, we deadlock :o
            let count = {
                if let Some(mut count) = self.manager.pools.referenced_variables.get_mut(&self.id) {
                    *count = count.saturating_sub(1);
                    Some(*count)
                } else {
                    None
                }
            };
            if count == Some(0) {
                self.manager.pools.referenced_variables.remove(&self.id);
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct ZddThreadPool {
    pools: ThreadPool,
    referenced_variables: DashMap<usize, usize>,
    free_slots: Vec<Mutex<Vec<usize>>>,
}

impl Default for ZddThreadPool {
    fn default() -> Self {
        let n_threads = rayon::current_num_threads();
        Self {
            free_slots: (0..n_threads).map(|_| Mutex::new(vec![])).collect(),
            pools: ThreadPoolBuilder::new()
                .num_threads(n_threads)
                .build()
                .unwrap(),
            referenced_variables: DashMap::new(),
        }
    }
}

impl<V: Eq + Hash + Clone + Send> ZddHolder<V> {
    pub(crate) fn protected_values(&self) -> impl ParallelIterator<Item = ZddIndex<V>> {
        self.pools.referenced_variables.par_iter().filter_map(|x| {
            if *x.value() != 0 {
                Some(ZddIndex::from(*x.key()))
            } else {
                None
            }
        })
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    pub(crate) fn pools(&self) -> &ThreadPool {
        &self.pools.pools
    }

    pub(super) fn distribute_free_index<I>(&self, indices: I)
    where
        I: IntoIterator<Item = usize>,
        I::IntoIter: ExactSizeIterator,
    {
        let mut iter = indices.into_iter();
        let s = iter.len();
        let n_per = s / self.pools.free_slots.len();
        let extra = s % self.pools.free_slots.len();

        for (i, x) in self.pools.free_slots.iter().enumerate() {
            let how_many_to_add = if i < extra { n_per + 1 } else { n_per };

            x.lock()
                .unwrap()
                .extend((0..how_many_to_add).filter_map(|_| iter.next()));
        }
    }

    pub(super) fn drain_free_indices(&self) {
        for x in &self.pools.free_slots {
            x.lock().unwrap().clear();
        }
    }

    pub(super) fn insert(&self, v: RawZddData<V>) -> ZddIndex<V> {
        let thread_id = current_thread_index().unwrap_or(0);
        let free_id = self.pools.free_slots[thread_id]
            .lock()
            .unwrap()
            .pop()
            .unwrap();
        *self.data[free_id].write().unwrap() = Some(v);
        ZddIndex::from(free_id)
    }
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    #[expect(clippy::needless_pass_by_value)]
    pub(crate) fn get_node<'a>(
        &'a self,
        value: V,
        lo: SetFamily<'a, V>,
        hi: SetFamily<'a, V>,
    ) -> SetFamily<'a, V> {
        if hi.is_zero() {
            return lo;
        }

        let zdd = RawZddData {
            value,
            lo: lo.as_raw(),
            hi: hi.as_raw(),
        };

        if let Some(s) = self.uniq_table.get(&zdd) {
            return SetFamily::from_set_family(*s, self);
        }

        let s = self.insert(zdd.clone());
        self.uniq_table.insert(zdd, s);
        SetFamily::from_set_family(s, self)
    }
}
