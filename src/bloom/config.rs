use crate::hash::{HashFunction, default_hash_function};
use derive_builder::Builder;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Builder)]
#[builder(pattern = "owned")]
pub struct BloomFilterConfig {
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    #[builder(default = "0.01")]
    pub false_positive_rate: f64,

    #[builder(default = "default_hash_function")]
    pub hash_function: HashFunction,

    #[builder(default = "None")]
    pub persistence: Option<PersistenceConfig>,
}

#[derive(Clone, Debug)]
pub struct SnapshotConfig {
    pub interval: Duration,
    pub after_inserts: usize,
    pub auto_enabled: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            after_inserts: 10000,
            auto_enabled: true,
        }
    }
}

#[derive(Builder, Clone, Debug)]
pub struct PersistenceConfig {
    pub db_path: PathBuf,
    #[builder(default)]
    pub snapshot_config: SnapshotConfig,
    #[builder(default = "4096")] // 4KB chunks by default
    pub chunk_size_bytes: usize,
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
}
