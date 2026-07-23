use rayon::prelude::*;
use std::{fmt::Debug, hash::Hash, time::Instant};

use crate::{
    SetFamily, ZddHolder,
    algorithms::{UsizeOrPositiveInfinity, max_weight::MinWeightCache},
    manager::{TempCache, ZddIndex},
};

/// Given sets $S$, with elements weighted by function $f$, returns the zdd
/// such that where $b$ is the budget:
///
/// x = { x | ∀s∈S s⊆x ∧ ∑e∈x f(e) ≤ b }
///
/// # Panics
/// Will panic if `sets` is empty or if the sets don't all share the same manager.
pub fn subset_cover<V, F>(
    mut sets: Vec<SetFamily<'_, V>>,
    budget: usize,
    f: F,
) -> Option<SetFamily<'_, V>>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
{
    assert!(!sets.is_empty(), "Sets cannot be empty!");
    let holder = sets.first().unwrap().manager();
    /*
    let mut universe = BTreeSet::new();

    for set in &sets {
        universe.extend(set.universe::<RandomState>());
    }

    let all_possibles = holder.sets_with_exact_weight(universe, 28, &f);
    println!("{}", all_possibles.size());
    println!("Preprocessing done!");
    */

    let cache = holder.create_temporary_cache();
    let min_weight_cache = holder.create_temporary_cache();
    sets.sort_by_key(|x| x.id);
    sets.dedup();
    let start = Instant::now();
    for i in 0..budget {
        println!("Trying {i}");
        let this_start = Instant::now();
        let x = mass_intersection(sets.clone(), holder, i, &f, &cache, &min_weight_cache);

        let this_op = this_start.elapsed().as_secs_f64();
        let total = start.elapsed().as_secs_f64();
        println!("Took {this_op} seconds, total time {total} seconds");
        if !x.is_zero() {
            println!("Success at {i}");
            return Some(x);
        }
    }
    None
}

fn mass_intersection<'a, V, F>(
    mut sets: Vec<SetFamily<'a, V>>,
    holder: &'a ZddHolder<V>,
    budget: usize,
    f: &F,
    cache: &TempCache<'a, V, (Vec<ZddIndex<V>>, usize)>,
    weight_cache: &MinWeightCache<'a, V>,
) -> SetFamily<'a, V>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
{
    if sets.iter().any(SetFamily::is_zero) {
        return holder.zero();
    }

    sets.retain(|x| !x.is_one());
    if sets.is_empty() {
        return holder.one();
    }

    let op = (sets.iter().map(SetFamily::as_raw).collect(), budget);
    if let Some(r) = cache.get(&op) {
        return r;
    }

    let min_w = sets
        .iter()
        .map(|x| x.clone().min_weight_inner(f, weight_cache).unwrap())
        .collect::<Vec<_>>();

    if min_w.iter().any(|x| x > &budget) {
        return cache.insert(op, holder.zero());
    }

    let (v, lo, hi) = sets
        .into_iter()
        .map(|x| x.get().unwrap())
        .collect::<(Vec<_>, Vec<_>, Vec<_>)>();

    let top = v.iter().min().unwrap().clone();
    let w = f(&top);

    let r = if let Some(hi_budget) = budget.checked_sub(w) {
        let (mut new_lo, mut new_hi) = holder.pools().install(|| {
            v.into_par_iter()
                .zip(lo)
                .zip(hi)
                .map(|((value, lo), hi)| {
                    if value == top {
                        let hi_w = hi.clone().min_weight_inner(f, weight_cache);
                        let hi = if hi_w > UsizeOrPositiveInfinity::Size(hi_budget) {
                            lo.clone()
                        } else {
                            hi.union(lo.clone())
                        };
                        (lo, hi)
                    } else {
                        let x = holder.get_node(value, lo, hi);
                        (x.clone(), x)
                    }
                })
                .collect::<(Vec<_>, Vec<_>)>()
        });

        new_lo.sort_by_key(|x| x.id);
        new_lo.dedup();
        new_hi.sort_by_key(|x| x.id);
        new_hi.dedup();
        let (lo, hi) = (
            mass_intersection(new_lo, holder, budget, f, cache, weight_cache),
            mass_intersection(new_hi, holder, hi_budget, f, cache, weight_cache),
        );
        holder.get_node(top, lo, hi)
    } else {
        let mut new_lo = v
            .into_iter()
            .zip(lo)
            .zip(hi)
            .map(|((value, lo), hi)| {
                if value == top {
                    lo
                } else {
                    holder.get_node(value, lo, hi)
                }
            })
            .collect::<Vec<_>>();
        new_lo.sort_by_key(|x| x.id);
        new_lo.dedup();
        mass_intersection(new_lo, holder, budget, f, cache, weight_cache)
    };

    cache.insert(op, r)
}
