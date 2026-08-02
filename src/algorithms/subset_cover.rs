use std::{
    collections::BTreeSet,
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Add, AddAssign},
};

use crate::{
    SetFamily,
    algorithms::{
        UsizeOrPositiveInfinity,
        max_weight::MinWeightCache,
        subset_cover::ISizeOrInfinity::{Finite, NegInfinity, PosInfinity},
    },
    manager::{TempCache, TempCacheItem, ZddIndex},
    utils::SingleSet,
};
use ahash::HashMap;
use indicatif::ProgressStyle;

#[cfg(test)]
use indicatif::ProgressDrawTarget;
use rangemap::RangeMap;

/// Given sets $S$, with elements weighted by function $f$, returns the zdd
/// such that where $b$ is the budget:
///
/// x = { x | ∀s∈S s⊆x ∧ ∑e∈x f(e) ≤ b }
///
/// # Panics
/// Will panic if `sets` is empty or if the sets don't all share the same manager.
pub fn subset_cover<'a, V, F>(
    sets: &[SetFamily<'a, V>],
    f: F,
    max_budget: Option<usize>,
) -> Option<SetFamily<'a, V>>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
{
    assert!(!sets.is_empty(), "Sets cannot be empty!");

    let holder = sets.first().unwrap().manager();
    if sets.iter().any(SetFamily::is_zero) {
        return Some(holder.zero());
    }

    if sets.len() == 1 {
        let minimum = sets[0].min_weight(&f);
        return Some(sets[0].clip_weight_usize(minimum, f));
    }

    let mut solution = holder.zero();
    let mut budget = 0;
    let n_chars = (sets.len() - 1).checked_ilog10().unwrap_or(0) + 1;
    'outer: while solution.is_zero() {
        if max_budget.is_some_and(|max_budget| budget > max_budget) {
            return None;
        }

        let style = ProgressStyle::default_bar()
            .template(
                format!(
                    "[Budget={budget} {{elapsed_precise}}] {{wide_bar}} {{pos:>{n_chars}}}/{{len}} (ETA {{eta_precise}})"
                )
                .as_str(),
            )
            .unwrap();
        let bar = indicatif::ProgressBar::new(sets.len() as u64 - 1).with_style(style);

        #[cfg(test)]
        bar.set_draw_target(ProgressDrawTarget::hidden());

        let mut sets = sets.to_vec();
        while sets.len() >= 2 {
            let a = sets.pop().unwrap();
            let b = sets.pop().unwrap();
            let c = a.bounded_join(b, &f, budget);
            if c.is_zero() {
                bar.finish_and_clear();
                budget += 1;
                continue 'outer;
            }
            sets.push(c);
            bar.inc(1);
        }
        bar.finish_and_clear();
        budget += 1;
        solution = sets.pop().unwrap();
    }
    Some(solution)
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

impl AddAssign for ISizeOrInfinity {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
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
struct IntervalCache<'a, K, V: Eq + Hash, T: PossiblyInfinite>(
    HashMap<K, RangeMap<T::PossiblyInfiniteType, NodeInterval<'a, T, V>>>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeInterval<'a, T: PossiblyInfinite, V: Eq + Hash> {
    node: SetFamily<'a, V>,
    accepted_worst: T::PossiblyInfiniteType,
    rejected_best: T::PossiblyInfiniteType,
}

trait PossiblyInfinite: Copy + PartialEq {
    type PossiblyInfiniteType: Copy
        + Clone
        + PartialEq
        + Ord
        + AddAssign
        + Add<Output = Self::PossiblyInfiniteType>;
    fn as_finite(self) -> Self::PossiblyInfiniteType;
}

impl PossiblyInfinite for isize {
    type PossiblyInfiniteType = ISizeOrInfinity;

    fn as_finite(self) -> Self::PossiblyInfiniteType {
        ISizeOrInfinity::Finite(self)
    }
}
impl PossiblyInfinite for usize {
    type PossiblyInfiniteType = UsizeOrPositiveInfinity;

    fn as_finite(self) -> Self::PossiblyInfiniteType {
        UsizeOrPositiveInfinity::Size(self)
    }
}

impl<'a, K: Hash + Eq, V: Eq + Hash + Clone, T: PossiblyInfinite> IntervalCache<'a, K, V, T> {
    fn get(&self, node: &K, budget: T) -> Option<NodeInterval<'a, T, V>> {
        self.0
            .get(node)
            .and_then(|x| x.get(&budget.as_finite()))
            .cloned()
    }

    fn insert(
        &mut self,
        node: K,
        accepted_worst: T::PossiblyInfiniteType,
        rejected_best: T::PossiblyInfiniteType,
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

impl<V: Eq + Hash + Display + Clone + Ord, T: PossiblyInfinite> Display for NodeInterval<'_, T, V>
where
    T::PossiblyInfiniteType: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}, {})",
            self.node, self.accepted_worst, self.rejected_best
        )
    }
}

impl<V: Eq + Hash + Clone + Ord + Send + Sync, T: PossiblyInfinite> NodeInterval<'_, T, V> {
    fn add_weight(&mut self, w: T::PossiblyInfiniteType) {
        self.accepted_worst += w;
        self.rejected_best += w;
    }

    fn combine<F>(lo: Self, hi: Self, value: V, f: F) -> Self
    where
        F: Fn(&V) -> T + Send + Sync,
    {
        let w = f(&value).as_finite();
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

impl<K, V: Eq + Hash, T: PossiblyInfinite> Default for IntervalCache<'_, K, V, T> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    pub fn clip_weight_usize<F>(&self, budget: usize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache = self.manager().create_temporary_cache();
        self.clone()
            .clip_weight_usize_inner(&f, budget, &mut IntervalCache::default(), &cache)
            .node
    }

    ///Adapted from Minato, S., Kawahara, J., Banbara, M., Horiyama, T., Takigawa, I., & Yamaguchi, Y. (2025). Fast enumeration of all cost-bounded solutions for combinatorial problems using ZDDs. Discrete Applied Mathematics, 360, 467–486. https://doi.org/10.1016/j.dam.2024.10.003
    fn clip_weight_usize_inner<F>(
        self,
        f: &F,
        budget: usize,
        cache: &mut IntervalCache<'a, SetFamily<'a, V>, V, usize>,
        min_cache: &MinWeightCache<'a, V>,
    ) -> NodeInterval<'a, usize, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return NodeInterval {
                node: self,
                accepted_worst: UsizeOrPositiveInfinity::Size(0),
                rejected_best: UsizeOrPositiveInfinity::PositiveInfinity,
            };
        }

        if self.is_one() {
            return NodeInterval {
                node: self,
                accepted_worst: UsizeOrPositiveInfinity::Size(0),
                rejected_best: UsizeOrPositiveInfinity::PositiveInfinity,
            };
        }

        if let Some(r) = cache.get(&self, budget) {
            return r;
        }

        let (v, lo, hi) = self.get().unwrap();

        let mut lo_interval = lo.clip_weight_usize_inner(f, budget, cache, min_cache);
        let w = f(&v);
        if let Some(hi_budget) = budget.checked_sub(w) {
            let hi_interval = hi.clip_weight_usize_inner(f, hi_budget, cache, min_cache);
            let combined = NodeInterval::combine(lo_interval, hi_interval, v, f);

            cache.insert(
                self.clone(),
                combined.accepted_worst,
                combined.rejected_best,
                combined.node.clone(),
            );
            combined
        } else {
            let h = hi.min_weight_inner(f, min_cache).add_usize(w);
            lo_interval.rejected_best = std::cmp::min(lo_interval.rejected_best, h);

            cache.insert(
                self.clone(),
                lo_interval.accepted_worst,
                lo_interval.rejected_best,
                lo_interval.node.clone(),
            );
            lo_interval
        }
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    pub fn clip_weight<F>(&self, budget: isize, f: F) -> SetFamily<'a, V>
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
        cache: &mut IntervalCache<'a, SetFamily<'a, V>, V, isize>,
    ) -> NodeInterval<'a, isize, V>
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
    ///Performs join (Minato, 1994 refers to this as "product") over two family
    ///subsets while capping the maximum size of output
    ///
    ///It is defined as join(f, g) = { α ∪ β | α ∈ f ∧ β ∈ g ∧ \sum_{x\in α ∪ β} f(x) <= budget }
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`].
    #[must_use]
    pub fn bounded_join<F>(
        mut self,
        mut other: SetFamily<'a, V>,
        f: F,
        budget: usize,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let min_cache = self.manager().create_temporary_cache();
        self.inner_bounded_join(
            other,
            &f,
            budget,
            &mut IntervalCache::default(),
            &mut IntervalCache::default(),
            &min_cache,
        )
        .node
    }

    fn inner_bounded_join<F>(
        mut self,
        mut other: SetFamily<'a, V>,
        f: &F,
        budget: usize,
        cache: &mut IntervalCache<'a, (SetFamily<'a, V>, SetFamily<'a, V>), V, usize>,
        clipping_cache: &mut IntervalCache<'a, SetFamily<'a, V>, V, usize>,
        min_cache: &MinWeightCache<'a, V>,
    ) -> NodeInterval<'a, usize, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if other.is_zero() || self.is_zero() {
            return NodeInterval {
                node: self.manager().zero(),
                accepted_worst: UsizeOrPositiveInfinity::Size(0),
                rejected_best: UsizeOrPositiveInfinity::PositiveInfinity,
            };
        }

        if other.is_one() {
            return self.clip_weight_usize_inner(f, budget, clipping_cache, min_cache);
        }

        if self.is_one() {
            return other.clip_weight_usize_inner(f, budget, clipping_cache, min_cache);
        }

        let (mut value, mut self_lo, mut self_hi) = self.get().expect("Invalid index!");
        let (mut other_v, mut other_lo, mut other_hi) = other.get().expect("Invalid index!");
        if value > other_v {
            std::mem::swap(&mut value, &mut other_v);
            std::mem::swap(&mut self_lo, &mut other_lo);
            std::mem::swap(&mut self_hi, &mut other_hi);
            std::mem::swap(&mut self, &mut other);
        }

        let holder = self.manager;
        let op = (self.clone(), other.clone());
        if let Some(r) = cache.get(&op, budget) {
            return r;
        }

        if other_v > value {
            other_lo = other;
            other_hi = self.manager.zero();
        }

        let w = f(&value);

        let his = if let Some(hi_budget) = budget.checked_sub(w) {
            let mut his = [
                self_hi.clone().inner_bounded_join(
                    other_hi.clone(),
                    f,
                    hi_budget,
                    cache,
                    clipping_cache,
                    min_cache,
                ),
                self_hi.inner_bounded_join(
                    other_lo.clone(),
                    f,
                    hi_budget,
                    cache,
                    clipping_cache,
                    min_cache,
                ),
                self_lo.clone().inner_bounded_join(
                    other_hi,
                    f,
                    hi_budget,
                    cache,
                    clipping_cache,
                    min_cache,
                ),
            ];

            for x in &mut his {
                x.add_weight(UsizeOrPositiveInfinity::Size(w));
            }

            his
        } else {
            [0; 3].map(|_| NodeInterval {
                node: self.manager().zero(),
                accepted_worst: UsizeOrPositiveInfinity::Size(0),
                rejected_best: UsizeOrPositiveInfinity::Size(budget + 1),
            })
        };
        let lo = self_lo.inner_bounded_join(other_lo, f, budget, cache, clipping_cache, min_cache);
        let accepted_worst = his
            .iter()
            .chain(std::iter::once(&lo))
            .map(|x| x.accepted_worst)
            .max()
            .unwrap();

        let rejected_best = his
            .iter()
            .chain(std::iter::once(&lo))
            .map(|x| x.rejected_best)
            .min()
            .unwrap();

        let product = his
            .into_iter()
            .map(|x| x.node)
            .reduce(SetFamily::union)
            .unwrap();

        let v_product = holder.get_node(value, holder.zero(), product);

        let joined = NodeInterval {
            node: v_product.union(lo.node),
            accepted_worst,
            rejected_best,
        };

        cache.insert(
            op,
            joined.accepted_worst,
            joined.rejected_best,
            joined.node.clone(),
        );
        joined
    }

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
        cache: &mut IntervalCache<'a, (SetFamily<'a, V>, SingleSet<'a, V>), V, isize>,
    ) -> NodeInterval<'a, isize, V>
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
    use rand::{RngExt, SeedableRng, rngs};

    use crate::{
        ZddHolder,
        utils::test::{random_family, random_isize_weights, random_weights},
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
    fn test_subset_cover() {
        let holder = ZddHolder::new();
        let universe = "abcdefghijklmnopqrstuvwxyz".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(37);

        for _ in 0..20 {
            let n = rng.random_range(1..30);
            let families = (0..n)
                .map(|_| SetFamily::from_sets(random_family(&universe, &mut rng), &holder))
                .collect::<Vec<_>>();

            let weights = random_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();

            let mut x = holder.one();
            for g in families.iter().cloned() {
                x = g.join(x);
            }
            let sol = subset_cover(&families, f, None).unwrap();
            if x.is_zero() {
                println!("No solution :(");
                assert_eq!(sol, x);
                continue;
            }
            let budget = x.min_weight(f);
            let clipped_sol = x.clip_weight_usize(budget, f);

            for g in families {
                let g = g
                    .members()
                    .map(|x| x.into_iter().collect::<BTreeSet<_>>())
                    .collect::<BTreeSet<_>>();

                for m in sol
                    .members()
                    .map(|x| x.into_iter().collect::<BTreeSet<_>>())
                {
                    assert!(g.iter().any(|x| m.is_superset(x)));
                }
            }
            let (x, m) = sol.bounds(f);
            println!("{x} == {budget} == {m}");
            assert_eq!(sol, clipped_sol, "{sol} != {clipped_sol}");
        }
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
    fn clipping_usize() {
        let holder = ZddHolder::new();
        let universe = "abcdef".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(37);

        for i in 0..1000 {
            println!("{i}");
            let family = random_family(&universe, &mut rng);
            let weights = random_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();
            let max_budget = weights.values().sum::<usize>();

            let s = SetFamily::from_sets(family.clone(), &holder);

            for budget in 0..=max_budget {
                let other = family
                    .iter()
                    .filter(|x| x.iter().map(f).sum::<usize>() <= budget)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let other = SetFamily::from_sets(other, &holder);
                let max_weight = s.clip_weight_usize(budget, f);
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

    #[test]
    fn bounded_join() {
        let holder = ZddHolder::new();
        let universe = "abcd".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(0);

        for _ in 0..1000 {
            let weights = random_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();

            let a = random_family(&universe, &mut rng);
            let b = random_family(&universe, &mut rng);
            let c = a
                .iter()
                .cartesian_product(b.iter())
                .map(|(a, b)| a.union(b).copied().collect::<BTreeSet<_>>())
                .collect::<BTreeSet<_>>();

            let a = SetFamily::from_sets(a, &holder);
            let b = SetFamily::from_sets(b, &holder);

            let max_budget = weights.values().sum::<usize>();
            for budget in 0..max_budget {
                println!("{weights:?}");
                println!("{a} x {b} while under {budget}");
                let alt_c = a.clone().join(b.clone());
                assert_eq!(alt_c, SetFamily::from_sets(c.clone(), &holder));

                let bounded_c = c
                    .iter()
                    .filter(|x| x.iter().map(f).sum::<usize>() <= budget)
                    .cloned()
                    .collect();
                let bounded_sets = SetFamily::from_sets(bounded_c, &holder);
                assert_eq!(alt_c.clip_weight_usize(budget, f), bounded_sets);
                let bounded_s = a.clone().bounded_join(b.clone(), f, budget);
                bounded_s.check_valid_zdd();

                assert_eq!(
                    bounded_s, bounded_sets,
                    "Calculated != desired {bounded_s} != {bounded_sets} "
                );
            }
        }
    }
}
