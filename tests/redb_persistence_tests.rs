#[cfg(test)]
mod tests {
    use expiring_bloom_rs::FilterConfigBuilder;
    use expiring_bloom_rs::RedbSlidingBloomFilter;
    use expiring_bloom_rs::SlidingBloomFilter;
    use std::{fs, path::PathBuf, thread, time::Duration};

    fn temp_db_path() -> PathBuf {
        use rand::random;
        format!("test_db_{}.redb", random::<u64>()).into()
    }

    fn cleanup_db(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_basic_workflow() {
        let path = temp_db_path();

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        let mut filter =
            RedbSlidingBloomFilter::new(config, path.clone()).unwrap();

        // Test insert and query
        filter.insert(b"test1").unwrap();
        assert!(filter.query(b"test1").unwrap());
        assert!(!filter.query(b"test2").unwrap());

        // Clean up
        cleanup_db(&path);
    }

    #[test]
    fn test_persistence() {
        let path = temp_db_path();
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        // Insert data with first instance
        {
            let mut filter =
                RedbSlidingBloomFilter::new(config.clone(), path.clone())
                    .unwrap();
            filter.insert(b"persist_test").unwrap();
            assert!(filter.query(b"persist_test").unwrap());
        }

        // Verify data with second instance
        {
            let filter =
                RedbSlidingBloomFilter::new(config, path.clone()).unwrap();
            assert!(filter.query(b"persist_test").unwrap());
        }

        cleanup_db(&path);
    }

    #[test]
    fn test_expiration() {
        let path = temp_db_path();
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        let mut filter =
            RedbSlidingBloomFilter::new(config, path.clone()).unwrap();

        filter.insert(b"expire_test").unwrap();
        assert!(filter.query(b"expire_test").unwrap());

        // Wait for expiration
        thread::sleep(Duration::from_secs(4));
        filter.cleanup_expired_levels().unwrap();

        assert!(!filter.query(b"expire_test").unwrap());

        cleanup_db(&path);
    }
}
