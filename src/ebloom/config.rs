use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::ebloom::error::{EbloomError, Result};

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct ExpiringPersistenceConfig {
    /// Path to the on-disk database directory.
    pub db_path: PathBuf,

    /// Chunk size in bytes for incremental dirty-chunk snapshots.
    #[builder(default = "4096")]
    pub chunk_size_bytes: usize,

    /// Enable the background auto-snapshot task for dirty chunks.
    ///
    /// When true, a background task periodically persists dirty chunks to disk
    /// based on `snapshot_interval` and/or `snapshot_after_inserts`.
    /// Full snapshots on level rotation are unaffected by this flag.
    /// A final snapshot is always attempted on clean shutdown regardless of this flag.
    #[builder(default = "false")]
    pub auto_snapshot: bool,

    /// How often the background snapshot task fires.
    /// Only used when `auto_snapshot` is true.
    #[builder(default = "Duration::from_secs(60)")]
    pub snapshot_interval: Duration,

    /// Trigger a snapshot after this many inserts since the last successful snapshot.
    /// Set to 0 to disable the insert-count trigger.
    /// Only used when `auto_snapshot` is true.
    #[builder(default = "0")]
    pub snapshot_after_inserts: usize,
}

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
#[builder(setter(into))]
pub struct ExpiringFilterConfig {
    #[builder(default = "1_000_000")]
    pub capacity_per_level: usize,
    #[builder(default = "0.01")]
    pub target_fpr: f64,
    #[builder(default = "Duration::from_secs(60 * 60)")] // 1 hour
    pub level_duration: Duration,
    #[builder(default = "3")]
    pub num_levels: usize,
    #[builder(default = "None")]
    pub persistence: Option<ExpiringPersistenceConfig>,
}

impl ExpiringFilterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.capacity_per_level == 0 {
            return Err(EbloomError::InvalidConfig(
                "Capacity per level must be greater than 0".to_string(),
            ));
        }
        if self.target_fpr <= 0.0 || self.target_fpr >= 1.0 {
            return Err(EbloomError::InvalidConfig(
                "Target false positive rate must be between 0 and 1".to_string(),
            ));
        }
        if self.level_duration.as_millis() == 0 {
            return Err(EbloomError::InvalidConfig(
                "Level duration must be greater than 0".to_string(),
            ));
        }
        if self.num_levels == 0 {
            return Err(EbloomError::InvalidConfig(
                "Number of levels must be greater than 0".to_string(),
            ));
        }
        if self.num_levels > 255 {
            return Err(EbloomError::InvalidConfig(
                "Number of levels must be <= 255".to_string(),
            ));
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        postcard::to_allocvec(self)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelMetadata {
    pub created_at: u64,
    pub insert_count: u64,
    pub last_snapshot_at: u64,
}
