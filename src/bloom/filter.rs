use super::{
    BloomError, BloomFilterConfig, BloomFilterOps, BloomResult, StorageBackend,
    storage::FjallBackend,
};
use crate::{
    bloom::traits::{BloomFilterStats, BulkBloomFilterOps},
    hash::{default_hash_function, optimal_bit_vector_size, optimal_num_hashes},
    snapshot::SnapshotState,
};
use bitvec::{bitvec, order::Lsb0, vec::BitVec};
use tracing::{debug, info, warn};

use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[cfg(feature = "fjall")]
use std::sync::Weak;
#[cfg(feature = "fjall")]
use tokio::{sync::Notify, task::JoinHandle, time::Duration};

const DEFAULT_CHUNK_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Persistence handle — all fjall-gated state in one place
// ---------------------------------------------------------------------------

/// All persistence-related state for a `BloomFilter`.
/// Only present when a `db_path` is configured and the `fjall` feature is enabled.
#[cfg(feature = "fjall")]
struct BloomPersistenceHandle {
    storage: Arc<FjallBackend>,
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
struct SnapshotLoopCtx {
    bits: Arc<RwLock<BitVec<usize, Lsb0>>>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    state: Arc<SnapshotState>,
    interval: Duration,
    shutdown: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[cfg(feature = "fjall")]
impl BloomPersistenceHandle {
    fn check_poison(&self) -> Option<BloomError> {
        self.snapshot_state
            .check_poison()
            .map(BloomError::StorageError)
    }

    fn mark_chunk_dirty(&self, bit_index: usize) {
        let chunk_id = bit_index / (self.chunk_size_bytes * 8);
        let mut dirty_chunks = self.dirty_chunks.write().unwrap();
        if chunk_id < dirty_chunks.len() {
            dirty_chunks.set(chunk_id, true);
        }
    }

    fn maybe_notify_count_trigger(&self, config: &BloomFilterConfig) {
        if let (Some(notify), Some(pers)) =
            (&self.snapshot_notify, &config.persistence)
            && pers.snapshot_after_inserts > 0
            && self.snapshot_state.inserts_since_snapshot()
                >= pers.snapshot_after_inserts
        {
            notify.notify_one();
        }
    }
}

// ---------------------------------------------------------------------------
// BloomFilter struct
// ---------------------------------------------------------------------------

pub struct BloomFilter {
    config: BloomFilterConfig,
    pub bit_vector_size: usize,
    pub num_hashes: usize,
    insert_count: AtomicUsize,
    bits: Arc<RwLock<BitVec<usize, Lsb0>>>,
    /// All persistence state lives here; one `#[cfg]` instead of five.
    #[cfg(feature = "fjall")]
    persistence: Option<BloomPersistenceHandle>,
}

impl BloomFilter {
    /// Creates a new bloom filter, optionally with persistence.
    /// If persistence is enabled and DB exists, it will be overwritten.
    pub async fn create(config: BloomFilterConfig) -> BloomResult<Self> {
        config.validate()?;

        #[cfg(feature = "fjall")]
        let storage = if let Some(persistence_config) = &config.persistence {
            if let Some(parent) = persistence_config.db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    BloomError::StorageError(format!(
                        "Failed to create db directory: {e}"
                    ))
                })?;
            }

            if persistence_config.db_path.exists() {
                std::fs::remove_dir_all(&persistence_config.db_path).map_err(
                    |e| {
                        BloomError::StorageError(format!(
                            "Failed to delete existing DB: {e}"
                        ))
                    },
                )?;
                warn!(
                    "Deleted existing database at {:?}",
                    persistence_config.db_path
                );
            }

            let storage = Arc::new(
                FjallBackend::new(persistence_config.db_path.clone()).await?,
            );
            info!(
                "Created new Fjall backend at {:?}",
                persistence_config.db_path
            );

            storage.save_config(&config).await?;
            info!("Saved config to database.");

            Some(storage)
        } else {
            None
        };

        Self::build_filter(config, storage).await
    }

    /// Loads an existing bloom filter from database.
    /// Returns error if database doesn't exist.
    #[cfg(feature = "fjall")]
    pub async fn load(db_path: PathBuf) -> BloomResult<Self> {
        if !db_path.exists() {
            return Err(BloomError::StorageError(format!(
                "Database does not exist at {db_path:?}"
            )));
        }

        let backend = FjallBackend::new(db_path.clone()).await?;
        info!("Created Fjall backend for existing DB at {:?}", db_path);

        let loaded_config = backend.load_config().await?;
        info!(
            "Loaded config from DB - capacity: {}, FPR: {:.3}%",
            loaded_config.capacity,
            loaded_config.false_positive_rate * 100.0
        );

        let mut filter =
            Self::build_filter(loaded_config, Some(Arc::new(backend))).await?;

        if let Some(ref ph) = filter.persistence {
            let chunks = ph.storage.load_snapshot().await?;
            filter.reconstruct_from_chunks(&chunks)?;
            info!("Loaded {} chunks from database", chunks.len());
        }

        Ok(filter)
    }

    /// Creates new filter or loads existing one.
    pub async fn create_or_load(config: BloomFilterConfig) -> BloomResult<Self> {
        #[cfg(feature = "fjall")]
        if let Some(persistence_config) = &config.persistence {
            if persistence_config.db_path.exists() {
                info!("DB exists, loading from {:?}", persistence_config.db_path);
                return Self::load(persistence_config.db_path.clone()).await;
            } else {
                info!(
                    "DB doesn't exist, creating new at {:?}",
                    persistence_config.db_path
                );
            }
        }
        Self::create(config).await
    }

    /// Internal helper to build the actual BloomFilter struct.
    async fn build_filter(
        config: BloomFilterConfig,
        #[cfg(feature = "fjall")] storage: Option<Arc<FjallBackend>>,
    ) -> BloomResult<Self> {
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);
        let bits = Arc::new(RwLock::new(bitvec![0; bit_vector_size]));

        #[cfg(feature = "fjall")]
        let persistence = if let Some(s) = storage {
            let chunk_size_bytes = config
                .persistence
                .as_ref()
                .map(|p| p.chunk_size_bytes)
                .unwrap_or(DEFAULT_CHUNK_SIZE);
            let chunk_count = (bit_vector_size + chunk_size_bytes * 8 - 1)
                .div_ceil(chunk_size_bytes * 8);
            let dirty_chunks = Arc::new(RwLock::new(bitvec![0; chunk_count]));
            let snapshot_state = SnapshotState::new();

            let (shutdown_signal, snapshot_notify, task_handle) =
                if let Some(pers) = &config.persistence {
                    if pers.auto_snapshot {
                        let shutdown = Arc::new(AtomicBool::new(false));
                        let notify = Arc::new(Notify::new());
                        let handle = tokio::spawn(background_snapshot_loop(
                            Arc::downgrade(&s),
                            SnapshotLoopCtx {
                                bits: Arc::clone(&bits),
                                dirty_chunks: Arc::clone(&dirty_chunks),
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

            Some(BloomPersistenceHandle {
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
            bits,
            insert_count: AtomicUsize::new(0),
            #[cfg(feature = "fjall")]
            persistence,
        })
    }

    /// Saves dirty chunks to the persistence backend.
    ///
    /// On success, clears dirty flags and resets the insert counter.
    /// On failure, poisons the filter — subsequent writes return the stored error.
    /// Returns immediately if nothing is dirty.
    pub async fn save_snapshot(&self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            do_snapshot(
                Arc::clone(&ph.storage),
                &self.bits,
                &ph.dirty_chunks,
                ph.chunk_size_bytes,
                &ph.snapshot_state,
            )
            .await?;
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

    pub fn extract_dirty_chunks(&self) -> Vec<(usize, Vec<u8>)> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            return extract_dirty_chunks_from_arcs(
                &self.bits,
                &ph.dirty_chunks,
                ph.chunk_size_bytes,
            );
        }
        Vec::new()
    }

    #[cfg(feature = "fjall")]
    fn reconstruct_from_chunks(
        &mut self,
        chunks: &[(usize, Vec<u8>)],
    ) -> BloomResult<()> {
        let chunk_size_bytes = self
            .persistence
            .as_ref()
            .map(|ph| ph.chunk_size_bytes)
            .unwrap_or(0);
        let chunk_size_bits = chunk_size_bytes * 8;
        let mut bits = self.bits.write().unwrap();

        for (chunk_id, chunk_bytes) in chunks {
            let start_bit = chunk_id * chunk_size_bits;
            for (byte_idx, &byte) in chunk_bytes.iter().enumerate() {
                for bit_pos in 0..8 {
                    let bit_idx = start_bit + byte_idx * 8 + bit_pos;
                    if bit_idx < bits.len() {
                        let bit_value = (byte & (1 << bit_pos)) != 0;
                        bits.set(bit_idx, bit_value);
                    }
                }
            }
        }

        debug!("Reconstructed filter from {} chunks", chunks.len());
        Ok(())
    }

    pub fn config(&self) -> &BloomFilterConfig {
        &self.config
    }

    pub fn approx_memory_bits(&self) -> usize {
        let binding = self.bits.read().unwrap();
        let words = binding.as_raw_slice();
        std::mem::size_of_val(words)
    }

    pub fn bits_per_item(&self) -> f64 {
        self.approx_memory_bits() as f64 / self.config.capacity as f64
    }
}

#[cfg(feature = "fjall")]
impl Drop for BloomFilter {
    fn drop(&mut self) {
        let Some(ref mut ph) = self.persistence else {
            return;
        };

        // Signal and wake the background task so it exits promptly.
        if let Some(ref signal) = ph.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
        }
        if let Some(ref notify) = ph.snapshot_notify {
            notify.notify_one();
        }
        if let Some(handle) = ph.task_handle.take() {
            handle.abort();
        }

        // Attempt a final snapshot. We use a dedicated thread + single-threaded
        // runtime so this works regardless of whether we're inside a tokio task.
        if ph.snapshot_state.check_poison().is_none() {
            let dirty = extract_dirty_chunks_from_arcs(
                &self.bits,
                &ph.dirty_chunks,
                ph.chunk_size_bytes,
            );
            if !dirty.is_empty() {
                let storage = Arc::clone(&ph.storage);
                let dc = Arc::clone(&ph.dirty_chunks);
                let state = Arc::clone(&ph.snapshot_state);
                let _ = std::thread::spawn(move || {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        rt.block_on(async move {
                            match storage.save_snapshot(&dirty).await {
                                Ok(()) => {
                                    {
                                        let mut dc = dc.write().unwrap();
                                        for (chunk_id, _) in &dirty {
                                            dc.set(*chunk_id, false);
                                        }
                                    }
                                    state.on_snapshot_success();
                                    info!("Final snapshot on drop succeeded");
                                }
                                Err(e) => {
                                    state.on_snapshot_failure(&e.to_string());
                                    warn!("Final snapshot on drop failed: {e}");
                                }
                            }
                        });
                    }
                })
                .join();
            }
        }
    }
}

impl BloomFilterStats for BloomFilter {
    fn insert_count(&self) -> usize {
        self.insert_count.load(Ordering::Relaxed)
    }

    fn capacity(&self) -> usize {
        self.config.capacity
    }

    fn false_positive_rate(&self) -> f64 {
        self.config.false_positive_rate
    }
}

impl BloomFilterOps for BloomFilter {
    fn insert(&self, item: &[u8]) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.check_poison()
        {
            return Err(err);
        }

        let indices =
            compute_indices(item, self.num_hashes, self.bit_vector_size)?;

        let mut bits = self.bits.write().unwrap();

        for &idx in &indices {
            #[cfg(feature = "fjall")]
            if let Some(ref ph) = self.persistence {
                ph.mark_chunk_dirty(idx);
            }

            bits.set(idx, true);
        }

        self.insert_count.fetch_add(1, Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.snapshot_state.record_inserts(1);
            ph.maybe_notify_count_trigger(&self.config);
        }

        Ok(())
    }

    fn contains(&self, item: &[u8]) -> BloomResult<bool> {
        let indices =
            compute_indices(item, self.num_hashes, self.bit_vector_size)?;
        let bits = self.bits.read().unwrap();

        for &idx in &indices {
            if !bits[idx] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn clear(&self) -> BloomResult<()> {
        let mut bits = self.bits.write().unwrap();
        bits.fill(false);
        self.insert_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}

impl BulkBloomFilterOps for BloomFilter {
    fn insert_bulk(&self, items: &[&[u8]]) -> BloomResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.check_poison()
        {
            return Err(err);
        }

        let all_indices: Vec<Vec<usize>> = items
            .iter()
            .map(|item| {
                compute_indices(item, self.num_hashes, self.bit_vector_size)
            })
            .collect::<BloomResult<Vec<_>>>()?;

        let mut bits = self.bits.write().unwrap();

        for indices in &all_indices {
            for &idx in indices {
                #[cfg(feature = "fjall")]
                if let Some(ref ph) = self.persistence {
                    ph.mark_chunk_dirty(idx);
                }

                bits.set(idx, true);
            }
        }

        self.insert_count.fetch_add(items.len(), Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            ph.snapshot_state.record_inserts(items.len());
            ph.maybe_notify_count_trigger(&self.config);
        }

        Ok(())
    }

    fn contains_bulk(&self, items: &[&[u8]]) -> BloomResult<Vec<bool>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let all_indices: Vec<Vec<usize>> = items
            .iter()
            .map(|item| {
                compute_indices(item, self.num_hashes, self.bit_vector_size)
            })
            .collect::<BloomResult<Vec<_>>>()?;

        let bits = self.bits.read().unwrap();

        let mut results = Vec::with_capacity(items.len());
        for indices in &all_indices {
            let mut exists = true;
            for &idx in indices {
                if !bits[idx] {
                    exists = false;
                    break;
                }
            }
            results.push(exists);
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Free functions shared between save_snapshot, Drop, and the background task
// ---------------------------------------------------------------------------

/// Extract bytes for a single chunk from the bit vector.
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

/// Extract all dirty chunks, given Arc-wrapped bit vectors.
fn extract_dirty_chunks_from_arcs(
    bits: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    dirty_chunks_arc: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
) -> Vec<(usize, Vec<u8>)> {
    let dirty = dirty_chunks_arc.read().unwrap();
    let bits = bits.read().unwrap();
    let chunk_size_bits = chunk_size_bytes * 8;
    let mut chunks = Vec::new();
    for chunk_id in 0..dirty.len() {
        if dirty[chunk_id] {
            let data = extract_chunk_bytes(&bits, chunk_id, chunk_size_bits);
            chunks.push((chunk_id, data));
        }
    }
    debug!("Extracted {} dirty chunks for snapshot", chunks.len());
    chunks
}

fn compute_indices(
    item: &[u8],
    num_hashes: usize,
    capacity: usize,
) -> BloomResult<Vec<usize>> {
    let indices = default_hash_function(item, num_hashes, capacity);
    let mut result = Vec::with_capacity(indices.len());
    for idx in indices {
        let idx = idx as usize;
        if idx >= capacity {
            return Err(BloomError::IndexOutOfBounds {
                index: idx,
                capacity,
            });
        }
        result.push(idx);
    }
    Ok(result)
}

/// Perform one snapshot attempt: save dirty chunks, clear dirty flags, update state.
/// Returns Ok(()) and skips silently if nothing is dirty.
/// On failure, poisons `state` and returns the error.
#[cfg(feature = "fjall")]
async fn do_snapshot(
    storage: Arc<FjallBackend>,
    bits: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    dirty_chunks_arc: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    state: &Arc<SnapshotState>,
) -> BloomResult<()> {
    if let Some(err) = state.check_poison() {
        return Err(BloomError::StorageError(err));
    }

    let dirty =
        extract_dirty_chunks_from_arcs(bits, dirty_chunks_arc, chunk_size_bytes);
    if dirty.is_empty() {
        return Ok(());
    }

    match storage.save_snapshot(&dirty).await {
        Ok(()) => {
            let mut dc = dirty_chunks_arc.write().unwrap();
            for (chunk_id, _) in &dirty {
                dc.set(*chunk_id, false);
            }
            state.on_snapshot_success();
            info!("Saved {} dirty chunks to database", dirty.len());
            Ok(())
        }
        Err(e) => {
            state.on_snapshot_failure(&e.to_string());
            Err(e)
        }
    }
}

/// Background task: wakes on time interval or insert-count notify, runs snapshot.
/// Exits on shutdown signal or after a snapshot failure (filter is then poisoned).
#[cfg(feature = "fjall")]
async fn background_snapshot_loop(
    storage: Weak<FjallBackend>,
    ctx: SnapshotLoopCtx,
) {
    let mut ticker = tokio::time::interval(ctx.interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // skip the immediate first tick

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = ctx.notify.notified() => {}
        }

        if ctx.shutdown.load(Ordering::Relaxed) {
            debug!("Background snapshot task received shutdown signal");
            break;
        }

        // If the filter has been dropped the Weak can no longer upgrade.
        let Some(storage) = storage.upgrade() else {
            debug!("Background snapshot task exiting — filter dropped");
            break;
        };

        if ctx.state.check_poison().is_some() {
            debug!("Background snapshot task exiting — filter is poisoned");
            break;
        }

        if let Err(e) = do_snapshot(
            storage,
            &ctx.bits,
            &ctx.dirty_chunks,
            ctx.chunk_size_bytes,
            &ctx.state,
        )
        .await
        {
            warn!("Background snapshot failed, filter poisoned: {e}");
            break;
        }
    }
}
