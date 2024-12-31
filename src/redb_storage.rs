use crate::backends::BloomFilterStorage;
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[derive(Serialize, Deserialize)]
struct LevelData {
    bits: Vec<bool>,
    timestamp: SystemTime,
}

pub struct RedbStorage {
    db: Arc<Database>,
    levels_table: TableDefinition<'static, u64, Vec<u8>>, // u64 key for level index, Vec<u8> for serialized LevelData
    capacity: usize,
    max_levels: usize,
}

impl RedbStorage {
    pub fn open(path: &str, capacity: usize, max_levels: usize) -> Self {
        // Open or create the database at the specified path
        let db = Database::create(path).unwrap();
        let levels_table = TableDefinition::new("levels");

        // Initialize levels if they don't exist
        {
            let write_txn = db.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(levels_table).unwrap();
                for level in 0..max_levels {
                    if table.get(level as u64).unwrap().is_none() {
                        let level_data = LevelData {
                            bits: vec![false; capacity],
                            timestamp: SystemTime::now(),
                        };
                        let serialized = bincode::serialize(&level_data).unwrap();
                        table.insert(level as u64, &serialized).unwrap();
                    }
                }
            }
            write_txn.commit().unwrap();
        }

        Self {
            db: Arc::new(db),
            levels_table,
            capacity,
            max_levels,
        }
    }
}

impl BloomFilterStorage for RedbStorage {
    fn new(_capacity: usize, _max_levels: usize) -> Self {
        panic!("Use RedbStorage::open(path, capacity, max_levels) instead");
    }

    fn set_bit(&mut self, level: usize, index: usize) {
        let db = Arc::clone(&self.db);
        let levels_table = self.levels_table.clone();
        let capacity = self.capacity;

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(levels_table).unwrap();
            let level_data_bytes = table.get(level as u64).unwrap().unwrap();
            let mut level_data: LevelData =
                bincode::deserialize(&level_data_bytes.value()).unwrap();

            if index >= capacity {
                panic!("Index out of bounds");
            }

            level_data.bits[index] = true;

            let serialized = bincode::serialize(&level_data).unwrap();
            table.insert(level as u64, &serialized).unwrap();
        }
        write_txn.commit().unwrap();
    }

    fn get_bit(&self, level: usize, index: usize) -> bool {
        let db = Arc::clone(&self.db);
        let levels_table = self.levels_table.clone();

        let read_txn = db.begin_read().unwrap();
        let table = read_txn.open_table(levels_table).unwrap();
        let level_data_bytes = table.get(level as u64).unwrap().unwrap();
        let level_data: LevelData =
            bincode::deserialize(&level_data_bytes.value()).unwrap();

        level_data.bits[index]
    }

    fn clear_level(&mut self, level: usize) {
        let db = Arc::clone(&self.db);
        let levels_table = self.levels_table.clone();
        let capacity = self.capacity;

        let mut write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(levels_table).unwrap();
            let mut level_data = LevelData {
                bits: vec![false; capacity],
                timestamp: SystemTime::now(),
            };
            let serialized = bincode::serialize(&level_data).unwrap();
            table.insert(level as u64, &serialized).unwrap();
        }
        write_txn.commit().unwrap();
    }

    fn set_timestamp(&mut self, level: usize, timestamp: SystemTime) {
        let db = Arc::clone(&self.db);
        let levels_table = self.levels_table.clone();

        let mut write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(levels_table).unwrap();
            let level_data_bytes = table.get(level as u64).unwrap().unwrap();
            let mut level_data: LevelData =
                bincode::deserialize(&level_data_bytes.value()).unwrap();

            level_data.timestamp = timestamp;

            let serialized = bincode::serialize(&level_data).unwrap();
            table.insert(level as u64, &serialized).unwrap();
        }
        write_txn.commit().unwrap();
    }

    fn get_timestamp(&self, level: usize) -> Option<SystemTime> {
        let db = Arc::clone(&self.db);
        let levels_table = self.levels_table.clone();

        let read_txn = db.begin_read().unwrap();
        let table = read_txn.open_table(&levels_table).unwrap();
        let level_data_bytes = table.get(&level as u64).unwrap().unwrap();
        let level_data: LevelData =
            bincode::deserialize(level_data_bytes.value()).unwrap();

        Some(level_data.timestamp)
    }

    fn num_levels(&self) -> usize {
        self.max_levels
    }
}
