//! Tools for working with ZDD algorithms in parallel.
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

use crate::manager::{RawZddData, ZddIndex};
use crate::{SetFamily, ZddHolder};

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    #[expect(dead_code)]
    pub(crate) fn children(&self) -> Option<(SetFamily<'a, V>, SetFamily<'a, V>)> {
        self.manager.uniq_table.get(self.id).map(|x| {
            (
                SetFamily::from_set_family(x.lo, self.manager),
                SetFamily::from_set_family(x.hi, self.manager),
            )
        })
    }

    pub(crate) fn lo(self) -> Option<SetFamily<'a, V>> {
        self.manager
            .uniq_table
            .get(self.id)
            .map(|x| SetFamily::from_set_family(x.lo, self.manager))
    }

    #[expect(dead_code)]
    pub(crate) fn hi(self) -> Option<SetFamily<'a, V>> {
        self.manager
            .uniq_table
            .get(self.id)
            .map(|x| SetFamily::from_set_family(x.hi, self.manager))
    }
}

impl<'a, V: Eq + Hash> SetFamily<'a, V> {
    pub(crate) fn from_set_family(s: ZddIndex<V>, manager: &'a ZddHolder<V>) -> SetFamily<'a, V> {
        let id = usize::from(s);
        manager.inc_count(id);
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
        self.manager.uniq_table.get(self.id).map(|x| {
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
            self.manager.inc_count(self.id);
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
            self.manager.dec_count(self.id);
        }
    }
}

#[derive(Debug)]
pub(super) struct ZddThreadPool {
    pools: ThreadPool,
}

impl Default for ZddThreadPool {
    fn default() -> Self {
        let n_threads = rayon::current_num_threads();
        Self {
            pools: ThreadPoolBuilder::new()
                .num_threads(n_threads)
                .build()
                .unwrap(),
        }
    }
}
impl ZddThreadPool {
    pub(super) fn n_pools(&self) -> usize {
        self.pools.current_num_threads()
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    pub(crate) fn pools(&self) -> &ThreadPool {
        &self.pools.pools
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

        let s = ZddIndex::from(self.uniq_table.find_or_insert(zdd));
        SetFamily::from_set_family(s, self)
    }
}
