use std::{collections::BTreeSet, hash::Hash};

use ahash::RandomState;
use indicatif::ProgressIterator;

use crate::SetFamily;

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

    for set in &sets {
        universe.extend(set.universe::<RandomState>());
    }

    let mut all_possibles = holder.sets_with_exact_weight(universe, 28, f);
    println!("{}", all_possibles.size());
    println!("Preprocessing done!");

    for set in sets.into_iter().progress() {
        all_possibles = all_possibles.has_subset_in(set);
    }
    all_possibles
}
