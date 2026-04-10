//! This module provides the Write-Aware Timestamp Tracking (WATT) cache policy.
use crate::cache::cache_policy::CachePolicy;
use crate::cache::{CacheAccess, RemoveError};
use gxhash::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};

const ACCESS_HISTORY_SIZE: usize = 8;
const WRITE_HISTORY_SIZE: usize = 4;
const DEFAULT_SAMPLE_SIZE: usize = 8;
const DEFAULT_WRITE_WEIGHT: f32 = 4.0;
const RECENCY_DAMPENING: f32 = 0.1;
const EPOCH_DIVISOR: usize = 10;

const ACCESS_NUMERATORS: [f32; ACCESS_HISTORY_SIZE] = [0.1, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
const WRITE_NUMERATORS: [f32; WRITE_HISTORY_SIZE] = [1.0, 2.0, 3.0, 4.0];

#[derive(Debug)]
struct WattHistory {
    access_log: [AtomicU32; ACCESS_HISTORY_SIZE],
    write_log: [AtomicU32; WRITE_HISTORY_SIZE],
    ac_head: AtomicU8,
    wr_head: AtomicU8,
    ac_count: AtomicU8,
    wr_count: AtomicU8,
    key_index: usize,
}

impl Clone for WattHistory {
    fn clone(&self) -> Self {
        Self {
            access_log: std::array::from_fn(|i| {
                AtomicU32::new(self.access_log[i].load(Ordering::Relaxed))
            }),
            write_log: std::array::from_fn(|i| {
                AtomicU32::new(self.write_log[i].load(Ordering::Relaxed))
            }),
            ac_head: AtomicU8::new(self.ac_head.load(Ordering::Relaxed)),
            wr_head: AtomicU8::new(self.wr_head.load(Ordering::Relaxed)),
            ac_count: AtomicU8::new(self.ac_count.load(Ordering::Relaxed)),
            wr_count: AtomicU8::new(self.wr_count.load(Ordering::Relaxed)),
            key_index: self.key_index,
        }
    }
}

impl Default for WattHistory {
    fn default() -> Self {
        Self {
            access_log: std::array::from_fn(|_| AtomicU32::new(0)),
            write_log: std::array::from_fn(|_| AtomicU32::new(0)),
            ac_head: AtomicU8::new((ACCESS_HISTORY_SIZE - 1) as u8),
            wr_head: AtomicU8::new((WRITE_HISTORY_SIZE - 1) as u8),
            ac_count: AtomicU8::new(0),
            wr_count: AtomicU8::new(0),
            key_index: 0,
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
    /// Init WATT cache policy
    pub fn new(cache_capacity_blocks: usize) -> Self {
        Self {
            history: HashMap::default(),
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
        let ac_count = hist.ac_count.load(Ordering::Relaxed) as usize;
        let ac_head = hist.ac_head.load(Ordering::Relaxed) as usize;
        for i in 1..=ac_count {
            let ts = hist.access_log
                [(ac_head + ACCESS_HISTORY_SIZE - (i - 1)) % ACCESS_HISTORY_SIZE]
                .load(Ordering::Relaxed);
            let age = (self.t_now.saturating_sub(ts)).max(1) as f32;

            let sf = ACCESS_NUMERATORS[i - 1] / age;

            if sf > max_ac_sf {
                max_ac_sf = sf;
            }
        }

        let mut max_wr_sf = 0.0;
        let wr_count = hist.wr_count.load(Ordering::Relaxed) as usize;
        let wr_head = hist.wr_head.load(Ordering::Relaxed) as usize;
        for i in 1..=wr_count {
            let ts = hist.write_log[(wr_head + WRITE_HISTORY_SIZE - (i - 1)) % WRITE_HISTORY_SIZE]
                .load(Ordering::Relaxed);
            let age = (self.t_now.saturating_sub(ts)).max(1) as f32;

            let sf = WRITE_NUMERATORS[i - 1] / age;

            if sf > max_wr_sf {
                max_wr_sf = sf;
            }
        }

        max_ac_sf + (self.write_weight * max_wr_sf)
    }

    fn pick(
        &mut self,
        start_idx: usize,
        mut f: impl FnMut(&K) -> Option<usize>,
    ) -> Option<(usize, usize)> {
        let len = self.keys.len();
        let n = self.sample_size.min(len);

        let mut best_result: Option<(usize, usize)> = None;
        let mut min_pv = f32::MAX;

        for i in 0..n {
            let idx = (start_idx + i) % len;
            let key = &self.keys[idx];
            let hist = self.history.get(key).unwrap();
            let pv = self.calculate_pv(hist);

            if pv < min_pv {
                if let Some(size) = f(key) {
                    min_pv = pv;
                    best_result = Some((idx, size));
                }
            }
        }

        best_result
    }
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> CachePolicy<K> for WattPolicy<K> {
    fn name(&self) -> &'static str {
        "WATT"
    }

    fn max_evict_failures(&self) -> usize {
        self.keys.len()
    }

    fn on_access(&self, accessed_key: &K, access: CacheAccess) {
        if let Some(hist) = self.history.get(accessed_key) {
            let now = self.t_now;

            let old_ac_pos = hist.ac_head.load(Ordering::Relaxed);
            let ac_up_to_date = hist.access_log[old_ac_pos as usize].load(Ordering::Relaxed) == now;

            if access == CacheAccess::READ && ac_up_to_date {
                return;
            }

            if access == CacheAccess::WRITE {
                let old_wr_pos = hist.wr_head.load(Ordering::Relaxed);
                if hist.write_log[old_wr_pos as usize].load(Ordering::Relaxed) != now {
                    let pos = (old_wr_pos + 1) % (WRITE_HISTORY_SIZE as u8);
                    hist.write_log[pos as usize].store(now, Ordering::Release);
                    hist.wr_head.store(pos, Ordering::Release);

                    let count = hist.wr_count.load(Ordering::Relaxed);
                    if count < WRITE_HISTORY_SIZE as u8 {
                        hist.wr_count.store(count + 1, Ordering::Relaxed);
                    }
                }
            }

            if !ac_up_to_date {
                let pos = (old_ac_pos + 1) % (ACCESS_HISTORY_SIZE as u8);
                hist.access_log[pos as usize].store(now, Ordering::Release);
                hist.ac_head.store(pos, Ordering::Release);

                let count = hist.ac_count.load(Ordering::Relaxed);
                if count < ACCESS_HISTORY_SIZE as u8 {
                    hist.ac_count.store(count + 1, Ordering::Relaxed);
                }
            }
        }
    }

    fn on_add(&mut self, added_key: K) {
        let mut hist = WattHistory::default();
        hist.ac_head.store(0, Ordering::Relaxed);
        hist.access_log[0].store(self.t_now, Ordering::Relaxed);
        hist.ac_count.store(1, Ordering::Relaxed);
        hist.key_index = self.keys.len();
        self.history.insert(added_key.clone(), hist);
        self.keys.push(added_key);
    }

    fn on_remove(&mut self, removed_key: &K) -> Option<RemoveError> {
        if let Some(hist) = self.history.remove(removed_key) {
            let pos = hist.key_index;
            self.keys.swap_remove(pos);

            let current_len = self.keys.len();
            // If the removed element wasn't the last one, the element that was swapped into `pos`
            // needs its index updated.
            if pos < current_len {
                let swapped_key = &self.keys[pos];
                if let Some(swapped_hist) = self.history.get_mut(swapped_key) {
                    swapped_hist.key_index = pos;
                }
            }

            if current_len > 0 {
                if self.cursor.load(Ordering::Relaxed) >= current_len {
                    self.cursor.store(0, Ordering::Relaxed);
                }
            } else {
                self.cursor.store(0, Ordering::Relaxed);
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
        if let Some(mut hist) = self.history.remove(old_key) {
            let pos = hist.key_index;
            self.keys[pos] = new_key.clone();
            self.history.insert(new_key, hist);
        }
    }

    fn pick_eviction_candidate(
        &mut self,
        mut f: &mut dyn FnMut(&K) -> Option<usize>,
    ) -> Option<(K, usize)> {
        let len = self.keys.len();
        if len == 0 {
            return None;
        }

        let n = self.sample_size.min(len);
        for _ in 0..self.max_evict_failures() {
            // Commit to the range immediately to prevent overlapping samples between threads.
            // fetch_add returns the PREVIOUS value.
            let start_idx = self.cursor.fetch_add(n, Ordering::Relaxed) % len;

            if let Some((idx, size)) = self.pick(start_idx, &mut f) {
                return Some((self.keys[idx].clone(), size));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watt_frequency_preference() {
        let mut policy = WattPolicy::new(100);
        policy.on_add(1);
        policy.on_add(2);

        for i in 0..5 {
            policy.on_access(&1, crate::cache::CacheAccess::READ);
            // Trigger epoch increments by simulating removals
            for j in 0..10 {
                let dummy = 1000 + i * 100 + j;
                policy.on_add(dummy);
                policy.on_remove(&dummy);
            }
        }

        // Key 2 should be the eviction candidate (lowest frequency)
        assert_eq!(
            policy.pick_eviction_candidate(&mut |_| Some(1)),
            Some((2, 1))
        );
    }

    #[test]
    fn test_watt_write_preference() {
        let mut policy = WattPolicy::new(100);
        policy.on_add(1);
        policy.on_add(2);

        // 3 Reads
        for _ in 0..3 {
            policy.on_access(&1, CacheAccess::READ);
        }

        // 1 Write
        policy.on_access(&2, CacheAccess::WRITE);

        // Key 1 should be evicted even though it has more total accesses, because the write weight
        // for Key 2 is much higher.
        // Depends on `DEFAULT_WRITE_WEIGHT` that this works
        assert_eq!(
            policy.pick_eviction_candidate(&mut |_| Some(1)),
            Some((1, 1))
        );
    }

    #[test]
    fn test_watt_sampling_cursor() {
        let mut policy = WattPolicy::new(100);
        for i in 0..20 {
            policy.on_add(i);
        }

        let first_candidate = policy.pick_eviction_candidate(&mut |_| Some(1)).unwrap().0;
        assert!(first_candidate < DEFAULT_SAMPLE_SIZE);

        let second_candidate = policy.pick_eviction_candidate(&mut |_| Some(1)).unwrap().0;
        assert!(
            second_candidate >= DEFAULT_SAMPLE_SIZE,
            "{} <= {} : FALSE",
            second_candidate,
            DEFAULT_SAMPLE_SIZE
        );
        assert!(second_candidate < 2 * DEFAULT_SAMPLE_SIZE);
    }
}
