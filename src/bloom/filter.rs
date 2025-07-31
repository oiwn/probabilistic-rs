use super::{
    BloomError, BloomFilterConfig, BloomFilterOps, BloomResult,
    PersistenceConfig, SnapshotConfig, StorageBackend, storage::FjallBackend,
};
use crate::{
    bloom::traits::BloomFilterStats,
    hash::{optimal_bit_vector_size, optimal_num_hashes},
};
use async_trait::async_trait;
use bitvec::{bitvec, order::Lsb0, vec::BitVec};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct BloomFilter {
    config: BloomFilterConfig,
    pub bit_vector_size: usize,
    pub num_hashes: usize,
    bits: BitVec<usize, Lsb0>,
    insert_count: AtomicUsize,

    // Persistence support
    #[cfg(feature = "fjall")]
    storage: Option<FjallBackend>,
    chunk_size_bytes: usize,
    pub(crate) dirty_chunks: Option<BitVec<usize, Lsb0>>,
}

impl BloomFilter {
    /// Creates a new bloom filter, optionally with persistence
    /// If persistence is enabled and DB exists, it will be overwritten
    pub async fn create(config: BloomFilterConfig) -> BloomResult<Self> {
        config.validate()?;

        #[cfg(feature = "fjall")]
        let storage = if let Some(persistence_config) = &config.persistence {
            // Create tmp directory if needed
            if let Some(parent) = persistence_config.db_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    BloomError::PersistenceError(format!(
                        "Failed to create db directory: {}",
                        e
                    ))
                })?;
            }

            // TODO: Delete existing DB if present
            println!(
                "TODO: Delete existing DB at {:?}",
                persistence_config.db_path
            );

            // TODO: Create new Fjall backend
            println!("TODO: Create new Fjall backend");

            // TODO: Save config to new DB
            println!("TODO: Save config to DB");

            None // Dummy for now
        } else {
            None
        };

        Self::build_filter(config, storage).await
    }

    /// Loads an existing bloom filter from database
    /// Returns error if database doesn't exist
    pub async fn load(db_path: PathBuf) -> BloomResult<Self> {
        #[cfg(feature = "fjall")]
        {
            // Check if DB exists
            if !db_path.exists() {
                return Err(BloomError::PersistenceError(format!(
                    "Database does not exist at {:?}",
                    db_path
                )));
            }

            // TODO: Create Fjall backend
            println!(
                "TODO: Create Fjall backend for existing DB at {:?}",
                db_path
            );

            // TODO: Load config from DB
            println!("TODO: Load config from DB");
            let loaded_config = BloomFilterConfig {
                capacity: 10000, // Dummy values
                false_positive_rate: 0.01,
                hash_function: crate::hash::default_hash_function,
                persistence: Some(PersistenceConfig {
                    db_path: db_path.clone(),
                    snapshot_config: SnapshotConfig::default(),
                    chunk_size_bytes: 4096,
                }),
            };

            // TODO: Load snapshot data
            println!("TODO: Load snapshot data from DB");

            Self::build_filter(loaded_config, None).await
        }

        #[cfg(not(feature = "fjall"))]
        Err(BloomError::PersistenceError(
            "Cannot load from database - fjall feature not enabled".to_string(),
        ))
    }

    /// Creates new filter or loads existing one
    /// If DB exists, loads it (ignoring config parameters)
    /// If DB doesn't exist, creates new one with provided config
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
            // No persistence, just create in-memory
            Self::create(config).await
        }

        #[cfg(not(feature = "fjall"))]
        Self::create(config).await
    }

    /// Internal helper to build the actual BloomFilter struct
    async fn build_filter(
        config: BloomFilterConfig,
        #[cfg(feature = "fjall")] storage: Option<FjallBackend>,
    ) -> BloomResult<Self> {
        // Calculate bloom filter parameters
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);
        let bits = bitvec![0; bit_vector_size];

        // Setup chunking if persistence enabled
        let (chunk_size_bytes, dirty_chunks) = if config.persistence.is_some() {
            let chunk_size =
                config.persistence.as_ref().unwrap().chunk_size_bytes;
            let chunk_count =
                (bit_vector_size + chunk_size * 8 - 1) / (chunk_size * 8);
            (chunk_size, Some(bitvec![0; chunk_count]))
        } else {
            (0, None)
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
        })
    }

    pub fn config(&self) -> &BloomFilterConfig {
        &self.config
    }

    pub fn approx_memory_bits(&self) -> usize {
        let words = self.bits.as_raw_slice(); // &[usize]
        words.len() * std::mem::size_of::<usize>()
    }

    pub fn bits_per_item(&self) -> f64 {
        self.approx_memory_bits() as f64 / self.config.capacity as f64
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

#[async_trait]
impl BloomFilterOps for BloomFilter {
    async fn insert(&mut self, item: &[u8]) -> BloomResult<()> {
        let indices = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }
            self.bits.set(idx, true);
        }

        self.insert_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn contains(&self, item: &[u8]) -> BloomResult<bool> {
        let indices = (self.config.hash_function)(
            item,
            self.num_hashes,
            self.bit_vector_size,
        );

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }
            if !self.bits[idx] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn clear(&mut self) -> BloomResult<()> {
        self.bits.fill(false);
        self.insert_count.store(0, Ordering::Relaxed);
        Ok(())
    }
}
