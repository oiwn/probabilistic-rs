use super::{BloomError, BloomFilterConfig, BloomResult};
use std::sync::Arc;

#[async_trait::async_trait]
pub trait StorageBackend {
    async fn save_config(&self, config: &BloomFilterConfig) -> BloomResult<()>;
    async fn load_config(&self) -> BloomResult<Option<BloomFilterConfig>>;
    async fn save_snapshot(&self, chunks: &[(usize, Vec<u8>)])
    -> BloomResult<()>;
    async fn load_snapshot(&self) -> BloomResult<Option<Vec<(usize, Vec<u8>)>>>;
}

#[cfg(feature = "fjall")]
pub struct FjallBackend {
    keyspace: Arc<fjall::Keyspace>,
    config_partition: Arc<fjall::Partition>,
    chunks_partition: Arc<fjall::Partition>,
}

#[cfg(feature = "fjall")]
impl FjallBackend {
    pub async fn new(db_path: std::path::PathBuf) -> BloomResult<Self> {
        let config = fjall::Config::new(db_path);
        let keyspace = Arc::new(config.open().map_err(|e| {
            BloomError::StorageError(format!("Failed to open Fjall DB: {}", e))
        })?);

        let options = fjall::PartitionCreateOptions::default();

        let config_partition = Arc::new(
            keyspace
                .open_partition("config", options.clone())
                .map_err(|e| {
                    BloomError::StorageError(format!(
                        "Failed to open config partition: {}",
                        e
                    ))
                })?,
        );

        let chunks_partition = Arc::new(
            keyspace.open_partition("chunks", options).map_err(|e| {
                BloomError::StorageError(format!(
                    "Failed to open chunks partition: {}",
                    e
                ))
            })?,
        );

        Ok(Self {
            keyspace,
            config_partition,
            chunks_partition,
        })
    }
}
