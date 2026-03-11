use lfu_cache::LfuCache;
use parking_lot::Mutex;
use std::hash::Hash;

use crate::cache::{cache_policy::CachePolicy, CacheAccess, RemoveError};

/// Implements an LFU Cache Policy using `lfu_cache::LfuCache`.
pub struct LFUCachePolicy<K: Hash + Eq> {
    lfu: Mutex<LfuCache<K, ()>>,
}

impl<K: Hash + Eq> LFUCachePolicy<K> {
    /// Returns new cache policy instance.
    pub fn new() -> Self {
        LFUCachePolicy {
            lfu: Mutex::new(LfuCache::unbounded()),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for LFUCachePolicy<K> {
    fn name(&self) -> &'static str {
        "LFU"
    }

    fn max_evict_failures(&self) -> usize {
        self.lfu.lock().len()
    }

    fn on_access(&self, accessed_key: &K, _access: CacheAccess) {
        // LfuCache's `get` mutates its internal state to update frequencies.
        self.lfu.lock().get(accessed_key);
    }

    fn on_add(&mut self, added_key: K) {
        self.lfu.get_mut().insert(added_key, ());
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        match self.lfu.get_mut().remove(removed_key) {
            Some(_) => None,
            None => Some(RemoveError::NotPresent),
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        let lfu = self.lfu.get_mut();
        let freq = match lfu.remove_with_frequency(old_key) {
            Some((_, freq)) => freq,
            None => 0,
        };
        lfu.insert_with_frequency(new_key, (), freq);
    }

    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(K, usize)> {
        let mut unevictable = Vec::new();
        let mut result = None;

        let lfu = self.lfu.get_mut();

        while let Some((k, v, freq)) = lfu.pop_lfu_key_value_frequency() {
            if let Some(size) = f(&k) {
                // We must reinsert it, because HashmapCache expects the key to still exist in the
                // policy, so that it can be removed cleanly via `on_remove` shortly after.
                lfu.insert_with_frequency(k.clone(), v, freq);
                result = Some((k, size));
                break;
            }
            unevictable.push((k, v, freq));
        }

        // Restore any unevictable elements that we popped off
        for (k, v, freq) in unevictable {
            lfu.insert_with_frequency(k, v, freq);
        }

        result
    }
}
