use std::{
    fmt,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use crate::{cache::AddSize, cache::StableDeref, cache::Stats, size::SizeMut};

pub struct CacheEntry<V> {
    pub value: V,
    pub referenced: AtomicBool,
}

/// Pinned cache entry
pub struct PinnedEntry<V: 'static> {
    pub size: &'static AtomicUsize,
    pub entry: Arc<CacheEntry<V>>,
}

impl<V> Deref for PinnedEntry<V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.entry.value
    }
}

unsafe impl<V> StableDeref for PinnedEntry<V> {}

impl<V: SizeMut> AddSize for PinnedEntry<V> {
    fn add_size(&self, size_delta: isize) {
        if size_delta >= 0 {
            self.size.fetch_add(size_delta as usize, Ordering::Relaxed);
        } else {
            self.size.fetch_sub(-size_delta as usize, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CacheStats {
    pub cache_name: &'static str,
    pub capacity: usize,
    pub size: usize,
    pub len: usize,
    pub hits: u64,
    pub misses: u64,
    pub insertions: u64,
    pub evictions: u64,
    pub removals: u64,
}

impl fmt::Display for CacheStats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let total = self.hits + self.misses;
        write!(
            f,
            r"
STATISTICS:
===
              Size: {s}/{c} ({s_p:.2}% filled)
          {n} size: {c_s:>6}
Average entry size: {avg_e:.2}

  Hits: {h:>8} ({h_p:>6.2}%)
Misses: {m:>8} ({m_p:>6.2}%)

Insertions: {i:>8}
 Evictions: {e:>8}
  Removals: {r:>8}",
            s = self.size,
            c = self.capacity,
            s_p = 100.0 * self.size as f32 / self.capacity as f32,
            n = self.cache_name,
            c_s = self.len,
            avg_e = self.size as f32 / self.len as f32,
            h = self.hits,
            h_p = 100.0 * self.hits as f32 / total as f32,
            m = self.misses,
            m_p = 100.0 * self.misses as f32 / total as f32,
            i = self.insertions,
            e = self.evictions,
            r = self.removals
        )
    }
}

impl Stats for CacheStats {
    fn capacity(&self) -> usize {
        self.capacity
    }

    fn size(&self) -> usize {
        self.size
    }

    fn len(&self) -> usize {
        self.len
    }

    fn hits(&self) -> u64 {
        self.hits
    }

    fn misses(&self) -> u64 {
        self.misses
    }

    fn insertions(&self) -> u64 {
        self.insertions
    }

    fn evictions(&self) -> u64 {
        self.evictions
    }

    fn removals(&self) -> u64 {
        self.removals
    }
}
