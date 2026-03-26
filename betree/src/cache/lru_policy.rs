//! This module provides an LRU cache implementation.
//!
//! LRU is implemented on a doubly linked list.

use crate::cache::linked_list::LinkedKeySet;
use crate::cache::{cache_policy::CachePolicy, CacheAccess, RemoveError};
use parking_lot::Mutex;
use std::hash::Hash;

/// Implements an LRU Cache Policy on top of a highly efficient O(1) doubly linked list.
/// Keys are added at the front (MRU). On eviction, the tail (LRU) is evicted.
pub struct LRUCachePolicy<K> {
    lru_list: Mutex<LinkedKeySet<K>>,
}

impl<K: Clone + Hash + Eq> LRUCachePolicy<K> {
    /// Returns new cache instance with the given `capacity`.
    pub fn new() -> Self {
        LRUCachePolicy {
            lru_list: Mutex::new(LinkedKeySet::new()),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for LRUCachePolicy<K> {
    fn name(&self) -> &'static str {
        "LRU"
    }

    fn max_evict_failures(&self) -> usize {
        return self.lru_list.lock().len();
    }

    fn on_access(&self, accessed_key: &K, _access: CacheAccess) {
        let mut list = self.lru_list.lock();
        if list.contains(accessed_key) {
            // push_front moves the item to the MRU position if it exists
            list.push_front(accessed_key.clone());
        }
    }

    fn on_add(&mut self, added_key: K) {
        self.lru_list.get_mut().push_front(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        if self.lru_list.get_mut().remove(removed_key) {
            None
        } else {
            Some(RemoveError::NotPresent)
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        let list = self.lru_list.get_mut();
        list.remove(old_key);
        list.push_front(new_key);
    }

    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(K, usize)> {
        let list = self.lru_list.get_mut();
        for key in list.iter_lru() {
            if let Some(size) = f(key) {
                return Some((key.clone(), size));
            }
        }

        None
    }
}
