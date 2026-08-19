use ahash::{HashSet, HashSetExt};
use mem_dbg::MemSize;

use crate::{
    ONE_IDX, SetFamily, ZERO_IDX,
    manager::{SizeKey, SizeValue, ZddHolder},
};
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

///A raw ZDD index without memory management for GC.
#[derive(Debug, MemSize)]
#[mem_size(flat)]
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

    fn n_nodes(self, holder: &ZddHolder<V>) -> usize {
        let op = SizeKey::NNodes(self);
        if self.is_zero() || self.is_one() {
            1
        } else if let Some(r) = holder.size_cache_get(&op) {
            r.unwrap_n_nodes()
        } else {
            let mut stack = vec![self];
            let mut visited = HashSet::new();
            while let Some(x) = stack.pop() {
                if x.is_zero() || x.is_one() {
                    visited.insert(x);
                } else if !visited.contains(&x) {
                    visited.insert(x);
                    let (lo, hi) = x.children(holder).unwrap();
                    if !visited.contains(&lo) {
                        stack.push(lo);
                    }
                    if !visited.contains(&hi) {
                        stack.push(hi);
                    }
                }
            }
            holder
                .size_cache_insert(op, SizeValue::NNodes(visited.len()))
                .unwrap_n_nodes()
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
        self.as_raw().n_nodes(self.manager)
    }
}
