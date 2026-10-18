//! A simple parallel hashmap used as a cache.
//!
//! Hash collisions are just overwritten

use std::{
    collections::hash_map::RandomState,
    fmt::Debug,
    hash::{BuildHasher, Hash},
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

///A single slot of an [`OpCache`].
struct Slot<K, V> {
    generation: u64,
    entry: Option<(K, V)>,
}

impl<K, V> Default for Slot<K, V> {
    fn default() -> Self {
        Self {
            generation: 0,
            entry: None,
        }
    }
}
///A simple parallel hashmap.
///
///# Examples
///```
///# use zuddy::utils::op_cache::OpCache;
///let cache = OpCache::new(4);
///cache.insert(1, 10);
///assert_eq!(cache.get(&1), Some(10));
///```
pub(crate) struct OpCache<K, V, S = RandomState> {
    slots: Vec<RwLock<Slot<K, V>>>,
    mask: usize,
    hasher: S,
    generation: AtomicU64,
}

impl<K, V, S> OpCache<K, V, S> {
    ///The current generation of the cache.
    ///
    ///Entries from a previous generation are logically cleared.
    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

impl<K: Hash + Eq, V: Clone, S: BuildHasher> OpCache<K, V, S> {
    ///Returns the value stored for `key`, or `None` if it is not cached.
    ///
    ///# Panics
    ///May panic if the internal lock is poisoned.
    pub fn get(&self, key: &K) -> Option<V> {
        let slot = &self.slots[self.index(key)];
        let guard = slot.read().unwrap();
        if guard.generation == self.current_generation() {
            match &guard.entry {
                Some((k, v)) if k == key => return Some(v.clone()),
                _ => (),
            }
        }
        None
    }

    ///Inserts a key value pair into the cache, overwriting any hash collisions.
    ///
    ///# Panics
    ///May panic if the internal lock is poisoned.
    pub fn insert(&self, key: K, value: V) {
        let slot = &self.slots[self.index(&key)];
        let mut guard = slot.write().unwrap();
        guard.generation = self.current_generation();
        guard.entry = Some((key, value));
    }
}

#[expect(clippy::missing_fields_in_debug)]
impl<K: Debug, V, S> Debug for OpCache<K, V, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpCache")
            .field("n_slots", &self.slots.len())
            .finish()
    }
}

impl<K: Hash, V, S: BuildHasher> OpCache<K, V, S> {
    fn hash_one(&self, key: &K) -> u64 {
        self.hasher.hash_one(key)
    }

    fn index(&self, key: &K) -> usize {
        usize::try_from(self.hash_one(key)).unwrap_or(usize::MAX) & self.mask
    }
}

impl<K: Hash, V, S: BuildHasher> OpCache<K, V, S> {
    ///Logically removes all entries from the cache in constant time.
    ///
    ///Existing entries are left in place but become invisible to [`get`].
    ///They are physically overwritten by later inserts.
    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    #[cfg(test)]
    ///Returns true if the cache contains no entries.
    ///
    ///# Panics
    ///May panic if an internal lock is poisoned.
    pub fn is_empty(&self) -> bool {
        let generation = self.current_generation();
        self.slots
            .iter()
            .all(|s| s.read().unwrap().generation != generation)
    }

    ///Creates a new [`OpCache`] with at least `capacity` slots.
    ///
    ///The capacity is rounded up to a power of two.
    #[must_use]
    pub fn with_hasher_and_capacity(capacity: usize, hasher: S) -> Self {
        let n_slots = capacity.max(2).next_power_of_two();
        Self {
            slots: (0..n_slots).map(|_| RwLock::default()).collect(),
            mask: n_slots - 1,
            hasher,
            generation: AtomicU64::new(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHasher;
    use std::{hash::BuildHasherDefault, sync::Arc};

    type DeterministicCache = OpCache<usize, usize, BuildHasherDefault<FxHasher>>;

    fn cache(capacity: usize) -> DeterministicCache {
        OpCache::with_hasher_and_capacity(capacity, BuildHasherDefault::default())
    }

    #[test]
    fn new_cache_is_empty() {
        let cache = cache(8);
        assert!(cache.is_empty());
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn insert_then_get() {
        let cache = cache(8);
        cache.insert(1, 10);
        cache.insert(2, 20);
        assert_eq!(cache.get(&1), Some(10));
        assert_eq!(cache.get(&2), Some(20));
        assert_eq!(cache.get(&3), None);
        assert!(!cache.is_empty());
    }

    #[test]
    fn overwrite_same_key() {
        let cache = cache(8);
        cache.insert(1, 10);
        cache.insert(1, 999);
        assert_eq!(cache.get(&1), Some(999));
    }

    #[test]
    fn clear_empties_cache() {
        let cache = cache(8);
        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn capacity_rounds_up_to_power_of_two() {
        assert_eq!(cache(1).slots.len(), 2);
        assert_eq!(cache(5).slots.len(), 8);
        assert_eq!(cache(16).slots.len(), 16);
    }

    #[test]
    fn many_sequential_inserts_and_gets() {
        let cache = cache(1024);
        for i in 0..1000_usize {
            cache.insert(i, i * 2);
            assert_eq!(cache.get(&i), Some(i * 2));
        }
    }

    #[test]
    fn colliding_keys_overwrite() {
        let cache = cache(1);
        cache.insert(1, 10);
        cache.insert(2, 20);
        let results = [cache.get(&1), cache.get(&2)];
        assert!(results.contains(&Some(10)) || results.contains(&Some(20)));
    }

    #[test]
    fn parallel_inserts_and_gets() {
        let cache = Arc::new(cache(256));
        let mut handles = vec![];
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            handles.push(std::thread::spawn(move || {
                for i in 0..1000_usize {
                    cache.insert(i, i * 2);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        for i in 0..1000_usize {
            let r = cache.get(&i);
            assert!(r.is_none() || r == Some(i * 2));
        }
    }

    #[test]
    fn values_are_dropped() {
        let cache: OpCache<usize, Arc<()>, BuildHasherDefault<FxHasher>> =
            OpCache::with_hasher_and_capacity(8, BuildHasherDefault::default());
        let value = Arc::new(());
        cache.insert(1, Arc::clone(&value));
        assert_eq!(Arc::strong_count(&value), 2);
        cache.insert(1, Arc::new(()));
        assert_eq!(Arc::strong_count(&value), 1);
        cache.insert(1, Arc::clone(&value));
        drop(cache);
        assert_eq!(Arc::strong_count(&value), 1);
    }
}
