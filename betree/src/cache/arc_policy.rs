use crate::cache::linked_list::LinkedKeySet;
use crate::cache::{cache_policy::CachePolicy, CacheAccess, RemoveError};
use parking_lot::Mutex;
use std::hash::Hash;

pub struct ArcCachePolicy<K> {
    inner: Mutex<ArcState<K>>,
}

struct ArcState<K> {
    t1: LinkedKeySet<K>,
    t2: LinkedKeySet<K>,
    b1: LinkedKeySet<K>,
    b2: LinkedKeySet<K>,
    p: usize,
    c: usize,
}

impl<K: Clone + Eq + Hash> ArcCachePolicy<K> {
    /// Creates a new ARC cache policy tracking a maximum of `c` items in the active cache.
    pub fn new(c: usize) -> Self {
        Self {
            inner: Mutex::new(ArcState {
                t1: LinkedKeySet::new(),
                t2: LinkedKeySet::new(),
                b1: LinkedKeySet::new(),
                b2: LinkedKeySet::new(),
                p: 0,
                c,
            }),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for ArcCachePolicy<K> {
    fn name(&self) -> &'static str {
        "ARC"
    }

    fn max_evict_failures(&self) -> usize {
        let state = self.inner.lock();
        state.t1.len() + state.t2.len()
    }

    fn on_access(&self, accessed_key: &K, _access: CacheAccess) {
        let mut state = self.inner.lock();
        // A cache hit moves the item to the MRU position of T2 (frequent list)
        if state.t1.remove(accessed_key) {
            state.t2.push_front(accessed_key.clone());
        } else if state.t2.contains(accessed_key) {
            state.t2.push_front(accessed_key.clone());
        }
    }

    fn on_add(&mut self, added_key: K) {
        let mut state = self.inner.lock();

        if state.b1.contains(&added_key) {
            // Hit in recent ghost list (B1): Adapt p upwards
            let b1_len = state.b1.len();
            let b2_len = state.b2.len();
            let delta = if b1_len >= b2_len { 1 } else { b2_len / b1_len };
            state.p = std::cmp::min(state.c, state.p + delta);

            state.b1.remove(&added_key);
            state.t2.push_front(added_key);
        } else if state.b2.contains(&added_key) {
            // Hit in frequent ghost list (B2): Adapt p downwards
            let b1_len = state.b1.len();
            let b2_len = state.b2.len();
            let delta = if b2_len >= b1_len { 1 } else { b1_len / b2_len };
            state.p = state.p.saturating_sub(delta);

            state.b2.remove(&added_key);
            state.t2.push_front(added_key);
        } else {
            // New element starts in recent list (T1)
            state.t1.push_front(added_key);
        }

        state.enforce_ghost_limits();
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        let mut state = self.inner.lock();
        // Items evicted from active lists transition to ghost lists
        if state.t1.remove(removed_key) {
            state.b1.push_front(removed_key.clone());
            state.enforce_ghost_limits();
            None
        } else if state.t2.remove(removed_key) {
            state.b2.push_front(removed_key.clone());
            state.enforce_ghost_limits();
            None
        } else {
            Some(RemoveError::NotPresent)
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        let mut state = self.inner.lock();
        if state.t1.remove(old_key) {
            state.t1.push_front(new_key);
        } else if state.t2.remove(old_key) {
            state.t2.push_front(new_key);
        } else if state.b1.remove(old_key) {
            state.b1.push_front(new_key);
        } else if state.b2.remove(old_key) {
            state.b2.push_front(new_key);
        }
    }

    fn pick_eviction_candidate(
        &mut self,
        f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(K, usize)> {
        let state = self.inner.lock();
        let t1_len = state.t1.len();

        let try_evict_from_t1 = t1_len > 0 && t1_len >= state.p;

        // Emulate REPLACE(p) from theoretical ARC by trying to evict from T1 or T2
        // We scan from Tail (LRU) to Head (MRU), bypassing pinned pages
        if try_evict_from_t1 {
            for key in state.t1.iter_lru() {
                if let Some(size) = f(key) {
                    return Some((key.clone(), size));
                }
            }
            // If everything in T1 was pinned, look in T2
            for key in state.t2.iter_lru() {
                if let Some(size) = f(key) {
                    return Some((key.clone(), size));
                }
            }
        } else {
            for key in state.t2.iter_lru() {
                if let Some(size) = f(key) {
                    return Some((key.clone(), size));
                }
            }
            // If everything in T2 was pinned, look in T1
            for key in state.t1.iter_lru() {
                if let Some(size) = f(key) {
                    return Some((key.clone(), size));
                }
            }
        }

        None
    }
}

impl<K: Clone + Eq + Hash> ArcState<K> {
    fn enforce_ghost_limits(&mut self) {
        while self.t1.len() + self.b1.len() > self.c && self.b1.len() > 0 {
            self.b1.pop_back();
        }
        while self.t1.len() + self.t2.len() + self.b1.len() + self.b2.len() > 2 * self.c
            && self.b2.len() > 0
        {
            self.b2.pop_back();
        }
    }
}
