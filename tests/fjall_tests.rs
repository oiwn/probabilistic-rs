#[cfg(feature = "fjall")]
mod tests {
    use expiring_bloom_rs::{
        ExpiringBloomFilter, FilterConfigBuilder, FjallFilter,
        FjallFilterConfigBuilder,
    };
    use std::{fs, thread, time::Duration};

    struct TestDb {
        path: std::path::PathBuf,
    }

    impl TestDb {
        fn new(test_name: &str) -> Self {
            let path = format!("test_db_{}.fjall", test_name).into();
            Self { path }
        }

        fn path_string(&self) -> String {
            self.path.to_string_lossy().to_string()
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            if self.path.exists() {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }

    fn setup_test_fjall(
        db_path: &str,
        capacity: usize,
        level_duration: Duration,
        max_levels: usize,
        snapshot_interval: Duration,
    ) -> FjallFilter {
        let filter_config = FilterConfigBuilder::default()
            .capacity(capacity)
            .false_positive_rate(0.01)
            .level_duration(level_duration)
            .max_levels(max_levels)
            .build()
            .unwrap();

        let fjall_config = FjallFilterConfigBuilder::default()
            .db_path(std::path::PathBuf::from(db_path))
            .filter_config(Some(filter_config))
            .snapshot_interval(snapshot_interval)
            .build()
            .unwrap();

        FjallFilter::new(fjall_config).expect("Unable to create filter...")
    }

    #[test]
    fn test_basic_workflow() {
        let test_db = TestDb::new("fjall_basic_workflow");
        let mut filter = setup_test_fjall(
            &test_db.path_string(),
            1000,
            Duration::from_secs(1),
            3,
            Duration::from_secs(60),
        );

        // Test insert and query
        filter.insert(b"test1").unwrap();
        assert!(filter.query(b"test1").unwrap());
        assert!(!filter.query(b"test2").unwrap());
    }

    #[test]
    fn test_persistence() {
        let test_db = TestDb::new("fjall_persistence");

        // Insert data with first instance
        {
            let mut filter = setup_test_fjall(
                &test_db.path_string(),
                1000,
                Duration::from_secs(1),
                3,
                Duration::from_secs(60),
            );

            filter.insert(b"persist_test").unwrap();
            assert!(filter.query(b"persist_test").unwrap());
            // Force snapshot to ensure data is written
            filter.save_snapshot().unwrap();
        }

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify data with second instance
        {
            let filter = setup_test_fjall(
                &test_db.path_string(),
                1000,
                Duration::from_secs(1),
                3,
                Duration::from_secs(60),
            );
            assert!(filter.query(b"persist_test").unwrap());
        }
    }

    #[test]
    fn test_expiration() {
        let test_db = TestDb::new("fjall_expiration");
        let mut filter = setup_test_fjall(
            &test_db.path_string(),
            1000,
            Duration::from_millis(300),
            3,
            Duration::from_millis(1000),
        );

        filter.insert(b"expire_test").unwrap();
        assert!(filter.query(b"expire_test").unwrap());

        // Wait for expiration
        thread::sleep(Duration::from_secs(1));
        filter.cleanup_expired_levels().unwrap();

        assert!(!filter.query(b"expire_test").unwrap());
    }
}
