//! Standard Bloom Filter implementation.
//!
//! # Snapshot Persistence
//!
//! When a `db_path` is configured, the filter persists its bit-vector state to disk using
//! incremental dirty-chunk snapshots. Two snapshot modes are supported:
//!
//! ## Manual snapshot
//!
//! Call [`BloomFilter::save_snapshot`] at any time to flush dirty chunks to disk.
//!
//! ## Automatic snapshot (background task)
//!
//! Enable via [`PersistenceConfig::auto_snapshot`]. When enabled, a background task
//! triggers a snapshot based on either:
//! - a time interval ([`PersistenceConfig::snapshot_interval`]), or
//! - an insert-count threshold ([`PersistenceConfig::snapshot_after_inserts`], set to 0
//!   to disable).
//!
//! The background task fires when **either** trigger fires (OR logic).
//!
//! ## Shutdown
//!
//! A final snapshot is always attempted on clean shutdown (when the filter is dropped),
//! regardless of the `auto_snapshot` setting.
//!
//! ## Failure semantics
//!
//! Snapshot errors are **hard failures**. After the first background snapshot write error:
//! - the error is stored in shared state,
//! - the filter is poisoned for future mutations (insert/clear return that error),
//! - read operations (contains) continue against in-memory state,
//! - manual `save_snapshot()` also returns the stored error while poisoned.
pub mod config;
pub mod error;
pub mod filter;
#[cfg(feature = "fjall")]
pub mod storage;
pub mod traits;

pub use config::{
    BloomFilterConfig, BloomFilterConfigBuilder, PersistenceConfig,
    PersistenceConfigBuilder,
};
pub use error::{BloomError, BloomResult};
pub use filter::BloomFilter;
pub use traits::{
    BloomFilterOps, BloomFilterStats, BulkBloomFilterOps, PersistentBloomFilter,
    StorageBackend,
};
