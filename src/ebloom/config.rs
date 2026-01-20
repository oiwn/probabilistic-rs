use bincode::{Decode, Encode};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::ebloom::error::{EbloomError, Result};

#[derive(Debug, Clone, Builder, Serialize, Deserialize, Decode, Encode)]
pub struct ExpiringPersistenceConfig {
    pub db_path: PathBuf,
    #[builder(default = "4096")]
    pub chunk_size_bytes: usize,
}

#[derive(Debug, Clone, Builder, Serialize, Deserialize, Decode, Encode)]
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
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::decode_from_slice(bytes, bincode::config::standard())
            .map(|(config, _)| config)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Decode, Encode)]
pub struct LevelMetadata {
    pub created_at: u64,
    pub insert_count: u64,
    pub last_snapshot_at: u64,
}
