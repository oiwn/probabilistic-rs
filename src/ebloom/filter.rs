use crate::ebloom::config::{ExpiringFilterConfig, LevelMetadata};
use crate::ebloom::error::{EbloomError, Result};
use crate::ebloom::traits::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilterOps, ExpiringBloomFilterStats,
};
use crate::hash::{
    default_hash_function, optimal_bit_vector_size, optimal_num_hashes,
};
use crate::snapshot::SnapshotState;
use bitvec::prelude::*;
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "fjall")]
use std::sync::Weak;
#[cfg(feature = "fjall")]
use tokio::{sync::Notify, task::JoinHandle, time::Duration};

#[cfg(feature = "fjall")]
use crate::ebloom::storage::{ExpiringStorageBackend, FjallExpiringBackend};

// ---------------------------------------------------------------------------
// Persistence handle — all fjall-gated state in one place
// ---------------------------------------------------------------------------

/// All persistence-related state for an `ExpiringBloomFilter`.
/// Only present when a `db_path` is configured and the `fjall` feature is enabled.
#[cfg(feature = "fjall")]
struct ExpiringPersistenceHandle {
    storage: Arc<FjallExpiringBackend>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    snapshot_state: Arc<SnapshotState>,
    // Only `Some` when `auto_snapshot` is true:
    shutdown_signal: Option<Arc<AtomicBool>>,
    snapshot_notify: Option<Arc<Notify>>,
    task_handle: Option<JoinHandle<()>>,
}

/// Arguments bundled for the background snapshot task.
#[cfg(feature = "fjall")]
struct ExpiringSnapshotLoopCtx {
    levels: Arc<RwLock<Vec<BitVec<usize, Lsb0>>>>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    metadata: Arc<RwLock<Vec<LevelMetadata>>>,
    current_level: Arc<AtomicUsize>,
    chunk_size_bytes: usize,
    state: Arc<SnapshotState>,
    interval: Duration,
    shutdown: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

// ---------------------------------------------------------------------------
// ExpiringBloomFilter struct
// ---------------------------------------------------------------------------

pub struct ExpiringBloomFilter {
    config: ExpiringFilterConfig,
    bit_vector_size: usize,
    num_hashes: usize,

    // Level data
    levels: Arc<RwLock<Vec<BitVec<usize, Lsb0>>>>,

    // Metadata
    metadata: Arc<RwLock<Vec<LevelMetadata>>>,
    current_level: Arc<AtomicUsize>,

    /// All persistence state lives here; one `#[cfg]` instead of five.
    #[cfg(feature = "fjall")]
    persistence: Option<ExpiringPersistenceHandle>,
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
            current_level: Arc::new(AtomicUsize::new(0)),
            #[cfg(feature = "fjall")]
            persistence: None,
        })
    }

    /// Internal builder for creating filter with optional persistence
    async fn build_filter(
        config: ExpiringFilterConfig,
        #[cfg(feature = "fjall")] storage: Option<Arc<FjallExpiringBackend>>,
    ) -> Result<Self> {
        config.validate()?;

        let bit_vector_size =
            optimal_bit_vector_size(config.capacity_per_level, config.target_fpr);
        let num_hashes =
            optimal_num_hashes(config.capacity_per_level, bit_vector_size);

        let levels = Arc::new(RwLock::new(
            (0..config.num_levels)
                .map(|_| bitvec![0; bit_vector_size])
                .collect::<Vec<_>>(),
        ));

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let metadata = Arc::new(RwLock::new(
            (0..config.num_levels)
                .map(|i| LevelMetadata {
                    created_at: if i == 0 { now_ms } else { 0 },
                    insert_count: 0,
                    last_snapshot_at: 0,
                })
                .collect::<Vec<_>>(),
        ));

        let current_level = Arc::new(AtomicUsize::new(0));

        #[cfg(feature = "fjall")]
        let persistence = if let Some(s) = storage {
            let chunk_size_bytes = config
                .persistence
                .as_ref()
                .map(|p| p.chunk_size_bytes)
                .unwrap_or(4096);
            let chunk_count = (bit_vector_size + chunk_size_bytes * 8 - 1)
                .div_ceil(chunk_size_bytes * 8);
            let dirty_chunks = Arc::new(RwLock::new(bitvec![0; chunk_count]));
            let snapshot_state = SnapshotState::new();

            let (shutdown_signal, snapshot_notify, task_handle) =
                if let Some(pers) = &config.persistence {
                    if pers.auto_snapshot {
                        let shutdown = Arc::new(AtomicBool::new(false));
                        let notify = Arc::new(Notify::new());
                        let handle =
                            tokio::spawn(background_snapshot_loop_ebloom(
                                Arc::downgrade(&s),
                                ExpiringSnapshotLoopCtx {
                                    levels: Arc::clone(&levels),
                                    dirty_chunks: Arc::clone(&dirty_chunks),
                                    metadata: Arc::clone(&metadata),
                                    current_level: Arc::clone(&current_level),
                                    chunk_size_bytes,
                                    state: Arc::clone(&snapshot_state),
                                    interval: pers.snapshot_interval,
                                    shutdown: Arc::clone(&shutdown),
                                    notify: Arc::clone(&notify),
                                },
                            ));
                        (Some(shutdown), Some(notify), Some(handle))
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };

            Some(ExpiringPersistenceHandle {
                storage: s,
                dirty_chunks,
                chunk_size_bytes,
                snapshot_state,
                shutdown_signal,
                snapshot_notify,
                task_handle,
            })
        } else {
            None
        };

        Ok(Self {
            config,
            bit_vector_size,
            num_hashes,
            levels,
            metadata,
            current_level,
            #[cfg(feature = "fjall")]
            persistence,
        })
    }

    /// Create new filter (overwrites existing DB if present)
    pub async fn create(config: ExpiringFilterConfig) -> Result<Self> {
        #[cfg(feature = "fjall")]
        let storage = if let Some(ref pers) = config.persistence {
            if let Some(parent) = pers.db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to create db directory: {e}"
                    ))
                })?;
            }

            if pers.db_path.exists() {
                std::fs::remove_dir_all(&pers.db_path).map_err(|e| {
                    EbloomError::StorageError(format!(
                        "Failed to delete existing DB: {e}"
                    ))
                })?;
            }

            let backend = FjallExpiringBackend::new(
                pers.db_path.clone(),
                config.num_levels,
            )
            .await?;

            backend.save_config(&config).await?;
            backend.save_current_level(0).await?;

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

            Some(Arc::new(backend))
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

        let temp_backend = FjallExpiringBackend::new(db_path.clone(), 10).await?;
        let config = temp_backend.load_config().await?;
        drop(temp_backend);

        let backend =
            FjallExpiringBackend::new(db_path, config.num_levels).await?;

        let mut filter =
            Self::build_filter(config, Some(Arc::new(backend))).await?;

        filter.reconstruct_from_storage().await?;

        Ok(filter)
    }

    /// Create or load (convenience method)
    pub async fn create_or_load(config: ExpiringFilterConfig) -> Result<Self> {
        #[cfg(feature = "fjall")]
        if let Some(ref pers) = config.persistence
            && pers.db_path.exists()
        {
            return Self::load(pers.db_path.clone()).await;
        }
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
                return Ok(false);
            }
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| EbloomError::TimeError(e.to_string()))?
                .as_millis() as u64;
            let level_age_ms = now_ms - level_meta.created_at;
            Ok(level_age_ms > self.config.level_duration.as_millis() as u64)
        } else {
            Ok(false)
        }
    }

    /// Rotate levels: move to next level in circular fashion.
    /// The new current level is cleared (oldest data expires).
    pub async fn rotate_levels(&self) -> Result<()> {
        let current_idx = self.current_level.load(Ordering::Relaxed);
        let new_current_idx = (current_idx + 1) % self.config.num_levels;

        // 1. Save FULL snapshot of current level (freeze it)
        self.save_full_snapshot().await?;

        // 2. Clear the new current level
        {
            let mut levels = self.levels.write().map_err(|_| {
                EbloomError::LockError("Failed to write levels".to_string())
            })?;
            levels[new_current_idx].fill(false);
        }

        // 3. Delete new current level's old data from DB
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.storage.delete_level(new_current_idx).await?;
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

        // 5. Persist metadata and current level pointer
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.storage.save_level_metadata(&new_metadata).await?;
            ph.storage.save_current_level(new_current_idx).await?;
        }

        // 6. Update current level pointer in memory
        self.current_level.store(new_current_idx, Ordering::Relaxed);

        // 7. Clear dirty chunks tracker
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.dirty_chunks
                .write()
                .map_err(|_| {
                    EbloomError::LockError(
                        "Failed to write dirty chunks".to_string(),
                    )
                })?
                .fill(false);
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

    /// Save incremental dirty chunks for CURRENT level (crash recovery).
    ///
    /// On success, clears the dirty-chunk tracker and resets the insert counter.
    /// On failure, poisons the filter — subsequent writes will return the stored error.
    pub async fn save_snapshot(&self) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            if let Some(err) = ph.snapshot_state.check_poison() {
                return Err(EbloomError::StorageError(err));
            }

            let current_idx = self.current_level.load(Ordering::Relaxed);
            let dirty_chunks = self.extract_dirty_chunks()?;

            if dirty_chunks.is_empty() {
                return Ok(());
            }

            match ph
                .storage
                .save_dirty_chunks(current_idx, &dirty_chunks)
                .await
            {
                Ok(()) => {
                    ph.dirty_chunks
                        .write()
                        .map_err(|_| {
                            EbloomError::LockError(
                                "Failed to write dirty chunks".into(),
                            )
                        })?
                        .fill(false);

                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| EbloomError::TimeError(e.to_string()))?
                        .as_millis() as u64;

                    let updated_metadata = {
                        let mut metadata =
                            self.metadata.write().map_err(|_| {
                                EbloomError::LockError(
                                    "Failed to write metadata".to_string(),
                                )
                            })?;
                        metadata[current_idx].last_snapshot_at = now_ms;
                        metadata.clone()
                    };

                    if let Err(e) =
                        ph.storage.save_level_metadata(&updated_metadata).await
                    {
                        ph.snapshot_state.on_snapshot_failure(&e.to_string());
                        return Err(e);
                    }

                    ph.snapshot_state.on_snapshot_success();
                }
                Err(e) => {
                    ph.snapshot_state.on_snapshot_failure(&e.to_string());
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Save full snapshot of CURRENT level (called on rotation)
    async fn save_full_snapshot(&self) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            let current_idx = self.current_level.load(Ordering::Relaxed);
            let chunks = self.extract_all_chunks()?;

            ph.storage.save_level_chunks(current_idx, &chunks).await?;

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

            ph.storage.save_level_metadata(&updated_metadata).await?;
        }
        Ok(())
    }

    /// Extract dirty chunks for current level only
    fn extract_dirty_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            let current_idx = self.current_level.load(Ordering::Relaxed);
            let levels = self.levels.read().map_err(|_| {
                EbloomError::LockError("Failed to read levels".to_string())
            })?;
            let dirty = ph.dirty_chunks.read().map_err(|_| {
                EbloomError::LockError("Failed to read dirty chunks".to_string())
            })?;
            let chunk_size_bits = ph.chunk_size_bytes * 8;
            let mut chunks = Vec::new();
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
            return Ok(chunks);
        }
        Ok(Vec::new())
    }

    /// Extract all chunks for current level only
    #[cfg(feature = "fjall")]
    fn extract_all_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>> {
        let chunk_size_bytes = self
            .persistence
            .as_ref()
            .map(|ph| ph.chunk_size_bytes)
            .unwrap_or(0);
        let current_idx = self.current_level.load(Ordering::Relaxed);
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError("Failed to read levels".to_string())
        })?;

        let chunk_size_bits = chunk_size_bytes * 8;
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
    #[cfg(feature = "fjall")]
    async fn reconstruct_from_storage(&mut self) -> Result<()> {
        if let Some(ref ph) = self.persistence {
            let backend = &ph.storage;

            let current_idx = backend.load_current_level().await?;
            self.current_level.store(current_idx, Ordering::Relaxed);

            let loaded_metadata = backend.load_level_metadata().await?;

            let mut loaded_levels_data = Vec::new();
            for level_idx in 0..self.config.num_levels {
                let dirty_chunks = backend.load_dirty_chunks(level_idx).await?;
                if !dirty_chunks.is_empty() {
                    loaded_levels_data.push((level_idx, dirty_chunks));
                } else {
                    let chunks = backend.load_level_chunks(level_idx).await?;
                    loaded_levels_data.push((level_idx, chunks));
                }
            }

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
                        ph.chunk_size_bytes,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Number of inserts recorded since the last successful snapshot.
    /// Returns 0 when persistence is not configured.
    #[cfg(feature = "fjall")]
    pub fn inserts_since_snapshot(&self) -> usize {
        self.persistence
            .as_ref()
            .map(|ph| ph.snapshot_state.inserts_since_snapshot())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Drop + free functions for background auto-snapshot
// ---------------------------------------------------------------------------

#[cfg(feature = "fjall")]
impl Drop for ExpiringBloomFilter {
    fn drop(&mut self) {
        let Some(ref mut ph) = self.persistence else {
            return;
        };

        if let Some(ref signal) = ph.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
        }
        if let Some(ref notify) = ph.snapshot_notify {
            notify.notify_one();
        }
        if let Some(handle) = ph.task_handle.take() {
            handle.abort();
        }

        if ph.snapshot_state.check_poison().is_none() {
            let storage = Arc::clone(&ph.storage);
            let levels = Arc::clone(&self.levels);
            let dc = Arc::clone(&ph.dirty_chunks);
            let metadata = Arc::clone(&self.metadata);
            let current_level = Arc::clone(&self.current_level);
            let chunk_size_bytes = ph.chunk_size_bytes;
            let state = Arc::clone(&ph.snapshot_state);
            let _ = std::thread::spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let _ = rt.block_on(do_ebloom_snapshot(
                        storage,
                        &levels,
                        &dc,
                        &metadata,
                        &current_level,
                        chunk_size_bytes,
                        &state,
                    ));
                }
            })
            .join();
        }
    }
}

/// Notify the background task when the insert-count threshold is crossed.
#[cfg(feature = "fjall")]
fn maybe_notify_count_trigger_ebloom(
    state: &Arc<SnapshotState>,
    notify: &Option<Arc<Notify>>,
    config: &ExpiringFilterConfig,
) {
    if let (Some(notify), Some(pers)) = (notify, &config.persistence)
        && pers.snapshot_after_inserts > 0
        && state.inserts_since_snapshot() >= pers.snapshot_after_inserts
    {
        notify.notify_one();
    }
}

/// Core snapshot logic: extract dirty chunks for current level, persist, clear flags, update state.
#[cfg(feature = "fjall")]
async fn do_ebloom_snapshot(
    storage: Arc<FjallExpiringBackend>,
    levels: &Arc<RwLock<Vec<BitVec<usize, Lsb0>>>>,
    dirty_chunks: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    metadata: &Arc<RwLock<Vec<LevelMetadata>>>,
    current_level: &Arc<AtomicUsize>,
    chunk_size_bytes: usize,
    state: &Arc<SnapshotState>,
) -> Result<()> {
    if let Some(err) = state.check_poison() {
        return Err(EbloomError::StorageError(err));
    }

    let current_idx = current_level.load(Ordering::Relaxed);
    let dirty = extract_ebloom_dirty_chunks(
        levels,
        dirty_chunks,
        current_idx,
        chunk_size_bytes,
    )?;

    if dirty.is_empty() {
        return Ok(());
    }

    match storage.save_dirty_chunks(current_idx, &dirty).await {
        Ok(()) => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| EbloomError::TimeError(e.to_string()))?
                .as_millis() as u64;

            let updated_metadata = {
                let mut meta = metadata.write().map_err(|_| {
                    EbloomError::LockError("Failed to write metadata".into())
                })?;
                meta[current_idx].last_snapshot_at = now_ms;
                meta.clone()
            };

            if let Err(e) = storage.save_level_metadata(&updated_metadata).await {
                state.on_snapshot_failure(&e.to_string());
                return Err(e);
            }

            // Clear dirty bits only after both saves succeed. This ensures that
            // if the task is aborted between the two awaits, the dirty bits
            // remain set and the final snapshot on drop can re-save idempotently.
            dirty_chunks
                .write()
                .map_err(|_| {
                    EbloomError::LockError("Failed to write dirty chunks".into())
                })?
                .fill(false);

            state.on_snapshot_success();
            Ok(())
        }
        Err(e) => {
            state.on_snapshot_failure(&e.to_string());
            Err(e)
        }
    }
}

/// Extract dirty chunks for a specific level index.
#[cfg(feature = "fjall")]
fn extract_ebloom_dirty_chunks(
    levels: &Arc<RwLock<Vec<BitVec<usize, Lsb0>>>>,
    dirty_chunks: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    current_idx: usize,
    chunk_size_bytes: usize,
) -> Result<Vec<(usize, Vec<u8>)>> {
    let levels = levels
        .read()
        .map_err(|_| EbloomError::LockError("Failed to read levels".into()))?;
    let dirty = dirty_chunks.read().map_err(|_| {
        EbloomError::LockError("Failed to read dirty chunks".into())
    })?;
    let chunk_size_bits = chunk_size_bytes * 8;
    let mut chunks = Vec::new();
    for chunk_id in 0..dirty.len() {
        if dirty[chunk_id] {
            let data = extract_chunk_bytes(
                &levels[current_idx],
                chunk_id,
                chunk_size_bits,
            );
            chunks.push((chunk_id, data));
        }
    }
    Ok(chunks)
}

/// Background task: wakes on time interval or insert-count notify, runs dirty snapshot.
#[cfg(feature = "fjall")]
async fn background_snapshot_loop_ebloom(
    storage: Weak<FjallExpiringBackend>,
    ctx: ExpiringSnapshotLoopCtx,
) {
    let mut ticker = tokio::time::interval(ctx.interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = ctx.notify.notified() => {}
        }

        if ctx.shutdown.load(Ordering::Relaxed) {
            tracing::debug!(
                "Ebloom background snapshot task received shutdown signal"
            );
            break;
        }

        let Some(storage) = storage.upgrade() else {
            tracing::debug!(
                "Ebloom background snapshot task exiting — filter dropped"
            );
            break;
        };

        if ctx.state.check_poison().is_some() {
            tracing::debug!(
                "Ebloom background snapshot task exiting — filter is poisoned"
            );
            break;
        }

        if let Err(e) = do_ebloom_snapshot(
            storage,
            &ctx.levels,
            &ctx.dirty_chunks,
            &ctx.metadata,
            &ctx.current_level,
            ctx.chunk_size_bytes,
            &ctx.state,
        )
        .await
        {
            tracing::warn!(
                "Ebloom background snapshot failed, filter poisoned: {e}"
            );
            break;
        }
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
    let indices = default_hash_function(item, num_hashes, bit_vector_size);

    if let Some(dirty_bits) = dirty {
        for &idx in &indices {
            let chunk_id = (idx as usize) / (chunk_size_bytes * 8);
            if chunk_id < dirty_bits.len() {
                dirty_bits.set(chunk_id, true);
            }
        }
    }

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
    let indices = default_hash_function(item, num_hashes, bit_vector_size);

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

        if all_bits_set {
            return Ok(true);
        }
    }

    Ok(false)
}

#[async_trait::async_trait]
impl ExpiringBloomFilterOps for ExpiringBloomFilter {
    fn insert(&self, item: &[u8]) -> Result<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.snapshot_state.check_poison()
        {
            return Err(EbloomError::StorageError(err));
        }

        let current_level_idx = self.current_level.load(Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        let mut dirty_guard = self
            .persistence
            .as_ref()
            .map(|ph| {
                ph.dirty_chunks.write().map_err(|_| {
                    EbloomError::LockError(
                        "Failed to write dirty chunks".to_string(),
                    )
                })
            })
            .transpose()?;

        #[cfg(feature = "fjall")]
        let chunk_size_bytes = self
            .persistence
            .as_ref()
            .map(|ph| ph.chunk_size_bytes)
            .unwrap_or(0);

        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        #[cfg(feature = "fjall")]
        insert_internal(
            item,
            current_level_idx,
            self.num_hashes,
            self.bit_vector_size,
            chunk_size_bytes,
            dirty_guard.as_deref_mut(),
            &mut levels,
        )?;

        #[cfg(not(feature = "fjall"))]
        insert_internal(
            item,
            current_level_idx,
            self.num_hashes,
            self.bit_vector_size,
            0,
            None,
            &mut levels,
        )?;

        let mut metadata = self.metadata.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on metadata".to_string(),
            )
        })?;
        if let Some(meta) = metadata.get_mut(current_level_idx) {
            meta.insert_count += 1;
        }
        drop(metadata);

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.snapshot_state.record_inserts(1);
            maybe_notify_count_trigger_ebloom(
                &ph.snapshot_state,
                &ph.snapshot_notify,
                &self.config,
            );
        }

        Ok(())
    }

    fn contains(&self, item: &[u8]) -> Result<bool> {
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire read lock on levels".to_string(),
            )
        })?;

        contains_internal(item, self.num_hashes, self.bit_vector_size, &levels)
    }

    fn clear(&self) -> Result<()> {
        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        for level in levels.iter_mut() {
            level.fill(false);
        }

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
            meta.created_at = now_ms;
            meta.insert_count = 0;
            meta.last_snapshot_at = 0;
        }

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
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.snapshot_state.check_poison()
        {
            return Err(EbloomError::StorageError(err));
        }

        let current_level_idx = self.current_level.load(Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        let mut dirty_guard = self
            .persistence
            .as_ref()
            .map(|ph| {
                ph.dirty_chunks.write().map_err(|_| {
                    EbloomError::LockError(
                        "Failed to write dirty chunks".to_string(),
                    )
                })
            })
            .transpose()?;

        #[cfg(feature = "fjall")]
        let chunk_size_bytes = self
            .persistence
            .as_ref()
            .map(|ph| ph.chunk_size_bytes)
            .unwrap_or(0);

        let mut levels = self.levels.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on levels".to_string(),
            )
        })?;

        for item in items {
            #[cfg(feature = "fjall")]
            insert_internal(
                item,
                current_level_idx,
                self.num_hashes,
                self.bit_vector_size,
                chunk_size_bytes,
                dirty_guard.as_deref_mut(),
                &mut levels,
            )?;

            #[cfg(not(feature = "fjall"))]
            insert_internal(
                item,
                current_level_idx,
                self.num_hashes,
                self.bit_vector_size,
                0,
                None,
                &mut levels,
            )?;
        }

        let mut metadata = self.metadata.write().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire write lock on metadata".to_string(),
            )
        })?;
        if let Some(meta) = metadata.get_mut(current_level_idx) {
            meta.insert_count += items.len() as u64;
        }
        drop(metadata);

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.snapshot_state.record_inserts(items.len());
            maybe_notify_count_trigger_ebloom(
                &ph.snapshot_state,
                &ph.snapshot_notify,
                &self.config,
            );
        }

        Ok(())
    }

    fn contains_bulk(&self, items: &[&[u8]]) -> Result<Vec<bool>> {
        let levels = self.levels.read().map_err(|_| {
            EbloomError::LockError(
                "Failed to acquire read lock on levels".to_string(),
            )
        })?;

        items
            .iter()
            .map(|item| {
                contains_internal(
                    item,
                    self.num_hashes,
                    self.bit_vector_size,
                    &levels,
                )
            })
            .collect()
    }
}
