//! A simple parallel hashmap used for caching results of operations.
//!
//! Hash collisions are resolved by overwriting the old value, trading accuracy
//! for simplicity and speed.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hash},
    sync::RwLock,
};

///A single slot of an [`OpCache`].
struct Slot<K, V> {
    hash: u64,
    entry: Option<(K, V)>,
}

impl<K, V> Default for Slot<K, V> {
    fn default() -> Self {
        Self {
            hash: 0,
            entry: None,
        }
    }
}

///A simple parallel hashmap.
///
///Hash collisions overwrite the old value, so this is only appropriate for
///caches where recomputation is cheap.
///
///Each slot is guarded by its own lock, so operations on different keys
///proceed in parallel.
///
///# Examples
///```
///# use zuddy::utils::op_cache::OpCache;
///let cache = OpCache::new(4);
///cache.insert(1, 10);
///assert_eq!(cache.get(&1), Some(10));
///```
pub struct OpCache<K, V, S = RandomState> {
    slots: Vec<RwLock<Slot<K, V>>>,
    mask: usize,
    hasher: S,
}

impl<K: Hash + Eq, V: Clone, S: BuildHasher> OpCache<K, V, S> {
    ///Returns the value stored for `key`, or `None` if it is not cached.
    ///
    ///# Panics
    ///May panic if the internal lock is poisoned.
    pub fn get(&self, key: &K) -> Option<V> {
        let slot = &self.slots[self.index(key)];
        let guard = slot.read().unwrap();
        match &guard.entry {
            Some((k, v)) if k == key => Some(v.clone()),
            _ => None,
        }
    }

    ///Inserts a key value pair into the cache, overwriting any hash collisions.
    ///
    ///# Panics
    ///May panic if the internal lock is poisoned.
    pub fn insert(&self, key: K, value: V) {
        let hash = self.hash_one(&key);
        let slot = &self.slots[self.index(&key)];
        let mut guard = slot.write().unwrap();
        guard.hash = hash;
        guard.entry = Some((key, value));
    }
}

impl<K: Hash, V, S: BuildHasher> OpCache<K, V, S> {
    fn hash_one(&self, key: &K) -> u64 {
        self.hasher.hash_one(key)
    }

    fn index(&self, key: &K) -> usize {
        usize::try_from(self.hash_one(key)).unwrap_or(usize::MAX) & self.mask
    }

    ///Removes all entries from the cache.
    ///
    ///# Panics
    ///May panic if an internal lock is poisoned.
    pub fn clear(&self) {
        for slot in &self.slots {
            let mut guard = slot.write().unwrap();
            guard.hash = 0;
            guard.entry = None;
        }
    }

    ///Returns true if the cache contains no entries.
    ///
    ///# Panics
    ///May panic if an internal lock is poisoned.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.read().unwrap().entry.is_none())
    }

    ///The number of slots in the cache.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.len()
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
        }
    }
}

impl<K: Hash + Eq, V> OpCache<K, V> {
    ///Creates a new [`OpCache`] with at least `capacity` slots.
    ///
    ///The capacity is rounded up to a power of two.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self::with_hasher_and_capacity(capacity, RandomState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn new_cache_is_empty() {
        let cache = OpCache::<usize, usize>::new(8);
        assert!(cache.is_empty());
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn insert_then_get() {
        let cache = OpCache::new(8);
        cache.insert(1, 10);
        cache.insert(2, 20);
        assert_eq!(cache.get(&1), Some(10));
        assert_eq!(cache.get(&2), Some(20));
        assert_eq!(cache.get(&3), None);
        assert!(!cache.is_empty());
    }

    #[test]
    fn overwrite_same_key() {
        let cache = OpCache::new(8);
        cache.insert(1, 10);
        cache.insert(1, 999);
        assert_eq!(cache.get(&1), Some(999));
    }

    #[test]
    fn clear_empties_cache() {
        let cache = OpCache::new(8);
        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn capacity_rounds_up_to_power_of_two() {
        assert_eq!(OpCache::<usize, usize>::new(1).capacity(), 2);
        assert_eq!(OpCache::<usize, usize>::new(5).capacity(), 8);
        assert_eq!(OpCache::<usize, usize>::new(16).capacity(), 16);
    }

    #[test]
    fn many_sequential_inserts_and_gets() {
        let cache = OpCache::new(1024);
        for i in 0..1000_u64 {
            cache.insert(i, i * 2);
            assert_eq!(cache.get(&i), Some(i * 2));
        }
    }

    #[test]
    fn colliding_keys_overwrite() {
        //Keys which land in the same slot overwrite each other.
        let cache = OpCache::new(2);
        cache.insert(1, 10);
        cache.insert(2, 20);
        //Both keys hash to slot 0 or 1; at most one survives.
        let results = [cache.get(&1), cache.get(&2)];
        assert!(results.contains(&Some(10)) || results.contains(&Some(20)));
    }

    #[test]
    fn parallel_inserts_and_gets() {
        //All threads insert the same pairs, so any survivor is correct.
        let cache = Arc::new(OpCache::new(256));
        let mut handles = vec![];
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            handles.push(std::thread::spawn(move || {
                for i in 0..1000_u64 {
                    cache.insert(i, i * 2);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        for i in 0..1000_u64 {
            let r = cache.get(&i);
            assert!(r.is_none() || r == Some(i * 2));
        }
    }

    #[test]
    fn values_are_dropped() {
        let cache = OpCache::new(8);
        let value = Arc::new(());
        cache.insert(1, Arc::clone(&value));
        assert_eq!(Arc::strong_count(&value), 2);
        cache.insert(2, Arc::clone(&value));
        cache.clear();
        assert_eq!(Arc::strong_count(&value), 1);
    }
}
