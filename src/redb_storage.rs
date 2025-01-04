use crate::expiring_bloom::{
    default_hash_function, BloomError, BloomFilterStorage, Result,
    SlidingBloomFilter,
};
use derive_builder::Builder;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// Key: u8 (just level), Value: Vec<u8> (bit array)
const BITS_TABLE: TableDefinition<u8, &[u8]> = TableDefinition::new("bits");

// Table for storing timestamps per level
const TIMESTAMPS_TABLE: TableDefinition<u8, &[u8]> =
    TableDefinition::new("timestamps");

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
        let storage =
            RedbStorage::open(&opts.path, opts.capacity, opts.max_levels)?;

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
    pub fn open(
        path: &PathBuf,
        capacity: usize,
        max_levels: usize,
    ) -> Result<Self> {
        if max_levels > 255 {
            return Err(BloomError::StorageError(
                "Max levels cannot exceed 255".to_string(),
            ));
        }

        let db = Database::create(path)
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let db = Arc::new(db);

        // Initialize bit arrays and timestamps for each level
        let write_txn = db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            // Initialize bit arrays
            let mut bits_table = write_txn
                .open_table(BITS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Calculate bytes needed for capacity
            let bytes_needed = (capacity + 7) / 8; // Round up division
            let empty_bits = vec![0u8; bytes_needed];

            // Initialize timestamps
            let mut timestamps_table = write_txn
                .open_table(TIMESTAMPS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
            let duration_bytes = bincode::serialize(&now)
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;

            // Initialize each level
            for level in 0..max_levels as u8 {
                // Initialize bits if not exists
                if bits_table
                    .get(&level)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    .is_none()
                {
                    bits_table
                        .insert(&level, empty_bits.as_slice())
                        .map_err(|e| BloomError::StorageError(e.to_string()))?;
                }

                // Initialize timestamp if not exists
                if timestamps_table
                    .get(&level)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    .is_none()
                {
                    timestamps_table
                        .insert(&level, duration_bytes.as_slice())
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

    // Helper function to calculate byte and bit position
    #[inline]
    fn get_byte_and_bit_pos(index: usize) -> (usize, u8) {
        let byte_pos = index / 8;
        let bit_pos = (index % 8) as u8;
        (byte_pos, bit_pos)
    }

    // Helper to set a bit in a byte array
    #[inline]
    fn set_bit_in_array(bits: &mut [u8], index: usize) {
        let (byte_pos, bit_pos) = Self::get_byte_and_bit_pos(index);
        bits[byte_pos] |= 1 << bit_pos;
    }

    // Helper to get a bit from a byte array
    #[inline]
    fn get_bit_from_array(bits: &[u8], index: usize) -> bool {
        let (byte_pos, bit_pos) = Self::get_byte_and_bit_pos(index);
        (bits[byte_pos] & (1 << bit_pos)) != 0
    }
}

impl BloomFilterStorage for RedbStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        // Check indices
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(BITS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Get current bits in a new scope
            let bits = {
                let current_bits = table
                    .get(&(level as u8))
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    .ok_or_else(|| {
                        BloomError::StorageError(
                            "Bit array not initialized".to_string(),
                        )
                    })?;

                // Create mutable copy of the bits
                let mut bits = current_bits.value().to_vec();

                // Set all required bits
                for &index in indices {
                    Self::set_bit_in_array(&mut bits, index);
                }

                bits
            }; // AccessGuard is dropped here

            // Now we can insert the modified bits
            table
                .insert(&(level as u8), bits.as_slice())
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        Ok(())
    }

    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        // Check indices
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let table = read_txn
            .open_table(BITS_TABLE)
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        let bits = table
            .get(&(level as u8))
            .map_err(|e| BloomError::StorageError(e.to_string()))?
            .ok_or_else(|| {
                BloomError::StorageError("Bit array not initialized".to_string())
            })?;

        // Get all requested bits
        Ok(indices
            .iter()
            .map(|&index| Self::get_bit_from_array(bits.value(), index))
            .collect())
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(BITS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Create empty bit array
            let bytes_needed = (self.capacity + 7) / 8;
            let empty_bits = vec![0u8; bytes_needed];

            // Reset level to empty bits
            table
                .insert(&(level as u8), empty_bits.as_slice())
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        Ok(())
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

        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(TIMESTAMPS_TABLE)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Get duration since UNIX_EPOCH
            let duration = timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;

            // Store full duration bytes
            let duration_bytes = bincode::serialize(&duration)
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;

            table
                .insert(&(level as u8), duration_bytes.as_slice())
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        if level >= self.max_levels {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        }

        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| BloomError::StorageError(e.to_string()))?;
        let table = read_txn
            .open_table(TIMESTAMPS_TABLE)
            .map_err(|e| BloomError::StorageError(e.to_string()))?;

        if let Some(bytes) = table
            .get(&(level as u8))
            .map_err(|e| BloomError::StorageError(e.to_string()))?
        {
            // Deserialize duration and convert back to SystemTime
            let duration: Duration = bincode::deserialize(bytes.value())
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;

            Ok(Some(SystemTime::UNIX_EPOCH + duration))
        } else {
            Ok(None)
        }
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
