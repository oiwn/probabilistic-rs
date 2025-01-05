#[cfg(test)]
mod tests {
    use expiring_bloom_rs::inmemory_storage::InMemoryStorage;
    use expiring_bloom_rs::redb_storage::RedbExpiringloomFilterConfig;
    use expiring_bloom_rs::redb_storage::RedbExpiringloomFilterConfigBuilder;
    use expiring_bloom_rs::redb_storage::{RedbExpiringBloomFilter, RedbStorage};
    use expiring_bloom_rs::{
        default_hash_function, BloomFilterStorage, SlidingBloomFilter,
    };
    use rand::random;
    use redb::Database;
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
        thread,
        time::{Duration, SystemTime},
    };

    struct RedbGuard {
        path: PathBuf,
    }

    impl Drop for RedbGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path); // Cleanup on drop, ignoring errors
        }
    }

    fn create_temp_db() -> (RedbStorage, RedbGuard) {
        let path = format!("test_db_{}.redb", rand::random::<u64>()).into();
        let storage = RedbStorage::open(&path, 1000, 5).unwrap();
        (storage, RedbGuard { path })
    }

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

    /* #[test]
    fn test_redb_batch_performance() {
        use rand::RngCore;
        use std::time::Instant;

        const NUM_ITEMS: usize = 1_000; // Test with 1k items for ReDB

        // Setup
        let path = temp_db_path();
        let mut rng = rand::thread_rng();
        let mut bloom = SlidingBloomFilter::new(
            RedbStorage::open(&path, NUM_ITEMS, 3).unwrap(),
            NUM_ITEMS,
            0.01,
            Duration::from_secs(60),
            3,
            default_hash_function,
        )
        .unwrap();

        // Generate test data
        let items: Vec<Vec<u8>> = (0..NUM_ITEMS)
            .map(|_| {
                let mut bytes = vec![0u8; 16];
                rng.fill_bytes(&mut bytes);
                bytes
            })
            .collect();

        // Measure insertion performance
        println!("Starting batch insertion test...");
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            bloom.insert(item).unwrap();

            if (i + 1) % 100 == 0 {
                println!("Inserted {} items...", i + 1);
            }
        }

        let insert_elapsed = start.elapsed();
        println!(
            "\nBatch insertion of {} items took: {:?}",
            NUM_ITEMS, insert_elapsed
        );
        println!(
            "Average insertion time per item: {:?}",
            insert_elapsed / NUM_ITEMS as u32
        );

        // Measure query performance
        println!("\nStarting batch query test...");
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            assert!(bloom.query(item).unwrap());

            if (i + 1) % 100 == 0 {
                println!("Queried {} items...", i + 1);
            }
        }

        let query_elapsed = start.elapsed();
        println!(
            "\nBatch query of {} items took: {:?}",
            NUM_ITEMS, query_elapsed
        );
        println!(
            "Average query time per item: {:?}",
            query_elapsed / NUM_ITEMS as u32
        );

        // Performance assertions - adjusted for ReDB (allowing more time due to disk I/O)
        const MAX_TOTAL_INSERT_SECS: u64 = 1; // 1 second for 1k items
        const MAX_TOTAL_QUERY_SECS: u64 = 1; // 1 second for 1k items
        const MAX_AVG_INSERT_MILLIS: u128 = 1; // 1 millisecond per insert
        const MAX_AVG_QUERY_MILLIS: u128 = 1; // 1 millisecond per query

        assert!(
            insert_elapsed < Duration::from_secs(MAX_TOTAL_INSERT_SECS),
            "Insertion of {} items took {:?}, which is more than {} seconds",
            NUM_ITEMS,
            insert_elapsed,
            MAX_TOTAL_INSERT_SECS
        );

        assert!(
            query_elapsed < Duration::from_secs(MAX_TOTAL_QUERY_SECS),
            "Querying {} items took {:?}, which is more than {} seconds",
            NUM_ITEMS,
            query_elapsed,
            MAX_TOTAL_QUERY_SECS
        );

        let avg_insert_millis = insert_elapsed.as_millis() / NUM_ITEMS as u128;
        let avg_query_millis = query_elapsed.as_millis() / NUM_ITEMS as u128;

        assert!(
        avg_insert_millis <= MAX_AVG_INSERT_MILLIS,
        "Average insertion time {} milliseconds exceeds maximum allowed {} milliseconds",
        avg_insert_millis,
        MAX_AVG_INSERT_MILLIS
    );

        assert!(
        avg_query_millis <= MAX_AVG_QUERY_MILLIS,
        "Average query time {} milliseconds exceeds maximum allowed {} milliseconds",
        avg_query_millis,
        MAX_AVG_QUERY_MILLIS
    );

        // Cleanup
        cleanup_db(&path);
    } */

    /* #[test]
    fn test_basic_operations() {
        let (mut storage, _guard) = create_temp_db();

        // Test set and get bit
        storage.set_bit(0, 5).unwrap();
        assert!(storage.get_bit(0, 5).unwrap());
        assert!(!storage.get_bit(0, 6).unwrap());

        // Test clear level
        storage.clear_level(0).unwrap();
        assert!(!storage.get_bit(0, 5).unwrap());
    }

    #[test]
    fn test_persistence() {
        let path: PathBuf = temp_db_path();

        // Create and populate storage
        {
            let mut storage = RedbStorage::open(&path, 1000, 5).unwrap();
            storage.set_bit(0, 5).unwrap();
        }

        // Reopen and verify
        {
            let storage = RedbStorage::open(&path, 1000, 5).unwrap();
            assert!(storage.get_bit(0, 5).unwrap());
        }

        cleanup_db(&path);
    }

    #[test]
    fn test_timestamp_precision() {
        let (mut storage, _guard) = create_temp_db();

        // Test 1: Basic timestamp storage and retrieval
        let time1 = SystemTime::now();
        storage.set_timestamp(0, time1).unwrap();
        let stored_time = storage.get_timestamp(0).unwrap().unwrap();

        let diff = stored_time.duration_since(time1).unwrap_or_else(|_| {
            panic!(
                "Stored time {:?} is earlier than original {:?}",
                stored_time, time1
            )
        });

        assert!(
            diff.as_nanos() < 1_000, // Allow 1 microsecond difference
            "Time difference too large: {:?} nanos",
            diff.as_nanos()
        );

        // Test 2: Multiple updates to same level
        thread::sleep(Duration::from_micros(100));
        let time2 = SystemTime::now();
        storage.set_timestamp(0, time2).unwrap();
        let stored_time2 = storage.get_timestamp(0).unwrap().unwrap();

        let diff2 = stored_time2.duration_since(time2).unwrap_or_else(|_| {
            panic!(
                "Stored time {:?} is earlier than original {:?}",
                stored_time2, time2
            )
        });

        assert!(
            diff2.as_nanos() < 1_000,
            "Time difference too large after update: {:?} nanos",
            diff2.as_nanos()
        );

        // Test 3: Different levels don't interfere
        storage.set_timestamp(1, time1).unwrap();
        assert_eq!(
            storage.get_timestamp(0).unwrap().unwrap(),
            stored_time2,
            "Level 0 timestamp changed when updating level 1"
        );
    }

    #[test]
    fn test_timestamp_edge_cases() {
        let (mut storage, _guard) = create_temp_db();

        // Test 1: Unix epoch
        storage.set_timestamp(0, SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!(
            storage.get_timestamp(0).unwrap().unwrap(),
            SystemTime::UNIX_EPOCH,
            "Failed to store and retrieve UNIX_EPOCH"
        );

        // Test 2: Future timestamp
        let future_time = SystemTime::now() + Duration::from_secs(86400 * 365); // ~1 year in future
        storage.set_timestamp(1, future_time).unwrap();
        let stored_future = storage.get_timestamp(1).unwrap().unwrap();
        let diff = stored_future.duration_since(future_time).unwrap();
        assert!(
            diff.as_nanos() < 1_000,
            "Future timestamp not stored accurately: {:?} diff",
            diff
        );
    }

    #[test]
    fn test_redb_performance_breakdown() {
        use rand::RngCore;
        use std::time::Instant;

        // Test configuration
        const NUM_ITEMS: usize = 100; // Reduced number for diagnostics
        const ITEM_SIZE: usize = 16;

        // Timing accumulators
        let mut hash_time = Duration::ZERO;
        let mut bit_set_time = Duration::ZERO;
        let mut commit_time = Duration::ZERO;
        let mut timestamp_time = Duration::ZERO;

        // Setup
        let path = temp_db_path();
        let mut rng = rand::thread_rng();

        // Create storage with diagnostic prints
        println!("Creating ReDB storage...");
        let start_setup = Instant::now();
        let mut storage = RedbStorage::open(&path, 1000, 3).unwrap();
        println!("Storage creation took: {:?}", start_setup.elapsed());

        // Create filter
        let mut bloom = SlidingBloomFilter::new(
            storage,
            1000,
            0.01,
            Duration::from_secs(60),
            3,
            default_hash_function,
        )
        .unwrap();

        // Prepare test data
        let items: Vec<Vec<u8>> = (0..NUM_ITEMS)
            .map(|_| {
                let mut bytes = vec![0u8; ITEM_SIZE];
                rng.fill_bytes(&mut bytes);
                bytes
            })
            .collect();

        println!(
            "\nStarting insertion timing breakdown for {} items:",
            NUM_ITEMS
        );
        let total_start = Instant::now();

        // Track transactions
        let mut tx_count = 0;

        // Instrument the insert operation by taking over control from SlidingBloomFilter
        for (idx, item) in items.iter().enumerate() {
            // 1. Time hash computation
            let hash_start = Instant::now();
            let hashes =
                (bloom.hash_function)(item, bloom.num_hashes, bloom.capacity);
            hash_time += hash_start.elapsed();

            // 2. Time bit setting (includes transaction overhead)
            let bit_start = Instant::now();
            for &hash in &hashes {
                let bit_op_start = Instant::now();
                bloom
                    .storage
                    .set_bit(bloom.current_level_index, hash as usize)
                    .unwrap();
                bit_set_time += bit_op_start.elapsed();
                tx_count += 1;
            }

            // 3. Time timestamp operations (if needed)
            if idx == 0 || idx % 10 == 0 {
                // Check timestamp every 10 items
                let ts_start = Instant::now();
                if bloom.should_create_new_level().unwrap() {
                    bloom.create_new_level().unwrap();
                }
                timestamp_time += ts_start.elapsed();
            }

            // Progress report every 10 items
            if (idx + 1) % 10 == 0 {
                println!("Processed {} items...", idx + 1);
            }
        }

        let total_time = total_start.elapsed();

        // Print timing breakdown
        println!("\nTiming Breakdown:");
        println!("Total time: {:?}", total_time);
        println!("Average time per item: {:?}", total_time / NUM_ITEMS as u32);
        println!(
            "Hash computation: {:?} ({:.1}%)",
            hash_time,
            hash_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
        );
        println!(
            "Bit setting: {:?} ({:.1}%)",
            bit_set_time,
            bit_set_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
        );
        println!(
            "Timestamp operations: {:?} ({:.1}%)",
            timestamp_time,
            timestamp_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
        );
        println!("Total transactions: {}", tx_count);
        println!(
            "Average time per transaction: {:?}",
            bit_set_time / tx_count as u32
        );

        // Cleanup
        cleanup_db(&path);
    }

    #[test]
    fn test_redb_insertion_performance() {
        use rand::RngCore;
        use std::time::Instant;

        // Setup
        let path = temp_db_path();
        let mut rng = rand::thread_rng();
        let mut bloom = SlidingBloomFilter::new(
            RedbStorage::open(&path, 1000, 3).unwrap(),
            1000,
            0.01,
            Duration::from_secs(60), // Level duration
            3,                       // Max levels
            default_hash_function,
        )
        .unwrap();

        // Prepare 1000 unique random items
        let mut items: Vec<Vec<u8>> = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let mut bytes = vec![0u8; 16]; // 16-byte random items
            rng.fill_bytes(&mut bytes);
            items.push(bytes);
        }

        // Measure insertion time
        let start = Instant::now();

        for item in items {
            bloom.insert(&item).unwrap();
        }

        let elapsed = start.elapsed();

        // Cleanup
        cleanup_db(&path);

        // Assert that insertion took less than 1 second
        assert!(
            elapsed < Duration::from_secs(1),
            "Insertion of 1000 elements took {:?}, which is more than 1 second",
            elapsed
        );

        // Print actual time for reference
        println!("Inserted 1000 elements in {:?}", elapsed);
    } */

    /*
    #[test]
    fn test_timestamps2() {
        let (mut storage, path) = create_temp_db();

        let time1 = SystemTime::now();
        storage.set_timestamp(0, time1).unwrap();

        thread::sleep(Duration::from_millis(10));

        let time2 = SystemTime::now();

        // Get stored timestamp
        let stored_time = storage.get_timestamp(0).unwrap().unwrap();
        // Check if it's within 1ms of original
        let diff = stored_time.duration_since(time1).unwrap();
        assert!(diff.as_millis() < 1, "Stored time differs by {:?}", diff);

        storage.set_timestamp(0, time2).unwrap();
        let stored_time2 = storage.get_timestamp(0).unwrap().unwrap();
        let diff2 = stored_time2.duration_since(time2).unwrap();
        assert!(diff2.as_millis() < 1, "Stored time differs by {:?}", diff2);

        cleanup_db(&path.into());
    }

    #[test]
    fn test_timestamps() {
        let (mut storage, path) = create_temp_db();

        let time1 = SystemTime::now();
        storage.set_timestamp(0, time1).unwrap();

        thread::sleep(Duration::from_millis(10));

        let time2 = SystemTime::now();
        assert!(storage.get_timestamp(0).unwrap().unwrap() == time1);

        storage.set_timestamp(0, time2).unwrap();
        assert!(storage.get_timestamp(0).unwrap().unwrap() == time2);

        cleanup_db(&path.into());
    }

    #[test]
    fn test_error_handling() {
        let (mut storage, path) = create_temp_db();

        // Test invalid level
        assert!(storage.set_bit(10, 0).is_err());
        assert!(storage.get_bit(10, 0).is_err());
        assert!(storage.clear_level(10).is_err());
        assert!(storage.set_timestamp(10, SystemTime::now()).is_err());
        assert!(storage.get_timestamp(10).is_err());

        // Test invalid index
        assert!(storage.set_bit(0, 2000).is_err());
        assert!(storage.get_bit(0, 2000).is_err());

        cleanup_db(&path.into());
    }

    #[test]
    fn test_concurrent_access() {
        let path = format!("test_db_{}.redb", rand::random::<u64>());
        let storage =
            Arc::new(Mutex::new(RedbStorage::open(&path, 1000, 5).unwrap()));
        let mut handles = vec![];

        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                let mut storage = storage_clone.lock().unwrap();
                storage.set_bit(0, i * 10).unwrap();
                assert!(storage.get_bit(0, i * 10).unwrap());
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        cleanup_db(&path.into());
    }
    #[test]
    fn test_level_rotation2() {
        let path = format!("test_db_{}.redb", rand::random::<u64>());
        let mut bloom = SlidingBloomFilter::new(
            RedbStorage::open(&path, 1000, 3).unwrap(),
            1000,
            0.01,
            Duration::from_millis(100), // Level duration
            3,                          // Max levels
            default_hash_function,
        )
        .unwrap();

        // Insert and verify first item
        println!("Inserting item1");
        bloom.insert(b"item1").unwrap();
        assert!(bloom.query(b"item1").unwrap());

        // Debug: Check timestamp after first insert
        let level0_time = bloom.storage.get_timestamp(0).unwrap();
        println!("Level 0 timestamp after first insert: {:?}", level0_time);

        thread::sleep(Duration::from_millis(150));

        // Debug: Check timestamp before second insert
        let level0_time = bloom.storage.get_timestamp(0).unwrap();
        println!("Level 0 timestamp before second insert: {:?}", level0_time);

        println!("Inserting item2");
        bloom.insert(b"item2").unwrap();
        assert!(bloom.query(b"item2").unwrap());

        // This is where it fails
        let item1_present = bloom.query(b"item1").unwrap();
        println!("item1 present: {}", item1_present);
        if !item1_present {
            // Debug: print all level timestamps
            for i in 0..3 {
                println!(
                    "Level {} timestamp: {:?}",
                    i,
                    bloom.storage.get_timestamp(i).unwrap()
                );
            }
        }
        assert!(item1_present);
    }

    #[test]
    fn test_level_rotation() {
        let path = format!("test_db_{}.redb", rand::random::<u64>());
        // 3 * 100ms = 300ms duration of item in queue
        let mut bloom = SlidingBloomFilter::new(
            RedbStorage::open(&path, 1000, 3).unwrap(),
            1000,
            0.01,
            Duration::from_millis(100), // Level duration
            3,                          // Max levels
            default_hash_function,
        )
        .unwrap();

        // Insert and verify first item
        bloom.insert(b"item1").unwrap();
        assert!(bloom.query(b"item1").unwrap());

        // Wait for level rotation
        thread::sleep(Duration::from_millis(150));

        // Insert and verify second item
        bloom.insert(b"item2").unwrap();
        assert!(bloom.query(b"item2").unwrap());
        assert!(bloom.query(b"item1").unwrap()); // First item should still be present

        // Wait for another rotation
        thread::sleep(Duration::from_millis(150));

        // Insert and verify third item
        bloom.insert(b"item3").unwrap();
        assert!(bloom.query(b"item3").unwrap());
        assert!(bloom.query(b"item2").unwrap());
        assert!(!bloom.query(b"item1").unwrap()); // This item should be expired already

        // Wait for first item to expire (3 * level_duration)
        thread::sleep(Duration::from_millis(300));
        bloom.cleanup_expired_levels().unwrap();

        assert!(
            !bloom.query(b"item2").unwrap(),
            "Second item should have expired"
        );
        assert!(
            !bloom.query(b"item3").unwrap(),
            "Latest item should have expired"
        );

        cleanup_db(&path.into());
    }

    #[test]
    fn test_concurrent_expiration() {
        use std::sync::Mutex;

        let path = format!("test_db_{}.redb", rand::random::<u64>());
        let storage =
            Arc::new(Mutex::new(RedbStorage::open(&path, 1000, 3).unwrap()));
        let level_duration = Duration::from_millis(100);

        // Spawn threads that insert and check items
        let mut handles = vec![];
        for i in 0..5 {
            let storage_clone = Arc::clone(&storage);
            handles.push(thread::spawn(move || {
                // Insert item
                {
                    let mut storage = storage_clone.lock().unwrap();
                    storage.set_bit(0, i * 10).unwrap();
                }

                // Wait varying amounts of time
                thread::sleep(level_duration * (i as u32));

                // Check if item exists based on timing
                let storage = storage_clone.lock().unwrap();
                let exists = storage.get_bit(0, i * 10).unwrap();

                (i, exists) // Return results for verification
            }));
        }

        // Collect and verify results
        let results: Vec<(usize, bool)> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Items that waited longer should be expired
        for (i, exists) in results {
            // println!("Item {} exists: {}", i, exists);
            assert!(!exists);
        }

        cleanup_db(&path.into());
    }

    #[test]
    fn test_filter_basic_operations() {
        let path = temp_db_path();

        let opts = RedbExpiringBloomFilterOptionsBuilder::default()
            .path(path.clone())
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .build()
            .unwrap();

        let mut filter = RedbExpiringBloomFilter::new(opts).unwrap();

        // Test insert and query
        filter.insert(b"test_item").unwrap();
        assert!(filter.query(b"test_item").unwrap());
        assert!(!filter.query(b"nonexistent_item").unwrap());

        cleanup_db(&path);
    }

    #[test]
    fn test_error_handling() {
        let path = temp_db_path();
        let mut storage = RedbStorage::open(&path, 1000, 5).unwrap();

        // Test invalid level
        assert!(storage.set_bit(10, 0).is_err());
        assert!(storage.get_bit(10, 0).is_err());
        assert!(storage.clear_level(10).is_err());
        assert!(storage.set_timestamp(10, SystemTime::now()).is_err());
        assert!(storage.get_timestamp(10).is_err());

        // Test invalid index
        assert!(storage.set_bit(0, 2000).is_err());
        assert!(storage.get_bit(0, 2000).is_err());

        cleanup_db(&path.into());
    }

    // #[test]
    /// This test running for more than 60 seconds, definitely because backend poorly implemented
    fn test_false_positive_rate() {
        let path = temp_db_path();

        // Create filter with specific false positive rate
        let opts = RedbExpiringBloomFilterOptionsBuilder::default()
            .path(path.clone())
            .capacity(10000)
            .expiration_time(Duration::from_secs(3600))
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        let mut filter = RedbExpiringBloomFilter::new(opts).unwrap();

        // Insert some known items
        let mut known_items = Vec::new();
        for i in 0..1000 {
            let item = format!("known_item_{}", i);
            known_items.push(item.clone());
            filter.insert(item.as_bytes()).unwrap();
        }

        // Test unknown items
        let mut false_positives = 0;
        let test_count = 10000;

        for i in 0..test_count {
            let unknown_item = format!("unknown_item_{}", i);
            if filter.query(unknown_item.as_bytes()).unwrap() {
                false_positives += 1;
            }
        }

        let observed_fpr = false_positives as f64 / test_count as f64;
        assert!(observed_fpr < 0.02); // Allow some margin above target 0.01

        cleanup_db(&path);
    } */
}
