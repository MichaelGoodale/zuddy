use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use ahash::AHashMap;

use crate::{
    SetFamily, ZddHolder,
    algorithms::max_weight::{BoundsWeightCache, MaxWeightCache},
};

pub(crate) struct MaxWeightOfCache<'a, V: Eq + Hash>(
    AHashMap<usize, AHashMap<SetFamily<'a, V>, SetFamily<'a, V>>>,
);
impl<'a, V: Eq + Hash> MaxWeightOfCache<'a, V> {
    pub fn new() -> Self {
        MaxWeightOfCache(AHashMap::default())
    }

    fn get(&self, key: &SetFamily<'a, V>, max_budget: usize) -> Option<SetFamily<'a, V>> {
        self.0.get(&max_budget).and_then(|x| x.get(key)).cloned()
    }

    fn insert(
        &mut self,
        key: SetFamily<'a, V>,
        max_budget: usize,
        value: SetFamily<'a, V>,
    ) -> SetFamily<'a, V> {
        let v = value.clone();
        self.0.entry(max_budget).or_default().insert(key, value);
        v
    }
}

fn exact_weight_of<'a, V, F>(
    x: SetFamily<'a, V>,
    budget: usize,
    f: &F,
    cache: &mut MaxWeightOfCache<'a, V>,
    bounds_cache: &BoundsWeightCache<'a, V>,
) -> SetFamily<'a, V>
where
    V: Eq + Hash + Clone + Send + Sync + Ord,
    F: Fn(&V) -> usize + Send + Sync,
{
    if x.is_zero() || x.is_one() {
        return if budget == 0 { x } else { x.manager().zero() };
    }

    let (min, max) = x.clone().bounds_inner(f, bounds_cache);
    let min = min.unwrap();
    if budget < min || budget > max {
        return x.manager().zero();
    }

    if let Some(r) = cache.get(&x, budget) {
        return r;
    }

    let (value, lo, hi) = x.get().unwrap();

    let w = f(&value);

    let r = if let Some(hi_budget) = budget.checked_sub(w) {
        let (lo, hi) = (
            exact_weight_of(lo, budget, f, cache, bounds_cache),
            exact_weight_of(hi, hi_budget, f, cache, bounds_cache),
        );
        x.manager().get_node(value, lo, hi)
    } else {
        exact_weight_of(lo, budget, f, cache, bounds_cache)
    };

    cache.insert(x, budget, r)
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Assign each element a weight using the `f` function, and return the Zdd consisting of all
    ///sets that have a summed weight of exactly `budget`.
    #[must_use]
    pub fn exact_weight_of<F>(&self, budget: usize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let mut cache = MaxWeightOfCache::new();
        let bounds_cache = self.manager().create_temporary_cache();
        exact_weight_of(self.clone(), budget, &f, &mut cache, &bounds_cache)
    }

    ///Assign each element a weight using the `f` function, and return the Zdd consisting of all
    ///sets that have a maximum summed weight of `budget` or less.
    #[must_use]
    pub fn max_weight_of<F>(&self, budget: usize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache: MaxWeightCache<'a, V> = self.manager().create_temporary_cache();
        let mut max_weight_of_cache = MaxWeightOfCache::new();
        self.clone()
            .max_weight_of_inner(budget, &f, &mut max_weight_of_cache, &cache)
    }

    #[must_use]
    pub(crate) fn max_weight_of_inner<F>(
        self,
        budget: usize,
        f: &F,
        map: &mut MaxWeightOfCache<'a, V>,
        cache: &MaxWeightCache<'a, V>,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let max_weight = self.clone().max_weight_inner(f, cache);
        if max_weight <= budget {
            return self;
        }
        //if let Some(r) = map.get(&self, budget) {
        //    return r;
        //}

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let r = if let Some(hi_budget) = budget.checked_sub(w) {
            let (lo, hi) = (
                lo.max_weight_of_inner(budget, f, map, cache),
                hi.max_weight_of_inner(hi_budget, f, map, cache),
            );
            self.manager().get_node(value, lo, hi)
        } else {
            lo.max_weight_of_inner(budget, f, map, cache)
        };

        map.insert(self, budget, r)
    }
}

fn budget_set_inner<V, F>(
    mut universe: BTreeSet<V>,
    holder: &ZddHolder<V>,
    budget: usize,
    f: F,
) -> BTreeMap<usize, SetFamily<'_, V>>
where
    V: Eq + Hash + Clone + Send + Sync + Ord,
    F: Fn(&V) -> usize + Send + Sync,
{
    let mut budgets = BTreeMap::from([(Reverse(budget), holder.one())]);
    while let Some(value) = universe.pop_last() {
        let w = f(&value);

        let mut new_budget = budgets.clone();

        for (Reverse(b), child) in budgets {
            if let Some(budget_with_x) = b.checked_sub(w) {
                let add_x = holder.get_node(value.clone(), holder.zero(), child);
                let entry = new_budget
                    .entry(Reverse(budget_with_x))
                    .or_insert_with(|| holder.zero());
                let old = std::mem::replace(entry, holder.zero());
                *entry = old.union(add_x);
            } else {
                break;
            }
        }
        budgets = new_budget;
    }

    for i in 0..=budget {
        budgets.entry(Reverse(i)).or_insert_with(|| holder.zero());
    }

    budgets
        .into_iter()
        .map(|(Reverse(k), v)| (budget - k, v))
        .collect()
}

impl<V: Eq + Hash + Send + Sync + Ord + Clone> ZddHolder<V> {
    /// Get all sets that can be made out of the universe such that their total weight is exactly
    /// equal to `budget` where the weight is provided by `f`.
    pub fn sets_with_exact_weight<F>(
        &self,
        universe: BTreeSet<V>,
        budget: usize,
        f: F,
    ) -> SetFamily<'_, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        #[expect(clippy::missing_panics_doc)] // won't panic since budget_set_inner is guaranteed to
        // return something for all keys 0..budget
        budget_set_inner(universe, self, budget, f)
            .remove(&budget)
            .unwrap()
    }

    /// Get all sets that can be made out of the universe such that their total weight is less than
    /// or equal to `budget` where the weight is provided by `f`.
    pub fn sets_with_weight_or_less<F>(
        &self,
        universe: BTreeSet<V>,
        budget: usize,
        f: F,
    ) -> SetFamily<'_, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let mut final_set = self.zero();
        for set in budget_set_inner(universe, self, budget, f).into_values() {
            final_set = final_set.union(set);
        }
        final_set
    }
}

#[cfg(test)]
mod test {
    use std::collections::{BTreeMap, BTreeSet, HashMap};

    use rand::{Rng, RngExt, SeedableRng, rngs, seq::IndexedRandom};

    use crate::{
        SetFamily, ZddHolder, algebra::str_to_sets, algorithms::minimum_cutoff::budget_set_inner,
        tests::all_subsets,
    };

    fn random_weights(universe: &[char], rng: &mut impl Rng) -> HashMap<char, usize> {
        universe
            .iter()
            .map(|x| (*x, rng.random_range(0..3)))
            .collect()
    }

    fn random_family(universe: &[char], rng: &mut impl Rng) -> BTreeSet<BTreeSet<char>> {
        let n_sets = rng.random_range(0..10);
        let mut sets = BTreeSet::new();
        for _ in 0..n_sets {
            let size = rng.random_range(0..universe.len());
            let set = universe.sample(rng, size).copied().collect::<BTreeSet<_>>();
            sets.insert(set);
        }
        sets
    }

    impl SetFamily<'_, char> {
        fn as_string(&self) -> String {
            let mut members = self
                .members()
                .map(|x| x.into_iter().map(|x| x.to_string()).collect::<String>())
                .collect::<Vec<_>>();
            members.sort();
            members.join(" ")
        }
    }

    #[test]
    fn exact_weight() {
        let universe = "abcdef".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(37);
        for _ in 0..100 {
            let holder = ZddHolder::new();
            let weights = random_weights(&universe, &mut rng);
            let universe = universe.iter().copied().collect();
            let f = |v: &char| *weights.get(v).unwrap();
            let max_budget = weights.values().sum::<usize>();

            let mut budget_to_set: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
            for x in all_subsets(&universe) {
                let s: usize = x.iter().map(f).sum();
                budget_to_set.entry(s).or_default().insert(x);
            }

            for i in 0..=max_budget {
                budget_to_set.entry(i).or_default();
            }

            for budget in 0..max_budget {
                let found_budgets = budget_set_inner(universe.clone(), &holder, budget, f);
                println!("{found_budgets:?}");
                assert_eq!(*found_budgets.last_key_value().unwrap().0, budget);
                for (b, v) in found_budgets {
                    let v = v
                        .members()
                        .map(|x| x.into_iter().collect())
                        .collect::<BTreeSet<_>>();

                    assert_eq!(&v, budget_to_set.get(&b).unwrap());
                }
            }
        }
    }
    #[test]
    fn minimum_weight_random() {
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
                let max_weight = s.max_weight_of(budget, f);
                max_weight.check_valid_zdd();
                assert_eq!(max_weight, other, "{max_weight} != {other}");

                let exact = family
                    .iter()
                    .filter(|x| x.iter().map(f).sum::<usize>() == budget)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let exact = SetFamily::from_sets(exact, &holder);
                let exact_weight = s.exact_weight_of(budget, f);
                exact_weight.check_valid_zdd();
                assert_eq!(
                    exact_weight, exact,
                    "{exact_weight} != {exact} budget = {budget}"
                );
            }
        }
    }

    #[test]
    fn minimum_weight() {
        let holder = ZddHolder::new();
        let weights = [
            ('a', 1),
            ('b', 2),
            ('c', 3),
            ('d', 6),
            ('e', 0),
            ('f', 2),
            ('g', 3),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let set = SetFamily::from_sets(str_to_sets("a b c ab ad ae gfe"), &holder);

        let f = |v: &char| *weights.get(v).unwrap();
        for (n, res) in [
            (0, ""),
            (1, "a ae"),
            (2, "a ae b"),
            (3, "a ab ae b c"),
            (4, "a ab ae b c"),
            (5, "a ab ae b c efg"),
            (6, "a ab ae b c efg"),
            (7, "a ab ad ae b c efg"),
            (8, "a ab ad ae b c efg"),
        ] {
            let set = set.max_weight_of(n, f);
            println!("{n}");
            assert_eq!(res, set.as_string());
        }
    }
}
