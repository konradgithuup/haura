//! This module provides the Write-Aware Timestamp Tracking (WATT) cache policy.
use crate::cache::cache_policy::{CacheIterator, CachePolicy};
use crate::cache::RemoveError;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};

const ACCESS_HISTORY_SIZE: usize = 8;
const WRITE_HISTORY_SIZE: usize = 4;
const DEFAULT_SAMPLE_SIZE: usize = 8;
const DEFAULT_WRITE_WEIGHT: f32 = 4.0;
const RECENCY_DAMPENING: f32 = 0.1;
const EPOCH_DIVISOR: usize = 10;

#[derive(Debug, Clone)]
struct WattHistory {
    access_log: [u32; ACCESS_HISTORY_SIZE],
    write_log: [u32; WRITE_HISTORY_SIZE],
    ac_head: u8,
    wr_head: u8,
    ac_count: u8,
    wr_count: u8,
}

impl Default for WattHistory {
    fn default() -> Self {
        Self {
            access_log: [0; ACCESS_HISTORY_SIZE],
            write_log: [0; WRITE_HISTORY_SIZE],
            ac_head: (ACCESS_HISTORY_SIZE - 1) as u8,
            wr_head: (WRITE_HISTORY_SIZE - 1) as u8,
            ac_count: 0,
            wr_count: 0,
        }
    }
}

/// WATT cache policy implementation.
pub struct WattPolicy<K> {
    history: HashMap<K, WattHistory>,
    keys: Vec<K>,
    cursor: AtomicUsize,
    t_now: u32,
    eviction_count: u32,
    epoch_threshold: u32,
    sample_size: usize,
    write_weight: f32,
}

impl<K: Eq + Hash + Clone> WattPolicy<K> {
    pub fn new(cache_capacity_blocks: usize) -> Self {
        Self {
            history: HashMap::new(),
            keys: Vec::new(),
            cursor: AtomicUsize::new(0),
            t_now: 1,
            eviction_count: 0,
            epoch_threshold: (cache_capacity_blocks / EPOCH_DIVISOR).max(1) as u32,
            sample_size: DEFAULT_SAMPLE_SIZE,
            write_weight: DEFAULT_WRITE_WEIGHT,
        }
    }

    fn calculate_pv(&self, hist: &WattHistory) -> f32 {
        let mut max_ac_sf = 0.0;
        for i in 1..=(hist.ac_count as usize) {
            let ts = hist.access_log
                [(hist.ac_head as usize + ACCESS_HISTORY_SIZE - (i - 1)) % ACCESS_HISTORY_SIZE];
            let age = (self.t_now.saturating_sub(ts)).max(1);
            let mut sf = (i as f32) / (age as f32);
            if i == 1 {
                sf *= RECENCY_DAMPENING;
            }
            if sf > max_ac_sf {
                max_ac_sf = sf;
            }
        }

        let mut max_wr_sf = 0.0;
        for i in 1..=(hist.wr_count as usize) {
            let ts = hist.write_log
                [(hist.wr_head as usize + WRITE_HISTORY_SIZE - (i - 1)) % WRITE_HISTORY_SIZE];
            let age = (self.t_now.saturating_sub(ts)).max(1);
            let sf = (i as f32) / (age as f32);
            if sf > max_wr_sf {
                max_wr_sf = sf;
            }
        }

        max_ac_sf + (self.write_weight * max_wr_sf)
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for WattPolicy<K> {
    fn name(&self) -> &'static str {
        "WATT"
    }

    fn max_evict_failures(&self) -> usize {
        self.keys.len()
    }

    fn on_access(&mut self, accessed_key: &K, is_write: bool) {
        if let Some(hist) = self.history.get_mut(accessed_key) {
            if is_write {
                hist.wr_head = (hist.wr_head + 1) % WRITE_HISTORY_SIZE as u8;
                hist.write_log[hist.wr_head as usize] = self.t_now;
                hist.wr_count = (hist.wr_count + 1).min(WRITE_HISTORY_SIZE as u8);
            }
            hist.ac_head = (hist.ac_head + 1) % ACCESS_HISTORY_SIZE as u8;
            hist.access_log[hist.ac_head as usize] = self.t_now;
            hist.ac_count = (hist.ac_count + 1).min(ACCESS_HISTORY_SIZE as u8);
        }
    }

    fn on_add(&mut self, added_key: K) {
        let mut hist = WattHistory::default();
        hist.ac_head = 0;
        hist.access_log[0] = self.t_now;
        hist.ac_count = 1;
        self.history.insert(added_key.clone(), hist);
        self.keys.push(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        if self.history.remove(removed_key).is_some() {
            if let Some(pos) = self.keys.iter().position(|k| k == removed_key) {
                self.keys.swap_remove(pos);
                // Adjust cursor if swap_remove affected it
                let current_len = self.keys.len();
                if current_len > 0 {
                    if self.cursor.load(Ordering::Relaxed) >= current_len {
                        self.cursor.store(0, Ordering::Relaxed);
                    }
                } else {
                    self.cursor.store(0, Ordering::Relaxed);
                }
            }
            self.eviction_count += 1;
            if self.eviction_count >= self.epoch_threshold {
                self.t_now += 1;
                self.eviction_count = 0;
            }
            None
        } else {
            Some(RemoveError::NotPresent)
        }
    }

    fn update(&mut self, old_key: &K, new_key: K) {
        if let Some(hist) = self.history.remove(old_key) {
            self.history.insert(new_key.clone(), hist);
            if let Some(pos) = self.keys.iter().position(|k| k == old_key) {
                self.keys[pos] = new_key;
            }
        }
    }

    fn pick_eviction_candidate(&self) -> Option<&K> {
        let len = self.keys.len();
        if len == 0 {
            return None;
        }

        let n = self.sample_size.min(len);

        // Commit to the range immediately to prevent overlapping samples between threads.
        // fetch_add returns the PREVIOUS value.
        let start_idx = self.cursor.fetch_add(n, Ordering::Relaxed) % len;

        let mut best_key_idx = start_idx;
        let mut min_pv = f32::MAX;

        for i in 0..n {
            let idx = (start_idx + i) % len;
            let key = &self.keys[idx];
            let hist = self.history.get(key).unwrap();
            let pv = self.calculate_pv(hist);

            if pv < min_pv {
                min_pv = pv;
                best_key_idx = idx;
            }
        }

        Some(&self.keys[best_key_idx])
    }

    fn iter<'a>(&'a self) -> impl CacheIterator<'a, K>
    where
        K: 'a,
    {
        WattIter {
            inner: self.keys.iter(),
        }
    }
}

struct WattIter<'a, K> {
    inner: std::slice::Iter<'a, K>,
}

impl<'a, K: 'a> Iterator for WattIter<'a, K> {
    type Item = &'a K;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

impl<'a, K: 'a> CacheIterator<'a, K> for WattIter<'a, K> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watt_frequency_preference() {
        let mut policy = WattPolicy::new(100);
        policy.on_add(1);
        policy.on_add(2);

        for _ in 0..5 {
            policy.on_access(&1, false);
        }

        // Key 2 should be the eviction candidate (lowest frequency)
        assert_eq!(policy.pick_eviction_candidate(), Some(&2));
    }

    #[test]
    fn test_watt_write_preference() {
        let mut policy = WattPolicy::new(100);
        policy.on_add(1);
        policy.on_add(2);

        // 3 Reads
        for _ in 0..3 {
            policy.on_access(&1, false);
        }

        // 1 Write
        policy.on_access(&2, true);

        // Key 1 should be evicted even though it has more total accesses, because the write weight
        // for Key 2 is much higher.
        // Depends on `DEFAULT_WRITE_WEIGHT` that this works
        assert_eq!(policy.pick_eviction_candidate(), Some(&1));
    }

    #[test]
    fn test_watt_sampling_cursor() {
        let mut policy = WattPolicy::new(100);
        for i in 0..20 {
            policy.on_add(i);
        }

        let first_candidate = *policy.pick_eviction_candidate().unwrap();
        assert!(first_candidate < DEFAULT_SAMPLE_SIZE);

        let second_candidate = *policy.pick_eviction_candidate().unwrap();
        assert!(second_candidate >= DEFAULT_SAMPLE_SIZE);
        assert!(second_candidate < 2 * DEFAULT_SAMPLE_SIZE);
    }
}
