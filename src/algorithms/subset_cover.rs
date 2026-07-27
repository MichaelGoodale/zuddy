use std::{
    collections::{BTreeSet, BinaryHeap},
    fmt::Debug,
    hash::Hash,
    time::Instant,
};

use ahash::RandomState;
use indicatif::{ParallelProgressIterator, ProgressIterator, ProgressState, ProgressStyle};
use rayon::prelude::*;

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
    SetFamily, ZddHolder,
    algorithms::{UsizeOrPositiveInfinity, max_weight::MinWeightCache},
    manager::{TempCache, ZddIndex},
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
    budget: usize,
    f: F,
) -> Option<SetFamily<'_, V>>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
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

    let up_sets = sets
        .into_iter()
        .map(SetFamily::superset)
        .progress_with_style(style.clone())
        .collect::<Vec<_>>();

    println!("Done supersetting!");
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
    println!("Joining took {time} seconds.");

    /*
    let start = Instant::now();
    for i in 1..budget {
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
    }*/
    None
}

type SubsetKey<V> = (Vec<ZddIndex<V>>, usize);

fn to_key<V, F>(sets: &[SetFamily<V>], budget: usize, f: &F) -> SubsetKey<V>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
{
    (sets.iter().map(SetFamily::as_raw).collect(), budget)
}

fn mass_intersection<'a, V, F>(
    mut sets: Vec<SetFamily<'a, V>>,
    holder: &'a ZddHolder<V>,
    budget: usize,
    f: &F,
    cache: &TempCache<'a, V, SubsetKey<V>>,
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

    let op = to_key(&sets, budget, f);
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

    let nodes = sets
        .into_iter()
        .map(|x| x.get().unwrap())
        .collect::<Vec<_>>();

    let top = nodes.iter().map(|(x, _, _)| x).min().unwrap().clone();
    let w = f(&top);
    let mut new_lo = nodes
        .iter()
        .map(|(value, lo, hi)| {
            if value == &top {
                lo.clone()
            } else {
                holder.get_node(value.clone(), lo.clone(), hi.clone())
            }
        })
        .collect::<Vec<_>>();
    new_lo.sort_by_key(|x| x.id);
    new_lo.dedup();

    let r = if let Some(hi_budget) = budget.checked_sub(w) {
        let mut new_hi = nodes
            .iter()
            .cloned()
            .map(|(value, lo, hi)| {
                if value == top {
                    let hi_w = hi.clone().min_weight_inner(f, weight_cache);
                    if hi_w > UsizeOrPositiveInfinity::Size(hi_budget) {
                        lo.clone()
                    } else {
                        hi
                        //hi.union(lo.clone())
                    }
                } else {
                    holder.get_node(value, lo, hi)
                }
            })
            .collect::<Vec<_>>();

        new_hi.sort_by_key(|x| x.id);
        new_hi.dedup();

        let (lo, hi) = (
            mass_intersection(new_lo, holder, budget, f, cache, weight_cache),
            mass_intersection(new_hi, holder, hi_budget, f, cache, weight_cache),
        );
        holder.get_node(top, lo, hi)
    } else {
        mass_intersection(new_lo, holder, budget, f, cache, weight_cache)
    };

    cache.insert(op, r)
}
