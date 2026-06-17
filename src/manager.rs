//! Zuddy is a crate for handling ZDDs
use std::{
    collections::BTreeSet,
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    sync::{Arc, Mutex, RwLock},
};

use ahash::RandomState;
use dashmap::DashMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
mod garbage;
mod parallelism;
mod raw;

use parallelism::ZddThreadPool;
pub(crate) use raw::RawZdd;
use raw::Zdd;

use super::{ONE_IDX, Operations, SetFamily, ZERO_IDX};

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "V: Eq+Serialize+DeserializeOwned+Hash")]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V: Eq + Hash> {
    #[serde(default, skip)]
    pools: ZddThreadPool,
    free: Arc<Mutex<Vec<usize>>>,
    data: Arc<RwLock<Vec<Option<Zdd<V>>>>>,
    uniq_table: DashMap<Zdd<V>, RawZdd<V>, RandomState>,
    cache: DashMap<Operations<V>, RawZdd<V>, RandomState>,
    sum_cache: DashMap<RawZdd<V>, Option<usize>, RandomState>,
}

fn free_id<V>(data: &mut Vec<Option<Zdd<V>>>, free: &mut Vec<usize>) -> RawZdd<V> {
    if let Some(x) = free.pop() {
        x.into()
    } else {
        data.push(None);
        (data.len() - 1).into()
    }
}

impl<V: Eq + Hash> Default for ZddHolder<V> {
    fn default() -> Self {
        Self {
            pools: ZddThreadPool::default(),
            free: Arc::new(Mutex::new(vec![])),
            data: Arc::new(RwLock::new(vec![None, None])),
            uniq_table: DashMap::default(),
            sum_cache: DashMap::default(),
            cache: DashMap::default(),
        }
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

    pub(crate) fn sum_cache_get(&self, key: &RawZdd<V>) -> Option<Option<usize>> {
        self.sum_cache.get(key).map(|x| x.value().clone())
    }

    pub(crate) fn sum_cache_insert(&self, key: RawZdd<V>, value: Option<usize>) -> Option<usize> {
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
        let mut data = Vec::with_capacity(n);
        data.push(None);
        data.push(None);

        let uniq_table = DashMap::with_capacity_and_hasher(n, RandomState::new());
        let sum_cache = DashMap::with_capacity_and_hasher(n, RandomState::new());
        let cache = DashMap::with_capacity_and_hasher(n, RandomState::new());

        Self {
            pools: ZddThreadPool::default(),
            free: Arc::new(Mutex::new(vec![])),
            data: Arc::new(RwLock::new(data)),
            uniq_table,
            sum_cache,
            cache,
        }
    }
}

fn get_node<V: Eq + Hash + Clone>(
    family: Zdd<V>,
    data: &mut Vec<Option<Zdd<V>>>,
    free: &mut Vec<usize>,
    uniq_table: &DashMap<Zdd<V>, RawZdd<V>, RandomState>,
) -> RawZdd<V> {
    if family.hi == RawZdd::ZERO {
        return family.lo;
    }

    if let Some(x) = uniq_table.get(&family) {
        return *x;
    }
    let id = free_id(data, free);
    data[usize::from(id)] = Some(family.clone());
    uniq_table.insert(family, id);
    id
}

fn from_sets<V: Eq + Hash + Ord + Clone>(
    mut sets: BTreeSet<BTreeSet<V>>,
    data: &mut Vec<Option<Zdd<V>>>,
    free: &mut Vec<usize>,
    uniq_table: &DashMap<Zdd<V>, RawZdd<V>, RandomState>,
) -> RawZdd<V> {
    if sets.is_empty() {
        return RawZdd::ZERO;
    }

    if sets.len() == 1 && sets.first().unwrap().is_empty() {
        return RawZdd::ONE;
    }

    //fine since at least one set will be non-empty since if it was only the empty set it would have been caught before.
    let value = sets.iter().filter_map(|x| x.first()).min().unwrap().clone();

    let with_min_val = sets
        .extract_if(.., |v| v.contains(&value))
        .map(|mut x| {
            x.remove(&value);
            x
        })
        .collect::<BTreeSet<_>>();

    let without_min_val = sets;

    let lo = from_sets(without_min_val, data, free, uniq_table);
    let hi = from_sets(with_min_val, data, free, uniq_table);

    get_node(Zdd { value, lo, hi }, data, free, uniq_table)
}

impl<'a, V: Ord + Clone + Hash + Eq> SetFamily<'a, V> {
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
    pub fn from_sets(sets: BTreeSet<BTreeSet<V>>, holder: &'a ZddHolder<V>) -> SetFamily<'a, V> {
        #[expect(clippy::missing_panics_doc)]
        let mut data = holder.data.write().unwrap();
        #[expect(clippy::missing_panics_doc)]
        let mut free = holder.free.lock().unwrap();
        let uniq_table = &holder.uniq_table;
        SetFamily::from_set_family(from_sets(sets, &mut data, &mut free, uniq_table), holder)
    }
}
