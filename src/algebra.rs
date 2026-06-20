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
use serde::{Deserialize, Serialize};

use crate::{SetFamily, manager::ZddIndex};

use std::{fmt::Debug, hash::Hash};

//TODO: Make this have a constructor that orders fields so that commmutative operations don't get
//doubled.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub(super) enum Operations<V> {
    Change(ZddIndex<V>, V),
    Offset(ZddIndex<V>, V),
    Onset(ZddIndex<V>, V),
    Union(ZddIndex<V>, ZddIndex<V>),
    Intersect(ZddIndex<V>, ZddIndex<V>),
    Difference(ZddIndex<V>, ZddIndex<V>),
    Join(ZddIndex<V>, ZddIndex<V>),
    Division(ZddIndex<V>, ZddIndex<V>),
}

mod unate;

impl<'a, V: Hash + Ord + Eq + Clone + Debug + Send + Sync> SetFamily<'a, V> {
    ///Creates a ZDD with all combinations that don't include `value`
    ///
    ///It is defined as `f.offset(x)` = { α | α ∉ f}
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
        self.check_valid_zdd();

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

        let r = holder.get_node(
            self_val.clone(),
            self_lo.onset(value.clone()),
            self_hi.onset(value),
        );
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
    pub fn intersect(self, other: Self) -> SetFamily<'a, V> {
        let holder = self.manager;
        if self.is_zero() || other.is_zero() {
            return holder.zero();
        }
        if self == other {
            return self;
        }
        let op = Operations::Intersect(self.as_raw(), other.as_raw());
        if let Some(r) = holder.get_from_cache(&op) {
            return r;
        }

        if self.is_one() || other.is_one() {
            let mut one = self;
            let mut other = other;
            if other.is_one() {
                std::mem::swap(&mut other, &mut one);
            }

            return one.intersect(other.lo().unwrap());
        }

        let (self_val, self_lo, self_hi) = self.get().expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get().expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => self_lo.intersect(other),
            std::cmp::Ordering::Greater => self.intersect(other_lo),
            std::cmp::Ordering::Equal => {
                let lo = self_lo.intersect(other_lo);
                let hi = self_hi.intersect(other_hi);
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
    /// assert_eq!(a.clone().union(b.clone()).size(), Some(2));
    /// assert_eq!(a.union(b).union(c).size(), Some(3));
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
                let lo = self_lo.difference(other_lo);
                let hi = self_hi.difference(other_hi);
                holder.get_node(self_val, lo, hi)
            }
        };
        holder.put_into_cache(op, r)
    }

    ///Inverts whether a value is included or not included on each combination in the family.
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∉ f} ∪ { α - {x} | α ∈ f}
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
        let new_lo = lo.change(value.clone());
        let new_hi = hi.change(value);

        let r = holder.get_node(this_val, new_lo, new_hi);
        holder.put_into_cache(op, r)
    }
}

#[cfg(test)]
use std::collections::BTreeSet;

#[cfg(test)]
use crate::ZddHolder;

#[cfg(test)]
fn str_to_sets(s: &str) -> BTreeSet<BTreeSet<char>> {
    if s.is_empty() {
        return BTreeSet::default();
    }

    s.split(' ')
        .map(|x| x.chars().collect::<BTreeSet<_>>())
        .collect::<BTreeSet<_>>()
}

#[cfg(test)]
///Allows for easy testing of operations, taking family of sets of chars as strings seperated
///by spaces, with `res` being the intended result with the operand supplied by `op`
fn test_op<F: for<'a> Fn(SetFamily<'a, char>, SetFamily<'a, char>) -> SetFamily<'a, char>>(
    a: &str,
    b: &str,
    res: &str,
    op: F,
    op_name: &'static str,
    holder: &ZddHolder<char>,
) {
    let a_sets = str_to_sets(a);
    let b_sets = str_to_sets(b);
    let a_op_b = str_to_sets(res);
    println!("{a_sets:?} {op_name} {b_sets:?} = {a_op_b:?}");
    let a_set_len = a_sets.len();
    let b_set_len = b_sets.len();

    let a = SetFamily::from_sets(a_sets, holder);
    let b = SetFamily::from_sets(b_sets, holder);
    assert_eq!(a.size(), Some(a_set_len));
    a.check_valid_zdd();
    assert_eq!(b.size(), Some(b_set_len));
    b.check_valid_zdd();

    let result = op(a, b);
    result.check_valid_zdd();

    let result_recon: BTreeSet<BTreeSet<char>> =
        result.members().map(|x| x.into_iter().collect()).collect();
    assert_eq!(result_recon, a_op_b);
}

#[cfg(test)]
///Allows for easy testing of operations, taking family of sets of chars as strings seperated
///by spaces, with `res` being the intended result with the operand supplied by `op`
fn test_single_op<F: for<'a> Fn(SetFamily<'a, char>, char) -> SetFamily<'a, char>>(
    start: &str,
    actions: Vec<char>,
    res: &str,
    op: F,
    op_name: &'static str,
    holder: &ZddHolder<char>,
) {
    let start = str_to_sets(start);

    let ops = actions
        .iter()
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join(format!(" {op_name} ").as_str());
    let intended = str_to_sets(res);
    println!("{start:?} {op_name} {ops} = {intended:?}");

    let start_len = start.len();
    let a = SetFamily::from_sets(start, holder);
    a.check_valid_zdd();
    assert_eq!(a.size(), Some(start_len));

    let mut result = a.clone();
    for action in actions {
        result = op(result, action);
        result.check_valid_zdd();
    }

    result.check_valid_zdd();
    let result_recon: BTreeSet<BTreeSet<char>> =
        result.members().map(|x| x.into_iter().collect()).collect();

    assert_eq!(result_recon, intended);
}

#[cfg(test)]
mod test {
    #![expect(clippy::redundant_closure_for_method_calls)]

    use super::*;
    use std::collections::BTreeSet;

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
        for (a, b, res) in [
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
        ] {
            test_single_op(a, b, res, |x, y| x.offset(y), "offset", &holder);
        }
    }

    #[test]
    fn test_onset() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
            ("ab a b c", vec!['a'], "b  "),
            ("b c", vec!['a'], ""),
            ("a ab abc", vec!['a'], " b bc"),
            ("a ab", vec!['a'], " b"),
            ("a", vec!['a'], " "),
            ("b bc", vec!['a'], ""),
            (" ", vec!['a'], ""),
            ("", vec!['a'], ""),
            ("ab b ac c", vec!['a'], "b c"),
        ] {
            test_single_op(a, b, res, |x, y| x.onset(y), "onset", &holder);
        }
    }

    #[test]
    fn test_change() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
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
        ] {
            test_single_op(a, b, res, |x, v| x.change(v), "change", &holder);
        }
    }

    #[test]
    fn test_intersect() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
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
        ] {
            test_op(a, b, res, |x, y| x.intersect(y), "∩", &holder);
        }
    }

    #[test]
    fn test_difference() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
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
        ] {
            test_op(a, b, res, |x, y| x.difference(y), "-", &holder);
        }
    }

    #[test]
    fn test_union() {
        let holder = ZddHolder::new();
        for (a, b, res) in [
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
        ] {
            test_op(a, b, res, |x, y| x.union(y), "∪", &holder);
        }
    }
}
