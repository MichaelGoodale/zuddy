use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::{Display, Write},
    hash::{BuildHasher, Hash},
};

use crate::SetFamily;
use crate::manager::{ZddHolder, ZddIndex};

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
    ///too many combinations. In this case, the function will return [`None`]
    ///
    ///# Panics
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn size(&self) -> Option<usize> {
        self.as_raw().size(self.manager)
    }
}

impl<V: Eq + Hash + Clone> ZddIndex<V> {
    pub(crate) fn size(self, holder: &ZddHolder<V>) -> Option<usize> {
        if self.is_zero() {
            return Some(0);
        }
        if self.is_one() {
            return Some(1);
        }

        if let Some(sum) = holder.sum_cache_get(&self) {
            return sum;
        }
        let (lo, hi) = self.children(holder).unwrap();

        let sum = lo
            .size(holder)
            .and_then(|x| hi.size(holder).and_then(|y| x.checked_add(y)));

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
