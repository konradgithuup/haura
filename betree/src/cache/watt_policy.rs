//! This module provides the Write-Aware Timestamp Tracking (WATT) cache policy.
use std::collections::HashMap;
use std::hash::Hash;
use std::cell::Cell;
use crate::cache::cache_policy::{CacheIterator, CachePolicy};
use crate::cache::RemoveError;
use crate::StoragePreference;

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
}

impl Default for WattHistory {
    fn default() -> Self {
        Self {
            access_log: [0; ACCESS_HISTORY_SIZE],
            write_log: [0; WRITE_HISTORY_SIZE],
            ac_head: (ACCESS_HISTORY_SIZE - 1) as u8,
            wr_head: (WRITE_HISTORY_SIZE - 1) as u8,
        }
    }
}

/// WATT cache policy implementation.
pub struct WattPolicy<K> {
    history: HashMap<K, WattHistory>,
    keys: Vec<K>,
    // Using Cell for internal mutability since pick_eviction_candidate is &self
    cursor: Cell<usize>,
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
            cursor: Cell::new(0),
            t_now: 1,
            eviction_count: 0,
            epoch_threshold: (cache_capacity_blocks / EPOCH_DIVISOR).max(1) as u32,
            sample_size: DEFAULT_SAMPLE_SIZE,
            write_weight: DEFAULT_WRITE_WEIGHT,
        }
    }

    fn calculate_pv(&self, hist: &WattHistory) -> f32 {
        let mut max_ac_sf = 0.0;
        for i in 1..=ACCESS_HISTORY_SIZE {
            let ts = hist.access_log[(hist.ac_head as usize + ACCESS_HISTORY_SIZE - (i - 1)) % ACCESS_HISTORY_SIZE];
            let age = (self.t_now.saturating_sub(ts)).max(1);
            let mut sf = (i as f32) / (age as f32);
            if i == 1 { sf *= RECENCY_DAMPENING; }
            if sf > max_ac_sf { max_ac_sf = sf; }
        }

        let mut max_wr_sf = 0.0;
        for i in 1..=WRITE_HISTORY_SIZE {
            let ts = hist.write_log[(hist.wr_head as usize + WRITE_HISTORY_SIZE - (i - 1)) % WRITE_HISTORY_SIZE];
            let age = (self.t_now.saturating_sub(ts)).max(1);
            let sf = (i as f32) / (age as f32);
            if sf > max_wr_sf { max_wr_sf = sf; }
        }

        max_ac_sf + (self.write_weight * max_wr_sf)
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for WattPolicy<K> {
    fn name(&self) -> &'static str { "WATT" }

    fn max_evict_failures(&self) -> usize { self.keys.len() }

    fn on_access(&mut self, accessed_key: &K, is_write: bool) {
        if let Some(hist) = self.history.get_mut(accessed_key) {
            if is_write {
                hist.wr_head = (hist.wr_head + 1) % WRITE_HISTORY_SIZE as u8;
                hist.write_log[hist.wr_head as usize] = self.t_now;
            }
            // Every write is also an access
            hist.ac_head = (hist.ac_head + 1) % ACCESS_HISTORY_SIZE as u8;
            hist.access_log[hist.ac_head as usize] = self.t_now;
        }
    }

    fn on_add(&mut self, added_key: K) {
        let mut hist = WattHistory::default();
        hist.ac_head = 0;
        hist.access_log[0] = self.t_now;
        self.history.insert(added_key.clone(), hist);
        self.keys.push(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        if self.history.remove(removed_key).is_some() {
            if let Some(pos) = self.keys.iter().position(|k| k == removed_key) {
                self.keys.swap_remove(pos);
                // Adjust cursor if swap_remove affected it
                if self.cursor.get() >= self.keys.len() && !self.keys.is_empty() {
                    self.cursor.set(0);
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
        if self.keys.is_empty() { return None; }

        let n = self.sample_size.min(self.keys.len());
        let mut best_key_idx = self.cursor.get() % self.keys.len();
        let mut min_pv = f32::MAX;

        for i in 0..n {
            let idx = (self.cursor.get() + i) % self.keys.len();
            let key = &self.keys[idx];
            let hist = self.history.get(key).unwrap();
            let pv = self.calculate_pv(hist);

            if pv < min_pv {
                min_pv = pv;
                best_key_idx = idx;
            }
        }

        // Advance cursor for next round-robin batch
        self.cursor.set((self.cursor.get() + n) % self.keys.len());

        Some(&self.keys[best_key_idx])
    }

    fn iter<'a>(&'a self) -> impl CacheIterator<'a, K> where K: 'a {
        self.keys.iter()
    }
}
