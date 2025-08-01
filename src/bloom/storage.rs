use super::{
    BloomError, BloomFilter, BloomFilterConfig, BloomResult, PersistenceConfig,
    PersistentBloomFilter, StorageBackend,
};
use async_trait::async_trait;
use std::sync::Arc;

#[cfg(feature = "fjall")]
pub struct FjallBackend {
    keyspace: Arc<fjall::Keyspace>,
    config_partition: Arc<fjall::Partition>,
    chunks_partition: Arc<fjall::Partition>,
}

#[async_trait]
impl PersistentBloomFilter for BloomFilter {
    async fn save_snapshot(&self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        println!("TODO: need to save snapshot!");
        Ok(())
    }

    async fn load_from_storage(&mut self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        println!("TODO: need to load filter data from storage!");
        Ok(())
    }

    fn is_dirty(&self) -> bool {
        self.dirty_chunks
            .as_ref()
            .map(|chunks| chunks.any())
            .unwrap_or(false)
    }
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

    pub async fn save_config(
        &self,
        config: &BloomFilterConfig,
    ) -> BloomResult<()> {
        let config_bytes = config.to_bytes()?;

        self.config_partition
            .insert("bloom_config", config_bytes)
            .map_err(|e| {
                BloomError::StorageError(format!("Failed to save config: {}", e))
            })?;

        // Ensure config is persisted to disk
        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
                BloomError::StorageError(format!(
                    "Failed to persist config: {}",
                    e
                ))
            })?;

        Ok(())
    }

    pub async fn load_config(&self) -> BloomResult<Option<BloomFilterConfig>> {
        match self.config_partition.get("bloom_config") {
            Ok(Some(config_bytes)) => {
                let config = BloomFilterConfig::from_bytes(&config_bytes)?;
                Ok(Some(config))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(BloomError::StorageError(format!(
                "Failed to load config: {}",
                e
            ))),
        }
    }
}
