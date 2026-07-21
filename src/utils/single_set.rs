use std::{collections::BTreeSet, fmt::Display, hash::Hash};

use crate::{SetFamily, ZddHolder};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SingleSet<'a, V: Eq + Hash> {
    set: SetFamily<'a, V>,
    last: Option<V>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PivotedSets<'a, V: Eq + Hash> {
    pub lower: SingleSet<'a, V>,
    pub higher_or_equal: SingleSet<'a, V>,
}

impl<V: Eq + Hash> SingleSet<'_, V> {
    pub fn is_empty(&self) -> bool {
        self.set.is_one()
    }
}

impl<V: Eq + Hash + Display + Clone> Display for SingleSet<'_, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{")?;
        let mut s = self.set.clone();
        let mut vals = vec![];
        while let Some((v, _, hi)) = s.get() {
            s = hi;
            vals.push(v.to_string());
        }
        write!(f, "{}}}", vals.join(", "))
    }
}

impl<V: Eq + Hash + Clone> IntoIterator for SingleSet<'_, V> {
    type Item = V;

    type IntoIter = std::vec::IntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        let mut pos = self.set;
        let mut s = vec![];
        while let Some((v, _, hi)) = pos.get() {
            s.push(v);
            pos = hi;
        }
        s.into_iter()
    }
}

impl<V: Eq + Hash + Clone + Send + Sync> SingleSet<'_, V> {
    #[expect(dead_code)]
    pub fn pop_last(&mut self) -> Option<V> {
        let mut pos = self.set.clone();
        let mut s = vec![];
        while let Some((v, _, hi)) = pos.get() {
            s.push(v);
            pos = hi;
        }
        let last = s.pop();
        self.last = s.last().cloned();
        let holder = pos.manager();
        let mut set = holder.one();
        for value in s.into_iter().rev() {
            set = holder.get_node(value, holder.zero(), set);
        }

        self.set = set;

        last
    }

    #[expect(dead_code)]
    pub fn last(&self) -> Option<V> {
        self.last.clone()
    }

    pub fn first(&self) -> Option<V> {
        self.set.get().map(|(x, _, _)| x)
    }

    pub fn pop_first(&mut self) -> Option<V> {
        if let Some((v, _, hi)) = self.set.get() {
            self.set = hi;
            if self.set.is_one() {
                self.last = None;
            }
            Some(v)
        } else {
            None
        }
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync + Ord> SingleSet<'a, V> {
    pub fn pivot(&self, v: &V) -> PivotedSets<'a, V> {
        let mut pos = self.set.clone();
        let mut lower_vals = vec![];
        while let Some((this_v, _, hi)) = pos.get() {
            if &this_v >= v {
                break;
            }
            lower_vals.push(this_v);
            pos = hi;
        }

        let holder = self.set.manager();
        let mut lower = holder.one();
        let last = lower_vals.last().cloned();
        for x in lower_vals.into_iter().rev() {
            lower = holder.get_node(x, holder.zero(), lower);
        }

        PivotedSets {
            lower: SingleSet { set: lower, last },
            higher_or_equal: SingleSet {
                set: pos,
                last: self.last.clone(),
            },
        }
    }
}

impl<V: Eq + Hash + Ord + Send + Sync + Clone> ZddHolder<V> {
    pub(crate) fn single_set(&self, mut values: BTreeSet<V>) -> SingleSet<'_, V> {
        let mut set = self.one();
        let last = values.last().cloned();
        while let Some(value) = values.pop_last() {
            set = self.get_node(value, self.zero(), set);
        }

        SingleSet { set, last }
    }
}

impl<V: Eq + Hash + Clone + Ord> From<SingleSet<'_, V>> for BTreeSet<V> {
    fn from(value: SingleSet<V>) -> Self {
        let mut set = BTreeSet::new();
        let mut pos = value.set;
        while let Some((v, _, hi)) = pos.get() {
            set.insert(v);
            pos = hi;
        }

        set
    }
}

#[cfg(test)]
mod tests {
    use crate::ZddHolder;
    use std::collections::BTreeSet;

    fn set_of(values: &[i32]) -> BTreeSet<i32> {
        values.iter().copied().collect()
    }

    #[test]
    fn empty_set_is_empty() {
        let holder = ZddHolder::<usize>::new();
        let single = holder.single_set(BTreeSet::new());
        assert!(single.is_empty());
        assert_eq!(single.last(), None);
    }

    #[test]
    fn nonempty_set_is_not_empty() {
        let holder = ZddHolder::new();
        let single = holder.single_set(set_of(&[1, 2, 3]));
        assert!(!single.is_empty());
    }

    #[test]
    fn round_trip_through_btreeset() {
        let holder = ZddHolder::new();
        let original = set_of(&[5, 1, 3, 2, 4]);
        let single = holder.single_set(original.clone());
        let recovered: BTreeSet<i32> = single.into();
        assert_eq!(original, recovered);
    }

    #[test]
    fn last_and_pop_last() {
        let holder = ZddHolder::new();
        let mut set = holder.single_set(set_of(&[5, 1, 3, 2, 4]));
        set.set.check_valid_zdd();
        assert_eq!(set.last(), Some(5));

        assert_eq!(set.pop_last(), Some(5));
        let remaining: BTreeSet<i32> = set.clone().into();
        assert_eq!(remaining, set_of(&[1, 2, 3, 4]));
        set.set.check_valid_zdd();

        assert_eq!(set.pop_last(), Some(4));
        let remaining: BTreeSet<i32> = set.into();
        assert_eq!(remaining, set_of(&[1, 2, 3]));
    }

    #[test]
    fn pop_last_on_empty_returns_none() {
        let holder = ZddHolder::<usize>::new();
        let mut single = holder.single_set(BTreeSet::new());
        assert_eq!(single.pop_last(), None);
        assert!(single.is_empty());
    }

    #[test]
    fn pop_last_repeatedly_drains_set() {
        let holder = ZddHolder::new();
        let mut single = holder.single_set(set_of(&[3, 1, 2]));

        let mut popped = vec![];
        while let Some(v) = single.pop_last() {
            popped.push(v);
        }

        assert_eq!(popped, vec![3, 2, 1]);
        assert!(single.is_empty());
    }

    #[test]
    fn pivot_splits_set_around_value_present_in_set() {
        let holder = ZddHolder::new();
        let single = holder.single_set(set_of(&[1, 2, 3, 4, 5]));

        let pivoted = single.pivot(&3);

        let lower: BTreeSet<i32> = pivoted.lower.into();
        let higher: BTreeSet<i32> = pivoted.higher_or_equal.into();

        assert_eq!(lower, set_of(&[1, 2]));
        assert_eq!(higher, set_of(&[3, 4, 5]));
    }

    #[test]
    fn pivot_splits_set_around_value_not_present() {
        let holder = ZddHolder::new();
        let single = holder.single_set(set_of(&[1, 2, 4, 5]));

        let pivoted = single.pivot(&3);

        let lower: BTreeSet<i32> = pivoted.lower.into();
        let higher: BTreeSet<i32> = pivoted.higher_or_equal.into();

        assert_eq!(lower, set_of(&[1, 2]));
        assert_eq!(higher, set_of(&[4, 5]));
    }

    #[test]
    fn pivot_with_value_lower_than_all_elements() {
        let holder = ZddHolder::new();
        let single = holder.single_set(set_of(&[3, 4, 5]));

        let pivoted = single.pivot(&1);

        let lower: BTreeSet<i32> = pivoted.lower.into();
        let higher: BTreeSet<i32> = pivoted.higher_or_equal.into();

        assert!(lower.is_empty());
        assert_eq!(higher, set_of(&[3, 4, 5]));
    }

    #[test]
    fn pivot_with_value_higher_than_all_elements() {
        let holder = ZddHolder::new();
        let single = holder.single_set(set_of(&[1, 2, 3]));

        let pivoted = single.pivot(&10);

        let lower: BTreeSet<i32> = pivoted.lower.into();
        let higher: BTreeSet<i32> = pivoted.higher_or_equal.into();

        assert_eq!(lower, set_of(&[1, 2, 3]));
        assert!(higher.is_empty());
    }

    #[test]
    fn pivot_on_empty_set() {
        let holder = ZddHolder::new();
        let single = holder.single_set(BTreeSet::new());

        let pivoted = single.pivot(&5);

        assert!(pivoted.lower.is_empty());
        assert!(pivoted.higher_or_equal.is_empty());
    }
}
