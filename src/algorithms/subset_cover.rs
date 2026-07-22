use std::{
    collections::{BTreeMap, BTreeSet},
    hash::Hash,
};

use ahash::RandomState;

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

    for set in sets.into_iter().take(2) {
        let elements = set_to_elements.get(&set).unwrap();
        let items_not_in_set = universe.difference(elements).cloned().collect::<Vec<_>>();
        let mut super_set = set.superset();
        println!("extending");
        super_set = super_set.extend_as_superset(items_not_in_set);
        let max_weight = super_set.max_weight(&f);
        //super_set = super_set.extend_as_superset_with_budget(items_not_in_set, budget, &f, &cache);
        println!("done, now intersecting, weight of {max_weight:?}");
        println!(
            "{} node and {} nodes",
            super_set.n_nodes(),
            all_possibles.n_nodes()
        );
        super_set = super_set.max_weight_of(28, &f);
        println!(
            "reduced! {} and {}",
            super_set.n_nodes(),
            all_possibles.n_nodes()
        );
    }

    todo!()
}
