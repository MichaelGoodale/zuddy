//! Benchmarks for various utility operations in Zuddy (e.g. constructing ZDDs
//! from families of sets via `SetFamily::from_sets`).

use divan::Bencher;
use rand::{Rng, RngExt, SeedableRng, rngs, seq::IndexedRandom};
use std::collections::BTreeSet;
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

fn families<T: Copy + Ord>(
    universe: &[T],
    rng: &mut impl Rng,
    n: usize,
    n_sets: std::ops::Range<usize>,
) -> Vec<BTreeSet<BTreeSet<T>>> {
    (0..n)
        .map(|_| random_family(universe, rng, n_sets.clone()))
        .collect()
}

fn main() {
    divan::main();
}

#[divan::bench()]
fn from_sets(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let families = families(&universe, &mut rng, 100, 0..30);
    bencher.bench_local(|| {
        for family in &families {
            let _ = SetFamily::from_sets(family.clone(), &holder);
        }
    });
}

///`from_sets` over large families with a large universe, which currently
///overflows the stack of rayon worker threads due to unbounded recursion.
#[divan::bench()]
fn slow_from_sets(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = (0..1000).collect::<Vec<_>>();
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let families = families(&universe, &mut rng, 20, 1000..2000);
    bencher.bench_local(|| {
        for family in &families {
            let _ = SetFamily::from_sets(family.clone(), &holder);
        }
    });
}
