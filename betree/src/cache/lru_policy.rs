//! This module provides an LRU cache implementation.
//!
//! LRU is implemented on a doubly linked list.

use std::{
    collections::LinkedList,
    hash::Hash,
};

use crate::cache::{
    cache_policy::CachePolicy,
    CacheAccess, RemoveError,
};

/// Implements an LRU Cache Policy ontop of a doubly linked list.
/// Keys are added at the front. On eviction, the tail is evicted.
pub struct LRUCachePolicy<K> {
    lru_list: LinkedList<K>,
}

impl<K: Hash + Eq> LRUCachePolicy<K> {
    /// Returns new cache instance with the given `capacity`.
    pub fn new() -> Self {
        LRUCachePolicy {
            lru_list: Default::default(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for LRUCachePolicy<K> {
    fn name(&self) -> &'static str {
        "LRU"
    }

    fn max_evict_failures(&self) -> usize {
        return self.lru_list.len();
    }

    fn on_access(&mut self, accessed_key: &K, _access: CacheAccess) {
        match self.lru_list.extract_if(|k| k == accessed_key).nth(0) {
            Some(k) => self.lru_list.push_front(k),
            None => (),
        };
    }

    fn on_add(&mut self, added_key: K) {
        self.lru_list.push_front(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        match self.lru_list.extract_if(|k| k == removed_key).count() {
            0 => Some(RemoveError::NotPresent),
            _ => None,
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        _ = self.lru_list.extract_if(|k| k == old_key);
        self.lru_list.push_front(new_key);
    }

    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(&K, usize)> {
        for key in self.lru_list.iter().rev() {
            if let Some(size) = f(key) {
                return Some((key, size));
            }
        }

        None
    }
}
