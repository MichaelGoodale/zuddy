use std::hash::Hash;

use rand::Rng;
use rand::prelude::*;

use crate::{SetFamily, ZddHolder};

enum EdgeType {
    Lo,
    Hi,
}

fn choose_lo_or_hi<V: Hash + Eq + Ord + Clone>(
    lo: SetFamily<V>,
    hi: SetFamily<V>,
    rng: &mut impl Rng,
    holder: &mut ZddHolder<V>,
) -> EdgeType {
    let hi_c = hi.size(holder).unwrap();
    let lo_c = lo.size(holder).unwrap();
    if lo_c == 0 {
        EdgeType::Hi
    } else if hi_c == 0 {
        EdgeType::Lo
    } else {
        let total = hi_c + lo_c;
        #[expect(clippy::cast_precision_loss)]
        if rng.random_bool(hi_c as f64 / total as f64) {
            EdgeType::Hi
        } else {
            EdgeType::Lo
        }
    }
}

impl<V: Hash + Eq + Ord + Clone> SetFamily<V> {
    ///Randomly samples from the [`SetFamily`] according to a uniform distribution.
    ///
    ///# Panics
    /// - If trying to sample from an empty family.
    /// - May panic if the number of possible paths is too large to be represented as a usize
    pub fn sample(mut self: SetFamily<V>, rng: &mut impl Rng, holder: &mut ZddHolder<V>) -> Vec<V> {
        assert!(!self.is_zero(), "Cannot sample from the empty set!");
        let mut path = vec![];
        while !self.is_zero() && !self.is_one() {
            let (_, lo, hi) = self.get(holder).unwrap();
            match choose_lo_or_hi(lo, hi, rng, holder) {
                EdgeType::Lo => self = lo,
                EdgeType::Hi => {
                    path.push(self.get(holder).unwrap().0.clone());
                    self = hi;
                }
            }
        }
        path
    }
}

#[cfg(test)]
mod test {
    use rand::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};

    use crate::{SetFamily, ZddHolder};

    #[expect(clippy::cast_precision_loss)]
    fn chi_squared_uniform(counts: &[usize]) -> bool {
        let n: usize = counts.iter().sum();
        let k: usize = counts.len();
        let expected: f64 = n as f64 / k as f64;

        let chi_sq: f64 = counts
            .iter()
            .map(|&obs| {
                let diff = obs as f64 - expected;
                (diff * diff) / expected
            })
            .sum();

        // Critical value for α=0.05, df=k-1 (for k=10, critical=16.92)
        let critical = if k == 1 {
            0.0
        } else if k <= 5 {
            11.07
        } else if k <= 10 {
            16.92
        } else {
            31.41
        };

        chi_sq < critical
    }

    #[test]
    fn minimum_cost_test() {
        let sets = "A B C AB BC ABC"
            .split(' ')
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();

        let mut sample_counts = sets
            .iter()
            .cloned()
            .map(|x| (x, 0))
            .collect::<BTreeMap<_, _>>();

        let mut holder = ZddHolder::new();
        let set = SetFamily::from_sets(sets, &mut holder);
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        for _ in 0..1000 {
            let sample = set
                .sample(&mut rng, &mut holder)
                .into_iter()
                .collect::<BTreeSet<_>>();
            *sample_counts.get_mut(&sample).unwrap() += 1;
        }
        println!("{sample_counts:?}");
        assert_eq!(sample_counts.values().sum::<usize>(), 1000);
        let mut counts = sample_counts.values().copied().collect::<Vec<_>>();
        assert!(chi_squared_uniform(&counts));
        counts[0] += 300;
        assert!(!chi_squared_uniform(&counts));
    }
}
