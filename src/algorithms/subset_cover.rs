use std::{
    collections::{BTreeSet, BinaryHeap},
    fmt::{Debug, Display},
    hash::Hash,
    ops::Add,
    time::Instant,
};

use ahash::{HashMap, RandomState};
use indicatif::{ProgressIterator, ProgressStyle};
use rangemap::RangeMap;
use serde::de::value::IsizeDeserializer;

#[derive(Debug, Clone, Eq, PartialEq)]
struct QueueWrapper<'a, V: Eq + Hash + Clone>(SetFamily<'a, V>);

impl<V: Eq + Hash + Clone> PartialOrd for QueueWrapper<'_, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V: Eq + Hash + Clone> Ord for QueueWrapper<'_, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .n_nodes()
            .cmp(&self.0.n_nodes())
            .then(self.0.id.cmp(&other.0.id))
    }
}

use crate::{
    SetFamily,
    algorithms::{
        UsizeOrPositiveInfinity,
        minimum_cutoff::MaxWeightOfCache,
        subset_cover::ISizeOrInfinity::{Finite, NegInfinity, PosInfinity},
    },
    manager::{TempCache, TempCacheItem, ZddIndex},
    utils::SingleSet,
};

//TODO: Write a DP style algo based on `[SetFamily::budget_exact]` which adds all sets that add up o 28.

/// Given sets $S$, with elements weighted by function $f$, returns the zdd
/// such that where $b$ is the budget:
///
/// x = { x | ∀s∈S s⊆x ∧ ∑e∈x f(e) ≤ b }
///
/// # Panics
/// Will panic if `sets` is empty or if the sets don't all share the same manager.
pub fn subset_cover<V, F>(
    mut sets: Vec<SetFamily<'_, V>>,
    budget: isize,
    f: F,
) -> Option<SetFamily<'_, V>>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> isize + Send + Sync,
{
    assert!(!sets.is_empty(), "Sets cannot be empty!");
    let sets = sets.into_iter().collect::<Vec<_>>();
    let time = Instant::now();
    //let cache = holder.create_temporary_cache();
    //let min_weight_cache = holder.create_temporary_cache();
    //sets.sort_by_key(|x| x.id);
    //sets.dedup();
    let mut universe = BTreeSet::new();
    for s in &sets {
        universe.extend(s.universe::<RandomState>());
    }
    println!("Made universe!");
    let style = ProgressStyle::default_bar()
        .template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({per_sec}, ETA {eta_precise})"
        )
        .unwrap()
        .progress_chars("#>-");

    println!("Now supersets!");

    let universe_single = sets.first().unwrap().manager().single_set(universe.clone());

    let split_sets = sets
        .into_iter()
        .map(|x| x.bounded_supersets(&f, budget))
        .progress_with_style(style.clone())
        .collect::<Vec<_>>();

    panic!("Done!");

    /*
    let up_sets = up_sets
        .into_iter()
        .map(|x| {
            let u = x.universe::<RandomState>().into_iter().collect();
            x.extend_as_superset(universe.difference(&u).cloned())
        })
        .progress_with_style(style.clone())
        .collect::<Vec<_>>();

    println!("Done extending!");
    let mut up_sets = up_sets
        .into_iter()
        .map(|x| QueueWrapper(x))
        .collect::<BinaryHeap<_>>();

    let bar = indicatif::ProgressBar::new(up_sets.len() as u64 - 1).with_style(style);
    while up_sets.len() >= 2 {
        let a = up_sets.pop().unwrap().0;
        let b = up_sets.pop().unwrap().0;
        let c = a.intersect(b);
        up_sets.push(QueueWrapper(c));
        bar.inc(1);
    }
    bar.finish();
    let time = time.elapsed().as_secs_f64();
    println!("Joining took {time} seconds.");*/

    None
}

struct SplitSetFamily<'a, V: Eq + Hash> {
    sets: HashMap<usize, SetFamily<'a, V>>,
}

struct RawSplitSetFamily<V: Eq + Hash> {
    sets: HashMap<usize, ZddIndex<V>>,
}

impl<'a, V: Eq + Hash + 'a> TempCacheItem<'a, V> for RawSplitSetFamily<V> {
    type Output = SplitSetFamily<'a, V>;

    fn to_gc(&self, holder: &'a crate::ZddHolder<V>) -> Self::Output {
        SplitSetFamily {
            sets: self
                .sets
                .iter()
                .map(|(k, v)| (*k, SetFamily::from_set_family(*v, holder)))
                .collect(),
        }
    }

    fn from_gc(x: &Self::Output) -> Self {
        RawSplitSetFamily {
            sets: x.sets.iter().map(|(k, v)| (*k, v.as_raw())).collect(),
        }
    }
}

impl<'a, V: Eq + Hash + Ord + Clone + Send + Sync> SplitSetFamily<'a, V> {
    ///adds a value v w/ weight w to each set, **assuming** that the value is not present and that
    ///it is the current least value.
    fn add_val(&mut self, v: V, w: usize) {
        self.sets = self
            .sets
            .drain()
            .zip(std::iter::repeat(v))
            .map(|((x, s), v)| {
                let holder = s.manager();
                (x + w, holder.get_node(v, holder.zero(), s))
            })
            .collect();
    }

    fn union(mut self, mut other: Self) -> Self {
        let keys = self
            .sets
            .keys()
            .chain(other.sets.keys())
            .copied()
            .collect::<BTreeSet<_>>();

        SplitSetFamily {
            sets: keys
                .into_iter()
                .map(|k| {
                    let a = self.sets.remove(&k);
                    let b = other.sets.remove(&k);
                    match (a, b) {
                        (None, None) => panic!("can't happen bc keys is made from the sets"),
                        (None, Some(x)) | (Some(x), None) => (k, x),
                        (Some(x), Some(y)) => (k, x.union(y)),
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum ISizeOrInfinity {
    NegInfinity,
    Finite(isize),
    PosInfinity,
}

impl Add for ISizeOrInfinity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (ISizeOrInfinity::Finite(x), ISizeOrInfinity::Finite(y)) => {
                ISizeOrInfinity::Finite(x + y)
            }
            (
                ISizeOrInfinity::NegInfinity | ISizeOrInfinity::Finite(_),
                ISizeOrInfinity::NegInfinity,
            )
            | (ISizeOrInfinity::NegInfinity, ISizeOrInfinity::Finite(_)) => {
                ISizeOrInfinity::NegInfinity
            }
            (
                ISizeOrInfinity::PosInfinity | ISizeOrInfinity::Finite(_),
                ISizeOrInfinity::PosInfinity,
            )
            | (ISizeOrInfinity::PosInfinity, ISizeOrInfinity::Finite(_)) => {
                ISizeOrInfinity::PosInfinity
            }
            (ISizeOrInfinity::PosInfinity, ISizeOrInfinity::NegInfinity)
            | (ISizeOrInfinity::NegInfinity, ISizeOrInfinity::PosInfinity) => {
                panic!("Negative infinity plus positive infinity is undefined!")
            }
        }
    }
}

impl Display for ISizeOrInfinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NegInfinity => write!(f, "-∞"),
            Finite(x) => write!(f, "{x}"),
            PosInfinity => write!(f, "∞"),
        }
    }
}

#[derive(Debug, Clone)]
struct IntervalCache<'a, K, V: Eq + Hash>(
    HashMap<K, RangeMap<ISizeOrInfinity, NodeInterval<'a, V>>>,
);

impl<'a, K: Hash + Eq, V: Eq + Hash + Clone> IntervalCache<'a, K, V> {
    fn get(&self, node: &K, budget: isize) -> Option<NodeInterval<'a, V>> {
        self.0
            .get(node)
            .and_then(|x| x.get(&Finite(budget)))
            .cloned()
    }

    fn insert(
        &mut self,
        node: K,
        accepted_worst: ISizeOrInfinity,
        rejected_best: ISizeOrInfinity,
        r: SetFamily<'a, V>,
    ) {
        self.0.entry(node).or_default().insert(
            accepted_worst..rejected_best,
            NodeInterval {
                node: r,
                accepted_worst,
                rejected_best,
            },
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeInterval<'a, V: Eq + Hash> {
    node: SetFamily<'a, V>,
    accepted_worst: ISizeOrInfinity,
    rejected_best: ISizeOrInfinity,
}

impl<V: Eq + Hash + Display + Clone + Ord> Display for NodeInterval<'_, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}, {})",
            self.node, self.accepted_worst, self.rejected_best
        )
    }
}

impl<V: Eq + Hash + Clone + Ord + Send + Sync> NodeInterval<'_, V> {
    fn combine<F>(lo: Self, hi: Self, value: V, f: F) -> Self
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        let w = Finite(f(&value));
        let accepted_worst = std::cmp::max(lo.accepted_worst, hi.accepted_worst + w);
        let rejected_best = std::cmp::min(lo.rejected_best, hi.rejected_best + w);
        let holder = lo.node.manager();
        NodeInterval {
            node: holder.get_node(value, lo.node, hi.node),
            accepted_worst,
            rejected_best,
        }
    }

    fn combine_superset(lo: Self, hi: Self) -> Self {
        let accepted_worst = std::cmp::max(lo.accepted_worst, hi.accepted_worst);
        let rejected_best = std::cmp::min(lo.rejected_best, hi.rejected_best);
        NodeInterval {
            node: hi.node.union(lo.node),
            accepted_worst,
            rejected_best,
        }
    }
}

impl<K, V: Eq + Hash> Default for IntervalCache<'_, K, V> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    fn clip_weight<F>(&self, budget: isize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        self.clone()
            .clip_weight_inner(&f, budget, &mut IntervalCache::default())
            .node
    }

    ///Adapted from Minato, S., Kawahara, J., Banbara, M., Horiyama, T., Takigawa, I., & Yamaguchi, Y. (2025). Fast enumeration of all cost-bounded solutions for combinatorial problems using ZDDs. Discrete Applied Mathematics, 360, 467–486. https://doi.org/10.1016/j.dam.2024.10.003
    fn clip_weight_inner<F>(
        self,
        f: &F,
        budget: isize,
        cache: &mut IntervalCache<'a, SetFamily<'a, V>, V>,
    ) -> NodeInterval<'a, V>
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        if self.is_zero() {
            return NodeInterval {
                node: self,
                accepted_worst: NegInfinity,
                rejected_best: PosInfinity,
            };
        }

        if self.is_one() {
            return if budget >= 0 {
                NodeInterval {
                    node: self,
                    accepted_worst: Finite(0),
                    rejected_best: PosInfinity,
                }
            } else {
                NodeInterval {
                    node: self.manager().zero(),
                    accepted_worst: NegInfinity,
                    rejected_best: Finite(0),
                }
            };
        }

        if let Some(r) = cache.get(&self, budget) {
            return r;
        }

        let (v, lo, hi) = self.get().unwrap();

        let lo_interval = lo.clip_weight_inner(f, budget, cache);
        let hi_interval = hi.clip_weight_inner(f, budget - f(&v), cache);
        let combined = NodeInterval::combine(lo_interval, hi_interval, v, f);

        cache.insert(
            self.clone(),
            combined.accepted_worst,
            combined.rejected_best,
            combined.node.clone(),
        );
        combined
    }
}

impl<'a, V: Eq + Hash + Ord + Clone + Send + Sync> SetFamily<'a, V> {
    fn split<F>(&self, f: &F, max_budget: usize) -> SplitSetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache = self.manager().create_temporary_cache();
        self.clone().inner_split(f, max_budget, &cache)
    }

    fn bounded_supersets<F>(self, f: F, budget: isize) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        let values = self.universe_single_set();
        self.inner_bounded_supersets(values, &f, budget, &mut IntervalCache::default())
            .node
    }

    fn inner_bounded_supersets<F>(
        self,
        mut values: SingleSet<'a, V>,
        f: &F,
        budget: isize,
        cache: &mut IntervalCache<'a, (SetFamily<'a, V>, SingleSet<'a, V>), V>,
    ) -> NodeInterval<'a, V>
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        if self.is_zero() {
            return NodeInterval {
                node: self,
                accepted_worst: NegInfinity,
                rejected_best: PosInfinity,
            };
        }

        let op = (self.clone(), values.clone());
        if let Some(r) = cache.get(&op, budget) {
            return r;
        }

        if self.is_one() {
            return if let Some(value) = values.pop_first() {
                let without_v =
                    self.clone()
                        .inner_bounded_supersets(values.clone(), f, budget, cache);
                let with_v =
                    self.clone()
                        .inner_bounded_supersets(values, f, budget - f(&value), cache);

                let combined = NodeInterval::combine(without_v, with_v, value, f);
                cache.insert(
                    op,
                    combined.accepted_worst,
                    combined.rejected_best,
                    combined.node.clone(),
                );
                combined
            } else if budget >= 0 {
                NodeInterval {
                    node: self,
                    accepted_worst: Finite(0),
                    rejected_best: PosInfinity,
                }
            } else {
                NodeInterval {
                    node: self.manager().zero(),
                    accepted_worst: NegInfinity,
                    rejected_best: Finite(0),
                }
            };
        }

        let (value, lo, hi) = self.get().unwrap();
        let set_v = values.pop_first().unwrap();

        if set_v < value {
            let without_v = self
                .clone()
                .inner_bounded_supersets(values.clone(), f, budget, cache);
            let with_v = self
                .clone()
                .inner_bounded_supersets(values, f, budget - f(&set_v), cache);

            let combined = NodeInterval::combine(without_v, with_v, set_v, f);
            cache.insert(
                op,
                combined.accepted_worst,
                combined.rejected_best,
                combined.node.clone(),
            );
            combined
        } else {
            let lo_without_x = lo
                .clone()
                .inner_bounded_supersets(values.clone(), f, budget, cache);
            let lo_with_x =
                lo.inner_bounded_supersets(values.clone(), f, budget - f(&value), cache);
            let hi = hi.inner_bounded_supersets(values, f, budget - f(&value), cache);

            let hi = NodeInterval::combine_superset(lo_with_x, hi);

            let combined = NodeInterval::combine(lo_without_x, hi, value, f);
            cache.insert(
                op,
                combined.accepted_worst,
                combined.rejected_best,
                combined.node.clone(),
            );
            combined
        }
    }

    fn inner_split<F>(
        self,
        f: &F,
        max_budget: usize,
        cache: &TempCache<'a, V, ZddIndex<V>, RawSplitSetFamily<V>>,
    ) -> SplitSetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return SplitSetFamily {
                sets: HashMap::default(),
            };
        }

        if self.is_one() {
            return SplitSetFamily {
                sets: [(0, self)].into_iter().collect(),
            };
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (min, max) = self.bounds(f);

        let r = if min == max {
            //then we can treat all subsequent sets identically.
            SplitSetFamily {
                sets: [(min, self.clone())].into_iter().collect(),
            }
        } else {
            let (v, lo, hi) = self.get().unwrap(); //since min == max if x is terminal.
            let mut sets = lo.inner_split(f, max_budget, cache);

            let w = f(&v);
            if let Some(hi_budget) = max_budget.checked_sub(w) {
                let mut hi = hi.inner_split(f, hi_budget, cache);
                hi.add_val(v, w);
                sets = sets.union(hi);
            }

            sets
        };
        cache.insert(self.as_raw(), r)
    }
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use rand::{SeedableRng, rngs};

    use crate::{
        ZddHolder,
        utils::test::{random_family, random_isize_weights},
    };

    use super::*;

    #[test]
    fn isize_tests() {
        assert!(ISizeOrInfinity::NegInfinity < ISizeOrInfinity::Finite(3));
        assert!(ISizeOrInfinity::NegInfinity < ISizeOrInfinity::PosInfinity);
        assert!(ISizeOrInfinity::Finite(3) < ISizeOrInfinity::PosInfinity);
        assert!(ISizeOrInfinity::Finite(-3) < ISizeOrInfinity::Finite(3));
    }

    #[test]
    fn clipping() {
        let holder = ZddHolder::new();
        let universe = "abcdef".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(37);

        for i in 0..1000 {
            println!("{i}");
            let family = random_family(&universe, &mut rng);
            let weights = random_isize_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();
            let max_budget = weights.values().sum::<isize>();

            let s = SetFamily::from_sets(family.clone(), &holder);

            for budget in 0..=max_budget {
                let other = family
                    .iter()
                    .filter(|x| x.iter().map(f).sum::<isize>() <= budget)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let other = SetFamily::from_sets(other, &holder);
                let max_weight = s.clip_weight(budget, f);
                max_weight.check_valid_zdd();
                assert_eq!(max_weight, other, "{max_weight} != {other}");
            }
        }
    }

    #[test]
    fn bounded_superset() {
        let holder = ZddHolder::new();
        let universe = "abcd".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(0);

        for _ in 0..1000 {
            let family = random_family(&universe, &mut rng);
            let weights = random_isize_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();
            let s = SetFamily::from_sets(family.clone(), &holder);
            let elements = family.iter().flatten().copied().collect::<BTreeSet<_>>();
            let mut supersets = BTreeSet::new();
            for s in family {
                let to_add = elements.difference(&s).copied().collect::<Vec<_>>();
                for x in to_add.into_iter().powerset() {
                    let mut s = s.clone();
                    s.extend(x);
                    supersets.insert(s);
                }
            }

            let max_budget = weights.values().sum::<isize>();
            for b in 0..max_budget {
                let bounded_sets = supersets
                    .iter()
                    .filter(|x| x.iter().map(f).sum::<isize>() <= b)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                println!("budget={b} s={s} bounded={bounded_sets:?} {weights:?}");
                let bounded_sets = SetFamily::from_sets(bounded_sets, &holder);
                let bounded_s = s.clone().bounded_supersets(f, b);
                bounded_s.check_valid_zdd();

                assert_eq!(
                    bounded_s, bounded_sets,
                    "Calculated != desired {bounded_s} != {bounded_sets} "
                );
            }
        }
    }
}
