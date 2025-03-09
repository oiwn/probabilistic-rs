use crate::error::{BloomError, Result};
use crate::filter::{FilterConfig, SlidingBloomFilter};
use crate::hash::{
    default_hash_function, optimal_bit_vector_size, optimal_num_hashes,
};
use crate::storage::{BloomStorage, InMemoryStorage};
use redb::{Database, TableDefinition};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

// Define table schemas for ReDB
const BITS_TABLE: TableDefinition<u8, &[u8]> = TableDefinition::new("bits");
const TIMESTAMPS_TABLE: TableDefinition<u8, &[u8]> =
    TableDefinition::new("timestamps");
const CONFIG_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("config");

pub struct RedbSlidingBloomFilter {
    pub storage: InMemoryStorage,
    config: FilterConfig,
    num_hashes: usize,
    current_level_index: usize,
    db: Arc<Database>,
}

impl RedbSlidingBloomFilter {
    /// Creates a new or opens an existing RedbSlidingBloomFilter.
    ///
    /// If the database file already exists, it loads the configuration from
    /// the database. In this case, the provided config parameter is ignored.
    ///
    /// If the database file doesn't exist, it creates a new one with the provided
    /// configuration, which must be Some.
    pub fn new(config: Option<FilterConfig>, db_path: PathBuf) -> Result<Self> {
        let db_exists = db_path.exists();

        // Handle configuration based on database existence
        let config = if db_exists {
            // Database exists, try to load configuration
            let db =
                Arc::new(Database::open(&db_path).map_err(redb::Error::from)?);
            match Self::load_config(&db)? {
                Some(loaded_config) => (loaded_config, db),
                None => {
                    return Err(BloomError::StorageError(
                        "Database exists but no configuration found".to_string(),
                    ));
                }
            }
        } else {
            // Database doesn't exist, require configuration
            let config = config.ok_or_else(|| {
                BloomError::InvalidConfig(
                    "Configuration required for new database".to_string(),
                )
            })?;

            // Create new database
            let db =
                Arc::new(Database::create(&db_path).map_err(redb::Error::from)?);

            // Save configuration
            Self::save_config(&db, &config)?;

            (config, db)
        };

        let (config, db) = config;

        let storage = InMemoryStorage::new(config.capacity, config.max_levels)?;
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);

        // Initialize filter
        let mut filter = Self {
            storage,
            config,
            num_hashes,
            current_level_index: 0,
            db,
        };

        filter.load_state()?;
        Ok(filter)
    }

    pub fn get_config(&self) -> &FilterConfig {
        &self.config
    }

    pub fn get_current_level_index(&self) -> usize {
        self.current_level_index
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
            // Deserialize config
            let (capacity, false_positive_rate, max_levels, level_duration): (
                usize,
                f64,
                usize,
                Duration,
            ) = bincode::deserialize(config_bytes.value())
                .map_err(|e| BloomError::SerializationError(e.to_string()))?;

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

            // Serialize important config fields
            let serialized = bincode::serialize(&(
                config.capacity,
                config.false_positive_rate,
                config.max_levels,
                config.level_duration,
            ))
            .map_err(|e| BloomError::SerializationError(e.to_string()))?;

            // Store in database
            config_table
                .insert("filter_config", serialized.as_slice())
                .map_err(redb::Error::from)?;
        }
        write_txn.commit().map_err(redb::Error::from)?;

        Ok(())
    }

    fn load_state(&mut self) -> Result<()> {
        let read_txn = self.db.begin_read().map_err(redb::Error::from)?;

        // Load bits
        if let Ok(bits_table) = read_txn.open_table(BITS_TABLE) {
            for level in 0..self.config.max_levels {
                let level_u8 = level as u8;
                if let Ok(Some(bits)) = bits_table.get(&level_u8) {
                    let bit_vec: Vec<bool> =
                        bits.value().iter().map(|&byte| byte != 0).collect();
                    if bit_vec.len() == self.config.capacity {
                        self.storage.levels[level] = bit_vec;
                    }
                }
            }
        }

        // Load timestamps
        if let Ok(timestamps_table) = read_txn.open_table(TIMESTAMPS_TABLE) {
            for level in 0..self.config.max_levels {
                let level_u8 = level as u8;
                if let Ok(Some(ts_bytes)) = timestamps_table.get(&level_u8) {
                    if let Ok(duration) = bincode::deserialize(ts_bytes.value()) {
                        self.storage.timestamps[level] =
                            SystemTime::UNIX_EPOCH + duration;
                    }
                }
            }
        }

        Ok(())
    }

    fn save_snapshot(&self) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(redb::Error::from)?;

        // Save bits
        {
            let mut bits_table = write_txn
                .open_table(BITS_TABLE)
                .map_err(redb::Error::from)?;

            for (level, bits) in self.storage.levels.iter().enumerate() {
                let bytes: Vec<u8> =
                    bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
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

    fn should_create_new_level(&self) -> Result<bool> {
        let current_level = self.current_level_index;
        if let Some(last_timestamp) = self.storage.get_timestamp(current_level)? {
            let now = SystemTime::now();
            Ok(now.duration_since(last_timestamp)? >= self.config.level_duration)
        } else {
            Ok(true)
        }
    }

    fn create_new_level(&mut self) -> Result<()> {
        self.current_level_index =
            (self.current_level_index + 1) % self.config.max_levels;
        self.storage.clear_level(self.current_level_index)?;
        self.storage
            .set_timestamp(self.current_level_index, SystemTime::now())?;
        self.save_snapshot()?;
        Ok(())
    }
}

impl SlidingBloomFilter for RedbSlidingBloomFilter {
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

        self.storage.set_bits(self.current_level_index, &indices)?;
        // TODO: run separate thread for it
        self.save_snapshot()?;
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

impl Drop for RedbSlidingBloomFilter {
    fn drop(&mut self) {
        // Take final snapshot on drop
        let _ = self.save_snapshot();
    }
}
