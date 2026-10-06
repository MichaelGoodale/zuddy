use std::{collections::BTreeMap, hash::Hash};

use mem_dbg::{FlatType, MemSize};
use num_traits::Num;

use crate::{
    SetFamily,
    manager::{SizeKey, SizeValue, TempCache, TempCacheItem, ZddIndex},
    utils::UsizeOrPositiveInfinity,
};

pub(crate) type WeightCache<'a, V, Int> = TempCache<'a, V, ZddIndex<V>, Int>;
pub(crate) type BoundsWeightCache<'a, V, I> = TempCache<'a, V, ZddIndex<V>, (Option<I>, I)>;

fn min_none_first<T: Ord>(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (None, None) => None,
        (None, Some(x)) | (Some(x), None) => Some(x),
        (Some(x), Some(y)) => Some(std::cmp::min(x, y)),
    }
}

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///The size of the biggest possible set by summed weight
    #[must_use]
    pub fn max_weight<F, Int>(&self, f: F) -> Int
    where
        F: Fn(&V) -> Int + Send + Sync,
        Int: Num + TempCacheItem<'a, V, Output = Int> + Ord + MemSize,
    {
        let cache: WeightCache<'a, V, Int> = self.manager().create_temporary_cache();
        self.clone().max_weight_inner(&f, &cache)
    }

    #[must_use]
    pub(crate) fn max_weight_inner<F, Int>(self, f: &F, cache: &WeightCache<'a, V, Int>) -> Int
    where
        F: Fn(&V) -> Int + Send + Sync,
        Int: Num + TempCacheItem<'a, V, Output = Int> + Ord + MemSize,
    {
        if self.is_zero() || self.is_one() {
            return Int::zero();
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let (lo, hi) = (
            lo.max_weight_inner(f, cache),
            hi.max_weight_inner(f, cache) + w,
        );

        cache.insert(self.as_raw(), std::cmp::max(lo, hi))
    }

    ///The size of the biggest possible set.
    #[must_use]
    pub fn max_cardinality(&self) -> usize {
        if self.is_zero() || self.is_one() {
            return 0;
        }

        let holder = self.manager();
        if let Some(SizeValue::Max(r)) = holder.size_cache_get(&SizeKey::Max(self.as_raw())) {
            return r;
        }

        #[expect(clippy::missing_panics_doc)]
        let (lo, hi) = self.children().unwrap();

        let (lo, hi) = (lo.max_cardinality(), hi.max_cardinality() + 1);

        holder
            .size_cache_insert(
                SizeKey::Max(self.as_raw()),
                SizeValue::Max(std::cmp::max(lo, hi)),
            )
            .unwrap_max()
    }

    ///The size of the smallest possible set.
    ///
    ///Returns [`UsizeOrPositiveInfinity::PositiveInfinity`] if it is the empty set.
    #[must_use]
    pub fn min_cardinality(&self) -> UsizeOrPositiveInfinity {
        if self.is_zero() {
            return UsizeOrPositiveInfinity::PositiveInfinity;
        } else if self.is_one() {
            return UsizeOrPositiveInfinity::Size(0);
        }

        let holder = self.manager();
        if let Some(SizeValue::Min(r)) = holder.size_cache_get(&SizeKey::Min(self.as_raw())) {
            return r;
        }

        #[expect(clippy::missing_panics_doc)]
        let (lo, hi) = self.children().unwrap();

        let (lo, hi) = (lo.min_cardinality(), hi.min_cardinality().add_usize(1));

        holder
            .size_cache_insert(
                SizeKey::Min(self.as_raw()),
                SizeValue::Min(std::cmp::min(lo, hi)),
            )
            .unwrap_min()
    }

    ///The lower and upper bound on the size of any set in the ZDD.
    ///For the empty family (zero), the lower bound is [`UsizeOrPositiveInfinity::PositiveInfinity`].
    #[must_use]
    pub fn bounds_cardinality(&self) -> (UsizeOrPositiveInfinity, usize) {
        if self.is_zero() {
            return (UsizeOrPositiveInfinity::PositiveInfinity, 0);
        } else if self.is_one() {
            return (UsizeOrPositiveInfinity::Size(0), 0);
        }

        let holder = self.manager();
        if let Some(SizeValue::Bounds(a, b)) =
            holder.size_cache_get(&SizeKey::Bounds(self.as_raw()))
        {
            return (a, b);
        }

        #[expect(clippy::missing_panics_doc)]
        let (lo, hi) = self.children().unwrap();

        let ((lo_min, lo_max), (hi_min, hi_max)) =
            (lo.bounds_cardinality(), hi.bounds_cardinality());

        let hi_min = hi_min.add_usize(1);
        let hi_max = hi_max + 1;

        holder
            .size_cache_insert(
                SizeKey::Bounds(self.as_raw()),
                SizeValue::Bounds(std::cmp::min(lo_min, hi_min), std::cmp::max(lo_max, hi_max)),
            )
            .unwrap_bounds()
    }

    ///The size of the smallest possible set by summed weight. Accepts weights as [`usize`] or [`isize`].
    ///# Panics
    ///Will panic if passed the empty set.
    #[must_use]
    pub fn min_weight<F, Int>(&self, f: F) -> Int
    where
        F: Fn(&V) -> Int + Send + Sync,
        Int: Num + Clone + Ord + TempCacheItem<'a, V, Output = Int> + MemSize,
    {
        let cache: WeightCache<'a, V, Option<Int>> = self.manager().create_temporary_cache();
        self.clone().min_weight_inner(&f, &cache).unwrap()
    }

    #[must_use]
    pub(crate) fn min_weight_inner<F, Int>(
        self,
        f: &F,
        cache: &WeightCache<'a, V, Option<Int>>,
    ) -> Option<Int>
    where
        F: Fn(&V) -> Int + Send + Sync,
        Int: Num + Clone + Ord + TempCacheItem<'a, V, Output = Int> + MemSize,
    {
        if self.is_zero() {
            return None;
        } else if self.is_one() {
            return Some(Int::zero());
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let (lo, hi) = (
            lo.min_weight_inner(f, cache),
            hi.min_weight_inner(f, cache).map(|x| x + w),
        );

        cache.insert(self.as_raw(), min_none_first(lo, hi))
    }

    ///The upper and lower bound of summed weight of any set in the ZDD.
    ///
    ///# Panics
    /// Will panic if passed an empty set, as the lower bound is undefined.
    #[must_use]
    pub fn bounds<F, T>(&self, f: F) -> (T, T)
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T> + MemSize,
    {
        let cache: BoundsWeightCache<'a, V, T> = self.manager().create_temporary_cache();
        let (min, max) = self.clone().bounds_inner(&f, &cache);
        (min.unwrap(), max)
    }

    #[must_use]
    pub(crate) fn bounds_inner<F, T>(
        self,
        f: &F,
        cache: &BoundsWeightCache<'a, V, T>,
    ) -> (Option<T>, T)
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T> + MemSize,
    {
        if self.is_zero() {
            return (None, T::zero());
        } else if self.is_one() {
            return (Some(T::zero()), T::zero());
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let ((lo_min, lo_max), (hi_min, hi_max)) =
            (lo.bounds_inner(f, cache), hi.bounds_inner(f, cache));

        let hi_min = hi_min.map(|x| x + w.clone());
        let hi_max = hi_max + w;

        cache.insert(
            self.as_raw(),
            (
                min_none_first(lo_min, hi_min),
                std::cmp::max(lo_max, hi_max),
            ),
        )
    }

    ///The histogram of the summed weights of the elements of sets.
    ///How many sets are there of each summed weight?
    ///Returns an empty map for the empty family (zero).
    #[must_use]
    pub fn set_weights<F, T>(&self, f: F) -> BTreeMap<T, usize>
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T> + Send + Sync + MemSize + FlatType,
    {
        let cache: WeightCache<'a, V, BTreeMap<T, usize>> = self.manager().create_temporary_cache();
        self.clone().set_weights_inner(&f, &cache)
    }

    #[must_use]
    pub(crate) fn set_weights_inner<F, T>(
        self,
        f: &F,
        cache: &WeightCache<'a, V, BTreeMap<T, usize>>,
    ) -> BTreeMap<T, usize>
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T> + Send + Sync + MemSize + FlatType,
    {
        if self.is_zero() {
            return BTreeMap::new();
        } else if self.is_one() {
            return BTreeMap::from([(T::zero(), 1)]);
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let (mut lo_hist, hi_hist) = self.manager().pools().join(
            || lo.set_weights_inner(f, cache),
            || hi.set_weights_inner(f, cache),
        );

        let w = f(&value);

        for (k, v) in hi_hist {
            let k = k + w.clone();
            *lo_hist.entry(k).or_insert(0) += v;
        }

        cache.insert(self.as_raw(), lo_hist)
    }
}

#[cfg(test)]
mod test {
    use std::collections::{BTreeMap, BTreeSet};

    use rand::{SeedableRng, rngs::SmallRng};

    use crate::{
        SetFamily, ZddHolder,
        utils::{
            UsizeOrPositiveInfinity,
            test::{random_family, random_isize_weights, str_to_sets},
        },
    };

    #[test]
    fn test_max_weight() {
        let f = |c: &char| (*c as usize) - ('a' as usize) + 1;
        let holder = ZddHolder::new();
        let zdds = ["ad ", "de d ab", "de d", "ab cd e w s f z a abcdq za"];
        for s in zdds {
            let s = str_to_sets(s);
            let max_size = s
                .iter()
                .map(|set| set.iter().map(f).sum::<usize>())
                .max()
                .unwrap_or(0);
            let max_card = s.iter().map(BTreeSet::len).max().unwrap_or(0);
            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.max_weight(f), max_size);
            assert_eq!(s.max_cardinality(), max_card);
        }
    }

    #[test]
    fn test_min_weight() {
        let f = |c: &char| (*c as usize) - ('a' as usize) + 1;
        let holder = ZddHolder::new();
        let zdds = ["ad ", "de d ab", "de d", "ab cd e w s f z a abcdq za"];
        for s in zdds {
            let s = str_to_sets(s);
            let min_size = s
                .iter()
                .map(|set| set.iter().map(f).sum::<usize>())
                .min()
                .unwrap_or(0);
            let min_card = s.iter().map(BTreeSet::len).min().unwrap_or(0);
            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.min_weight(f), min_size);
            assert_eq!(s.min_cardinality().unwrap(), min_card);
        }
    }

    #[test]
    fn test_bounds_weight() {
        let f = |c: &char| (*c as usize) - ('a' as usize) + 1;
        let holder = ZddHolder::new();
        let zdds = ["ad ", "de d ab", "de d", "ab cd e w s f z a abcdq za"];
        for s in zdds {
            let s = str_to_sets(s);
            let min_size = s
                .iter()
                .map(|set| set.iter().map(f).sum::<usize>())
                .min()
                .unwrap_or(0);
            let max_size = s
                .iter()
                .map(|set| set.iter().map(f).sum::<usize>())
                .max()
                .unwrap_or(0);

            let min_card = s.iter().map(BTreeSet::len).min().unwrap_or(0);
            let max_card = s.iter().map(BTreeSet::len).max().unwrap_or(0);

            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.bounds(f), (min_size, max_size));
            assert_eq!(
                s.bounds_cardinality(),
                (UsizeOrPositiveInfinity::Size(min_card), max_card)
            );
        }
    }

    #[test]
    fn histogram_test() {
        let universe = ['a', 'b', 'c', 'd', 'e', 'f', 'g'];
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..200 {
            let mut fam = random_family(&universe, &mut rng);
            while fam.is_empty() {
                fam = random_family(&universe, &mut rng);
            }

            let weights = random_isize_weights(&universe, &mut rng);

            let f = |x: &char| *weights.get(x).unwrap();

            let mut hist = BTreeMap::new();
            for set in &fam {
                let size = set.iter().map(f).sum::<isize>();
                *hist.entry(size).or_insert(0) += 1;
            }

            let holder = ZddHolder::new();
            let sets = SetFamily::from_sets(fam, &holder);
            let calculated_hist = sets.set_weights(f);

            assert_eq!(calculated_hist, hist);
        }
    }
}
