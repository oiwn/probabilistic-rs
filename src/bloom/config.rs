use crate::hash::{
    HashFunction, default_hash_function, optimal_bit_vector_size,
    optimal_num_hashes,
};
use derive_builder::Builder;
use std::time::Duration;

#[derive(Clone, Debug, Builder)]
#[builder(pattern = "owned")]
pub struct BloomConfig {
    /// Maximum number of elements
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    /// Target false positive rate (0.0 to 1.0)
    #[builder(default = "0.01")]
    pub false_positive_rate: f64,

    /// Hash function to use
    #[builder(default = "default_hash_function")]
    pub hash_function: HashFunction,

    /// Auto-snapshot interval
    #[builder(default = "Duration::from_secs(300)")]
    pub snapshot_interval: Duration,

    /// Snapshot after N inserts
    #[builder(default = "10_000")]
    pub snapshot_after_inserts: usize,
}

impl BloomConfig {
    pub fn validate(&self) -> super::BloomResult<()> {
        if self.capacity == 0 {
            return Err(super::BloomError::InvalidConfig(
                "Capacity must be > 0".into(),
            ));
        }
        if self.false_positive_rate <= 0.0 || self.false_positive_rate >= 1.0 {
            return Err(super::BloomError::InvalidConfig(
                "FPR must be between 0 and 1".into(),
            ));
        }
        Ok(())
    }
}

/// Derived parameters calculated from BloomConfig
#[derive(Debug, Clone)]
pub struct BloomParams {
    pub bit_vector_size: usize,
    pub num_hashes: usize,
}

impl From<&BloomConfig> for BloomParams {
    fn from(config: &BloomConfig) -> Self {
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);

        Self {
            bit_vector_size,
            num_hashes,
        }
    }
}
