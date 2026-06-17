//! Zuddy is a crate for handling ZDDs
use std::{collections::BTreeSet, fmt::Debug, hash::Hash, marker::PhantomData, sync::RwLock};

use ahash::RandomState;
use dashmap::DashMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
mod garbage;
mod parallelism;
mod raw;

use parallelism::ZddThreadPool;
use raw::RawZddData;
pub(crate) use raw::ZddIndex;

use super::{ONE_IDX, Operations, SetFamily, ZERO_IDX};

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "V: Eq+Serialize+DeserializeOwned+Hash")]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V: Eq + Hash> {
    #[serde(default, skip)]
    pools: ZddThreadPool,
    data: Vec<RwLock<Option<RawZddData<V>>>>,
    uniq_table: DashMap<RawZddData<V>, ZddIndex<V>, RandomState>,
    cache: DashMap<Operations<V>, ZddIndex<V>, RandomState>,
    sum_cache: DashMap<ZddIndex<V>, Option<usize>, RandomState>,
}

impl<V: Eq + Hash> Default for ZddHolder<V> {
    fn default() -> Self {
        let s = Self {
            pools: ZddThreadPool::default(),
            data: (0..10_000).map(|_| RwLock::new(None)).collect(),
            uniq_table: DashMap::default(),
            sum_cache: DashMap::default(),
            cache: DashMap::default(),
        };
        s.distribute_free_index(2..10_000);
        s
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn new() -> ZddHolder<V> {
        ZddHolder::default()
    }

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

    pub(crate) fn sum_cache_get(&self, key: &ZddIndex<V>) -> Option<Option<usize>> {
        self.sum_cache.get(key).map(|x| *x.value())
    }

    pub(crate) fn sum_cache_insert(&self, key: ZddIndex<V>, value: Option<usize>) -> Option<usize> {
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
        self.uniq_table.len() + 2
    }

    ///Create a new [`ZddHolder`] to hold various ZDDs with a preallocated capacity.
    ///
    ///# Panics
    ///May panic if there is difficulty making thing thread pool in Rayon.
    #[must_use]
    pub fn with_capacity(n: usize) -> ZddHolder<V> {
        let uniq_table = DashMap::with_capacity_and_hasher(n, RandomState::new());
        let sum_cache = DashMap::with_capacity_and_hasher(n, RandomState::new());
        let cache = DashMap::with_capacity_and_hasher(n, RandomState::new());

        let s = Self {
            pools: ZddThreadPool::default(),
            data: (0..n).map(|_| RwLock::new(None)).collect(),
            uniq_table,
            sum_cache,
            cache,
        };
        s.distribute_free_index(2..n);
        s
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
