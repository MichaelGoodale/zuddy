use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use uuid::Uuid;

use crate::{
    Operations::{self},
    SetFamily, ZddHolder,
    algorithms::max_weight::MaxWeightCache,
};

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///Assign each element a weight using the `f` function, and return the Zdd consisting of all
    ///sets that have a maximum summed weight of `weight` or less.
    #[must_use]
    pub fn max_weight_of<F>(&self, budget: usize, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        //We give the function a UUID so that the cache doesn't mix up if someone runs w/ two
        //different functions for weight.
        let f_id = Uuid::new_v4();
        let cache: MaxWeightCache<'a, V> = self.manager().create_temporary_cache();
        self.clone().max_weight_of_inner(budget, &f, f_id, &cache)
    }

    #[must_use]
    pub(crate) fn max_weight_of_inner<F>(
        self,
        budget: usize,
        f: &F,
        f_id: Uuid,
        cache: &MaxWeightCache<'a, V>,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let op = Operations::MaxWeightCutoff(self.as_raw(), budget, f_id);
        if let Some(r) = self.manager().get_from_cache(&op) {
            return r;
        }

        let max_weight = self.clone().max_weight_inner(f, cache);
        if max_weight <= budget {
            return self.manager().put_into_cache(op, self);
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let r = if let Some(hi_budget) = budget.checked_sub(w) {
            let (lo, hi) = (
                lo.max_weight_of_inner(budget, f, f_id, cache),
                hi.max_weight_of_inner(hi_budget, f, f_id, cache),
            );
            self.manager().get_node(value, lo, hi)
        } else {
            lo.max_weight_of_inner(budget, f, f_id, cache)
        };

        self.manager().put_into_cache(op, r)
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

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Combines [`SetFamily::join`] with [`SetFamily::max_weight`] in one step.
    #[must_use]
    pub fn max_weight_product<F>(
        self,
        other: SetFamily<'a, V>,
        budget: usize,
        f: F,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        //We give the function a UUID so that the cache doesn't mix up if someone runs w/ two
        //different functions for weight.
        let f_id = Uuid::new_v4();
        let cache: MaxWeightCache<'a, V> = self.manager().create_temporary_cache();
        self.max_weight_product_inner(other, budget, &f, f_id, &cache)
    }

    fn max_weight_product_inner<F>(
        mut self,
        mut other: SetFamily<'a, V>,
        budget: usize,
        f: &F,
        f_id: Uuid,
        cache: &MaxWeightCache<'a, V>,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if other.is_zero() || self.is_zero() {
            return self.manager().zero();
        }

        if other.is_one() {
            return self.max_weight_of_inner(budget, f, f_id, cache);
        } else if self.is_one() {
            return other.max_weight_of_inner(budget, f, f_id, cache);
        }

        let holder = self.manager();
        let op = Operations::MaxWeightCutoffJoin(self.as_raw(), other.as_raw(), budget, f_id);
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let (mut value, mut self_lo, mut self_hi) = self.get().expect("Invalid index!");
        let (mut other_v, mut other_lo, mut other_hi) = other.get().expect("Invalid index!");

        //Ensure that value is the value that we're switching over.
        if value > other_v {
            std::mem::swap(&mut other, &mut self);
            std::mem::swap(&mut value, &mut other_v);
            std::mem::swap(&mut self_lo, &mut other_lo);
            std::mem::swap(&mut self_hi, &mut other_hi);
        }

        let weight = f(&value);
        let r = if let Some(hi_budget) = budget.checked_sub(weight) {
            if other_v > value {
                other_lo = other;
                other_hi = self.manager.zero();
            }

            let self_hi_clone = self_hi.clone();
            let other_lo_clone = other_lo.clone();
            let other_hi_clone = other_hi.clone();

            // if other_v > value then other_hi is 0 so we don't add anything.
            // if other_v == value then we only subtract from budget the once so we use hi_budget
            let (a, (b, c)) = self.manager().pools().join(
                || self_hi_clone.max_weight_product_inner(other_hi, hi_budget, f, f_id, cache),
                || {
                    self.manager().pools().join(
                        || {
                            self_hi.max_weight_product_inner(
                                other_lo_clone,
                                hi_budget,
                                f,
                                f_id,
                                cache,
                            )
                        },
                        || {
                            self_lo.clone().max_weight_product_inner(
                                other_hi_clone,
                                hi_budget,
                                f,
                                f_id,
                                cache,
                            )
                        },
                    )
                },
            );

            let product = a.union(b).union(c);
            let v_product = holder.get_node(value, holder.zero(), product);
            v_product.union(self_lo.max_weight_product_inner(other_lo, budget, f, f_id, cache))
        } else if other_v == value {
            self_lo.max_weight_product_inner(other_lo, budget, f, f_id, cache)
        } else {
            self_lo.max_weight_product_inner(other, budget, f, f_id, cache)
        };

        self.manager().put_into_cache(op, r)
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
    fn minimum_weight_product() {
        let universe = "abcdef".chars().collect::<Vec<_>>();
        let mut rng = rngs::SmallRng::seed_from_u64(37);
        for _ in 0..100 {
            let holder = ZddHolder::new();
            let family_a = SetFamily::from_sets(random_family(&universe, &mut rng), &holder);
            let family_b = SetFamily::from_sets(random_family(&universe, &mut rng), &holder);

            let product = family_a.clone().join(family_b.clone());
            let weights = random_weights(&universe, &mut rng);
            let f = |v: &char| *weights.get(v).unwrap();

            println!(
                "A = \"{}\"\nB=\"{}\"\nProduct=\"{}\"",
                family_a.as_string(),
                family_b.as_string(),
                product.as_string()
            );

            println!("{}", family_a.graphviz());
            println!("{}", family_b.graphviz());
            println!("Weights = {weights:?}");

            for budget in 0..10 {
                println!("Budget = {budget}");
                let indirect = product.max_weight_of(budget, f);
                println!("Product then MaxWeight = \"{}\"", indirect.as_string());
                let direct = family_a
                    .clone()
                    .max_weight_product(family_b.clone(), budget, f);
                println!("Direct MaxWeightProduct = \"{}\"", direct.as_string());

                let raw_direct = product
                    .members()
                    .filter_map(|x| {
                        if x.iter().map(f).sum::<usize>() <= budget {
                            Some(x.into_iter().collect())
                        } else {
                            None
                        }
                    })
                    .collect::<BTreeSet<BTreeSet<char>>>();
                let raw = SetFamily::from_sets(raw_direct, &holder);
                println!("Raw Direct Product = \"{}\"", raw.as_string());

                assert_eq!(raw, indirect, "Problem with max_weight!");
                assert_eq!(direct, raw, "Problem with max_weight_product!");
                println!();
            }
        }
    }

    #[test]
    fn simple_min_weight_product() {
        let holder = ZddHolder::new();
        let weights = [
            ('a', 1),
            ('b', 2),
            ('c', 3),
            ('d', 4),
            ('e', 5),
            ('f', 6),
            ('g', 7),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        for (a, b, n, res) in [
            ("a", "a b c", 1, "a"),
            ("a", "a b c", 4, "a ab ac"),
            ("a b c", "a b c", 3, "a ab b c"),
            ("a b c ", "a b c", 3, "a ab b c"),
            ("a b c ", "a b c ", 3, " a ab b c"),
        ] {
            let a = SetFamily::from_sets(str_to_sets(a), &holder);
            let b = SetFamily::from_sets(str_to_sets(b), &holder);
            let f = |v: &char| *weights.get(v).unwrap();
            let s = a.max_weight_product(b, n, f);
            assert_eq!(s.as_string(), res);
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
