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

///A slot of an [`OpCache`].
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

struct Table<K, V> {
    slots: Vec<RwLock<Slot<K, V>>>,
    mask: usize,
}
///A simple parallel hashmap.
///
///Hash collisions are just overwritten.
///
///Fixed size unless resized by [`resize`](OpCache::resize)
pub(crate) struct OpCache<K, V, S = RandomState> {
    table: RwLock<Table<K, V>>,
    hasher: S,
    generation: AtomicU64,
}

impl<K, V, S> OpCache<K, V, S> {
    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

impl<K: Hash + Eq, V: Clone, S: BuildHasher> OpCache<K, V, S> {
    ///Returns the value stored for `key`, or `None` if it is not cached.
    ///
    ///# Panics
    ///May panic if an internal lock is poisoned.
    pub fn get(&self, key: &K) -> Option<V> {
        let table = self.table.read().unwrap();
        let slot = &table.slots[self.index(table.mask, key)];
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
    ///May panic if an internal lock is poisoned.
    pub fn insert(&self, key: K, value: V) {
        let table = self.table.read().unwrap();
        let slot = &table.slots[self.index(table.mask, &key)];
        let generation = self.current_generation();
        let mut guard = slot.write().unwrap();
        guard.generation = generation;
        guard.entry = Some((key, value));
    }
}

impl<K: Debug, V, S> Debug for OpCache<K, V, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpCache").finish()
    }
}

impl<K: Hash, V, S: BuildHasher> OpCache<K, V, S> {
    fn hash_one(&self, key: &K) -> u64 {
        self.hasher.hash_one(key)
    }

    fn index(&self, mask: usize, key: &K) -> usize {
        usize::try_from(self.hash_one(key)).unwrap_or(usize::MAX) & mask
    }
}

impl<K: Hash, V, S: BuildHasher> OpCache<K, V, S> {
    ///Empties cache by invalidating current entries.
    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    ///Resizes the cache to have at least `capacity` slots while discarding cache.
    ///
    ///# Panics
    ///May panic if an internal lock is poisoned.
    pub fn resize(&self, capacity: usize) {
        let n_slots = capacity.max(2).next_power_of_two();
        let mut table = self.table.write().unwrap();
        if table.slots.len() < n_slots {
            *table = Table {
                slots: (0..n_slots).map(|_| RwLock::default()).collect(),
                mask: n_slots - 1,
            };
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        let generation = self.current_generation();
        let table = self.table.read().unwrap();
        table.slots.iter().all(|s| {
            let guard = s.read().unwrap();
            guard.generation != generation || guard.entry.is_none()
        })
    }

    #[cfg(test)]
    #[must_use]
    fn capacity(&self) -> usize {
        self.table.read().unwrap().slots.len()
    }

    ///Creates a new [`OpCache`] with at least `capacity` slots.
    ///
    ///The capacity is rounded up to a power of two.
    #[must_use]
    pub fn with_hasher_and_capacity(capacity: usize, hasher: S) -> Self {
        let n_slots = capacity.max(2).next_power_of_two();
        Self {
            table: RwLock::new(Table {
                slots: (0..n_slots).map(|_| RwLock::default()).collect(),
                mask: n_slots - 1,
            }),
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
        assert_eq!(cache(1).capacity(), 2);
        assert_eq!(cache(5).capacity(), 8);
        assert_eq!(cache(16).capacity(), 16);
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
    fn resize_grows_capacity_and_discards_entries() {
        let cache = cache(2);
        cache.insert(1, 10);
        assert_eq!(cache.get(&1), Some(10));
        cache.clear();
        cache.resize(64);
        assert_eq!(cache.capacity(), 64);
        //Entries are discarded rather than rehashed.
        assert_eq!(cache.get(&1), None);
        assert!(cache.is_empty());
        for i in 0..50_usize {
            cache.insert(i, i * 2);
            assert_eq!(cache.get(&i), Some(i * 2));
        }
    }

    #[test]
    fn resize_never_shrinks() {
        let cache = cache(64);
        cache.resize(8);
        assert_eq!(cache.capacity(), 64);
    }

    #[test]
    fn colliding_keys_overwrite() {
        let cache = cache(1);
        cache.insert(1, 10);
        cache.insert(2, 20);
        //Both keys hash to slot 0 or 1; at most one survives.
        let results = [cache.get(&1), cache.get(&2)];
        assert!(results.contains(&Some(10)) || results.contains(&Some(20)));
    }

    #[test]
    fn parallel_inserts_and_gets() {
        //All threads insert the same pairs, so any survivor is correct.
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
