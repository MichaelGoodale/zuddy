use std::hash::Hash;

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
        Int: Num + TempCacheItem<'a, V, Output = Int> + Ord,
    {
        let cache: WeightCache<'a, V, Int> = self.manager().create_temporary_cache();
        self.clone().max_weight_inner(&f, &cache)
    }

    #[must_use]
    pub(crate) fn max_weight_inner<F, Int>(self, f: &F, cache: &WeightCache<'a, V, Int>) -> Int
    where
        F: Fn(&V) -> Int + Send + Sync,
        Int: Num + TempCacheItem<'a, V, Output = Int> + Ord,
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

    ///The size of the smallest possible set.
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
        Int: Num + Clone + Ord + TempCacheItem<'a, V, Output = Int>,
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
        Int: Num + Clone + Ord + TempCacheItem<'a, V, Output = Int>,
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

    ///The upper and lower bound of size of any set in the ZDD.
    ///# Panics
    ///Will panic if passed the empty set.
    #[must_use]
    pub fn bounds<F, T>(&self, f: F) -> (T, T)
    where
        F: Fn(&V) -> T + Send + Sync,
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T>,
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
        T: Num + Clone + Ord + TempCacheItem<'a, V, Output = T>,
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
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use crate::{SetFamily, ZddHolder, utils::UsizeOrPositiveInfinity, utils::test::str_to_sets};

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
}
