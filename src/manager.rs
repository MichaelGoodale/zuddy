//! Zuddy is a crate for handling ZDDs
use std::{
    collections::BTreeSet,
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    sync::atomic::{
        AtomicU64,
        Ordering::{self, Relaxed, Release},
    },
};

use ahash::RandomState;
use dashmap::DashMap;
mod garbage;
mod hashtable;
mod parallelism;
mod raw;
mod temp_cache;
pub(crate) use temp_cache::TempCache;

use raw::RawZddData;
pub(crate) use raw::ZddIndex;
use uuid::Uuid;

use crate::{algorithms::UsizeOrPositiveInfinity, manager::hashtable::HashTable};

use super::{ONE_IDX, Operations, SetFamily, ZERO_IDX};

#[derive(Debug)]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V: Eq + Hash> {
    generation: AtomicU64,
    uniq_table: HashTable<RawZddData<V>>,
    cache: DashMap<Operations<V>, ZddIndex<V>, RandomState>,
    sum_cache: DashMap<ZddIndex<V>, UsizeOrPositiveInfinity, RandomState>,
    id: Uuid,
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn new() -> ZddHolder<V> {
        Self::with_capacity_and_pools(100, rayon::current_num_threads())
    }

    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn with_capacity(n: usize) -> ZddHolder<V> {
        Self::with_capacity_and_pools(n, rayon::current_num_threads())
    }
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn with_capacity_and_pools(n: usize, n_pools: usize) -> ZddHolder<V> {
        let id = Uuid::new_v4();
        Self {
            generation: AtomicU64::new(0),
            uniq_table: HashTable::new(n, n_pools),
            sum_cache: DashMap::default(),
            cache: DashMap::default(),
            id,
        }
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }
}

impl<V: Eq + Hash + Clone> Default for ZddHolder<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    ///Create a new `[SetFamily]` representing the empty set ({}).
    #[must_use]
    pub fn zero(&self) -> SetFamily<'_, V> {
        SetFamily {
            id: ZERO_IDX,
            phantom: PhantomData,
            manager: self,
        }
    }

    ///Create a new `[SetFamily]` representing the set containing only the empty set ({{}}).
    #[must_use]
    pub fn one(&self) -> SetFamily<'_, V> {
        SetFamily {
            id: ONE_IDX,
            phantom: PhantomData,
            manager: self,
        }
    }

    pub(crate) fn get_from_cache<'a>(&'a self, op: &Operations<V>) -> Option<SetFamily<'a, V>> {
        self.cache
            .get(op)
            .map(|s| SetFamily::from_set_family(*s, self))
    }

    pub(crate) fn sum_cache_get(&self, key: ZddIndex<V>) -> Option<UsizeOrPositiveInfinity> {
        self.sum_cache.get(&key).map(|x| *x.value())
    }

    pub(crate) fn sum_cache_insert(
        &self,
        key: ZddIndex<V>,
        value: UsizeOrPositiveInfinity,
    ) -> UsizeOrPositiveInfinity {
        self.sum_cache.insert(key, value);
        value
    }

    pub(crate) fn put_into_cache<'a>(
        &'a self,
        op: Operations<V>,
        value: SetFamily<'a, V>,
    ) -> SetFamily<'a, V> {
        self.cache.insert(op, value.as_raw());
        value
    }

    ///Counts the number of nodes currently held by the holder.
    #[must_use]
    pub fn n_nodes(&self) -> usize {
        self.uniq_table.n_used()
    }
}

impl<'a, V: Ord + Clone + Hash + Eq + Send + Sync> SetFamily<'a, V> {
    ///Creates a [`SetFamily`] from a [`BTreeSet<BTreeSet<V>>`].
    ///
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///let sets = ["abcd", "ac", "a", "bc", "b", "c"];
    ///let x = sets.iter().map(|x| x.chars().collect()).collect();
    ///let z = SetFamily::from_sets(x, &holder);
    ///let members: Vec<String> = z.members().map(|x| x.into_iter().collect()).collect();
    ///assert_eq!(members, sets);
    ///```
    #[must_use]
    pub fn from_sets(
        mut sets: BTreeSet<BTreeSet<V>>,
        holder: &'a ZddHolder<V>,
    ) -> SetFamily<'a, V> {
        if sets.is_empty() {
            return holder.zero();
        }

        #[expect(clippy::missing_panics_doc)]
        if sets.len() == 1 && sets.first().unwrap().is_empty() {
            return holder.one();
        }

        //fine since at least one set will be non-empty since if it was only the empty set it would have been caught before.
        #[expect(clippy::missing_panics_doc)]
        let value = sets.iter().filter_map(|x| x.first()).min().unwrap().clone();

        let with_min_val = sets
            .extract_if(.., |v| v.contains(&value))
            .map(|mut x| {
                x.remove(&value);
                x
            })
            .collect::<BTreeSet<_>>();

        let without_min_val = sets;

        let (lo, hi) = holder.pools().join(
            || SetFamily::from_sets(without_min_val, holder),
            || SetFamily::from_sets(with_min_val, holder),
        );

        holder.get_node(value, lo, hi)
    }
}
