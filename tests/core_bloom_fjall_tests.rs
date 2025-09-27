#[cfg(feature = "fjall")]
mod tests {
    use expiring_bloom_rs::bloom::{
        BloomFilter, BloomFilterConfig, BloomFilterConfigBuilder, BloomFilterOps,
        BloomFilterStats, PersistenceConfigBuilder,
    };
    use std::{fs, path::PathBuf, sync::Arc, thread, time::Duration};

    struct TestDb {
        path: PathBuf,
    }

    impl TestDb {
        fn new(test_name: &str) -> Self {
            let path =
                PathBuf::from(format!("test_core_bloom_{}.fjall", test_name));
            Self { path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            if self.path.exists() {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }

    fn create_test_config(db_path: PathBuf) -> BloomFilterConfig {
        let persistence = PersistenceConfigBuilder::default()
            .db_path(db_path)
            .chunk_size_bytes(4096)
            .snapshot_interval(Duration::from_secs(60))
            .auto_snapshot(false)
            .build()
            .unwrap();

        BloomFilterConfigBuilder::default()
            .capacity(10_000)
            .false_positive_rate(0.01)
            .persistence(Some(persistence))
            .build()
            .unwrap()
    }

    fn create_in_memory_config() -> BloomFilterConfig {
        BloomFilterConfigBuilder::default()
            .capacity(10_000)
            .false_positive_rate(0.01)
            .persistence(None)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_basic_persistence_lifecycle() {
        let test_db = TestDb::new("basic_lifecycle");
        let config = create_test_config(test_db.path.clone());

        // Create new filter and insert data
        {
            let filter = BloomFilter::create(config.clone()).await.unwrap();

            // Insert test data
            filter.insert(b"test_item_1").unwrap();
            filter.insert(b"test_item_2").unwrap();
            filter.insert(b"test_item_3").unwrap();

            // Verify items exist
            assert!(filter.contains(b"test_item_1").unwrap());
            assert!(filter.contains(b"test_item_2").unwrap());
            assert!(filter.contains(b"test_item_3").unwrap());
            assert!(!filter.contains(b"nonexistent").unwrap());

            // Save snapshot
            filter.save_snapshot().await.unwrap();
        }

        // Load filter from database and verify
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();

            // Verify all items still exist
            assert!(filter.contains(b"test_item_1").unwrap());
            assert!(filter.contains(b"test_item_2").unwrap());
            assert!(filter.contains(b"test_item_3").unwrap());
            assert!(!filter.contains(b"nonexistent").unwrap());

            // Verify config was loaded correctly
            assert_eq!(filter.capacity(), 10_000);
            assert_eq!(filter.false_positive_rate(), 0.01);
        }
    }

    #[tokio::test]
    async fn test_create_or_load_new_db() {
        let test_db = TestDb::new("create_or_load_new");
        let config = create_test_config(test_db.path.clone());

        // Verify DB doesn't exist
        assert!(!test_db.path.exists());

        // create_or_load should create new DB
        let filter = BloomFilter::create_or_load(config).await.unwrap();
        filter.insert(b"new_item").unwrap();
        assert!(filter.contains(b"new_item").unwrap());

        // Verify DB was created
        assert!(test_db.path.exists());
    }

    #[tokio::test]
    async fn test_create_or_load_existing_db() {
        let test_db = TestDb::new("create_or_load_existing");
        let config = create_test_config(test_db.path.clone());

        // First create a DB with some data
        {
            let filter = BloomFilter::create(config.clone()).await.unwrap();
            filter.insert(b"existing_item").unwrap();
            filter.save_snapshot().await.unwrap();
        }

        // create_or_load should load existing DB (ignoring config params)
        let filter = BloomFilter::create_or_load(config).await.unwrap();
        assert!(filter.contains(b"existing_item").unwrap());
    }

    #[tokio::test]
    async fn test_chunked_storage_integrity() {
        let test_db = TestDb::new("chunked_storage");
        let config = create_test_config(test_db.path.clone());

        let test_items: Vec<String> =
            (0..1000).map(|i| format!("chunked_item_{}", i)).collect();

        // Create filter and insert many items to test chunking
        {
            let filter = BloomFilter::create(config).await.unwrap();

            for item in &test_items {
                filter.insert(item.as_bytes()).unwrap();
            }

            // Verify all items
            for item in &test_items {
                assert!(
                    filter.contains(item.as_bytes()).unwrap(),
                    "Item should exist: {}",
                    item
                );
            }

            filter.save_snapshot().await.unwrap();
        }

        // Load and verify all data survived chunked persistence
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();

            for item in &test_items {
                assert!(
                    filter.contains(item.as_bytes()).unwrap(),
                    "Item should exist after load: {}",
                    item
                );
            }
        }
    }

    #[tokio::test]
    async fn test_multiple_save_load_cycles() {
        let test_db = TestDb::new("multiple_cycles");
        let config = create_test_config(test_db.path.clone());

        let mut all_items = Vec::new();

        // Multiple save/load cycles
        for cycle in 0..3 {
            let filter = if cycle == 0 {
                BloomFilter::create(config.clone()).await.unwrap()
            } else {
                BloomFilter::load(test_db.path.clone()).await.unwrap()
            };

            // Add new items in each cycle
            let cycle_items: Vec<String> = (0..100)
                .map(|i| format!("cycle_{}_item_{}", cycle, i))
                .collect();

            for item in &cycle_items {
                filter.insert(item.as_bytes()).unwrap();
                all_items.push(item.clone());
            }

            // Verify all items from all cycles
            for item in &all_items {
                assert!(filter.contains(item.as_bytes()).unwrap());
            }

            filter.save_snapshot().await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_config_persistence() {
        let test_db = TestDb::new("config_persistence");

        let custom_config = {
            let persistence = PersistenceConfigBuilder::default()
                .db_path(test_db.path.clone())
                .chunk_size_bytes(8192) // Custom chunk size
                .snapshot_interval(Duration::from_secs(30)) // Custom interval
                .auto_snapshot(true) // Custom auto snapshot
                .build()
                .unwrap();

            BloomFilterConfigBuilder::default()
                .capacity(50_000) // Custom capacity
                .false_positive_rate(0.005) // Custom FPR
                .persistence(Some(persistence))
                .build()
                .unwrap()
        };

        // Create filter with custom config
        {
            let filter = BloomFilter::create(custom_config).await.unwrap();
            assert_eq!(filter.capacity(), 50_000);
            assert_eq!(filter.false_positive_rate(), 0.005);
        }

        // Load filter and verify config was preserved
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();
            assert_eq!(filter.capacity(), 50_000);
            assert_eq!(filter.false_positive_rate(), 0.005);
        }
    }

    #[tokio::test]
    async fn test_empty_filter_persistence() {
        let test_db = TestDb::new("empty_filter");
        let config = create_test_config(test_db.path.clone());

        // Create and save empty filter
        {
            let filter = BloomFilter::create(config).await.unwrap();
            assert_eq!(filter.insert_count(), 0);
            filter.save_snapshot().await.unwrap();
        }

        // Load empty filter
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();
            assert_eq!(filter.insert_count(), 0);
            assert!(!filter.contains(b"anything").unwrap());
        }
    }

    #[tokio::test]
    async fn test_large_items_persistence() {
        let test_db = TestDb::new("large_items");
        let config = create_test_config(test_db.path.clone());

        // Create large test items (1KB each)
        let large_items: Vec<Vec<u8>> = (0..10)
            .map(|i| {
                let base = format!("large_item_{}_", i);
                let mut item = base.into_bytes();
                item.resize(1024, b'x'); // Pad to 1KB
                item
            })
            .collect();

        // Insert and save large items
        {
            let filter = BloomFilter::create(config).await.unwrap();

            for item in &large_items {
                filter.insert(item).unwrap();
            }

            for item in &large_items {
                assert!(filter.contains(item).unwrap());
            }

            filter.save_snapshot().await.unwrap();
        }

        // Verify large items survived persistence
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();

            for item in &large_items {
                assert!(filter.contains(item).unwrap());
            }
        }
    }

    #[tokio::test]
    async fn test_error_handling_invalid_db_path() {
        let invalid_path = PathBuf::from("/invalid/nonexistent/path/bloom.fjall");

        let result = BloomFilter::load(invalid_path).await;
        assert!(result.is_err());
        match result {
            Err(expiring_bloom_rs::bloom::BloomError::StorageError(_)) => {}
            _ => panic!("Expected StorageError"),
        }
    }

    #[tokio::test]
    async fn test_error_handling_corrupted_config() {
        let test_db = TestDb::new("corrupted_config");
        let config = create_test_config(test_db.path.clone());

        // Create filter first
        {
            let _filter = BloomFilter::create(config).await.unwrap();
        }

        // Test that invalid config fails validation when used
        let invalid_config = BloomFilterConfigBuilder::default()
            .capacity(0) // Invalid capacity
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        // This should fail when trying to create a filter
        let result = BloomFilter::create(invalid_config).await;
        assert!(result.is_err());
        match result {
            Err(expiring_bloom_rs::bloom::BloomError::InvalidConfig(_)) => {}
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_concurrent_access_during_persistence() {
        let test_db = TestDb::new("concurrent_access");
        let config = create_test_config(test_db.path.clone());

        // Create filter and insert items sequentially (simplified test)
        let filter = BloomFilter::create(config).await.unwrap();

        // Insert items
        filter.insert(b"concurrent_item_1").unwrap();
        filter.insert(b"concurrent_item_2").unwrap();

        // Save snapshot
        filter.save_snapshot().await.unwrap();

        // Verify items exist
        assert!(filter.contains(b"concurrent_item_1").unwrap());
        assert!(filter.contains(b"concurrent_item_2").unwrap());
    }

    #[tokio::test]
    async fn test_arc_shared_concurrent_read_write() {
        const WRITER_THREADS: usize = 4;
        const ITEMS_PER_WRITER: usize = 50;
        const READER_THREADS: usize = 3;

        let test_db = TestDb::new("arc_concurrent_rw");
        let config = create_test_config(test_db.path.clone());
        let filter = Arc::new(BloomFilter::create(config).await.unwrap());

        // Spawn writers that concurrently insert disjoint items.
        let mut writer_handles = Vec::new();
        for writer_id in 0..WRITER_THREADS {
            let filter_clone = Arc::clone(&filter);
            writer_handles.push(thread::spawn(move || {
                for item_idx in 0..ITEMS_PER_WRITER {
                    let item = format!("writer_{writer_id}_item_{item_idx}");
                    filter_clone.insert(item.as_bytes()).unwrap();
                }
            }));
        }

        for handle in writer_handles {
            handle.join().expect("writer thread should finish");
        }

        // Prepare the full set of items each reader will validate.
        let all_items: Vec<Vec<u8>> = (0..WRITER_THREADS)
            .flat_map(|writer_id| {
                (0..ITEMS_PER_WRITER).map(move |item_idx| {
                    format!("writer_{writer_id}_item_{item_idx}").into_bytes()
                })
            })
            .collect();

        let mut reader_handles = Vec::new();
        for reader_id in 0..READER_THREADS {
            let filter_clone = Arc::clone(&filter);
            let chunk: Vec<Vec<u8>> = all_items
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % READER_THREADS == reader_id)
                .map(|(_, item)| item.clone())
                .collect();

            reader_handles.push(thread::spawn(move || {
                for item in chunk {
                    assert!(
                        filter_clone.contains(&item).unwrap(),
                        "reader observed missing item"
                    );
                }
            }));
        }

        for handle in reader_handles {
            handle.join().expect("reader thread should finish");
        }

        assert_eq!(
            filter.insert_count(),
            WRITER_THREADS * ITEMS_PER_WRITER,
            "all concurrent inserts should be tracked"
        );

        // Ensure persistence still works after concurrent activity.
        filter.save_snapshot().await.unwrap();

        // Drop the Arc to release backend handles before reloading.
        drop(filter);

        // Reload and confirm one of the items persists.
        let reloaded = BloomFilter::load(test_db.path.clone()).await.unwrap();
        assert!(
            reloaded.contains(b"writer_0_item_0").unwrap(),
            "item should persist after reload"
        );
    }

    #[tokio::test]
    async fn test_in_memory_vs_persistent_behavior() {
        // Test that persistent and in-memory filters behave identically
        let test_db = TestDb::new("behavior_comparison");
        let persistent_config = create_test_config(test_db.path.clone());
        let memory_config = create_in_memory_config();

        let test_items: Vec<&[u8]> = vec![b"item1", b"item2", b"item3", b"item4"];

        // Test persistent filter
        let persistent_filter =
            BloomFilter::create(persistent_config).await.unwrap();
        for &item in &test_items {
            persistent_filter.insert(item).unwrap();
        }

        // Test in-memory filter
        let memory_filter = BloomFilter::create(memory_config).await.unwrap();
        for &item in &test_items {
            memory_filter.insert(item).unwrap();
        }

        // Both should have identical behavior for contains()
        for &item in &test_items {
            assert_eq!(
                persistent_filter.contains(item).unwrap(),
                memory_filter.contains(item).unwrap()
            );
        }

        // Both should have identical stats
        assert_eq!(persistent_filter.capacity(), memory_filter.capacity());
        assert_eq!(
            persistent_filter.false_positive_rate(),
            memory_filter.false_positive_rate()
        );
        assert_eq!(
            persistent_filter.insert_count(),
            memory_filter.insert_count()
        );
    }

    #[tokio::test]
    async fn test_persistence_with_clear() {
        let test_db = TestDb::new("clear_persistence");
        let config = create_test_config(test_db.path.clone());

        // Create filter, add items, clear, add different items
        {
            let filter = BloomFilter::create(config).await.unwrap();

            filter.insert(b"item1").unwrap();
            filter.insert(b"item2").unwrap();
            assert!(filter.contains(b"item1").unwrap());

            filter.clear().unwrap();
            assert!(!filter.contains(b"item1").unwrap());
            assert_eq!(filter.insert_count(), 0);

            filter.insert(b"item3").unwrap();
            assert!(filter.contains(b"item3").unwrap());
            assert!(!filter.contains(b"item1").unwrap());

            filter.save_snapshot().await.unwrap();
        }

        // Verify cleared state persisted
        {
            let filter = BloomFilter::load(test_db.path.clone()).await.unwrap();
            assert!(!filter.contains(b"item1").unwrap());
            assert!(!filter.contains(b"item2").unwrap());
            assert!(filter.contains(b"item3").unwrap());
        }
    }
}
