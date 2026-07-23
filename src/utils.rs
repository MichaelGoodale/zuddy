use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt::{Display, Write},
    hash::{BuildHasher, Hash},
};

use ahash::HashSetExt;
mod single_set;
use crate::manager::{ZddHolder, ZddIndex};
use crate::{SetFamily, algorithms::UsizeOrPositiveInfinity};
pub(crate) use single_set::{PivotedSets, SingleSet};

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

impl<V: Eq + Hash + Clone> SetFamily<'_, V> {
    ///Count the number of possible comibinations.
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

impl<V: Eq + Hash + Clone> ZddIndex<V> {
    pub(crate) fn size(self, holder: &ZddHolder<V>) -> UsizeOrPositiveInfinity {
        if self.is_zero() {
            return UsizeOrPositiveInfinity::Size(0);
        }
        if self.is_one() {
            return UsizeOrPositiveInfinity::Size(1);
        }

        if let Some(sum) = holder.sum_cache_get(self) {
            return sum;
        }
        let (lo, hi) = self.children(holder).unwrap();

        let sum = lo.size(holder) + hi.size(holder);

        holder.sum_cache_insert(self, sum)
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
pub mod test {
    use std::collections::{BTreeSet, HashMap};

    use rand::{Rng, RngExt, seq::IndexedRandom};

    use crate::SetFamily;
    use crate::ZddHolder;

    pub fn random_weights(universe: &[char], rng: &mut impl Rng) -> HashMap<char, usize> {
        universe
            .iter()
            .map(|x| (*x, rng.random_range(0..3)))
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
}
