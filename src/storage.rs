/// Thread-safe storage for multiple Bloom filter levels.
///
/// This implementation provides concurrent read access to different threads
/// while ensuring exclusive write access when needed. The storage uses RwLock
/// for synchronization between threads.
use crate::error::{FilterError, Result};
use bitvec::{
    order::Lsb0,
    prelude::{BitVec, bitvec},
};
use std::{sync::RwLock, time::SystemTime};

#[cfg(feature = "fjall")]
pub mod fjall_filter;
pub mod inmemory_filter;
#[cfg(feature = "redb")]
pub mod redb_filter;

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
    pub levels: Vec<BitVec<usize, Lsb0>>,
    pub timestamps: Vec<SystemTime>,
    pub capacity: usize,
}

impl InMemoryStorage {
    pub fn new(capacity: usize, max_levels: usize) -> Result<Self> {
        let levels = (0..max_levels).map(|_| bitvec![0; capacity]).collect();
        Ok(Self {
            levels,
            timestamps: vec![SystemTime::now(); max_levels],
            capacity,
        })
    }

    pub fn bit_vector_len(&self) -> usize {
        self.levels.first().unwrap().len()
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
        for level in self.levels.iter() {
            total_bytes += level.capacity() / 8;
        }

        // Calculate size of timestamps
        // Each SystemTime is typically two u64s
        let timestamps_size =
            self.timestamps.len() * std::mem::size_of::<SystemTime>();
        let vec_overhead = 3 * std::mem::size_of::<usize>();

        total_bytes += timestamps_size + vec_overhead;

        // Add RwLock overhead (approximate)
        let rwlock_overhead = 2 * std::mem::size_of::<usize>() * 2; // For both locks
        total_bytes += rwlock_overhead;
        total_bytes
    }

    pub fn bitvec_to_bytes(&self, bits: &BitVec<usize, Lsb0>) -> Vec<u8> {
        // Calculate how many bytes we need (ceiling division of capacity by 8)
        let byte_count = (self.capacity + 7).div_ceil(8);
        let mut result = Vec::with_capacity(byte_count);

        // Iterate through all bytes that would be needed for capacity
        for byte_idx in 0..byte_count {
            let mut byte = 0u8;
            // For each bit position in the byte
            for bit_pos in 0..8 {
                let bit_idx = byte_idx * 8 + bit_pos;
                // Only set the bit if it's within bounds and set in the source
                if bit_idx < bits.len() && bits[bit_idx] {
                    byte |= 1 << bit_pos;
                }
            }
            result.push(byte);
        }

        result
    }

    pub fn bytes_to_bitvec(&self, bytes: &[u8]) -> Result<BitVec<usize, Lsb0>> {
        // Create a new bitvec with the known capacity
        let mut bv = bitvec![usize, Lsb0; 0; self.capacity];

        // Ensure we have enough bytes
        let expected_bytes = (self.capacity + 7).div_ceil(8);
        if bytes.len() < expected_bytes {
            return Err(FilterError::StorageError(format!(
                "Byte array too short for bit vector: expected at least {expected_bytes} bytes"
            )));
        }

        // Fill in the bits
        for (i, &byte) in bytes.iter().enumerate() {
            for bit_pos in 0..8 {
                let idx = i * 8 + bit_pos;
                if idx < self.capacity {
                    bv.set(idx, (byte & (1 << bit_pos)) != 0);
                }
            }
        }

        Ok(bv)
    }
}

impl FilterStorage for InMemoryStorage {
    fn set_bits(&mut self, level: usize, indices: &[usize]) -> Result<()> {
        if level >= self.levels.len() {
            return Err(FilterError::InvalidLevel {
                level,
                max_levels: self.levels.len(),
            });
        }

        debug_assert!(
            indices.iter().all(|&i| i < self.capacity),
            "IndexOutOfBounds in batch: capacity = {}",
            self.capacity
        );

        // Set all bits in one go
        for &index in indices {
            self.levels[level].set(index, true);
        }
        Ok(())
    }

    fn get_bits(&self, level: usize, indices: &[usize]) -> Result<Vec<bool>> {
        debug_assert!(
            level < self.levels.len(),
            "InvalidLevel: level = {}, max_levels = {}",
            level,
            self.levels.len()
        );

        // Check all indices first
        if let Some(&max_index) = indices.iter().max()
            && max_index >= self.capacity
        {
            return Err(FilterError::IndexOutOfBounds {
                index: max_index,
                capacity: self.capacity,
            });
        }

        // Get all bits in one go
        Ok(indices
            .iter()
            .map(|&index| self.levels[level][index])
            .collect())
    }

    fn clear_level(&mut self, level: usize) -> Result<()> {
        debug_assert!(
            level < self.levels.len(),
            "InvalidLevel: level = {}, max_levels = {}",
            level,
            self.levels.len()
        );
        self.levels[level].fill(false);
        Ok(())
    }

    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()> {
        debug_assert!(
            level < self.timestamps.len(),
            "InvalidLevel: level = {}, max_levels = {}",
            level,
            self.timestamps.len()
        );
        self.timestamps[level] = timestamp;
        Ok(())
    }

    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>> {
        debug_assert!(
            level < self.timestamps.len(),
            "InvalidLevel: level = {}, max_levels = {}",
            level,
            self.timestamps.len()
        );

        Ok(Some(self.timestamps[level]))
    }

    fn num_levels(&self) -> usize {
        self.levels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitvec_serialization() {
        // Create a test storage instance
        let storage = InMemoryStorage::new(100, 1).unwrap();

        // Create a BitVec with various patterns
        let mut bv = bitvec![usize, Lsb0; 0; 100];

        // Set some bits
        bv.set(0, true);
        bv.set(7, true);
        bv.set(8, true);
        bv.set(42, true);
        bv.set(99, true);

        // Serialize to bytes
        let bytes = storage.bitvec_to_bytes(&bv);

        // Deserialize back to BitVec
        let bv2 = storage.bytes_to_bitvec(&bytes).unwrap();

        // Verify they're the same
        assert_eq!(bv.len(), bv2.len());
        for i in 0..bv.len() {
            assert_eq!(bv[i], bv2[i], "Bit at position {i} doesn't match");
        }
    }

    #[test]
    fn test_bitvec_empty() {
        // Test with an empty BitVec
        let storage = InMemoryStorage::new(0, 1).unwrap();
        let bv = bitvec![usize, Lsb0; 0; 0];

        let bytes = storage.bitvec_to_bytes(&bv);
        let bv2 = storage.bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        assert_eq!(bv.len(), 0);
    }

    #[test]
    fn test_bitvec_all_set() {
        // Test with all bits set
        let storage = InMemoryStorage::new(50, 1).unwrap();
        let mut bv = bitvec![usize, Lsb0; 0; 50];
        bv.fill(true);

        let bytes = storage.bitvec_to_bytes(&bv);
        let bv2 = storage.bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        for i in 0..bv.len() {
            assert!(bv2[i], "Bit at position {i} should be set");
        }
    }

    #[test]
    fn test_bitvec_non_multiple_of_8() {
        // Test with bit count that's not a multiple of 8
        let storage = InMemoryStorage::new(17, 1).unwrap();
        let mut bv = bitvec![usize, Lsb0; 0; 17];
        bv.set(0, true);
        bv.set(8, true);
        bv.set(16, true);

        let bytes = storage.bitvec_to_bytes(&bv);
        let bv2 = storage.bytes_to_bitvec(&bytes).unwrap();

        assert_eq!(bv.len(), bv2.len());
        assert!(bv2[0]);
        assert!(bv2[8]);
        assert!(bv2[16]);
    }

    #[test]
    fn test_bitvec_serialization_error() {
        // Test error handling with too short byte array
        let storage = InMemoryStorage::new(100, 1).unwrap();
        let result = storage.bytes_to_bitvec(&[1, 2, 3]);
        assert!(result.is_err());
    }
}
