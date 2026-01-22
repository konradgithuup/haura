//! This module provides the cache policy interface.

use crate::cache::RemoveError;

/// Cache policy
pub trait CachePolicy<'a, K: 'a, I: Iterator<Item = &'a K>>: Sync + Send {
    /// Get name of policy.
    fn name(&self) -> &'static str;

    /// Updates cache policy after object access.
    fn on_access(&mut self, accessed_key: &K, is_write: bool);

    /// Updates cache policy after an object is added.
    fn on_add(&mut self, added_key: K);

    /// Updates cache policy after an object is removed.
    fn on_remove(&mut self, removed_key: &K) -> RemoveError;

    /// Replace a key with another.
    fn update(&mut self, old_key: &K, new_key: K);

    /// Returns the key of the least useful cache object (according
    /// to the cache policy), or `Option:None` if a selection is impossible.
    fn pick_eviction_candidate(&self) -> Option<&K>;

    /// Returns an iterator over the cache entries as layed out in the cache.
    fn iter(&self) -> I;
}
