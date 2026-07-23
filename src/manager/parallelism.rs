//! Tools for working with ZDD algorithms in parallel.
use crate::manager::hashtable::FullTable;
use crate::manager::{RawZddData, ZddIndex};
use crate::{SetFamily, ZddHolder};
use std::{hash::Hash, marker::PhantomData};

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    ///Gets the lo and hi children of the node, if they exist.
    #[must_use]
    pub fn children(&self) -> Option<(SetFamily<'a, V>, SetFamily<'a, V>)> {
        self.manager.uniq_table.get(self.id).map(|x| {
            (
                SetFamily::from_set_family(x.lo, self.manager),
                SetFamily::from_set_family(x.hi, self.manager),
            )
        })
    }

    ///Gets the lo child if it exists
    #[must_use]
    pub fn lo(self) -> Option<SetFamily<'a, V>> {
        self.manager
            .uniq_table
            .get(self.id)
            .map(|x| SetFamily::from_set_family(x.lo, self.manager))
    }

    ///Gets the hi child if it exists
    #[must_use]
    pub fn hi(self) -> Option<SetFamily<'a, V>> {
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
    ///Returns the value of this node along with its lo and hi children.
    ///
    ///Returns None if the element is terminal or doesn't exist.
    #[must_use]
    pub fn get(&self) -> Option<(V, SetFamily<'a, V>, SetFamily<'a, V>)> {
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
impl<V: Eq + Hash + Clone + Ord + Send + Sync> ZddHolder<V> {
    ///Creates a new ZDD without checking while ensuring that children have values greater than the
    ///root.
    ///
    ///# Panics
    ///Will panic if the node's value is greater than its children.
    pub fn zdd_node<'a>(
        &'a self,
        value: V,
        lo: SetFamily<'a, V>,
        hi: SetFamily<'a, V>,
    ) -> SetFamily<'a, V> {
        if let Some((v, _, _)) = lo.get()
            && v <= value
        {
            panic!(
                "A child has a smaller or equal value than its parent, violating the ZDD definition!"
            );
        }
        if let Some((v, _, _)) = hi.get()
            && v <= value
        {
            panic!(
                "A child has a smaller or equal value than its parent, violating the ZDD definition!"
            );
        }

        self.get_node(value, lo, hi)
    }
}
impl<V: Eq + Hash + Clone + Send + Sync> ZddHolder<V> {
    ///Creates a new ZDD without checking that it is valid.
    ///
    ///# Safety
    ///The value of `lo` and `hi` must be higher than the value of `value`. Otherwise, you run the
    ///risk of infinite loops or other nastiness.
    pub unsafe fn zdd_node_unchecked<'a>(
        &'a self,
        value: V,
        lo: SetFamily<'a, V>,
        hi: SetFamily<'a, V>,
    ) -> SetFamily<'a, V> {
        self.get_node(value, lo, hi)
    }

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

        match self.uniq_table.find_or_insert(zdd) {
            Ok((s, probe_length)) => {
                let s = SetFamily::from_set_family(ZddIndex::from(s), self);
                if let Some(probe_length) = probe_length
                    && probe_length > 32
                {
                    self.gc(false);
                }
                s
            }
            Err(FullTable { value, .. }) => {
                self.gc(true);
                self.get_node(value.value, lo, hi)
            }
        }
    }
}
