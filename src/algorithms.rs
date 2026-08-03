//! Defines various miscellaneous algorithms over [`SetFamily`]
//!
//! ## Finding minimal subsets according to summed weights:
//!  - [`SetFamily::min_weight`]
//!  - [`SetFamily::minimal_sets`]

mod max_weight;
mod minimum_cutoff;
mod subset_cover;
use std::hash::Hash;

use num_traits::Num;
pub use subset_cover::subset_cover;

use crate::{
    SetFamily, algorithms::subset_cover::PossiblyPointlessCheckedSub, manager::TempCacheItem,
};

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Given a ZDD, restrict to only the sets of the smallest summed weight, where weights are
    ///defined by the function `f`. Should accept any numeric type (for floats, use
    ///[`OrderedFloat`](ordered_float::OrderedFloat) or [`NotNan`](ordered_float::NotNan) since
    ///floats don't otherwise implement `Ord`
    #[must_use]
    pub fn minimal_sets<F, T>(&self, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T> + PossiblyPointlessCheckedSub,
    {
        let budget = self.min_weight(&f);

        self.clip_weight(budget, f)
    }
}

#[cfg(test)]
mod test {
    use std::collections::{BTreeSet, HashMap};

    use ordered_float::NotNan;
    use rand::{RngExt, SeedableRng, rngs::SmallRng};

    use crate::{SetFamily, ZddHolder, utils::test::random_family};

    const SETS: &str = "ABCD ABCE EFG GH";

    #[expect(clippy::trivially_copy_pass_by_ref)]
    fn char_value(c: &char) -> usize {
        match c {
            'A' | 'C' => 1,
            'B' => 2,
            'D' | 'E' => 4,
            'F' => 50,
            'G' => 0,
            'H' => 45,
            _ => 999,
        }
    }

    #[test]
    fn minimum_cost_test() {
        let lorem_sets = SETS
            .split(' ')
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();

        let n = lorem_sets
            .iter()
            .map(|x| x.iter().map(char_value).sum::<usize>())
            .min()
            .unwrap();
        let mins = lorem_sets
            .iter()
            .filter(|x| x.iter().map(char_value).sum::<usize>() == n)
            .cloned()
            .collect::<BTreeSet<_>>();

        let holder = ZddHolder::new();
        let mins = SetFamily::from_sets(mins, &holder);
        let lorem = SetFamily::from_sets(lorem_sets, &holder);

        assert_eq!(lorem.min_weight(char_value), n);

        let min_sets = lorem.minimal_sets(char_value);
        assert_eq!(min_sets, mins);
    }

    #[test]
    fn minimum_cost_test_other_types() {
        let universe = ['a', 'b', 'c', 'd', 'e', 'f', 'g'];
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..20 {
            let mut fam = random_family(&universe, &mut rng);
            while fam.is_empty() {
                fam = random_family(&universe, &mut rng);
            }

            let weights = universe
                .iter()
                .map(|x| (*x, NotNan::new(rng.random::<f64>() - 0.5).unwrap()))
                .collect::<HashMap<_, NotNan<f64>>>();

            let f = |x: &char| *weights.get(x).unwrap();
            let n: NotNan<f64> = fam
                .iter()
                .map(|x| x.iter().map(f).sum::<NotNan<f64>>())
                .min()
                .unwrap();

            let mins = fam
                .iter()
                .filter(|x| x.iter().map(f).sum::<NotNan<f64>>() == n)
                .cloned()
                .collect::<BTreeSet<_>>();
            println!("{n} {mins:?}");

            let holder = ZddHolder::new();
            let mins = SetFamily::from_sets(mins, &holder);
            let lorem = SetFamily::from_sets(fam, &holder);

            assert_eq!(lorem.min_weight(f), n);

            let min_sets = lorem.minimal_sets(f);
            assert_eq!(min_sets, mins);
        }
    }
}
