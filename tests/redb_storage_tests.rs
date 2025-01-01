#[cfg(test)]
mod tests {
    use expiring_bloom_rs::backends::BloomFilterStorage;
    use expiring_bloom_rs::redb_storage::RedbStorage;
    use std::{
        fs,
        sync::Arc,
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
        use std::sync::Mutex;

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
}
