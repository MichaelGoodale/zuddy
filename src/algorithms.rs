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

use crate::Zdd;

use super::{SetFamily, ZddHolder};

enum OptimizationFrame<V> {
    Search(SetFamily<V>),
    Climb(SetFamily<V>),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
enum UsizeOrPositiveInfinity {
    Size(usize),
    PositiveInfinity,
}

impl From<UsizeOrPositiveInfinity> for Option<usize> {
    fn from(value: UsizeOrPositiveInfinity) -> Self {
        match value {
            UsizeOrPositiveInfinity::Size(x) => Some(x),
            UsizeOrPositiveInfinity::PositiveInfinity => None,
        }
    }
}

impl UsizeOrPositiveInfinity {
    fn add(self, x: usize) -> Self {
        match self {
            UsizeOrPositiveInfinity::Size(s) => UsizeOrPositiveInfinity::Size(x + s),
            UsizeOrPositiveInfinity::PositiveInfinity => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }
}

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

impl<V> SetFamily<V> {
    fn minimal_set_inner<F: Fn(&V) -> usize>(
        self,
        f: F,
        holder: &ZddHolder<V>,
    ) -> HashMap<SetFamily<V>, MinWeightCost> {
        let mut minimum_cost: HashMap<SetFamily<V>, MinWeightCost> = HashMap::default();
        //None here is semantically positive infinity
        minimum_cost.insert(SetFamily::ZERO, MinWeightCost::INFINITY);
        minimum_cost.insert(SetFamily::ONE, MinWeightCost::ZERO);
        let mut stack = vec![OptimizationFrame::Search(self)];

        while let Some(x) = stack.pop() {
            match x {
                OptimizationFrame::Search(this) => {
                    let (_, lo, hi) = this.get(holder).unwrap();
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
                    let (v, lo, hi) = this.get(holder).unwrap();
                    let lo_w = minimum_cost.get(&lo).unwrap().min_set_weight;

                    let element_weight = f(v);
                    let hi_w = minimum_cost
                        .get(&hi)
                        .unwrap()
                        .min_set_weight
                        .add(element_weight);

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
    pub fn minimal_set_size<F: Fn(&V) -> usize>(
        &self,
        f: F,
        holder: &ZddHolder<V>,
    ) -> Option<usize> {
        if self.is_zero() {
            return None;
        }

        if self.is_one() {
            return Some(0);
        }

        let minimum_cost = self.minimal_set_inner(f, holder);

        minimum_cost.get(self).unwrap().min_set_weight.into()
    }

    /// Returns a [`MinimalSetIterator`] which iterates over the minimal sets by summed weight of the family.
    /// The weight is calculated by the provided closure.
    ///
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    pub fn minimal_sets<F: Fn(&V) -> usize>(
        self,
        f: F,
        holder: &ZddHolder<V>,
    ) -> MinimalSetIterator<'_, V> {
        let minimum_cost_lookup = self.minimal_set_inner(f, holder);
        let min_cost = minimum_cost_lookup.get(&self).unwrap().min_set_weight;

        MinimalSetIterator {
            stack: vec![(self, vec![])],
            minimum_cost_lookup,
            holder,
            min_cost,
        }
    }
}

impl<V: Eq + Hash + Clone> SetFamily<V> {
    /// Returns a [`SetFamily`] consisting only of sets with the smallest possible summed weight.
    /// The weight is calculated by the provided closure.
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    #[must_use]
    pub fn only_minimal_sets<F: Fn(&V) -> usize>(
        self,
        f: F,
        holder: &mut ZddHolder<V>,
    ) -> SetFamily<V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let min_cost_lookup = self.minimal_set_inner(f, holder);
        let overall_min = min_cost_lookup.get(&self).unwrap().min_set_weight;
        self.only_minimal_sets_inner(holder, overall_min, &min_cost_lookup)
    }

    fn only_minimal_sets_inner(
        self,
        holder: &mut ZddHolder<V>,
        overall_min: UsizeOrPositiveInfinity,
        min_cost_lookup: &HashMap<SetFamily<V>, MinWeightCost>,
    ) -> SetFamily<V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let (v, lo, hi) = self.get(holder).unwrap();
        let element_weight = min_cost_lookup.get(&self).unwrap().element_weight;

        let lo_w = min_cost_lookup.get(&lo).unwrap().min_set_weight;
        let hi_w = min_cost_lookup
            .get(&hi)
            .unwrap()
            .min_set_weight
            .add(element_weight);

        match (lo_w <= overall_min, hi_w <= overall_min) {
            (true, true) => {
                let z = Zdd {
                    value: v.clone(),
                    lo: lo.only_minimal_sets_inner(holder, overall_min, min_cost_lookup),
                    hi: hi.only_minimal_sets_inner(holder, overall_min, min_cost_lookup),
                };
                holder.get_node(z)
            }
            (false, true) => {
                let z = Zdd {
                    value: v.clone(),
                    lo: SetFamily::ZERO,
                    hi: hi.only_minimal_sets_inner(holder, overall_min, min_cost_lookup),
                };
                holder.get_node(z)
            }
            (true, false) => lo.only_minimal_sets_inner(holder, overall_min, min_cost_lookup),
            (false, false) => SetFamily::ZERO,
        }
    }
}

///Iterates over all sets that are minimal by weight.
///
///See [`SetFamily::only_minimal_sets`]
pub struct MinimalSetIterator<'a, V> {
    stack: Vec<(SetFamily<V>, Vec<V>)>,
    holder: &'a ZddHolder<V>,
    minimum_cost_lookup: HashMap<SetFamily<V>, MinWeightCost>,
    min_cost: UsizeOrPositiveInfinity,
}

impl<V> MinimalSetIterator<'_, V> {
    ///Find the minimal cost of all sets.
    #[must_use]
    pub fn min_cost(&self) -> Option<usize> {
        self.min_cost.into()
    }
}

impl<V: Clone + Debug> Iterator for MinimalSetIterator<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.min_cost == UsizeOrPositiveInfinity::PositiveInfinity {
            return None;
        }

        while let Some((this, mut path)) = self.stack.pop() {
            if this.is_zero() {
                continue;
            }
            if this.is_one() {
                return Some(path);
            }

            let (v, lo, hi) = this.get(self.holder).unwrap();
            let element_weight = self.minimum_cost_lookup.get(&this).unwrap().element_weight;
            let lo_w = self.minimum_cost_lookup.get(&lo).unwrap().min_set_weight;
            let hi_w = self
                .minimum_cost_lookup
                .get(&hi)
                .unwrap()
                .min_set_weight
                .add(element_weight);

            match (lo_w <= self.min_cost, hi_w <= self.min_cost) {
                (true, true) => {
                    self.stack.push((lo, path.clone()));
                    path.push(v.clone());
                    self.stack.push((hi, path));
                }
                (false, true) => {
                    path.push(v.clone());
                    self.stack.push((hi, path));
                }
                (true, false) => self.stack.push((lo, path)),
                (false, false) => (),
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

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
        assert!(
            UsizeOrPositiveInfinity::PositiveInfinity == UsizeOrPositiveInfinity::PositiveInfinity
        );
        assert!(UsizeOrPositiveInfinity::Size(3) > UsizeOrPositiveInfinity::Size(0));
        assert!(UsizeOrPositiveInfinity::Size(0) == UsizeOrPositiveInfinity::Size(0));
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

        let mut holder = ZddHolder::new();
        let lorem = SetFamily::from_sets(lorem_sets, &mut holder);

        assert_eq!(lorem.minimal_set_size(char_value, &holder).unwrap(), n);

        let min_sets = lorem.minimal_sets(char_value, &holder);
        println!(
            "{}",
            lorem.graphviz_with_extra(&min_sets.minimum_cost_lookup, &holder)
        );
        let sets = min_sets
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(sets, mins);

        let restricted_lorem = lorem
            .only_minimal_sets(char_value, &mut holder)
            .members(&holder)
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(restricted_lorem, mins);
    }
}
