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

use super::{RawZdd, Zdd, ZddHolder};
use std::{fmt::Debug, hash::Hash};

//TODO: Make this have a constructor that orders fields so that commmutative operations don't get
//doubled.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub(super) enum Operations<V> {
    Change(RawZdd<V>, V),
    Offset(RawZdd<V>, V),
    Onset(RawZdd<V>, V),
    Union(RawZdd<V>, RawZdd<V>),
    Intersect(RawZdd<V>, RawZdd<V>),
    Difference(RawZdd<V>, RawZdd<V>),
    Join(RawZdd<V>, RawZdd<V>),
    Division(RawZdd<V>, RawZdd<V>),
}

mod unate;

impl<V: Hash + Ord + Eq + Clone> RawZdd<V> {
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
    /// let z = SetFamily::from_sets(sets, &mut holder);
    /// let z_offset = z.offset('a', &mut holder);
    ///
    /// let members: Vec<Vec<char>> = z_offset.members(&holder).collect();
    /// assert_eq!(members, vec![vec!['b']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn offset(self, value: V, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        if self_val == value {
            return self_lo;
        }
        if self_val > value {
            return self;
        }

        let op = Operations::Offset(self, value.clone());
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let v = Zdd {
            value: self_val,
            lo: self_lo.offset(value.clone(), holder),
            hi: self_hi.offset(value, holder),
        };

        let r = holder.get_node_seq(v);
        holder.cache.insert(op, r);
        r
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
    /// let z = SetFamily::from_sets(sets, &mut holder);
    /// let z_offset = z.onset('a', &mut holder);
    ///
    /// let members: Vec<Vec<char>> = z_offset.members(&holder).collect();
    /// assert_eq!(members, vec![vec!['b'], vec![]]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn onset(self, value: V, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if self.is_zero() || self.is_one() {
            return RawZdd::ZERO;
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        if self_val == value {
            return self_hi;
        }
        if self_val > value {
            return RawZdd::ZERO;
        }

        let op = Operations::Onset(self, value.clone());
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let v = Zdd {
            value: self_val.clone(),
            lo: self_lo.onset(value.clone(), holder),
            hi: self_hi.onset(value, holder),
        };

        let r = holder.get_node_seq(v);
        holder.cache.insert(op, r);
        r
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
    /// let z1 = SetFamily::from_sets(sets1, &mut holder);
    ///
    /// let mut z2 = SetFamily::singleton('a', &mut holder);
    ///
    /// let z_intersect = z1.intersect(z2, &mut holder);
    ///
    /// let members: Vec<Vec<char>> = z_intersect.members(&holder).collect();
    /// assert_eq!(members, vec![vec!['a']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn intersect(self, other: Self, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if self.is_zero() || other.is_zero() {
            return RawZdd::ZERO;
        }
        if self == other {
            return self;
        }
        let op = Operations::Intersect(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        if self.is_one() || other.is_one() {
            let mut one = self;
            let mut other = other;
            if other.is_one() {
                std::mem::swap(&mut other, &mut one);
            }

            let q = holder.data.read().unwrap()[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            return one.intersect(q.lo, holder);
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => self_lo.intersect(other, holder),
            std::cmp::Ordering::Greater => self.intersect(other_lo, holder),
            std::cmp::Ordering::Equal => {
                let lo = self_lo.intersect(other_lo, holder);
                let hi = self_hi.intersect(other_hi, holder);
                holder.get_node_seq(Zdd {
                    value: self_val,
                    lo,
                    hi,
                })
            }
        };
        holder.cache.insert(op, r);
        r
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
    /// let z1 = SetFamily::from_sets(sets1, &mut holder);
    ///
    /// let mut z2 = SetFamily::singleton('a', &mut holder);
    ///
    /// let z_intersect = z1.difference(z2, &mut holder);
    ///
    /// let members: Vec<Vec<char>> = z_intersect.members(&holder).collect();
    /// assert_eq!(members, vec![vec!['a', 'b']]);
    ///```
    ///
    ///# Panics
    ///May panic if `self` or `other` is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn difference(self, other: Self, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if self.is_zero() || self == other {
            return RawZdd::ZERO;
        }
        if other.is_zero() {
            return self;
        }
        let op = Operations::Difference(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        if self.is_one() {
            let q = holder.data.read().unwrap()[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            return self.difference(q.lo, holder);
        }

        if other.is_one() {
            let Zdd { value, lo, hi } = holder.data.read().unwrap()[self.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            let lo = lo.difference(other, holder);
            return holder.get_node_seq(Zdd { value, lo, hi });
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => {
                let v = Zdd {
                    value: self_val,
                    lo: self_lo.difference(other, holder),
                    hi: self_hi,
                };

                holder.get_node_seq(v)
            }
            std::cmp::Ordering::Greater => self.difference(other_lo, holder),
            std::cmp::Ordering::Equal => {
                let lo = self_lo.difference(other_lo, holder);
                let hi = self_hi.difference(other_hi, holder);
                holder.get_node_seq(Zdd {
                    value: self_val,
                    lo,
                    hi,
                })
            }
        };
        holder.cache.insert(op, r);
        r
    }

    ///Inverts whether a value is included or not included on each combination in the family.
    ///
    ///It is defined as `f.change(x)` = { α ∪ {x} | α ∉ f} ∪ { α - {x} | α ∈ f}
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    ///let a = SetFamily::singleton('a', &mut holder);
    ///let b = SetFamily::singleton('b', &mut holder);
    ///let c = SetFamily::singleton('c', &mut holder);
    ///let a_b_c = a.union(b, &mut holder).union(c, &mut holder);
    ///assert_eq!(a_b_c.members(&holder).map(|x| x.into_iter().collect::<String>()).collect::<Vec<_>>(), vec![ "a", "b", "c",]);
    ///println!("{}", a_b_c.graphviz(&holder));
    ///let changed = a_b_c.change('c', &mut holder);
    ///println!("{}", changed.graphviz(&holder));
    ///assert_eq!(changed.members(&holder).map(|x| x.into_iter().collect::<String>()).collect::<Vec<_>>(), vec!["ac", "bc", ""]);
    ///
    ///```
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn change(self, value: V, holder: &mut ZddHolder<V>) -> RawZdd<V> {
        if self.is_zero() {
            return RawZdd::ZERO;
        }
        if self.is_one() {
            return RawZdd::singleton(value, holder);
        }

        let (this_val, lo, hi) = self.get(holder).expect("Invalid index");

        if this_val == value {
            return holder.get_node_seq(Zdd {
                value,
                lo: hi,
                hi: lo,
            });
        }
        if this_val > value {
            return holder.get_node_seq(Zdd {
                value,
                lo: RawZdd::ZERO,
                hi: self,
            });
        }
        let op = Operations::Change(self, value.clone());
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let this_val = this_val.clone();
        let new_lo = lo.change(value.clone(), holder);
        let new_hi = hi.change(value, holder);

        let r = holder.get_node_seq(Zdd {
            value: this_val,
            lo: new_lo,
            hi: new_hi,
        });
        holder.cache.insert(op, r);
        r
    }

    ///Takes the set union of two families of sets.
    ///
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', &mut holder);
    /// let b = SetFamily::singleton('b', &mut holder);
    /// let c = SetFamily::singleton('c', &mut holder);
    /// assert_eq!(a.union(b, &mut holder).size(&mut holder), Some(2));
    /// assert_eq!(a.union(b, &mut holder).union(c, &mut holder).size(&mut holder), Some(3));
    ///```
    ///# Panics
    ///May panic if the self or other value is not a valid index in the [`ZddHolder`]
    #[must_use]
    pub fn union(self, other: Self, holder: &mut ZddHolder<V>) -> Self {
        if self.is_zero() {
            return other;
        }
        if other.is_zero() || self == other {
            return self;
        }

        let op = Operations::Union(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        if self.is_one() || other.is_one() {
            let mut one = self;
            let mut other = other;
            if other.is_one() {
                std::mem::swap(&mut other, &mut one);
            }

            let q = holder.data.read().unwrap()[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            let lo = one.union(q.lo, holder);
            return holder.get_node_seq(Zdd {
                value: q.value,
                lo,
                hi: q.hi,
            });
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(&other_val) {
            std::cmp::Ordering::Less => {
                let value = self_val.clone();
                let lo = self_lo.union(other, holder);
                holder.get_node_seq(Zdd {
                    value,
                    lo,
                    hi: self_hi,
                })
            }
            std::cmp::Ordering::Greater => {
                let lo = self.union(other_lo, holder);
                holder.get_node_seq(Zdd {
                    value: other_val,
                    lo,
                    hi: other_hi,
                })
            }
            std::cmp::Ordering::Equal => {
                let lo = self_lo.union(other_lo, holder);
                let hi = self_hi.union(other_hi, holder);
                holder.get_node_seq(Zdd {
                    value: self_val,
                    lo,
                    hi,
                })
            }
        };

        holder.cache.insert(op, r);
        r
    }
}

#[cfg(test)]
use std::collections::BTreeSet;

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
fn test_op<F: Fn(RawZdd<char>, RawZdd<char>, &mut ZddHolder<char>) -> RawZdd<char>>(
    a: &str,
    b: &str,
    res: &str,
    op: F,
    op_name: &'static str,
) {
    let a_sets = str_to_sets(a);
    let b_sets = str_to_sets(b);
    let a_op_b = str_to_sets(res);
    println!("{a_sets:?} {op_name} {b_sets:?} = {a_op_b:?}");
    let a_set_len = a_sets.len();
    let b_set_len = b_sets.len();

    let mut holder = ZddHolder::new();
    let a = RawZdd::from_sets(a_sets, &mut holder);
    let b = RawZdd::from_sets(b_sets, &mut holder);
    assert_eq!(a.size(&mut holder), Some(a_set_len));
    assert_eq!(b.size(&mut holder), Some(b_set_len));
    let result = op(a, b, &mut holder);

    let result_recon: BTreeSet<BTreeSet<char>> = result
        .members(&holder)
        .map(|x| x.into_iter().collect())
        .collect();

    assert_eq!(result_recon, a_op_b);
    assert_eq!(result.size(&mut holder), Some(a_op_b.len()));
}

#[cfg(test)]
///Allows for easy testing of operations, taking family of sets of chars as strings seperated
///by spaces, with `res` being the intended result with the operand supplied by `op`
fn test_single_op<F: Fn(RawZdd<char>, char, &mut ZddHolder<char>) -> RawZdd<char>>(
    a: &str,
    b: char,
    res: &str,
    op: F,
    op_name: &'static str,
) {
    let a_sets = str_to_sets(a);
    let a_op_b = str_to_sets(res);
    println!("{a_sets:?} {op_name} {b} = {a_op_b:?}");
    let a_set_len = a_sets.len();

    let mut holder = ZddHolder::new();
    let a = RawZdd::from_sets(a_sets, &mut holder);
    assert_eq!(a.size(&mut holder), Some(a_set_len));
    let result = op(a, b, &mut holder);

    let result_recon: BTreeSet<BTreeSet<char>> = result
        .members(&holder)
        .map(|x| x.into_iter().collect())
        .collect();

    assert_eq!(result_recon, a_op_b);
    assert_eq!(result.size(&mut holder), Some(a_op_b.len()));
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn singleton() {
        let mut holder = ZddHolder::<char>::default();

        for ch in ['a', 'b', 'c'] {
            let set = RawZdd::singleton(ch, &mut holder);
            assert_eq!(set.size(&mut holder).unwrap(), 1);
            assert_eq!(set.members(&holder).collect::<Vec<_>>(), vec![vec![ch]]);
            let (ch2, lo, hi) = set.get(&holder).unwrap();
            assert_eq!(ch2, ch);
            assert_eq!(lo, RawZdd::ZERO);
            assert_eq!(hi, RawZdd::ONE);
        }
    }

    #[test]
    fn test_from_sets_basic() {
        let mut holder = ZddHolder::<char>::new();

        let sets = str_to_sets("ab a b");

        let z = RawZdd::from_sets(sets.clone(), &mut holder);
        let members: BTreeSet<Vec<char>> = z.members(&holder).collect();

        let sets: BTreeSet<Vec<_>> = sets.into_iter().map(|x| x.into_iter().collect()).collect();
        assert_eq!(members, sets);
    }

    #[test]
    fn test_from_sets_empty() {
        let mut holder = ZddHolder::<char>::new();
        let sets = BTreeSet::new();

        let z = RawZdd::from_sets(sets, &mut holder);
        let members: Vec<Vec<char>> = z.members(&holder).collect();

        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_from_sets_single_element() {
        let mut holder = ZddHolder::<char>::new();
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['x']));

        let z = RawZdd::from_sets(sets, &mut holder);
        let members: Vec<Vec<char>> = z.members(&holder).collect();

        assert_eq!(members, vec![vec!['x']]);
    }

    #[test]
    fn test_offset() {
        for (a, b, res) in [
            ("ab a b c", 'a', "b c"),
            ("a ab", 'a', ""),
            ("b c d", 'a', "b c d"),
            ("ab a b c ", 'a', "b c "),
            ("a", 'a', ""),
            ("b", 'a', "b"),
            ("a ab abc", 'a', ""),
            ("bc bd", 'a', "bc bd"),
            (" ", 'a', " "),
            ("", 'a', ""),
        ] {
            test_single_op(a, b, res, RawZdd::offset, "offset");
        }
    }

    #[test]
    fn test_onset() {
        for (a, b, res) in [
            ("ab a b c", 'a', "b  "),
            ("b c", 'a', ""),
            ("a ab abc", 'a', " b bc"),
            ("a ab", 'a', " b"),
            ("a", 'a', " "),
            ("b bc", 'a', ""),
            (" ", 'a', ""),
            ("", 'a', ""),
            ("ab b ac c", 'a', "b c"),
        ] {
            test_single_op(a, b, res, RawZdd::onset, "onset");
        }
    }

    #[test]
    fn test_change() {
        for (a, b, res) in [
            ("ab a b c", 'a', "b ab ac "),
            ("b c", 'a', "ab ac"),
            ("a", 'a', " "),
            (" ", 'a', "a"),
            ("b", 'a', "ab"),
            ("ab b", 'a', "b ab"),
            ("ab a b c", 'a', "b ab ac "),
            ("abc bc", 'a', "bc abc"),
        ] {
            test_single_op(a, b, res, RawZdd::change, "change");
        }
    }

    #[test]
    fn test_intersect() {
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
            test_op(a, b, res, RawZdd::intersect, "∩");
        }
    }

    #[test]
    fn test_difference() {
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
            test_op(a, b, res, RawZdd::difference, "-");
        }
    }

    #[test]
    fn test_union() {
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
            test_op(a, b, res, RawZdd::union, "∪");
        }
    }
}
