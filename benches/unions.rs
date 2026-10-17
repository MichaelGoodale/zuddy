//! Benchmarks for ZDD unions, in the style of `bench/joins.rs`.
//!
//! `slow_unions` uses large random families which currently take a long time to union.
//! `owned_slow_unions` builds the same large families via [`OwnedZdd`] so specific
//! slow unions found elsewhere can be dropped in as serialized input.

use divan::Bencher;
use rand::{Rng, RngExt, SeedableRng, rngs, seq::IndexedRandom};
use std::collections::BTreeSet;
use zuddy::serialize::OwnedZdd;
use zuddy::{SetFamily, ZddHolder};

fn random_family(universe: &[char], rng: &mut impl Rng, n_sets: std::ops::Range<usize>) -> BTreeSet<BTreeSet<char>> {
    let n_sets = rng.random_range(n_sets);
    let mut sets = BTreeSet::new();
    for _ in 0..n_sets {
        let size = rng.random_range(0..universe.len());
        let set = universe.sample(rng, size).copied().collect::<BTreeSet<_>>();
        sets.insert(set);
    }
    sets
}

fn pairs<'a>(
    holder: &'a ZddHolder<char>,
    universe: &[char],
    rng: &mut impl Rng,
    n_pairs: usize,
    n_sets: std::ops::Range<usize>,
) -> Vec<(SetFamily<'a, char>, SetFamily<'a, char>)> {
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

///Pairs of (`OwnedZdd`, `OwnedZdd`) so unions on serialized ZDDs can be benchmarked.
fn owned_pairs(universe: &[char], rng: &mut impl Rng) -> Vec<(OwnedZdd<char>, OwnedZdd<char>)> {
    let holder = ZddHolder::new();
    let mut v = vec![];
    for _ in 0..20 {
        let family_a = random_family(universe, rng, 40..200);
        let family_b = random_family(universe, rng, 40..200);
        v.push((
            SetFamily::from_sets(family_a, &holder).to_owned_zdd(),
            SetFamily::from_sets(family_b, &holder).to_owned_zdd(),
        ));
    }
    v
}

fn main() {
    divan::main();
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

///Unions of large families which currently take a long time.
#[divan::bench()]
fn slow_unions(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
        'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let sets = pairs(&holder, &universe, &mut rng, 20, 40..200);
    bencher.bench_local(|| {
        for (a, b) in sets.iter().cloned() {
            let _ = a.union(b);
            holder.clear_cache();
        }
    });
}

///Same as `slow_unions`, but the ZDDs are provided as [`OwnedZdd`]s, matching how
/// specific slow unions found elsewhere can be loaded into the benchmark.
#[divan::bench()]
fn owned_slow_unions(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
        'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);
    let pairs = owned_pairs(&universe, &mut rng);
    bencher.bench_local(|| {
        for (a, b) in pairs.iter().cloned() {
            let a = a.to_set_family(&holder);
            let b = b.to_set_family(&holder);
            let _ = a.union(b);
            holder.clear_cache();
        }
    });
}
