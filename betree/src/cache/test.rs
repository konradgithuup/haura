#[cfg(test)]
mod cache_tests {
    use rstest::rstest;

    use crate::{
        cache::{
            cache_policy::CachePolicy, clock_policy::ClockCachePolicy, lfu_policy::LFUCachePolicy,
            lru_policy::LRUCachePolicy, random_policy::RandomCachePolicy, Cache, HashmapCache,
            WattPolicy,
        },
        size::SizeMut,
    };

    /// Insertions of new values should always work, even if capacity is exceeded.
    #[rstest]
    #[case(Box::new(RandomCachePolicy::new()))]
    #[case(Box::new(LFUCachePolicy::new()))]
    #[case(Box::new(LRUCachePolicy::new()))]
    #[case(Box::new(ClockCachePolicy::new()))]
    #[case(Box::new(WattPolicy::new(100)))]
    fn test_insert_exceeding_cap(#[case] policy: Box<dyn CachePolicy<u64>>) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal> = HashmapCache::new(policy, cap);

        assert!(cache.size() == 0);

        assert!(!cache.contains_key(&1));
        cache.insert(1, TestVal {}, 1);
        assert!(cache.size() == 1);
        assert!(cache.contains_key(&1));

        assert!(!cache.contains_key(&2));
        cache.insert(2, TestVal {}, 1);
        assert!(cache.size() == 2);
        assert!(cache.contains_key(&1));
        assert!(cache.contains_key(&2));

        assert!(!cache.contains_key(&3));
        cache.insert(3, TestVal {}, 1);
        assert!(cache.size() == 3);
        assert!(cache.contains_key(&1));
        assert!(cache.contains_key(&2));
        assert!(cache.contains_key(&3));
    }

    /// Insertions of existing values should panic
    #[rstest]
    #[should_panic]
    #[case(Box::new(RandomCachePolicy::new()))]
    #[should_panic]
    #[case(Box::new(LFUCachePolicy::new()))]
    #[should_panic]
    #[case(Box::new(LRUCachePolicy::new()))]
    #[should_panic]
    #[case(Box::new(ClockCachePolicy::new()))]
    #[should_panic]
    #[case(Box::new(WattPolicy::new(100)))]
    fn test_insert_duplicate(#[case] policy: Box<dyn CachePolicy<u64>>) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal> = HashmapCache::new(policy, cap);

        assert!(cache.size() == 0);

        assert!(!cache.contains_key(&1));
        cache.insert(1, TestVal {}, 1);
        assert!(cache.size() == 1);
        assert!(cache.contains_key(&1));

        // should panic here
        cache.insert(1, TestVal {}, 1);
    }

    #[rstest]
    #[case(Box::new(RandomCachePolicy::new()))]
    #[case(Box::new(LFUCachePolicy::new()))]
    #[case(Box::new(LRUCachePolicy::new()))]
    #[case(Box::new(ClockCachePolicy::new()))]
    #[case(Box::new(WattPolicy::new(100)))]
    fn test_remove(#[case] policy: Box<dyn CachePolicy<u64>>) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal> = HashmapCache::new(policy, cap);

        cache.insert(1, TestVal {}, 1);
        assert!(cache.size() == 1);

        assert!(cache.contains_key(&1));
        let res = cache.remove(&2, |_| 1);
        assert!(res.is_err());
        assert!(cache.size() == 1);

        assert!(cache.contains_key(&1));
        let res = cache.remove(&1, |_| 1);
        assert!(res.is_ok());
        assert!(cache.size() == 0);
    }

    #[rstest]
    #[case(Box::new(ClockCachePolicy::new()), 0)]
    #[case(Box::new(LFUCachePolicy::new()), 2)]
    #[case(Box::new(LRUCachePolicy::new()), 0)]
    #[case(Box::new(WattPolicy::new(100)), 0)]
    fn test_evict(#[case] policy: Box<dyn CachePolicy<u64>>, #[case] evicted_key: u64) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal> = HashmapCache::new(policy, cap);

        cache.insert(0, TestVal {}, 1);
        cache.insert(1, TestVal {}, 1);
        cache.insert(2, TestVal {}, 1);
        let ret = cache.evict(|_, _, _| Some(1));

        assert!(ret.is_some());
        assert_eq!(ret.unwrap().0, evicted_key);
        assert!(!cache.contains_key(&evicted_key));
    }

    /// The cache policy should provide new eviction candidates if possible.
    #[rstest]
    #[case(Box::new(RandomCachePolicy::new()))]
    #[case(Box::new(ClockCachePolicy::new()))]
    #[case(Box::new(LFUCachePolicy::new()))]
    #[case(Box::new(LRUCachePolicy::new()))]
    #[case(Box::new(WattPolicy::new(100)))]
    fn test_evict_skip(#[case] policy: Box<dyn CachePolicy<u64>>) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal> = HashmapCache::new(policy, cap);

        cache.insert(0, TestVal {}, 1);
        cache.insert(1, TestVal {}, 1);
        cache.insert(2, TestVal {}, 1);
        let ret = cache.evict(|key, _, _| match key.clone() == 2 {
            true => Some(1),
            false => None,
        });
        assert!(ret.is_some());
        assert_eq!(cache.size(), 2);

        assert!(cache.contains_key(&0));
        assert!(cache.contains_key(&1));
        assert!(!cache.contains_key(&2));
    }

    struct TestVal {}

    impl SizeMut for TestVal {
        fn size(&mut self) -> usize {
            1
        }

        fn cache_size(&mut self) -> usize {
            1
        }
    }
}
