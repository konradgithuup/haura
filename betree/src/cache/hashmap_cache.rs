//! This module provides a Hashmap-based cache.
//!
//! The cache is initialized with an arbitrary caching policy (Clock, LRU,...).

use super::{Cache, ChangeKeyError, RemoveError};
use crate::{
    cache::{
        cache_policy::CachePolicy,
        cache_util::{CacheEntry, CacheStats, PinnedEntry},
    },
    size::SizeMut,
};
use std::{
    collections::HashMap,
    hash::Hash,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

/// A cache based on a `std::collections::HashMap` and a given `CachePolicy`.
pub struct HashmapCache<K, V, P> {
    map: HashMap<K, Arc<CacheEntry<V>>>,
    policy: Box<P>,
    capacity: usize,
    // Let's leak it
    size: &'static AtomicUsize,
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: u64,
    evictions: u64,
    removals: u64,
}

impl<'a, K: 'a + Hash + Eq, V: SizeMut, P: CachePolicy<K>> HashmapCache<K, V, P> {
    /// Returns a new cache instance with the given `capacity`.
    pub fn new(cache_policy: Box<P>, capacity: usize) -> Self {
        HashmapCache {
            map: Default::default(),
            policy: cache_policy,
            size: Box::leak(Default::default()),
            hits: Default::default(),
            misses: Default::default(),
            capacity,
            insertions: 0,
            evictions: 0,
            removals: 0,
        }
    }
}

impl<K, V, P> Cache for HashmapCache<K, V, P>
where
    K: Clone + Sized + Eq + Hash + Send + Sync + 'static,
    V: Sync + Send + SizeMut + 'static,
    P: CachePolicy<K>,
{
    type Key = K;
    type Value = V;
    type Policy = P;
    type ValueRef = PinnedEntry<V>;
    type Stats = CacheStats;

    fn new(capacity: usize, policy: Box<P>) -> Self {
        Self::new(policy, capacity)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    fn get(&self, key: &K, count_miss: bool) -> Option<Self::ValueRef> {
        if let Some(entry) = self.map.get(key).cloned() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            entry.referenced.store(true, Ordering::Relaxed);
            Some(PinnedEntry {
                size: self.size,
                entry,
            })
        } else {
            if count_miss {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
            None
        }
    }

    fn remove<F>(&mut self, key: &K, f: F) -> Result<V, RemoveError>
    where
        F: FnOnce(&mut V) -> usize,
    {
        self.verify();
        {
            let entry = self.map.get_mut(key).ok_or(RemoveError::NotPresent)?;
            Arc::get_mut(entry).ok_or(RemoveError::Pinned)?;
        }
        self.policy.on_remove(key);
        let entry = self.map.remove(key).unwrap();
        let mut value = Arc::try_unwrap(entry).ok().unwrap().value;
        let size = f(&mut value);
        self.removals += 1;
        self.size.fetch_sub(size, Ordering::Relaxed);
        self.verify();
        Ok(value)
    }

    fn force_remove(&mut self, key: &Self::Key, size: usize) -> bool {
        self.verify();
        self.policy.on_remove(key);
        if self.map.remove(key).is_none() {
            return false;
        }
        self.removals += 1;
        self.size.fetch_sub(size, Ordering::Relaxed);
        self.verify();
        true
    }

    fn change_key<E, F>(&mut self, key: &K, f: F) -> Result<(), ChangeKeyError<E>>
    where
        F: FnOnce(&K, &mut V, &dyn Fn(&K) -> bool) -> Result<K, E>,
    {
        self.verify();
        let new_key = {
            let second_ref: &Self = unsafe { &*(self as *mut _) };
            let entry = self.map.get_mut(key).ok_or(ChangeKeyError::NotPresent)?;
            let entry = Arc::get_mut(entry).ok_or(ChangeKeyError::Pinned)?;
            f(key, &mut entry.value, &|k| second_ref.contains_key(k))?
        };
        let entry = self.map.remove(key).unwrap();
        self.map.insert(new_key.clone(), entry);

        self.policy.update(key, new_key);

        self.verify();
        Ok(())
    }

    fn force_change_key(&mut self, key: &Self::Key, new_key: Self::Key) -> bool {
        self.verify();
        let entry = match self.map.remove(key) {
            None => return false,
            Some(entry) => entry,
        };
        self.map.insert(new_key.clone(), entry);

        self.policy.update(key, new_key);

        self.verify();
        true
    }

    fn evict<F>(&mut self, mut f: F) -> Option<(K, V)>
    where
        F: FnMut(&K, &mut V, &dyn Fn(&K) -> bool) -> Option<usize>,
    {
        self.verify();

        let key = match self.policy.pick_eviction_candidate().cloned() {
            None => {
                warn!("Unable to pick eviction candidate");
                return None;
            }
            Some(key) => key,
        };
        let eviction_successful = {
            let second_ref: &Self = unsafe { &*(self as *mut _) };
            let entry = self.map.get_mut(&key).unwrap();

            if let Some(entry) = Arc::get_mut(entry) {
                f(&key, &mut entry.value, &|k| second_ref.contains_key(k))
            } else {
                None
            }
        };

        let ret = if let Some(size) = eviction_successful {
            let value = self.map.remove(&key).unwrap();
            let value = Arc::try_unwrap(value).ok().unwrap().value;
            self.policy.on_remove(&key);

            self.evictions += 1;
            self.size.fetch_sub(size, Ordering::Relaxed);
            Some((key, value))
        } else {
            None
        };

        self.verify();

        ret
    }

    fn insert(&mut self, key: K, mut value: V, size: usize) {
        assert_eq!(value.cache_size(), size);

        let old_value = self.map.insert(
            key.clone(),
            Arc::new(CacheEntry {
                value,
                referenced: AtomicBool::new(false),
            }),
        );
        assert!(old_value.is_none());

        self.policy.on_add(key);

        self.insertions += 1;
        self.size.fetch_add(size, Ordering::Relaxed);
    }

    fn stats(&self) -> Self::Stats {
        CacheStats {
            cache_name: self.policy.name(),
            capacity: self.capacity,
            size: self.size.load(Ordering::Relaxed),
            len: self.map.len(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            insertions: self.insertions,
            evictions: self.evictions,
            removals: self.removals,
        }
    }

    fn iter<'b>(&'b self) -> Box<dyn Iterator<Item = &'b K> + 'b> {
        Box::new(self.policy.iter())
    }

    fn size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    // This is wildly unsafe, because it was hacked on top of a cache design which assumed interior
    // mutability, but it's only a debugging feature to locate faulty size adjustments, and if you
    // only run it without optimisations, the nasal demons might leave you alone.
    #[cfg(feature = "cache-paranoia")]
    fn verify(&mut self) {
        {
            let size = self
                .map
                .iter_mut()
                .map(|(k, mut v): (_, &mut Arc<CacheEntry<_>>)| {
                    let p: *mut CacheEntry<_> = Arc::as_ptr(&v) as *mut CacheEntry<_>;
                    let v2: &mut CacheEntry<V> = unsafe { &mut *p };
                    v2.value.cache_size()
                })
                .sum::<usize>();

            let actual = self.size.load(Ordering::Relaxed);
            if size != actual {
                log::error!(
                    "invalid cache size! supposed({}) != actual({})",
                    size,
                    actual
                );
            }
        }
    }

    #[cfg(not(feature = "cache-paranoia"))]
    #[inline(always)]
    fn verify(&mut self) {}
}
