use super::{
    CuckooError, CuckooFilterConfig, CuckooResult,
    storage::CuckooStorageBackend,
    traits::{BulkCuckooFilterOps, CuckooFilterOps, CuckooFilterStats},
};
use crate::hash::{hash_fnv32, hash_murmur32};
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
use super::storage::CuckooFjallBackend;
#[cfg(feature = "fjall")]
use crate::snapshot::SnapshotState;
#[cfg(feature = "fjall")]
use std::sync::Weak;
#[cfg(feature = "fjall")]
use tokio::{sync::Notify, task::JoinHandle, time::Duration};

pub type Bucket = [u16; 4];

// ---------------------------------------------------------------------------
// Persistence handle
// ---------------------------------------------------------------------------

#[cfg(feature = "fjall")]
struct CuckooPersistenceHandle {
    storage: Arc<CuckooFjallBackend>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    snapshot_state: Arc<SnapshotState>,
    shutdown_signal: Option<Arc<AtomicBool>>,
    snapshot_notify: Option<Arc<Notify>>,
    task_handle: Option<JoinHandle<()>>,
}

#[cfg(feature = "fjall")]
struct SnapshotLoopCtx {
    buckets: Arc<RwLock<Vec<Bucket>>>,
    dirty_chunks: Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    state: Arc<SnapshotState>,
    interval: Duration,
    shutdown: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

#[cfg(feature = "fjall")]
impl CuckooPersistenceHandle {
    fn check_poison(&self) -> Option<CuckooError> {
        self.snapshot_state
            .check_poison()
            .map(CuckooError::StorageError)
    }

    fn mark_chunk_dirty(&self, bucket_idx: usize) {
        let buckets_per_chunk =
            self.chunk_size_bytes / std::mem::size_of::<Bucket>();
        let chunk_id = bucket_idx / buckets_per_chunk;
        let mut dirty = self.dirty_chunks.write().unwrap();
        if chunk_id < dirty.len() {
            dirty.set(chunk_id, true);
        }
    }
}

// ---------------------------------------------------------------------------
// CuckooFilter struct
// ---------------------------------------------------------------------------

pub struct CuckooFilter {
    config: CuckooFilterConfig,
    pub num_buckets: usize,
    pub entries_per_bucket: usize,
    pub max_kicks: usize,
    pub fingerprint_bitmask: u16,
    insert_count: AtomicUsize,
    buckets: Arc<RwLock<Vec<Bucket>>>,
    #[cfg(feature = "fjall")]
    persistence: Option<CuckooPersistenceHandle>,
}

impl CuckooFilter {
    pub fn new(config: CuckooFilterConfig) -> CuckooResult<Self> {
        config.validate()?;
        let num_buckets = config.num_buckets();
        let entries_per_bucket = config.entries_per_bucket;
        let max_kicks = config.max_kicks;
        let bitmask = config.fingerprint_bitmask();
        let buckets = Arc::new(RwLock::new(vec![[0u16; 4]; num_buckets]));

        Ok(Self {
            config,
            num_buckets,
            entries_per_bucket,
            max_kicks,
            fingerprint_bitmask: bitmask,
            insert_count: AtomicUsize::new(0),
            buckets,
            #[cfg(feature = "fjall")]
            persistence: None,
        })
    }

    pub async fn create(config: CuckooFilterConfig) -> CuckooResult<Self> {
        config.validate()?;

        #[cfg(feature = "fjall")]
        let storage = if let Some(ref pers) = config.persistence {
            if let Some(parent) = pers.db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CuckooError::StorageError(format!(
                        "Failed to create db directory: {e}"
                    ))
                })?;
            }
            if pers.db_path.exists() {
                std::fs::remove_dir_all(&pers.db_path).map_err(|e| {
                    CuckooError::StorageError(format!(
                        "Failed to delete existing DB: {e}"
                    ))
                })?;
                warn!("Deleted existing database at {:?}", pers.db_path);
            }
            let backend =
                Arc::new(CuckooFjallBackend::new(pers.db_path.clone()).await?);
            info!("Created new Fjall backend at {:?}", pers.db_path);
            backend.save_config(&config).await?;
            info!("Saved config to database.");
            Some(backend)
        } else {
            None
        };

        #[cfg(feature = "fjall")]
        let result = Self::build_filter(config, storage).await;
        #[cfg(not(feature = "fjall"))]
        let result = Self::new(config);
        result
    }

    #[cfg(feature = "fjall")]
    pub async fn load(db_path: PathBuf) -> CuckooResult<Self> {
        if !db_path.exists() {
            return Err(CuckooError::StorageError(format!(
                "Database does not exist at {db_path:?}"
            )));
        }
        let backend = CuckooFjallBackend::new(db_path.clone()).await?;
        info!("Created Fjall backend for existing DB at {:?}", db_path);

        let config = backend.load_config().await?;
        let mut filter =
            Self::build_filter(config, Some(Arc::new(backend))).await?;

        if let Some(ref ph) = filter.persistence {
            let chunks = ph.storage.load_chunks().await?;
            filter.reconstruct_from_chunks(&chunks)?;
            info!("Loaded {} chunks from database", chunks.len());
        }
        Ok(filter)
    }

    pub async fn create_or_load(
        config: CuckooFilterConfig,
    ) -> CuckooResult<Self> {
        #[cfg(feature = "fjall")]
        if let Some(ref pers) = config.persistence {
            if pers.db_path.exists() {
                info!("DB exists, loading from {:?}", pers.db_path);
                return Self::load(pers.db_path.clone()).await;
            } else {
                info!("DB doesn't exist, creating new at {:?}", pers.db_path);
            }
        }
        Self::create(config).await
    }

    async fn build_filter(
        config: CuckooFilterConfig,
        #[cfg(feature = "fjall")] storage: Option<Arc<CuckooFjallBackend>>,
    ) -> CuckooResult<Self> {
        let num_buckets = config.num_buckets();
        let entries_per_bucket = config.entries_per_bucket;
        let max_kicks = config.max_kicks;
        let bitmask = config.fingerprint_bitmask();
        let buckets = Arc::new(RwLock::new(vec![[0u16; 4]; num_buckets]));

        #[cfg(feature = "fjall")]
        let persistence = if let Some(s) = storage {
            let chunk_size_bytes = config
                .persistence
                .as_ref()
                .map(|p| p.chunk_size_bytes)
                .unwrap_or(4096);
            let buckets_per_chunk =
                chunk_size_bytes / std::mem::size_of::<Bucket>();
            let chunk_count = num_buckets.div_ceil(buckets_per_chunk);
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
                                buckets: Arc::clone(&buckets),
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

            Some(CuckooPersistenceHandle {
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
            num_buckets,
            entries_per_bucket,
            max_kicks,
            fingerprint_bitmask: bitmask,
            insert_count: AtomicUsize::new(0),
            buckets,
            #[cfg(feature = "fjall")]
            persistence,
        })
    }

    pub async fn save_snapshot(&self) -> CuckooResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            return do_snapshot(
                Arc::clone(&ph.storage),
                &self.buckets,
                &ph.dirty_chunks,
                ph.chunk_size_bytes,
                &ph.snapshot_state,
            )
            .await;
        }
        Ok(())
    }

    #[cfg(feature = "fjall")]
    fn reconstruct_from_chunks(
        &mut self,
        chunks: &[(usize, Vec<u8>)],
    ) -> CuckooResult<()> {
        let entry_size = std::mem::size_of::<Bucket>();
        let chunk_size_bytes = self
            .persistence
            .as_ref()
            .map(|ph| ph.chunk_size_bytes)
            .unwrap_or(4096);
        let buckets_per_chunk = chunk_size_bytes / entry_size;
        let mut buckets = self.buckets.write().unwrap();

        for (chunk_id, data) in chunks {
            let bucket_offset = chunk_id * buckets_per_chunk;
            let expected_len = (self.num_buckets - bucket_offset)
                .min(buckets_per_chunk)
                * entry_size;
            let data = &data[..data.len().min(expected_len)];

            for (i, chunk) in data.chunks(entry_size).enumerate() {
                let bucket_idx = bucket_offset + i;
                if bucket_idx >= self.num_buckets || chunk.len() < entry_size {
                    break;
                }
                for slot in 0..4 {
                    let off = slot * 2;
                    buckets[bucket_idx][slot] =
                        u16::from_le_bytes([chunk[off], chunk[off + 1]]);
                }
            }
        }
        debug!("Reconstructed filter from {} chunks", chunks.len());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Drop
// ---------------------------------------------------------------------------

#[cfg(feature = "fjall")]
impl Drop for CuckooFilter {
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
            let dirty = extract_dirty_chunks(
                &self.buckets,
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
                            match storage.save_chunks(&dirty).await {
                                Ok(()) => {
                                    let mut d = dc.write().unwrap();
                                    for (chunk_id, _) in &dirty {
                                        d.set(*chunk_id, false);
                                    }
                                    state.on_snapshot_success();
                                    info!(
                                        "Final snapshot on drop \
                                         succeeded"
                                    );
                                }
                                Err(e) => {
                                    state.on_snapshot_failure(&e.to_string());
                                    warn!(
                                        "Final snapshot on drop \
                                         failed: {e}"
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

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

fn compute_fingerprint(item: &[u8], mask: u16) -> CuckooResult<u16> {
    let mut salt = 0u8;
    loop {
        let mut buf = item.to_vec();
        buf.push(salt);
        let fp = hash_fnv32(&buf) as u16 & mask;
        if fp != 0 {
            return Ok(fp);
        }
        salt = salt.wrapping_add(1);
        if salt == 0 {
            return Err(CuckooError::ZeroFingerprint);
        }
    }
}

fn fingerprint_hash(fp: u16) -> usize {
    hash_fnv32(&fp.to_le_bytes()) as usize
}

fn bucket_index(item: &[u8], num_buckets: usize) -> usize {
    hash_murmur32(item) as usize & (num_buckets - 1)
}

fn alt_bucket(current: usize, fp: u16, num_buckets: usize) -> usize {
    (current ^ fingerprint_hash(fp)) & (num_buckets - 1)
}

impl CuckooFilterOps for CuckooFilter {
    fn insert(&self, item: &[u8]) -> CuckooResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.check_poison()
        {
            return Err(err);
        }

        let fp = compute_fingerprint(item, self.fingerprint_bitmask)?;
        let i1 = bucket_index(item, self.num_buckets);
        let i2 = alt_bucket(i1, fp, self.num_buckets);

        let mut buckets = self.buckets.write().unwrap();
        let entries = self.entries_per_bucket;

        for &idx in &[i1, i2] {
            let bucket = &mut buckets[idx];
            for slot in bucket.iter_mut().take(entries) {
                if *slot == 0 {
                    *slot = fp;
                    self.insert_count.fetch_add(1, Ordering::Relaxed);
                    #[cfg(feature = "fjall")]
                    if let Some(ref ph) = self.persistence {
                        ph.mark_chunk_dirty(idx);
                        ph.snapshot_state.record_inserts(1);
                        maybe_notify_count_trigger(
                            &ph.snapshot_state,
                            &ph.snapshot_notify,
                            &self.config,
                        );
                    }
                    return Ok(());
                }
            }
        }

        let mut cur_fp = fp;
        let mut cur_bucket = i1;

        for kick in 0..self.max_kicks {
            let slot_idx = kick % entries;
            let bucket = &mut buckets[cur_bucket];
            std::mem::swap(&mut cur_fp, &mut bucket[slot_idx]);

            #[cfg(feature = "fjall")]
            if let Some(ref ph) = self.persistence {
                ph.mark_chunk_dirty(cur_bucket);
            }

            cur_bucket = alt_bucket(cur_bucket, cur_fp, self.num_buckets);

            let bucket = &mut buckets[cur_bucket];
            for slot in bucket.iter_mut().take(entries) {
                if *slot == 0 {
                    *slot = cur_fp;
                    self.insert_count.fetch_add(1, Ordering::Relaxed);
                    #[cfg(feature = "fjall")]
                    if let Some(ref ph) = self.persistence {
                        ph.mark_chunk_dirty(cur_bucket);
                        ph.snapshot_state.record_inserts(1);
                        maybe_notify_count_trigger(
                            &ph.snapshot_state,
                            &ph.snapshot_notify,
                            &self.config,
                        );
                    }
                    return Ok(());
                }
            }
        }

        Err(CuckooError::CapacityExceeded)
    }

    fn contains(&self, item: &[u8]) -> CuckooResult<bool> {
        let fp = compute_fingerprint(item, self.fingerprint_bitmask)?;
        let i1 = bucket_index(item, self.num_buckets);
        let i2 = alt_bucket(i1, fp, self.num_buckets);

        let buckets = self.buckets.read().unwrap();
        let entries = self.entries_per_bucket;

        for &idx in &[i1, i2] {
            for slot in buckets[idx].iter().take(entries) {
                if *slot == fp {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn delete(&self, item: &[u8]) -> CuckooResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence
            && let Some(err) = ph.check_poison()
        {
            return Err(err);
        }

        let fp = compute_fingerprint(item, self.fingerprint_bitmask)?;
        let i1 = bucket_index(item, self.num_buckets);
        let i2 = alt_bucket(i1, fp, self.num_buckets);

        let mut buckets = self.buckets.write().unwrap();
        let entries = self.entries_per_bucket;

        for &idx in &[i1, i2] {
            for slot in buckets[idx].iter_mut().take(entries) {
                if *slot == fp {
                    *slot = 0;
                    #[cfg(feature = "fjall")]
                    if let Some(ref ph) = self.persistence {
                        ph.mark_chunk_dirty(idx);
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    fn clear(&self) -> CuckooResult<()> {
        let mut buckets = self.buckets.write().unwrap();
        for bucket in buckets.iter_mut() {
            for slot in bucket.iter_mut() {
                *slot = 0;
            }
        }

        #[cfg(feature = "fjall")]
        if let Some(ref ph) = self.persistence {
            let mut dirty = ph.dirty_chunks.write().unwrap();
            dirty.fill(true);
        }

        self.insert_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}

impl CuckooFilterStats for CuckooFilter {
    fn capacity(&self) -> usize {
        self.config.capacity
    }

    fn fingerprint_bits(&self) -> usize {
        self.config.fingerprint_bits
    }

    fn entries_per_bucket(&self) -> usize {
        self.config.entries_per_bucket
    }

    fn insert_count(&self) -> usize {
        self.insert_count.load(Ordering::Relaxed)
    }

    fn load_factor(&self) -> f64 {
        let total_slots = self.num_buckets * self.entries_per_bucket;
        let used_slots = self.insert_count.load(Ordering::Relaxed);
        used_slots as f64 / total_slots as f64
    }
}

impl BulkCuckooFilterOps for CuckooFilter {
    fn insert_bulk(&self, items: &[&[u8]]) -> CuckooResult<()> {
        for item in items {
            CuckooFilterOps::insert(self, item)?;
        }
        Ok(())
    }

    fn contains_bulk(&self, items: &[&[u8]]) -> CuckooResult<Vec<bool>> {
        items
            .iter()
            .map(|item| CuckooFilterOps::contains(self, item))
            .collect()
    }

    fn delete_bulk(&self, items: &[&[u8]]) -> CuckooResult<()> {
        for item in items {
            CuckooFilterOps::delete(self, item)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Free functions — chunk extraction and snapshot
// ---------------------------------------------------------------------------

fn extract_chunk_bytes(
    buckets: &[Bucket],
    chunk_id: usize,
    buckets_per_chunk: usize,
) -> Vec<u8> {
    let start = chunk_id * buckets_per_chunk;
    if start >= buckets.len() {
        return Vec::new();
    }
    let end = std::cmp::min(start + buckets_per_chunk, buckets.len());
    let slice = &buckets[start..end];
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(slice));
    for bucket in slice {
        for slot in bucket {
            bytes.extend_from_slice(&slot.to_le_bytes());
        }
    }
    bytes
}

fn extract_dirty_chunks(
    buckets: &Arc<RwLock<Vec<Bucket>>>,
    dirty_chunks: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
) -> Vec<(usize, Vec<u8>)> {
    let buckets_per_chunk = chunk_size_bytes / std::mem::size_of::<Bucket>();
    let dirty = dirty_chunks.read().unwrap();
    let buckets = buckets.read().unwrap();
    let mut chunks = Vec::new();
    for chunk_id in 0..dirty.len() {
        if dirty[chunk_id] {
            let data = extract_chunk_bytes(&buckets, chunk_id, buckets_per_chunk);
            chunks.push((chunk_id, data));
        }
    }
    debug!("Extracted {} dirty chunks for snapshot", chunks.len());
    chunks
}

#[cfg(feature = "fjall")]
async fn do_snapshot(
    storage: Arc<CuckooFjallBackend>,
    buckets: &Arc<RwLock<Vec<Bucket>>>,
    dirty_chunks: &Arc<RwLock<BitVec<usize, Lsb0>>>,
    chunk_size_bytes: usize,
    state: &Arc<SnapshotState>,
) -> CuckooResult<()> {
    if let Some(err) = state.check_poison() {
        return Err(CuckooError::StorageError(err));
    }

    let dirty = extract_dirty_chunks(buckets, dirty_chunks, chunk_size_bytes);
    if dirty.is_empty() {
        return Ok(());
    }

    match storage.save_chunks(&dirty).await {
        Ok(()) => {
            let mut dc = dirty_chunks.write().unwrap();
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

#[cfg(feature = "fjall")]
fn maybe_notify_count_trigger(
    state: &Arc<SnapshotState>,
    notify: &Option<Arc<Notify>>,
    config: &CuckooFilterConfig,
) {
    if let (Some(notify), Some(pers)) = (notify, &config.persistence)
        && pers.snapshot_after_inserts > 0
        && state.inserts_since_snapshot() >= pers.snapshot_after_inserts
    {
        notify.notify_one();
    }
}

#[cfg(feature = "fjall")]
async fn background_snapshot_loop(
    storage: Weak<CuckooFjallBackend>,
    ctx: SnapshotLoopCtx,
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
            debug!("Cuckoo background snapshot task received shutdown signal");
            break;
        }

        let Some(storage) = storage.upgrade() else {
            debug!("Cuckoo background snapshot task exiting — filter dropped");
            break;
        };

        if ctx.state.check_poison().is_some() {
            debug!(
                "Cuckoo background snapshot task exiting — filter is \
                 poisoned"
            );
            break;
        }

        if let Err(e) = do_snapshot(
            storage,
            &ctx.buckets,
            &ctx.dirty_chunks,
            ctx.chunk_size_bytes,
            &ctx.state,
        )
        .await
        {
            warn!("Cuckoo background snapshot failed, filter poisoned: {e}");
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cuckoo::traits::{
        BulkCuckooFilterOps, CuckooFilterOps, CuckooFilterStats,
    };

    fn default_config() -> CuckooFilterConfig {
        CuckooFilterConfig::default()
    }

    #[test]
    fn test_insert_and_contains() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        cf.insert(b"hello").unwrap();
        cf.insert(b"world").unwrap();

        assert!(cf.contains(b"hello").unwrap());
        assert!(cf.contains(b"world").unwrap());
        assert!(!cf.contains(b"other").unwrap());
    }

    #[test]
    fn test_delete() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        cf.insert(b"hello").unwrap();
        assert!(cf.contains(b"hello").unwrap());

        cf.delete(b"hello").unwrap();
        assert!(!cf.contains(b"hello").unwrap());
    }

    #[test]
    fn test_delete_nonexistent() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();
        cf.delete(b"nope").unwrap();
    }

    #[test]
    fn test_clear() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        cf.insert(b"hello").unwrap();
        cf.clear().unwrap();
        assert!(!cf.contains(b"hello").unwrap());
    }

    #[test]
    fn test_stats() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        assert_eq!(cf.capacity(), 100);
        cf.insert(b"a").unwrap();
        cf.insert(b"b").unwrap();
        assert_eq!(cf.insert_count(), 2);
        assert!(cf.load_factor() > 0.0);
    }

    #[test]
    fn test_bulk_operations() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 1000,
            ..default_config()
        })
        .unwrap();

        let items: Vec<Vec<u8>> =
            (0..100).map(|i| format!("item{i}").into_bytes()).collect();
        let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();

        cf.insert_bulk(&refs).unwrap();
        let results = cf.contains_bulk(&refs).unwrap();
        assert!(results.iter().all(|&r| r));

        cf.delete_bulk(&refs[..10]).unwrap();
        let removed = cf.contains_bulk(&refs[..10]).unwrap();
        assert!(removed.iter().all(|&r| !r));
    }

    #[test]
    fn test_duplicate_insertions() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        cf.insert(b"dup").unwrap();
        cf.insert(b"dup").unwrap();
        assert!(cf.contains(b"dup").unwrap());
    }

    #[test]
    fn test_empty_filter() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 100,
            ..default_config()
        })
        .unwrap();

        assert!(!cf.contains(b"nothing").unwrap());
    }

    #[test]
    fn test_fingerprint_bits_variants() {
        for bits in &[4, 8, 12] {
            let cf = CuckooFilter::new(CuckooFilterConfig {
                capacity: 100,
                fingerprint_bits: *bits,
                ..default_config()
            })
            .unwrap();
            cf.insert(b"test").unwrap();
            assert!(cf.contains(b"test").unwrap());
        }
    }

    #[test]
    fn test_high_load_factor() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 10_000,
            ..default_config()
        })
        .unwrap();

        for i in 0..10_000 {
            cf.insert(format!("item{i}").as_bytes()).unwrap();
        }
        let load = cf.load_factor();
        assert!(load > 0.5, "load factor {load} too low");

        for i in 0..10_000 {
            assert!(cf.contains(format!("item{i}").as_bytes()).unwrap());
        }
    }

    #[test]
    fn test_concurrent_reads() {
        use std::thread;

        let cf = Arc::new(
            CuckooFilter::new(CuckooFilterConfig {
                capacity: 1000,
                ..default_config()
            })
            .unwrap(),
        );

        for i in 0..100 {
            cf.insert(format!("item{i}").as_bytes()).unwrap();
        }

        let mut handles = vec![];
        for _t in 0..4 {
            let cf = Arc::clone(&cf);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let _ = cf.contains(format!("item{i}").as_bytes()).unwrap();
                }
            });
            handles.push(handle);
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_false_positive_rate_approximate() {
        let cf = CuckooFilter::new(CuckooFilterConfig {
            capacity: 500,
            ..default_config()
        })
        .unwrap();

        for i in 0..500 {
            cf.insert(format!("item{i}").as_bytes()).unwrap();
        }

        let mut fp_count = 0;
        for i in 0..10_000 {
            if cf.contains(format!("other{i}").as_bytes()).unwrap() {
                fp_count += 1;
            }
        }
        let measured_fpr = fp_count as f64 / 10_000.0;
        let expected = 2.0 * cf.entries_per_bucket() as f64
            / (1u64 << cf.fingerprint_bits()) as f64;

        assert!(
            measured_fpr <= expected * 5.0,
            "measured FPR {measured_fpr} far exceeds expected {expected}"
        );
    }

    // -------------------------------------------------------
    // Persistence tests
    // -------------------------------------------------------

    #[cfg(feature = "fjall")]
    mod persistence_tests {
        use super::*;
        use crate::cuckoo::{
            CuckooPersistenceConfig, config::CuckooPersistenceConfigBuilder,
        };
        use std::time::Duration;

        fn temp_db(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("cuckoo_test_{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        fn pers_config(db_path: PathBuf) -> CuckooPersistenceConfig {
            CuckooPersistenceConfigBuilder::default()
                .db_path(db_path)
                .chunk_size_bytes(64usize)
                .build()
                .unwrap()
        }

        #[tokio::test]
        async fn test_basic_persistence_lifecycle() {
            let db = temp_db("basic_lifecycle");
            let config = CuckooFilterConfig {
                capacity: 100,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            cf.insert(b"alpha").unwrap();
            cf.insert(b"beta").unwrap();
            cf.save_snapshot().await.unwrap();
            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            assert!(cf2.contains(b"alpha").unwrap());
            assert!(cf2.contains(b"beta").unwrap());
            assert!(!cf2.contains(b"gamma").unwrap());
        }

        #[tokio::test]
        async fn test_snapshot_on_drop() {
            let db = temp_db("drop_snapshot");
            let config = CuckooFilterConfig {
                capacity: 100,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            cf.insert(b"delta").unwrap();
            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            assert!(cf2.contains(b"delta").unwrap());
        }

        #[tokio::test]
        async fn test_create_and_load_roundtrip() {
            let db = temp_db("roundtrip");
            let config = CuckooFilterConfig {
                capacity: 200,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            for i in 0..50 {
                cf.insert(format!("item_{i}").as_bytes()).unwrap();
            }
            cf.save_snapshot().await.unwrap();

            let config2 = cf.config.clone();
            assert_eq!(config2.capacity, 200);

            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            for i in 0..50 {
                assert!(
                    cf2.contains(format!("item_{i}").as_bytes()).unwrap(),
                    "item_{i} should be present"
                );
            }
            assert!(!cf2.contains(b"not_present").unwrap());
        }

        #[tokio::test]
        async fn test_load_nonexistent_path() {
            let result =
                CuckooFilter::load(PathBuf::from("/nonexistent/cuckoo/path"))
                    .await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_create_or_load_new() {
            let db = temp_db("create_or_load_new");
            let config = CuckooFilterConfig {
                capacity: 50,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create_or_load(config).await.unwrap();
            cf.insert(b"new_item").unwrap();
            cf.save_snapshot().await.unwrap();
            drop(cf);

            let config2 = CuckooFilterConfig {
                capacity: 50,
                persistence: Some(
                    CuckooPersistenceConfigBuilder::default()
                        .db_path(db.clone())
                        .chunk_size_bytes(64usize)
                        .build()
                        .unwrap(),
                ),
                ..default_config()
            };
            let cf2 = CuckooFilter::create_or_load(config2).await.unwrap();
            assert!(cf2.contains(b"new_item").unwrap());
        }

        #[tokio::test]
        async fn test_insert_count_trigger() {
            let db = temp_db("count_trigger");
            let pers = CuckooPersistenceConfigBuilder::default()
                .db_path(db.clone())
                .chunk_size_bytes(64usize)
                .auto_snapshot(true)
                .snapshot_interval(Duration::from_secs(9999))
                .snapshot_after_inserts(5usize)
                .build()
                .unwrap();

            let config = CuckooFilterConfig {
                capacity: 1000,
                persistence: Some(pers),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            for i in 0..10 {
                cf.insert(format!("item_{i}").as_bytes()).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            for i in 0..10 {
                assert!(
                    cf2.contains(format!("item_{i}").as_bytes()).unwrap(),
                    "item_{i} should be present"
                );
            }
        }

        #[tokio::test]
        async fn test_delete_persists_chunk() {
            let db = temp_db("delete_persist");
            let config = CuckooFilterConfig {
                capacity: 100,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            cf.insert(b"to_delete").unwrap();
            cf.insert(b"to_keep").unwrap();
            cf.save_snapshot().await.unwrap();

            cf.delete(b"to_delete").unwrap();
            cf.save_snapshot().await.unwrap();
            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            assert!(!cf2.contains(b"to_delete").unwrap());
            assert!(cf2.contains(b"to_keep").unwrap());
        }

        #[tokio::test]
        async fn test_clear_persists() {
            let db = temp_db("clear_persist");
            let config = CuckooFilterConfig {
                capacity: 100,
                persistence: Some(pers_config(db.clone())),
                ..default_config()
            };

            let cf = CuckooFilter::create(config).await.unwrap();
            cf.insert(b"something").unwrap();
            cf.clear().unwrap();
            cf.save_snapshot().await.unwrap();
            drop(cf);

            let cf2 = CuckooFilter::load(db).await.unwrap();
            assert!(!cf2.contains(b"something").unwrap());
        }
    }
}
