use super::{
    BloomError, BloomFilter, BloomFilterConfig, BloomResult, PersistenceConfig,
    PersistentBloomFilter, StorageBackend,
};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

#[cfg(feature = "fjall")]
pub struct FjallBackend {
    keyspace: Arc<fjall::Keyspace>,
    config_partition: Arc<fjall::Partition>,
    chunks_partition: Arc<fjall::Partition>,
}

#[cfg(feature = "fjall")]
#[async_trait]
impl StorageBackend for FjallBackend {
    async fn save_config(&self, config: &BloomFilterConfig) -> BloomResult<()> {
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

    async fn load_config(&self) -> BloomResult<Option<BloomFilterConfig>> {
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

    async fn save_snapshot(
        &self,
        chunks: &[(usize, Vec<u8>)],
    ) -> BloomResult<()> {
        for (chunk_id, chunk_data) in chunks {
            let key = format!("chunk_{}", chunk_id);
            self.chunks_partition
                .insert(&key, chunk_data)
                .map_err(|e| {
                    BloomError::StorageError(format!(
                        "Failed to save chunk: {}",
                        e
                    ))
                })?;
        }

        // Persist to disk
        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
                BloomError::StorageError(format!(
                    "Failed to persist chunks: {}",
                    e
                ))
            })?;

        Ok(())
    }

    async fn load_snapshot(&self) -> BloomResult<Option<Vec<(usize, Vec<u8>)>>> {
        let mut chunks = Vec::new();

        // Get iterator (no error handling here - iter() doesn't return Result)
        let iter = self.chunks_partition.iter();

        for item in iter {
            let (key, value) = item.map_err(|e| {
                BloomError::StorageError(format!("Failed to read chunk: {}", e))
            })?;

            // Parse chunk_id from key "chunk_123"
            if let Some(chunk_id_str) = key.strip_prefix(b"chunk_") {
                if let Ok(chunk_id_str) = std::str::from_utf8(chunk_id_str) {
                    if let Ok(chunk_id) = chunk_id_str.parse::<usize>() {
                        chunks.push((chunk_id, value.to_vec()));
                    }
                }
            }
        }

        if chunks.is_empty() {
            Ok(None)
        } else {
            // Sort chunks by ID for consistent ordering
            chunks.sort_by_key(|(id, _)| *id);
            Ok(Some(chunks))
        }
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
}
