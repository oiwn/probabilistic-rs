use super::{CuckooError, CuckooResult};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

pub const LOAD_FACTOR: f64 = 0.95;

#[derive(Clone, Debug, Builder, Serialize, Deserialize)]
#[builder(pattern = "owned")]
pub struct CuckooFilterConfig {
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    #[builder(default = "8")]
    pub fingerprint_bits: usize,

    #[builder(default = "4")]
    pub entries_per_bucket: usize,

    #[builder(default = "500")]
    pub max_kicks: usize,

    #[builder(default = "None")]
    pub persistence: Option<CuckooPersistenceConfig>,
}

#[derive(Builder, Clone, Debug, Serialize, Deserialize)]
pub struct CuckooPersistenceConfig {
    pub db_path: PathBuf,

    #[builder(default = "Duration::from_secs(60)")]
    pub snapshot_interval: Duration,

    #[builder(default = "4096")]
    pub chunk_size_bytes: usize,

    #[builder(default = "false")]
    pub auto_snapshot: bool,

    #[builder(default = "0")]
    pub snapshot_after_inserts: usize,
}

impl Default for CuckooFilterConfig {
    fn default() -> Self {
        CuckooFilterConfigBuilder::default()
            .build()
            .expect("default CuckooFilterConfig is valid")
    }
}

impl CuckooFilterConfig {
    pub fn validate(&self) -> CuckooResult<()> {
        if self.capacity == 0 {
            return Err(CuckooError::ZeroCapacity);
        }
        if self.fingerprint_bits < 4 || self.fingerprint_bits > 16 {
            return Err(CuckooError::InvalidFingerprintBits(
                self.fingerprint_bits,
            ));
        }
        if self.entries_per_bucket < 2 || self.entries_per_bucket > 8 {
            return Err(CuckooError::InvalidEntriesPerBucket(
                self.entries_per_bucket,
            ));
        }
        if self.max_kicks == 0 {
            return Err(CuckooError::ZeroMaxKicks);
        }
        Ok(())
    }

    pub fn num_buckets(&self) -> usize {
        let raw = (self.capacity as f64
            / (self.entries_per_bucket as f64 * LOAD_FACTOR))
            .ceil() as usize;
        raw.max(1).next_power_of_two()
    }

    pub fn fingerprint_bitmask(&self) -> u16 {
        if self.fingerprint_bits >= 16 {
            0xFFFF
        } else {
            (1u16 << self.fingerprint_bits) - 1
        }
    }

    pub fn to_bytes(&self) -> CuckooResult<Vec<u8>> {
        postcard::to_allocvec(self)
            .map_err(|e| CuckooError::SerializationError(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> CuckooResult<Self> {
        postcard::from_bytes(bytes)
            .map_err(|e| CuckooError::SerializationError(e.to_string()))
    }
}
