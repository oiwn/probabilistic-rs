use crate::backends::{BloomError, BloomFilterStorage, Result};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;

const LEVELS_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("levels");

#[derive(Serialize, Deserialize)]
struct LevelData {
    bits: Vec<bool>,
    timestamp: SystemTime,
}

pub struct RedbStorage {
    db: Arc<Database>,
    capacity: usize,
    max_levels: usize,
}

impl RedbStorage {
    pub fn open(path: &str, capacity: usize, max_levels: usize) -> Result<Self> {
        // Open or create the database
        let db = Database::create(path)
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let db = Arc::new(db);

        // Initialize the database with empty levels if they don't exist
        let write_txn = db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(LEVELS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Initialize each level if it doesn't exist
            for level in 0..max_levels {
                let level_key = level.to_le_bytes();
                if table
                    .get(&level_key[..])
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    .is_none()
                {
                    let level_data = LevelData {
                        bits: vec![false; capacity],
                        timestamp: SystemTime::now(),
                    };
                    let serialized =
                        bincode::serialize(&level_data).map_err(|e| {
                            BloomError::SerializationError(e.to_string())
                        })?;
                    table
                        .insert(&level_key[..], &serialized[..])
                        .map_err(|e| BloomError::StorageError(e.to_string()))?;
                }
            }
        }
        write_txn
            .commit()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        Ok(Self {
            db,
            capacity,
            max_levels,
        })
    }

    fn get_level_data(&self, level: usize) -> Result<LevelData> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let table = read_txn
            .open_table(LEVELS_TABLE)
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let level_key = level.to_le_bytes();
        let data = table
            .get(&level_key[..])
            .map_err(|e| BloomError::StorageError(e.to_string()))?
            .ok_or_else(|| BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            })?;

        bincode::deserialize(data.value())
            .map_err(|e| BloomError::SerializationError(e.to_string()))
    }

    fn save_level_data(&self, level: usize, data: &LevelData) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(LEVELS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
            let level_key = level.to_le_bytes();
            let serialized = bincode::serialize(data)
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;
            table
                .insert(&level_key[..], &serialized[..])
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        Ok(())
    }
}

impl BloomFilterStorage for RedbStorage {
    fn new(_capacity: usize, _max_levels: usize) -> Result<Self> {
        Err(BloomError::StorageError(
            "Use RedbStorage::open() instead".to_string(),
        ))
    }

    fn set_bit(&mut self, level: usize, index: usize) -> Result<()> {
        if index >= self.capacity {
            return Err(BloomError::IndexOutOfBounds {
                index,
                capacity: self.capacity,
            });
        }
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let mut level_data = self.get_level_data(level)?;
        level_data.bits[index] = true;
        self.save_level_data(level, &level_data)
    }

    fn get_bit(&self, level: usize, index: usize) -> Result<bool> {
        if index >= self.capacity {
            return Err(BloomError::IndexOutOfBounds {
                index,
                capacity: self.capacity,
            });
        }
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let level_data = self.get_level_data(level)?;
        Ok(level_data.bits[index])
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let mut level_data = self.get_level_data(level)?;
        level_data.bits = vec![false; self.capacity];
        level_data.timestamp = SystemTime::now();
        self.save_level_data(level, &level_data)
    }

    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let mut level_data = self.get_level_data(level)?;
        level_data.timestamp = timestamp;
        self.save_level_data(level, &level_data)
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let level_data = self.get_level_data(level)?;
        Ok(Some(level_data.timestamp))
    }

    fn num_levels(&self) -> usize {
        self.max_levels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;

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
