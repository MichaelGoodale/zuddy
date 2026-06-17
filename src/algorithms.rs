//! Defines various miscellaneous algorithms over [`SetFamily`]
//!
//! ## Finding minimal subsets according to summed weights:
//!  - [`SetFamily::minimal_set_size`]
//!  - [`SetFamily::minimal_sets`]
//!  - [`SetFamily::only_minimal_sets`]
use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
};

use crate::{SetFamily, manager::ZddIndex};

enum OptimizationFrame<V> {
    Search(ZddIndex<V>),
    Climb(ZddIndex<V>),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
enum UsizeOrPositiveInfinity {
    Size(usize),
    PositiveInfinity,
}

impl From<UsizeOrPositiveInfinity> for Option<usize> {
    fn from(value: UsizeOrPositiveInfinity) -> Self {
        match value {
            UsizeOrPositiveInfinity::Size(x) => Some(x),
            UsizeOrPositiveInfinity::PositiveInfinity => None,
        }
    }
}

impl UsizeOrPositiveInfinity {
    fn add(self, x: usize) -> Self {
        match self {
            UsizeOrPositiveInfinity::Size(s) => UsizeOrPositiveInfinity::Size(x + s),
            UsizeOrPositiveInfinity::PositiveInfinity => UsizeOrPositiveInfinity::PositiveInfinity,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Hash)]
struct MinWeightCost {
    min_set_weight: UsizeOrPositiveInfinity,
    element_weight: usize,
}

impl Display for MinWeightCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "f(e)={}, min_set_weight=", self.element_weight)?;
        match self.min_set_weight {
            UsizeOrPositiveInfinity::Size(x) => write!(f, "{x}"),
            UsizeOrPositiveInfinity::PositiveInfinity => write!(f, "+∞"),
        }
    }
}

impl MinWeightCost {
    const INFINITY: Self = MinWeightCost {
        min_set_weight: UsizeOrPositiveInfinity::PositiveInfinity,
        element_weight: 0,
    };
    const ZERO: Self = MinWeightCost {
        min_set_weight: UsizeOrPositiveInfinity::Size(0),
        element_weight: 0,
    };
}

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    fn minimal_set_inner<F: Fn(&V) -> usize>(&self, f: F) -> HashMap<ZddIndex<V>, MinWeightCost> {
        let mut minimum_cost: HashMap<ZddIndex<V>, MinWeightCost> = HashMap::default();
        //None here is semantically positive infinity
        minimum_cost.insert(ZddIndex::ZERO, MinWeightCost::INFINITY);
        minimum_cost.insert(ZddIndex::ONE, MinWeightCost::ZERO);
        let mut stack = vec![OptimizationFrame::Search(self.as_raw())];

        while let Some(x) = stack.pop() {
            match x {
                OptimizationFrame::Search(this) => {
                    let (lo, hi) = this.children(self.manager).unwrap();
                    stack.push(OptimizationFrame::Climb(this));
                    stack.extend([lo, hi].into_iter().filter_map(|k| {
                        if minimum_cost.contains_key(&k) {
                            None
                        } else {
                            Some(OptimizationFrame::Search(k))
                        }
                    }));
                }
                OptimizationFrame::Climb(this) => {
                    let (v, lo, hi) = this.get(self.manager).unwrap();
                    let lo_w = minimum_cost.get(&lo).unwrap().min_set_weight;

                    let element_weight = f(&v);
                    let hi_w = minimum_cost
                        .get(&hi)
                        .unwrap()
                        .min_set_weight
                        .add(element_weight);

                    let min_set_weight = hi_w.min(lo_w);

                    minimum_cost.insert(
                        this,
                        MinWeightCost {
                            min_set_weight,
                            element_weight,
                        },
                    );
                }
            }
        }
        minimum_cost
    }

    /// Gets the size of the smallest set by summed weights where node values are weighted by the closure in `f`.
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    pub fn minimal_set_size<F: Fn(&V) -> usize>(&self, f: F) -> Option<usize> {
        if self.is_zero() {
            return None;
        }

        if self.is_one() {
            return Some(0);
        }

        let minimum_cost = self.minimal_set_inner(f);

        minimum_cost
            .get(&self.as_raw())
            .unwrap()
            .min_set_weight
            .into()
    }

    /// Returns a [`MinimalSetIterator`] which iterates over the minimal sets by summed weight of the family.
    /// The weight is calculated by the provided closure.
    ///
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    pub fn minimal_sets<F: Fn(&V) -> usize>(&self, f: F) -> MinimalSetIterator<'a, V> {
        let minimum_cost_lookup = self.minimal_set_inner(f);
        let min_cost = minimum_cost_lookup
            .get(&self.as_raw())
            .unwrap()
            .min_set_weight;

        MinimalSetIterator {
            stack: vec![(self.as_raw(), (vec![], 0))],
            minimum_cost_lookup,
            root: self.clone(),
            min_cost,
        }
    }
}

impl<'a, V: Eq + Hash + Clone> SetFamily<'a, V> {
    /// Returns a [`SetFamily`] consisting only of sets with the smallest possible summed weight.
    /// The weight is calculated by the provided closure.
    ///
    ///# Panics
    ///May panic if `self` is an invalid index for the [`ZddHolder`]
    #[must_use]
    pub fn only_minimal_sets<F: Fn(&V) -> usize>(self, f: F) -> SetFamily<'a, V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let min_cost_lookup = self.minimal_set_inner(f);
        let overall_min = min_cost_lookup.get(&self.as_raw()).unwrap().min_set_weight;
        self.only_minimal_sets_inner(0, overall_min, &min_cost_lookup)
    }

    fn only_minimal_sets_inner(
        self,
        current_cost: usize,
        overall_min: UsizeOrPositiveInfinity,
        min_cost_lookup: &HashMap<ZddIndex<V>, MinWeightCost>,
    ) -> SetFamily<'a, V> {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let (v, lo, hi) = self.get().unwrap();
        let element_weight = min_cost_lookup.get(&self.as_raw()).unwrap().element_weight;

        let lo_w = min_cost_lookup.get(&lo.as_raw()).unwrap().min_set_weight;
        let hi_w = min_cost_lookup.get(&hi.as_raw()).unwrap().min_set_weight;

        match (
            lo_w.add(current_cost) <= overall_min,
            hi_w.add(current_cost) <= overall_min,
        ) {
            (true, true) => self.manager.get_node(
                v.clone(),
                lo.only_minimal_sets_inner(current_cost, overall_min, min_cost_lookup),
                hi.only_minimal_sets_inner(
                    current_cost + element_weight,
                    overall_min,
                    min_cost_lookup,
                ),
            ),
            (false, true) => self.manager.get_node(
                v.clone(),
                self.manager.zero(),
                hi.only_minimal_sets_inner(
                    current_cost + element_weight,
                    overall_min,
                    min_cost_lookup,
                ),
            ),
            (true, false) => lo.only_minimal_sets_inner(current_cost, overall_min, min_cost_lookup),
            (false, false) => self.manager.zero(),
        }
    }
}

///Iterates over all sets that are minimal by weight.
///
///See [`SetFamily::only_minimal_sets`]
pub struct MinimalSetIterator<'a, V: Eq + Hash> {
    #[expect(clippy::type_complexity)]
    stack: Vec<(ZddIndex<V>, (Vec<V>, usize))>,
    root: SetFamily<'a, V>,
    minimum_cost_lookup: HashMap<ZddIndex<V>, MinWeightCost>,
    min_cost: UsizeOrPositiveInfinity,
}

impl<V: Eq + Hash> MinimalSetIterator<'_, V> {
    ///Find the minimal cost of all sets.
    #[must_use]
    pub fn min_cost(&self) -> Option<usize> {
        self.min_cost.into()
    }

    #[cfg(test)]
    fn minimum_costs(&self) -> HashMap<SetFamily<'_, V>, MinWeightCost> {
        self.minimum_cost_lookup
            .iter()
            .map(|(x, y)| (SetFamily::from_set_family(*x, self.root.manager), *y))
            .collect()
    }
}

impl<V: Clone + Debug + Eq + Hash> Iterator for MinimalSetIterator<'_, V> {
    type Item = Vec<V>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.min_cost == UsizeOrPositiveInfinity::PositiveInfinity {
            return None;
        }

        while let Some((this, (mut path, current_cost))) = self.stack.pop() {
            if this.is_zero() {
                continue;
            }
            if this.is_one() {
                return Some(path);
            }

            let (v, lo, hi) = this.get(self.root.manager).unwrap();
            let element_weight = self.minimum_cost_lookup.get(&this).unwrap().element_weight;
            let lo_w = self.minimum_cost_lookup.get(&lo).unwrap().min_set_weight;
            let hi_w = self.minimum_cost_lookup.get(&hi).unwrap().min_set_weight;

            match (
                lo_w.add(current_cost) <= self.min_cost,
                hi_w.add(current_cost) <= self.min_cost,
            ) {
                (true, true) => {
                    self.stack.push((lo, (path.clone(), current_cost)));
                    path.push(v.clone());
                    self.stack.push((hi, (path, current_cost + element_weight)));
                }
                (false, true) => {
                    path.push(v.clone());
                    self.stack.push((hi, (path, current_cost + element_weight)));
                }
                (true, false) => self.stack.push((lo, (path, current_cost))),
                (false, false) => (),
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use serde::{Deserialize, Serialize};

    use crate::ZddHolder;

    use super::*;

    const SETS: &str = "ABCD ABCE EFG GH";

    #[expect(clippy::trivially_copy_pass_by_ref)]
    fn char_value(c: &char) -> usize {
        match c {
            'A' | 'C' => 1,
            'B' => 2,
            'D' | 'E' => 4,
            'F' => 50,
            'G' => 0,
            'H' => 45,
            _ => 999,
        }
    }

    #[test]
    fn ordering_of_usize_with_inf() {
        assert!(
            UsizeOrPositiveInfinity::PositiveInfinity > UsizeOrPositiveInfinity::Size(usize::MAX)
        );
        assert!(
            UsizeOrPositiveInfinity::PositiveInfinity == UsizeOrPositiveInfinity::PositiveInfinity
        );
        assert!(UsizeOrPositiveInfinity::Size(3) > UsizeOrPositiveInfinity::Size(0));
        assert!(UsizeOrPositiveInfinity::Size(0) == UsizeOrPositiveInfinity::Size(0));
    }

    #[test]
    fn minimum_cost_test() {
        let lorem_sets = SETS
            .split(' ')
            .map(|x| x.chars().collect::<BTreeSet<_>>())
            .collect::<BTreeSet<_>>();

        let n = lorem_sets
            .iter()
            .map(|x| x.iter().map(char_value).sum::<usize>())
            .min()
            .unwrap();
        let mins = lorem_sets
            .iter()
            .filter(|x| x.iter().map(char_value).sum::<usize>() == n)
            .cloned()
            .collect::<Vec<_>>();

        let holder = ZddHolder::new();
        let lorem = SetFamily::from_sets(lorem_sets, &holder);

        assert_eq!(lorem.minimal_set_size(char_value).unwrap(), n);

        let min_sets = lorem.minimal_sets(char_value);
        println!("{}", lorem.graphviz_with_extra(&min_sets.minimum_costs()));
        let sets = min_sets
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(sets, mins);

        let restricted_lorem = lorem
            .only_minimal_sets(char_value)
            .members()
            .map(|x| x.into_iter().collect::<BTreeSet<_>>())
            .collect::<Vec<_>>();

        assert_eq!(restricted_lorem, mins);
    }

    #[derive(Eq, Clone, Copy, PartialOrd, Ord, PartialEq, Debug, Hash, Serialize, Deserialize)]
    struct WeightedId {
        id: usize,
        weight: u8,
    }

    impl Display for WeightedId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.id)
        }
    }

    /*
    #[test]
    fn complicated_minimization() {
        const MIN_GRAMMAR_SIZE: usize = 25;
        let s = "([(17,()),(31,()),(56,()),(72,()),(88,())],(free:[],data:[None,None,Some((value:(id:13,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:12,weight:5),lo:(0,()),hi:(2,()))),Some((value:(id:12,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:11,weight:4),lo:(3,()),hi:(4,()))),Some((value:(id:10,weight:6),lo:(0,()),hi:(1,()))),Some((value:(id:9,weight:3),lo:(5,()),hi:(6,()))),Some((value:(id:9,weight:3),lo:(0,()),hi:(1,()))),Some((value:(id:8,weight:6),lo:(7,()),hi:(8,()))),Some((value:(id:7,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:6,weight:3),lo:(9,()),hi:(10,()))),Some((value:(id:5,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:4,weight:3),lo:(11,()),hi:(12,()))),Some((value:(id:3,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:2,weight:5),lo:(13,()),hi:(14,()))),Some((value:(id:1,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:0,weight:5),lo:(15,()),hi:(16,()))),Some((value:(id:21,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:20,weight:4),lo:(18,()),hi:(1,()))),Some((value:(id:12,weight:5),lo:(0,()),hi:(19,()))),Some((value:(id:19,weight:6),lo:(0,()),hi:(1,()))),Some((value:(id:18,weight:6),lo:(21,()),hi:(1,()))),Some((value:(id:9,weight:3),lo:(20,()),hi:(22,()))),Some((value:(id:17,weight:3),lo:(0,()),hi:(1,()))),Some((value:(id:7,weight:4),lo:(23,()),hi:(24,()))),Some((value:(id:16,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:4,weight:3),lo:(25,()),hi:(26,()))),Some((value:(id:15,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:3,weight:2),lo:(27,()),hi:(28,()))),Some((value:(id:14,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:0,weight:5),lo:(29,()),hi:(30,()))),Some((value:(id:36,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:35,weight:4),lo:(0,()),hi:(32,()))),Some((value:(id:34,weight:4),lo:(33,()),hi:(32,()))),Some((value:(id:29,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:27,weight:3),lo:(34,()),hi:(35,()))),Some((value:(id:24,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:22,weight:2),lo:(36,()),hi:(37,()))),Some((value:(id:35,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:34,weight:4),lo:(39,()),hi:(1,()))),Some((value:(id:12,weight:5),lo:(38,()),hi:(40,()))),Some((value:(id:33,weight:6),lo:(0,()),hi:(1,()))),Some((value:(id:32,weight:6),lo:(42,()),hi:(1,()))),Some((value:(id:31,weight:6),lo:(43,()),hi:(1,()))),Some((value:(id:30,weight:6),lo:(44,()),hi:(1,()))),Some((value:(id:9,weight:3),lo:(41,()),hi:(45,()))),Some((value:(id:27,weight:3),lo:(0,()),hi:(1,()))),Some((value:(id:7,weight:4),lo:(46,()),hi:(47,()))),Some((value:(id:28,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:26,weight:4),lo:(49,()),hi:(1,()))),Some((value:(id:4,weight:3),lo:(48,()),hi:(50,()))),Some((value:(id:25,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:23,weight:5),lo:(52,()),hi:(1,()))),Some((value:(id:3,weight:2),lo:(51,()),hi:(53,()))),Some((value:(id:22,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:0,weight:5),lo:(54,()),hi:(55,()))),Some((value:(id:50,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:49,weight:5),lo:(0,()),hi:(57,()))),Some((value:(id:49,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:48,weight:4),lo:(58,()),hi:(59,()))),Some((value:(id:47,weight:6),lo:(0,()),hi:(1,()))),Some((value:(id:46,weight:3),lo:(60,()),hi:(61,()))),Some((value:(id:46,weight:3),lo:(0,()),hi:(1,()))),Some((value:(id:45,weight:6),lo:(62,()),hi:(63,()))),Some((value:(id:44,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:43,weight:3),lo:(64,()),hi:(65,()))),Some((value:(id:42,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:41,weight:3),lo:(66,()),hi:(67,()))),Some((value:(id:40,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:39,weight:5),lo:(68,()),hi:(69,()))),Some((value:(id:38,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:37,weight:5),lo:(70,()),hi:(71,()))),Some((value:(id:64,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:63,weight:5),lo:(0,()),hi:(73,()))),Some((value:(id:63,weight:5),lo:(0,()),hi:(1,()))),Some((value:(id:62,weight:4),lo:(74,()),hi:(75,()))),Some((value:(id:61,weight:6),lo:(0,()),hi:(1,()))),Some((value:(id:60,weight:3),lo:(76,()),hi:(77,()))),Some((value:(id:60,weight:3),lo:(0,()),hi:(1,()))),Some((value:(id:59,weight:6),lo:(78,()),hi:(79,()))),Some((value:(id:58,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:57,weight:3),lo:(80,()),hi:(81,()))),Some((value:(id:56,weight:4),lo:(0,()),hi:(1,()))),Some((value:(id:55,weight:3),lo:(82,()),hi:(83,()))),Some((value:(id:54,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:53,weight:5),lo:(84,()),hi:(85,()))),Some((value:(id:52,weight:2),lo:(0,()),hi:(1,()))),Some((value:(id:51,weight:5),lo:(86,()),hi:(87,())))],uniq_table:{(value:(id:38,weight:2),lo:(0,()),hi:(1,())):(71,()),(value:(id:23,weight:5),lo:(52,()),hi:(1,())):(53,()),(value:(id:27,weight:3),lo:(0,()),hi:(1,())):(47,()),(value:(id:37,weight:5),lo:(70,()),hi:(71,())):(72,()),(value:(id:63,weight:5),lo:(0,()),hi:(1,())):(75,()),(value:(id:59,weight:6),lo:(78,()),hi:(79,())):(80,()),(value:(id:63,weight:5),lo:(0,()),hi:(73,())):(74,()),(value:(id:12,weight:5),lo:(0,()),hi:(1,())):(4,()),(value:(id:35,weight:4),lo:(0,()),hi:(1,())):(39,()),(value:(id:32,weight:6),lo:(42,()),hi:(1,())):(43,()),(value:(id:30,weight:6),lo:(44,()),hi:(1,())):(45,()),(value:(id:21,weight:4),lo:(0,()),hi:(1,())):(18,()),(value:(id:44,weight:4),lo:(0,()),hi:(1,())):(65,()),(value:(id:9,weight:3),lo:(41,()),hi:(45,())):(46,()),(value:(id:55,weight:3),lo:(82,()),hi:(83,())):(84,()),(value:(id:39,weight:5),lo:(68,()),hi:(69,())):(70,()),(value:(id:54,weight:2),lo:(0,()),hi:(1,())):(85,()),(value:(id:0,weight:5),lo:(29,()),hi:(30,())):(31,()),(value:(id:34,weight:4),lo:(39,()),hi:(1,())):(40,()),(value:(id:26,weight:4),lo:(49,()),hi:(1,())):(50,()),(value:(id:13,weight:4),lo:(0,()),hi:(1,())):(2,()),(value:(id:18,weight:6),lo:(21,()),hi:(1,())):(22,()),(value:(id:49,weight:5),lo:(0,()),hi:(1,())):(59,()),(value:(id:10,weight:6),lo:(0,()),hi:(1,())):(6,()),(value:(id:4,weight:3),lo:(25,()),hi:(26,())):(27,()),(value:(id:5,weight:4),lo:(0,()),hi:(1,())):(12,()),(value:(id:4,weight:3),lo:(11,()),hi:(12,())):(13,()),(value:(id:16,weight:4),lo:(0,()),hi:(1,())):(26,()),(value:(id:9,weight:3),lo:(5,()),hi:(6,())):(7,()),(value:(id:9,weight:3),lo:(0,()),hi:(1,())):(8,()),(value:(id:48,weight:4),lo:(58,()),hi:(59,())):(60,()),(value:(id:12,weight:5),lo:(0,()),hi:(2,())):(3,()),(value:(id:11,weight:4),lo:(3,()),hi:(4,())):(5,()),(value:(id:64,weight:4),lo:(0,()),hi:(1,())):(73,()),(value:(id:58,weight:4),lo:(0,()),hi:(1,())):(81,()),(value:(id:12,weight:5),lo:(0,()),hi:(19,())):(20,()),(value:(id:22,weight:2),lo:(0,()),hi:(1,())):(55,()),(value:(id:3,weight:2),lo:(27,()),hi:(28,())):(29,()),(value:(id:19,weight:6),lo:(0,()),hi:(1,())):(21,()),(value:(id:2,weight:5),lo:(13,()),hi:(14,())):(15,()),(value:(id:7,weight:4),lo:(0,()),hi:(1,())):(10,()),(value:(id:24,weight:5),lo:(0,()),hi:(1,())):(37,()),(value:(id:1,weight:2),lo:(0,()),hi:(1,())):(16,()),(value:(id:17,weight:3),lo:(0,()),hi:(1,())):(24,()),(value:(id:45,weight:6),lo:(62,()),hi:(63,())):(64,()),(value:(id:43,weight:3),lo:(64,()),hi:(65,())):(66,()),(value:(id:20,weight:4),lo:(18,()),hi:(1,())):(19,()),(value:(id:41,weight:3),lo:(66,()),hi:(67,())):(68,()),(value:(id:60,weight:3),lo:(0,()),hi:(1,())):(79,()),(value:(id:3,weight:2),lo:(51,()),hi:(53,())):(54,()),(value:(id:35,weight:4),lo:(0,()),hi:(32,())):(33,()),(value:(id:46,weight:3),lo:(0,()),hi:(1,())):(63,()),(value:(id:62,weight:4),lo:(74,()),hi:(75,())):(76,()),(value:(id:22,weight:2),lo:(36,()),hi:(37,())):(38,()),(value:(id:51,weight:5),lo:(86,()),hi:(87,())):(88,()),(value:(id:25,weight:5),lo:(0,()),hi:(1,())):(52,()),(value:(id:46,weight:3),lo:(60,()),hi:(61,())):(62,()),(value:(id:15,weight:5),lo:(0,()),hi:(1,())):(28,()),(value:(id:34,weight:4),lo:(33,()),hi:(32,())):(34,()),(value:(id:28,weight:4),lo:(0,()),hi:(1,())):(49,()),(value:(id:53,weight:5),lo:(84,()),hi:(85,())):(86,()),(value:(id:40,weight:2),lo:(0,()),hi:(1,())):(69,()),(value:(id:57,weight:3),lo:(80,()),hi:(81,())):(82,()),(value:(id:50,weight:4),lo:(0,()),hi:(1,())):(57,()),(value:(id:61,weight:6),lo:(0,()),hi:(1,())):(77,()),(value:(id:14,weight:2),lo:(0,()),hi:(1,())):(30,()),(value:(id:7,weight:4),lo:(23,()),hi:(24,())):(25,()),(value:(id:36,weight:5),lo:(0,()),hi:(1,())):(32,()),(value:(id:0,weight:5),lo:(54,()),hi:(55,())):(56,()),(value:(id:29,weight:4),lo:(0,()),hi:(1,())):(35,()),(value:(id:49,weight:5),lo:(0,()),hi:(57,())):(58,()),(value:(id:8,weight:6),lo:(7,()),hi:(8,())):(9,()),(value:(id:3,weight:2),lo:(0,()),hi:(1,())):(14,()),(value:(id:6,weight:3),lo:(9,()),hi:(10,())):(11,()),(value:(id:47,weight:6),lo:(0,()),hi:(1,())):(61,()),(value:(id:12,weight:5),lo:(38,()),hi:(40,())):(41,()),(value:(id:56,weight:4),lo:(0,()),hi:(1,())):(83,()),(value:(id:33,weight:6),lo:(0,()),hi:(1,())):(42,()),(value:(id:9,weight:3),lo:(20,()),hi:(22,())):(23,()),(value:(id:4,weight:3),lo:(48,()),hi:(50,())):(51,()),(value:(id:31,weight:6),lo:(43,()),hi:(1,())):(44,()),(value:(id:7,weight:4),lo:(46,()),hi:(47,())):(48,()),(value:(id:52,weight:2),lo:(0,()),hi:(1,())):(87,()),(value:(id:0,weight:5),lo:(15,()),hi:(16,())):(17,()),(value:(id:27,weight:3),lo:(34,()),hi:(35,())):(36,()),(value:(id:60,weight:3),lo:(76,()),hi:(77,())):(78,()),(value:(id:42,weight:4),lo:(0,()),hi:(1,())):(67,())},cache:{},sum_cache:{},protected:[]))";
        let (sets, holder) =
            ron::from_str::<(Vec<ZddIndex<WeightedId>>, ZddHolder<WeightedId>)>(s).unwrap();

        let mut all_grammar = holder.one();
        for s in sets {
            let s = SetFamily::from_set_family(s, &holder);
            s.check_valid_zdd();
            all_grammar = all_grammar.join(s);
            all_grammar.check_valid_zdd();
        }

        let mut min_size = usize::MAX;
        let mut minimal_sets = vec![];
        for s in all_grammar.members() {
            let size = s.iter().map(|x| usize::from(x.weight)).sum::<usize>();
            if size > min_size {
                continue;
            }
            if size < min_size {
                min_size = size;
                minimal_sets = vec![];
            }

            minimal_sets.push(s.into_iter().collect::<BTreeSet<_>>());
        }
        minimal_sets.sort();

        let min_size = all_grammar
            .minimal_set_size(|x| usize::from(x.weight))
            .unwrap();

        assert_eq!(min_size, MIN_GRAMMAR_SIZE);

        let mut iterated_sets = vec![];
        let iter = all_grammar.minimal_sets(|x| usize::from(x.weight));

        let gviz = all_grammar.graphviz_with_extra(&iter.minimum_costs());
        println!("{gviz}");

        for g in iter {
            let size = g.iter().map(|x| usize::from(x.weight)).sum::<usize>();
            assert_eq!(size, MIN_GRAMMAR_SIZE);
            iterated_sets.push(g.into_iter().collect::<BTreeSet<_>>());
        }
        iterated_sets.sort();
        assert_eq!(iterated_sets, minimal_sets);

        let simple_grammar = all_grammar.only_minimal_sets(|x| usize::from(x.weight));

        let mut raw_sets = vec![];
        for g in simple_grammar.members() {
            let size = g.iter().map(|x| usize::from(x.weight)).sum::<usize>();
            assert_eq!(size, MIN_GRAMMAR_SIZE);
            raw_sets.push(g.into_iter().collect::<BTreeSet<_>>());
        }

        raw_sets.sort();
        assert_eq!(raw_sets, minimal_sets);
    }*/
}
