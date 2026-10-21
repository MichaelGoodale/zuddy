use std::{
    collections::BTreeMap,
    hash::Hash,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::utils::op_cache::OpCache;
use ahash::RandomState;
use mem_dbg::{MemSize, SizeFlags};

use crate::{SetFamily, ZddHolder, manager::ZddIndex, utils::UsizeOrPositiveInfinity};

///A cache for [`SetFamily`] which empties automatically when garbage collection occurs.
pub(crate) struct TempCache<'a, V: Eq + Hash, K, T = ZddIndex<V>> {
    holder: &'a ZddHolder<V>,
    cache: OpCache<K, T, RandomState>,
    generation: AtomicU64,
    size: AtomicU64,
}

pub trait TempCacheItem<'a, V: Eq + Hash> {
    type Output;
    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output;
    fn from_gc(x: &Self::Output) -> Self;
}

impl<'a, V: Eq + Hash + 'a> TempCacheItem<'a, V> for ZddIndex<V> {
    type Output = SetFamily<'a, V>;
    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
        SetFamily::from_set_family(*self, holder)
    }

    fn from_gc(x: &Self::Output) -> Self {
        x.as_raw()
    }
}

impl<'a, V, K, T> TempCache<'a, V, K, T>
where
    V: Eq + Hash,
    K: Eq + Hash + MemSize + Clone,
    T: TempCacheItem<'a, V> + MemSize + Clone,
{
    fn clear_if_not_current(&self) {
        let current = self.holder.current_generation();
        let our_gen = self.generation.load(Ordering::Acquire);
        let current_size = self.size.load(Ordering::Acquire);

        if current != our_gen
            && self
                .generation
                .compare_exchange(our_gen, current, Ordering::Release, Ordering::Relaxed)
                .is_ok()
        {
            //Entries from before the GC are stale but keep occupying memory, so only
            //invalidate them cheaply and keep counting them towards the cap.
            self.cache.clear();
        }

        if self.holder.temp_cache_max.is_some_and(|x| current_size > x) {
            //Drop roughly half the entries, keeping the accounting in sync via the
            //tracked sizes. Concurrent wipes are safe: each entry is only dropped
            //and counted once.
            let freed = self.cache.clear_half();
            if freed > 0 {
                self.size.fetch_sub(freed, Ordering::Release);
            }
        }
    }

    ///Retrieve a value from the cache
    pub fn get(&self, key: &K) -> Option<T::Output> {
        self.clear_if_not_current();
        self.cache.get(key).map(|s| s.to_gc(self.holder))
    }

    ///Insert a value to the cache.
    pub fn insert(&self, key: K, value: T::Output) -> T::Output {
        self.clear_if_not_current();
        let inner_value = T::from_gc(&value);
        let key_size = key.mem_size(SizeFlags::default()) as u64;
        let value_size = inner_value.mem_size(SizeFlags::default()) as u64;

        let old_value = self.cache.insert_with_size(key, inner_value, key_size + value_size);

        match old_value {
            Some((old_key, old_value)) => {
                self.size
                    .fetch_add(key_size + value_size, Ordering::Relaxed);
                let old_key_size = old_key.mem_size(SizeFlags::default()) as u64;
                let old_value_size = old_value.mem_size(SizeFlags::default()) as u64;
                self.size
                    .fetch_sub(old_key_size + old_value_size, Ordering::Relaxed);
            }
            None => {
                self.size
                    .fetch_add(key_size + value_size, Ordering::Relaxed);
            }
        }

        value
    }
}

impl<V: Eq + Hash> ZddHolder<V> {
    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    ///Create a [`TempCache`] which allows for the construction of algorithms that require hashing
    ///of partial results. Crucially, this hashmap will empty if garbage collection is triggered,
    ///allowing for caching without requiring all partial values to be held indefinitely.
    pub(crate) fn create_temporary_cache<K: Eq + Hash, T>(
        &self,
        capacity: usize,
    ) -> TempCache<'_, V, K, T> {
        TempCache {
            holder: self,
            cache: OpCache::with_hasher_and_capacity(capacity, RandomState::default()),
            generation: AtomicU64::from(self.current_generation()),
            size: AtomicU64::new(0),
        }
    }
}

macro_rules! impl_temp_cache_item_copy {
    ($($t:ty),* $(,)?) => {
        $(
            impl<'a, V: Eq + Hash + 'a> TempCacheItem<'a, V> for $t {
                type Output = Self;

                fn to_gc(&self, _holder: &'a ZddHolder<V>) -> Self::Output {
                    *self
                }

                fn from_gc(x: &Self::Output) -> Self {
                    *x
                }
            }
        )*
    };
}

use ordered_float::{NotNan, OrderedFloat};

impl_temp_cache_item_copy!(
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    NotNan<f32>,
    NotNan<f64>,
    OrderedFloat<f32>,
    OrderedFloat<f64>,
    f32,
    f64,
    bool,
    char,
    UsizeOrPositiveInfinity,
    (),
    std::cmp::Ordering,
    std::time::Duration,
);

impl<'a, V, T, const N: usize> TempCacheItem<'a, V> for [T; N]
where
    V: Eq + Hash + 'a,
    T: TempCacheItem<'a, V> + Clone,
    T::Output: Clone,
{
    type Output = [T::Output; N];

    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
        self.clone().map(|x| x.to_gc(holder))
    }

    fn from_gc(x: &Self::Output) -> Self {
        x.clone().map(|x| T::from_gc(&x))
    }
}

impl<'a, V, T> TempCacheItem<'a, V> for Vec<T>
where
    V: Eq + Hash + 'a,
    T: TempCacheItem<'a, V> + Clone,
    T::Output: Clone,
{
    type Output = Vec<T::Output>;

    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
        self.iter().map(|x| x.to_gc(holder)).collect()
    }

    fn from_gc(x: &Self::Output) -> Self {
        x.iter().map(|x| T::from_gc(x)).collect()
    }
}

impl<'a, V, T, K> TempCacheItem<'a, V> for BTreeMap<K, T>
where
    V: Eq + Hash + 'a,
    T: TempCacheItem<'a, V> + Clone,
    T::Output: Clone,
    K: TempCacheItem<'a, V> + Clone + Ord,
    K::Output: Clone + Ord,
{
    type Output = BTreeMap<K::Output, T::Output>;

    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
        self.iter()
            .map(|(k, v)| (k.to_gc(holder), v.to_gc(holder)))
            .collect()
    }

    fn from_gc(x: &Self::Output) -> Self {
        x.iter()
            .map(|(k, v)| (K::from_gc(k), T::from_gc(v)))
            .collect()
    }
}

impl<'a, V, T> TempCacheItem<'a, V> for Option<T>
where
    V: Eq + Hash + 'a,
    T: TempCacheItem<'a, V> + Clone,
    T::Output: Clone,
{
    type Output = Option<T::Output>;

    fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
        self.clone().map(|x| x.to_gc(holder))
    }

    fn from_gc(x: &Self::Output) -> Self {
        x.clone().map(|x| T::from_gc(&x))
    }
}

macro_rules! impl_temp_cache_item_tuple {
    () => {};
    ($T:ident $($U:ident)*) => {
        impl_temp_cache_item_tuple!($($U)*);

        impl<'a, V, $T, $($U),*> TempCacheItem<'a, V> for ($T, $($U,)*)
        where
            V: Eq + Hash + 'a,
            $T: TempCacheItem<'a, V> + Clone,
            $T::Output: Clone,
            $($U: TempCacheItem<'a, V> + Clone,)*
            $($U::Output: Clone,)*
        {
            type Output = ($T::Output, $($U::Output,)*);

            #[allow(non_snake_case)]
            fn to_gc(&self, holder: &'a ZddHolder<V>) -> Self::Output {
                let ($T, $($U,)*) = self.clone();
                ($T.to_gc(holder), $($U.to_gc(holder),)*)
            }

            #[allow(non_snake_case)]
            fn from_gc(x: &Self::Output) -> Self {
                let ($T, $($U,)*) = x.clone();
                (<$T>::from_gc(&$T), $(<$U>::from_gc(&$U),)*)
            }
        }
    };
}

impl_temp_cache_item_tuple! { A B C D E F G H I J K L }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_cache_byte_cap_frees_memory() {
        let mut holder = ZddHolder::<usize>::new();
        //Small enough that the cap fires partway through the inserts below.
        holder.set_temp_cache_size(Some(4096));

        let cache: TempCache<'_, usize, Vec<usize>, usize> =
            holder.create_temporary_cache(1024_usize.next_power_of_two());

        //Each key is ~800 bytes, so the cap is hit after a handful of inserts.
        for i in 0..100_usize {
            cache.insert((0..100).chain([i]).collect(), i);
        }

        //Without the cap, the size would be ~80_000. The cap must have cleared
        //the cache along the way, so the counter is far below that.
        let size = cache.size.load(Ordering::Acquire);
        assert!(0 < size && size < 4096 + 1000, "size was {size}");
    }

    ///The temp cache must be invalidated when a GC occurs.
    #[test]
    fn temp_cache_invalidated_by_gc() {
        let holder = ZddHolder::<usize>::new();

        let cache: TempCache<'_, usize, usize, usize> =
            holder.create_temporary_cache(1024_usize.next_power_of_two());
        cache.insert(1, 1);
        assert_eq!(cache.get(&1), Some(1));
        holder.gc(false);
        assert_eq!(cache.get(&1), None);
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    ///The size counter must stay bounded near the cap despite half-wipes
    ///refilling the surviving half.
    #[test]
    fn temp_cache_size_stays_bounded_under_repeated_wipes() {
        let mut holder = ZddHolder::<usize>::new();
        holder.set_temp_cache_size(Some(4096));
        let cache: TempCache<'_, usize, Vec<usize>, usize> =
            holder.create_temporary_cache(1024);
        let entry_size = 840;
        for i in 0..1000usize {
            cache.insert((0..100).chain([i]).collect(), i);
            let size = cache.size.load(Ordering::Acquire);
            assert!(
                size < 4096 + 2 * entry_size,
                "size {size} escaped the cap at insert {i}"
            );
        }
    }
}
