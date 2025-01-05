use crate::error::{BloomError, Result};
use crate::filter::{FilterConfig, SlidingBloomFilter};
use crate::hash::{optimal_bit_vector_size, optimal_num_hashes};
use crate::storage::{BloomStorage, InMemoryStorage};
use std::time::SystemTime;

// Base filter implementation
pub struct InMemorySlidingBloomFilter {
    storage: InMemoryStorage,
    config: FilterConfig,
    num_hashes: usize,
    current_level_index: usize,
}

impl InMemorySlidingBloomFilter {
    pub fn new(config: FilterConfig) -> Result<Self> {
        let storage = InMemoryStorage::new(config.capacity, config.max_levels)?;
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);

        Ok(Self {
            storage,
            config,
            num_hashes,
            current_level_index: 0,
        })
    }

    pub fn should_create_new_level(&self) -> Result<bool> {
        let current_level = self.current_level_index;
        if let Some(last_timestamp) = self.storage.get_timestamp(current_level)? {
            let now = SystemTime::now();
            Ok(now
                .duration_since(last_timestamp)
                .map_err(|e| BloomError::StorageError(e.to_string()))?
                >= self.config.level_duration)
        } else {
            Ok(true)
        }
    }

    pub fn create_new_level(&mut self) -> Result<()> {
        // Advance current level index in a circular manner
        self.current_level_index =
            (self.current_level_index + 1) % self.config.max_levels;
        // Clear the level at the new current level index
        self.storage.clear_level(self.current_level_index)?;
        // Set the timestamp
        self.storage
            .set_timestamp(self.current_level_index, SystemTime::now())?;
        Ok(())
    }
}

impl SlidingBloomFilter for InMemorySlidingBloomFilter {
    fn insert(&mut self, item: &[u8]) -> Result<()> {
        if self.should_create_new_level()? {
            self.create_new_level()?;
        }

        // Get all hash indices at once
        let indices: Vec<usize> = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        // Set all bits in one operation
        self.storage.set_bits(self.current_level_index, &indices)
    }

    fn query(&self, item: &[u8]) -> Result<bool> {
        let indices: Vec<usize> = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        let now = SystemTime::now();

        for level in 0..self.config.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                let elapsed = now
                    .duration_since(timestamp)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?;

                if elapsed
                    <= self.config.level_duration * self.config.max_levels as u32
                {
                    // Check all bits in one operation
                    let bits = self.storage.get_bits(level, &indices)?;
                    if bits.iter().all(|&bit| bit) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn cleanup_expired_levels(&mut self) -> Result<()> {
        let now = SystemTime::now();
        for level in 0..self.config.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                if now
                    .duration_since(timestamp)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?
                    >= self.config.level_duration * self.config.max_levels as u32
                {
                    self.storage.clear_level(level)?;
                }
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for InMemorySlidingBloomFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "InMemorySlidingBloomFilter {{ capacity: {}, num_hashes: {}, false_positive_rate: {}, level_time: {:?}, max_levels: {} }}",
            self.config.capacity,
            self.num_hashes,
            self.config.false_positive_rate,
            self.config.level_duration,
            self.config.max_levels
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterConfigBuilder, SlidingBloomFilter};
    use crate::hash::{hash_fnv32, hash_murmur32};
    use rand::Rng;
    use std::thread;
    use std::time::Duration;

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

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(10))
            .max_levels(5)
            .hash_function(hash_function)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        filter.insert(b"some data").unwrap();
        filter.insert(b"another data").unwrap();
        assert!(filter.query(b"some data").unwrap());
        assert!(filter.query(b"another data").unwrap());
        assert!(!filter.query(b"some").unwrap());
        assert!(!filter.query(b"another").unwrap());
    }

    #[test]
    fn test_expiration_of_elements() {
        let config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_millis(500))
            .max_levels(3)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        filter.insert(b"item1").unwrap();
        assert!(filter.query(b"item1").unwrap());

        // Wait enough time for the item to expire
        thread::sleep(Duration::from_secs(3)); // Exceeds MAX_LEVELS * LEVEL_TIME

        // Call cleanup explicitly
        filter.cleanup_expired_levels().unwrap();

        assert!(!filter.query(b"item1").unwrap());
    }

    #[test]
    fn test_no_false_negatives_within_decay_time() {
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(2))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        let items: Vec<&[u8]> =
            vec![b"apple", b"banana", b"cherry", b"date", b"elderberry"];

        for item in &items {
            filter.insert(item).unwrap();
        }

        // Query immediately
        for item in &items {
            assert!(filter.query(item).unwrap());
        }

        // Wait less than total decay time
        thread::sleep(Duration::from_secs(5)); // Less than MAX_LEVELS * LEVEL_TIME
        filter.cleanup_expired_levels().unwrap();

        for item in &items {
            assert!(filter.query(item).unwrap());
        }
    }

    #[test]
    fn test_items_expire_after_decay_time() {
        let config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        filter.insert(b"item_to_expire").unwrap();
        assert!(filter.query(b"item_to_expire").unwrap());

        // Wait for the item to expire
        thread::sleep(Duration::from_secs(4)); // Exceeds MAX_LEVELS * LEVEL_TIME
        filter.cleanup_expired_levels().unwrap();

        assert!(!filter.query(b"item_to_expire").unwrap());
    }

    #[test]
    fn test_immediate_expiration() {
        let config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        filter.insert(b"test_item").unwrap();
        assert!(filter.query(b"test_item").unwrap());

        // Wait for total decay time
        thread::sleep(Duration::from_secs(4));
        filter.cleanup_expired_levels().unwrap();
        assert!(
            !filter.query(b"test_item").unwrap(),
            "Item should have expired after total decay time"
        );
    }

    #[test]
    fn test_partial_expiration() {
        let config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        // Insert old items
        for i in 0..5 {
            let item = format!("old_item_{}", i);
            filter.insert(item.as_bytes()).unwrap();
            thread::sleep(Duration::from_millis(200));
        }

        // Wait so that old items surpass the decay time
        thread::sleep(Duration::from_secs(6));

        // Insert new items
        for i in 0..5 {
            let item = format!("new_item_{}", i);
            filter.insert(item.as_bytes()).unwrap();
        }

        filter.cleanup_expired_levels().unwrap();

        // Old items should have expired
        for i in 0..5 {
            let item = format!("old_item_{}", i);
            assert!(
                !filter.query(item.as_bytes()).unwrap(),
                "Old item {} should have expired",
                item
            );
        }

        // New items should still be present
        for i in 0..5 {
            let item = format!("new_item_{}", i);
            assert!(
                filter.query(item.as_bytes()).unwrap(),
                "New item {} should still be present",
                item
            );
        }
    }

    #[test]
    fn test_continuous_insertion_and_query() {
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        // This loop with end in 1second
        let inserts_time = SystemTime::now();
        for i in 0..10 {
            let item = format!("item_{}", i);
            filter.insert(item.as_bytes()).unwrap();
            assert!(filter.query(item.as_bytes()).unwrap());
            // 0.5s so 2 elements should go to the each level
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
            filter.storage.levels.len(),
            5,
            "After 5 seconds there is should be 5 levels of filter"
        );

        for i in 0..filter.storage.num_levels() {
            assert!(
                !filter.storage.levels[i].is_empty(),
                "Each level should contain at least 1 elements"
            );
        }

        // All above should take little bit more than 5 seconds
        // items will start expire after 5 seconds, so wait 3 seconds more.

        // Wait for earlier items to expire
        thread::sleep(Duration::from_secs(3));
        filter.cleanup_expired_levels().unwrap();

        // Items 0 to 6 should have expired
        for i in 0..8 {
            let item = format!("item_{}", i);
            assert!(
                !filter.query(item.as_bytes()).unwrap(),
                "Item {} should have expired",
                item
            );
        }

        // Items 8 to 9 should still be present
        for i in 8..10 {
            let item = format!("item_{}", i);
            assert!(
                filter.query(item.as_bytes()).unwrap(),
                "Item {} should still be present",
                item
            );
        }
    }

    #[test]
    fn test_false_positive_rate() {
        const FALSE_POSITIVE_RATE: f64 = 0.05;
        let config = FilterConfigBuilder::default()
            .capacity(10000)
            .false_positive_rate(FALSE_POSITIVE_RATE)
            .level_duration(Duration::from_secs(2))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        let num_items = 1000;
        let mut rng = rand::thread_rng();
        let mut inserted_items = Vec::new();

        // Insert random items
        for _ in 0..num_items {
            let item: Vec<u8> = (0..10).map(|_| rng.gen()).collect();
            filter.insert(&item).unwrap();
            inserted_items.push(item);
        }

        // Test for false positives
        let mut false_positives = 0;
        let num_tests = 1000;

        filter.cleanup_expired_levels().unwrap();

        for _ in 0..num_tests {
            let item: Vec<u8> = (0..10).map(|_| rng.gen()).collect();
            if filter.query(&item).unwrap() {
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

        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let filter = Arc::new(Mutex::new(
            InMemorySlidingBloomFilter::new(config)
                .expect("Failed to create InMemorySlidingBloomFilter"),
        ));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let bloom_filter = Arc::clone(&filter);
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

        filter.lock().unwrap().cleanup_expired_levels().unwrap();

        // Verify that all items have been inserted
        for i in 0..10 {
            let item = format!("concurrent_item_{}", i);
            let bf = filter.lock().unwrap();
            assert!(bf.query(item.as_bytes()).unwrap());
        }
    }

    #[test]
    fn test_full_capacity() {
        const FALSE_POSITIVE_RATE: f64 = 0.1;
        let config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(FALSE_POSITIVE_RATE)
            .level_duration(Duration::from_secs(1))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        // Insert more items than capacity to test behavior
        for i in 0..200 {
            let item = format!("item_{}", i);
            filter.insert(item.as_bytes()).unwrap();

            filter.cleanup_expired_levels().unwrap();
            assert!(filter.query(item.as_bytes()).unwrap());
        }

        filter.cleanup_expired_levels().unwrap();
        // Expect higher false positive rate due to saturation
        let false_queries = (200..300)
            .filter(|i| {
                let item = format!("item_{}", i);
                filter.query(item.as_bytes()).unwrap()
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
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(5)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        filter.insert(b"persistent_item").unwrap();

        // Insert items that should expire
        filter.insert(b"temp_item").unwrap();

        filter.cleanup_expired_levels().unwrap();
        assert!(filter.query(b"temp_item").unwrap());

        // Wait for the temporary item to expire
        thread::sleep(Duration::from_secs(6)); // Exceeds MAX_LEVELS * LEVEL_TIME
        filter.cleanup_expired_levels().unwrap();

        // "temp_item" should be expired
        assert!(!filter.query(b"temp_item").unwrap());

        // "persistent_item" should be also expired
        assert!(!filter.query(b"persistent_item").unwrap());
    }

    #[test]
    fn test_should_create_new_level_edge_case() {
        const MAX_LEVELS: usize = 3;
        let config = FilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_millis(500))
            .max_levels(MAX_LEVELS)
            .build()
            .expect("Unable to build FilterConfig");

        let mut filter = InMemorySlidingBloomFilter::new(config)
            .expect("Failed to create InMemorySlidingBloomFilter");

        // Rapid insertions to test level creation
        for i in 0..10 {
            let item = format!("rapid_item_{}", i);
            filter.insert(item.as_bytes()).unwrap();
            thread::sleep(Duration::from_millis(100)); // Sleep less than LEVEL_TIME
        }

        // Levels should have been created appropriately
        assert!(filter.storage.num_levels() <= MAX_LEVELS);
    }
}
