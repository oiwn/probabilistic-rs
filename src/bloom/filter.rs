use super::{BloomConfig, BloomError, BloomParams, BloomResult};
use bitvec::{bitvec, order::Lsb0, vec::BitVec};
use std::sync::atomic::{AtomicUsize, Ordering};

pub trait BloomFilter {
    fn insert(&mut self, item: &[u8]) -> BloomResult<()>;
    fn contains(&self, item: &[u8]) -> BloomResult<bool>;
    fn clear(&mut self) -> BloomResult<()>;

    // Statistics
    fn estimated_count(&self) -> usize;
    fn capacity(&self) -> usize;
    fn false_positive_rate(&self) -> f64;
}

pub struct BitVectorBloom {
    config: BloomConfig,
    params: BloomParams,
    bits: BitVec<usize, Lsb0>,
    insert_count: AtomicUsize,
}

impl BitVectorBloom {
    pub fn new(config: BloomConfig) -> BloomResult<Self> {
        config.validate()?;

        let params = BloomParams::from(&config);
        let bits = bitvec![0; params.bit_vector_size];

        Ok(Self {
            config,
            params,
            bits,
            insert_count: AtomicUsize::new(0),
        })
    }

    pub fn config(&self) -> &BloomConfig {
        &self.config
    }

    pub fn params(&self) -> &BloomParams {
        &self.params
    }
}

impl BloomFilter for BitVectorBloom {
    fn insert(&mut self, item: &[u8]) -> BloomResult<()> {
        let indices = (self.config.hash_function)(
            item,
            self.params.num_hashes,
            self.params.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.params.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.params.bit_vector_size,
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
            self.params.num_hashes,
            self.params.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.params.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.params.bit_vector_size,
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
