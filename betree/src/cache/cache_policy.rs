//! This module provides the cache policy interface.

use crate::cache::{CacheAccess, RemoveError};

/// Cache policy
pub trait CachePolicy<K>: Sync + Send {
    /// Get name of policy.
    fn name(&self) -> &'static str;

    /// The maximum number of consecutive eviction failures.
    fn max_evict_failures(&self) -> usize;

    /// Updates cache policy after object access.
    fn on_access(&mut self, accessed_key: &K, access: CacheAccess);

    /// Updates cache policy after an object is added.
    fn on_add(&mut self, added_key: K);

    /// Updates cache policy after an object is removed.
    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError>;

    /// Replace a key with another.
    fn update(&mut self, old_key: &K, new_key: K);

    /// Returns the key of the least useful cache object (according
    /// to the cache policy), or `Option:None` if a selection is impossible.
    /// Internally, the policy may perform updates to prevent the same candidate
    /// from being chosen every time.
    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(&K, usize)>;
}
