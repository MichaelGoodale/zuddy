use super::{SetFamily, Zdd, ZddHolder};
use std::{fmt::Debug, hash::Hash};

//TODO: Make this have a constructor that orders fields so that commmutative operations don't get
//doubled.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(super) enum Operations<V> {
    Change(SetFamily<V>, V),
    Offset(SetFamily<V>, V),
    Onset(SetFamily<V>, V),
    Union(SetFamily<V>, SetFamily<V>),
    Intersect(SetFamily<V>, SetFamily<V>),
    Difference(SetFamily<V>, SetFamily<V>),
}

impl<V: Hash + Ord + Eq + Clone> SetFamily<V> {
    ///Creates a ZDD with all combinations that don't include `value`
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
    pub fn offset(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        if self_val == &value {
            return self_lo;
        }
        if self_val > &value {
            return self;
        }

        let op = Operations::Offset(self, value.clone());
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        let v = Zdd {
            value: self_val.clone(),
            lo: self_lo.offset(value.clone(), holder),
            hi: self_hi.offset(value, holder),
        };

        let r = holder.get_node(v);
        holder.cache.insert(op, r);
        r
    }

    ///Creates a ZDD with all combinations that include `value` and then deletes `value` from those
    ///combinations.
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
    pub fn onset(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        if self_val == &value {
            return self_hi;
        }
        if self_val > &value {
            return SetFamily::ZERO;
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

        let r = holder.get_node(v);
        holder.cache.insert(op, r);
        r
    }

    ///The intersection of `self` and `other`
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
    pub fn intersect(self, other: Self, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if self.is_zero() || other.is_zero() {
            return SetFamily::ZERO;
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

            let q = holder.data[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            return one.intersect(q.lo, holder);
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(other_val) {
            std::cmp::Ordering::Less => self_lo.intersect(other, holder),
            std::cmp::Ordering::Greater => self.intersect(other_lo, holder),
            std::cmp::Ordering::Equal => {
                let value = self_val.clone();
                let lo = self_lo.intersect(other_lo, holder);
                let hi = self_hi.intersect(other_hi, holder);
                holder.get_node(Zdd { value, lo, hi })
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
    pub fn difference(self, other: Self, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if self.is_zero() || self == other {
            return SetFamily::ZERO;
        }
        if other.is_zero() {
            return self;
        }
        let op = Operations::Difference(self, other);
        if let Some(r) = holder.cache.get(&op) {
            return *r;
        }

        if self.is_one() {
            let q = holder.data[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            return self.difference(q.lo, holder);
        }

        if other.is_one() {
            let Zdd { value, lo, hi } =
                holder.data[self.0].as_ref().expect("Invalid index").clone();
            let lo = lo.difference(other, holder);
            return holder.get_node(Zdd { value, lo, hi });
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(other_val) {
            std::cmp::Ordering::Less => {
                let v = Zdd {
                    value: self_val.clone(),
                    lo: self_lo.difference(other, holder),
                    hi: self_hi,
                };

                holder.get_node(v)
            }
            std::cmp::Ordering::Greater => self.difference(other_lo, holder),
            std::cmp::Ordering::Equal => {
                let value = self_val.clone();
                let lo = self_lo.difference(other_lo, holder);
                let hi = self_hi.difference(other_hi, holder);
                holder.get_node(Zdd { value, lo, hi })
            }
        };
        holder.cache.insert(op, r);
        r
    }

    ///Creates a singleton set from a value.
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', &mut holder);
    /// assert_eq!(a.members(&holder).collect::<Vec<_>>(), vec![vec!['a']]);
    ///```
    #[must_use]
    pub fn singleton(value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        holder.get_node(Zdd {
            value,
            lo: SetFamily::ZERO,
            hi: SetFamily::ONE,
        })
    }

    ///Inverts whether a value is included or not included on each combination in the family.
    ///
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
    pub fn change(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if self.is_zero() {
            return SetFamily::ZERO;
        }
        if self.is_one() {
            return SetFamily::singleton(value, holder);
        }

        let (this_val, lo, hi) = self.get(holder).expect("Invalid index");

        if this_val == &value {
            return holder.get_node(Zdd {
                value,
                lo: hi,
                hi: lo,
            });
        }
        if this_val > &value {
            return holder.get_node(Zdd {
                value,
                lo: SetFamily::ZERO,
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

        let r = holder.get_node(Zdd {
            value: this_val,
            lo: new_lo,
            hi: new_hi,
        });
        holder.cache.insert(op, r);
        r
    }

    ///Takes the union of two families of sets.
    ///
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

            let q = holder.data[other.0]
                .as_ref()
                .expect("Invalid index")
                .clone();
            let lo = one.union(q.lo, holder);
            return holder.get_node(Zdd {
                value: q.value,
                lo,
                hi: q.hi,
            });
        }

        let (self_val, self_lo, self_hi) = self.get(holder).expect("Invalid index");
        let (other_val, other_lo, other_hi) = other.get(holder).expect("Invalid index");

        let r = match self_val.cmp(other_val) {
            std::cmp::Ordering::Less => {
                let value = self_val.clone();
                let lo = self_lo.union(other, holder);
                holder.get_node(Zdd {
                    value,
                    lo,
                    hi: self_hi,
                })
            }
            std::cmp::Ordering::Greater => {
                let value = other_val.clone();
                let lo = self.union(other_lo, holder);
                holder.get_node(Zdd {
                    value,
                    lo,
                    hi: other_hi,
                })
            }
            std::cmp::Ordering::Equal => {
                let value = self_val.clone();
                let lo = self_lo.union(other_lo, holder);
                let hi = self_hi.union(other_hi, holder);
                holder.get_node(Zdd { value, lo, hi })
            }
        };

        holder.cache.insert(op, r);
        r
    }

    ///Count the number of possible comibinations.
    ///
    ///Due to the combinatorial nature of ZDDs, if you have a sufficiently big ZDD, there will be
    ///too many combinations. In this case, the function will return [`None`]
    ///
    ///# Panics
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    pub fn size(&self, holder: &mut ZddHolder<V>) -> Option<usize> {
        if self.is_zero() {
            return Some(0);
        }
        if self.is_one() {
            return Some(1);
        }

        if let Some(&sum) = holder.sum_cache.get(self) {
            return sum;
        }
        let Zdd { value: _, lo, hi } = *holder.data[self.0].as_ref().expect("Invalid index!");
        let sum = lo
            .size(holder)
            .and_then(|x| hi.size(holder).and_then(|y| x.checked_add(y)));

        holder.sum_cache.insert(*self, sum);
        sum
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn singleton() {
        let mut holder = ZddHolder::<char>::default();
        let a = SetFamily::singleton('a', &mut holder);
        let b = SetFamily::singleton('b', &mut holder);
        let c = SetFamily::singleton('c', &mut holder);

        for x in [a, b, c] {
            assert_eq!(x.size(&mut holder).unwrap(), 1);
            println!("{}", x.graphviz(&holder));
        }

        let ab = a.change('b', &mut holder);
        assert_eq!(ab.size(&mut holder).unwrap(), 1);

        let ab_a = ab.union(a, &mut holder);

        println!("{}", ab_a.graphviz(&holder));
        assert_eq!(ab.union(a, &mut holder).size(&mut holder).unwrap(), 2);
        assert_eq!(
            ab.union(a, &mut holder)
                .union(b, &mut holder)
                .size(&mut holder)
                .unwrap(),
            3
        );
        assert_eq!(
            ab.union(a, &mut holder)
                .union(b, &mut holder)
                .union(c, &mut holder)
                .size(&mut holder)
                .unwrap(),
            4
        );
    }

    #[test]
    fn test_from_sets_basic() {
        let mut holder = ZddHolder::<char>::new();

        // Create a simple set family: {a, b}, {a}, {b}
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['a', 'b']));
        sets.insert(BTreeSet::from(['a']));
        sets.insert(BTreeSet::from(['b']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let members: Vec<Vec<char>> = z.members(&holder).collect();

        assert_eq!(members.len(), 3);
        assert!(members.contains(&vec!['a']));
        assert!(members.contains(&vec!['b']));
        assert!(members.contains(&vec!['a', 'b']));
    }

    #[test]
    fn test_from_sets_empty() {
        let mut holder = ZddHolder::<char>::new();
        let sets = BTreeSet::new();

        let z = SetFamily::from_sets(sets, &mut holder);
        let members: Vec<Vec<char>> = z.members(&holder).collect();

        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_from_sets_single_element() {
        let mut holder = ZddHolder::<char>::new();
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['x']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let members: Vec<Vec<char>> = z.members(&holder).collect();

        assert_eq!(members.len(), 1);
        assert_eq!(members[0], vec!['x']);
    }

    #[test]
    fn test_offset_excludes_value() {
        let mut holder = ZddHolder::<char>::new();

        // Set family: {a, b}, {a}, {b}, {c}
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['a', 'b']));
        sets.insert(BTreeSet::from(['a']));
        sets.insert(BTreeSet::from(['b']));
        sets.insert(BTreeSet::from(['c']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let z_offset = z.offset('a', &mut holder);
        let members: Vec<Vec<char>> = z_offset.members(&holder).collect();

        // Should not contain any set with 'a'
        assert_eq!(members.len(), 2);
        assert!(members.contains(&vec!['b']));
        assert!(members.contains(&vec!['c']));

        for member in &members {
            assert!(!member.contains(&'a'));
        }
    }

    #[test]
    fn test_offset_all_contains_value() {
        let mut holder = ZddHolder::<char>::new();

        // All sets contain 'a'
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['a']));
        sets.insert(BTreeSet::from(['a', 'b']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let z_offset = z.offset('a', &mut holder);
        let members: Vec<Vec<char>> = z_offset.members(&holder).collect();

        // Should be empty since all sets contained 'a'
        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_onincludes_value_then_removes_it() {
        let mut holder = ZddHolder::<char>::new();

        // Set family: {a, b}, {a}, {b}, {c}
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['a', 'b']));
        sets.insert(BTreeSet::from(['a']));
        sets.insert(BTreeSet::from(['b']));
        sets.insert(BTreeSet::from(['c']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let z_onset = z.onset('a', &mut holder);
        let members: Vec<Vec<char>> = z_onset.members(&holder).collect();

        // Should contain sets that had 'a' but without 'a': {b}, {}
        assert_eq!(members.len(), 2);
        assert!(members.contains(&vec!['b']));
        assert!(members.contains(&vec![]));

        for member in &members {
            assert!(!member.contains(&'a'));
        }
    }

    #[test]
    fn test_onset_no_value_present() {
        let mut holder = ZddHolder::<char>::new();

        // None of the sets contain 'a'
        let mut sets = BTreeSet::new();
        sets.insert(BTreeSet::from(['b']));
        sets.insert(BTreeSet::from(['c']));

        let z = SetFamily::from_sets(sets, &mut holder);
        let z_onset = z.onset('a', &mut holder);
        let members: Vec<Vec<char>> = z_onset.members(&holder).collect();

        // Should be empty since no sets contained 'a'
        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_intersect_basic() {
        let mut holder = ZddHolder::<char>::new();

        // First family: {a, b}, {a}, {b}
        let mut sets1 = BTreeSet::new();
        sets1.insert(BTreeSet::from(['a', 'b']));
        sets1.insert(BTreeSet::from(['a']));
        sets1.insert(BTreeSet::from(['b']));

        let z1 = SetFamily::from_sets(sets1, &mut holder);
        assert_eq!(
            z1.members(&holder)
                .map(|x| x.into_iter().collect::<String>())
                .collect::<Vec<String>>(),
            vec!["ab", "a", "b"]
        );

        // Second family: {a, b}, {a}, {c}
        let mut sets2 = BTreeSet::new();
        sets2.insert(BTreeSet::from(['a', 'b']));
        sets2.insert(BTreeSet::from(['a']));
        sets2.insert(BTreeSet::from(['c']));
        let z2 = SetFamily::from_sets(sets2, &mut holder);

        assert_eq!(
            z2.members(&holder)
                .map(|x| x.into_iter().collect::<String>())
                .collect::<Vec<String>>(),
            vec!["ab", "a", "c"]
        );

        let z_intersect = z1.intersect(z2, &mut holder);
        let members: Vec<Vec<char>> = z_intersect.members(&holder).collect();

        // Intersection: {a, b}, {a}
        assert_eq!(members.len(), 2);
        assert!(members.contains(&vec!['a', 'b']));
        assert!(members.contains(&vec!['a']));
    }

    #[test]
    fn test_intersect_empty() {
        let mut holder = ZddHolder::<char>::new();

        // First family: {a}
        let mut sets1 = BTreeSet::new();
        sets1.insert(BTreeSet::from(['a']));

        // Second family: {b}
        let mut sets2 = BTreeSet::new();
        sets2.insert(BTreeSet::from(['b']));

        let z1 = SetFamily::from_sets(sets1, &mut holder);
        let z2 = SetFamily::from_sets(sets2, &mut holder);
        let z_intersect = z1.intersect(z2, &mut holder);
        let members: Vec<Vec<char>> = z_intersect.members(&holder).collect();

        // No intersection
        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_difference_basic() {
        let mut holder = ZddHolder::<char>::new();

        // First family: {a, b}, {a}, {b}, {c}
        let mut sets1 = BTreeSet::new();
        sets1.insert(BTreeSet::from(['a', 'b']));
        sets1.insert(BTreeSet::from(['a']));
        sets1.insert(BTreeSet::from(['b']));
        sets1.insert(BTreeSet::from(['c']));

        // Second family: {a, b}, {a}
        let mut sets2 = BTreeSet::new();
        sets2.insert(BTreeSet::from(['a', 'b']));
        sets2.insert(BTreeSet::from(['a']));

        let z1 = SetFamily::from_sets(sets1, &mut holder);
        let z2 = SetFamily::from_sets(sets2, &mut holder);
        let z_diff = z1.difference(z2, &mut holder);
        let members: Vec<Vec<char>> = z_diff.members(&holder).collect();

        // Difference: {b}, {c}
        assert_eq!(members.len(), 2);
        assert!(members.contains(&vec!['b']));
        assert!(members.contains(&vec!['c']));
    }

    #[test]
    fn test_difference_all_removed() {
        let mut holder = ZddHolder::<char>::new();

        // Both families are identical
        let mut sets1 = BTreeSet::new();
        sets1.insert(BTreeSet::from(['a']));
        sets1.insert(BTreeSet::from(['b']));

        let mut sets2 = BTreeSet::new();
        sets2.insert(BTreeSet::from(['a']));
        sets2.insert(BTreeSet::from(['b']));

        let z1 = SetFamily::from_sets(sets1, &mut holder);
        let z2 = SetFamily::from_sets(sets2, &mut holder);
        let z_diff = z1.difference(z2, &mut holder);
        let members: Vec<Vec<char>> = z_diff.members(&holder).collect();

        // All elements removed
        assert_eq!(members.len(), 0);
    }

    #[test]
    fn test_difference_empty_second() {
        let mut holder = ZddHolder::<char>::new();

        // First family: {a}, {b}
        let mut sets1 = BTreeSet::new();
        sets1.insert(BTreeSet::from(['a']));
        sets1.insert(BTreeSet::from(['b']));

        // Second family: empty
        let sets2 = BTreeSet::new();

        let z1 = SetFamily::from_sets(sets1, &mut holder);
        let z2 = SetFamily::from_sets(sets2, &mut holder);
        let z_diff = z1.difference(z2, &mut holder);
        let members: Vec<Vec<char>> = z_diff.members(&holder).collect();

        // Should be unchanged
        assert_eq!(members.len(), 2);
        assert!(members.contains(&vec!['a']));
        assert!(members.contains(&vec!['b']));
    }
}
