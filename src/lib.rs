//! Zudd is a crate for handling ZDDs
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::{Debug, Display, Write},
    hash::Hash,
    marker::PhantomData,
};

///A representation of a family of sets (or otherwise a set of sets).
///
///It is always connected to a particular [`ZddHolder`] which holds the actual memory.
///
///
#[derive(Debug)]
pub struct SetFamily<V>(usize, PhantomData<V>);

impl<V> Copy for SetFamily<V> {}

impl<V> Clone for SetFamily<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V> PartialEq for SetFamily<V> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<V> Hash for SetFamily<V> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<V> Eq for SetFamily<V> {}

impl<V> PartialOrd for SetFamily<V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V> Ord for SetFamily<V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<V> SetFamily<V> {
    const ZERO: Self = SetFamily(0, PhantomData);
    const ONE: Self = SetFamily(1, PhantomData);
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Zdd<V> {
    value: V,
    lo: SetFamily<V>,
    hi: SetFamily<V>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
enum Operations<V> {
    Change(SetFamily<V>, V),
    Union(SetFamily<V>, SetFamily<V>),
}

impl<V> SetFamily<V> {
    fn is_zero(self) -> bool {
        self == SetFamily::ZERO
    }

    fn is_one(self) -> bool {
        self == SetFamily::ONE
    }
}

///A simple iterator over the members of the ZDD.
///May not be very memory efficient.
pub struct ZDDIter<'a, V> {
    stack: Vec<(SetFamily<V>, Vec<&'a V>)>,
    holder: &'a ZddHolder<V>,
}

impl<'a, V: Hash + Ord + Eq + Clone + Debug> Iterator for ZDDIter<'a, V> {
    type Item = Vec<&'a V>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, current_set)) = self.stack.pop() {
            if node.is_zero() {
                continue;
            }
            if node.is_one() {
                return Some(current_set);
            }

            let (v, lo, hi) = node.get(self.holder).unwrap();

            if !lo.is_zero() {
                self.stack.push((lo, current_set.clone()));
            }
            if !hi.is_zero() {
                let mut hi_set = current_set;
                hi_set.push(v);
                self.stack.push((hi, hi_set));
            }
        }
        None
    }
}

impl<V: Hash + Ord + Eq + Clone + Debug> SetFamily<V> {
    ///Creates a [`SetFamily`] from a [`BTreeSet<BTreeSet<V>>`].
    ///
    ///```
    ///use zudd::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///let sets = ["abcd", "ac", "a", "bc", "b", "c"];
    ///let x = sets.iter().map(|x| x.chars().collect()).collect();
    ///let z = SetFamily::from_sets(x, &mut holder);
    ///let members: Vec<String> = z.members(&mut holder).map(|x| x.into_iter().collect()).collect();
    ///assert_eq!(members, sets);
    ///```
    pub fn from_sets(mut sets: BTreeSet<BTreeSet<V>>, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        if sets.is_empty() {
            return SetFamily::ZERO;
        }

        #[expect(clippy::missing_panics_doc)]
        if sets.len() == 1 && sets.first().unwrap().is_empty() {
            return SetFamily::ONE;
        }

        //fine since at least one set will be non-empty since if it was only the empty set it would have been caught before.
        #[expect(clippy::missing_panics_doc)]
        let value = sets.iter().filter_map(|x| x.first()).min().unwrap().clone();

        let with_min_val = sets
            .extract_if(.., |v| v.contains(&value))
            .map(|mut x| {
                x.remove(&value);
                x
            })
            .collect::<BTreeSet<_>>();

        let without_min_val = sets;

        let lo = SetFamily::from_sets(without_min_val, holder);
        let hi = SetFamily::from_sets(with_min_val, holder);

        holder.get_node(Zdd { value, lo, hi })
    }

    ///Creates a ZDD with all combinations that don't include [`value`]
    #[must_use]
    pub fn offset(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        todo!()
    }

    ///Creates a ZDD with all combinations that include [`value`] and then deletes [`value`] from those
    ///combinations.
    #[must_use]
    pub fn onset(self, value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        todo!()
    }

    ///The intersection of [`self`] and [`other`]
    #[must_use]
    pub fn intersect(self, other: Self, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        todo!()
    }

    ///The set difference of [`self`] and [`other`]
    #[must_use]
    pub fn difference(self, other: Self, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        todo!()
    }

    ///Creates a singleton set from a value.
    ///```
    ///use zudd::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', &mut holder);
    /// assert_eq!(a.members(&holder).collect::<Vec<_>>(), vec![vec![&'a']]);
    ///```
    #[must_use]
    pub fn singleton(value: V, holder: &mut ZddHolder<V>) -> SetFamily<V> {
        holder.get_node(Zdd {
            value,
            lo: SetFamily::ZERO,
            hi: SetFamily::ONE,
        })
    }

    ///Returns a [`ZDDIter`] to iterate over all the valid combinations in this family.
    #[must_use]
    pub fn members(self, holder: &ZddHolder<V>) -> ZDDIter<'_, V> {
        ZDDIter {
            stack: vec![(self, Vec::new())],
            holder,
        }
    }

    ///Inverts whether a value is included or not included on each combination in the family.
    ///
    ///```
    ///use zudd::{ZddHolder, SetFamily};
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

    fn get(self, holder: &ZddHolder<V>) -> Option<(&V, SetFamily<V>, SetFamily<V>)> {
        holder.data[self.0].as_ref().map(|x| (&x.value, x.lo, x.hi))
    }

    ///Takes the union of two families of sets.
    ///
    ///
    ///```
    ///use zudd::{ZddHolder, SetFamily};
    ///let mut holder = ZddHolder::<char>::new();
    ///
    /// let a = SetFamily::singleton('a', &mut holder);
    /// let b = SetFamily::singleton('b', &mut holder);
    /// let c = SetFamily::singleton('c', &mut holder);
    /// assert_eq!(a.union(b, &mut holder).count(&mut holder), Some(2));
    /// assert_eq!(a.union(b, &mut holder).union(c, &mut holder).count(&mut holder), Some(3));
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
    ///Will panic if [`self`] is not a valid ZDD in [`ZddHolder`]
    pub fn count(&self, holder: &mut ZddHolder<V>) -> Option<usize> {
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
            .count(holder)
            .and_then(|x| hi.count(holder).and_then(|y| x.checked_add(y)));

        holder.sum_cache.insert(*self, sum);
        sum
    }
}

impl<V: Display> SetFamily<V> {
    ///Returns the [`SetFamily`] as a string with a [Graphviz](https://graphviz.org/) formatted graph
    ///
    ///# Panics
    ///
    ///Will panic if [`self`] is not a valid ZDD in [`ZddHolder`]
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

#[derive(Debug)]
///An arena for storing the data associated with different [`SetFamily`]s.
pub struct ZddHolder<V> {
    free: Vec<usize>,
    data: Vec<Option<Zdd<V>>>,
    uniq_table: HashMap<Zdd<V>, SetFamily<V>>,
    cache: HashMap<Operations<V>, SetFamily<V>>,
    sum_cache: HashMap<SetFamily<V>, Option<usize>>,
}

fn free_id<V>(data: &mut Vec<Option<Zdd<V>>>, free: &mut Vec<usize>) -> SetFamily<V> {
    if let Some(x) = free.pop() {
        SetFamily(x, PhantomData)
    } else {
        data.push(None);
        SetFamily(data.len() - 1, PhantomData)
    }
}

impl<V> Default for ZddHolder<V> {
    fn default() -> Self {
        Self {
            free: vec![],
            data: vec![None, None],
            uniq_table: HashMap::default(),
            sum_cache: HashMap::default(),
            cache: HashMap::default(),
        }
    }
}

impl<V: Eq + Hash + Clone> ZddHolder<V> {
    ///Create a new [`ZddHolder`] to hold various ZDDs.
    #[must_use]
    pub fn new() -> ZddHolder<V> {
        ZddHolder::default()
    }

    fn get_node(&mut self, family: Zdd<V>) -> SetFamily<V> {
        if family.hi == SetFamily::ZERO {
            return family.lo;
        }

        if let Some(x) = self.uniq_table.get(&family) {
            return *x;
        }
        let id = free_id(&mut self.data, &mut self.free);
        self.data[id.0] = Some(family.clone());
        self.uniq_table.insert(family, id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton() {
        let mut holder = ZddHolder::<char>::default();
        let a = SetFamily::singleton('a', &mut holder);
        let b = SetFamily::singleton('b', &mut holder);
        let c = SetFamily::singleton('c', &mut holder);

        for x in [a, b, c] {
            assert_eq!(x.count(&mut holder).unwrap(), 1);
            println!("{}", x.graphviz(&holder));
        }

        let ab = a.change('b', &mut holder);
        assert_eq!(ab.count(&mut holder).unwrap(), 1);

        let ab_a = ab.union(a, &mut holder);

        println!("{}", ab_a.graphviz(&holder));
        assert_eq!(ab.union(a, &mut holder).count(&mut holder).unwrap(), 2);
        assert_eq!(
            ab.union(a, &mut holder)
                .union(b, &mut holder)
                .count(&mut holder)
                .unwrap(),
            3
        );
        assert_eq!(
            ab.union(a, &mut holder)
                .union(b, &mut holder)
                .union(c, &mut holder)
                .count(&mut holder)
                .unwrap(),
            4
        );
    }
}
