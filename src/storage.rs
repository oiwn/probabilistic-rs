use crate::error::{FilterError, Result};
use std::time::SystemTime;

// Trait for the storage backend
pub trait BloomStorage {
    /// Sets multiple bits at the specified level and indices
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()>;
    /// Gets multiple bit values at the specified level and indices
    /// Returns a Vec of booleans corresponding to each requested index
    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>>;
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

// In-memory storage implementation
pub struct InMemoryStorage {
    pub levels: Vec<Vec<bool>>,
    pub timestamps: Vec<SystemTime>,
    pub capacity: usize,
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

impl BloomStorage for InMemoryStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        if level >= self.levels.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        // Check all indices first
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(FilterError::IndexOutOfBounds {
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
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        // Check all indices first
        if let Some(&max_index) = indices.iter().max() {
            if max_index >= self.capacity {
                return Err(FilterError::IndexOutOfBounds {
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
            return Err(FilterError::InvalidLevel {
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
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: self.timestamps.len(),
            });
        }

        self.timestamps[level] = timestamp;
        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        if level >= self.timestamps.len() {
            return Err(FilterError::InvalidLevel {
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
