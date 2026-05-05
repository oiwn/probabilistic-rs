use super::{CuckooError, CuckooFilterConfig, CuckooResult};
use async_trait::async_trait;
use std::sync::Arc;

#[cfg(feature = "fjall")]
#[derive(Clone)]
pub struct CuckooFjallBackend {
    db: Arc<fjall::Database>,
    config_partition: Arc<fjall::Keyspace>,
    chunks_partition: Arc<fjall::Keyspace>,
}

#[async_trait]
pub trait CuckooStorageBackend {
    async fn save_config(&self, config: &CuckooFilterConfig) -> CuckooResult<()>;
    async fn load_config(&self) -> CuckooResult<CuckooFilterConfig>;
    async fn save_chunks(&self, chunks: &[(usize, Vec<u8>)]) -> CuckooResult<()>;
    async fn load_chunks(&self) -> CuckooResult<Vec<(usize, Vec<u8>)>>;
}

#[cfg(feature = "fjall")]
#[async_trait]
impl CuckooStorageBackend for CuckooFjallBackend {
    async fn save_config(&self, config: &CuckooFilterConfig) -> CuckooResult<()> {
        let config_bytes = config.to_bytes()?;
        self.config_partition
            .insert("cuckoo_config", config_bytes)
            .map_err(|e| {
                CuckooError::StorageError(format!("Failed to save config: {e}"))
            })?;
        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            CuckooError::StorageError(format!("Failed to persist config: {e}"))
        })?;
        Ok(())
    }

    async fn load_config(&self) -> CuckooResult<CuckooFilterConfig> {
        match self.config_partition.get("cuckoo_config") {
            Ok(Some(config_bytes)) => {
                CuckooFilterConfig::from_bytes(&config_bytes)
            }
            Ok(None) => Err(CuckooError::ConfigNotFound),
            Err(e) => Err(CuckooError::StorageError(format!(
                "Failed to load config: {e}"
            ))),
        }
    }

    async fn save_chunks(&self, chunks: &[(usize, Vec<u8>)]) -> CuckooResult<()> {
        for (chunk_id, chunk_data) in chunks {
            let key = format!("chunk_{chunk_id}");
            self.chunks_partition
                .insert(&key, chunk_data)
                .map_err(|e| {
                    CuckooError::StorageError(format!(
                        "Failed to save chunk: {e}"
                    ))
                })?;
        }
        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            CuckooError::StorageError(format!("Failed to persist chunks: {e}"))
        })?;
        Ok(())
    }

    async fn load_chunks(&self) -> CuckooResult<Vec<(usize, Vec<u8>)>> {
        let mut chunks = Vec::new();
        for guard in self.chunks_partition.iter() {
            let (key, value) = guard.into_inner().map_err(|e| {
                CuckooError::StorageError(format!("Failed to read chunk: {e}"))
            })?;
            if let Some(chunk_id_str) = key.strip_prefix(b"chunk_")
                && let Ok(chunk_id_str) = std::str::from_utf8(chunk_id_str)
                && let Ok(chunk_id) = chunk_id_str.parse::<usize>()
            {
                chunks.push((chunk_id, value.to_vec()));
            }
        }
        chunks.sort_by_key(|(id, _)| *id);
        Ok(chunks)
    }
}

#[cfg(feature = "fjall")]
impl CuckooFjallBackend {
    pub async fn new(db_path: std::path::PathBuf) -> CuckooResult<Self> {
        let db =
            Arc::new(fjall::Database::builder(&db_path).open().map_err(|e| {
                CuckooError::StorageError(format!("Failed to open Fjall DB: {e}"))
            })?);
        let config_partition = Arc::new(
            db.keyspace("cuckoo_config", fjall::KeyspaceCreateOptions::default)
                .map_err(|e| {
                    CuckooError::StorageError(format!(
                        "Failed to open config partition: {e}"
                    ))
                })?,
        );
        let chunks_partition = Arc::new(
            db.keyspace("cuckoo_chunks", fjall::KeyspaceCreateOptions::default)
                .map_err(|e| {
                    CuckooError::StorageError(format!(
                        "Failed to open chunks partition: {e}"
                    ))
                })?,
        );
        Ok(Self {
            db,
            config_partition,
            chunks_partition,
        })
    }
}
