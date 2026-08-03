use std::{
    fmt::{Debug, Display},
    hash::Hash,
    ops::{Add, AddAssign, Sub},
};

use crate::{SetFamily, algorithms::max_weight::MinWeightCache, manager::TempCacheItem};
use ahash::HashMap;
use indicatif::ProgressStyle;

#[cfg(test)]
use indicatif::ProgressDrawTarget;
use num_traits::Num;
use rangemap::RangeMap;

#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
enum Infinite<T> {
    NegInf,
    Finite(T),
    PosInf,
}

impl<T: Add<T, Output = T>> Add for Infinite<T> {
    type Output = Infinite<T>;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Infinite::NegInf | Infinite::Finite(_), Infinite::NegInf)
            | (Infinite::NegInf, Infinite::Finite(_)) => Infinite::NegInf,
            (Infinite::Finite(x), Infinite::Finite(y)) => Infinite::Finite(x + y),
            (Infinite::Finite(_) | Infinite::PosInf, Infinite::PosInf)
            | (Infinite::PosInf, Infinite::Finite(_)) => Infinite::PosInf,
            (Infinite::NegInf, Infinite::PosInf) | (Infinite::PosInf, Infinite::NegInf) => {
                panic!("Addining positive and negative infinity is undefined!")
            }
        }
    }
}

impl<T: Add<T, Output = T> + Clone> AddAssign for Infinite<T> {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.clone() + rhs;
    }
}

impl<T: Display> Display for Infinite<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Infinite::NegInf => write!(f, "-∞"),
            Infinite::Finite(v) => write!(f, "{v}"),
            Infinite::PosInf => write!(f, "∞"),
        }
    }
}

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
        return Some(sets[0].clip_weight(minimum, f));
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

#[derive(Debug, Clone)]
struct IntervalCache<'a, K, V: Eq + Hash, T>(
    HashMap<K, RangeMap<Infinite<T>, NodeInterval<'a, T, V>>>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeInterval<'a, T, V: Eq + Hash> {
    node: SetFamily<'a, V>,
    accepted_worst: Infinite<T>,
    rejected_best: Infinite<T>,
}

impl<'a, K: Hash + Eq, V: Eq + Hash + Clone, T> IntervalCache<'a, K, V, T>
where
    T: Ord + Clone,
{
    fn get(&self, node: &K, budget: T) -> Option<NodeInterval<'a, T, V>> {
        self.0
            .get(node)
            .and_then(|x| x.get(&Infinite::Finite(budget)))
            .cloned()
    }

    fn insert(
        &mut self,
        node: K,
        accepted_worst: Infinite<T>,
        rejected_best: Infinite<T>,
        r: SetFamily<'a, V>,
    ) {
        self.0.entry(node).or_default().insert(
            accepted_worst.clone()..rejected_best.clone(),
            NodeInterval {
                node: r,
                accepted_worst,
                rejected_best,
            },
        );
    }
}

impl<V: Eq + Hash + Display + Clone + Ord, T> Display for NodeInterval<'_, T, V>
where
    T: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}, {})",
            self.node, self.accepted_worst, self.rejected_best
        )
    }
}

impl<V: Eq + Hash + Clone + Ord + Send + Sync, T: Num + Clone + Ord> NodeInterval<'_, T, V> {
    fn add_weight(&mut self, w: T) {
        self.accepted_worst += Infinite::Finite(w.clone());
        self.rejected_best += Infinite::Finite(w);
    }

    fn combine<F>(lo: Self, hi: Self, value: V, f: F) -> Self
    where
        F: Fn(&V) -> T + Send + Sync,
    {
        let w = Infinite::Finite(f(&value));
        let accepted_worst = std::cmp::max(lo.accepted_worst, hi.accepted_worst + w.clone());
        let rejected_best = std::cmp::min(lo.rejected_best, hi.rejected_best + w);
        let holder = lo.node.manager();
        NodeInterval {
            node: holder.get_node(value, lo.node, hi.node),
            accepted_worst,
            rejected_best,
        }
    }
}

///Re-implementation of `checked_sub` even when not necessary.
pub trait PossiblyPointlessCheckedSub: Sized + Sub<Output = Self> {
    fn checked_sub(&self, v: &Self) -> Option<Self>;
}

/// For types with a native `checked_sub` inherent method (integers).
macro_rules! impl_checked_sub_native {
    ($($t:ty),* $(,)?) => {
        $(
            impl PossiblyPointlessCheckedSub for $t {
                fn checked_sub(&self, v: &Self) -> Option<Self> {
                    <$t>::checked_sub(*self, *v)
                }
            }
        )*
    };
}

/// For types with only a regular `Sub` impl (e.g. floats), so the
/// subtraction is always "checked" successfully.
macro_rules! impl_checked_sub_always {
    ($($t:ty),* $(,)?) => {
        $(
            impl PossiblyPointlessCheckedSub for $t {
                fn checked_sub(&self, v: &Self) -> Option<Self> {
                    Some(*self - *v)
                }
            }
        )*
    };
}

impl_checked_sub_native!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

use ordered_float::{NotNan, OrderedFloat};
impl_checked_sub_always!(
    f32,
    f64,
    OrderedFloat<f32>,
    OrderedFloat<f64>,
    NotNan<f32>,
    NotNan<f64>
);

impl<K, V: Eq + Hash, T> Default for IntervalCache<'_, K, V, T> {
    fn default() -> Self {
        Self(HashMap::default())
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Given a function that maps elements of the [`SetFamily`] to usize and a `budget`, return the
    ///ZDD consisting of all sets whose elements sum to budget or less. Allows for only positive or
    ///zero weights.
    ///
    ///Adapted from Minato, S., Kawahara, J., Banbara, M., Horiyama, T., Takigawa, I., & Yamaguchi, Y. (2025). Fast enumeration of all cost-bounded solutions for combinatorial problems using ZDDs. Discrete Applied Mathematics, 360, 467–486. `<https://doi.org/10.1016/j.dam.2024.10.003>`
    #[must_use]
    pub fn clip_weight<F, T>(&self, budget: T, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + TempCacheItem<'a, V, Output = T> + Ord + Clone + PossiblyPointlessCheckedSub,
    {
        let cache = self.manager().create_temporary_cache();
        self.clone()
            .clip_weight_inner(&f, budget, &mut IntervalCache::default(), &cache)
            .node
    }

    ///Adapted from Minato, S., Kawahara, J., Banbara, M., Horiyama, T., Takigawa, I., & Yamaguchi, Y. (2025). Fast enumeration of all cost-bounded solutions for combinatorial problems using ZDDs. Discrete Applied Mathematics, 360, 467–486. `<https://doi.org/10.1016/j.dam.2024.10.003>`
    #[expect(clippy::needless_pass_by_value)]
    fn clip_weight_inner<F, T>(
        self,
        f: &F,
        budget: T,
        cache: &mut IntervalCache<'a, SetFamily<'a, V>, V, T>,
        min_cache: &MinWeightCache<'a, V, Option<T>>,
    ) -> NodeInterval<'a, T, V>
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + TempCacheItem<'a, V, Output = T> + Ord + Clone + PossiblyPointlessCheckedSub,
    {
        if self.is_zero() {
            return NodeInterval {
                node: self,
                accepted_worst: Infinite::NegInf,
                rejected_best: Infinite::PosInf,
            };
        }

        if self.is_one() {
            return if budget >= T::zero() {
                NodeInterval {
                    node: self,
                    accepted_worst: Infinite::Finite(T::zero()),
                    rejected_best: Infinite::PosInf,
                }
            } else {
                NodeInterval {
                    node: self.manager().zero(),
                    accepted_worst: Infinite::NegInf,
                    rejected_best: Infinite::Finite(T::zero()),
                }
            };
        }

        if let Some(r) = cache.get(&self, budget.clone()) {
            return r;
        }

        let (v, lo, hi) = self.get().unwrap();

        let mut lo_interval = lo.clip_weight_inner(f, budget.clone(), cache, min_cache);
        let w = f(&v);
        if let Some(hi_budget) = budget.checked_sub(&w) {
            let hi_interval = hi.clip_weight_inner(f, hi_budget, cache, min_cache);
            let combined = NodeInterval::combine(lo_interval, hi_interval, v, f);

            cache.insert(
                self.clone(),
                combined.accepted_worst.clone(),
                combined.rejected_best.clone(),
                combined.node.clone(),
            );
            combined
        } else {
            let h = hi.min_weight_inner(f, min_cache).map(|x| x + w);
            let h = match h {
                Some(num) => Infinite::Finite(num),
                None => Infinite::PosInf,
            };
            lo_interval.rejected_best = std::cmp::min(lo_interval.rejected_best, h);

            cache.insert(
                self.clone(),
                lo_interval.accepted_worst.clone(),
                lo_interval.rejected_best.clone(),
                lo_interval.node.clone(),
            );
            lo_interval
        }
    }
}

/*
impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Given a function that maps elements of the [`SetFamily`] to isize and a `budget`, return the
    ///ZDD consisting of all sets whose elements sum to budget or less. Allows for negative weights.
    ///
    ///Adapted from Minato, S., Kawahara, J., Banbara, M., Horiyama, T., Takigawa, I., & Yamaguchi, Y. (2025). Fast enumeration of all cost-bounded solutions for combinatorial problems using ZDDs. Discrete Applied Mathematics, 360, 467–486. `<https://doi.org/10.1016/j.dam.2024.10.003>`
    #[must_use]
    pub fn clip_weight<F>(&self, budget: isize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> isize + Send + Sync,
    {
        self.clone()
            .clip_weight_inner(&f, budget, &mut IntervalCache::default())
            .node
    }

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
                accepted_worst: IsizeOrInfinity::NegInfinity,
                rejected_best: IsizeOrInfinity::PosInfinity,
            };
        }

        if self.is_one() {
            return if budget >= 0 {
                NodeInterval {
                    node: self,
                    accepted_worst: IsizeOrInfinity::Finite(0),
                    rejected_best: IsizeOrInfinity::PosInfinity,
                }
            } else {
                NodeInterval {
                    node: self.manager().zero(),
                    accepted_worst: IsizeOrInfinity::NegInfinity,
                    rejected_best: IsizeOrInfinity::Finite(0),
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
}*/

impl<'a, V: Eq + Hash + Ord + Clone + Send + Sync> SetFamily<'a, V> {
    ///Performs join (Minato, 1994 refers to this as "product") over two family
    ///subsets while capping the maximum size of output
    ///
    ///It is defined as join(f, g) = { α ∪ β | α ∈ f ∧ β ∈ g ∧ \sum_{x\in α ∪ β} f(x) <= budget }
    ///
    ///# Panics
    ///May panic if `self` or `other` are undefined in the [`ZddHolder`](crate::manager::ZddHolder).
    #[must_use]
    pub fn bounded_join<F>(self, other: SetFamily<'a, V>, f: F, budget: usize) -> SetFamily<'a, V>
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
        min_cache: &MinWeightCache<'a, V, Option<usize>>,
    ) -> NodeInterval<'a, usize, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if other.is_zero() || self.is_zero() {
            return NodeInterval {
                node: self.manager().zero(),
                accepted_worst: Infinite::Finite(0),
                rejected_best: Infinite::PosInf,
            };
        }

        if other.is_one() {
            return self.clip_weight_inner(f, budget, clipping_cache, min_cache);
        }

        if self.is_one() {
            return other.clip_weight_inner(f, budget, clipping_cache, min_cache);
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
                x.add_weight(w);
            }

            his
        } else {
            [0; 3].map(|_| NodeInterval {
                node: self.manager().zero(),
                accepted_worst: Infinite::Finite(0),
                rejected_best: Infinite::Finite(budget + 1),
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
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use itertools::Itertools;
    use rand::{RngExt, SeedableRng, rngs};

    use crate::{
        ZddHolder,
        utils::test::{random_family, random_isize_weights, random_weights},
    };

    use super::*;

    #[test]
    fn isize_tests() {
        assert!(Infinite::NegInf < Infinite::Finite(3));
        assert!(Infinite::<u32>::NegInf < Infinite::PosInf);
        assert!(Infinite::Finite(3) < Infinite::PosInf);
        assert!(Infinite::Finite(-3) < Infinite::Finite(3));
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
            let clipped_sol = x.clip_weight(budget, f);

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
                let max_weight = s.clip_weight(budget, f);
                max_weight.check_valid_zdd();
                assert_eq!(max_weight, other, "{max_weight} != {other}");
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
                assert_eq!(alt_c.clip_weight(budget, f), bounded_sets);
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
