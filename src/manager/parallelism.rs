//! Tools for working with ZDD algorithms in parallel.
use std::{hash::Hash, marker::PhantomData};

use crate::manager::hashtable::InsertionError;
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

impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
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

        let mut zdd = RawZddData {
            value,
            lo: lo.as_raw(),
            hi: hi.as_raw(),
        };

        loop {
            let res = self.uniq_table.find_or_insert(zdd);
            match res {
                Ok(s) => return SetFamily::from_set_family(ZddIndex::from(s), self),
                Err(InsertionError::CapacityWarning(s)) => {
                    let x = SetFamily::from_set_family(ZddIndex::from(s), self);
                    self.gc();
                    return x;
                }
                Err(InsertionError::DoingGC(old_zdd)) => {
                    zdd = old_zdd;
                }
                Err(InsertionError::FullTable) => panic!("Table is full!"),
            }
        }
    }
}
