#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_utils::TestDb;
    use expiring_bloom_rs::ExpiringBloomFilter;
    use expiring_bloom_rs::FilterConfigBuilder;
    use expiring_bloom_rs::{RedbFilter, RedbFilterConfigBuilder};
    use std::{thread, time::Duration};

    #[test]
    fn test_basic_workflow() {
        let test_db = TestDb::new("basic_workflow");

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        // Create the RedbFilterConfig
        let redb_config = RedbFilterConfigBuilder::default()
            .db_path(test_db.path())
            .filter_config(Some(config))
            .snapshot_interval(Duration::from_secs(60))
            .build()
            .expect("Failed to build RedbFilterConfig");

        let mut filter = RedbFilter::new(redb_config).unwrap();

        // Test insert and query
        filter.insert(b"test1").unwrap();
        assert!(filter.query(b"test1").unwrap());
        assert!(!filter.query(b"test2").unwrap());
    }

    #[test]
    fn test_persistence() {
        let test_db = TestDb::new("basic_persistence");

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        let redb_config = RedbFilterConfigBuilder::default()
            .db_path(test_db.path())
            .filter_config(Some(config.clone()))
            .snapshot_interval(Duration::from_secs(60))
            .build()
            .expect("Failed to build RedbFilterConfig");

        // Insert data with first instance
        {
            let mut filter = RedbFilter::new(redb_config.clone()).unwrap();
            filter.insert(b"persist_test").unwrap();
            assert!(filter.query(b"persist_test").unwrap());
        }

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify data with second instance
        {
            let filter = RedbFilter::new(redb_config).unwrap();
            assert!(filter.query(b"persist_test").unwrap());
        }
    }

    #[test]
    fn test_expiration() {
        let test_db = TestDb::new("basic_expiration");

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        let redb_config = RedbFilterConfigBuilder::default()
            .db_path(test_db.path())
            .filter_config(Some(config.clone()))
            .snapshot_interval(Duration::from_secs(60))
            .build()
            .expect("Failed to build RedbFilterConfig");

        let mut filter = RedbFilter::new(redb_config).unwrap();

        filter.insert(b"expire_test").unwrap();
        assert!(filter.query(b"expire_test").unwrap());

        // Wait for expiration
        thread::sleep(Duration::from_secs(4));
        filter.cleanup_expired_levels().unwrap();

        assert!(!filter.query(b"expire_test").unwrap());
    }
}
