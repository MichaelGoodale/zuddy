//! Tools for working with ZDD algorithms in parallel.
use std::{fmt::Debug, hash::Hash, sync::Arc};

use dashmap::{
    DashMap,
    Entry::{Occupied, Vacant},
};
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::{SetFamily, Zdd, ZddHolder, free_id};

///A ZDD used for local variables within a library function.
#[derive(Debug)]
pub(crate) struct InternalZdd<'a, V: Eq + Hash> {
    id: usize,
    manager: &'a ZddHolder<V>,
}

impl<'a, V: Eq + Hash> InternalZdd<'a, V> {
    fn from_set_family(s: SetFamily<V>, manager: &'a ZddHolder<V>) -> InternalZdd<'a, V> {
        let SetFamily(id, _) = s;
        match manager.pools.referenced_variables.entry(id) {
            Occupied(mut oc) => *oc.get_mut() += 1,
            Vacant(vac) => {
                vac.insert(1);
            }
        };

        InternalZdd { id, manager }
    }
}

impl<V: Eq + Hash> Clone for InternalZdd<'_, V> {
    fn clone(&self) -> Self {
        let mut count = self
            .manager
            .pools
            .referenced_variables
            .get_mut(&self.id)
            .expect("Interal ZDD should always have its counts available");
        *count += 1;
        Self {
            id: self.id,
            manager: self.manager,
        }
    }
}

impl<V: Eq + Hash> Drop for InternalZdd<'_, V> {
    fn drop(&mut self) {
        let mut count = self
            .manager
            .pools
            .referenced_variables
            .get_mut(&self.id)
            .expect("Interal ZDD should always have its counts available");
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.manager.pools.referenced_variables.remove(&self.id);
        }
    }
}

#[derive(Debug)]
pub(super) struct ZddThreadPool {
    pools: Arc<ThreadPool>,
    referenced_variables: DashMap<usize, usize>,
}

impl Default for ZddThreadPool {
    fn default() -> Self {
        let n_threads = rayon::current_num_threads();
        Self {
            pools: Arc::new(
                ThreadPoolBuilder::new()
                    .num_threads(n_threads)
                    .build()
                    .unwrap(),
            ),
            referenced_variables: DashMap::new(),
        }
    }
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    pub(crate) fn pools(&self) -> Arc<ThreadPool> {
        self.pools.pools.clone()
    }

    pub(crate) fn get_node<'a>(&'a self, family: Zdd<V>) -> InternalZdd<'a, V> {
        if family.hi == SetFamily::ZERO {
            return InternalZdd::from_set_family(family.lo, self);
        }

        if let Some(s) = self.uniq_table.get(&family) {
            return InternalZdd::from_set_family(*s, self);
        }

        let mut data = self.data.write().unwrap();
        let s = free_id(&mut data, &mut self.free.lock().unwrap());
        data[s.0] = Some(family.clone());
        self.uniq_table.insert(family, s);
        InternalZdd::from_set_family(s, self)
    }
}
