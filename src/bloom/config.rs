use super::{BloomError, BloomResult};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Builder, Serialize, Deserialize)]
#[builder(pattern = "owned")]
pub struct BloomFilterConfig {
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    #[builder(default = "0.01")]
    pub false_positive_rate: f64,

    #[builder(default = "None")]
    pub persistence: Option<PersistenceConfig>,
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Path to the on-disk database directory.
    pub db_path: PathBuf,

    /// How often the background snapshot task fires.
    /// Only used when `auto_snapshot` is true.
    #[builder(default = "Duration::from_secs(60)")]
    pub snapshot_interval: Duration,

    /// Chunk size in bytes for incremental dirty-chunk snapshots.
    #[builder(default = "4096")]
    pub chunk_size_bytes: usize,

    /// Enable the background auto-snapshot task.
    ///
    /// When true, a background task periodically persists dirty chunks to disk
    /// based on `snapshot_interval` and/or `snapshot_after_inserts`.
    /// A final snapshot is always attempted on clean shutdown regardless of this flag.
    #[builder(default = "false")]
    pub auto_snapshot: bool,

    /// Trigger a snapshot after this many inserts since the last successful snapshot.
    /// Set to 0 to disable the insert-count trigger.
    /// Only used when `auto_snapshot` is true.
    #[builder(default = "0")]
    pub snapshot_after_inserts: usize,
}

impl BloomFilterConfig {
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

    pub fn to_bytes(&self) -> BloomResult<Vec<u8>> {
        postcard::to_allocvec(self)
            .map_err(|e| BloomError::SerializationError(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> BloomResult<Self> {
        postcard::from_bytes(bytes)
            .map_err(|e| BloomError::SerializationError(e.to_string()))
    }
}
