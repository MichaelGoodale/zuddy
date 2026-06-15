//! Zuddy is a crate for handling ZDDs
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    sync::{Arc, Mutex, RwLock},
};

///Defines algebraic manipulations of [`SetFamily`]s.
pub mod algebra;
pub mod algorithms;
///Defines iterators of various kinds over [`SetFamily`]
pub mod iterators;
mod utils;

use dashmap::DashMap;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[cfg(feature = "sampling")]
mod sampling;
use ahash::RandomState;
use algebra::Operations;

mod garbage;
mod parallelism;
use parallelism::ZddThreadPool;

pub struct SetFamily<'a, V: Eq + Hash> {
    id: usize,
    phantom: PhantomData<V>,
    manager: &'a ZddHolder<V>,
}

///A representation of a family of sets (or otherwise a set of sets).
///
///It is always connected to a particular [`ZddHolder`] which holds the actual memory.
#[derive(Debug, Serialize, Deserialize)]
pub struct RawZdd<V>(usize, PhantomData<V>);

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
    pub const ZERO: Self = RawZdd(0, PhantomData);

    ///The family containing the empty set {{}}.
    pub const ONE: Self = RawZdd(1, PhantomData);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
struct Zdd<V> {
    value: V,
    lo: RawZdd<V>,
    hi: RawZdd<V>,
}

impl<V: Eq + Hash + Clone> RawZdd<V> {
    fn get(self, holder: &ZddHolder<V>) -> Option<(V, RawZdd<V>, RawZdd<V>)> {
        holder.data.read().unwrap()[self.0]
            .as_ref()
            .map(|x| (x.value.clone(), x.lo, x.hi))
    }
}

impl<V: Eq + Hash> RawZdd<V> {
    fn is_zero(self) -> bool {
        self == RawZdd::ZERO
    }

    fn is_one(self) -> bool {
        self == RawZdd::ONE
    }

    fn children(self, holder: &ZddHolder<V>) -> Option<(RawZdd<V>, RawZdd<V>)> {
        holder.data.read().unwrap()[self.0]
            .as_ref()
            .map(|x| (x.lo, x.hi))
    }

    ///Counts the number of nodes in this [`SetFamily`]
    ///
    ///# Panics
    ///Will panic if `self` is not defined in `holder`.
    #[must_use]
    pub fn n_nodes(&self, holder: &ZddHolder<V>) -> usize {
        if self.is_zero() || self.is_one() {
            1
        } else {
            let mut edge_cache = HashSet::<RawZdd<V>, RandomState>::default();
            self.n_nodes_inner(&mut edge_cache, holder);
            edge_cache.len()
        }
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound = "V: Eq+Serialize+DeserializeOwned+Hash")]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V: Eq + Hash> {
    #[serde(default, skip)]
    pools: ZddThreadPool,
    free: Arc<Mutex<Vec<usize>>>,
    protected: BTreeSet<RawZdd<V>>,
    data: Arc<RwLock<Vec<Option<Zdd<V>>>>>,
    uniq_table: DashMap<Zdd<V>, RawZdd<V>, RandomState>,
    cache: HashMap<Operations<V>, RawZdd<V>, RandomState>,
    sum_cache: HashMap<RawZdd<V>, Option<usize>, RandomState>,
}

fn free_id<V>(data: &mut Vec<Option<Zdd<V>>>, free: &mut Vec<usize>) -> RawZdd<V> {
    if let Some(x) = free.pop() {
        RawZdd(x, PhantomData)
    } else {
        data.push(None);
        RawZdd(data.len() - 1, PhantomData)
    }
}

impl<V: Eq + Hash> Default for ZddHolder<V> {
    fn default() -> Self {
        Self {
            pools: ZddThreadPool::default(),
            protected: BTreeSet::new(),
            free: Arc::new(Mutex::new(vec![])),
            data: Arc::new(RwLock::new(vec![None, None])),
            uniq_table: DashMap::default(),
            sum_cache: HashMap::default(),
            cache: HashMap::default(),
        }
    }
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn new() -> ZddHolder<V> {
        ZddHolder::default()
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
        let sum_cache = HashMap::with_capacity_and_hasher(n, RandomState::new());
        let cache = HashMap::with_capacity_and_hasher(n, RandomState::new());

        Self {
            protected: BTreeSet::new(),
            pools: ZddThreadPool::default(),
            free: Arc::new(Mutex::new(vec![])),
            data: Arc::new(RwLock::new(data)),
            uniq_table,
            sum_cache,
            cache,
        }
    }

    fn get_node_seq(&mut self, family: Zdd<V>) -> RawZdd<V> {
        if family.hi == RawZdd::ZERO {
            return family.lo;
        }

        if let Some(x) = self.uniq_table.get(&family) {
            return *x;
        }
        let mut data = self.data.write().unwrap();
        let id = free_id(&mut data, &mut self.free.lock().unwrap());
        data[id.0] = Some(family.clone());
        self.uniq_table.insert(family, id);
        id
    }
}

impl<V: Ord + Clone + Hash + Eq> RawZdd<V> {
    ///Creates a [`SetFamily`] from a [`BTreeSet<BTreeSet<V>>`].
    ///
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///let sets = ["abcd", "ac", "a", "bc", "b", "c"];
    ///let x = sets.iter().map(|x| x.chars().collect()).collect();
    ///let z = SetFamily::from_sets(x, &mut holder);
    ///let members: Vec<String> = z.members(&mut holder).map(|x| x.into_iter().collect()).collect();
    ///assert_eq!(members, sets);
    ///```
    pub fn from_sets(mut sets: BTreeSet<BTreeSet<V>>, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if sets.is_empty() {
            return RawZdd::ZERO;
        }

        #[expect(clippy::missing_panics_doc)]
        if sets.len() == 1 && sets.first().unwrap().is_empty() {
            return RawZdd::ONE;
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

        let lo = RawZdd::from_sets(without_min_val, holder);
        let hi = RawZdd::from_sets(with_min_val, holder);

        holder.get_node_seq(Zdd { value, lo, hi })
    }
}

#[cfg(test)]
fn check_valid_zdd<V: Eq + Hash + Ord + Clone>(x: RawZdd<V>, holder: &ZddHolder<V>) {
    if x.is_one() || x.is_zero() {
        return;
    }
    let mut stack = vec![(x, BTreeSet::from([x]))];

    while let Some((x, mut path)) = stack.pop() {
        let (v, lo, hi) = x.get(holder).unwrap();

        assert!(!hi.is_zero());

        if !lo.is_zero() && !lo.is_one() {
            let (lo_v, _, _) = lo.get(holder).unwrap();
            assert!(lo_v > v);
            let mut path = path.clone();
            let not_already_included = path.insert(lo);
            assert!(not_already_included);
            stack.push((lo, path));
        }

        if !hi.is_one() {
            let (hi_v, _, _) = hi.get(holder).unwrap();
            assert!(hi_v > v);
            let not_already_included = path.insert(hi);
            assert!(not_already_included);
            stack.push((hi, path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_to_bools(value: usize, num_bits: usize) -> impl Iterator<Item = bool> {
        (0..num_bits).map(move |i| ((value >> i) & 1) == 1)
    }

    fn all_subsets<T: Clone + Ord>(universe: &BTreeSet<T>) -> BTreeSet<BTreeSet<T>> {
        let mut all_sets = BTreeSet::new();
        for i in 0..2_usize.pow(u32::try_from(universe.len()).unwrap()) {
            let set = universe
                .iter()
                .cloned()
                .zip(int_to_bools(i, universe.len()))
                .filter_map(|(a, b)| if b { Some(a) } else { None })
                .collect::<BTreeSet<_>>();
            all_sets.insert(set);
        }
        all_sets
    }

    #[test]
    fn combinations_check() {
        let universe = BTreeSet::from([1, 2, 3]);
        let subsets = all_subsets(&universe);
        let combos_of_subsets = all_subsets(&subsets);
        let mut holder = ZddHolder::<usize>::default();
        for x in combos_of_subsets {
            let set_zdd = RawZdd::from_sets(x.clone(), &mut holder);
            check_valid_zdd(set_zdd, &holder);
            let reconstructed_set = set_zdd
                .members(&holder)
                .map(|x| x.into_iter().collect())
                .collect();
            assert_eq!(x, reconstructed_set);
        }
    }
}
