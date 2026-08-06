use std::{
    collections::BTreeMap,
    hash::Hash,
    sync::atomic::{AtomicU64, Ordering},
};

use dashmap::DashMap;

use crate::{SetFamily, ZddHolder, manager::ZddIndex, utils::UsizeOrPositiveInfinity};

///A cache for [`SetFamily`] which empties automatically when garbage collection occurs.
pub(crate) struct TempCache<'a, V: Eq + Hash, K, T = ZddIndex<V>> {
    holder: &'a ZddHolder<V>,
    cache: DashMap<K, T>,
    generation: AtomicU64,
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
    K: Eq + Hash,
    T: TempCacheItem<'a, V>,
{
    fn clear_if_not_current(&self) {
        let current = self.holder.current_generation();
        let our_gen = self.generation.load(Ordering::Acquire);

        if current != our_gen
            && self
                .generation
                .compare_exchange(our_gen, current, Ordering::Release, Ordering::Relaxed)
                .is_ok()
        {
            self.cache.clear();
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
        self.cache.insert(key, T::from_gc(&value));
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
    pub(crate) fn create_temporary_cache<K: Eq + Hash, T>(&self) -> TempCache<'_, V, K, T> {
        TempCache {
            holder: self,
            cache: DashMap::new(),
            generation: AtomicU64::from(self.current_generation()),
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
