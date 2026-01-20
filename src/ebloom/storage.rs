use crate::ebloom::config::{ExpiringFilterConfig, LevelMetadata};
use crate::ebloom::error::EbloomError;
use async_trait::async_trait;
use bincode;
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
        // In-memory implementation doesn't actually save
        // In a real implementation, this would serialize the config
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
        // In-memory implementation would copy the metadata
        Ok(())
    }

    async fn load_level_metadata(&self) -> Result<Vec<LevelMetadata>> {
        Ok(self.metadata.clone())
    }

    async fn save_current_level(&self, _current_level: usize) -> Result<()> {
        // In-memory implementation would store this
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
        // In-memory implementation would store these chunks
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
        // In-memory implementation would store these chunks
        Ok(())
    }

    async fn load_dirty_chunks(
        &self,
        level: usize,
    ) -> Result<Vec<(usize, Vec<u8>)>> {
        Ok(self.dirty_chunks.get(&level).cloned().unwrap_or_default())
    }

    async fn delete_level(&self, _level: usize) -> Result<()> {
        // In-memory implementation would remove level data
        Ok(())
    }
}

/// Fjall storage backend for expiring bloom filters
#[cfg(feature = "fjall")]
pub struct FjallExpiringBackend {
    keyspace: Arc<fjall::Keyspace>,
    config_partition: Arc<fjall::Partition>,
    metadata_partition: Arc<fjall::Partition>,
    chunks_partitions: Vec<Arc<fjall::Partition>>,
    dirty_partitions: Vec<Arc<fjall::Partition>>,
    max_levels: usize,
}

#[cfg(feature = "fjall")]
impl FjallExpiringBackend {
    pub async fn new(
        db_path: std::path::PathBuf,
        max_levels: usize,
    ) -> Result<Self> {
        let config = fjall::Config::new(db_path);
        let keyspace = Arc::new(config.open().map_err(|e| {
            EbloomError::StorageError(format!("Failed to open Fjall DB: {e}"))
        })?);

        let options = fjall::PartitionCreateOptions::default();

        let config_partition = Arc::new(
            keyspace
                .open_partition("expiring_config", options.clone())
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open config partition: {e}",
                    ))
                })?,
        );

        let metadata_partition = Arc::new(
            keyspace
                .open_partition("level_metadata", options.clone())
                .map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to open metadata partition: {e}"
                    ))
                })?,
        );

        // Create partitions for each level's chunks and dirty chunks
        let mut chunks_partitions = Vec::with_capacity(max_levels);
        let mut dirty_partitions = Vec::with_capacity(max_levels);

        for level in 0..max_levels {
            let chunks_partition = Arc::new(
                keyspace
                    .open_partition(
                        &format!("level_{level}_chunks"),
                        options.clone(),
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
                keyspace
                    .open_partition(
                        &format!("level_{level}_dirty"),
                        options.clone(),
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
            keyspace,
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
    ) -> Option<&Arc<fjall::Partition>> {
        self.chunks_partitions.get(level)
    }

    fn get_dirty_partition(
        &self,
        level: usize,
    ) -> Option<&Arc<fjall::Partition>> {
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

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to persist config: {e}"
                ))
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
        // Serialize metadata as bytes (LevelMetadata should implement serialization)
        let metadata_bytes = self.serialize_metadata(metadata)?;

        self.metadata_partition
            .insert("level_metadata", metadata_bytes)
            .map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to save level metadata: {e}"
                ))
            })?;

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
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
            Ok(None) => Ok(vec![]), // No metadata yet
            Err(e) => Err(EbloomError::StorageError(format!(
                "Failed to load level metadata: {e}"
            ))),
        }
    }

    async fn save_current_level(&self, current_level: usize) -> Result<()> {
        // Store as single byte (u8)
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

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to persist current level: {e}"
                ))
            })?;

        Ok(())
    }

    async fn load_current_level(&self) -> Result<usize> {
        match self.config_partition.get("current_level") {
            Ok(Some(level_bytes)) => {
                if level_bytes.len() >= 1 {
                    Ok(level_bytes[0] as usize)
                } else {
                    Err(EbloomError::StorageError(
                        "Invalid current level data".to_string(),
                    ))
                }
            }
            Ok(None) => Ok(0), // Default to level 0
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

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
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
        let iter = partition.iter();

        for item in iter {
            let (key, value) = item.map_err(|e| {
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

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
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
        let iter = partition.iter();

        for item in iter {
            let (key, value) = item.map_err(|e| {
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

        // Clear all chunks for this level
        let iter = chunks_partition.iter();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to iterate level {} chunks for deletion: {e}",
                    level
                ))
            })?;

            if let Ok(key_str) = std::str::from_utf8(&key) {
                chunks_partition.remove(key_str).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete level {} chunk {}: {e}",
                        level, key_str
                    ))
                })?;
            }
        }

        // Clear all dirty chunks for this level
        let iter = dirty_partition.iter();
        for item in iter {
            let (key, _) = item.map_err(|e| {
                EbloomError::StorageError(format!(
                    "Failed to iterate level {} dirty chunks for deletion: {e}",
                    level
                ))
            })?;

            if let Ok(key_str) = std::str::from_utf8(&key) {
                dirty_partition.remove(key_str).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete level {} dirty chunk {}: {e}",
                        level, key_str
                    ))
                })?;
            }
        }

        self.keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(|e| {
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
        bincode::encode_to_vec(metadata, bincode::config::standard())
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }

    fn deserialize_metadata(&self, bytes: &[u8]) -> Result<Vec<LevelMetadata>> {
        bincode::decode_from_slice(bytes, bincode::config::standard())
            .map(|(metadata, _)| metadata)
            .map_err(|e| EbloomError::SerializationError(e.to_string()))
    }
}
