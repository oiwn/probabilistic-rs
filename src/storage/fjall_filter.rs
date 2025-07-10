use crate::{
    error::{FilterError, Result},
    filter::{ExpiringBloomFilter, FilterConfig},
    hash::{calculate_optimal_params, default_hash_function},
    storage::{FilterStorage, InMemoryStorage},
};
use derive_builder::Builder;
use fjall::{
    Config as FjallConfig, Keyspace, Partition, PartitionCreateOptions,
    PersistMode,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

// Configuration for FjallFilter with builder pattern
#[derive(Builder, Clone)]
#[builder(pattern = "owned")]
pub struct FjallFilterConfig {
    #[builder(default = "PathBuf::from(\"bloom.fjall\")")]
    pub db_path: PathBuf,
    #[builder(default = "None")]
    pub filter_config: Option<FilterConfig>,
    #[builder(default = "Duration::from_secs(60)")]
    pub snapshot_interval: Duration,
}

// Main FjallFilter implementation
pub struct FjallFilter {
    pub storage: InMemoryStorage,
    config: FilterConfig,
    num_hashes: usize,
    current_level_index: AtomicUsize,
    keyspace: Arc<Keyspace>,
    // Add these fields to cache the partitions
    bits_partition: Arc<Partition>,
    timestamps_partition: Arc<Partition>,
    // threading
    dirty: Arc<AtomicBool>,
    snapshot_interval: Duration,
    last_snapshot: RwLock<SystemTime>,
}

impl FjallFilter {
    /// Creates a new or opens an existing FjallBloomFilter.
    pub fn new(config: FjallFilterConfig) -> Result<Self> {
        let db_exists = config.db_path.exists();

        // Open or create Fjall database
        let fjall_config = FjallConfig::new(&config.db_path);
        let keyspace = Arc::new(fjall_config.open().map_err(|e| {
            FilterError::StorageError(format!("Failed to open Fjall DB: {e}"))
        })?);

        // Handle configuration based on database existence
        let filter_config = if db_exists {
            // Database exists, try to load configuration
            match Self::load_config(&keyspace)? {
                Some(loaded_config) => loaded_config,
                _ => {
                    return Err(FilterError::StorageError(
                        "Database exists but no configuration found".to_string(),
                    ));
                }
            }
        } else {
            // Database doesn't exist, require configuration
            let filter_config = config.filter_config.ok_or_else(|| {
                FilterError::InvalidConfig(
                    "Configuration required for new database".to_string(),
                )
            })?;

            // Save configuration
            Self::save_config(&keyspace, &filter_config)?;

            filter_config
        };

        let (_level_fpr, bit_vector_size, num_hashes) = calculate_optimal_params(
            filter_config.capacity,
            filter_config.false_positive_rate,
            filter_config.max_levels,
            0.8, // Default active ratio
        );

        let storage =
            InMemoryStorage::new(bit_vector_size, filter_config.max_levels)?;

        // State for background thread coordination
        let dirty = Arc::new(AtomicBool::new(false));

        let options = PartitionCreateOptions::default()
            .compression(fjall::CompressionType::None);

        // Open partitions once during initialization
        let bits_partition =
            Arc::new(keyspace.open_partition("bits", options.clone()).map_err(
                |e| {
                    FilterError::StorageError(format!(
                        "Failed to open bits partition: {e}"
                    ))
                },
            )?);

        let timestamps_partition =
            Arc::new(keyspace.open_partition("timestamps", options).map_err(
                |e| {
                    FilterError::StorageError(format!(
                        "Failed to open timestamps partition: {e}"
                    ))
                },
            )?);

        // Create the filter instance
        let mut filter = Self {
            storage,
            config: filter_config,
            num_hashes,
            current_level_index: AtomicUsize::new(0),
            keyspace,
            // _keyspace: keyspace.clone(),
            bits_partition,
            timestamps_partition,
            dirty: dirty.clone(),
            snapshot_interval: config.snapshot_interval,
            last_snapshot: RwLock::new(SystemTime::now()),
        };

        // Load saved state from DB
        filter.load_state()?;

        Ok(filter)
    }

    pub fn config(&self) -> &FilterConfig {
        &self.config
    }

    pub fn num_hashes(&self) -> usize {
        self.num_hashes
    }

    pub fn current_level_index(&self) -> usize {
        self.current_level_index.load(Ordering::Relaxed)
    }

    /// Loads filter configuration from the database
    fn load_config(keyspace: &Arc<Keyspace>) -> Result<Option<FilterConfig>> {
        // Open config partition
        let config_partition = keyspace
            .open_partition("config", PartitionCreateOptions::default())
            .map_err(|e| {
                FilterError::StorageError(format!(
                    "Failed to open config partition: {e}"
                ))
            })?;

        // Try to get config
        if let Some(config_bytes) =
            config_partition.get("filter_config").map_err(|e| {
                FilterError::StorageError(format!("Failed to read config: {e}"))
            })?
        {
            let (capacity, false_positive_rate, max_levels, level_duration): (
                usize,
                f64,
                usize,
                Duration,
            ) = bincode::decode_from_slice(
                &config_bytes,
                bincode::config::standard(),
            )
            .map_err(|e| FilterError::SerializationError(e.to_string()))?
            .0;

            // Rebuild config with default hash function
            Ok(Some(FilterConfig {
                capacity,
                false_positive_rate,
                max_levels,
                level_duration,
                hash_function: default_hash_function,
            }))
        } else {
            // No config found
            Ok(None)
        }
    }

    /// Saves filter configuration to the database
    fn save_config(
        keyspace: &Arc<Keyspace>,
        config: &FilterConfig,
    ) -> Result<()> {
        let config_partition = keyspace
            .open_partition("config", PartitionCreateOptions::default())
            .map_err(|e| {
                FilterError::StorageError(format!(
                    "Failed to open config partition: {e}"
                ))
            })?;

        let serialized = bincode::encode_to_vec(
            (
                config.capacity,
                config.false_positive_rate,
                config.max_levels,
                config.level_duration,
            ),
            bincode::config::standard(),
        )
        .map_err(|e| FilterError::SerializationError(e.to_string()))?;

        // Store in database
        config_partition
            .insert("filter_config", serialized)
            .map_err(|e| {
                FilterError::StorageError(format!("Failed to save config: {e}"))
            })?;

        // Ensure config is persisted
        keyspace.persist(PersistMode::SyncAll).map_err(|e| {
            FilterError::StorageError(format!("Failed to persist config: {e}"))
        })?;

        Ok(())
    }

    fn load_state(&mut self) -> Result<()> {
        // Load bits
        for level in 0..self.config.max_levels {
            let level_key = format!("level_{level}");

            if let Some(bits) =
                self.bits_partition.get(&level_key).map_err(|e| {
                    FilterError::StorageError(format!("Failed to read bits: {e}"))
                })?
            {
                // Use the efficient conversion method instead of manual bit-by-bit setting
                if let Ok(bit_vec) = self.storage.bytes_to_bitvec(&bits) {
                    self.storage.levels[level] = bit_vec;
                }
            }
        }

        // Load timestamps
        for level in 0..self.config.max_levels {
            let ts_key = format!("level_{level}");
            if let Some(ts_bytes) =
                self.timestamps_partition.get(&ts_key).map_err(|e| {
                    FilterError::StorageError(format!(
                        "Failed to read timestamp: {e}"
                    ))
                })?
            {
                if let Ok((duration, _)) = bincode::decode_from_slice::<Duration, _>(
                    &ts_bytes,
                    bincode::config::standard(),
                ) {
                    self.storage.timestamps[level] =
                        SystemTime::UNIX_EPOCH + duration;
                }
            }
        }

        Ok(())
    }

    pub fn save_snapshot(&self) -> Result<()> {
        // Save bits
        for (level, bits) in self.storage.levels.iter().enumerate() {
            let level_key = format!("level_{level}");
            let bytes = self.storage.bitvec_to_bytes(bits);
            self.bits_partition.insert(&level_key, bytes).map_err(|e| {
                FilterError::StorageError(format!("Failed to save bits: {e}"))
            })?;
        }

        // Save timestamps
        for (level, &timestamp) in self.storage.timestamps.iter().enumerate() {
            let ts_key = format!("level_{level}");
            let duration = timestamp.duration_since(SystemTime::UNIX_EPOCH)?;
            let ts_bytes =
                bincode::encode_to_vec(duration, bincode::config::standard())
                    .map_err(|e| {
                        FilterError::SerializationError(e.to_string())
                    })?;

            self.timestamps_partition
                .insert(&ts_key, ts_bytes)
                .map_err(|e| {
                    FilterError::StorageError(format!(
                        "Failed to save timestamp: {e}"
                    ))
                })?;
        }

        // Ensure data is persisted
        self.keyspace.persist(PersistMode::SyncAll).map_err(|e| {
            FilterError::StorageError(format!("Failed to persist snapshot: {e}"))
        })?;

        Ok(())
    }

    fn should_create_new_level(&self) -> Result<bool> {
        let current_level = self.current_level_index.load(Ordering::Relaxed);
        if let Some(last_timestamp) = self.storage.get_timestamp(current_level)? {
            let now = SystemTime::now();
            Ok(now.duration_since(last_timestamp)? >= self.config.level_duration)
        } else {
            Ok(true)
        }
    }

    fn create_new_level(&mut self) -> Result<()> {
        let current = self.current_level_index.load(Ordering::Relaxed);
        let new_index = (current + 1) % self.config.max_levels;
        self.current_level_index.store(new_index, Ordering::Relaxed);

        self.storage.clear_level(new_index)?;
        self.storage.set_timestamp(new_index, SystemTime::now())?;
        self.dirty.store(true, Ordering::Relaxed);
        Ok(())
    }
}

impl ExpiringBloomFilter for FjallFilter {
    fn insert(&mut self, item: &[u8]) -> Result<()> {
        if self.should_create_new_level()? {
            self.create_new_level()?;
        }

        let indices: Vec<usize> = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        // Set bits at current level
        let current_level = self.current_level_index.load(Ordering::Relaxed);
        self.storage.set_bits(current_level, &indices)?;

        // Signal thread to shut down
        self.dirty.store(true, Ordering::Relaxed);

        // Snapshot logic
        let now = SystemTime::now();
        {
            let last_snapshot = self.last_snapshot.read().unwrap();
            if now.duration_since(*last_snapshot)? >= self.snapshot_interval {
                drop(last_snapshot); // release read lock
                let mut last_snapshot = self.last_snapshot.write().unwrap();
                self.save_snapshot()?;
                *last_snapshot = now;
                self.dirty.store(false, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    fn query(&self, item: &[u8]) -> Result<bool> {
        let indices: Vec<usize> = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.config.capacity,
        )
        .into_iter()
        .map(|h| h as usize)
        .collect();

        let now = SystemTime::now();

        for level in 0..self.config.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                let elapsed = now.duration_since(timestamp)?;

                if elapsed
                    <= self.config.level_duration * self.config.max_levels as u32
                {
                    let bits = self.storage.get_bits(level, &indices)?;
                    if bits.iter().all(|&bit| bit) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn cleanup_expired_levels(&mut self) -> Result<()> {
        let now = SystemTime::now();
        for level in 0..self.config.max_levels {
            if let Some(timestamp) = self.storage.get_timestamp(level)? {
                let elapsed = now.duration_since(timestamp)?;
                if elapsed
                    >= self.config.level_duration * self.config.max_levels as u32
                {
                    self.storage.clear_level(level)?;
                }
            }
        }
        self.save_snapshot()?;
        Ok(())
    }
}

impl Drop for FjallFilter {
    fn drop(&mut self) {
        // FIXME: this is probably because of benchmarks
        // Take final snapshot on drop if dirty
        if self.dirty.load(Ordering::Relaxed) {
            // if let Err(err) = self.save_snapshot() {
            //     error!("Error saving snapshot: {}", err);
            // }
        }
    }
}
