use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct ExpiringPersistenceConfig {
    pub db_path: PathBuf,
    #[builder(default = "4096")]
    pub chunk_size_bytes: usize,
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
    pub fn validate(&self) -> Result<(), String> {
        if self.capacity_per_level == 0 {
            return Err("Capacity per level must be greater than 0".to_string());
        }
        if self.target_fpr <= 0.0 || self.target_fpr >= 1.0 {
            return Err(
                "Target false positive rate must be between 0 and 1".to_string()
            );
        }
        if self.level_duration.as_millis() == 0 {
            return Err("Level duration must be greater than 0".to_string());
        }
        if self.num_levels == 0 {
            return Err("Number of levels must be greater than 0".to_string());
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelMetadata {
    pub created_at: u64,
    pub insert_count: usize,
    pub last_snapshot_at: u64,
}
