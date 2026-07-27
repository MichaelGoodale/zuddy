use std::{
    collections::{BTreeSet, BinaryHeap},
    fmt::Debug,
    hash::Hash,
    time::Instant,
};

use ahash::{HashMap, RandomState};
use indicatif::{ProgressIterator, ProgressStyle};

#[derive(Debug, Clone, Eq, PartialEq)]
struct QueueWrapper<'a, V: Eq + Hash + Clone>(SetFamily<'a, V>);

impl<V: Eq + Hash + Clone> PartialOrd for QueueWrapper<'_, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<V: Eq + Hash + Clone> Ord for QueueWrapper<'_, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .0
            .n_nodes()
            .cmp(&self.0.n_nodes())
            .then(self.0.id.cmp(&other.0.id))
    }
}

use crate::{
    SetFamily,
    algorithms::UsizeOrPositiveInfinity,
    manager::{TempCache, TempCacheItem, ZddIndex},
    utils::SingleSet,
};

//TODO: Write a DP style algo based on `[SetFamily::budget_exact]` which adds all sets that add up o 28.

/// Given sets $S$, with elements weighted by function $f$, returns the zdd
/// such that where $b$ is the budget:
///
/// x = { x | ∀s∈S s⊆x ∧ ∑e∈x f(e) ≤ b }
///
/// # Panics
/// Will panic if `sets` is empty or if the sets don't all share the same manager.
pub fn subset_cover<V, F>(
    mut sets: Vec<SetFamily<'_, V>>,
    budget: usize,
    f: F,
) -> Option<SetFamily<'_, V>>
where
    V: Eq + Hash + Clone + Ord + Send + Sync + Debug,
    F: Fn(&V) -> usize + Send + Sync,
{
    assert!(!sets.is_empty(), "Sets cannot be empty!");
    let sets = sets.into_iter().collect::<Vec<_>>();
    let time = Instant::now();
    //let cache = holder.create_temporary_cache();
    //let min_weight_cache = holder.create_temporary_cache();
    //sets.sort_by_key(|x| x.id);
    //sets.dedup();
    let mut universe = BTreeSet::new();
    for s in &sets {
        universe.extend(s.universe::<RandomState>());
    }
    println!("Made universe!");
    let style = ProgressStyle::default_bar()
        .template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({per_sec}, ETA {eta_precise})"
        )
        .unwrap()
        .progress_chars("#>-");

    println!("Now splitting!");
    let split_sets = sets
        .into_iter()
        .map(|x| x.bounded_supersets(&f, budget))
        .progress_with_style(style.clone())
        .collect::<Vec<_>>();

    panic!("Done!");

    /*
    let up_sets = up_sets
        .into_iter()
        .map(|x| {
            let u = x.universe::<RandomState>().into_iter().collect();
            x.extend_as_superset(universe.difference(&u).cloned())
        })
        .progress_with_style(style.clone())
        .collect::<Vec<_>>();

    println!("Done extending!");
    let mut up_sets = up_sets
        .into_iter()
        .map(|x| QueueWrapper(x))
        .collect::<BinaryHeap<_>>();

    let bar = indicatif::ProgressBar::new(up_sets.len() as u64 - 1).with_style(style);
    while up_sets.len() >= 2 {
        let a = up_sets.pop().unwrap().0;
        let b = up_sets.pop().unwrap().0;
        let c = a.intersect(b);
        up_sets.push(QueueWrapper(c));
        bar.inc(1);
    }
    bar.finish();
    let time = time.elapsed().as_secs_f64();
    println!("Joining took {time} seconds.");*/

    None
}

struct SplitSetFamily<'a, V: Eq + Hash> {
    sets: HashMap<usize, SetFamily<'a, V>>,
}

struct RawSplitSetFamily<V: Eq + Hash> {
    sets: HashMap<usize, ZddIndex<V>>,
}

impl<'a, V: Eq + Hash + 'a> TempCacheItem<'a, V> for RawSplitSetFamily<V> {
    type Output = SplitSetFamily<'a, V>;

    fn to_gc(&self, holder: &'a crate::ZddHolder<V>) -> Self::Output {
        SplitSetFamily {
            sets: self
                .sets
                .iter()
                .map(|(k, v)| (*k, SetFamily::from_set_family(*v, holder)))
                .collect(),
        }
    }

    fn from_gc(x: &Self::Output) -> Self {
        RawSplitSetFamily {
            sets: x.sets.iter().map(|(k, v)| (*k, v.as_raw())).collect(),
        }
    }
}

impl<'a, V: Eq + Hash + Ord + Clone + Send + Sync> SplitSetFamily<'a, V> {
    ///adds a value v w/ weight w to each set, **assuming** that the value is not present and that
    ///it is the current least value.
    fn add_val(&mut self, v: V, w: usize) {
        self.sets = self
            .sets
            .drain()
            .zip(std::iter::repeat(v))
            .map(|((x, s), v)| {
                let holder = s.manager();
                (x + w, holder.get_node(v, holder.zero(), s))
            })
            .collect();
    }

    fn union(mut self, mut other: Self) -> Self {
        let keys = self
            .sets
            .keys()
            .chain(other.sets.keys())
            .copied()
            .collect::<BTreeSet<_>>();

        SplitSetFamily {
            sets: keys
                .into_iter()
                .map(|k| {
                    let a = self.sets.remove(&k);
                    let b = other.sets.remove(&k);
                    match (a, b) {
                        (None, None) => panic!("can't happen bc keys is made from the sets"),
                        (None, Some(x)) | (Some(x), None) => (k, x),
                        (Some(x), Some(y)) => (k, x.union(y)),
                    }
                })
                .collect(),
        }
    }
}

impl<'a, V: Eq + Hash + Ord + Clone + Send + Sync> SetFamily<'a, V> {
    fn split<F>(&self, f: &F, max_budget: usize) -> SplitSetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache = self.manager().create_temporary_cache();
        self.clone().inner_split(f, max_budget, &cache)
    }

    fn bounded_supersets<F>(self, f: &F, max_budget: usize) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache = self.manager().create_temporary_cache();
        let w_cache = self.manager().create_temporary_cache();
        self.inner_bounded_supersets(f, max_budget, &cache, &w_cache)
    }

    fn inner_bounded_supersets<F>(
        self,
        f: &F,
        max_budget: usize,
        cache: &TempCache<'a, V, (ZddIndex<V>, usize)>,
        w_cache: &TempCache<'a, V, ZddIndex<V>, UsizeOrPositiveInfinity>,
    ) -> SetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() || self.is_one() {
            return self;
        }

        let holder = self.manager();
        let op = (self.as_raw(), max_budget);
        if let Some(r) = cache.get(&op) {
            return r;
        }

        let min = self.clone().min_weight_inner(f, w_cache);
        if min.unwrap() > max_budget {
            return holder.zero();
        }

        let (value, lo, hi) = self.get().unwrap();
        let mut r = lo
            .clone()
            .inner_bounded_supersets(f, max_budget, cache, w_cache);

        let w = f(&value);
        if let Some(hi_budget) = max_budget.checked_sub(w) {
            let lo = lo.inner_bounded_supersets(f, hi_budget, cache, w_cache);
            let hi = hi.inner_bounded_supersets(f, hi_budget, cache, w_cache);
            r = holder.get_node(value, r, lo.union(hi));
        }

        cache.insert(op, r)
    }

    fn inner_split<F>(
        self,
        f: &F,
        max_budget: usize,
        cache: &TempCache<'a, V, ZddIndex<V>, RawSplitSetFamily<V>>,
    ) -> SplitSetFamily<'a, V>
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return SplitSetFamily {
                sets: HashMap::default(),
            };
        }

        if self.is_one() {
            return SplitSetFamily {
                sets: [(0, self)].into_iter().collect(),
            };
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (min, max) = self.bounds(f);

        let r = if min == max {
            //then we can treat all subsequent sets identically.
            SplitSetFamily {
                sets: [(min, self.clone())].into_iter().collect(),
            }
        } else {
            let (v, lo, hi) = self.get().unwrap(); //since min == max if x is terminal.
            let mut sets = lo.inner_split(f, max_budget, cache);

            let w = f(&v);
            if let Some(hi_budget) = max_budget.checked_sub(w) {
                let mut hi = hi.inner_split(f, hi_budget, cache);
                hi.add_val(v, w);
                sets = sets.union(hi);
            }

            sets
        };
        cache.insert(self.as_raw(), r)
    }
}
