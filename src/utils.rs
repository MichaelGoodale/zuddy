use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::{Display, Write},
    hash::{BuildHasher, Hash},
};

use super::{SetFamily, Zdd, ZddHolder};

impl<V: Display + Eq + Hash> SetFamily<V> {
    ///Returns the [`SetFamily`] as a string with a [Graphviz](https://graphviz.org/) formatted graph
    ///
    ///# Panics
    ///
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn graphviz(&self, holder: &ZddHolder<V>) -> String {
        let extra = HashMap::new();
        self.graphviz_with_extra::<char, _>(&extra, holder)
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
        extra: &HashMap<SetFamily<V>, T, S>,
        holder: &ZddHolder<V>,
    ) -> String {
        let mut s = String::new();
        writeln!(s, "digraph DAG {{\n  node [ordering=\"out\"];").unwrap();
        let mut q = VecDeque::from([self]);
        let mut nodes = BTreeMap::new();
        let mut seen: BTreeSet<&SetFamily<_>> = BTreeSet::new();
        let mut edges = vec![];

        let data = holder.data.read().unwrap();
        while let Some(x) = q.pop_front() {
            if x.is_zero() {
                nodes.insert(x, "⊥".to_string());
                continue;
            }
            if x.is_one() {
                nodes.insert(x, "⊤".to_string());
                continue;
            }
            let Zdd { value, lo, hi } = data[x.0].as_ref().unwrap();
            nodes.insert(x, value.to_string());
            edges.extend([(x, lo, "dashed"), (x, hi, "solid")]);
            q.extend([lo, hi].into_iter().filter(|x| !seen.contains(x)));
            seen.extend([lo, hi]);
        }

        for (n, i) in nodes {
            if let Some(x) = extra.get(n) {
                writeln!(s, "  {} [label=\"{} ({})\"];", n.0, i, x).unwrap();
            } else {
                writeln!(s, "  {} [label=\"{}\"];", n.0, i).unwrap();
            }
        }

        for (src, end, style) in edges {
            writeln!(s, "  {} -> {} [style={}];", src.0, end.0, style).unwrap();
        }

        writeln!(s, "}}").unwrap();
        s
    }
}

impl<V: Eq + Hash> SetFamily<V> {
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
        let (lo, hi) = self.children(holder).unwrap();

        let sum = lo
            .size(holder)
            .and_then(|x| hi.size(holder).and_then(|y| x.checked_add(y)));

        holder.sum_cache.insert(*self, sum);
        sum
    }
}

impl<V: Eq + Hash + Clone> SetFamily<V> {
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
        holder.get_node_seq(Zdd {
            value,
            lo: SetFamily::ZERO,
            hi: SetFamily::ONE,
        })
    }
}
