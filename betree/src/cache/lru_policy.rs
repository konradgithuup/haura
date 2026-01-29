//! This module provides an LRU cache implementation.
//!
//! LRU is implemented on a doubly linked list.

use std::{
    collections::{linked_list::Iter, LinkedList},
    hash::Hash,
};

use crate::cache::{
    cache_policy::{CacheIterator, CachePolicy},
    RemoveError,
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

    fn on_access(&mut self, accessed_key: &K, is_write: bool) {
        _ = is_write;
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

    fn pick_eviction_candidate(&self) -> Option<&K> {
        self.lru_list.back()
    }

    fn iter<'a>(&'a self) -> impl CacheIterator<'a, K>
    where
        K: 'a,
    {
        LRUIterator {
            iter: self.lru_list.iter(),
        }
    }
}

struct LRUIterator<'a, T> {
    iter: Iter<'a, T>,
}

impl<'a, T> CacheIterator<'a, T> for LRUIterator<'a, T> {}

impl<'a, T: 'a> Iterator for LRUIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
