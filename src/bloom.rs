//! Standard Bloom Filter implementation
pub mod config;
pub mod error;
pub mod filter;
// pub mod storage;

// pub use storage::{BloomStorage, InMemoryBloomStorage, RedbBloomStorage};
pub use config::{BloomConfig, BloomConfigBuilder, BloomParams};
pub use error::{BloomError, BloomResult};
pub use filter::{BitVectorBloom, BloomFilter};
