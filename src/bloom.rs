//! Standard Bloom Filter implementation
pub mod config;
pub mod error;
pub mod filter;
// pub mod storage;

// pub use storage::{BloomStorage, InMemoryBloomStorage, RedbBloomStorage};
pub use config::{BloomFilterConfig, BloomFilterConfigBuilder};
pub use error::{BloomError, BloomResult};
pub use filter::BloomFilter;

pub trait BloomFilterOps {
    fn insert(&mut self, item: &[u8]) -> BloomResult<()>;
    fn contains(&self, item: &[u8]) -> BloomResult<bool>;
    fn clear(&mut self) -> BloomResult<()>;

    // Statistics
    fn estimated_count(&self) -> usize;
    fn capacity(&self) -> usize;
    fn false_positive_rate(&self) -> f64;
}
