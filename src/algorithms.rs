//! Defines various miscellaneous algorithms over [`SetFamily`]
//!
//! ## Finding minimal subsets according to summed weights:
//!  - [`SetFamily::minimal_set_size`]
//!  - [`SetFamily::minimal_sets`]
//!  - [`SetFamily::only_minimal_sets`]
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
};

use crate::{SetFamily, manager::ZddIndex};

mod utils;

mod max_weight;
mod minimum_cutoff;
mod subset_cover;
pub use subset_cover::subset_cover;

pub use utils::UsizeOrPositiveInfinity;

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
struct MinWeightCost {
    min_set_weight: UsizeOrPositiveInfinity,
    element_weight: usize,
}

impl Display for MinWeightCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "f(e)={}, min_set_weight=", self.element_weight)?;
        match self.min_set_weight {
            UsizeOrPositiveInfinity::Size(x) => write!(f, "{x}"),
            UsizeOrPositiveInfinity::PositiveInfinity => write!(f, "+∞"),
        }
    }
}

impl MinWeightCost {
    const INFINITY: Self = MinWeightCost {
        min_set_weight: UsizeOrPositiveInfinity::PositiveInfinity,
        element_weight: 0,
    };
    const ZERO: Self = MinWeightCost {
        min_set_weight: UsizeOrPositiveInfinity::Size(0),
        element_weight: 0,
    };
}

enum OptimizationFrame<V> {
    Search(ZddIndex<V>),
    Climb(ZddIndex<V>),
}

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    fn minimal_set_inner<F: Fn(&V) -> usize>(&self, f: F) -> HashMap<ZddIndex<V>, MinWeightCost> {
        let mut minimum_cost: HashMap<ZddIndex<V>, MinWeightCost> = HashMap::default();
        //None here is semantically positive infinity
        minimum_cost.insert(ZddIndex::ZERO, MinWeightCost::INFINITY);
        minimum_cost.insert(ZddIndex::ONE, MinWeightCost::ZERO);
        let mut stack = vec![OptimizationFrame::Search(self.as_raw())];

        while let Some(x) = stack.pop() {
            match x {
                OptimizationFrame::Search(this) => {
                    let (lo, hi) = this.children(self.manager).unwrap();
                    stack.push(OptimizationFrame::Climb(this));
                    stack.extend([lo, hi].into_iter().filter_map(|k| {
                        if minimum_cost.contains_key(&k) {
                            None
                        } else {
                            Some(OptimizationFrame::Search(k))
                        }
                    }));
                }
                OptimizationFrame::Climb(this) => {
                    let (v, lo, hi) = this.get(self.manager).unwrap();
                    let lo_w = minimum_cost.get(&lo).unwrap().min_set_weight;

                    let element_weight = f(&v);
                    let hi_w = minimum_cost
                        .get(&hi)
                        .unwrap()
                        .min_set_weight
                        .add_usize(element_weight);

                    let min_set_weight = hi_w.min(lo_w);

                    minimum_cost.insert(
                        this,
                        MinWeightCost {
                            min_set_weight,
                            element_weight,
                        },
                    );
                }
            }
        }
        minimum_cost
    }

    /// Gets the size of the smallest set by summed weights where node values are weighted by the closure in `f`.
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    pub fn minimal_set_size<F: Fn(&V) -> usize>(&self, f: F) -> Option<usize> {
        if self.is_zero() {
            return None;
        }

        if self.is_one() {
            return Some(0);
        }

        let minimum_cost = self.minimal_set_inner(f);

        minimum_cost
            .get(&self.as_raw())
            .unwrap()
            .min_set_weight
            .into()
    }

    /// Returns a [`MinimalSetIterator`] which iterates over the minimal sets by summed weight of the family.
    /// The weight is calculated by the provided closure.
    ///
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    pub fn minimal_sets<F: Fn(&V) -> usize>(&self, f: F) -> MinimalSetIterator<'a, V> {
        let minimum_cost_lookup = self.minimal_set_inner(f);
        let min_cost = minimum_cost_lookup
            .get(&self.as_raw())
            .unwrap()
            .min_set_weight;

        MinimalSetIterator {
            stack: vec![(self.as_raw(), (vec![], 0))],
            minimum_cost_lookup,
            root: self.clone(),
            min_cost,
        }
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    /// Returns a [`SetFamily`] consisting only of sets with the smallest possible summed weight.
    /// The weight is calculated by the provided closure.
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    #[must_use]
    pub fn only_minimal_sets<F: Fn(&V) -> usize>(self, f: F) -> SetFamily<'a, V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let min_cost_lookup = self.minimal_set_inner(f);
        let overall_min = min_cost_lookup.get(&self.as_raw()).unwrap().min_set_weight;
        self.only_minimal_sets_inner(0, overall_min, &min_cost_lookup)
    }

    fn only_minimal_sets_inner(
        self,
        current_cost: usize,
        overall_min: UsizeOrPositiveInfinity,
        min_cost_lookup: &HashMap<ZddIndex<V>, MinWeightCost>,
    ) -> SetFamily<'a, V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let (v, lo, hi) = self.get().unwrap();
        let element_weight = min_cost_lookup.get(&self.as_raw()).unwrap().element_weight;

        let lo_w = min_cost_lookup.get(&lo.as_raw()).unwrap().min_set_weight;
        let hi_w = min_cost_lookup.get(&hi.as_raw()).unwrap().min_set_weight;

        match (
            lo_w.add_usize(current_cost) <= overall_min,
            hi_w.add_usize(current_cost) <= overall_min,
        ) {
            (true, true) => self.manager.get_node(
                v.clone(),
                lo.only_minimal_sets_inner(current_cost, overall_min, min_cost_lookup),
                hi.only_minimal_sets_inner(
                    current_cost + element_weight,
                    overall_min,
                    min_cost_lookup,
                ),
            ),
            (false, true) => self.manager.get_node(
                v.clone(),
                self.manager.zero(),
                hi.only_minimal_sets_inner(
                    current_cost + element_weight,
                    overall_min,
                    min_cost_lookup,
                ),
            ),
            (true, false) => lo.only_minimal_sets_inner(current_cost, overall_min, min_cost_lookup),
            (false, false) => self.manager.zero(),
        }
    }
}

///Iterates over all sets that are minimal by weight.
///
///See [`SetFamily::only_minimal_sets`]
pub struct MinimalSetIterator<'a, V: Eq + Hash> {
    #[expect(clippy::type_complexity)]
    stack: Vec<(ZddIndex<V>, (Vec<V>, usize))>,
    root: SetFamily<'a, V>,
    minimum_cost_lookup: HashMap<ZddIndex<V>, MinWeightCost>,
    min_cost: UsizeOrPositiveInfinity,
}

impl<V: Eq + Hash> MinimalSetIterator<'_, V> {
    ///Find the minimal cost of all sets.
    #[must_use]
    pub fn min_cost(&self) -> Option<usize> {
        self.min_cost.into()
    }

    #[cfg(test)]
    fn minimum_costs(&self) -> HashMap<SetFamily<'_, V>, MinWeightCost> {
        self.minimum_cost_lookup
            .iter()
            .map(|(x, y)| (SetFamily::from_set_family(*x, self.root.manager), *y))
            .collect()
    }
}

impl<V: Clone + Debug + Eq + Hash> Iterator for MinimalSetIterator<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.min_cost == UsizeOrPositiveInfinity::PositiveInfinity {
            return None;
        }

        while let Some((this, (mut path, current_cost))) = self.stack.pop() {
            if this.is_zero() {
                continue;
            }
            if this.is_one() {
                return Some(path);
            }

            let (v, lo, hi) = this.get(self.root.manager).unwrap();
            let element_weight = self.minimum_cost_lookup.get(&this).unwrap().element_weight;
            let lo_w = self.minimum_cost_lookup.get(&lo).unwrap().min_set_weight;
            let hi_w = self.minimum_cost_lookup.get(&hi).unwrap().min_set_weight;

            match (
                lo_w.add_usize(current_cost) <= self.min_cost,
                hi_w.add_usize(current_cost) <= self.min_cost,
            ) {
                (true, true) => {
                    self.stack.push((lo, (path.clone(), current_cost)));
                    path.push(v.clone());
                    self.stack.push((hi, (path, current_cost + element_weight)));
                }
                (false, true) => {
                    path.push(v.clone());
                    self.stack.push((hi, (path, current_cost + element_weight)));
                }
                (true, false) => self.stack.push((lo, (path, current_cost))),
                (false, false) => (),
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use crate::ZddHolder;

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
            .collect::<Vec<_>>();

        let holder = ZddHolder::new();
        let lorem = SetFamily::from_sets(lorem_sets, &holder);

        assert_eq!(lorem.minimal_set_size(char_value).unwrap(), n);

        let min_sets = lorem.minimal_sets(char_value);
        println!("{}", lorem.graphviz_with_extra(&min_sets.minimum_costs()));
        let sets = min_sets
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(sets, mins);

        let restricted_lorem = lorem
            .only_minimal_sets(char_value)
            .members()
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(restricted_lorem, mins);
    }
}
