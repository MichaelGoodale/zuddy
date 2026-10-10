//! Useful utility functions for ZDDs.
//!
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt::{Debug, Display, Write},
    hash::{BuildHasher, Hash},
    ops::{Add, AddAssign},
};

use ahash::{HashMapExt, HashSetExt};
pub mod single_set;
use crate::SetFamily;
use crate::manager::{SizeKey, SizeValue, ZddHolder, ZddIndex};
use single_set::SingleSet;

///Represents a usize, or positive infinity
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum UsizeOrPositiveInfinity {
    ///A usize
    Size(usize),
    ///Positive Infinity
    PositiveInfinity,
}
impl Display for UsizeOrPositiveInfinity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsizeOrPositiveInfinity::Size(x) => write!(f, "{x}"),
            UsizeOrPositiveInfinity::PositiveInfinity => write!(f, "∞"),
        }
    }
}

impl From<UsizeOrPositiveInfinity> for Option<usize> {
    fn from(value: UsizeOrPositiveInfinity) -> Self {
        match value {
            UsizeOrPositiveInfinity::Size(x) => Some(x),
            UsizeOrPositiveInfinity::PositiveInfinity => None,
        }
    }
}

impl Add for UsizeOrPositiveInfinity {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (UsizeOrPositiveInfinity::Size(x), UsizeOrPositiveInfinity::Size(y)) => x
                .checked_add(y)
                .map_or(UsizeOrPositiveInfinity::PositiveInfinity, |z| {
                    UsizeOrPositiveInfinity::Size(z)
                }),
            _ => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }
}

impl AddAssign for UsizeOrPositiveInfinity {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl UsizeOrPositiveInfinity {
    ///Adds a value to a [`UsizeOrPositiveInfinity`], turning to [`UsizeOrPositiveInfinity::PositiveInfinity`] if there is an
    ///overflow.
    #[must_use]
    pub fn add_usize(self, x: usize) -> Self {
        match self {
            UsizeOrPositiveInfinity::Size(s) => s
                .checked_add(x)
                .map_or(UsizeOrPositiveInfinity::PositiveInfinity, |z| {
                    UsizeOrPositiveInfinity::Size(z)
                }),
            UsizeOrPositiveInfinity::PositiveInfinity => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }

    ///Take a [`UsizeOrPositiveInfinity`] and unwrap it, assuming it is
    ///[`UsizeOrPositiveInfinity::Size`]
    ///
    ///# Panics
    ///Will panic if this is [`UsizeOrPositiveInfinity::PositiveInfinity`]
    #[must_use]
    pub fn unwrap(self) -> usize {
        match self {
            UsizeOrPositiveInfinity::Size(x) => x,
            UsizeOrPositiveInfinity::PositiveInfinity => panic!("Size is infinite!"),
        }
    }
}

impl<'a, V: Display + Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///Returns the [`SetFamily`] as a string with a [Graphviz](https://graphviz.org/) formatted graph
    ///
    ///# Panics
    ///
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn graphviz(&self) -> String {
        let extra = HashMap::new();
        self.graphviz_with_extra::<char, _>(&extra)
    }

    ///Returns the [`SetFamily`] as a string with a [Graphviz](https://graphviz.org/) formatted graph
    ///
    ///This includes extra data in `extra` which can be associated with each [`SetFamily`].
    ///
    ///# Panics
    ///
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn graphviz_with_extra<T: Display, S: BuildHasher>(
        &self,
        extra: &HashMap<SetFamily<'a, V>, T, S>,
    ) -> String {
        let mut s = String::new();
        writeln!(s, "digraph DAG {{\n  node [ordering=\"out\"];").unwrap();
        let mut q = VecDeque::from([self.as_raw()]);
        let mut nodes = BTreeMap::new();
        let mut seen: BTreeSet<_> = BTreeSet::new();
        let mut edges = vec![];

        while let Some(x) = q.pop_front() {
            if seen.contains(&x) {
                continue;
            }

            if x.is_zero() {
                nodes.insert(x, "⊥".to_string());
                continue;
            }
            if x.is_one() {
                nodes.insert(x, "⊤".to_string());
                continue;
            }
            let (value, lo, hi) = x.get(self.manager).unwrap();
            nodes.insert(x, value.to_string());
            edges.extend([(x, lo, "dashed"), (x, hi, "solid")]);
            q.extend([lo, hi].into_iter().filter(|x| !seen.contains(x)));
            seen.insert(x);
        }

        for (n, i) in nodes {
            if let Some(x) = extra.get(&SetFamily::from_set_family(n, self.manager)) {
                writeln!(s, "  {} [label=\"{} ({})\"];", usize::from(n), i, x).unwrap();
            } else {
                writeln!(s, "  {} [label=\"{}\"];", usize::from(n), i).unwrap();
            }
        }

        for (src, end, style) in edges {
            writeln!(
                s,
                "  {} -> {} [style={}];",
                usize::from(src),
                usize::from(end),
                style
            )
            .unwrap();
        }

        writeln!(s, "}}").unwrap();
        s
    }
}

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    ///Converts this [`SetFamily`] into a [`SetFamily`] of `Y`s associated with `holder`, by applying
    ///`f` to every value.
    ///
    ///This is a structural conversion: the shape of the underlying ZDD is preserved, so no sets
    ///are materialised.
    ///
    ///`f` must be injective and preserve the ordering of the ZDD's values (i.e. whenever a value
    ///`v` appears strictly below another value `w` in the ZDD, `f(v) < f(w)`).
    ///
    ///# Panics
    ///Will panic if `f` breaks the ordering of the ZDD (i.e. if a parent's mapped value is not
    ///strictly greater than its children's mapped values).
    ///
    ///```rust
    ///# use zuddy::{ZddHolder, SetFamily, utils::UsizeOrPositiveInfinity};
    ///# use std::collections::BTreeSet;
    ///let holder = ZddHolder::<char>::new();
    ///let target = ZddHolder::<u8>::new();
    ///let sets = ["ab", "b"].into_iter().map(|x| x.chars().collect::<BTreeSet<_>>()).collect::<BTreeSet<_>>();
    ///let zdd = SetFamily::from_sets(sets, &holder);
    ///let converted = zdd.convert(|c| c as u8, &target);
    ///assert_eq!(converted.size(), UsizeOrPositiveInfinity::Size(2));
    ///```
    #[must_use]
    pub fn convert<Y: Eq + Hash + Clone + Ord + Send + Sync>(
        self,
        f: impl Fn(V) -> Y,
        holder: &'a ZddHolder<Y>,
    ) -> SetFamily<'a, Y> {
        let mut mapping = ahash::HashMap::<ZddIndex<V>, SetFamily<Y>>::new();
        mapping.insert(ZddIndex::ZERO, holder.zero());
        mapping.insert(ZddIndex::ONE, holder.one());
        let mut stack = vec![self.as_raw()];
        while let Some(x) = stack.pop() {
            if mapping.contains_key(&x) {
                continue;
            }
            let Some((value, lo, hi)) = x.get(self.manager()) else {
                continue;
            };
            if !mapping.contains_key(&lo) {
                stack.push(x);
                stack.push(lo);
                stack.push(hi);
                continue;
            }
            if !mapping.contains_key(&hi) {
                stack.push(x);
                stack.push(hi);
                continue;
            }
            let new = holder.zdd_node(f(value), mapping[&lo].clone(), mapping[&hi].clone());
            mapping.insert(x, new);
        }
        mapping[&self.as_raw()].clone()
    }
}

impl<V: Eq + Hash + Clone> SetFamily<'_, V> {
    ///Count the number of possible combinations.
    ///
    ///Due to the combinatorial nature of ZDDs, if you have a sufficiently big ZDD, there will be
    ///too many combinations. In this case, the function will return [`UsizeOrPositiveInfinity::PositiveInfinity`]
    ///
    ///# Panics
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn size(&self) -> UsizeOrPositiveInfinity {
        self.as_raw().size(self.manager)
    }

    ///Returns the universe of elements in this ZDD (e.g. any node that is in any set).
    ///
    ///```
    ///# use zuddy::{ZddHolder, SetFamily};
    ///# use std::collections::{HashSet, BTreeSet};
    ///let holder = ZddHolder::<char>::new();
    ///let sets = ["a", "bc", "cdefa", "bde"].into_iter().map(|x| x.chars().collect::<BTreeSet<_>>()).collect::<BTreeSet<_>>();
    ///let zdd = SetFamily::from_sets(sets, &holder);
    ///assert_eq!(zdd.universe(), "abcdef".chars().collect::<HashSet<_>>());
    ///```
    #[must_use]
    pub fn universe<S: BuildHasher + Default>(&self) -> HashSet<V, S> {
        let mut stack = vec![self.as_raw()];
        let mut seen = HashSet::<ZddIndex<V>, ahash::RandomState>::default();
        let mut nodes = HashSet::<V, S>::new();

        while let Some(x) = stack.pop() {
            if !seen.contains(&x)
                && let Some((v, lo, hi)) = x.get(self.manager())
            {
                seen.insert(x);
                nodes.insert(v);
                stack.extend([lo, hi].into_iter().filter(|x| !seen.contains(x)));
            }
        }
        nodes
    }
}
impl<'a, V: Eq + Hash + Clone + Ord + Send + Sync> SetFamily<'a, V> {
    pub(crate) fn universe_single_set(&self) -> SingleSet<'a, V> {
        let mut stack = vec![self.as_raw()];
        let mut seen = HashSet::<ZddIndex<V>, ahash::RandomState>::default();
        let mut nodes = BTreeSet::new();

        while let Some(x) = stack.pop() {
            if !seen.contains(&x)
                && let Some((v, lo, hi)) = x.get(self.manager())
            {
                seen.insert(x);
                nodes.insert(v);
                stack.extend([lo, hi].into_iter().filter(|x| !seen.contains(x)));
            }
        }
        self.manager().single_set(nodes)
    }
}

impl<V: Eq + Hash + Clone> ZddIndex<V> {
    pub(crate) fn size(self, holder: &ZddHolder<V>) -> UsizeOrPositiveInfinity {
        if self.is_zero() {
            return UsizeOrPositiveInfinity::Size(0);
        }
        if self.is_one() {
            return UsizeOrPositiveInfinity::Size(1);
        }

        if let Some(SizeValue::Size(sum)) = holder.size_cache_get(&SizeKey::Size(self)) {
            return sum;
        }
        let (lo, hi) = self.children(holder).unwrap();

        let sum = lo.size(holder) + hi.size(holder);

        holder
            .size_cache_insert(SizeKey::Size(self), SizeValue::Size(sum))
            .unwrap_size()
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///Creates a singleton set from a value.
    ///```
    ///use zuddy::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', &holder);
    /// assert_eq!(a.members().collect::<Vec<_>>(), vec![vec!['a']]);
    ///```
    #[must_use]
    pub fn singleton(value: V, holder: &'a ZddHolder<V>) -> SetFamily<'a, V> {
        holder.get_node(value, holder.zero(), holder.one())
    }
}

#[cfg(test)]
#[expect(missing_docs, clippy::missing_panics_doc)]
pub mod test {
    use std::collections::{BTreeSet, HashMap};

    use rand::{Rng, RngExt, seq::IndexedRandom};

    use crate::SetFamily;
    use crate::ZddHolder;
    use crate::utils::UsizeOrPositiveInfinity;

    pub fn random_weights(universe: &[char], rng: &mut impl Rng) -> HashMap<char, usize> {
        universe
            .iter()
            .map(|x| (*x, rng.random_range(0..4)))
            .collect()
    }

    pub fn random_isize_weights(universe: &[char], rng: &mut impl Rng) -> HashMap<char, isize> {
        universe
            .iter()
            .map(|x| {
                let w: u8 = rng.random_range(0..10);

                (*x, isize::from(w) - 5)
            })
            .collect()
    }

    pub fn random_family(universe: &[char], rng: &mut impl Rng) -> BTreeSet<BTreeSet<char>> {
        let n_sets = rng.random_range(0..10);
        let mut sets = BTreeSet::new();
        for _ in 0..n_sets {
            let size = rng.random_range(0..universe.len());
            let set = universe.sample(rng, size).copied().collect::<BTreeSet<_>>();
            sets.insert(set);
        }
        sets
    }

    impl SetFamily<'_, char> {
        pub(crate) fn as_string(&self) -> String {
            let mut members = self
                .members()
                .map(|x| x.into_iter().map(|x| x.to_string()).collect::<String>())
                .collect::<Vec<_>>();
            members.sort();
            members.join(" ")
        }
    }

    #[must_use]
    pub fn str_to_sets(s: &str) -> BTreeSet<BTreeSet<char>> {
        if s.is_empty() {
            return BTreeSet::default();
        }

        s.split(' ')
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>()
    }

    ///Allows for easy testing of operations, taking family of sets of chars as strings seperated
    ///by spaces, with `res` being the intended result with the operand supplied by `op`
    pub fn test_op<
        F: for<'a> Fn(SetFamily<'a, char>, SetFamily<'a, char>) -> SetFamily<'a, char>,
    >(
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
        assert_eq!(a.size().unwrap(), a_set_len);
        a.check_valid_zdd();
        assert_eq!(b.size().unwrap(), b_set_len);
        b.check_valid_zdd();

        let result = op(a, b);
        result.check_valid_zdd();

        let result_recon: BTreeSet<BTreeSet<char>> =
            result.members().map(|x| x.into_iter().collect()).collect();
        assert_eq!(result_recon, a_op_b);
    }

    ///Allows for easy testing of operations, taking family of sets of chars as strings seperated
    ///by spaces, with `res` being the intended result with the operand supplied by `op`
    pub fn test_solo_op<F: for<'a> Fn(SetFamily<'a, char>) -> SetFamily<'a, char>>(
        a: &str,
        res: &str,
        op: F,
        op_name: &'static str,
        holder: &ZddHolder<char>,
    ) {
        let a_sets = str_to_sets(a);
        let a_op_b = str_to_sets(res);
        println!("{a_sets:?} {op_name} = {a_op_b:?}");
        let a_set_len = a_sets.len();

        let a = SetFamily::from_sets(a_sets, holder);
        assert_eq!(a.size().unwrap(), a_set_len);
        a.check_valid_zdd();

        let result = op(a);
        result.check_valid_zdd();

        let result_recon: BTreeSet<BTreeSet<char>> =
            result.members().map(|x| x.into_iter().collect()).collect();
        assert_eq!(result_recon, a_op_b);
    }

    ///Allows for easy testing of operations, taking family of sets of chars as strings seperated
    ///by spaces, with `res` being the intended result with the operand supplied by `op`
    pub fn test_single_op<F: for<'a> Fn(SetFamily<'a, char>, char) -> SetFamily<'a, char>>(
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
        assert_eq!(a.size().unwrap(), start_len);

        println!("{}", a.graphviz());

        let mut result = a.clone();
        for action in actions {
            result = op(result, action);
            println!("{}", result.graphviz());
            result.check_valid_zdd();
        }

        result.check_valid_zdd();
        let result_recon: BTreeSet<BTreeSet<char>> =
            result.members().map(|x| x.into_iter().collect()).collect();

        assert_eq!(result_recon, intended);
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
    fn convert_matches_btreeset_roundtrip() {
        let holder = ZddHolder::<char>::new();
        let target = ZddHolder::<u8>::new();
        let sets = ["abcd", "ac", "a", "bc", "b", "c"];
        let x = sets
            .iter()
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();
        let zdd = SetFamily::from_sets(x.clone(), &holder);
        let converted = zdd.convert(|c| c as u8, &target);
        let mut actual: Vec<Vec<u8>> = converted.members().map(|x| x).collect();
        actual.sort();
        let mut expected: Vec<Vec<u8>> = sets
            .iter()
            .map(|x| x.chars().map(|c| c as u8).collect::<Vec<u8>>())
            .collect();
        expected.sort();
        assert_eq!(actual, expected);
    }
    #[test]
    #[should_panic(expected = "violating the ZDD definition")]
    fn convert_with_order_breaking_map_panics() {
        let holder = ZddHolder::<char>::new();
        let target = ZddHolder::<char>::new();
        let sets = ["ab", "b"];
        let x = sets
            .iter()
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();
        let zdd = SetFamily::from_sets(x, &holder);
        let _converted = zdd.convert(|_| 'x', &target);
    }
}
