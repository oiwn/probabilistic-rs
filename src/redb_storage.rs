use crate::expiring_bloom::{
    default_hash_function, optimal_bit_vector_size, optimal_num_hashes,
    BloomError, BloomFilterStorage, InMemoryStorage, Result, SlidingBloomFilter,
};
use derive_builder::Builder;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, SystemTime};

// Key: u8 (just level), Value: Vec<u8> (bit array)
const BITS_TABLE: TableDefinition<u8, &[u8]> = TableDefinition::new("bits");
// Table for storing timestamps per level
const TIMESTAMPS_TABLE: TableDefinition<u8, &[u8]> =
    TableDefinition::new("timestamps");

pub struct RedbStorage {
    db: Arc<Database>,
    capacity: usize,
    max_levels: usize,
}

#[derive(Builder, Debug)]
#[builder(pattern = "owned")]
pub struct RedbExpiringloomFilterConfig {
    path: PathBuf,
    capacity: usize,
    max_levels: usize,
    #[builder(default = "Duration::from_secs(1)")]
    snapshot_interval: Duration,
    #[builder(default = "0.01")]
    false_positive_rate: f64,
}

pub struct RedbExpiringBloomFilter {
    memory_storage: Arc<RwLock<InMemoryStorage>>,
    db: Arc<Database>,
    shutdown: Arc<AtomicBool>,
    config: RedbExpiringloomFilterConfig,
}

impl RedbExpiringBloomFilter {
    pub fn new(config: RedbExpiringloomFilterConfig) -> Result<Self> {
        let db =
            Arc::new(Database::create(&config.path).map_err(redb::Error::from)?);
        let mut initial_storage = Self::load_or_create_storage(&db, &config)?;

        // Initial cleanup
        Self::cleanup_expired_levels(&mut initial_storage, config.max_levels)?;

        let storage = Arc::new(RwLock::new(initial_storage));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Start background thread for maintenance
        let storage_clone = Arc::clone(&storage);
        let shutdown_clone = Arc::clone(&shutdown);
        let max_levels = config.max_levels;

        thread::spawn(move || {
            while !shutdown_clone.load(Ordering::Relaxed) {
                if let Ok(mut guard) = storage_clone.write() {
                    let _ = Self::cleanup_expired_levels(&mut guard, max_levels);
                }
                thread::sleep(config.snapshot_interval);
            }
        });

        Ok(Self {
            memory_storage: storage,
            db,
            shutdown,
            config,
        })
    }

    pub fn insert(&mut self, item: &[u8]) -> Result<()> {
        let mut storage = self.memory_storage.write().map_err(|e| {
            BloomError::StorageError(format!("Lock error: {}", e))
        })?;

        let indices: Vec<usize> = (default_hash_function)(
            item,
            optimal_num_hashes(self.config.capacity, self.config.max_levels),
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        storage.set_bits(0, &indices) // Always insert into level 0
    }

    pub fn query(&self, item: &[u8]) -> Result<bool> {
        let storage = self.memory_storage.read().map_err(|e| {
            BloomError::StorageError(format!("Lock error: {}", e))
        })?;

        let indices: Vec<usize> = (default_hash_function)(
            item,
            optimal_num_hashes(self.config.capacity, self.config.max_levels),
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        // Check all active (non-expired) levels
        for level in 0..self.config.max_levels {
            if let Some(timestamp) = storage.get_timestamp(level)? {
                let elapsed = SystemTime::now()
                    .duration_since(timestamp)
                    .map_err(|e| BloomError::StorageError(e.to_string()))?;

                if elapsed
                    <= Duration::from_secs(1) * self.config.max_levels as u32
                {
                    let bits = storage.get_bits(level, &indices)?;
                    if bits.iter().all(|&bit| bit) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn cleanup_expired_levels(
        storage: &mut InMemoryStorage,
        max_levels: usize,
    ) -> Result<()> {
        let now = SystemTime::now();
        for level in 0..max_levels {
            if let Some(timestamp) = storage.get_timestamp(level)? {
                if let Ok(elapsed) = now.duration_since(timestamp) {
                    if elapsed >= Duration::from_secs(1) * max_levels as u32 {
                        storage.clear_level(level)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn load_or_create_storage(
        db: &Database,
        config: &RedbExpiringloomFilterConfig,
    ) -> Result<InMemoryStorage> {
        // Initialize empty storage
        let mut levels = vec![vec![false; config.capacity]; config.max_levels];
        let mut timestamps = vec![SystemTime::now(); config.max_levels];

        // Create a write transaction first to ensure tables exist
        let write_txn = db.begin_write().map_err(redb::Error::from)?;
        {
            // Create tables if they don't exist
            let mut bits_table = write_txn
                .open_table(BITS_TABLE)
                .map_err(redb::Error::from)?;
            let mut timestamps_table = write_txn
                .open_table(TIMESTAMPS_TABLE)
                .map_err(redb::Error::from)?;

            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| BloomError::StorageError(e.to_string()))?;
            let duration_bytes = bincode::serialize(&now)
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;

            // For each level, ensure we have initial data
            for level in 0..config.max_levels {
                let level_u8 = level as u8;

                // Initialize bits if not exists
                if bits_table
                    .get(&level_u8)
                    .map_err(redb::Error::from)?
                    .is_none()
                {
                    let empty_bits = vec![0u8; (config.capacity + 7) / 8];
                    bits_table
                        .insert(&level_u8, empty_bits.as_slice())
                        .map_err(|e| BloomError::StorageError(e.to_string()))?;
                }

                // Initialize timestamp if not exists
                if timestamps_table
                    .get(&level_u8)
                    .map_err(redb::Error::from)?
                    .is_none()
                {
                    timestamps_table
                        .insert(&level_u8, duration_bytes.as_slice())
                        .map_err(|e| BloomError::StorageError(e.to_string()))?;
                }
            }
        }
        write_txn.commit().map_err(redb::Error::from)?;

        // Now read the initialized data
        let read_txn = db.begin_read().map_err(redb::Error::from)?;
        let bits_table =
            read_txn.open_table(BITS_TABLE).map_err(redb::Error::from)?;
        let timestamps_table = read_txn
            .open_table(TIMESTAMPS_TABLE)
            .map_err(redb::Error::from)?;

        // Load existing state
        for level in 0..config.max_levels {
            let level_u8 = level as u8;

            // Load bits
            if let Some(bits) =
                bits_table.get(&level_u8).map_err(redb::Error::from)?
            {
                levels[level] =
                    bits.value().iter().map(|&byte| byte != 0).collect();
            }

            // Load timestamp
            if let Some(ts_bytes) =
                timestamps_table.get(&level_u8).map_err(redb::Error::from)?
            {
                if let Ok(duration) = bincode::deserialize(ts_bytes.value()) {
                    timestamps[level] = SystemTime::UNIX_EPOCH + duration;
                }
            }
        }

        Ok(InMemoryStorage {
            levels,
            timestamps,
            capacity: config.capacity,
        })
    }

    fn start_snapshot_thread(
        memory_storage: Arc<RwLock<InMemoryStorage>>,
        db: Arc<Database>,
        shutdown: Arc<AtomicBool>,
        interval: Duration,
    ) {
        thread::spawn(move || {
            while !shutdown.load(Ordering::Relaxed) {
                thread::sleep(interval);

                // Take snapshot
                if let Ok(storage) = memory_storage.read() {
                    Self::write_snapshot(&db, &storage).ok(); // Log error but continue
                }
            }
        });
    }

    pub fn write_snapshot(
        db: &Database,
        storage: &InMemoryStorage,
    ) -> Result<()> {
        let write_txn = db.begin_write().map_err(redb::Error::from)?;
        {
            // Write bit vectors
            let mut bits_table = write_txn
                .open_table(BITS_TABLE)
                .map_err(redb::Error::from)?;
            for (level, bits) in storage.levels.iter().enumerate() {
                // Convert Vec<bool> to Vec<u8>
                let bytes: Vec<u8> =
                    bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
                bits_table
                    .insert(&(level as u8), bytes.as_slice())
                    .map_err(redb::Error::from)?;
            }

            // Write timestamps
            let mut timestamps_table = write_txn
                .open_table(TIMESTAMPS_TABLE)
                .map_err(redb::Error::from)?;
            for (level, &timestamp) in storage.timestamps.iter().enumerate() {
                let duration =
                    timestamp.duration_since(SystemTime::UNIX_EPOCH)?;
                let ts_bytes = bincode::serialize(&duration)
                    .map_err(|e| BloomError::SerializationError(e.to_string()))?;
                timestamps_table
                    .insert(&(level as u8), ts_bytes.as_slice())
                    .map_err(redb::Error::from)?;
            }
        }
        write_txn.commit().map_err(redb::Error::from)?;
        Ok(())
    }
}

impl Drop for RedbExpiringBloomFilter {
    fn drop(&mut self) {
        // Signal thread to stop
        self.shutdown.store(true, Ordering::Relaxed);

        // Take final snapshot
        if let Ok(storage) = self.memory_storage.read() {
            let _ = Self::write_snapshot(&self.db, &storage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /* #[test]
    fn test_builder_required_fields() {
        // Test builder with only required fields
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .build();

        assert!(result.is_ok());
        let opts = result.unwrap();
        assert_eq!(opts.false_positive_rate, 0.01); // Check default value
        assert_eq!(opts.max_levels, 5); // Check default value
    }

    #[test]
    fn test_builder_custom_fields() {
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .false_positive_rate(0.001)
            .max_levels(10)
            .build();

        assert!(result.is_ok());
        let opts = result.unwrap();
        assert_eq!(opts.false_positive_rate, 0.001);
        assert_eq!(opts.max_levels, 10);
    }

    #[test]
    fn test_builder_missing_required() {
        // Test missing path
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .capacity(1000)
            .expiration_time(Duration::from_secs(3600))
            .build();
        assert!(result.is_err());

        // Test missing capacity
        let result = RedbExpiringBloomFilterOptionsBuilder::default()
            .path("filter_tests.redb".into())
            .expiration_time(Duration::from_secs(3600))
            .build();
        assert!(result.is_err());
    } */
}
