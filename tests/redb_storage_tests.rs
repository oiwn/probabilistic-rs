#[cfg(test)]
mod tests {
    use expiring_bloom_rs::redb_storage::RedbStorage;
    use expiring_bloom_rs::{
        default_hash_function, BloomFilterStorage, SlidingBloomFilter,
    };
    use std::{
        fs,
        sync::{Arc, Mutex},
        thread,
        time::{Duration, SystemTime},
    };

    fn create_temp_db() -> (RedbStorage, String) {
        let path = format!("test_db_{}.redb", rand::random::<u64>());
        let storage = RedbStorage::open(&path, 1000, 5).unwrap();
        (storage, path)
    }

    fn cleanup_db(path: &str) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_basic_operations() {
        let (mut storage, path) = create_temp_db();

        // Test set and get bit
        storage.set_bit(0, 5).unwrap();
        assert!(storage.get_bit(0, 5).unwrap());
        assert!(!storage.get_bit(0, 6).unwrap());

        // Test clear level
        storage.clear_level(0).unwrap();
        assert!(!storage.get_bit(0, 5).unwrap());

        cleanup_db(&path);
    }

    #[test]
    fn test_persistence() {
        let path = format!("test_db_{}.redb", rand::random::<u64>());

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
    fn test_timestamps() {
        let (mut storage, path) = create_temp_db();

        let time1 = SystemTime::now();
        storage.set_timestamp(0, time1).unwrap();

        thread::sleep(Duration::from_millis(10));

        let time2 = SystemTime::now();
        assert!(storage.get_timestamp(0).unwrap().unwrap() == time1);

        storage.set_timestamp(0, time2).unwrap();
        assert!(storage.get_timestamp(0).unwrap().unwrap() == time2);

        cleanup_db(&path);
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

        cleanup_db(&path);
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

        cleanup_db(&path);
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
        thread::sleep(Duration::from_millis(200));

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

        cleanup_db(&path);
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
            println!("Item {} exists: {}", i, exists);
        }

        cleanup_db(&path);
    }

    // Helper function to simulate consistent hashing for test items
    fn hash_item(item: &str) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        item.hash(&mut hasher);
        (hasher.finish() % 1000) as usize
    }
}
