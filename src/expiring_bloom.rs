use fnv::FnvHasher;
use murmur3::murmur3_32;
use std::hash::Hasher;
use std::io::Cursor;
use std::time::{Duration, SystemTime};
use thiserror::Error;

pub use crate::inmemory_storage::InMemoryStorage;

pub type Result<T> = std::result::Result<T, BloomError>;

#[derive(Error, Debug)]
pub enum BloomError {
    #[error("Storage operation failed: {0}")]
    StorageError(String),

    #[error("Index out of bounds: {index} >= {capacity}")]
    IndexOutOfBounds { index: usize, capacity: usize },

    #[error("Invalid level: {level} >= {max_levels}")]
    InvalidLevel { level: usize, max_levels: usize },

    #[cfg(feature = "redis")]
    #[error("Redis error: {0}")]
    RedisError(#[from] rustis::Error),

    #[cfg(feature = "redb")]
    #[error("ReDB error: {0}")]
    RedbError(#[from] redb::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

// Trait for the storage backend
pub trait BloomFilterStorage {
    /// Sets a bit at the specified level and index
    fn set_bit(&mut self, level: usize, index: usize) -> Result<()>;
    /// Gets a bit value at the specified level and index
    fn get_bit(&self, level: usize, index: usize) -> Result<bool>;
    /// Clears all bits in the specified level
    fn clear_level(&mut self, level: usize) -> Result<()>;
    /// Sets the timestamp for a level
    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()>;
    /// Gets the timestamp for a level
    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>>;
    /// Returns the number of levels in the storage
    fn num_levels(&self) -> usize;
}

/// A type alias for the hash function used in the Bloom filter.
///
/// This function takes an input item and computes multiple hash indices
/// for the Bloom filter's bit vector.
///
/// **Parameters:**
///
/// - `item: &[u8]`
///   - A byte slice representing the item to be hashed.
/// - `num_hashes: usize`
///   - The number of hash values to compute for the item.
/// - `capacity: usize`
///   - The size of the Bloom filter's bit vector. This ensures that
///     the generated hash indices are within valid bounds.
///
/// **Returns:**
///
/// - `Vec<u32>`
///   - A vector of hash indices corresponding to positions in the bit vector.
///
/// **Usage:**
///
/// The hash function computes `num_hashes` hash indices for the given `item`,
/// ensuring each index is within the range `[0, capacity)`. These indices are
/// used to set or check bits in the Bloom filter's bit vector.
pub type HashFunction = fn(&[u8], usize, usize) -> Vec<u32>;

fn hash_murmur32(key: &[u8]) -> u32 {
    let mut cursor = Cursor::new(key);
    murmur3_32(&mut cursor, 0).expect("Failed to compute Murmur3 hash")
}

fn hash_fnv32(key: &[u8]) -> u32 {
    let mut hasher = FnvHasher::default();
    hasher.write(key);
    hasher.finish() as u32
}

pub fn default_hash_function(
    item: &[u8],
    num_hashes: usize,
    capacity: usize,
) -> Vec<u32> {
    let h1 = hash_murmur32(item);
    let h2 = hash_fnv32(item);
    (0..num_hashes)
        .map(|i| h1.wrapping_add((i as u32).wrapping_mul(h2)) % capacity as u32)
        .collect()
}

fn optimal_bit_vector_size(n: usize, fpr: f64) -> usize {
    let ln2 = std::f64::consts::LN_2;
    ((-(n as f64) * fpr.ln()) / (ln2 * ln2)).ceil() as usize
}

fn optimal_num_hashes(n: usize, m: usize) -> usize {
    ((m as f64 / n as f64) * std::f64::consts::LN_2).round() as usize
}

pub struct SlidingBloomFilter<S: BloomFilterStorage> {
    storage: S,
    hash_function: HashFunction,
    capacity: usize,
    num_hashes: usize,
    false_positive_rate: f64,
    level_time: Duration,
    max_levels: usize,
    current_level_index: usize,
}

impl<S: BloomFilterStorage> SlidingBloomFilter<S> {
    pub fn new(
        storage: S,
        capacity: usize,
        false_positive_rate: f64,
        level_time: Duration,
        max_levels: usize,
        hash_function: HashFunction,
    ) -> Result<Self> {
        let bit_vector_size =
            optimal_bit_vector_size(capacity, false_positive_rate);
        let num_hashes = optimal_num_hashes(capacity, bit_vector_size);

        Ok(Self {
            storage, // Use provided storage instead of creating new
            hash_function,
            capacity,
            num_hashes,
            false_positive_rate,
            level_time,
            max_levels,
            current_level_index: 0,
        })
    }

    pub fn cleanup_expired_levels(&mut self) -> Result<()> {
        let now = SystemTime::now();
        for level in 0..self.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                if now
                    .duration_since(timestamp)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    >= self.level_time * self.max_levels as u32
                {
                    self.storage.clear_level(level)?;
                }
            }
        }
        Ok(())
    }

    fn should_create_new_level(&self) -> Result<bool> {
        let current_level = self.current_level_index;
        if let Some(last_timestamp) = self.storage.get_timestamp(current_level)? {
            let now = SystemTime::now();
            Ok(now
                .duration_since(last_timestamp)
                .map_err(|e| BloomError::StorageError(e.to_string()))?
                >= self.level_time)
        } else {
            Ok(true)
        }
    }

    fn create_new_level(&mut self) -> Result<()> {
        // Advance current level index in a circular manner
        self.current_level_index =
            (self.current_level_index + 1) % self.max_levels;
        // Clear the level at the new current level index
        self.storage.clear_level(self.current_level_index)?;
        // Set the timestamp
        self.storage
            .set_timestamp(self.current_level_index, SystemTime::now())?;
        Ok(())
    }

    pub fn insert(&mut self, item: &[u8]) -> Result<()> {
        if self.should_create_new_level()? {
            self.create_new_level()?;
        }
        let current_level = self.current_level_index;
        let hashes = (self.hash_function)(item, self.num_hashes, self.capacity);
        for &hash in &hashes {
            self.storage.set_bit(current_level, hash as usize)?;
        }
        Ok(())
    }

    pub fn query(&self, item: &[u8]) -> Result<bool> {
        let hashes = (self.hash_function)(item, self.num_hashes, self.capacity);
        let now = SystemTime::now();

        for level in 0..self.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                let elapsed = now
                    .duration_since(timestamp)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?;

                if elapsed <= self.level_time * self.max_levels as u32 {
                    let all_bits_set = hashes.iter().try_fold(
                        true,
                        |acc, &hash| -> Result<bool> {
                            Ok(acc
                                && self.storage.get_bit(level, hash as usize)?)
                        },
                    )?;

                    if all_bits_set {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

impl<B: BloomFilterStorage> std::fmt::Debug for SlidingBloomFilter<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SlidingBloomFilter {{ capacity: {}, num_hashes: {}, false_positive_rate: {}, level_time: {:?}, max_levels: {} }}",
            self.capacity,
            self.num_hashes,
            self.false_positive_rate,
            self.level_time,
            self.max_levels
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inmemory_storage::InMemoryStorage;
    use rand::Rng;
    use std::thread;

    #[test]
    fn test_workflow() {
        let hash_function = |item: &[u8],
                             num_hashes_var: usize,
                             capacity_var: usize|
         -> Vec<u32> {
            let h1 = hash_murmur32(item);
            let h2 = hash_fnv32(item);
            (0..num_hashes_var)
                .map(|i| {
                    h1.wrapping_add((i as u32).wrapping_mul(h2))
                        % capacity_var as u32
                })
                .collect()
        };
        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::new(
            storage,
            1000,
            0.01,
            Duration::from_secs(10),
            5,
            hash_function,
        )
        .unwrap();

        bloom_filter.insert(b"some data").unwrap();
        bloom_filter.insert(b"another data").unwrap();
        assert!(bloom_filter.query(b"some data").unwrap());
        assert!(bloom_filter.query(b"another data").unwrap());
        assert!(!bloom_filter.query(b"some").unwrap());
        assert!(!bloom_filter.query(b"another").unwrap());
    }

    #[test]
    fn test_expiration_of_elements() {
        let storage = InMemoryStorage::new(100, 2).unwrap();
        let mut bloom_filter = SlidingBloomFilter::new(
            storage,
            100,
            0.01,
            Duration::from_secs(1),
            2,
            default_hash_function,
        )
        .unwrap();

        bloom_filter.insert(b"item1").unwrap();
        assert!(bloom_filter.query(b"item1").unwrap());

        // Wait enough time for the item to expire
        thread::sleep(Duration::from_secs(5)); // Exceeds MAX_LEVELS * LEVEL_TIME

        // Call cleanup explicitly
        bloom_filter.cleanup_expired_levels().unwrap();

        assert!(!bloom_filter.query(b"item1").unwrap());
    }

    #[test]
    fn test_no_false_negatives_within_decay_time() {
        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            1000,
            0.01,
            Duration::from_secs(2),
            5,
            default_hash_function,
        )
        .unwrap();

        let items: Vec<&[u8]> =
            vec![b"apple", b"banana", b"cherry", b"date", b"elderberry"];

        for item in &items {
            bloom_filter.insert(item).unwrap();
        }

        // Query immediately
        for item in &items {
            assert!(bloom_filter.query(item).unwrap());
        }

        // Wait less than total decay time
        thread::sleep(Duration::from_secs(5)); // Less than MAX_LEVELS * LEVEL_TIME
        bloom_filter.cleanup_expired_levels().unwrap();

        for item in &items {
            assert!(bloom_filter.query(item).unwrap());
        }
    }

    #[test]
    fn test_items_expire_after_decay_time() {
        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            100,
            0.01,
            Duration::from_secs(1),
            3,
            default_hash_function,
        )
        .unwrap();

        bloom_filter.insert(b"item_to_expire").unwrap();
        assert!(bloom_filter.query(b"item_to_expire").unwrap());

        // Wait for the item to expire
        thread::sleep(Duration::from_secs(4)); // Exceeds MAX_LEVELS * LEVEL_TIME
        bloom_filter.cleanup_expired_levels().unwrap();

        assert!(!bloom_filter.query(b"item_to_expire").unwrap());
    }

    #[test]
    fn test_immediate_expiration() {
        let storage = InMemoryStorage::new(100, 3).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            100,
            0.01,
            Duration::from_secs(1),
            3,
            default_hash_function,
        )
        .unwrap();

        bloom_filter.insert(b"test_item").unwrap();
        assert!(bloom_filter.query(b"test_item").unwrap());

        // Wait for total decay time
        thread::sleep(Duration::from_secs(4));
        bloom_filter.cleanup_expired_levels().unwrap();
        assert!(
            !bloom_filter.query(b"test_item").unwrap(),
            "Item should have expired after total decay time"
        );
    }

    #[test]
    fn test_partial_expiration() {
        let storage = InMemoryStorage::new(100, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            100,
            0.01,
            Duration::from_secs(1),
            5,
            default_hash_function,
        )
        .unwrap();

        // Insert old items
        for i in 0..5 {
            let item = format!("old_item_{}", i);
            bloom_filter.insert(item.as_bytes()).unwrap();
            thread::sleep(Duration::from_millis(200));
        }

        // Wait so that old items surpass the decay time
        thread::sleep(Duration::from_secs(6));

        // Insert new items
        for i in 0..5 {
            let item = format!("new_item_{}", i);
            bloom_filter.insert(item.as_bytes()).unwrap();
        }

        bloom_filter.cleanup_expired_levels().unwrap();

        // Old items should have expired
        for i in 0..5 {
            let item = format!("old_item_{}", i);
            assert!(
                !bloom_filter.query(item.as_bytes()).unwrap(),
                "Old item {} should have expired",
                item
            );
        }

        // New items should still be present
        for i in 0..5 {
            let item = format!("new_item_{}", i);
            assert!(
                bloom_filter.query(item.as_bytes()).unwrap(),
                "New item {} should still be present",
                item
            );
        }
    }

    #[test]
    fn test_continuous_insertion_and_query() {
        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            1000,
            0.01,
            Duration::from_secs(1),
            5,
            default_hash_function,
        )
        .unwrap();

        // This loop with end in 1second
        let inserts_time = SystemTime::now();
        for i in 0..10 {
            let item = format!("item_{}", i);
            bloom_filter.insert(item.as_bytes()).unwrap();
            assert!(bloom_filter.query(item.as_bytes()).unwrap());
            // 0.5s so 2 elements should to to the each level
            // and total time passed - 5 seconds
            thread::sleep(Duration::from_millis(500))
        }

        // Ensure inserts executed within 5-6 seconds
        let inserts_duration =
            SystemTime::now().duration_since(inserts_time).unwrap();
        assert!(
            inserts_duration >= Duration::from_secs(5),
            "Should take at least 5 secs"
        );
        assert!(
            inserts_duration < Duration::from_secs(6),
            "Should take less than 6 secs"
        );

        // Should pass 5 seconds and have 5 levels!
        assert_eq!(
            bloom_filter.storage.levels.len(),
            5,
            "After 5 seconds there is should be 5 levels of filter"
        );

        for i in 0..bloom_filter.storage.num_levels() {
            assert!(
                !bloom_filter.storage.levels[i].is_empty(),
                "Each level should contain at least 1 elements"
            );
        }

        // All above should take little bit more than 5 seconds
        // items will start expire after 5 seconds, so wait 3 seconds more.

        // Wait for earlier items to expire
        thread::sleep(Duration::from_secs(3));
        bloom_filter.cleanup_expired_levels().unwrap();

        // Items 0 to 6 should have expired
        for i in 0..8 {
            let item = format!("item_{}", i);
            assert!(
                !bloom_filter.query(item.as_bytes()).unwrap(),
                "Item {} should have expired",
                item
            );
        }

        // Items 8 to 9 should still be present
        for i in 8..10 {
            let item = format!("item_{}", i);
            assert!(
                bloom_filter.query(item.as_bytes()).unwrap(),
                "Item {} should still be present",
                item
            );
        }
    }

    #[test]
    fn test_false_positive_rate() {
        const FALSE_POSITIVE_RATE: f64 = 0.05;

        let storage = InMemoryStorage::new(10000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            10000,
            FALSE_POSITIVE_RATE,
            Duration::from_secs(2),
            5,
            default_hash_function,
        )
        .unwrap();

        let num_items = 1000;
        let mut rng = rand::thread_rng();
        let mut inserted_items = Vec::new();

        // Insert random items
        for _ in 0..num_items {
            let item: Vec<u8> = (0..10).map(|_| rng.gen()).collect();
            bloom_filter.insert(&item).unwrap();
            inserted_items.push(item);
        }

        // Test for false positives
        let mut false_positives = 0;
        let num_tests = 1000;

        bloom_filter.cleanup_expired_levels().unwrap();

        for _ in 0..num_tests {
            let item: Vec<u8> = (0..10).map(|_| rng.gen()).collect();
            if bloom_filter.query(&item).unwrap() {
                // Check if the item was actually inserted
                if !inserted_items.contains(&item) {
                    false_positives += 1;
                }
            }
        }

        let observed_fpr = false_positives as f64 / num_tests as f64;
        assert!(
            observed_fpr <= FALSE_POSITIVE_RATE * 1.5,
            "False positive rate is too high: observed {}, expected {}",
            observed_fpr,
            FALSE_POSITIVE_RATE
        );
    }

    #[test]
    fn test_concurrent_inserts() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let bloom_filter = Arc::new(Mutex::new(
            SlidingBloomFilter::<InMemoryStorage>::new(
                storage,
                1000,
                0.01,
                Duration::from_secs(1),
                5,
                default_hash_function,
            )
            .unwrap(),
        ));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let bloom_filter = Arc::clone(&bloom_filter);
                thread::spawn(move || {
                    let item = format!("concurrent_item_{}", i);
                    let mut bf = bloom_filter.lock().unwrap();
                    bf.insert(item.as_bytes()).unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        bloom_filter
            .lock()
            .unwrap()
            .cleanup_expired_levels()
            .unwrap();

        // Verify that all items have been inserted
        for i in 0..10 {
            let item = format!("concurrent_item_{}", i);
            let bf = bloom_filter.lock().unwrap();
            assert!(bf.query(item.as_bytes()).unwrap());
        }
    }

    #[test]
    fn test_full_capacity() {
        const FALSE_POSITIVE_RATE: f64 = 0.1;

        let storage = InMemoryStorage::new(100, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            100,
            FALSE_POSITIVE_RATE,
            Duration::from_secs(1),
            5,
            default_hash_function,
        )
        .unwrap();

        // Insert more items than capacity to test behavior
        for i in 0..200 {
            let item = format!("item_{}", i);
            bloom_filter.insert(item.as_bytes()).unwrap();

            bloom_filter.cleanup_expired_levels().unwrap();
            assert!(bloom_filter.query(item.as_bytes()).unwrap());
        }

        bloom_filter.cleanup_expired_levels().unwrap();
        // Expect higher false positive rate due to saturation
        let false_queries = (200..300)
            .filter(|i| {
                let item = format!("item_{}", i);
                bloom_filter.query(item.as_bytes()).unwrap()
            })
            .count();

        let observed_fpr = false_queries as f64 / 100.0;
        assert!(
            observed_fpr >= FALSE_POSITIVE_RATE,
            "False positive rate is lower than expected: observed {}, expected {}",
            observed_fpr,
            FALSE_POSITIVE_RATE
        );
    }

    #[test]
    fn test_clear_functionality() {
        let storage = InMemoryStorage::new(1000, 5).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            1000,
            0.01,
            Duration::from_secs(1),
            5,
            default_hash_function,
        )
        .unwrap();

        bloom_filter.insert(b"persistent_item").unwrap();

        // Insert items that should expire
        bloom_filter.insert(b"temp_item").unwrap();

        bloom_filter.cleanup_expired_levels().unwrap();
        assert!(bloom_filter.query(b"temp_item").unwrap());

        // Wait for the temporary item to expire
        thread::sleep(Duration::from_secs(6)); // Exceeds MAX_LEVELS * LEVEL_TIME
        bloom_filter.cleanup_expired_levels().unwrap();

        // "temp_item" should be expired
        assert!(!bloom_filter.query(b"temp_item").unwrap());

        // "persistent_item" should be also expired
        assert!(!bloom_filter.query(b"persistent_item").unwrap());
    }

    #[test]
    fn test_should_create_new_level_edge_case() {
        const MAX_LEVELS: usize = 3;

        let storage = InMemoryStorage::new(1000, MAX_LEVELS).unwrap();
        let mut bloom_filter = SlidingBloomFilter::<InMemoryStorage>::new(
            storage,
            1000,
            0.01,
            Duration::from_millis(500),
            MAX_LEVELS,
            default_hash_function,
        )
        .unwrap();

        // Rapid insertions to test level creation
        for i in 0..10 {
            let item = format!("rapid_item_{}", i);
            bloom_filter.insert(item.as_bytes()).unwrap();
            thread::sleep(Duration::from_millis(100)); // Sleep less than LEVEL_TIME
        }

        // Levels should have been created appropriately
        assert!(bloom_filter.storage.num_levels() <= MAX_LEVELS);
    }
}
