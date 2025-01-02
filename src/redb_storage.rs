use crate::expiring_bloom::{
    default_hash_function, BloomError, BloomFilterStorage, Result,
    SlidingBloomFilter,
};
use derive_builder::Builder;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

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

#[derive(Builder, Debug)]
#[builder(pattern = "owned")]
pub struct RedbExpiringBloomFilterOptions {
    /// Path to the ReDB database file
    path: PathBuf,
    /// Maximum number of elements the filter is expected to contain
    capacity: usize,
    /// How long elements should stay in the filter
    expiration_time: Duration,
    /// False positive rate (default: 0.01)
    #[builder(default = "0.01")]
    false_positive_rate: f64,
    /// Number of filter levels (default: 5)
    #[builder(default = "5")]
    max_levels: usize,
}

pub struct RedbExpiringBloomFilter {
    filter: SlidingBloomFilter<RedbStorage>,
}

impl RedbExpiringBloomFilter {
    /// Creates a new RedbExpiringBloomFilter from the provided options
    pub fn new(opts: RedbExpiringBloomFilterOptions) -> Result<Self> {
        // Calculate level duration based on expiration time and max levels
        let level_duration = Duration::from_secs(
            opts.expiration_time.as_secs() / opts.max_levels as u64,
        );

        // Create ReDB storage
        let storage = RedbStorage::open(
            opts.path.to_str().ok_or_else(|| {
                BloomError::StorageError("Invalid path".to_string())
            })?,
            opts.capacity,
            opts.max_levels,
        )?;

        // Create the sliding bloom filter
        let filter = SlidingBloomFilter::new(
            storage,
            opts.capacity,
            opts.false_positive_rate,
            level_duration,
            opts.max_levels,
            default_hash_function,
        )?;

        Ok(Self { filter })
    }

    /// Insert an item into the filter
    pub fn insert(&mut self, item: &[u8]) -> Result<()> {
        self.filter.insert(item)
    }

    /// Query if an item might be in the filter
    pub fn query(&self, item: &[u8]) -> Result<bool> {
        self.filter.query(item)
    }

    /// Clean up expired items from the filter
    pub fn cleanup_expired(&mut self) -> Result<()> {
        self.filter.cleanup_expired_levels()
    }
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

    #[inline]
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

    #[test]
    fn test_builder_required_fields() {
        // Test builder with only required fields
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .build();

        assert!(result.is_ok());
        let opts = result.unwrap();
        assert_eq!(opts.false_positive_rate, 0.01); // Check default value
        assert_eq!(opts.max_levels, 5); // Check default value
    }

    #[test]
    fn test_builder_custom_fields() {
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .false_positive_rate(0.001)
            .max_levels(10)
            .build();

        assert!(result.is_ok());
        let opts = result.unwrap();
        assert_eq!(opts.false_positive_rate, 0.001);
        assert_eq!(opts.max_levels, 10);
    }

    #[test]
    fn test_builder_missing_required() {
        // Test missing path
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .build();
        assert!(result.is_err());

        // Test missing capacity
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .expiration_time(Duration::from_secs(3600))
            .build();
        assert!(result.is_err());
    }
}
