//! This module provides an LRU cache implementation.
//!
//! LRU is implemented on a doubly linked list.

use std::{hash::Hash, slice::Iter};

use rand::random;

use crate::cache::{
    cache_policy::{CacheIterator, CachePolicy},
    CacheAccess, RemoveError,
};

/// Implements an LRU Cache Policy ontop of a doubly linked list.
/// Keys are added at the front. On eviction, the tail is evicted.
pub struct RandomCachePolicy<K> {
    inner: Vec<K>,
}

impl<K: Hash + Eq> RandomCachePolicy<K> {
    /// Returns new cache instance with the given `capacity`.
    pub fn new() -> Self {
        RandomCachePolicy {
            inner: Default::default(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for RandomCachePolicy<K> {
    fn name(&self) -> &'static str {
        "LRU"
    }

    fn max_evict_failures(&self) -> usize {
        return self.inner.len();
    }

    fn on_access(&mut self, _accessed_key: &K, _access: CacheAccess) {
        // do nothing
    }

    fn on_add(&mut self, added_key: K) {
        self.inner.push(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        match self.inner.iter().position(|k| k == removed_key) {
            Some(idx) => {
                self.inner.swap_remove(idx);
                return None;
            }
            None => Some(RemoveError::NotPresent),
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        if let Some(idx) = self.inner.iter().position(|k| k == old_key) {
            _ = std::mem::replace(&mut self.inner[idx], new_key);
        }
    }

    fn pick_eviction_candidate(
        &mut self,
        mut f: impl FnMut(&K) -> Option<usize>,
    ) -> Option<(&K, usize)> {
        let len = self.max_evict_failures();
        let random_offset = random::<usize>() % len;

        for i in 0..len {
            let idx = (i + random_offset) % len;
            if let Some(size) = f(&self.inner[idx]) {
                return Some((&self.inner[idx], size));
            }
        }

        None
    }

    fn iter<'a>(&'a self) -> impl CacheIterator<'a, K>
    where
        K: 'a,
    {
        RandomIterator {
            iter: self.inner.iter(),
        }
    }
}

struct RandomIterator<'a, T> {
    iter: Iter<'a, T>,
}

impl<'a, T> CacheIterator<'a, T> for RandomIterator<'a, T> {}

impl<'a, T: 'a> Iterator for RandomIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
