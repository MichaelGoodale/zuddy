//! Zuddy is a crate for handling ZDDs
use std::{fmt::Debug, hash::Hash, marker::PhantomData};

///Defines algebraic manipulations of [`SetFamily`]s.
pub mod algebra;
pub mod algorithms;
///Defines iterators of various kinds over [`SetFamily`]
pub mod iterators;
pub mod manager;
mod utils;

#[cfg(feature = "sampling")]
mod sampling;
use algebra::Operations;

pub use crate::manager::ZddHolder;

///A representation of a family of sets (or otherwise a set of sets).
///
///It is always connected to a particular [`ZddHolder`] which holds the actual memory.
pub struct SetFamily<'a, V: Eq + Hash> {
    id: usize,
    phantom: PhantomData<V>,
    manager: &'a ZddHolder<V>,
}
impl<V: Eq + Hash> Debug for SetFamily<'_, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetFamily").field("id", &self.id).finish()
    }
}

impl<V: Eq + Hash> Hash for SetFamily<'_, V> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.phantom.hash(state);
        std::ptr::from_ref(self.manager).hash(state);
    }
}

impl<V: Eq + Hash> PartialEq for SetFamily<'_, V> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.manager, other.manager)
    }
}

impl<V: Eq + Hash> Eq for SetFamily<'_, V> {}

const ZERO_IDX: usize = 0;
const ONE_IDX: usize = 1;

impl<V: Eq + Hash> SetFamily<'_, V> {
    fn is_zero(&self) -> bool {
        self.id == ZERO_IDX
    }
    fn is_one(&self) -> bool {
        self.id == ONE_IDX
    }
}

#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
///Panics if the ZDD is corrupted
impl<V: Eq + Hash + Ord + Clone> SetFamily<'_, V> {
    fn check_valid_zdd(&self) {
        if self.is_one() || self.is_zero() {
            return;
        }
        let holder = self.manager;
        let mut stack = vec![(self.as_raw(), BTreeSet::from([self.as_raw()]))];

        while let Some((x, mut path)) = stack.pop() {
            let (v, lo, hi) = x.get(holder).unwrap();

            assert!(!hi.is_zero());

            if !lo.is_zero() && !lo.is_one() {
                let (lo_v, _, _) = lo.get(holder).unwrap();
                assert!(lo_v > v);
                let mut path = path.clone();
                let not_already_included = path.insert(lo);
                assert!(not_already_included);
                stack.push((lo, path));
            }

            if !hi.is_one() {
                let (hi_v, _, _) = hi.get(holder).unwrap();
                assert!(hi_v > v);
                let not_already_included = path.insert(hi);
                assert!(not_already_included);
                stack.push((hi, path));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_to_bools(value: usize, num_bits: usize) -> impl Iterator<Item = bool> {
        (0..num_bits).map(move |i| ((value >> i) & 1) == 1)
    }

    fn all_subsets<T: Clone + Ord>(universe: &BTreeSet<T>) -> BTreeSet<BTreeSet<T>> {
        let mut all_sets = BTreeSet::new();
        for i in 0..2_usize.pow(u32::try_from(universe.len()).unwrap()) {
            let set = universe
                .iter()
                .cloned()
                .zip(int_to_bools(i, universe.len()))
                .filter_map(|(a, b)| if b { Some(a) } else { None })
                .collect::<BTreeSet<_>>();
            all_sets.insert(set);
        }
        all_sets
    }

    #[test]
    fn combinations_check() {
        let universe = BTreeSet::from([1, 2, 3]);
        let subsets = all_subsets(&universe);
        let combos_of_subsets = all_subsets(&subsets);
        let holder = ZddHolder::<usize>::default();
        for x in combos_of_subsets {
            let set_zdd = SetFamily::from_sets(x.clone(), &holder);
            set_zdd.check_valid_zdd();
            let reconstructed_set = set_zdd.members().map(|x| x.into_iter().collect()).collect();
            assert_eq!(x, reconstructed_set);
        }
    }
}
