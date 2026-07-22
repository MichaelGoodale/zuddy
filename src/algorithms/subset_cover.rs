use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use ahash::RandomState;
use uuid::Uuid;

use crate::{
    SetFamily, ZddHolder,
    algorithms::max_weight::MaxWeightCache,
    manager::{TempCache, ZddIndex},
    utils::{PivotedSets, SingleSet},
};

struct SubsetCoverCache<'a, V: Eq + Hash> {
    id: Uuid,
    max_weight_cache: MaxWeightCache<'a, V>,
    cache: TempCache<'a, V, (ZddIndex<V>, SingleSet<'a, V>, usize)>,
}

impl<V: Eq + Hash> SubsetCoverCache<'_, V> {
    fn new(holder: &ZddHolder<V>) -> SubsetCoverCache<'_, V> {
        SubsetCoverCache {
            id: Uuid::new_v4(),
            cache: holder.create_temporary_cache(),
            max_weight_cache: holder.create_temporary_cache(),
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
pub fn subset_cover<V, F>(sets: Vec<SetFamily<'_, V>>, budget: usize, f: F) -> SetFamily<'_, V>
where
    V: Eq + Hash + Clone + Ord + Send + Sync,
    F: Fn(&V) -> usize + Send + Sync,
{
    assert!(!sets.is_empty(), "Sets cannot be empty!");
    let holder = sets.first().unwrap().manager();
    let mut universe = BTreeSet::new();

    let set_to_elements: BTreeMap<_, _> = sets
        .iter()
        .cloned()
        .map(|set| {
            let set_elements = set.universe::<RandomState>();
            universe.extend(set_elements.clone());
            (set, set_elements.into_iter().collect())
        })
        .collect();

    let all_possibles = holder.sets_with_exact_weight(universe.clone(), budget, &f);
    println!("Preprocessing done!");

    let cache = SubsetCoverCache::new(holder);

    for set in sets.into_iter().take(2) {
        let elements = set_to_elements.get(&set).unwrap();
        let items_not_in_set = universe.difference(elements).cloned().collect::<Vec<_>>();
        let mut super_set = set.superset();
        super_set = super_set.max_weight_of_inner(budget, &f, cache.id, &cache.max_weight_cache);
        println!("extending");
        super_set = super_set.extend_as_superset_with_budget(items_not_in_set, budget, &f, &cache);
        println!("done, now intersecting!");
        println!(
            "{} node and {} nodes",
            super_set.n_nodes(),
            all_possibles.n_nodes()
        );
        println!(
            "reduced! {} and {}",
            super_set.n_nodes(),
            all_possibles.n_nodes()
        );
    }

    todo!()
}

impl<'a, V: Hash + Ord + Eq + Clone + Send + Sync> SetFamily<'a, V> {
    ///Adds a value to all sets, but keeps the original sets, while also removing any set with a
    ///higher total weight than budget.
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∈ f} ∪ f
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    fn extend_as_superset_with_budget<F>(
        &self,
        values: impl IntoIterator<Item = V>,
        budget: usize,
        f: F,
        cache: &SubsetCoverCache<'a, V>,
    ) -> Self
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return self.clone();
        }
        let values = self.manager().single_set(values.into_iter().collect());
        extend_as_superset_inner(self, values, budget, &f, cache)
    }
}

fn extend_as_superset_inner<'a, V, F>(
    set: &SetFamily<'a, V>,
    values: SingleSet<'a, V>,
    budget: usize,
    f: &F,
    cache: &SubsetCoverCache<'a, V>,
) -> SetFamily<'a, V>
where
    F: Fn(&V) -> usize + Send + Sync,
    V: Eq + Hash + Ord + Send + Sync + Clone,
{
    if set.is_zero() || values.is_empty() {
        return set
            .clone()
            .max_weight_of_inner(budget, f, cache.id, &cache.max_weight_cache);
    }
    let holder = set.manager;
    if set.is_one() {
        return add_all_subsets_bounded(holder.one(), values, budget, f, cache);
    }

    let op = (set.as_raw(), values.clone(), budget);
    if let Some(r) = cache.cache.get(&op) {
        return r;
    }

    let (this_val, lo, hi) = set.get().expect("Invalid index");
    let w = f(&this_val);

    let PivotedSets {
        lower,
        mut higher_or_equal,
    } = values.pivot(&this_val);

    let set = if let Some(top) = higher_or_equal.first() {
        if top > this_val {
            if let Some(hi_budget) = budget.checked_sub(w) {
                let (lo, hi) = holder.pools().join(
                    || extend_as_superset_inner(&lo, higher_or_equal.clone(), budget, f, cache),
                    || extend_as_superset_inner(&hi, higher_or_equal.clone(), hi_budget, f, cache),
                );
                holder.get_node(this_val, lo, hi)
            } else {
                let lo = extend_as_superset_inner(&lo, higher_or_equal.clone(), budget, f, cache);
                holder.get_node(this_val, lo, holder.zero())
            }
        } else {
            higher_or_equal.pop_first();
            if let Some(hi_budget) = budget.checked_sub(w) {
                // top must be equal since we've checked if it was smaller or bigger.
                let (lo, (cheap_lo, hi)) = holder.pools().join(
                    || extend_as_superset_inner(&lo, higher_or_equal.clone(), budget, f, cache),
                    || {
                        holder.pools().join(
                            || {
                                extend_as_superset_inner(
                                    &lo,
                                    higher_or_equal.clone(),
                                    hi_budget,
                                    f,
                                    cache,
                                )
                            },
                            || {
                                extend_as_superset_inner(
                                    &hi,
                                    higher_or_equal.clone(),
                                    hi_budget,
                                    f,
                                    cache,
                                )
                            },
                        )
                    },
                );
                holder.get_node(this_val, lo.clone(), hi.union(cheap_lo))
            } else {
                let lo = extend_as_superset_inner(&lo, higher_or_equal.clone(), budget, f, cache);
                holder.get_node(this_val, lo.clone(), holder.zero())
            }
        }
    } else {
        //if there are no more values to add, we just return the set itself
        set.clone()
            .max_weight_of_inner(budget, f, cache.id, &cache.max_weight_cache)
    };

    //Add all possible subsets that are smaller to the set.
    let r = add_all_subsets_bounded(set, lower, budget, f, cache);
    cache.cache.insert(op, r)
}

///Adds all subsets from `values` to `set`, assuming that all members of `values` are lower than all
///members of `values`.
fn add_all_subsets_bounded<'a, V, F>(
    set: SetFamily<'a, V>,
    mut values: SingleSet<'a, V>,
    budget: usize,
    f: &F,
    cache: &SubsetCoverCache<'a, V>,
) -> SetFamily<'a, V>
where
    F: Fn(&V) -> usize + Send + Sync,
    V: Eq + Hash + Ord + Send + Sync + Clone,
{
    //TODO: Add cache here!
    let holder = set.manager();
    if let Some(v) = values.pop_first() {
        let w = f(&v);
        if let Some(hi_budget) = budget.checked_sub(w) {
            let lo = add_all_subsets_bounded(set.clone(), values.clone(), budget, f, cache);
            let hi = add_all_subsets_bounded(set, values, hi_budget, f, cache);
            holder.get_node(v, lo, hi)
        } else {
            let lo = add_all_subsets_bounded(set, values, budget, f, cache);
            holder.get_node(v, lo, holder.zero())
        }
    } else {
        set.max_weight_of_inner(budget, f, cache.id, &cache.max_weight_cache)
    }
}

#[cfg(test)]
mod test {
    use crate::{ZddHolder, algebra::str_to_sets};

    use super::*;

    #[test]
    fn test_add_all_subsets_bounded() {
        let holder = ZddHolder::new();
        let ops = [("c d ", "ab"), ("", "a"), (" ", "abc"), ("de ef", "abc")];
        let f = |c: &char| (*c as usize) - ('a' as usize) + 1;
        let cache = SubsetCoverCache::new(&holder);

        for (s, ops) in ops {
            for budget in 0..20 {
                let ops = ops.chars().collect::<BTreeSet<_>>();
                let s = str_to_sets(s);
                let mut res = s.clone();
                for op in ops.iter().copied() {
                    res = res
                        .into_iter()
                        .flat_map(|x| {
                            let mut y = x.clone();
                            y.insert(op);
                            [x, y]
                        })
                        .filter(|x| x.iter().map(f).sum::<usize>() <= budget)
                        .collect();
                }

                let s = SetFamily::from_sets(s, &holder);

                let result = SetFamily::from_sets(res, &holder);

                let mut iterative_s = s.clone();
                for op in ops.iter().copied() {
                    iterative_s = iterative_s.insert_as_superset(op);
                }
                iterative_s = iterative_s.max_weight_of(budget, f);
                println!("Extending {s} with {ops:?} with budget = {budget} to make {result}");
                let batched_s = add_all_subsets_bounded(
                    s.max_weight_of(budget, f),
                    holder.single_set(ops),
                    budget,
                    &f,
                    &cache,
                );
                batched_s.check_valid_zdd();
                iterative_s.check_valid_zdd();
                assert_eq!(iterative_s, result);
                assert_eq!(batched_s, result);
            }
        }
    }

    #[test]
    fn test_weighted_extend_subset() {
        let holder = ZddHolder::new();
        let ops = [("ab ", "abcd"), ("", "a"), (" ", "abc"), ("de ef", "abcef")];
        let f = |c: &char| (*c as usize) - ('a' as usize) + 1;
        let cache = SubsetCoverCache::new(&holder);

        for (s, ops) in ops {
            for budget in 0..20 {
                let ops = ops.chars().collect::<BTreeSet<_>>();
                let s = str_to_sets(s);
                let mut res = s.clone();
                for op in ops.iter().copied() {
                    res = res
                        .into_iter()
                        .flat_map(|x| {
                            let mut y = x.clone();
                            y.insert(op);
                            [x, y]
                        })
                        .filter(|x| x.iter().map(f).sum::<usize>() <= budget)
                        .collect();
                }

                let s = SetFamily::from_sets(s, &holder);

                let result = SetFamily::from_sets(res, &holder);

                let mut iterative_s = s.clone();
                for op in ops.iter().copied() {
                    iterative_s = iterative_s.insert_as_superset(op);
                }
                iterative_s = iterative_s.max_weight_of(budget, f);
                println!("Extending {s} with {ops:?} with budget = {budget} to make {result}");
                let batched_s = s.extend_as_superset_with_budget(ops.clone(), budget, f, &cache);
                batched_s.check_valid_zdd();
                if result == batched_s {
                    println!("Success!");
                } else {
                    println!("Failure!");
                }
                assert_eq!(
                    iterative_s, batched_s,
                    "Inserting and extending are not equivalent! {batched_s} != {iterative_s}",
                );
                assert_eq!(
                    batched_s, result,
                    "The set is not what was expected! {batched_s} != {result}"
                );
            }
        }
    }
}
