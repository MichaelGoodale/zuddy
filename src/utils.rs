use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::{Display, Write},
};

use super::{SetFamily, Zdd, ZddHolder};

impl<V: Display> SetFamily<V> {
    ///Returns the [`SetFamily`] as a string with a [Graphviz](https://graphviz.org/) formatted graph
    ///
    ///# Panics
    ///
    ///Will panic if `self` is not a valid ZDD in [`ZddHolder`]
    #[must_use]
    pub fn graphviz(&self, holder: &ZddHolder<V>) -> String {
        let mut s = String::new();
        writeln!(s, "digraph DAG {{\n  node [ordering=\"out\"];").unwrap();
        let mut q = VecDeque::from([self]);
        let mut nodes = BTreeMap::new();
        let mut seen: BTreeSet<&SetFamily<_>> = BTreeSet::new();
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
            let Zdd { value, lo, hi } = holder.data[x.0].as_ref().unwrap();
            nodes.insert(x, value.to_string());
            edges.extend([(x, lo, "dashed"), (x, hi, "solid")]);
            q.extend([lo, hi].into_iter().filter(|x| !seen.contains(x)));
            seen.extend([lo, hi]);
        }

        for (n, i) in nodes {
            writeln!(s, "  {} [label=\"{}\"];", n.0, i).unwrap();
        }

        for (src, end, style) in edges {
            writeln!(s, "  {} -> {} [style={}];", src.0, end.0, style).unwrap();
        }

        writeln!(s, "}}").unwrap();
        s
    }
}
