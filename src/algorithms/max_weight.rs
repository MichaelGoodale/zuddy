use std::hash::Hash;

use crate::{
    SetFamily,
    algorithms::UsizeOrPositiveInfinity,
    manager::{TempCache, ZddIndex},
};

pub(crate) type MaxWeightCache<'a, V> = TempCache<'a, V, ZddIndex<V>, usize>;
pub(crate) type MinWeightCache<'a, V> = TempCache<'a, V, ZddIndex<V>, UsizeOrPositiveInfinity>;
pub(crate) type BoundsWeightCache<'a, V> =
    TempCache<'a, V, ZddIndex<V>, (UsizeOrPositiveInfinity, usize)>;

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///The size of the biggest possible set by summed weight
    #[must_use]
    pub fn max_weight<F>(&self, f: F) -> usize
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache: MaxWeightCache<'a, V> = self.manager().create_temporary_cache();
        self.clone().max_weight_inner(&f, &cache)
    }

    #[must_use]
    pub(crate) fn max_weight_inner<F>(self, f: &F, cache: &MaxWeightCache<'a, V>) -> usize
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() || self.is_one() {
            return 0;
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

    ///The size of the smallest possible set by summed weight
    ///# Panics
    ///Will panic if passed the empty set.
    #[must_use]
    pub fn min_weight<F>(&self, f: F) -> usize
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache: MinWeightCache<'a, V> = self.manager().create_temporary_cache();
        self.clone().min_weight_inner(&f, &cache).unwrap()
    }

    #[must_use]
    pub(crate) fn min_weight_inner<F>(
        self,
        f: &F,
        cache: &MinWeightCache<'a, V>,
    ) -> UsizeOrPositiveInfinity
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return UsizeOrPositiveInfinity::PositiveInfinity;
        } else if self.is_one() {
            return UsizeOrPositiveInfinity::Size(0);
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let (lo, hi) = (
            lo.min_weight_inner(f, cache),
            hi.min_weight_inner(f, cache).add_usize(w),
        );

        cache.insert(self.as_raw(), std::cmp::min(lo, hi))
    }

    ///The upper and lower bound of size of any set in the ZDD.
    ///# Panics
    ///Will panic if passed the empty set.
    #[must_use]
    pub fn bounds<F>(&self, f: F) -> (usize, usize)
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        let cache: BoundsWeightCache<'a, V> = self.manager().create_temporary_cache();
        let (min, max) = self.clone().bounds_inner(&f, &cache);
        (min.unwrap(), max)
    }

    #[must_use]
    pub(crate) fn bounds_inner<F>(
        self,
        f: &F,
        cache: &BoundsWeightCache<'a, V>,
    ) -> (UsizeOrPositiveInfinity, usize)
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        if self.is_zero() {
            return (UsizeOrPositiveInfinity::PositiveInfinity, 0);
        } else if self.is_one() {
            return (UsizeOrPositiveInfinity::Size(0), 0);
        }

        if let Some(r) = cache.get(&self.as_raw()) {
            return r;
        }

        let (value, lo, hi) = self.get().unwrap();

        let w = f(&value);

        let ((lo_min, lo_max), (hi_min, hi_max)) =
            (lo.bounds_inner(f, cache), hi.bounds_inner(f, cache));

        let hi_min = hi_min.add_usize(w);
        let hi_max = hi_max + w;

        cache.insert(
            self.as_raw(),
            (std::cmp::min(lo_min, hi_min), std::cmp::max(lo_max, hi_max)),
        )
    }
}

#[cfg(test)]
mod test {
    use crate::{SetFamily, ZddHolder, algebra::str_to_sets};

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
            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.max_weight(f), max_size);
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
            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.min_weight(f), min_size);
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
            let s = SetFamily::from_sets(s, &holder);
            assert_eq!(s.bounds(f), (min_size, max_size));
        }
    }
}
