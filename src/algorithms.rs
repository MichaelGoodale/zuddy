//! Defines various miscellaneous algorithms over [`SetFamily`]
//!
//! ## Finding minimal subsets according to summed weights:
//!  - [`SetFamily::minimal_set_size`]
//!  - [`SetFamily::minimal_sets`]
//!  - [`SetFamily::only_minimal_sets`]
use std::{
    cmp::Ordering::{Equal, Greater, Less},
    collections::HashMap,
    hash::Hash,
};

use crate::Zdd;

use super::{SetFamily, ZddHolder};

enum OptimizationFrame<V> {
    Search(SetFamily<V>),
    Climb(SetFamily<V>),
}

impl<V> SetFamily<V> {
    fn minimal_set_inner<F: Fn(&V) -> usize>(
        self,
        f: F,
        holder: &ZddHolder<V>,
    ) -> HashMap<SetFamily<V>, Option<usize>> {
        let mut minimum_cost: HashMap<SetFamily<V>, Option<usize>> = HashMap::default();
        //None here is semantically positive infinity
        minimum_cost.insert(SetFamily::ZERO, None);
        minimum_cost.insert(SetFamily::ONE, Some(0));
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
                    let lo_w = *minimum_cost.get(&lo).unwrap();
                    let hi_w = minimum_cost.get(&hi).unwrap().map(|w| w + f(v));

                    let v = match (hi_w, lo_w) {
                        (None, None) => None,
                        (None, Some(x)) | (Some(x), None) => Some(x),
                        (Some(x), Some(y)) => Some(x.min(y)),
                    };

                    minimum_cost.insert(this, v);
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

        *minimum_cost.get(self).unwrap()
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
        let min_cost = *minimum_cost_lookup.get(&self).unwrap();

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
        let min_cost_lookup = self.minimal_set_inner(f, holder);
        self.only_minimal_sets_inner(holder, &min_cost_lookup)
    }

    fn only_minimal_sets_inner(
        self,
        holder: &mut ZddHolder<V>,
        min_cost_lookup: &HashMap<SetFamily<V>, Option<usize>>,
    ) -> SetFamily<V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let (v, lo, hi) = self.get(holder).unwrap();
        let lo_w = min_cost_lookup.get(&lo).unwrap();
        let hi_w = min_cost_lookup.get(&hi).unwrap();
        match (lo_w, hi_w) {
            (None, None) => panic!("This means that hi is 0 somewhere and the ZDD is invalid"),
            (None, Some(_)) => {
                let z = Zdd {
                    value: v.clone(),
                    lo: SetFamily::ZERO,
                    hi: hi.only_minimal_sets_inner(holder, min_cost_lookup),
                };
                holder.get_node(z)
            }
            (Some(_), None) => lo.only_minimal_sets_inner(holder, min_cost_lookup),
            (Some(lo_w), Some(hi_w)) => match hi_w.cmp(lo_w) {
                Less => {
                    let z = Zdd {
                        value: v.clone(),
                        lo: SetFamily::ZERO,
                        hi: hi.only_minimal_sets_inner(holder, min_cost_lookup),
                    };
                    holder.get_node(z)
                }
                Greater => lo.only_minimal_sets_inner(holder, min_cost_lookup),
                Equal => {
                    let z = Zdd {
                        value: v.clone(),
                        lo: lo.only_minimal_sets_inner(holder, min_cost_lookup),
                        hi: hi.only_minimal_sets_inner(holder, min_cost_lookup),
                    };

                    holder.get_node(z)
                }
            },
        }
    }
}

///Iterates over all sets that are minimal by weight.
///
///See [`SetFamily::only_minimal_sets`]
pub struct MinimalSetIterator<'a, V> {
    stack: Vec<(SetFamily<V>, Vec<V>)>,
    holder: &'a ZddHolder<V>,
    minimum_cost_lookup: HashMap<SetFamily<V>, Option<usize>>,
    min_cost: Option<usize>,
}

impl<V> MinimalSetIterator<'_, V> {
    ///Find the minimal cost of all sets.
    #[must_use]
    pub fn min_cost(&self) -> Option<usize> {
        self.min_cost
    }
}

impl<V: Clone> Iterator for MinimalSetIterator<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((this, mut path)) = self.stack.pop() {
            if this.is_zero() {
                continue;
            }
            if this.is_one() {
                return Some(path);
            }

            let (v, lo, hi) = this.get(self.holder).unwrap();
            let lo_w = self.minimum_cost_lookup.get(&lo).unwrap();
            let hi_w = self.minimum_cost_lookup.get(&hi).unwrap();
            match (lo_w, hi_w) {
                (None, None) => (),
                (None, Some(_)) => {
                    path.push(v.clone());
                    self.stack.push((hi, path));
                }
                (Some(_), None) => {
                    self.stack.push((lo, path));
                }
                (Some(lo_w), Some(hi_w)) => match hi_w.cmp(lo_w) {
                    Less => {
                        path.push(v.clone());
                        self.stack.push((hi, path));
                    }
                    Greater => {
                        self.stack.push((lo, path));
                    }
                    Equal => {
                        self.stack.push((lo, path.clone()));
                        path.push(v.clone());
                        self.stack.push((hi, path));
                    }
                },
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use super::*;

    const LOREM: &str = "LOREM IPSUM DOLOR SIT AMET CONSECTETUR ADIPISCING ELIT";

    #[expect(clippy::trivially_copy_pass_by_ref)]
    fn alphabet_pos(c: &char) -> usize {
        usize::from(*c as u8 - b'A' + 1)
    }

    #[test]
    fn minimum_cost_test() {
        let lorem_sets = LOREM
            .split(' ')
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();

        let (n, smallest_word) = lorem_sets
            .iter()
            .map(|x| {
                (
                    x.iter().map(alphabet_pos).sum::<usize>(),
                    x.iter().collect::<String>(),
                )
            })
            .min()
            .unwrap();

        let mut holder = ZddHolder::new();
        let lorem = SetFamily::from_sets(lorem_sets, &mut holder);

        assert_eq!(lorem.minimal_set_size(alphabet_pos, &holder).unwrap(), n);

        let sets = lorem
            .minimal_sets(alphabet_pos, &holder)
            .map(|x| {
                x.into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(sets, vec![smallest_word.clone()]);

        let restricted_lorem = lorem
            .only_minimal_sets(alphabet_pos, &mut holder)
            .members(&holder)
            .map(|x| {
                x.into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(restricted_lorem, vec![smallest_word]);
    }
}
