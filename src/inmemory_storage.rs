use super::{BloomError, BloomFilterStorage, Result};
use std::time::SystemTime;

// In-memory storage implementation
pub struct InMemoryStorage {
    pub levels: Vec<Vec<bool>>,
    timestamps: Vec<SystemTime>,
    capacity: usize,
}
impl InMemoryStorage {
    pub fn new(capacity: usize, max_levels: usize) -> Result<Self> {
        Ok(Self {
            levels: vec![vec![false; capacity]; max_levels],
            timestamps: vec![SystemTime::now(); max_levels],
            capacity,
        })
    }
}

impl BloomFilterStorage for InMemoryStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        if level >= self.levels.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        // Check all indices first
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        // Set all bits in one go
        for &index in indices {
            self.levels[level][index] = true;
        }
        Ok(())
    }

    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>> {
        if level >= self.levels.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        // Check all indices first
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(BloomError::IndexOutOfBounds {
                    index: max_index,
                    capacity: self.capacity,
                });
            }
        }

        // Get all bits in one go
        Ok(indices
            .iter()
            .map(|&index| self.levels[level][index])
            .collect())
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        if level >= self.levels.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        self.levels[level] = vec![false; self.capacity];
        Ok(())
    }

    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()> {
        if level >= self.timestamps.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.timestamps.len(),
            });
        }

        self.timestamps[level] = timestamp;
        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        if level >= self.timestamps.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.timestamps.len(),
            });
        }

        Ok(Some(self.timestamps[level]))
    }

    fn num_levels(&self) -> usize {
        self.levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_hash_function, SlidingBloomFilter};
    use std::time::Duration;

    #[test]
    fn test_inmemory_batch_performance() {
        use rand::RngCore;
        use std::time::Instant;

        const NUM_ITEMS: usize = 100_000; // Test with 100k items

        // Setup
        let mut rng = rand::thread_rng();
        let mut bloom = SlidingBloomFilter::new(
            InMemoryStorage::new(NUM_ITEMS, 3).unwrap(),
            NUM_ITEMS,
            0.01,
            Duration::from_secs(60),
            3,
            default_hash_function,
        )
        .unwrap();

        // Generate test data
        let items: Vec<Vec<u8>> = (0..NUM_ITEMS)
            .map(|_| {
                let mut bytes = vec![0u8; 16];
                rng.fill_bytes(&mut bytes);
                bytes
            })
            .collect();

        // Measure insertion performance
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            bloom.insert(item).unwrap();

            if (i + 1) % 10_000 == 0 {
                println!("Inserted {} items...", i + 1);
            }
        }

        let insert_elapsed = start.elapsed();
        println!(
            "\nBatch insertion of {} items took: {:?}",
            NUM_ITEMS, insert_elapsed
        );
        println!(
            "Average insertion time per item: {:?}",
            insert_elapsed / NUM_ITEMS as u32
        );

        // Performance assertions
        assert!(
            insert_elapsed < Duration::from_secs(1),
            "Insertion of {} items took {:?}, which is more than 1 second",
            NUM_ITEMS,
            insert_elapsed
        );

        // Measure query performance
        let start = Instant::now();

        for (i, item) in items.iter().enumerate() {
            assert!(bloom.query(item).unwrap());

            if (i + 1) % 10_000 == 0 {
                println!("Queried {} items...", i + 1);
            }
        }

        let query_elapsed = start.elapsed();
        println!(
            "\nBatch query of {} items took: {:?}",
            NUM_ITEMS, query_elapsed
        );
        println!(
            "Average query time per item: {:?}",
            query_elapsed / NUM_ITEMS as u32
        );

        // Performance assertions
        assert!(
            query_elapsed < Duration::from_secs(1),
            "Querying {} items took {:?}, which is more than 1 second",
            NUM_ITEMS,
            query_elapsed
        );
    }
}
