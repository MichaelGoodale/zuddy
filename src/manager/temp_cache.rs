use super::*;

///A cache for [`SetFamily`] which empties automatically when garbage collection occurs.
pub(crate) struct TempCache<'a, V: Eq + Hash, K, T = ZddIndex<V>> {
    holder: &'a ZddHolder<V>,
    cache: DashMap<K, T>,
    generation: AtomicU64,
}

pub(crate) trait TempCacheItem<'a, V: Eq + Hash> {
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
                .compare_exchange(our_gen, current, Release, Relaxed)
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
    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    ///Create a [`TempCache`] which allows for the construction of algorithms that require hashing
    ///of partial results. Crucially, this hashmap will empty if garbage collection is triggered,
    ///allowing for caching without requiring all partial values to be held indefinitely.
    pub(crate) fn create_temporary_cache<K: Eq + Hash>(&self) -> TempCache<'_, V, K> {
        TempCache {
            holder: self,
            cache: DashMap::new(),
            generation: AtomicU64::from(self.current_generation()),
        }
    }
}
