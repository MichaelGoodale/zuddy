//! Benchmarks for unions of ZDDs in Zuddy

use divan::Bencher;
use rand::{Rng, RngExt, SeedableRng, rngs, seq::IndexedRandom};
use std::{collections::BTreeSet, hash::Hash};
use zuddy::{SetFamily, ZddHolder};

fn random_family<T: Copy + Ord>(
    universe: &[T],
    rng: &mut impl Rng,
    n_sets: std::ops::Range<usize>,
) -> BTreeSet<BTreeSet<T>> {
    let n_sets = rng.random_range(n_sets);
    let mut sets = BTreeSet::new();
    for _ in 0..n_sets {
        let size = rng.random_range(0..universe.len());
        let set = universe.sample(rng, size).copied().collect::<BTreeSet<_>>();
        sets.insert(set);
    }
    sets
}

fn main() {
    divan::main();
}

fn pairs<'a, T: Eq + Copy + Ord + Hash + Send + Sync>(
    holder: &'a ZddHolder<T>,
    universe: &[T],
    rng: &mut impl Rng,
    n_pairs: usize,
    n_sets: std::ops::Range<usize>,
) -> Vec<(SetFamily<'a, T>, SetFamily<'a, T>)> {
    let mut v = vec![];
    for _ in 0..n_pairs {
        let family_a = random_family(universe, rng, n_sets.clone());
        let family_b = random_family(universe, rng, n_sets.clone());
        v.push((
            SetFamily::from_sets(family_a, holder),
            SetFamily::from_sets(family_b, holder),
        ));
    }
    v
}

#[divan::bench()]
fn union(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let sets = pairs(&holder, &universe, &mut rng, 100, 0..30);
    bencher.bench_local(|| {
        for (a, b) in sets.iter().cloned() {
            let _ = a.union(b);
            holder.clear_cache();
        }
    });
}

#[divan::bench()]
fn slow_unions(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = (0..200).collect::<Vec<_>>();
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let sets = pairs(&holder, &universe, &mut rng, 20, 200..201);
    bencher.bench_local(|| {
        for (a, b) in sets.iter().cloned() {
            let _ = a.union(b);
            holder.clear_cache();
        }
    });
}
