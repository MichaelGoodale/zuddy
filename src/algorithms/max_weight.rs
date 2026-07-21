use std::hash::Hash;

use crate::{
    SetFamily,
    manager::{TempCache, ZddIndex},
};

pub(crate) type MaxWeightCache<'a, V> = TempCache<'a, V, ZddIndex<V>, usize>;

impl<'a, V: Eq + Hash + Clone + Send + Sync> SetFamily<'a, V> {
    ///Assign each element a weight using the `f` function, and return the Zdd consisting of all
    ///sets that have a maximum summed weight of `weight` or less.
    #[must_use]
    pub fn max_weight<F>(&self, f: F) -> usize
    where
        F: Fn(&V) -> usize + Send + Sync,
    {
        //We give the function a UUID so that the cache doesn't mix up if someone runs w/ two
        //different functions for weight.
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
}
