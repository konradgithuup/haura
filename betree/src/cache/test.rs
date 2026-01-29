#[cfg(test)]
mod cache_tests {
    use rstest::rstest;

    use crate::{
        cache::{
            cache_policy::CachePolicy, clock_policy::ClockCachePolicy, lru_policy::LRUCachePolicy,
            Cache, HashmapCache,
        },
        size::SizeMut,
    };

    /// Insertions of new values should always work, even if capacity is exceeded.
    #[rstest]
    #[case(LRUCachePolicy::new())]
    #[case(ClockCachePolicy::new())]
    fn test_insert_exceeding_cap<P: CachePolicy<u64>>(#[case] policy: P) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal, P> = HashmapCache::new(Box::new(policy), cap);

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
    #[case(LRUCachePolicy::new())]
    #[should_panic]
    #[case(ClockCachePolicy::new())]
    fn test_insert_duplicate<P: CachePolicy<u64>>(#[case] policy: P) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal, P> = HashmapCache::new(Box::new(policy), cap);

        assert!(cache.size() == 0);

        assert!(!cache.contains_key(&1));
        cache.insert(1, TestVal {}, 1);
        assert!(cache.size() == 1);
        assert!(cache.contains_key(&1));

        // should panic here
        cache.insert(1, TestVal {}, 1);
    }

    #[rstest]
    #[case(LRUCachePolicy::new())]
    #[case(ClockCachePolicy::new())]
    fn test_remove<P: CachePolicy<u64>>(#[case] policy: P) {
        let cap = 2;
        let mut cache: HashmapCache<u64, TestVal, P> = HashmapCache::new(Box::new(policy), cap);

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
