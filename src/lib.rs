//! Zuddy is a crate for handling ZDDs
use std::{
    collections::{BTreeSet, HashMap},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
};

mod algebra;
///Defines various miscellaneous algorithms over [`SetFamily`]
pub mod algorithms;
///Defines iterators of various kinds over [`SetFamily`]
pub mod iterators;
mod utils;

use algebra::Operations;

///A representation of a family of sets (or otherwise a set of sets).
///
///It is always connected to a particular [`ZddHolder`] which holds the actual memory.
///
///
#[derive(Debug)]
pub struct SetFamily<V>(usize, PhantomData<V>);

impl<V> Copy for SetFamily<V> {}

impl<V> Clone for SetFamily<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V> PartialEq for SetFamily<V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<V> Hash for SetFamily<V> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<V> Eq for SetFamily<V> {}

impl<V> PartialOrd for SetFamily<V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V> Ord for SetFamily<V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V> SetFamily<V> {
    const ZERO: Self = SetFamily(0, PhantomData);
    const ONE: Self = SetFamily(1, PhantomData);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Zdd<V> {
    value: V,
    lo: SetFamily<V>,
    hi: SetFamily<V>,
}

impl<V> SetFamily<V> {
    fn is_zero(self) -> bool {
        self == SetFamily::ZERO
    }

    fn is_one(self) -> bool {
        self == SetFamily::ONE
    }

    fn get(self, holder: &ZddHolder<V>) -> Option<(&V, SetFamily<V>, SetFamily<V>)> {
        holder.data[self.0].as_ref().map(|x| (&x.value, x.lo, x.hi))
    }
}

#[derive(Debug)]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V> {
    free: Vec<usize>,
    data: Vec<Option<Zdd<V>>>,
    uniq_table: HashMap<Zdd<V>, SetFamily<V>>,
    cache: HashMap<Operations<V>, SetFamily<V>>,
    sum_cache: HashMap<SetFamily<V>, Option<usize>>,
}

fn free_id<V>(data: &mut Vec<Option<Zdd<V>>>, free: &mut Vec<usize>) -> SetFamily<V> {
    if let Some(x) = free.pop() {
        SetFamily(x, PhantomData)
    } else {
        data.push(None);
        SetFamily(data.len() - 1, PhantomData)
    }
}

impl<V> Default for ZddHolder<V> {
    fn default() -> Self {
        Self {
            free: vec![],
            data: vec![None, None],
            uniq_table: HashMap::default(),
            sum_cache: HashMap::default(),
            cache: HashMap::default(),
        }
    }
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn new() -> ZddHolder<V> {
        ZddHolder::default()
    }

    fn get_node(&mut self, family: Zdd<V>) -> SetFamily<V> {
        if family.hi == SetFamily::ZERO {
            return family.lo;
        }

        if let Some(x) = self.uniq_table.get(&family) {
            return *x;
        }
        let id = free_id(&mut self.data, &mut self.free);
        self.data[id.0] = Some(family.clone());
        self.uniq_table.insert(family, id);
        id
    }
}

impl<V: Ord + Clone + Hash + Eq> SetFamily<V> {
    ///Creates a [`SetFamily`] from a [`BTreeSet<BTreeSet<V>>`].
    ///
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///let sets = ["abcd", "ac", "a", "bc", "b", "c"];
    ///let x = sets.iter().map(|x| x.chars().collect()).collect();
    ///let z = SetFamily::from_sets(x, &mut holder);
    ///let members: Vec<String> = z.members(&mut holder).map(|x| x.into_iter().collect()).collect();
    ///assert_eq!(members, sets);
    ///```
    pub fn from_sets(mut sets: BTreeSet<BTreeSet<V>>, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if sets.is_empty() {
            return SetFamily::ZERO;
        }

        #[expect(clippy::missing_panics_doc)]
        if sets.len() == 1 && sets.first().unwrap().is_empty() {
            return SetFamily::ONE;
        }

        //fine since at least one set will be non-empty since if it was only the empty set it would have been caught before.
        #[expect(clippy::missing_panics_doc)]
        let value = sets.iter().filter_map(|x| x.first()).min().unwrap().clone();

        let with_min_val = sets
            .extract_if(.., |v| v.contains(&value))
            .map(|mut x| {
                x.remove(&value);
                x
            })
            .collect::<BTreeSet<_>>();

        let without_min_val = sets;

        let lo = SetFamily::from_sets(without_min_val, holder);
        let hi = SetFamily::from_sets(with_min_val, holder);

        holder.get_node(Zdd { value, lo, hi })
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
        let mut holder = ZddHolder::<usize>::default();
        for x in combos_of_subsets {
            let set_zdd = SetFamily::from_sets(x.clone(), &mut holder);
            let reconstructed_set = set_zdd
                .members(&holder)
                .map(|x| x.into_iter().collect())
                .collect();
            assert_eq!(x, reconstructed_set);
        }
    }
}
