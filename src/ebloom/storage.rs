use crate::ebloom::config::{ExpiringFilterConfig, LevelMetadata};
use crate::ebloom::error::EbloomError;
use async_trait::async_trait;
use std::sync::Arc;

type Result<T> = std::result::Result<T, EbloomError>;

/// Storage backend trait for expiring bloom filter persistence
#[async_trait]
pub trait ExpiringStorageBackend {
    /// Save the expiring filter configuration
    async fn save_config(&self, config: &ExpiringFilterConfig) -> Result<()>;

    /// Load the expiring filter configuration
    async fn load_config(&self) -> Result<ExpiringFilterConfig>;

    /// Save metadata for all levels
    async fn save_level_metadata(&self, metadata: &[LevelMetadata])
    -> Result<()>;

    /// Load metadata for all levels
    async fn load_level_metadata(&self) -> Result<Vec<LevelMetadata>>;

    /// Save current level index
    async fn save_current_level(&self, current_level: usize) -> Result<()>;

    /// Load current level index
    async fn load_current_level(&self) -> Result<usize>;

    /// Save chunks for a specific level
    async fn save_level_chunks(
        &self,
        level: usize,
        chunks: &[(usize, Vec<u8>)],
    ) -> Result<()>;

    /// Load chunks for a specific level
    async fn load_level_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>>;

    /// Save dirty chunks for a specific level
    async fn save_dirty_chunks(
        &self,
        level: usize,
        dirty_chunks: &[(usize, Vec<u8>)],
    ) -> Result<()>;

    /// Load dirty chunks for a specific level
    async fn load_dirty_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>>;

    /// Delete all data for a specific level (during rotation)
    async fn delete_level(&self, level: usize) -> Result<()>;
}

/// In-memory storage backend for testing
pub struct InMemoryExpiringStorage {
    config: Option<ExpiringFilterConfig>,
    metadata: Vec<LevelMetadata>,
    current_level: usize,
    level_chunks: std::collections::HashMap<usize, Vec<(usize, Vec<u8>)>>,
    dirty_chunks: std::collections::HashMap<usize, Vec<(usize, Vec<u8>)>>,
}

impl Default for InMemoryExpiringStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryExpiringStorage {
    pub fn new() -> Self {
        Self {
            config: None,
            metadata: Vec::new(),
            current_level: 0,
            level_chunks: std::collections::HashMap::new(),
            dirty_chunks: std::collections::HashMap::new(),
        }
    }
}

#[async_trait]
impl ExpiringStorageBackend for InMemoryExpiringStorage {
    async fn save_config(&self, _config: &ExpiringFilterConfig) -> Result<()> {
        Ok(())
    }

    async fn load_config(&self) -> Result<ExpiringFilterConfig> {
        Ok(self
            .config
            .as_ref()
            .ok_or_else(|| {
                EbloomError::ConfigError("No config found".to_string())
            })?
            .clone())
    }

    async fn save_level_metadata(
        &self,
        _metadata: &[LevelMetadata],
    ) -> Result<()> {
        Ok(())
    }

    async fn load_level_metadata(&self) -> Result<Vec<LevelMetadata>> {
        Ok(self.metadata.clone())
    }

    async fn save_current_level(&self, _current_level: usize) -> Result<()> {
        Ok(())
    }

    async fn load_current_level(&self) -> Result<usize> {
        Ok(self.current_level)
    }

    async fn save_level_chunks(
        &self,
        _level: usize,
        _chunks: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        Ok(())
    }

    async fn load_level_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>> {
        Ok(self.level_chunks.get(&level).cloned().unwrap_or_default())
    }

    async fn save_dirty_chunks(
        &self,
        _level: usize,
        _dirty_chunks: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        Ok(())
    }

    async fn load_dirty_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>> {
        Ok(self.dirty_chunks.get(&level).cloned().unwrap_or_default())
    }

    async fn delete_level(&self, _level: usize) -> Result<()> {
        Ok(())
    }
}

/// Fjall storage backend for expiring bloom filters
#[cfg(feature = "fjall")]
#[derive(Clone)]
pub struct FjallExpiringBackend {
    db: Arc<fjall::Database>,
    config_partition: Arc<fjall::Keyspace>,
    metadata_partition: Arc<fjall::Keyspace>,
    chunks_partitions: Vec<Arc<fjall::Keyspace>>,
    dirty_partitions: Vec<Arc<fjall::Keyspace>>,
    max_levels: usize,
}

#[cfg(feature = "fjall")]
impl FjallExpiringBackend {
    pub async fn new(
        db_path: std::path::PathBuf,
        max_levels: usize,
    ) -> Result<Self> {
        let db =
            Arc::new(fjall::Database::builder(&db_path).open().map_err(|e| {
                EbloomError::StorageError(format!("Failed to open Fjall DB: {e}"))
            })?);

        let config_partition = Arc::new(
            db.keyspace("expiring_config", fjall::KeyspaceCreateOptions::default)
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open config partition: {e}",
                    ))
                })?,
        );

        let metadata_partition = Arc::new(
            db.keyspace("level_metadata", fjall::KeyspaceCreateOptions::default)
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open metadata partition: {e}"
                    ))
                })?,
        );

        let mut chunks_partitions = Vec::with_capacity(max_levels);
        let mut dirty_partitions = Vec::with_capacity(max_levels);

        for level in 0..max_levels {
            let chunks_partition = Arc::new(
                db.keyspace(
                    &format!("level_{level}_chunks"),
                    fjall::KeyspaceCreateOptions::default,
                )
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open level {} chunks partition: {e}",
                        level
                    ))
                })?,
            );
            chunks_partitions.push(chunks_partition);

            let dirty_partition = Arc::new(
                db.keyspace(
                    &format!("level_{level}_dirty"),
                    fjall::KeyspaceCreateOptions::default,
                )
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open level {} dirty partition: {e}",
                        level
                    ))
                })?,
            );
            dirty_partitions.push(dirty_partition);
        }

        Ok(Self {
            db,
            config_partition,
            metadata_partition,
            chunks_partitions,
            dirty_partitions,
            max_levels,
        })
    }

    fn get_chunks_partition(
        &self,
        level: usize,
    ) -> Option<&Arc<fjall::Keyspace>> {
        self.chunks_partitions.get(level)
    }

    fn get_dirty_partition(&self, level: usize) -> Option<&Arc<fjall::Keyspace>> {
        self.dirty_partitions.get(level)
    }
}

#[cfg(feature = "fjall")]
#[async_trait]
impl ExpiringStorageBackend for FjallExpiringBackend {
    async fn save_config(&self, config: &ExpiringFilterConfig) -> Result<()> {
        let config_bytes = config.to_bytes()?;

        self.config_partition
            .insert("expiring_bloom_config", config_bytes)
            .map_err(|e| {
                EbloomError::StorageError(format!("Failed to save config: {e}"))
            })?;

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!("Failed to persist config: {e}"))
        })?;

        Ok(())
    }

    async fn load_config(&self) -> Result<ExpiringFilterConfig> {
        match self.config_partition.get("expiring_bloom_config") {
            Ok(Some(config_bytes)) => {
                let config = ExpiringFilterConfig::from_bytes(&config_bytes)?;
                Ok(config)
            }
            Ok(None) => {
                Err(EbloomError::ConfigError("Config not found".to_string()))
            }
            Err(e) => Err(EbloomError::StorageError(format!(
                "Failed to load config: {e}"
            ))),
        }
    }

    async fn save_level_metadata(
        &self,
        metadata: &[LevelMetadata],
    ) -> Result<()> {
        let metadata_bytes = self.serialize_metadata(metadata)?;

        self.metadata_partition
            .insert("level_metadata", metadata_bytes)
            .map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to save level metadata: {e}"
                ))
            })?;

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!(
                "Failed to persist level metadata: {e}"
            ))
        })?;

        Ok(())
    }

    async fn load_level_metadata(&self) -> Result<Vec<LevelMetadata>> {
        match self.metadata_partition.get("level_metadata") {
            Ok(Some(metadata_bytes)) => {
                let metadata = self.deserialize_metadata(&metadata_bytes)?;
                Ok(metadata)
            }
            Ok(None) => Ok(vec![]),
            Err(e) => Err(EbloomError::StorageError(format!(
                "Failed to load level metadata: {e}"
            ))),
        }
    }

    async fn save_current_level(&self, current_level: usize) -> Result<()> {
        if current_level > 255 {
            return Err(EbloomError::InvalidLevel {
                level: current_level,
                max_levels: 255,
            });
        }
        let level_bytes = (current_level as u8).to_le_bytes();

        self.config_partition
            .insert("current_level", level_bytes)
            .map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to save current level: {e}"
                ))
            })?;

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!(
                "Failed to persist current level: {e}"
            ))
        })?;

        Ok(())
    }

    async fn load_current_level(&self) -> Result<usize> {
        match self.config_partition.get("current_level") {
            Ok(Some(level_bytes)) => {
                if !level_bytes.is_empty() {
                    Ok(level_bytes[0] as usize)
                } else {
                    Err(EbloomError::StorageError(
                        "Invalid current level data".to_string(),
                    ))
                }
            }
            Ok(None) => Ok(0),
            Err(e) => Err(EbloomError::StorageError(format!(
                "Failed to load current level: {e}"
            ))),
        }
    }

    async fn save_level_chunks(
        &self,
        level: usize,
        chunks: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        let Some(partition) = self.get_chunks_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        for (chunk_id, chunk_data) in chunks {
            let key = format!("chunk_{chunk_id}");
            partition.insert(&key, chunk_data).map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to save level {} chunk {}: {e}",
                    level, chunk_id
                ))
            })?;
        }

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!(
                "Failed to persist level {} chunks: {e}",
                level
            ))
        })?;

        Ok(())
    }

    async fn load_level_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>> {
        let Some(partition) = self.get_chunks_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        let mut chunks = Vec::new();

        for guard in partition.iter() {
            let (key, value) = guard.into_inner().map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to read level {} chunk: {e}",
                    level
                ))
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

    async fn save_dirty_chunks(
        &self,
        level: usize,
        dirty_chunks: &[(usize, Vec<u8>)],
    ) -> Result<()> {
        let Some(partition) = self.get_dirty_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        for (chunk_id, chunk_data) in dirty_chunks {
            let key = format!("dirty_{chunk_id}");
            partition.insert(&key, chunk_data).map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to save level {} dirty chunk {}: {e}",
                    level, chunk_id
                ))
            })?;
        }

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!(
                "Failed to persist level {} dirty chunks: {e}",
                level
            ))
        })?;

        Ok(())
    }

    async fn load_dirty_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>> {
        let Some(partition) = self.get_dirty_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        let mut chunks = Vec::new();

        for guard in partition.iter() {
            let (key, value) = guard.into_inner().map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to read level {} dirty chunk: {e}",
                    level
                ))
            })?;

            if let Some(chunk_id_str) = key.strip_prefix(b"dirty_")
                && let Ok(chunk_id_str) = std::str::from_utf8(chunk_id_str)
                && let Ok(chunk_id) = chunk_id_str.parse::<usize>()
            {
                chunks.push((chunk_id, value.to_vec()));
            }
        }

        chunks.sort_by_key(|(id, _)| *id);
        Ok(chunks)
    }

    async fn delete_level(&self, level: usize) -> Result<()> {
        let Some(chunks_partition) = self.get_chunks_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        let Some(dirty_partition) = self.get_dirty_partition(level) else {
            return Err(EbloomError::InvalidLevel {
                level,
                max_levels: self.max_levels,
            });
        };

        let mut keys_to_delete_chunks = Vec::new();
        for guard in chunks_partition.iter() {
            let (key, _) = guard.into_inner().map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to iterate level {} chunks for deletion: {e}",
                    level
                ))
            })?;
            keys_to_delete_chunks.push(key.to_vec());
        }

        for key in keys_to_delete_chunks {
            if let Ok(key_str) = std::str::from_utf8(&key) {
                chunks_partition.remove(key_str).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete level {} chunk {}: {e}",
                        level, key_str
                    ))
                })?;
            }
        }

        let mut keys_to_delete_dirty = Vec::new();
        for guard in dirty_partition.iter() {
            let (key, _) = guard.into_inner().map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to iterate level {} dirty chunks for deletion: {e}",
                    level
                ))
            })?;
            keys_to_delete_dirty.push(key.to_vec());
        }

        for key in keys_to_delete_dirty {
            if let Ok(key_str) = std::str::from_utf8(&key) {
                dirty_partition.remove(key_str).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete level {} dirty chunk {}: {e}",
                        level, key_str
                    ))
                })?;
            }
        }

        self.db.persist(fjall::PersistMode::SyncAll).map_err(|e| {
            EbloomError::StorageError(format!(
                "Failed to persist level {} deletion: {e}",
                level
            ))
        })?;

        Ok(())
    }
}

#[cfg(feature = "fjall")]
impl FjallExpiringBackend {
    fn serialize_metadata(&self, metadata: &[LevelMetadata]) -> Result<Vec<u8>> {
        postcard::to_allocvec(metadata)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }

    fn deserialize_metadata(&self, bytes: &[u8]) -> Result<Vec<LevelMetadata>> {
        postcard::from_bytes(bytes)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }
}
