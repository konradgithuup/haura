//! This module provides an LRU cache implementation.
//!
//! LRU is implemented on a doubly linked list.

use std::{
    collections::LinkedList,
    hash::Hash,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

use gxhash::HashMap;
use libc::remove;

use crate::{
    cache::{
        cache_util::{CacheEntry, CacheStats, PinnedEntry},
        Cache, RemoveError,
    },
    size::SizeMut,
};

pub struct LRUCache<K, V> {
    map: HashMap<K, Arc<CacheEntry<V>>>,
    lru_list: LinkedList<K>,
    capacity: usize,
    size: &'static AtomicUsize,
    hits: AtomicU64,
    misses: AtomicU64,
    insertions: u64,
    evictions: u64,
    removals: u64,
}

impl<K: Hash + Eq, V: SizeMut> LRUCache<K, V> {
    /// Returns new cache instance with the given `capacity`.
    pub fn new(capacity: usize) -> Self {
        LRUCache {
            map: Default::default(),
            lru_list: Default::default(),
            size: Box::leak(Default::default()),
            hits: Default::default(),
            misses: Default::default(),
            capacity,
            insertions: 0,
            evictions: 0,
            removals: 0,
        }
    }
}

impl<K: Clone + Eq + Hash + Sync + Send + 'static, V: Sync + Send + SizeMut + 'static> Cache
    for LRUCache<K, V>
{
    type Key = K;
    type Value = V;
    type ValueRef = PinnedEntry<V>;
    type Stats = CacheStats;

    fn new(capacity: usize) -> Self {
        Self::new(capacity)
    }

    fn contains_key(&self, key: &Self::Key) -> bool {
        self.map.contains_key(key)
    }

    fn get(&self, key: &K, count_miss: bool) -> Option<Self::ValueRef> {
        if let Some(entry) = self.map.get(key).cloned() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            entry.referenced.store(true, Ordering::Relaxed);
            Some(PinnedEntry {
                size: self.size,
                entry,
            })
        } else {
            if count_miss {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
            None
        }
    }

    fn remove<F>(&mut self, key: &Self::Key, f: F) -> Result<Self::Value, super::RemoveError>
    where
        F: FnOnce(&mut Self::Value) -> usize,
    {
        self.verify();
        {
            let entry = self.map.get_mut(key).ok_or(RemoveError::NotPresent)?;
            Arc::get_mut(entry).ok_or(RemoveError::Pinned)?;
        }
        let removed = self.lru_list.extract_if(|entry| entry == key).count() > 0;
        if !removed {
            println!("The stored value is unexpectedly not listed in LRU!");
            return Result::Err(RemoveError::NotPresent);
        }

        let entry = self.map.remove(key).unwrap();
        let mut value = Arc::try_unwrap(entry).ok().unwrap().value;

        let size = f(&mut value);
        self.removals += 1;
        self.size.fetch_sub(size, Ordering::Relaxed);
        self.verify();

        Ok(value)
    }

    fn force_remove(&mut self, key: &Self::Key, size: usize) -> bool {
        todo!()
    }

    fn change_key<E, F>(&mut self, key: &Self::Key, f: F) -> Result<(), super::ChangeKeyError<E>>
    where
        F: FnOnce(
            &Self::Key,
            &mut Self::Value,
            &dyn Fn(&Self::Key) -> bool,
        ) -> Result<Self::Key, E>,
    {
        todo!()
    }

    fn force_change_key(&mut self, key: &Self::Key, new_key: Self::Key) -> bool {
        todo!()
    }

    fn evict<F>(&mut self, f: F) -> Option<(Self::Key, Self::Value)>
    where
        F: FnMut(&Self::Key, &mut Self::Value, &dyn Fn(&Self::Key) -> bool) -> Option<usize>,
    {
        todo!()
    }

    fn insert(&mut self, key: Self::Key, value: Self::Value, size: usize) {
        todo!()
    }

    fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a Self::Key> + 'a> {
        todo!()
    }

    fn size(&self) -> usize {
        todo!()
    }

    fn capacity(&self) -> usize {
        todo!()
    }

    fn stats(&self) -> Self::Stats {
        todo!()
    }

    fn verify(&mut self) {
        todo!()
    }
}

pub struct LRUIter<'a, T: 'a> {}

impl<'a, T> Iterator for LRUIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {}
}
