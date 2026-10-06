//! Repeated unions causing issue with zdds.
use std::{collections::BTreeSet, env, fs, path::Path};

use indicatif::ProgressIterator;
use serde::{Deserialize, Serialize};
use zuddy::{SetFamily, ZddHolder, algorithms::subset_cover, serialize::to_owned_zdds};

#[derive(Eq, Clone, Copy, PartialOrd, Ord, PartialEq, Debug, Hash, Deserialize, Serialize)]
struct Id(u32);

#[derive(Eq, Clone, Copy, PartialEq, PartialOrd, Ord, Debug, Hash, Deserialize, Serialize)]
struct WeightedId {
    id: Id,
    weight: u8,
}

fn main() -> anyhow::Result<()> {
    let base = env!("CARGO_MANIFEST_DIR");
    let path = Path::new(base).join("examples/resources/treesets.ron");
    let tree_sets: Vec<Vec<BTreeSet<BTreeSet<WeightedId>>>> =
        ron::from_str(fs::read_to_string(path)?.as_str())?;
    let holder = ZddHolder::new();

    let mut zdds = vec![];
    for v in tree_sets.into_iter().progress() {
        let mut big_set = holder.zero();
        for sets in v.into_iter().progress() {
            let sets = SetFamily::from_sets(sets, &holder);
            big_set = big_set.union(sets);
        }
        zdds.push(big_set);
    }
    println!("{:?}", holder.stats());
    let g = to_owned_zdds(zdds.clone());
    let holder = ZddHolder::new();
    let mut zdds = g.to_set_families(&holder).into_iter().collect::<Vec<_>>();
    zdds.sort_by_key(|(a, _)| *a);
    let zdds = zdds.into_iter().map(|(_, x)| x).collect::<Vec<_>>();
    println!("{:?}", holder.stats());

    let all_grammar = subset_cover(&zdds, |x| usize::from(x.weight), None).unwrap();
    let size = all_grammar.min_weight(|x| usize::from(x.weight));

    println!(
        "Done minimising with {} solutions w/ size {size}!",
        all_grammar.size().unwrap(),
    );

    Ok(())
}
