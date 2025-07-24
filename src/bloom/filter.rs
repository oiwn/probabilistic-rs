use super::{BloomError, BloomFilterConfig, BloomFilterOps, BloomResult};
use crate::hash::{optimal_bit_vector_size, optimal_num_hashes};
use bitvec::{bitvec, order::Lsb0, vec::BitVec};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct BloomFilter {
    config: BloomFilterConfig,
    bit_vector_size: usize,
    num_hashes: usize,
    bits: BitVec<usize, Lsb0>,
    insert_count: AtomicUsize,
}

impl BloomFilter {
    pub fn new(config: BloomFilterConfig) -> BloomResult<Self> {
        config.validate()?;

        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);

        let bits = bitvec![0; bit_vector_size];

        Ok(Self {
            config,
            bit_vector_size,
            num_hashes,
            bits,
            insert_count: AtomicUsize::new(0),
        })
    }

    pub fn config(&self) -> &BloomFilterConfig {
        &self.config
    }

    pub fn approx_memory_bits(&self) -> usize {
        let words = self.bits.as_raw_slice(); // &[u64]
        words.len() * std::mem::size_of::<u64>()
    }

    pub fn bits_per_item(&self) -> f64 {
        self.approx_memory_bits() as f64 / self.config.capacity as f64
    }
}

impl BloomFilterOps for BloomFilter {
    fn insert(&mut self, item: &[u8]) -> BloomResult<()> {
        let indices = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }
            self.bits.set(idx, true);
        }

        self.insert_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn contains(&self, item: &[u8]) -> BloomResult<bool> {
        let indices = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }
            if !self.bits[idx] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn clear(&mut self) -> BloomResult<()> {
        self.bits.fill(false);
        self.insert_count.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn estimated_count(&self) -> usize {
        self.insert_count.load(Ordering::Relaxed)
    }

    fn capacity(&self) -> usize {
        self.config.capacity
    }

    fn false_positive_rate(&self) -> f64 {
        self.config.false_positive_rate
    }
}
