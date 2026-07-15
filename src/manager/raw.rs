use crate::{ONE_IDX, SetFamily, ZERO_IDX, manager::ZddHolder};
use ahash::RandomState;
use std::{collections::HashSet, fmt::Debug, hash::Hash, marker::PhantomData};

///A raw ZDD index without memory management for GC.
#[derive(Debug)]
pub(crate) struct ZddIndex<V>(usize, PhantomData<V>);

impl<V> From<usize> for ZddIndex<V> {
    fn from(value: usize) -> Self {
        ZddIndex(value, PhantomData)
    }
}

impl<V> From<ZddIndex<V>> for usize {
    fn from(value: ZddIndex<V>) -> Self {
        value.0
    }
}

impl<V> Copy for ZddIndex<V> {}

impl<V> Clone for ZddIndex<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> PartialEq for ZddIndex<V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<V> Hash for ZddIndex<V> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<V> Eq for ZddIndex<V> {}

impl<V> PartialOrd for ZddIndex<V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V> Ord for ZddIndex<V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V> ZddIndex<V> {
    ///The empty set {}.
    pub const ZERO: Self = ZddIndex(ZERO_IDX, PhantomData);

    ///The family containing the empty set {{}}.
    pub const ONE: Self = ZddIndex(ONE_IDX, PhantomData);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(super) struct RawZddData<V> {
    pub(super) value: V,
    pub(super) lo: ZddIndex<V>,
    pub(super) hi: ZddIndex<V>,
}

impl<V: Eq + Hash + Clone> ZddIndex<V> {
    pub fn get(self, holder: &ZddHolder<V>) -> Option<(V, ZddIndex<V>, ZddIndex<V>)> {
        holder.uniq_table.get(self.0).map(|x| (x.value, x.lo, x.hi))
    }

    pub fn children(self, holder: &ZddHolder<V>) -> Option<(ZddIndex<V>, ZddIndex<V>)> {
        holder.uniq_table.get(self.0).map(|x| (x.lo, x.hi))
    }

    fn n_nodes_inner(
        self,
        count_cache: &mut HashSet<ZddIndex<V>, RandomState>,
        holder: &ZddHolder<V>,
    ) {
        if !count_cache.contains(&self) {
            if self.is_zero() || self.is_one() {
                count_cache.insert(self);
            } else {
                let (lo, hi) = self.children(holder).unwrap();
                lo.n_nodes_inner(count_cache, holder);
                hi.n_nodes_inner(count_cache, holder);
                count_cache.insert(self);
            }
        }
    }
}

impl<V> ZddIndex<V> {
    pub fn is_zero(self) -> bool {
        self == ZddIndex::ZERO
    }

    pub fn is_one(self) -> bool {
        self == ZddIndex::ONE
    }
}

impl<V: Eq + Hash + Clone> SetFamily<'_, V> {
    ///Counts the number of nodes in this [`SetFamily`]
    ///
    ///# Panics
    ///Will panic if `self` is not defined in `holder`.
    #[must_use]
    pub fn n_nodes(&self) -> usize {
        if self.is_zero() || self.is_one() {
            1
        } else {
            let mut edge_cache = HashSet::<_, RandomState>::default();
            self.as_raw().n_nodes_inner(&mut edge_cache, self.manager);
            edge_cache.len()
        }
    }
}
