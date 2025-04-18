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
    // NOTE: u8 to convert fo db storage or usize?
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

    pub fn bit_vector_len(&self) -> usize {
        self.levels.read().unwrap().first().unwrap().len()
    }

    #[allow(unused)]
    fn estimate_bytes(lock: &RwLock<Vec<BitVec<usize, Lsb0>>>) -> usize {
        let guard = lock.read().unwrap();
        let vec = &*guard;

        let mut total = size_of_val(lock); // size of RwLock struct
        total += size_of_val(vec); // Vec metadata

        for bv in vec {
            // Estimate the buffer behind BitVec
            total += size_of_val(bv); // BitVec struct itself
            total += bv.capacity() * std::mem::size_of::<usize>();
        }

        total
    }

    // Calculate approximate amount of memory in bytes required to store levels
    pub fn approx_memory_usage(&self) -> usize {
        let mut total_bytes = 0;

        // Calculate size of bit vectors
        if let Ok(levels) = self.levels.read() {
            for level in levels.iter() {
                total_bytes += level.capacity() / 8;
            }
        }

        // Calculate size of timestamps
        if let Ok(timestamps) = self.timestamps.read() {
            // Each SystemTime is typically two u64s
            let timestamps_size =
                timestamps.len() * std::mem::size_of::<SystemTime>();
            let vec_overhead = 3 * std::mem::size_of::<usize>();

            total_bytes += timestamps_size + vec_overhead;
        }

        // Add RwLock overhead (approximate)
        let rwlock_overhead = 2 * std::mem::size_of::<usize>() * 2; // For both locks
        total_bytes += rwlock_overhead;
        total_bytes
    }

    #[allow(dead_code)]
    fn bitvec_to_bytes(bits: &BitVec<usize, Lsb0>) -> Vec<u8> {
        // We need to convert from bitvec's internal representation to bytes
        // First, get the bit count to store alongside the data
        let bit_count = bits.len();

        // Serialize the bit count as a u64 (8 bytes)
        let mut result = Vec::new();
        result.extend_from_slice(&(bit_count as u64).to_le_bytes());

        // Now convert the bit vector to bytes
        for chunk in bits.chunks(8) {
            let mut byte = 0u8;
            for (i, bit) in chunk.iter().enumerate() {
                if *bit {
                    byte |= 1 << i;
                }
            }
            result.push(byte);
        }

        result
    }

    #[allow(unused)]
    fn bytes_to_bitvec(bytes: &[u8]) -> Result<BitVec<usize, Lsb0>> {
        if bytes.len() < 8 {
            return Err(FilterError::SerializationError(
                "Byte array too short for bit vector".to_string(),
            ));
        }

        // Extract the bit count from the first 8 bytes
        let bit_count = u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
            bytes[7],
        ]) as usize;

        // Create a new bitvec with the right size
        let mut bv = bitvec![usize, Lsb0; 0; bit_count];

        // Fill in the bits
        for (i, &byte) in bytes[8..].iter().enumerate() {
            for bit_pos in 0..8 {
                if i * 8 + bit_pos < bit_count {
                    bv.set(i * 8 + bit_pos, (byte & (1 << bit_pos)) != 0);
                }
            }
        }

        Ok(bv)
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
        // i think it will not re-allocate, right?
        levels[level].fill(false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitvec_serialization() {
        // Create a BitVec with various patterns
        let mut bv = bitvec![usize, Lsb0; 0; 100];

        // Set some bits
        bv.set(0, true);
        bv.set(7, true);
        bv.set(8, true);
        bv.set(42, true);
        bv.set(99, true);

        // Serialize to bytes
        let bytes = InMemoryStorage::bitvec_to_bytes(&bv);

        // Deserialize back to BitVec
        let bv2 = InMemoryStorage::bytes_to_bitvec(&bytes).unwrap();

        // Verify they're the same
        assert_eq!(bv.len(), bv2.len());
        for i in 0..bv.len() {
            assert_eq!(bv[i], bv2[i], "Bit at position {} doesn't match", i);
        }
    }

    #[test]
    fn test_bitvec_empty() {
        // Test with an empty BitVec
        let bv = bitvec![usize, Lsb0; 0; 0];
        let bytes = InMemoryStorage::bitvec_to_bytes(&bv);
        let bv2 = InMemoryStorage::bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        assert_eq!(bv.len(), 0);
    }

    #[test]
    fn test_bitvec_all_set() {
        // Test with all bits set
        let mut bv = bitvec![usize, Lsb0; 0; 50];
        bv.fill(true);

        let bytes = InMemoryStorage::bitvec_to_bytes(&bv);
        let bv2 = InMemoryStorage::bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        for i in 0..bv.len() {
            assert!(bv2[i], "Bit at position {} should be set", i);
        }
    }

    #[test]
    fn test_bitvec_non_multiple_of_8() {
        // Test with bit count that's not a multiple of 8
        let mut bv = bitvec![usize, Lsb0; 0; 17];
        bv.set(0, true);
        bv.set(8, true);
        bv.set(16, true);

        let bytes = InMemoryStorage::bitvec_to_bytes(&bv);
        let bv2 = InMemoryStorage::bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        assert!(bv2[0]);
        assert!(bv2[8]);
        assert!(bv2[16]);
    }

    #[test]
    fn test_bitvec_serialization_error() {
        // Test error handling with too short byte array
        let result = InMemoryStorage::bytes_to_bitvec(&[1, 2, 3]);
        assert!(result.is_err());
    }
}
