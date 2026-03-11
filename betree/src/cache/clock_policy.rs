//! This module provides a clock cache implementation

use std::{
    hash::Hash,
    sync::atomic::{AtomicBool, Ordering},
};

use gxhash::HashMap;

use crate::cache::{cache_policy::CachePolicy, clock::Clock, CacheAccess};

/// Implements a Clock Cache Policy ontop of a circular linked list.
pub struct ClockCachePolicy<K> {
    clock: Clock<K>,
    ref_map: HashMap<K, AtomicBool>,
}

impl<K: Hash + Eq> ClockCachePolicy<K> {
    /// Init clock cache policy
    pub fn new() -> Self {
        ClockCachePolicy {
            clock: Default::default(),
            ref_map: Default::default(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for ClockCachePolicy<K> {
    fn name(&self) -> &'static str {
        "Clock"
    }

    fn max_evict_failures(&self) -> usize {
        self.ref_map.len() * 2
    }

    fn on_access(&self, accessed_key: &K, _access: CacheAccess) {
        if let Some(is_ref) = self.ref_map.get(accessed_key) {
            is_ref.store(true, Ordering::Relaxed);
        }
    }

    fn on_add(&mut self, added_key: K) {
        self.clock.push_back(added_key.clone());
        self.ref_map.insert(added_key, AtomicBool::new(true));
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<super::RemoveError> {
        self.clock.retain(|entry| entry != removed_key);
        self.ref_map.remove(removed_key);
        None
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        if let Some(entry) = self.clock.iter_mut().find(|entry| *entry == old_key) {
            *entry = new_key;
        }
    }

    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(K, usize)> {
        for _ in 0..self.max_evict_failures() {
            let key = self.clock.peek_front().cloned()?;

            let was_referenced = *self.ref_map.get_mut(&key)?.get_mut();
            self.ref_map.get(&key)?.store(false, Ordering::Relaxed);

            if was_referenced {
                // pass
            } else if let Some(size) = f(&key) {
                return Some((key, size));
            }

            self.clock.next();
        }

        None
    }
}
