use crate::{ONE_IDX, SetFamily, ZERO_IDX, manager::ZddHolder};
use ahash::RandomState;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt::Debug, hash::Hash, marker::PhantomData};

///A raw ZDD index without memory management for GC.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RawZdd<V>(usize, PhantomData<V>);

impl<V> From<usize> for RawZdd<V> {
    fn from(value: usize) -> Self {
        RawZdd(value, PhantomData)
    }
}

impl<V> From<RawZdd<V>> for usize {
    fn from(value: RawZdd<V>) -> Self {
        value.0
    }
}

impl<V> Copy for RawZdd<V> {}

impl<V> Clone for RawZdd<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> PartialEq for RawZdd<V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<V> Hash for RawZdd<V> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<V> Eq for RawZdd<V> {}

impl<V> PartialOrd for RawZdd<V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V> Ord for RawZdd<V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V> RawZdd<V> {
    ///The empty set {}.
    pub const ZERO: Self = RawZdd(ZERO_IDX, PhantomData);

    ///The family containing the empty set {{}}.
    pub const ONE: Self = RawZdd(ONE_IDX, PhantomData);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub(super) struct Zdd<V> {
    pub(super) value: V,
    pub(super) lo: RawZdd<V>,
    pub(super) hi: RawZdd<V>,
}

impl<V: Eq + Hash + Clone> RawZdd<V> {
    pub fn get(self, holder: &ZddHolder<V>) -> Option<(V, RawZdd<V>, RawZdd<V>)> {
        holder.data.read().unwrap()[self.0]
            .as_ref()
            .map(|x| (x.value.clone(), x.lo, x.hi))
    }
}

impl<V: Eq + Hash> RawZdd<V> {
    pub fn is_zero(self) -> bool {
        self == RawZdd::ZERO
    }

    pub fn is_one(self) -> bool {
        self == RawZdd::ONE
    }

    pub fn children(self, holder: &ZddHolder<V>) -> Option<(RawZdd<V>, RawZdd<V>)> {
        holder.data.read().unwrap()[self.0]
            .as_ref()
            .map(|x| (x.lo, x.hi))
    }

    fn n_nodes_inner(
        self,
        count_cache: &mut HashSet<RawZdd<V>, RandomState>,
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

impl<V: Eq + Hash> SetFamily<'_, V> {
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
