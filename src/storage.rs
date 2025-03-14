use crate::error::{FilterError, Result};
use bitvec::{
    order::Lsb0,
    prelude::{BitVec, bitvec},
};
use std::{sync::RwLock, time::SystemTime};

// Trait for the storage backend
pub trait FilterStorage {
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
    pub levels: RwLock<Vec<BitVec<usize, Lsb0>>>,
    pub timestamps: RwLock<Vec<SystemTime>>,
    pub capacity: usize,
}

impl InMemoryStorage {
    pub fn new(capacity: usize, max_levels: usize) -> Result<Self> {
        let levels = (0..max_levels).map(|_| bitvec![0; capacity]).collect();
        Ok(Self {
            levels: RwLock::new(levels),
            timestamps: RwLock::new(vec![SystemTime::now(); max_levels]),
            capacity,
        })
    }
}

impl FilterStorage for InMemoryStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        let mut levels = self.levels.write().map_err(|e| {
            FilterError::StorageError(format!(
                "Failed to acquire write lock: {}",
                e
            ))
        })?;
        if level >= levels.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: levels.len(),
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
            levels[level].set(index, true);
        }
        Ok(())
    }

    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>> {
        let levels = self.levels.read().map_err(|e| {
            FilterError::StorageError(format!(
                "Failed to acquire read lock: {}",
                e
            ))
        })?;
        if level >= levels.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: levels.len(),
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
        Ok(indices.iter().map(|&index| levels[level][index]).collect())
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        let mut levels = self.levels.write().map_err(|e| {
            FilterError::StorageError(format!(
                "Failed to acquire write lock: {}",
                e
            ))
        })?;
        if level >= levels.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: levels.len(),
            });
        }
        levels[level].fill(false);
        // self.levels[level] = vec![false; self.capacity];
        Ok(())
    }

    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()> {
        let mut timestamps = self.timestamps.write().map_err(|e| {
            FilterError::StorageError(format!(
                "Failed to acquire write lock: {}",
                e
            ))
        })?;
        if level >= timestamps.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: timestamps.len(),
            });
        }

        timestamps[level] = timestamp;
        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        let timestamps = self.timestamps.read().map_err(|e| {
            FilterError::StorageError(format!(
                "Failed to acquire read lock: {}",
                e
            ))
        })?;
        if level >= timestamps.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: timestamps.len(),
            });
        }

        Ok(Some(timestamps[level]))
    }

    fn num_levels(&self) -> usize {
        match self.levels.read() {
            Ok(levels) => levels.len(),
            Err(_) => 0, // Consider how to handle this error case
        }
    }
}
