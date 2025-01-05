/* #[cfg(test)]
mod tests {
    use expiring_bloom_rs::inmemory_storage::InMemoryStorage;
    use expiring_bloom_rs::redb_storage::RedbExpiringBloomFilter;
    use expiring_bloom_rs::redb_storage::RedbExpiringloomFilterConfigBuilder;
    use rand::random;
    use redb::Database;
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{Duration, SystemTime},
    };

    /* struct RedbGuard {
        path: PathBuf,
    } */

    /* impl Drop for RedbGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path); // Cleanup on drop, ignoring errors
        }
    } */

    fn temp_db_path() -> PathBuf {
        format!("test_db_{}.redb", random::<u64>()).into()
    }

    fn cleanup_db(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_storage_persistence_and_recovery() {
        let path = temp_db_path();

        // Create initial configuration
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // Create database
        let db = Database::create(&path).expect("Failed to create database");
        let db = Arc::new(db);

        // Initialize storage with some data
        {
            let mut storage = InMemoryStorage::new(1000, 3).unwrap();

            // Set some bits in level 0
            storage.levels[0][5] = true;
            storage.levels[0][10] = true;

            // Set timestamp for level 0
            let test_time = SystemTime::now();
            storage.timestamps[0] = test_time;

            // Write snapshot
            RedbExpiringBloomFilter::write_snapshot(&db, &storage)
                .expect("Failed to write snapshot");
        }

        // Load storage and verify state
        let loaded_storage =
            RedbExpiringBloomFilter::load_or_create_storage(&db, &config)
                .expect("Failed to load storage");

        // Verify bits were persisted correctly
        assert!(loaded_storage.levels[0][5], "Bit at index 5 should be set");
        assert!(
            loaded_storage.levels[0][10],
            "Bit at index 10 should be set"
        );
        assert!(
            !loaded_storage.levels[0][7],
            "Bit at index 7 should not be set"
        );

        // Verify timestamp was persisted
        let timestamp_diff = loaded_storage.timestamps[0]
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let current_diff = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Timestamp should be within 1 second of current time
        assert!(
            current_diff - timestamp_diff <= 1,
            "Timestamp was not persisted correctly"
        );

        // Test unset levels remain initialized but empty
        for i in 0..1000 {
            if i != 5 && i != 10 {
                assert!(
                    !loaded_storage.levels[0][i],
                    "Bit {} should not be set",
                    i
                );
            }
        }

        // Verify other levels are empty
        for level in 1..3 {
            assert!(
                loaded_storage.levels[level].iter().all(|&bit| !bit),
                "Level {} should be empty",
                level
            );
        }

        cleanup_db(&path);
    }

    #[test]
    fn test_multiple_snapshots() {
        let path = temp_db_path();

        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        let db = Database::create(&path).expect("Failed to create database");
        let db = Arc::new(db);

        // Create and write first snapshot
        {
            let mut storage = InMemoryStorage::new(1000, 3).unwrap();
            storage.levels[0][5] = true;
            RedbExpiringBloomFilter::write_snapshot(&db, &storage)
                .expect("Failed to write first snapshot");
        }

        // Create and write second snapshot with different data
        {
            let mut storage = InMemoryStorage::new(1000, 3).unwrap();
            storage.levels[0][5] = true;
            storage.levels[0][15] = true;
            RedbExpiringBloomFilter::write_snapshot(&db, &storage)
                .expect("Failed to write second snapshot");
        }

        // Load and verify final state
        let loaded_storage =
            RedbExpiringBloomFilter::load_or_create_storage(&db, &config)
                .expect("Failed to load storage");

        assert!(loaded_storage.levels[0][5], "Bit 5 should still be set");
        assert!(
            loaded_storage.levels[0][15],
            "Bit 15 should be set from second snapshot"
        );

        cleanup_db(&path);
    }

    #[test]
    fn test_storage_initialization() {
        let path = temp_db_path();
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // Create new database without any snapshots
        let db = Database::create(&path).expect("Failed to create database");
        let db = Arc::new(db);

        // Load storage - should initialize with empty state
        let storage =
            RedbExpiringBloomFilter::load_or_create_storage(&db, &config)
                .expect("Failed to load storage");

        // Verify all levels are empty
        for level in 0..3 {
            assert!(
                storage.levels[level].iter().all(|&bit| !bit),
                "Level {} should be empty on initialization",
                level
            );
        }

        // Verify timestamps are initialized
        for level in 0..3 {
            let timestamp = storage.timestamps[level];
            assert!(
                timestamp <= SystemTime::now()
                    && timestamp >= SystemTime::now() - Duration::from_secs(1),
                "Timestamp for level {} should be initialized to current time",
                level
            );
        }

        cleanup_db(&path);
    }

    #[test]
    fn test_complete_workflow() {
        let path = temp_db_path();

        // Create configuration
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // 1. First instance: Insert items and verify presence
        {
            let mut filter = RedbExpiringBloomFilter::new(config)
                .expect("Failed to create filter");

            // Insert test items
            filter.insert(b"item1").expect("Failed to insert item1");
            filter.insert(b"item2").expect("Failed to insert item2");

            // Verify items are present
            assert!(
                filter.query(b"item1").expect("Failed to query item1"),
                "item1 should be present"
            );
            assert!(
                filter.query(b"item2").expect("Failed to query item2"),
                "item2 should be present"
            );
            assert!(
                !filter.query(b"item3").expect("Failed to query item3"),
                "item3 should not be present"
            );

            // Force a snapshot by sleeping
            std::thread::sleep(Duration::from_millis(150));
        } // Filter is dropped here, forcing a final snapshot

        // Create configuration
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // 2. Second instance: Verify state is restored after loading from disk
        {
            let filter = RedbExpiringBloomFilter::new(config)
                .expect("Failed to create filter");

            // Check items are still present after reload
            assert!(
                filter.query(b"item1").expect("Failed to query item1"),
                "item1 should be present after reload"
            );
            assert!(
                filter.query(b"item2").expect("Failed to query item2"),
                "item2 should be present after reload"
            );
        }

        // Create configuration
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // 3. Third instance: Wait for items to expire
        {
            let mut filter = RedbExpiringBloomFilter::new(config)
                .expect("Failed to create filter");

            // Wait for items to expire (3 * level_duration)
            std::thread::sleep(Duration::from_secs(1));

            // Insert a new item to trigger cleanup
            filter.insert(b"item3").expect("Failed to insert item3");

            // Original items should be expired
            assert!(
                !filter.query(b"item1").expect("Failed to query item1"),
                "item1 should have expired"
            );
            assert!(
                !filter.query(b"item2").expect("Failed to query item2"),
                "item2 should have expired"
            );

            // New item should be present
            assert!(
                filter.query(b"item3").expect("Failed to query item3"),
                "item3 should be present"
            );

            // Force a snapshot
            std::thread::sleep(Duration::from_millis(150));
        }

        // Create configuration
        let config = RedbExpiringloomFilterConfigBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .max_levels(3)
            .snapshot_interval(Duration::from_millis(100))
            .false_positive_rate(0.01)
            .build()
            .expect("Failed to build config");

        // 4. Fourth instance: Verify expired state is maintained after reload
        {
            let filter = RedbExpiringBloomFilter::new(config)
                .expect("Failed to create filter");

            // Verify expired items are still expired after reload
            assert!(
                !filter.query(b"item1").expect("Failed to query item1"),
                "item1 should still be expired after reload"
            );
            assert!(
                !filter.query(b"item2").expect("Failed to query item2"),
                "item2 should still be expired after reload"
            );

            // Verify recent item is still present
            assert!(
                filter.query(b"item3").expect("Failed to query item3"),
                "item3 should still be present after reload"
            );
        }

        cleanup_db(&path);
    }
} */
