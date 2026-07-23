//! Implementations of basic set theoretic operations over ZDDs.
//!
//! These functions largely come from Minato, 1993 [^minato_93].
//! It also includes the unate cube set algebra from Minato, 1994 [^minato_94]
//! Knuth refers to this as family algebra in exercise 203-206 of Volume 4A 7.1.4 of the Art of Computer Programming.
//!
//! This module defines:
//!
//! ## Element wise modification of sets.
//!   - [`SetFamily::offset`]
//!   - [`SetFamily::onset`]
//!   - [`SetFamily::change`]
//!   - [`SetFamily::element_division`]
//!   - [`SetFamily::element_remainder`]
//! ## Set wise modifications
//!   - [`SetFamily::union`]
//!   - [`SetFamily::intersect`]
//!   - [`SetFamily::difference`]
//!   - [`SetFamily::join`]
//!   - [`SetFamily::divide`]
//!   - [`SetFamily::remainder`]
//!
//! [^minato_93]: S. Minato, "Zero-suppressed BDDS for set manipulation in combinatorial problems". Proceedings of the 30th international on Design automation conference - DAC '93. pp. 272–277. doi:10.1145/157485.164890
//! [^minato_94]: S. Minato, "Calculation of Unate Cube Set Algebra Using Zero-Suppressed BDDs," 31st Design Automation Conference, San Diego, CA, USA, 1994, pp. 420-424, doi: 10.1145/196244.196446.

use crate::{
    ZddHolder,
    manager::TempCache,
    utils::{PivotedSets, SingleSet},
};
use std::collections::BTreeSet;

use crate::{SetFamily, manager::ZddIndex};

use std::{fmt::Debug, hash::Hash};

//TODO: Make this have a constructor that orders fields so that commmutative operations don't get
//doubled.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(super) enum Operations<V> {
    Change(ZddIndex<V>, V),
    Offset(ZddIndex<V>, V),
    Onset(ZddIndex<V>, V),
    Insert(ZddIndex<V>, V),
    InsertSuperset(ZddIndex<V>, V),
    Union(ZddIndex<V>, ZddIndex<V>),
    Intersect(ZddIndex<V>, ZddIndex<V>),
    Difference(ZddIndex<V>, ZddIndex<V>),
    Join(ZddIndex<V>, ZddIndex<V>),
    Division(ZddIndex<V>, ZddIndex<V>),
    NonSup(ZddIndex<V>, ZddIndex<V>),
    Minimal(ZddIndex<V>),
    SubsetOf(ZddIndex<V>, ZddIndex<V>),
    Supersets(ZddIndex<V>),
}

mod unate;
impl<'a, V: Hash + Ord + Eq + Clone + Send + Sync> SetFamily<'a, V> {
    ///Creates a ZDD with all combinations that don't include `value`
    ///
    ///It is defined as `f.offset(x)` = { α | α ∈ f ∧ x ∉ α }
    ///
    ///```
    ///# use std::collections::BTreeSet;
    ///# use zuddy::{ZddHolder, SetFamily};
    /// let mut holder = ZddHolder::<char>::new();
    ///
    /// // Create set family: {a, b}, {a}, {b}
    /// let mut sets = BTreeSet::new();
    /// sets.insert(BTreeSet::from(['a', 'b']));
    /// sets.insert(BTreeSet::from(['a']));
    /// sets.insert(BTreeSet::from(['b']));
    ///
    /// let z = SetFamily::from_sets(sets, &holder);
    /// let z_offset = z.offset('a');
    ///
    /// let members: Vec<Vec<char>> = z_offset.members().collect();
    /// assert_eq!(members, vec![vec!['b']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn offset(self, value: V) -> SetFamily<'a, V> {
        if self.is_zero() || self.is_one() {
            return self.clone();
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        if self_val == value {
            return self_lo;
        }
        if self_val > value {
            return self.clone();
        }

        let holder = self.manager;
        let op = Operations::Offset(self.as_raw(), value.clone());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let value_clone = value.clone();
        let (lo, hi) = self
            .manager
            .pools()
            .join(|| self_lo.offset(value), || self_hi.offset(value_clone));
        let r = holder.get_node(self_val, lo, hi);
        holder.put_into_cache(op, r)
    }

    ///Creates a ZDD with all combinations that include `value` and then deletes `value` from those
    ///combinations.
    ///
    ///It is defined as `f.onset(x)` = { α - {x} | α ∈ f ∧ x ∈ α}
    ///```
    ///# use std::collections::BTreeSet;
    ///# use zuddy::{ZddHolder, SetFamily};
    /// let mut holder = ZddHolder::<char>::new();
    ///
    /// // Create set family: {a, b}, {a}, {b}
    /// let mut sets = BTreeSet::new();
    /// sets.insert(BTreeSet::from(['a', 'b']));
    /// sets.insert(BTreeSet::from(['a']));
    /// sets.insert(BTreeSet::from(['b']));
    ///
    /// let z = SetFamily::from_sets(sets, &holder);
    /// let z_offset = z.onset('a');
    ///
    /// let members: Vec<Vec<char>> = z_offset.members().collect();
    /// assert_eq!(members, vec![vec!['b'], vec![]]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn onset(self, value: V) -> SetFamily<'a, V> {
        let holder = self.manager;
        if self.is_zero() || self.is_one() {
            return holder.zero();
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        if self_val == value {
            return self_hi;
        }
        if self_val > value {
            return holder.zero();
        }

        let op = Operations::Onset(self.as_raw(), value.clone());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let value_cloned = value.clone();
        let (lo, hi) = self
            .manager
            .pools()
            .join(|| self_lo.onset(value), || self_hi.onset(value_cloned));

        let r = holder.get_node(self_val.clone(), lo, hi);
        holder.put_into_cache(op, r)
    }

    ///The set intersection of `self` and `other`
    ///
    ///```
    ///# use std::collections::BTreeSet;
    ///# use zuddy::{ZddHolder, SetFamily};
    /// let mut holder = ZddHolder::<char>::new();
    ///
    /// let mut sets1 = BTreeSet::new();
    /// sets1.insert(BTreeSet::from(['a', 'b']));
    /// sets1.insert(BTreeSet::from(['a']));
    /// let z1 = SetFamily::from_sets(sets1, &holder);
    ///
    /// let mut z2 = SetFamily::singleton('a', &holder);
    ///
    /// let z_intersect = z1.intersect(z2);
    ///
    /// let members: Vec<Vec<char>> = z_intersect.members().collect();
    /// assert_eq!(members, vec![vec!['a']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn intersect(mut self, mut other: Self) -> SetFamily<'a, V> {
        let holder = self.manager;
        if self.is_zero() || other.is_zero() {
            return holder.zero();
        }

        if self == other {
            return self;
        }

        if self.id > other.id {
            std::mem::swap(&mut self, &mut other);
        }

        let op = Operations::Intersect(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        if self.is_one() {
            // since we ensure self has a lower id, and one has an id of 1, we only
            // need to check self and not other since we also checked self==other
            return if other.contains_empty_set() {
                //since {empty} intersect X must be either empty or nothing.
                self
            } else {
                holder.zero()
            };
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get().expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => self_lo.intersect(other),
            std::cmp::Ordering::Greater => self.intersect(other_lo),
            std::cmp::Ordering::Equal => {
                let (lo, hi) = holder.pools().join(
                    || self_lo.intersect(other_lo),
                    || self_hi.intersect(other_hi),
                );
                holder.get_node(self_val, lo, hi)
            }
        };
        holder.put_into_cache(op, r)
    }

    ///Takes the set union of two families of sets.
    ///
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', & holder);
    /// let b = SetFamily::singleton('b', & holder);
    /// let c = SetFamily::singleton('c', & holder);
    /// assert_eq!(a.clone().union(b.clone()).size().unwrap(), 2);
    /// assert_eq!(a.union(b).union(c).size().unwrap(), 3);
    ///```
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if self.is_zero() {
            return other;
        }
        if other.is_zero() || self == other {
            return self;
        }

        let holder = self.manager;
        let op = Operations::Union(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        if self.is_one() || other.is_one() {
            let mut one = self;
            let mut other = other;
            if other.is_one() {
                std::mem::swap(&mut other, &mut one);
            }

            let (v, lo, hi) = other.get().unwrap();
            let lo = one.union(lo);
            return holder.get_node(v, lo, hi);
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get().expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => {
                let value = self_val.clone();
                let lo = self_lo.union(other);
                holder.get_node(value, lo, self_hi)
            }
            std::cmp::Ordering::Greater => {
                let lo = self.union(other_lo);
                holder.get_node(other_val, lo, other_hi)
            }
            std::cmp::Ordering::Equal => {
                let (lo, hi) = self
                    .manager
                    .pools()
                    .join(|| self_lo.union(other_lo), || self_hi.union(other_hi));
                holder.get_node(self_val, lo, hi)
            }
        };

        holder.put_into_cache(op, r)
    }

    ///The set difference of `self` and `other`
    ///
    ///```
    ///# use std::collections::BTreeSet;
    ///# use zuddy::{ZddHolder, SetFamily};
    /// let mut holder = ZddHolder::<char>::new();
    ///
    /// let mut sets1 = BTreeSet::new();
    /// sets1.insert(BTreeSet::from(['a', 'b']));
    /// sets1.insert(BTreeSet::from(['a']));
    /// let z1 = SetFamily::from_sets(sets1, &holder);
    ///
    /// let mut z2 = SetFamily::singleton('a', &holder);
    ///
    /// let z_intersect = z1.difference(z2);
    ///
    /// let members: Vec<Vec<char>> = z_intersect.members().collect();
    /// assert_eq!(members, vec![vec!['a', 'b']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn difference(self, other: Self) -> SetFamily<'a, V> {
        if self.is_zero() || self == other {
            return self.manager.zero();
        }
        if other.is_zero() {
            return self;
        }
        let holder = self.manager;
        let op = Operations::Difference(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        if self.is_one() {
            let lo = other.lo().unwrap();
            return self.difference(lo);
        }

        if other.is_one() {
            let (value, lo, hi) = self.get().expect("Invalid index");
            let lo = lo.difference(other);
            return holder.get_node(value, lo, hi);
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get().expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => {
                holder.get_node(self_val, self_lo.difference(other), self_hi)
            }
            std::cmp::Ordering::Greater => self.difference(other_lo),
            std::cmp::Ordering::Equal => {
                let (lo, hi) = self.manager.pools().join(
                    || self_lo.difference(other_lo),
                    || self_hi.difference(other_hi),
                );
                holder.get_node(self_val, lo, hi)
            }
        };
        holder.put_into_cache(op, r)
    }

    ///Inverts whether a value is included or not included on each combination in the family.
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∈ f ∧ x ∉ α} ∪ { α - {x} | α ∈ f}
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    ///let a = SetFamily::singleton('a', &holder);
    ///let b = SetFamily::singleton('b', &holder);
    ///let c = SetFamily::singleton('c', &holder);
    ///let a_b_c = a.union(b).union(c);
    ///assert_eq!(a_b_c.members().map(|x| x.into_iter().collect::<String>()).collect::<Vec<_>>(), vec![ "a", "b", "c",]);
    ///println!("{}", a_b_c.graphviz());
    ///let changed = a_b_c.change('c');
    ///println!("{}", changed.graphviz());
    ///assert_eq!(changed.members().map(|x| x.into_iter().collect::<String>()).collect::<Vec<_>>(), vec!["ac", "bc", ""]);
    ///
    ///```
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn change(&self, value: V) -> Self {
        if self.is_zero() {
            return self.clone();
        }
        let holder = self.manager;
        if self.is_one() {
            return SetFamily::singleton(value, holder);
        }

        let (this_val, lo, hi) = self.get().expect("Invalid index");

        if this_val == value {
            return holder.get_node(value, hi, lo);
        }
        if this_val > value {
            return holder.get_node(value, holder.zero(), self.clone());
        }
        let op = Operations::Change(self.as_raw(), value.clone());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let this_val = this_val.clone();
        let value_cloned = value.clone();
        let (new_lo, new_hi) = self
            .manager
            .pools()
            .join(|| lo.change(value), || hi.change(value_cloned));

        let r = holder.get_node(this_val, new_lo, new_hi);
        holder.put_into_cache(op, r)
    }

    ///Adds a value to all sets.
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∈ f}
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn insert(&self, value: V) -> Self {
        if self.is_zero() {
            return self.clone();
        }
        let holder = self.manager;
        if self.is_one() {
            return SetFamily::singleton(value, holder);
        }

        let op = Operations::Insert(self.as_raw(), value.clone());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let (this_val, lo, hi) = self.get().expect("Invalid index");

        let r = match this_val.cmp(&value) {
            std::cmp::Ordering::Less => {
                let (lo, hi) = holder
                    .pools()
                    .join(|| lo.insert(value.clone()), || hi.insert(value.clone()));

                holder.get_node(this_val, lo, hi)
            }
            std::cmp::Ordering::Equal => holder.get_node(value, holder.zero(), hi.union(lo)),
            std::cmp::Ordering::Greater => holder.get_node(value, holder.zero(), self.clone()),
        };

        holder.put_into_cache(op, r)
    }

    ///Adds a value to all sets, but keeps the original sets
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∈ f} ∪ f
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn insert_as_superset(&self, value: V) -> Self {
        if self.is_zero() {
            return self.clone();
        }
        let holder = self.manager;
        if self.is_one() {
            return holder.get_node(value, holder.one(), holder.one());
        }

        let op = Operations::InsertSuperset(self.as_raw(), value.clone());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        let (this_val, lo, hi) = self.get().expect("Invalid index");

        let r = match this_val.cmp(&value) {
            std::cmp::Ordering::Less => {
                let (lo, hi) = holder.pools().join(
                    || lo.insert_as_superset(value.clone()),
                    || hi.insert_as_superset(value.clone()),
                );

                holder.get_node(this_val, lo, hi)
            }
            std::cmp::Ordering::Equal => holder.get_node(value, lo.clone(), hi.union(lo)),
            std::cmp::Ordering::Greater => holder.get_node(value, self.clone(), self.clone()),
        };

        holder.put_into_cache(op, r)
    }

    ///Adds a value to all sets, but keeps the original sets
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∈ f} ∪ f
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn extend_as_superset(&self, values: impl IntoIterator<Item = V>) -> Self {
        if self.is_zero() {
            return self.clone();
        }
        let values = self.manager().single_set(values.into_iter().collect());
        let cache = self.manager().create_temporary_cache();
        extend_as_superset_inner(self.clone(), values, &cache)
    }
}

fn extend_as_superset_inner<'a, V>(
    set: SetFamily<'a, V>,
    values: SingleSet<'a, V>,
    cache: &TempCache<'a, V, (ZddIndex<V>, SingleSet<'a, V>)>,
) -> SetFamily<'a, V>
where
    V: Eq + Hash + Ord + Send + Sync + Clone,
{
    if set.is_zero() || values.is_empty() {
        return set.clone();
    }
    let holder = set.manager;
    if set.is_one() {
        return add_all_subsets(holder.one(), values);
    }

    let op = (set.as_raw(), values.clone());
    if let Some(r) = cache.get(&op) {
        return r;
    }

    let (this_val, lo, hi) = set.get().expect("Invalid index");

    let PivotedSets {
        lower,
        mut higher_or_equal,
    } = values.pivot(&this_val);

    let set = if let Some(top) = higher_or_equal.first() {
        if top > this_val {
            let (lo, hi) = holder.pools().join(
                || extend_as_superset_inner(lo, higher_or_equal.clone(), cache),
                || extend_as_superset_inner(hi, higher_or_equal.clone(), cache),
            );
            holder.get_node(this_val, lo, hi)
        } else {
            higher_or_equal.pop_first();
            // top must be equal since we've checked if it was smaller or bigger.
            let (lo, hi) = holder.pools().join(
                || extend_as_superset_inner(lo, higher_or_equal.clone(), cache),
                || extend_as_superset_inner(hi, higher_or_equal.clone(), cache),
            );
            holder.get_node(this_val, lo.clone(), hi.union(lo))
        }
    } else {
        //if there are no more values to add, we just return the set itself
        set
    };

    //Add all possible subsets that are smaller to the set.
    let r = add_all_subsets(set, lower);
    cache.insert(op, r)
}

///Adds all subsets from `values` to `set`, assuming that all members of `values` are lower than all
///members of `values`.
fn add_all_subsets<'a, V>(mut set: SetFamily<'a, V>, values: SingleSet<'a, V>) -> SetFamily<'a, V>
where
    V: Eq + Hash + Ord + Send + Sync + Clone,
{
    let holder = set.manager();
    for value in values.into_iter().rev() {
        set = holder.get_node(value, set.clone(), set);
    }
    set
}

impl<V: Eq + Hash + Ord + Send + Sync + Clone> ZddHolder<V> {
    ///Get all possible subsets from an iterator of values.
    pub fn all_subsets(&self, values: impl IntoIterator<Item = V>) -> SetFamily<'_, V> {
        let mut values = values.into_iter().collect::<BTreeSet<_>>();
        let mut set = self.one();
        while let Some(value) = values.pop_last() {
            set = self.get_node(value, set.clone(), set);
        }
        set
    }
}

#[cfg(test)]
mod test {
    #![expect(clippy::redundant_closure_for_method_calls)]

    use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

    use crate::utils::test::{str_to_sets, test_op, test_single_op};

    use super::*;
    use std::collections::BTreeSet;

    const PARALLEL_REPS: usize = 20;

    #[test]
    fn singleton() {
        let holder = ZddHolder::<char>::new();

        for ch in ['a', 'b', 'c'] {
            let set = SetFamily::singleton(ch, &holder);
            assert_eq!(set.size().unwrap(), 1);
            assert_eq!(set.clone().members().collect::<Vec<_>>(), vec![vec![ch]]);
            let (ch2, lo, hi) = set.get().unwrap();
            assert_eq!(ch2, ch);
            assert_eq!(lo.as_raw(), ZddIndex::ZERO);
            assert_eq!(hi.as_raw(), ZddIndex::ONE);
        }
    }

    #[test]
    fn test_from_sets_basic() {
        let holder = ZddHolder::<char>::new();

        let sets = str_to_sets("ab a b");

        let z = SetFamily::from_sets(sets.clone(), &holder);
        let members: BTreeSet<Vec<char>> = z.members().collect();

        let sets: BTreeSet<Vec<_>> = sets.into_iter().map(|x| x.into_iter().collect()).collect();
        assert_eq!(members, sets);
    }

    #[test]
    fn test_from_sets_empty() {
        let holder = ZddHolder::<char>::new();
        let sets = BTreeSet::new();

        let z = SetFamily::from_sets(sets, &holder);
        let members: Vec<Vec<char>> = z.members().collect();

        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_from_sets_single_element() {
        let holder = ZddHolder::<char>::new();
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['x']));

        let z = SetFamily::from_sets(sets, &holder);
        let members: Vec<Vec<char>> = z.members().collect();

        assert_eq!(members, vec![vec!['x']]);
    }

    #[test]
    fn test_offset() {
        let holder = ZddHolder::new();
        let ops = [
            ("ab a b c", vec!['a'], "b c"),
            ("a ab", vec!['a'], ""),
            ("b c d", vec!['a'], "b c d"),
            ("ab a b c ", vec!['a'], "b c "),
            ("a", vec!['a'], ""),
            ("b", vec!['a'], "b"),
            ("a ab abc", vec!['a'], ""),
            ("bc bd", vec!['a'], "bc bd"),
            (" ", vec!['a'], " "),
            ("", vec!['a'], ""),
            ("ef g h l", vec!['g'], "ef h l"),
            ("ab a b c df e g h l", "abceghl".chars().collect(), "df"),
        ];

        for (a, b, res) in ops.iter().cloned() {
            test_single_op(a, b, res, |x, y| x.offset(y), "offset", &holder);
        }

        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().cloned())
            .for_each(|(a, b, res)| {
                test_single_op(a, b, res, |x, y| x.offset(y), "offset", &holder);
            });
    }

    #[test]
    fn test_onset() {
        let holder = ZddHolder::new();

        let ops = [
            ("ab a b c", vec!['a'], "b  "),
            ("b c", vec!['a'], ""),
            ("a ab abc", vec!['a'], " b bc"),
            ("a ab", vec!['a'], " b"),
            ("a", vec!['a'], " "),
            ("b bc", vec!['a'], ""),
            (" ", vec!['a'], ""),
            ("", vec!['a'], ""),
            ("ab b ac c", vec!['a'], "b c"),
        ];
        for (a, b, res) in ops.iter().cloned() {
            test_single_op(a, b, res, |x, y| x.onset(y), "onset", &holder);
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().cloned())
            .for_each(|(a, b, res)| {
                test_single_op(a, b, res, |x, y| x.onset(y), "onset", &holder);
            });
    }

    #[test]
    fn test_insert() {
        let holder = ZddHolder::new();

        let ops = [
            ("ab a b c", vec!['a'], "ab a ac"),
            ("b c", vec!['a'], "ab ac"),
            ("a ab abc", vec!['a'], "a ab abc"),
            ("a ab", vec!['a'], "a ab"),
            ("a", vec!['a'], "a"),
            ("b bc", vec!['a'], "ab abc"),
            (" ", vec!['a'], "a"),
            ("", vec!['a'], ""),
            ("ab b ac c", vec!['a'], "ab ac"),
            ("a", vec!['b', 'c'], "abc"),
        ];
        for (a, b, res) in ops.iter().cloned() {
            test_single_op(a, b, res, |x, y| x.insert(y), "insert", &holder);
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().cloned())
            .for_each(|(a, b, res)| {
                test_single_op(a, b, res, |x, y| x.insert(y), "insert", &holder);
            });
    }

    #[test]
    fn test_insert_as_superset() {
        let holder = ZddHolder::new();

        let ops = [
            ("ab a b c", vec!['a'], "ab a ac b c"),
            ("b c", vec!['a'], "ab ac b c"),
            ("a ab abc", vec!['a'], "a ab abc"),
            ("a ab", vec!['a'], "a ab"),
            ("a", vec!['a'], "a"),
            ("b bc", vec!['a'], "ab abc b bc"),
            (" ", vec!['a'], "a "),
            ("", vec!['a'], ""),
            ("ab b ac c", vec!['a'], "ab ac b c"),
            ("a", vec!['b', 'c'], "a ab ac abc"),
        ];
        for (a, b, res) in ops.iter().cloned() {
            test_single_op(
                a,
                b,
                res,
                |x, y| x.insert_as_superset(y),
                "insert as superset",
                &holder,
            );
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().cloned())
            .for_each(|(a, b, res)| {
                test_single_op(
                    a,
                    b,
                    res,
                    |x, y| x.insert_as_superset(y),
                    "insert as superset",
                    &holder,
                );
            });
    }

    #[test]
    fn test_change() {
        let holder = ZddHolder::new();
        let ops = [
            ("ab a b c", vec!['a'], "b ab ac "),
            ("b c", vec!['a'], "ab ac"),
            ("a", vec!['a'], " "),
            (" ", vec!['a'], "a"),
            ("b", vec!['a'], "ab"),
            ("ab b", vec!['a'], "b ab"),
            ("ab a b c", vec!['a'], "b ab ac "),
            ("ab a b c", vec!['a', 'a', 'a', 'a'], "ab a b c"),
            ("ab a b c", vec!['a', 'b'], " a b abc"),
            ("abc bc", vec!['a'], "bc abc"),
        ];
        for (a, b, res) in ops.iter().cloned() {
            test_single_op(a, b, res, |x, v| x.change(v), "change", &holder);
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().cloned())
            .for_each(|(a, b, res)| {
                test_single_op(a, b, res, |x, y| x.change(y), "change", &holder);
            });
    }

    #[test]
    fn test_intersect() {
        let holder = ZddHolder::new();
        let ops = [
            ("ab a b", "ab a c", "ab a"),
            ("a", "b", ""),
            ("ab cd c e f df", "cd e f z", "cd e f"),
            ("a b c", "a b c", "a b c"),
            ("a b", "c d", ""),
            ("a b", "", ""),
            (" a", " b", " "),
            (" a", "a b", "a"),
            ("ab ac a", "a b", "a"),
            ("a b c", "a b", "a b"),
            (" ", " ", " "),
        ];
        for (a, b, res) in ops {
            test_op(a, b, res, |x, y| x.intersect(y), "∩", &holder);
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().copied())
            .for_each(|(a, b, res)| {
                test_op(a, b, res, |x, y| x.intersect(y), "∩", &holder);
            });
    }

    #[test]
    fn test_difference() {
        let holder = ZddHolder::new();
        let ops = [
            ("ab a b c", "ab a", "b c"),
            ("a", "b", "a"),
            ("a b", "a b", ""),
            ("a b", "", "a b"),
            ("a", "a b c", ""),
            ("a b", "c d", "a b"),
            (" a b", " ", "a b"),
            (" a", "a", " "),
            ("ab bc cd", "bc", "ab cd"),
            ("", "a b", ""),
        ];

        for (a, b, res) in ops {
            test_op(a, b, res, |x, y| x.difference(y), "-", &holder);
        }

        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().copied())
            .for_each(|(a, b, res)| {
                test_op(a, b, res, |x, y| x.difference(y), "-", &holder);
            });
    }

    #[test]
    fn test_union() {
        let holder = ZddHolder::new();
        let ops = [
            ("ab ", "a", "ab a "),
            ("", "", ""),
            ("", "a", "a"),
            ("a", "", "a"),
            ("a b c", "a b c", "a b c"),
            ("a b", "c d", "a b c d"),
            ("a b", "b c", "a b c"),
            (" a", "b", " a b"),
            (" a", " b", " a b"),
            ("ab c", "a bc", "ab c a bc"),
        ];
        for (a, b, res) in ops {
            test_op(a, b, res, |x, y| x.union(y), "∪", &holder);
        }
        rayon::iter::repeat_n(ops.as_slice(), PARALLEL_REPS)
            .flat_map(|x| x.par_iter().copied())
            .for_each(|(a, b, res)| {
                test_op(a, b, res, |x, y| x.union(y), "∪", &holder);
            });
    }

    #[test]
    fn test_extend_as_superset() {
        let holder = ZddHolder::new();
        let ops = [("ab ", "abcd"), ("", "a"), (" ", "abc"), ("de ef", "abcef")];
        for (s, ops) in ops {
            let ops = ops.chars().collect::<BTreeSet<_>>();
            let s = str_to_sets(s);
            let mut res = s.clone();
            for op in ops.iter().copied() {
                res = res
                    .into_iter()
                    .flat_map(|x| {
                        let mut y = x.clone();
                        y.insert(op);
                        [x, y]
                    })
                    .collect();
            }

            let s = SetFamily::from_sets(s, &holder);

            let result = SetFamily::from_sets(res, &holder);

            let mut iterative_s = s.clone();
            for op in ops.iter().copied() {
                iterative_s = iterative_s.insert_as_superset(op);
            }
            iterative_s.check_valid_zdd();
            let batched_s = s.extend_as_superset(ops);
            batched_s.check_valid_zdd();

            println!("{}", batched_s.graphviz());

            assert_eq!(
                iterative_s, batched_s,
                "Inserting and extending are not equivalent! {batched_s} != {iterative_s}",
            );
            assert_eq!(
                batched_s, result,
                "The set is not what was expected! {batched_s} != {result}"
            );
        }
    }
}
