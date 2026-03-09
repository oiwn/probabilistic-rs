//! Time-decaying (expiring) Bloom Filter implementation.
//!
//! # Snapshot Persistence
//!
//! When a `db_path` is configured, the filter persists state to disk. There are two
//! distinct snapshot mechanisms:
//!
//! ## Level-rotation full snapshot
//!
//! When an expired level is rotated out, a full snapshot of all levels is written to disk
//! automatically. This happens inside [`ExpiringBloomFilter::rotate_levels`] and
//! [`ExpiringBloomFilter::cleanup_expired_levels`] and is not user-configurable.
//!
//! ## Dirty-chunk background snapshot
//!
//! Enable via [`ExpiringPersistenceConfig::auto_snapshot`]. A background task periodically
//! flushes only the chunks modified since the last snapshot, triggered by:
//! - a time interval ([`ExpiringPersistenceConfig::snapshot_interval`]), or
//! - an insert-count threshold ([`ExpiringPersistenceConfig::snapshot_after_inserts`],
//!   set to 0 to disable).
//!
//! Manual dirty-chunk snapshots are also available via [`ExpiringBloomFilter::save_snapshot`].
//!
//! The two mechanisms are independent — a level rotation does not reset the dirty-chunk
//! background timer, and a dirty-chunk snapshot does not affect rotation behavior.
//!
//! ## Shutdown
//!
//! A final dirty-chunk snapshot is always attempted on clean shutdown (when the filter is
//! dropped), regardless of the `auto_snapshot` setting.
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
pub mod storage;
pub mod traits;

pub use config::{
    ExpiringFilterConfig, ExpiringFilterConfigBuilder, ExpiringPersistenceConfig,
};
pub use error::{EbloomError, Result};
pub use filter::ExpiringBloomFilter;
pub use traits::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilterOps, ExpiringBloomFilterStats,
};
