//! Benchmarks for zuddy using 8-queens as an example problem
use divan::Bencher;
use zuddy::{SetFamily, ZddHolder};

use std::collections::{BTreeSet, HashMap};

use rand::{Rng, RngExt, SeedableRng, rngs, seq::IndexedRandom};

pub fn random_weights(universe: &[char], rng: &mut impl Rng) -> HashMap<char, usize> {
    universe
        .iter()
        .map(|x| (*x, rng.random_range(0..3)))
        .collect()
}

pub fn random_family(universe: &[char], rng: &mut impl Rng) -> BTreeSet<BTreeSet<char>> {
    let n_sets = rng.random_range(0..30);
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

fn sets<'a>(
    holder: &'a ZddHolder<char>,
    universe: &[char],
    rng: &mut impl Rng,
) -> Vec<(SetFamily<'a, char>, SetFamily<'a, char>, usize)> {
    let mut v = vec![];
    for _ in 0..100 {
        let family_a = random_family(universe, rng);
        let family_b = random_family(universe, rng);
        let budget: u8 = rng.random_range(0..20);

        v.push((
            SetFamily::from_sets(family_a, holder),
            SetFamily::from_sets(family_b, holder),
            budget.into(),
        ));
    }

    v
}
fn clip_sets<'a>(
    holder: &'a ZddHolder<char>,
    universe: &[char],
    rng: &mut impl Rng,
) -> Vec<(SetFamily<'a, char>, usize)> {
    let mut v = vec![];
    for _ in 0..100 {
        let family_a = random_family(universe, rng);
        let budget: u8 = rng.random_range(0..10);

        v.push((SetFamily::from_sets(family_a, holder), budget.into()));
    }

    v
}

#[divan::bench()]
fn clip(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);

    let sets = clip_sets(&holder, &universe, &mut rng);
    let weights = random_weights(&universe, &mut rng);
    let f = |c: &char| *weights.get(c).unwrap() as isize;
    bencher.bench_local(|| {
        for (a, budget) in sets.iter().cloned() {
            a.clip_weight(budget as isize, f);
            holder.clear_cache();
        }
    });
}

#[divan::bench()]
fn clip_usize(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);

    let sets = clip_sets(&holder, &universe, &mut rng);
    let weights = random_weights(&universe, &mut rng);
    let f = |c: &char| *weights.get(c).unwrap();
    bencher.bench_local(|| {
        for (a, budget) in sets.iter().cloned() {
            a.clip_weight_usize(budget, f);
            holder.clear_cache();
        }
    });
}

#[divan::bench()]
fn join(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);

    let sets = sets(&holder, &universe, &mut rng);
    let weights = random_weights(&universe, &mut rng);
    bencher.bench_local(|| {
        for (a, b, _) in sets.iter().cloned() {
            let c = a.join(b);
            holder.clear_cache();
        }
    });
}

#[divan::bench()]
fn join_and_clip(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);

    let sets = sets(&holder, &universe, &mut rng);
    let weights = random_weights(&universe, &mut rng);
    let f = |c: &char| *weights.get(c).unwrap();
    bencher.bench_local(|| {
        for (a, b, budget) in sets.iter().cloned() {
            let c = a.join(b).clip_weight_usize(budget, f);
            holder.clear_cache();
        }
    });
}

#[divan::bench()]
fn join_clip_fused(bencher: Bencher) {
    let holder = ZddHolder::new();
    let universe = [
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p',
    ];
    let mut rng = rngs::SmallRng::seed_from_u64(32);

    let sets = sets(&holder, &universe, &mut rng);
    let weights = random_weights(&universe, &mut rng);
    let f = |c: &char| *weights.get(c).unwrap();
    bencher.bench_local(|| {
        for (a, b, budget) in sets.iter().cloned() {
            let c = a.bounded_join(b, f, budget);
            holder.clear_cache();
        }
    });
}
