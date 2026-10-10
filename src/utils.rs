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
    ///Remap a [`SetFamily<X>`] to a [`SetFamily<Y>`] using `f`, a function which maps values of `X`
    ///to `Y`. `f` can be any function, it needn't be injective.
    ///
    ///# Panics
    ///Will panic if `self` is not a valid ZDD in its [`ZddHolder`]
    ///    ///```rust
    ///# use zuddy::{ZddHolder, SetFamily};
    ///# use std::collections::{BTreeSet, HashMap};
    ///let f: HashMap<_, _> = [('a', 3), ('b', 1), ('c', 2)].into();
    ///let holder = ZddHolder::<char>::new();
    ///let target = ZddHolder::<usize>::new();
    ///let sets = ["abc", "c"].into_iter().map(|x| x.chars().collect::<BTreeSet<_>>()).collect::<BTreeSet<_>>();
    ///let zdd = SetFamily::from_sets(sets, &holder);
    ///let mapped = zdd.map(|c| f[&c], &target);
    ///let actual: BTreeSet<BTreeSet<usize>> = mapped.members().map(BTreeSet::from_iter).collect();
    ///let expected: BTreeSet<BTreeSet<usize>> = [vec![3, 1, 2], vec![2]].into_iter().map(BTreeSet::from_iter).collect();
    ///assert_eq!(actual, expected);
    ///```
    #[must_use]
    pub fn map<Y: Eq + Hash + Clone + Ord + Send + Sync>(
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
            let (value, lo, hi) = x.get(self.manager()).expect("Invalid index");
            if !mapping.contains_key(&lo) || !mapping.contains_key(&hi) {
                stack.extend([x, lo, hi]);
                continue;
            }
            let with_value = mapping[&hi].clone().insert(f(value));
            let new = mapping[&lo].clone().union(with_value);
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
    fn map_with_non_injective_map_deduplicates() {
        let holder = ZddHolder::<char>::new();
        let target = ZddHolder::<char>::new();
        let sets = ["ab", "b"]
            .iter()
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();
        let zdd = SetFamily::from_sets(sets, &holder);
        let mapped = zdd.map(|_| 'x', &target);
        let actual: BTreeSet<String> = mapped
            .members()
            .map(|x| x.into_iter().collect())
            .collect();
        assert_eq!(actual, BTreeSet::from(["x".to_string()]));
    }
    #[test]
    fn map_with_order_reversing_map() {
        let holder = ZddHolder::<char>::new();
        let target = ZddHolder::<std::cmp::Reverse<char>>::new();
        let sets = ["abc", "c"]
            .iter()
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();
        let zdd = SetFamily::from_sets(sets, &holder);
        let mapped = zdd.map(std::cmp::Reverse, &target);
        let actual: BTreeSet<BTreeSet<std::cmp::Reverse<char>>> =
            mapped.members().map(BTreeSet::from_iter).collect();
        let expected: BTreeSet<_> = ["cba", "c"]
            .iter()
            .map(|x| x.chars().map(std::cmp::Reverse).collect())
            .collect();
        assert_eq!(actual, expected);
    }
    #[test]
    fn map_random_families_with_random_mappings() {
        let universe: Vec<char> = "abcdef".chars().collect();
        let mut rng = rand::rng();
        for _ in 0..100 {
            let sets = random_family(&universe, &mut rng);
            let holder = ZddHolder::<char>::new();
            let target = ZddHolder::<u8>::new();
            let zdd = SetFamily::from_sets(sets.clone(), &holder);
            let weights: HashMap<char, u8> = universe
                .iter()
                .map(|x| (*x, rng.random_range(0..4)))
                .collect();
            let mapped = zdd.map(|c| weights[&c], &target);
            let actual: BTreeSet<BTreeSet<u8>> =
                mapped.members().map(BTreeSet::from_iter).collect();
            let expected: BTreeSet<BTreeSet<u8>> = sets
                .iter()
                .map(|x| x.iter().map(|c| weights[c]).collect())
                .collect();
            assert_eq!(actual, expected);
        }
    }
}
