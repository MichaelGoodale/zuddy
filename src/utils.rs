use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt::{Display, Write},
    hash::{BuildHasher, Hash},
};

use ahash::HashSetExt;

use crate::manager::{ZddHolder, ZddIndex};
use crate::{SetFamily, algorithms::UsizeOrPositiveInfinity};

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
            seen.extend([lo, hi]);
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
    ///
    ///```
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
