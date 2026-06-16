use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::{Display, Write},
    hash::{BuildHasher, Hash},
    marker::PhantomData,
};

use crate::SetFamily;

use super::{RawZdd, Zdd, ZddHolder};

impl<'a, V: Display + Eq + Hash> SetFamily<'a, V> {
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
        let mut q = VecDeque::from([RawZdd(self.id, PhantomData)]);
        let mut nodes = BTreeMap::new();
        let mut seen: BTreeSet<&RawZdd<_>> = BTreeSet::new();
        let mut edges = vec![];

        let data = self.manager.data.read().unwrap();
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
            if let Some(x) = extra.get(&SetFamily::from_set_family(n, self.manager)) {
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

impl<V: Eq + Hash> SetFamily<'_, V> {
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

impl<V: Eq + Hash> RawZdd<V> {
    pub(crate) fn size(self, holder: &ZddHolder<V>) -> Option<usize> {
        if self.is_zero() {
            return Some(0);
        }
        if self.is_one() {
            return Some(1);
        }

        if let Some(sum) = holder.sum_cache.get(&self) {
            return *sum;
        }
        let (lo, hi) = self.children(holder).unwrap();

        let sum = lo
            .size(holder)
            .and_then(|x| hi.size(holder).and_then(|y| x.checked_add(y)));

        holder.sum_cache.insert(self, sum);
        sum
    }
}

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
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
        SetFamily::from_set_family(
            holder.get_node_seq(Zdd {
                value,
                lo: RawZdd::ZERO,
                hi: RawZdd::ONE,
            }),
            holder,
        )
    }
}
