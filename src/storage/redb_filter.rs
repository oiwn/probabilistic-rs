use crate::{
    error::{FilterError, Result},
    filter::{ExpiringBloomFilter, FilterConfig},
    hash::{calculate_optimal_params, default_hash_function},
    storage::{FilterStorage, InMemoryStorage},
};
// use bitvec::{bitvec, order::Lsb0};
use derive_builder::Builder;
use redb::{Database, TableDefinition};
use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tracing::error;

// Define table schemas for ReDB
const BITS_TABLE: TableDefinition<u8, &[u8]> = TableDefinition::new("bits");
const TIMESTAMPS_TABLE: TableDefinition<u8, &[u8]> =
    TableDefinition::new("timestamps");
const CONFIG_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config");

#[derive(Builder, Clone)]
#[builder(pattern = "owned")]
pub struct RedbFilterConfig {
    #[builder(default = "PathBuf::from(\"bloom.redb\")")]
    pub db_path: PathBuf,
    #[builder(default = "None")]
    pub filter_config: Option<FilterConfig>,
    #[builder(default = "Duration::from_secs(60)")]
    pub snapshot_interval: Duration,
}

pub struct RedbFilter {
    pub storage: InMemoryStorage,
    config: FilterConfig,
    num_hashes: usize,
    current_level_index: AtomicUsize,
    db: Arc<Database>,
    // trhreading
    dirty: Arc<AtomicBool>,
    // shutdown: Arc<AtomicBool>,
    // snapshot_thread: Option<JoinHandle<()>>,
    snapshot_interval: Duration,
    last_snapshot: RwLock<SystemTime>, // Track last snapshot time
}

impl From<redb::Error> for FilterError {
    fn from(err: redb::Error) -> Self {
        FilterError::RedbError(Box::new(err))
    }
}

impl RedbFilter {
    /// Creates a new or opens an existing RedbSlidingBloomFilter.
    ///
    /// If the database file already exists, it loads the configuration from
    /// the database. In this case, the provided config parameter is ignored.
    ///
    /// If the database file doesn't exist, it creates a new one with the provided
    /// configuration, which must be Some.
    ///
    /// Run parallel threads to drop snapshots into the redb and cleanup levels
    pub fn new(config: RedbFilterConfig) -> Result<Self> {
        let db_exists = config.db_path.exists();

        // Handle configuration based on database existence
        let (filter_config, db) = if db_exists {
            // Database exists, try to load configuration
            let db = Arc::new(
                Database::open(&config.db_path).map_err(redb::Error::from)?,
            );
            match Self::load_config(&db)? {
                Some(loaded_config) => (loaded_config, db),
                None => {
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

            // Create new database
            let db = Arc::new(
                Database::create(&config.db_path).map_err(redb::Error::from)?,
            );

            // Save configuration
            Self::save_config(&db, &filter_config)?;

            (filter_config, db)
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
        // let shutdown = Arc::new(AtomicBool::new(false));
        let dirty = Arc::new(AtomicBool::new(false));

        // Create the filter instance first (without threads)
        let mut filter = Self {
            storage,
            config: filter_config,
            num_hashes,
            current_level_index: AtomicUsize::new(0),
            db: db.clone(),
            dirty: dirty.clone(),
            // shutdown: shutdown.clone(),
            // snapshot_thread: None,
            snapshot_interval: config.snapshot_interval,
            last_snapshot: RwLock::new(SystemTime::now()),
        };

        // Load saved state from DB
        filter.load_state()?;

        // TODO: in future need to do thread here

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
    fn load_config(db: &Arc<Database>) -> Result<Option<FilterConfig>> {
        let read_txn = db.begin_read().map_err(redb::Error::from)?;

        // Try to open config table, return None if it doesn't exist
        let config_table = match read_txn.open_table(CONFIG_TABLE) {
            Ok(table) => table,
            Err(_) => return Ok(None),
        };

        // Try to get config
        if let Some(config_bytes) = config_table
            .get("filter_config")
            .map_err(redb::Error::from)?
        {
            let (capacity, false_positive_rate, max_levels, level_duration): (
                usize,
                f64,
                usize,
                Duration,
            ) = bincode::decode_from_slice(
                config_bytes.value(),
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
    fn save_config(db: &Arc<Database>, config: &FilterConfig) -> Result<()> {
        let write_txn = db.begin_write().map_err(redb::Error::from)?;

        {
            let mut config_table = write_txn
                .open_table(CONFIG_TABLE)
                .map_err(redb::Error::from)?;

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

            // Store in database if key exist it will be replaced
            config_table
                .insert("filter_config", serialized.as_slice())
                .map_err(redb::Error::from)?;
        }
        write_txn.commit().map_err(redb::Error::from)?;

        Ok(())
    }

    fn load_state(&mut self) -> Result<()> {
        let read_txn = self.db.begin_read().map_err(redb::Error::from)?;

        // let bit_vector_size = self.storage.bit_vector_len();

        // Load bits
        if let Ok(bits_table) = read_txn.open_table(BITS_TABLE) {
            for level in 0..self.config.max_levels {
                let level_u8 = level as u8;
                if let Ok(Some(bits)) = bits_table.get(&level_u8)
                    && let Ok(bit_vec) =
                        self.storage.bytes_to_bitvec(bits.value())
                    {
                        self.storage.levels[level] = bit_vec;
                    }

                    // let bit_vec: Vec<bool> =
                    //     bits.value().iter().map(|&byte| byte != 0).collect();
                    // if bit_vec.len() == bit_vector_size {
                    //     let mut bit_vec_new =
                    //         bitvec![usize, Lsb0; 0; bit_vector_size];
                    //     for (i, &val) in bit_vec.iter().enumerate() {
                    //         bit_vec_new.set(i, val);
                    //     }
                    //     self.storage.levels[level] = bit_vec_new;
                    // }
            }
        }

        // Load timestamps
        if let Ok(timestamps_table) = read_txn.open_table(TIMESTAMPS_TABLE) {
            for level in 0..self.config.max_levels {
                let level_u8 = level as u8;
                if let Ok(Some(ts_bytes)) = timestamps_table.get(&level_u8)
                    && let Ok((duration, _)) =
                        bincode::decode_from_slice::<Duration, _>(
                            ts_bytes.value(),
                            bincode::config::standard(),
                        )
                    {
                        self.storage.timestamps[level] =
                            SystemTime::UNIX_EPOCH + duration;
                    }
            }
        }

        Ok(())
    }

    pub fn save_snapshot(&self) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(redb::Error::from)?;

        // Save bits
        {
            let mut bits_table = write_txn
                .open_table(BITS_TABLE)
                .map_err(redb::Error::from)?;

            for (level, bits) in self.storage.levels.iter().enumerate() {
                // let bytes: Vec<u8> =
                //     bits.iter().map(|b| if *b { 1u8 } else { 0u8 }).collect();
                let bytes = self.storage.bitvec_to_bytes(bits);
                bits_table
                    .insert(&(level as u8), bytes.as_slice())
                    .map_err(redb::Error::from)?;
            }
        }

        // Save timestamps
        {
            let mut timestamps_table = write_txn
                .open_table(TIMESTAMPS_TABLE)
                .map_err(redb::Error::from)?;

            for (level, &timestamp) in self.storage.timestamps.iter().enumerate()
            {
                let duration =
                    timestamp.duration_since(SystemTime::UNIX_EPOCH)?;
                let ts_bytes =
                    bincode::encode_to_vec(duration, bincode::config::standard())
                        .map_err(|e| {
                            FilterError::SerializationError(e.to_string())
                        })?;
                timestamps_table
                    .insert(&(level as u8), ts_bytes.as_slice())
                    .map_err(redb::Error::from)?;
            }
        }

        write_txn.commit().map_err(redb::Error::from)?;
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

impl ExpiringBloomFilter for RedbFilter {
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

    // TODO: return amount of levels cleared
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

impl Drop for RedbFilter {
    fn drop(&mut self) {
        // TODO: here will need to shutdown parallel thread

        // Take final snapshot on drop if dirty
        if self.dirty.load(Ordering::Relaxed)
            && let Err(err) = self.save_snapshot() {
                error!("Error saving snapshot: {}", err);
            }
    }
}
