mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_utils::{TestDb, setup_test_redb};
    use expiring_bloom_rs::ExpiringBloomFilter;
    use std::{thread, time::Duration};

    #[test]
    fn test_basic_workflow() {
        let test_db = TestDb::new("basic_workflow");
        let mut filter = setup_test_redb(
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
        let test_db = TestDb::new("basic_persistence");

        // Insert data with first instance
        {
            let mut filter = setup_test_redb(
                &test_db.path_string(),
                1000,
                Duration::from_secs(1),
                3,
                Duration::from_secs(60),
            );

            filter.insert(b"persist_test").unwrap();
            assert!(filter.query(b"persist_test").unwrap());
        }

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify data with second instance
        {
            let filter = setup_test_redb(
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
        let test_db = TestDb::new("basic_expiration");
        let mut filter = setup_test_redb(
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
