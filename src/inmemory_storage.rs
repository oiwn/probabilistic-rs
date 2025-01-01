use super::{BloomError, BloomFilterStorage, Result};
use std::time::SystemTime;

// In-memory storage implementation
pub struct InMemoryStorage {
    pub levels: Vec<Vec<bool>>,
    timestamps: Vec<SystemTime>,
    capacity: usize,
}

impl BloomFilterStorage for InMemoryStorage {
    fn new(capacity: usize, max_levels: usize) -> Result<Self> {
        Ok(Self {
            levels: vec![vec![false; capacity]; max_levels],
            timestamps: vec![SystemTime::now(); max_levels],
            capacity,
        })
    }

    fn set_bit(&mut self, level: usize, index: usize) -> Result<()> {
        if index >= self.capacity {
            return Err(BloomError::IndexOutOfBounds {
                index,
                capacity: self.capacity,
            });
        }
        if level >= self.levels.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        self.levels[level][index] = true;
        Ok(())
    }

    fn get_bit(&self, level: usize, index: usize) -> Result<bool> {
        if index >= self.capacity {
            return Err(BloomError::IndexOutOfBounds {
                index,
                capacity: self.capacity,
            });
        }
        if level >= self.levels.len() {
            return Err(BloomError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        Ok(self.levels[level][index])
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
