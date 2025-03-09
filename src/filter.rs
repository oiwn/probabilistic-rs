use crate::error::Result;
use crate::hash::{HashFunction, default_hash_function};
use derive_builder::Builder;
use std::time::Duration;

/// Configuration for all filter implementations
#[derive(Clone, Debug, Builder)]
#[builder(pattern = "owned")]
pub struct FilterConfig {
    /// Maximum number of elements the filter can hold
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    /// Number of filter levels for time-based expiration
    #[builder(default = "3")]
    pub max_levels: usize,

    /// Desired false positive rate (between 0 and 1)
    #[builder(default = "0.01")]
    pub false_positive_rate: f64,

    /// Duration after which entries in a level expire
    #[builder(default = "Duration::from_secs(60)")]
    pub level_duration: Duration,

    /// Hash function used for bloom filter operations
    #[builder(default = "default_hash_function")]
    pub hash_function: HashFunction,
}

pub trait SlidingBloomFilter {
    fn insert(&mut self, item: &[u8]) -> Result<()>;
    fn query(&self, item: &[u8]) -> Result<bool>;
    fn cleanup_expired_levels(&mut self) -> Result<()>;
}
