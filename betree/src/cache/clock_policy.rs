//! This module provides a clock cache implementation

use std::hash::Hash;

use crate::cache::{cache_policy::CachePolicy, clock::Clock};

/// Implements a Clock Cache Policy ontop of a circular linked list.
pub struct ClockCachePolicy<K> {
    clock: Clock<K>,
}

impl<K: Hash + Eq> ClockCachePolicy<K> {
    /// Init clock cache policy
    pub fn new() -> Self {
        ClockCachePolicy {
            clock: Default::default(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for ClockCachePolicy<K> {
    fn name(&self) -> &'static str {
        "Clock"
    }

    fn max_evict_failures(&self) -> usize {
        return self.clock.len() * 2;
    }

    fn on_access(&mut self, _accessed_key: &K, _is_write: bool) {
        // do nothing?
    }

    fn on_add(&mut self, added_key: K) {
        self.clock.push_back(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<super::RemoveError> {
        self.clock.retain(|entry| entry != removed_key);
        None
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        if let Some(entry) = self.clock.iter_mut().find(|entry| *entry == old_key) {
            *entry = new_key;
        }
    }

    fn pick_eviction_candidate(&self) -> Option<&K> {
        self.clock.peek_front()
    }

    fn iter<'a>(&'a self) -> impl super::cache_policy::CacheIterator<'a, K>
    where
        K: 'a,
    {
        self.clock.iter()
    }
}
