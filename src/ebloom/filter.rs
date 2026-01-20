use crate::ebloom::config::{ExpiringFilterConfig, LevelMetadata};
use crate::ebloom::error::{EbloomError, Result};
use crate::ebloom::traits::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilterOps, ExpiringBloomFilterStats,
};
use crate::hash::{
    default_hash_function, optimal_bit_vector_size, optimal_num_hashes,
};
use bitvec::prelude::*;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "fjall")]
use crate::ebloom::storage::{ExpiringStorageBackend, FjallExpiringBackend};

pub struct ExpiringBloomFilter {
    config: ExpiringFilterConfig,
    bit_vector_size: usize,
    num_hashes: usize,

    // Level data
    levels: Arc<RwLock<Vec<BitVec<usize, Lsb0>>>>,

    // Metadata
    metadata: Arc<RwLock<Vec<LevelMetadata>>>,
    current_level: AtomicUsize,

    // Persistence support
    #[cfg(feature = "fjall")]
    storage: Option<FjallExpiringBackend>,
    chunk_size_bytes: usize,
    dirty_chunks: Option<Arc<RwLock<BitVec<usize, Lsb0>>>>,
}

impl ExpiringBloomFilter {
    pub fn new(config: ExpiringFilterConfig) -> Result<Self> {
        config.validate()?;

        let bit_vector_size =
            optimal_bit_vector_size(config.capacity_per_level, config.target_fpr);
        let num_hashes =
            optimal_num_hashes(config.capacity_per_level, bit_vector_size);

        let levels = (0..config.num_levels)
            .map(|_| bitvec![0; bit_vector_size])
            .collect();

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let metadata: Vec<LevelMetadata> = (0..config.num_levels)
            .map(|i| LevelMetadata {
                // Only the first level (current) has a timestamp (in milliseconds)
                // Others are not yet active (created_at = 0 means not initialized)
                created_at: if i == 0 { now_ms } else { 0 },
                insert_count: 0,
                last_snapshot_at: 0,
            })
            .collect();

        Ok(Self {
            config,
            bit_vector_size,
            num_hashes,
            levels: Arc::new(RwLock::new(levels)),
            metadata: Arc::new(RwLock::new(metadata)),
            current_level: AtomicUsize::new(0),
            #[cfg(feature = "fjall")]
            storage: None,
            chunk_size_bytes: 0,
            dirty_chunks: None,
        })
    }

    /// Internal builder for creating filter with optional persistence
    async fn build_filter(
        config: ExpiringFilterConfig,
        #[cfg(feature = "fjall")] storage: Option<FjallExpiringBackend>,
    ) -> Result<Self> {
        config.validate()?;

        let bit_vector_size =
            optimal_bit_vector_size(config.capacity_per_level, config.target_fpr);
        let num_hashes =
            optimal_num_hashes(config.capacity_per_level, bit_vector_size);

        let levels = (0..config.num_levels)
            .map(|_| bitvec![0; bit_vector_size])
            .collect();

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let metadata: Vec<LevelMetadata> = (0..config.num_levels)
            .map(|i| LevelMetadata {
                created_at: if i == 0 { now_ms } else { 0 },
                insert_count: 0,
                last_snapshot_at: 0,
            })
            .collect();

        // Setup dirty chunks if persistence enabled
        let (chunk_size_bytes, dirty_chunks) = if config.persistence.is_some() {
            let chunk_size =
                config.persistence.as_ref().unwrap().chunk_size_bytes;
            let chunk_count =
                (bit_vector_size + chunk_size * 8 - 1).div_ceil(chunk_size * 8);
            (
                chunk_size,
                Some(Arc::new(RwLock::new(bitvec![0; chunk_count]))),
            )
        } else {
            (0, None)
        };

        Ok(Self {
            config,
            bit_vector_size,
            num_hashes,
            levels: Arc::new(RwLock::new(levels)),
            metadata: Arc::new(RwLock::new(metadata)),
            current_level: AtomicUsize::new(0),
            #[cfg(feature = "fjall")]
            storage,
            chunk_size_bytes,
            dirty_chunks,
        })
    }

    /// Create new filter (overwrites existing DB if present)
    pub async fn create(config: ExpiringFilterConfig) -> Result<Self> {
        #[cfg(feature = "fjall")]
        let storage = if let Some(ref pers) = config.persistence {
            // Create parent directory if needed
            if let Some(parent) = pers.db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to create db directory: {e}"
                    ))
                })?;
            }

            // Delete existing DB if present
            if pers.db_path.exists() {
                std::fs::remove_dir_all(&pers.db_path).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete existing DB: {e}"
                    ))
                })?;
            }

            // Create Fjall backend
            let backend = FjallExpiringBackend::new(
                pers.db_path.clone(),
                config.num_levels,
            )
            .await?;

            // Save initial config
            backend.save_config(&config).await?;
            backend.save_current_level(0).await?;

            // Save initial metadata
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let metadata: Vec<LevelMetadata> = (0..config.num_levels)
                .map(|i| LevelMetadata {
                    created_at: if i == 0 { now_ms } else { 0 },
                    insert_count: 0,
                    last_snapshot_at: 0,
                })
                .collect();
            backend.save_level_metadata(&metadata).await?;

            Some(backend)
        } else {
            None
        };

        Self::build_filter(
            config,
            #[cfg(feature = "fjall")]
            storage,
        )
        .await
    }

    /// Load existing filter from DB
    #[cfg(feature = "fjall")]
    pub async fn load(db_path: std::path::PathBuf) -> Result<Self> {
        use crate::ebloom::storage::ExpiringStorageBackend;

        if !db_path.exists() {
            return Err(EbloomError::StorageError(format!(
                "Database does not exist at {db_path:?}"
            )));
        }

        // Load config first to get num_levels
        let temp_backend = FjallExpiringBackend::new(db_path.clone(), 10).await?;
        let config = temp_backend.load_config().await?;
        drop(temp_backend);

        // Create backend with correct num_levels
        let backend =
            FjallExpiringBackend::new(db_path, config.num_levels).await?;

        // Build filter
        let mut filter = Self::build_filter(config, Some(backend)).await?;

        // Reconstruct all levels from storage
        filter.reconstruct_from_storage().await?;

        Ok(filter)
    }

    /// Create or load (convenience method)
    pub async fn create_or_load(config: ExpiringFilterConfig) -> Result<Self> {
        #[cfg(feature = "fjall")]
        if let Some(ref pers) = config.persistence {
            if pers.db_path.exists() {
                Self::load(pers.db_path.clone()).await
            } else {
                Self::create(config).await
            }
        } else {
            Self::create(config).await
        }

        #[cfg(not(feature = "fjall"))]
        Self::create(config).await
    }

    /// Get current active level index
    pub fn get_active_level(&self) -> usize {
        self.current_level.load(Ordering::Relaxed)
    }

    /// Check if a level has expired based on its creation time
    pub fn is_level_expired(&self, level_index: usize) -> Result<bool> {
        let metadata = self.metadata.read().map_err(|_| {
            EbloomError::LockError("Failed to read metadata".to_string())
        })?;
        if let Some(level_meta) = metadata.get(level_index) {
            if level_meta.created_at == 0 {
                return Ok(false); // Not initialized yet
            }
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| EbloomError::TimeError(e.to_string()))?
                .as_millis() as u64;
            let level_age_ms = now_ms - level_meta.created_at; // Both in milliseconds
            Ok(level_age_ms > self.config.level_duration.as_millis() as u64)
        } else {
            Ok(false) // Index out of bounds
        }
    }

    /// Rotate levels: move to next level in circular fashion
    /// The new current level is cleared (oldest data expires)
    pub async fn rotate_levels(&self) -> Result<()> {
        let current_idx = self.current_level.load(Ordering::Relaxed);

        // Calculate next level index (circular)
        let new_current_idx = (current_idx + 1) % self.config.num_levels;

        // 1. Save FULL snapshot of current level (freeze it forever)
        self.save_full_snapshot().await?;

        // 2. Get write locks and clear the new current level
        {
            let mut levels = self.levels.write().map_err(|_| {
                EbloomError::LockError("Failed to write levels".to_string())
            })?;
            levels[new_current_idx].fill(false);
        }

        // 3. Delete new current level's old data from DB (both chunks AND dirty)
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            backend.delete_level(new_current_idx).await?;
        }

        // 4. Update metadata for the new current level
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| EbloomError::TimeError(e.to_string()))?
            .as_millis() as u64;

        let new_metadata = {
            let mut metadata = self.metadata.write().map_err(|_| {
                EbloomError::LockError("Failed to write metadata".to_string())
            })?;
            metadata[new_current_idx] = LevelMetadata {
                created_at: now_ms,
                insert_count: 0,
                last_snapshot_at: 0,
            };
            metadata.clone()
        };

        // 5. Save metadata and current level pointer to DB
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            backend.save_level_metadata(&new_metadata).await?;
            backend.save_current_level(new_current_idx).await?;
        }

        // 7. Update current level pointer in memory
        self.current_level.store(new_current_idx, Ordering::Relaxed);

        // 8. Clear dirty chunks tracker (for new current level)
        if let Some(ref dirty_chunks_arc) = self.dirty_chunks {
            let mut dirty = dirty_chunks_arc.write().map_err(|_| {
                EbloomError::LockError("Failed to write dirty chunks".to_string())
            })?;
            dirty.fill(false);
        }

        Ok(())
    }

    /// Clean up expired levels by rotating when current level expires
    pub async fn cleanup_expired_levels(&self) -> Result<()> {
        let current_level = self.current_level.load(Ordering::Relaxed);

        if self.is_level_expired(current_level)? {
            self.rotate_levels().await?;
        }

        Ok(())
    }

    /// Save incremental dirty chunks for CURRENT level (crash recovery)
    pub async fn save_snapshot(&self) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            let current_idx = self.current_level.load(Ordering::Relaxed);
            let dirty_chunks = self.extract_dirty_chunks()?;

            if !dirty_chunks.is_empty() {
                backend
                    .save_dirty_chunks(current_idx, &dirty_chunks)
                    .await?;

                // Update last_snapshot_at
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| EbloomError::TimeError(e.to_string()))?
                    .as_millis() as u64;

                let updated_metadata = {
                    let mut metadata = self.metadata.write().map_err(|_| {
                        EbloomError::LockError(
                            "Failed to write metadata".to_string(),
                        )
                    })?;
                    metadata[current_idx].last_snapshot_at = now_ms;
                    metadata.clone()
                };

                backend.save_level_metadata(&updated_metadata).await?;
            }
        }
        Ok(())
    }

    /// Save full snapshot of CURRENT level (called on rotation)
    async fn save_full_snapshot(&self) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            let current_idx = self.current_level.load(Ordering::Relaxed);
            let chunks = self.extract_all_chunks()?;

            backend.save_level_chunks(current_idx, &chunks).await?;

            // Update last_snapshot_at
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| EbloomError::TimeError(e.to_string()))?
                .as_millis() as u64;

            let updated_metadata = {
                let mut metadata = self.metadata.write().map_err(|_| {
                    EbloomError::LockError("Failed to write metadata".to_string())
                })?;
                metadata[current_idx].last_snapshot_at = now_ms;
                metadata.clone()
            };

            backend.save_level_metadata(&updated_metadata).await?;
        }
        Ok(())
    }

    /// Extract dirty chunks for current level only
    fn extract_dirty_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>> {
        let mut chunks = Vec::new();

        if let Some(ref dirty_chunks_arc) = self.dirty_chunks {
            let current_idx = self.current_level.load(Ordering::Relaxed);
            let levels = self.levels.read().map_err(|_| {
                EbloomError::LockError("Failed to read levels".to_string())
            })?;
            let dirty = dirty_chunks_arc.read().map_err(|_| {
                EbloomError::LockError("Failed to read dirty chunks".to_string())
            })?;

            let chunk_size_bits = self.chunk_size_bytes * 8;

            for chunk_id in 0..dirty.len() {
                if dirty[chunk_id] {
                    let chunk_data = extract_chunk_bytes(
                        &levels[current_idx],
                        chunk_id,
                        chunk_size_bits,
                    );
                    chunks.push((chunk_id, chunk_data));
                }
            }
        }

        Ok(chunks)
    }

    /// Extract all chunks for current level only
    fn extract_all_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>> {
        let current_idx = self.current_level.load(Ordering::Relaxed);
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError("Failed to read levels".to_string())
        })?;

        let chunk_size_bits = self.chunk_size_bytes * 8;
        let num_chunks = (self.bit_vector_size + chunk_size_bits - 1)
            .div_ceil(chunk_size_bits);

        let mut chunks = Vec::new();
        for chunk_id in 0..num_chunks {
            let chunk_data = extract_chunk_bytes(
                &levels[current_idx],
                chunk_id,
                chunk_size_bits,
            );
            chunks.push((chunk_id, chunk_data));
        }

        Ok(chunks)
    }

    /// Reconstruct all N levels from storage (on load)
    async fn reconstruct_from_storage(&mut self) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            use crate::ebloom::storage::ExpiringStorageBackend;

            // Load current level index
            let current_idx = backend.load_current_level().await?;
            self.current_level.store(current_idx, Ordering::Relaxed);

            // Load all data from DB first (no locks held)
            let loaded_metadata = backend.load_level_metadata().await?;

            // Load all N levels from DB
            let mut loaded_levels_data = Vec::new();
            for level_idx in 0..self.config.num_levels {
                // Try dirty chunks first, fallback to full chunks
                let dirty_chunks = backend.load_dirty_chunks(level_idx).await?;
                if !dirty_chunks.is_empty() {
                    loaded_levels_data.push((level_idx, dirty_chunks));
                } else {
                    let chunks = backend.load_level_chunks(level_idx).await?;
                    loaded_levels_data.push((level_idx, chunks));
                }
            }

            // Now acquire locks and write data (no await points)
            {
                let mut metadata = self.metadata.write().map_err(|_| {
                    EbloomError::LockError("Failed to write metadata".to_string())
                })?;
                *metadata = loaded_metadata;
            }

            let mut levels = self.levels.write().map_err(|_| {
                EbloomError::LockError("Failed to write levels".to_string())
            })?;

            for (level_idx, chunks) in loaded_levels_data {
                if !chunks.is_empty() {
                    reconstruct_level_from_chunks(
                        &mut levels[level_idx],
                        &chunks,
                        self.chunk_size_bytes,
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// Helper: extract chunk bytes from BitVec
fn extract_chunk_bytes(
    bits: &BitVec<usize, Lsb0>,
    chunk_id: usize,
    chunk_size_bits: usize,
) -> Vec<u8> {
    let start_bit = chunk_id * chunk_size_bits;
    if start_bit >= bits.len() {
        return Vec::new();
    }

    let end_bit = std::cmp::min(start_bit + chunk_size_bits, bits.len());
    let chunk_bits = &bits[start_bit..end_bit];

    let mut bytes = Vec::new();
    for byte_chunk in chunk_bits.chunks(8) {
        let mut byte = 0u8;
        for (bit_pos, bit) in byte_chunk.iter().enumerate() {
            if *bit {
                byte |= 1 << bit_pos;
            }
        }
        bytes.push(byte);
    }
    bytes
}

/// Helper: reconstruct level from chunks
fn reconstruct_level_from_chunks(
    level_bits: &mut BitVec<usize, Lsb0>,
    chunks: &[(usize, Vec<u8>)],
    chunk_size_bytes: usize,
) -> Result<()> {
    let chunk_size_bits = chunk_size_bytes * 8;

    for (chunk_id, chunk_bytes) in chunks {
        let start_bit = chunk_id * chunk_size_bits;
        for (byte_idx, &byte) in chunk_bytes.iter().enumerate() {
            for bit_pos in 0..8 {
                let bit_idx = start_bit + byte_idx * 8 + bit_pos;
                if bit_idx < level_bits.len() {
                    let bit_value = (byte & (1 << bit_pos)) != 0;
                    level_bits.set(bit_idx, bit_value);
                }
            }
        }
    }
    Ok(())
}

/// Helper function to insert an item into the filter with already-held locks
fn insert_internal(
    item: &[u8],
    current_level_idx: usize,
    num_hashes: usize,
    bit_vector_size: usize,
    chunk_size_bytes: usize,
    dirty: Option<&mut BitVec<usize, Lsb0>>,
    levels: &mut [BitVec<usize, Lsb0>],
) -> Result<()> {
    // Calculate hash indices
    let indices = default_hash_function(item, num_hashes, bit_vector_size);

    // Mark dirty chunks (if dirty tracker provided)
    if let Some(dirty_bits) = dirty {
        for &idx in &indices {
            let chunk_id = (idx as usize) / (chunk_size_bytes * 8);
            if chunk_id < dirty_bits.len() {
                dirty_bits.set(chunk_id, true);
            }
        }
    }

    // Insert into current level only
    if let Some(current_level) = levels.get_mut(current_level_idx) {
        for idx in indices {
            let idx = idx as usize;
            if idx >= bit_vector_size {
                return Err(EbloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: bit_vector_size,
                });
            }
            current_level.set(idx, true);
        }
    }

    Ok(())
}

/// Helper function to check if an item exists with already-held lock
fn contains_internal(
    item: &[u8],
    num_hashes: usize,
    bit_vector_size: usize,
    levels: &[BitVec<usize, Lsb0>],
) -> Result<bool> {
    // Calculate hash indices
    let indices = default_hash_function(item, num_hashes, bit_vector_size);

    // Check all levels
    for level in levels.iter() {
        let mut all_bits_set = true;

        for idx in &indices {
            let idx = *idx as usize;
            if idx >= bit_vector_size {
                return Err(EbloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: bit_vector_size,
                });
            }

            if !level[idx] {
                all_bits_set = false;
                break;
            }
        }

        // If found in any level, return true
        if all_bits_set {
            return Ok(true);
        }
    }

    // Not found in any level
    Ok(false)
}

#[async_trait::async_trait]
impl ExpiringBloomFilterOps for ExpiringBloomFilter {
    fn insert(&self, item: &[u8]) -> Result<()> {
        // Get the current level index
        let current_level_idx = self.current_level.load(Ordering::Relaxed);

        // Mark dirty chunks (if persistence enabled)
        let mut dirty_guard = if let Some(ref dirty_chunks_arc) =
            self.dirty_chunks
        {
            Some(dirty_chunks_arc.write().map_err(|_| {
                EbloomError::LockError("Failed to write dirty chunks".to_string())
            })?)
        } else {
            None
        };

        // Get write lock on levels
        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        // Perform the insertion
        insert_internal(
            item,
            current_level_idx,
            self.num_hashes,
            self.bit_vector_size,
            self.chunk_size_bytes,
            dirty_guard.as_deref_mut(),
            &mut levels,
        )?;

        // Update metadata for current level
        let mut metadata = self.metadata.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on metadata".to_string(),
            )
        })?;
        if let Some(meta) = metadata.get_mut(current_level_idx) {
            meta.insert_count += 1;
        }

        Ok(())
    }

    fn contains(&self, item: &[u8]) -> Result<bool> {
        // Get read lock on levels
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire read lock on levels".to_string(),
            )
        })?;

        contains_internal(item, self.num_hashes, self.bit_vector_size, &levels)
    }

    fn clear(&self) -> Result<()> {
        // Get write lock on levels
        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        // Clear all levels
        for level in levels.iter_mut() {
            level.fill(false);
        }

        // Reset all metadata
        let mut metadata = self.metadata.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on metadata".to_string(),
            )
        })?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| EbloomError::TimeError(e.to_string()))?
            .as_millis() as u64;

        for meta in metadata.iter_mut() {
            meta.created_at = now_ms; // Store in milliseconds
            meta.insert_count = 0;
            meta.last_snapshot_at = 0;
        }

        // Reset to level 0 as current
        self.current_level.store(0, Ordering::Relaxed);

        Ok(())
    }

    async fn cleanup_expired_levels(&self) -> Result<()> {
        let current_level = self.current_level.load(Ordering::Relaxed);

        if self.is_level_expired(current_level)? {
            self.rotate_levels().await?;
        }

        Ok(())
    }
}

impl ExpiringBloomFilterStats for ExpiringBloomFilter {
    fn capacity_per_level(&self) -> usize {
        self.config.capacity_per_level
    }

    fn target_fpr(&self) -> f64 {
        self.config.target_fpr
    }

    fn total_insert_count(&self) -> u64 {
        let metadata = self.metadata.read().unwrap();
        metadata.iter().map(|m| m.insert_count).sum()
    }

    fn active_levels(&self) -> usize {
        self.config.num_levels
    }

    fn num_levels(&self) -> usize {
        self.config.num_levels
    }
}

impl BulkExpiringBloomFilterOps for ExpiringBloomFilter {
    fn insert_bulk(&self, items: &[&[u8]]) -> Result<()> {
        // Get the current level index
        let current_level_idx = self.current_level.load(Ordering::Relaxed);

        // Mark dirty chunks (if persistence enabled)
        let mut dirty_guard = if let Some(ref dirty_chunks_arc) =
            self.dirty_chunks
        {
            Some(dirty_chunks_arc.write().map_err(|_| {
                EbloomError::LockError("Failed to write dirty chunks".to_string())
            })?)
        } else {
            None
        };

        // Get write lock on levels
        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        // Perform all insertions with single lock
        for item in items {
            insert_internal(
                item,
                current_level_idx,
                self.num_hashes,
                self.bit_vector_size,
                self.chunk_size_bytes,
                dirty_guard.as_deref_mut(),
                &mut levels,
            )?;
        }

        // Update metadata for current level with total count
        let mut metadata = self.metadata.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on metadata".to_string(),
            )
        })?;
        if let Some(meta) = metadata.get_mut(current_level_idx) {
            meta.insert_count += items.len() as u64;
        }

        Ok(())
    }

    fn contains_bulk(&self, items: &[&[u8]]) -> Result<Vec<bool>> {
        // Get read lock on levels once
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire read lock on levels".to_string(),
            )
        })?;

        // Check all items with single lock
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            results.push(contains_internal(
                item,
                self.num_hashes,
                self.bit_vector_size,
                &levels,
            )?);
        }
        Ok(results)
    }
}
