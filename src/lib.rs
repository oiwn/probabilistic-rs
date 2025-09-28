//! Time-Decaying Bloom Filter implementation with Fjall or InMemory storage backends.
//!
//! This crate provides a Bloom filter implementation that automatically expires
//! elements after a configurable time period using a sliding window approach.
//!
//! HowTo:
//!    * Sub-Filters: The main Bloom filter is divided into N sub-filters:  BF_1, BF_2, …, BF_N .
//!    * Time Windows: Each sub-filter corresponds to a fixed time window  T  (e.g., 1 minute).
//!    * Rotation Mechanism: Sub-filters are rotated in a circular manner to represent sliding
//!      time intervals.
//!
//! Insertion:
//!     * When an element is added at time  t , it is inserted into the current sub-filter  BF_{current}.
//!     * Hash the element using the standard Bloom filter hash functions and set the bits in  BF_{current} .
//! Query:
//!     * To check if an element is in the filter, perform the query against all active sub-filters.
//!     * If all the required bits are set in any sub-filter, the element is considered present.
//! Expiration:
//!     * At each time interval  T , the oldest sub-filter  BF_{oldest}  is cleared.
//!     * The cleared sub-filter becomes the new  BF_{current}  for incoming elements.
//!     * This effectively removes elements that were only in  BF_{oldest} , thus expiring them.
//!
//! Obvious problems:
//!     * False Positives: As elements may exist in multiple sub-filters,
//!       the probability of false positives can increase.
//!     * Synchronization: In concurrent environments, care must be taken to synchronize
//!       access during sub-filter rotation.
//!     * Since 32 bit hashes used, max capacity would be 2**32-1 (Not sure)

pub mod bloom;
pub mod common;
mod error;
mod hash;
mod probablistic;
#[cfg(feature = "server")]
pub mod server;
mod storage;
#[cfg(feature = "cli")]
pub mod tui;

pub use error::{FilterError, Result};
pub use hash::{
    HashFunction, default_hash_function, optimal_bit_vector_size,
    optimal_num_hashes,
};
pub use probablistic::filter::{
    self, ExpiringBloomFilter, FilterConfig, FilterConfigBuilder,
    FilterConfigBuilderError,
};
#[cfg(feature = "server")]
pub use server::types::{
    AppState, ErrorResponse, InsertRequest, QueryResponse, ServerConfig,
    ServerConfigBuilder, ServerConfigBuilderError,
};
#[cfg(feature = "fjall")]
pub use storage::fjall_filter::{
    FjallFilter, FjallFilterConfig, FjallFilterConfigBuilder,
};
pub use storage::inmemory_filter::InMemoryFilter;
pub use storage::{FilterStorage, InMemoryStorage};
