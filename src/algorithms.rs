//! Defines various miscellaneous algorithms over [`SetFamily`]
//!
//! ## Finding minimal subsets according to summed weights:
//!  - [`SetFamily::min_weight`]
//!  - [`SetFamily::minimal_sets`]

mod utils;

mod max_weight;
mod minimum_cutoff;
mod subset_cover;
use std::hash::Hash;

pub use subset_cover::subset_cover;

pub use utils::{IsizeOrInfinity, UsizeOrPositiveInfinity};

use crate::SetFamily;

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SetFamily<'a, V> {
    ///Given a ZDD, restrict to only the sets of the smallest summed weight, where weights are
    ///defined by the function `f`.
    #[must_use]
    pub fn minimal_sets<F>(&self, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let budget = self.min_weight(&f);

        self.clip_weight_usize(budget, f)
    }

    ///Given a ZDD, restrict to only the sets of the smallest summed weight, where weights are
    ///defined by the function `f`.
    #[must_use]
    pub fn minimal_sets_usize<F>(&self, f: F) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let budget = self.min_weight(&f);

        self.clip_weight_usize(budget, f)
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use crate::{SetFamily, ZddHolder};

    use super::*;

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
    fn ordering_of_usize_with_inf() {
        assert!(
            UsizeOrPositiveInfinity::PositiveInfinity > UsizeOrPositiveInfinity::Size(usize::MAX)
        );
        assert_eq!(
            UsizeOrPositiveInfinity::PositiveInfinity,
            UsizeOrPositiveInfinity::PositiveInfinity
        );
        assert!(UsizeOrPositiveInfinity::Size(3) > UsizeOrPositiveInfinity::Size(0));
        assert_eq!(
            UsizeOrPositiveInfinity::Size(0),
            UsizeOrPositiveInfinity::Size(0)
        );
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
}
