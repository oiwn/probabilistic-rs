#[cfg(feature = "fjall")]
mod tests {
    use probabilistic_rs::ebloom::{
        ExpiringBloomFilter,
        config::{ExpiringFilterConfigBuilder, ExpiringPersistenceConfigBuilder},
        traits::{ExpiringBloomFilterOps, ExpiringBloomFilterStats},
    };
    use std::{fs, path::PathBuf, time::Duration};

    struct TestDb {
        path: PathBuf,
    }

    impl TestDb {
        fn new(test_name: &str) -> Self {
            let path = PathBuf::from(format!("test_ebloom_{}.fjall", test_name));
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

    fn make_config(
        db_path: PathBuf,
    ) -> probabilistic_rs::ebloom::config::ExpiringFilterConfig {
        let persistence = ExpiringPersistenceConfigBuilder::default()
            .db_path(db_path)
            .chunk_size_bytes(4096)
            .build()
            .unwrap();

        ExpiringFilterConfigBuilder::default()
            .capacity_per_level(10_000usize)
            .target_fpr(0.01)
            .level_duration(Duration::from_secs(3600))
            .num_levels(3usize)
            .persistence(Some(persistence))
            .build()
            .unwrap()
    }

    /// insert → save_snapshot (dirty chunks) → reload → data present
    #[tokio::test]
    async fn test_dirty_snapshot_survives_reload() {
        let db = TestDb::new("dirty_snapshot_reload");
        let config = make_config(db.path.clone());

        {
            let filter = ExpiringBloomFilter::create(config).await.unwrap();
            filter.insert(b"item_a").unwrap();
            filter.insert(b"item_b").unwrap();
            filter.insert(b"item_c").unwrap();

            assert!(filter.contains(b"item_a").unwrap());
            filter.save_snapshot().await.unwrap();
        }

        {
            let filter =
                ExpiringBloomFilter::load(db.path.clone()).await.unwrap();
            assert!(filter.contains(b"item_a").unwrap());
            assert!(filter.contains(b"item_b").unwrap());
            assert!(filter.contains(b"item_c").unwrap());
            assert!(!filter.contains(b"not_inserted").unwrap());
        }
    }

    /// insert → rotate_levels (full snapshot) → reload → pre-rotation data present
    #[tokio::test]
    async fn test_full_snapshot_on_rotation_survives_reload() {
        let db = TestDb::new("full_snapshot_rotation");
        let config = make_config(db.path.clone());

        {
            let filter = ExpiringBloomFilter::create(config).await.unwrap();
            filter.insert(b"before_rotation").unwrap();

            // Rotation saves a full snapshot of the current level
            filter.rotate_levels().await.unwrap();

            // Insert into the new current level after rotation
            filter.insert(b"after_rotation").unwrap();
            filter.save_snapshot().await.unwrap();
        }

        {
            let filter =
                ExpiringBloomFilter::load(db.path.clone()).await.unwrap();
            // Both items should be visible (different levels)
            assert!(filter.contains(b"before_rotation").unwrap());
            assert!(filter.contains(b"after_rotation").unwrap());
        }
    }

    /// Multiple inserts, multiple rotations, reload preserves all levels
    #[tokio::test]
    async fn test_multi_level_data_survives_reload() {
        let db = TestDb::new("multi_level_reload");
        let config = make_config(db.path.clone());

        {
            let filter = ExpiringBloomFilter::create(config).await.unwrap();

            filter.insert(b"level0_item").unwrap();
            filter.rotate_levels().await.unwrap();

            filter.insert(b"level1_item").unwrap();
            filter.rotate_levels().await.unwrap();

            filter.insert(b"level2_item").unwrap();
            filter.save_snapshot().await.unwrap();
        }

        {
            let filter =
                ExpiringBloomFilter::load(db.path.clone()).await.unwrap();
            assert!(filter.contains(b"level0_item").unwrap());
            assert!(filter.contains(b"level1_item").unwrap());
            assert!(filter.contains(b"level2_item").unwrap());
            assert!(!filter.contains(b"never_inserted").unwrap());
        }
    }

    /// create_or_load on nonexistent path creates fresh filter
    #[tokio::test]
    async fn test_create_or_load_new() {
        let db = TestDb::new("create_or_load_new");
        assert!(!db.path.exists());

        let config = make_config(db.path.clone());
        let filter = ExpiringBloomFilter::create_or_load(config).await.unwrap();
        filter.insert(b"fresh").unwrap();
        assert!(filter.contains(b"fresh").unwrap());
        assert!(db.path.exists());
    }

    /// create_or_load on existing path loads persisted data
    #[tokio::test]
    async fn test_create_or_load_existing() {
        let db = TestDb::new("create_or_load_existing");
        let config = make_config(db.path.clone());

        {
            let filter =
                ExpiringBloomFilter::create(config.clone()).await.unwrap();
            filter.insert(b"persisted_item").unwrap();
            filter.save_snapshot().await.unwrap();
        }

        let filter = ExpiringBloomFilter::create_or_load(config).await.unwrap();
        assert!(filter.contains(b"persisted_item").unwrap());
    }

    /// Config values survive a save/load round trip
    #[tokio::test]
    async fn test_config_survives_reload() {
        let db = TestDb::new("config_reload");

        let persistence = ExpiringPersistenceConfigBuilder::default()
            .db_path(db.path.clone())
            .build()
            .unwrap();
        let config = ExpiringFilterConfigBuilder::default()
            .capacity_per_level(50_000usize)
            .target_fpr(0.005)
            .level_duration(Duration::from_secs(7200))
            .num_levels(4usize)
            .persistence(Some(persistence))
            .build()
            .unwrap();

        {
            let _filter = ExpiringBloomFilter::create(config).await.unwrap();
        }

        let filter = ExpiringBloomFilter::load(db.path.clone()).await.unwrap();
        assert_eq!(filter.capacity_per_level(), 50_000);
        assert_eq!(filter.target_fpr(), 0.005);
        assert_eq!(filter.num_levels(), 4);
    }

    /// Insert counter increments on each write
    #[tokio::test]
    async fn test_insert_counter_increments() {
        let db = TestDb::new("insert_counter");
        let config = make_config(db.path.clone());
        let filter = ExpiringBloomFilter::create(config).await.unwrap();

        assert_eq!(filter.inserts_since_snapshot(), 0);
        filter.insert(b"a").unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 1);
        filter.insert(b"b").unwrap();
        filter.insert(b"c").unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 3);
    }

    /// Bulk inserts are counted correctly
    #[tokio::test]
    async fn test_bulk_insert_counter_increments() {
        use probabilistic_rs::ebloom::traits::BulkExpiringBloomFilterOps;

        let db = TestDb::new("bulk_insert_counter");
        let config = make_config(db.path.clone());
        let filter = ExpiringBloomFilter::create(config).await.unwrap();

        filter
            .insert_bulk(&[b"x".as_ref(), b"y".as_ref(), b"z".as_ref()])
            .unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 3);
    }

    /// Successful save_snapshot resets the counter
    #[tokio::test]
    async fn test_successful_snapshot_resets_counter() {
        let db = TestDb::new("snapshot_resets_counter");
        let config = make_config(db.path.clone());
        let filter = ExpiringBloomFilter::create(config).await.unwrap();

        filter.insert(b"item1").unwrap();
        filter.insert(b"item2").unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 2);

        filter.save_snapshot().await.unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 0);
    }

    /// save_snapshot on a clean filter (no dirty chunks) succeeds and keeps counter at 0
    #[tokio::test]
    async fn test_snapshot_skips_when_nothing_dirty() {
        let db = TestDb::new("snapshot_no_dirty");
        let config = make_config(db.path.clone());
        let filter = ExpiringBloomFilter::create(config).await.unwrap();

        filter.save_snapshot().await.unwrap();
        assert_eq!(filter.inserts_since_snapshot(), 0);
    }

    /// load from nonexistent path returns an error
    #[tokio::test]
    async fn test_load_nonexistent_path_errors() {
        let result =
            ExpiringBloomFilter::load(PathBuf::from("/no/such/path.fjall")).await;
        assert!(result.is_err());
    }

    /// Bulk inserts are correctly persisted
    #[tokio::test]
    async fn test_bulk_insert_persistence() {
        use probabilistic_rs::ebloom::traits::BulkExpiringBloomFilterOps;

        let db = TestDb::new("bulk_persistence");
        let config = make_config(db.path.clone());

        let items: Vec<Vec<u8>> =
            (0..500).map(|i| format!("bulk_{i}").into_bytes()).collect();

        {
            let filter = ExpiringBloomFilter::create(config).await.unwrap();
            let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();
            filter.insert_bulk(&refs).unwrap();
            filter.save_snapshot().await.unwrap();
        }

        {
            let filter =
                ExpiringBloomFilter::load(db.path.clone()).await.unwrap();
            for item in &items {
                assert!(
                    filter.contains(item).unwrap(),
                    "bulk item missing after reload: {:?}",
                    item
                );
            }
        }
    }
}
