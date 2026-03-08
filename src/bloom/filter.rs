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

pub struct BloomFilter {
    config: BloomFilterConfig,
    pub bit_vector_size: usize,
    pub num_hashes: usize,
    insert_count: AtomicUsize,

    // Read-heavy data
    bits: Arc<RwLock<BitVec<usize, Lsb0>>>,
    pub(crate) dirty_chunks: Option<Arc<RwLock<BitVec<usize, Lsb0>>>>,

    // Persistence support — wrapped in Arc so the background task can hold a Weak ref
    // without keeping the DB file lock alive after the filter is dropped.
    #[cfg(feature = "fjall")]
    pub storage: Option<Arc<FjallBackend>>,
    chunk_size_bytes: usize,

    #[cfg(feature = "fjall")]
    snapshot_state: Option<Arc<SnapshotState>>,

    // Background auto-snapshot
    #[cfg(feature = "fjall")]
    shutdown_signal: Option<Arc<AtomicBool>>,
    #[cfg(feature = "fjall")]
    snapshot_notify: Option<Arc<Notify>>,
    #[cfg(feature = "fjall")]
    task_handle: Option<JoinHandle<()>>,
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

        if let Some(ref backend) = filter.storage {
            let chunks = backend.load_snapshot().await?;
            filter.reconstruct_from_chunks(&chunks)?;
            info!("Loaded {} chunks from database", chunks.len());
        }

        Ok(filter)
    }

    /// Creates new filter or loads existing one.
    /// If DB exists, loads it (ignoring config parameters).
    /// If DB doesn't exist, creates new one with provided config.
    pub async fn create_or_load(config: BloomFilterConfig) -> BloomResult<Self> {
        #[cfg(feature = "fjall")]
        if let Some(persistence_config) = &config.persistence {
            if persistence_config.db_path.exists() {
                println!(
                    "DB exists, loading from {:?}",
                    persistence_config.db_path
                );
                Self::load(persistence_config.db_path.clone()).await
            } else {
                println!(
                    "DB doesn't exist, creating new at {:?}",
                    persistence_config.db_path
                );
                Self::create(config).await
            }
        } else {
            Self::create(config).await
        }
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

        let (chunk_size_bytes, dirty_chunks) =
            if let Some(persistence) = &config.persistence {
                let chunk_size = persistence.chunk_size_bytes;
                let chunk_count = (bit_vector_size + chunk_size * 8 - 1)
                    .div_ceil(chunk_size * 8);
                (
                    chunk_size,
                    Some(Arc::new(RwLock::new(bitvec![0; chunk_count]))),
                )
            } else {
                (0, None)
            };

        #[cfg(feature = "fjall")]
        let snapshot_state = if storage.is_some() {
            Some(SnapshotState::new())
        } else {
            None
        };

        // Spawn background auto-snapshot task if configured
        #[cfg(feature = "fjall")]
        let (shutdown_signal, snapshot_notify, task_handle) =
            if let (Some(s), Some(pers), Some(state), Some(dc)) = (
                &storage,
                &config.persistence,
                &snapshot_state,
                &dirty_chunks,
            ) {
                if pers.auto_snapshot {
                    let shutdown = Arc::new(AtomicBool::new(false));
                    let notify = Arc::new(Notify::new());
                    let handle = tokio::spawn(background_snapshot_loop(
                        Arc::downgrade(s),
                        Arc::clone(&bits),
                        Arc::clone(dc),
                        chunk_size_bytes,
                        Arc::clone(state),
                        pers.snapshot_interval,
                        pers.snapshot_after_inserts,
                        Arc::clone(&shutdown),
                        Arc::clone(&notify),
                    ));
                    (Some(shutdown), Some(notify), Some(handle))
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            };

        Ok(Self {
            config,
            bit_vector_size,
            num_hashes,
            bits,
            insert_count: AtomicUsize::new(0),
            #[cfg(feature = "fjall")]
            storage,
            chunk_size_bytes,
            dirty_chunks,
            #[cfg(feature = "fjall")]
            snapshot_state,
            #[cfg(feature = "fjall")]
            shutdown_signal,
            #[cfg(feature = "fjall")]
            snapshot_notify,
            #[cfg(feature = "fjall")]
            task_handle,
        })
    }

    /// Saves dirty chunks to the persistence backend.
    ///
    /// On success, clears dirty flags and resets the insert counter.
    /// On failure, poisons the filter — subsequent writes return the stored error.
    /// Returns immediately if nothing is dirty.
    pub async fn save_snapshot(&self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        if let (Some(storage), Some(dc), Some(state)) =
            (&self.storage, &self.dirty_chunks, &self.snapshot_state)
        {
            do_snapshot(
                Arc::clone(storage),
                &self.bits,
                dc,
                self.chunk_size_bytes,
                state,
            )
            .await?;
        }
        Ok(())
    }

    /// Number of inserts recorded since the last successful snapshot.
    /// Returns 0 when persistence is not configured.
    #[cfg(feature = "fjall")]
    pub fn inserts_since_snapshot(&self) -> usize {
        self.snapshot_state
            .as_ref()
            .map(|s| s.inserts_since_snapshot())
            .unwrap_or(0)
    }

    pub fn extract_dirty_chunks(&self) -> Vec<(usize, Vec<u8>)> {
        match (&self.dirty_chunks, self.chunk_size_bytes) {
            (Some(dc), csz) if csz > 0 => {
                extract_dirty_chunks_from_arcs(&self.bits, dc, csz)
            }
            _ => Vec::new(),
        }
    }

    fn reconstruct_from_chunks(
        &mut self,
        chunks: &[(usize, Vec<u8>)],
    ) -> BloomResult<()> {
        let chunk_size_bits = self.chunk_size_bytes * 8;
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
        // Signal and wake the background task so it exits promptly.
        if let Some(ref signal) = self.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
        }
        if let Some(ref notify) = self.snapshot_notify {
            notify.notify_one();
        }
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        // Attempt a final snapshot. We use a dedicated thread + single-threaded
        // runtime so this works regardless of whether we're inside a tokio task.
        if let (Some(storage), Some(dc), Some(state)) =
            (&self.storage, &self.dirty_chunks, &self.snapshot_state)
        {
            if state.check_poison().is_none() {
                let dirty = extract_dirty_chunks_from_arcs(
                    &self.bits,
                    dc,
                    self.chunk_size_bytes,
                );
                if !dirty.is_empty() {
                    let storage = storage.clone();
                    let dc = Arc::clone(dc);
                    let state = Arc::clone(state);
                    let _ = std::thread::spawn(move || {
                        if let Ok(rt) =
                            tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                        {
                            rt.block_on(async move {
                                match storage.save_snapshot(&dirty).await {
                                    Ok(()) => {
                                        dc.write().unwrap().fill(false);
                                        state.on_snapshot_success();
                                        info!("Final snapshot on drop succeeded");
                                    }
                                    Err(e) => {
                                        state.on_snapshot_failure(&e.to_string());
                                        warn!(
                                            "Final snapshot on drop failed: {e}"
                                        );
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
        if let Some(ref state) = self.snapshot_state {
            if let Some(err) = state.check_poison() {
                return Err(BloomError::StorageError(err));
            }
        }

        let indices =
            default_hash_function(item, self.num_hashes, self.bit_vector_size);

        let mut bits = self.bits.write().unwrap();

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }

            if let Some(ref dirty_chunks_arc) = self.dirty_chunks {
                let mut dirty_chunks = dirty_chunks_arc.write().unwrap();
                let chunk_id = idx / (self.chunk_size_bytes * 8);
                if chunk_id < dirty_chunks.len() {
                    dirty_chunks.set(chunk_id, true);
                }
            }

            bits.set(idx, true);
        }

        self.insert_count.fetch_add(1, Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        if let Some(ref state) = self.snapshot_state {
            state.record_inserts(1);
            maybe_notify_count_trigger(
                state,
                &self.snapshot_notify,
                &self.config,
            );
        }

        Ok(())
    }

    fn contains(&self, item: &[u8]) -> BloomResult<bool> {
        let indices =
            default_hash_function(item, self.num_hashes, self.bit_vector_size);
        let bits = self.bits.read().unwrap();

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }
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
        if let Some(ref state) = self.snapshot_state {
            if let Some(err) = state.check_poison() {
                return Err(BloomError::StorageError(err));
            }
        }

        let all_indices: Vec<Vec<u32>> = items
            .iter()
            .map(|item| {
                default_hash_function(item, self.num_hashes, self.bit_vector_size)
            })
            .collect();

        let mut bits = self.bits.write().unwrap();

        for indices in &all_indices {
            for &idx in indices {
                let idx = idx as usize;
                if idx >= self.bit_vector_size {
                    return Err(BloomError::IndexOutOfBounds {
                        index: idx,
                        capacity: self.bit_vector_size,
                    });
                }

                if let Some(ref dirty_chunks_arc) = self.dirty_chunks {
                    let mut dirty_chunks = dirty_chunks_arc.write().unwrap();
                    let chunk_id = idx / (self.chunk_size_bytes * 8);
                    if chunk_id < dirty_chunks.len() {
                        dirty_chunks.set(chunk_id, true);
                    }
                }

                bits.set(idx, true);
            }
        }

        self.insert_count.fetch_add(items.len(), Ordering::Relaxed);

        #[cfg(feature = "fjall")]
        if let Some(ref state) = self.snapshot_state {
            state.record_inserts(items.len());
            maybe_notify_count_trigger(
                state,
                &self.snapshot_notify,
                &self.config,
            );
        }

        Ok(())
    }

    fn contains_bulk(&self, items: &[&[u8]]) -> BloomResult<Vec<bool>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let all_indices: Vec<Vec<u32>> = items
            .iter()
            .map(|item| {
                default_hash_function(item, self.num_hashes, self.bit_vector_size)
            })
            .collect();

        let bits = self.bits.read().unwrap();

        let mut results = Vec::with_capacity(items.len());
        for indices in &all_indices {
            let mut exists = true;
            for &idx in indices {
                let idx = idx as usize;
                if idx >= self.bit_vector_size {
                    return Err(BloomError::IndexOutOfBounds {
                        index: idx,
                        capacity: self.bit_vector_size,
                    });
                }
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
fn extract_chunk_bytes_bloom(
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
            let data =
                extract_chunk_bytes_bloom(&bits, chunk_id, chunk_size_bits);
            chunks.push((chunk_id, data));
        }
    }
    debug!("Extracted {} dirty chunks for snapshot", chunks.len());
    chunks
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
            dirty_chunks_arc.write().unwrap().fill(false);
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

/// Notify the background task when the insert-count threshold is crossed.
#[cfg(feature = "fjall")]
fn maybe_notify_count_trigger(
    state: &Arc<SnapshotState>,
    notify: &Option<Arc<Notify>>,
    config: &BloomFilterConfig,
) {
    if let (Some(notify), Some(pers)) = (notify, &config.persistence) {
        if pers.snapshot_after_inserts > 0
            && state.inserts_since_snapshot() >= pers.snapshot_after_inserts
        {
            notify.notify_one();
        }
    }
}

/// Background task: wakes on time interval or insert-count notify, runs snapshot.
/// Exits on shutdown signal or after a snapshot failure (filter is then poisoned).
#[cfg(feature = "fjall")]
async fn background_snapshot_loop(
    storage: Weak<FjallBackend>,
    bits: Arc<RwLock<BitVec<usize, Lsb0>>>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    state: Arc<SnapshotState>,
    interval: Duration,
    _count_threshold: usize,
    shutdown: Arc<AtomicBool>,
    notify: Arc<Notify>,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // skip the immediate first tick

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = notify.notified() => {}
        }

        if shutdown.load(Ordering::Relaxed) {
            debug!("Background snapshot task received shutdown signal");
            break;
        }

        // If the filter has been dropped the Weak can no longer upgrade.
        let Some(storage) = storage.upgrade() else {
            debug!("Background snapshot task exiting — filter dropped");
            break;
        };

        if state.check_poison().is_some() {
            debug!("Background snapshot task exiting — filter is poisoned");
            break;
        }

        if let Err(e) =
            do_snapshot(storage, &bits, &dirty_chunks, chunk_size_bytes, &state)
                .await
        {
            warn!("Background snapshot failed, filter poisoned: {e}");
            break;
        }
    }
}
