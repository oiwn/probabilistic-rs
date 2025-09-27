use super::{
    BloomError, BloomFilterConfig, BloomFilterOps, BloomResult, StorageBackend,
    storage::FjallBackend,
};
use crate::{
    bloom::traits::BloomFilterStats,
    hash::{default_hash_function, optimal_bit_vector_size, optimal_num_hashes},
};
use bitvec::{bitvec, order::Lsb0, vec::BitVec};
use tracing::{debug, info, warn};

use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct BloomFilter {
    config: BloomFilterConfig,
    pub bit_vector_size: usize,
    pub num_hashes: usize,
    insert_count: AtomicUsize,

    // Read-heavy data
    bits: Arc<RwLock<BitVec<usize, Lsb0>>>,
    pub(crate) dirty_chunks: Option<Arc<RwLock<BitVec<usize, Lsb0>>>>,

    // Persistence support
    #[cfg(feature = "fjall")]
    pub storage: Option<FjallBackend>,
    chunk_size_bytes: usize,
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
                    BloomError::StorageError(format!(
                        "Failed to create db directory: {e}"
                    ))
                })?;
            }

            // Delete existing DB if present
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

            let storage =
                FjallBackend::new(persistence_config.db_path.clone()).await?;
            info!(
                "Created new Fjall backend at {:?}",
                persistence_config.db_path
            );

            // Save config to new DB
            storage.save_config(&config).await?;
            info!("Saved config to database.");

            Some(storage)
        } else {
            None
        };

        Self::build_filter(config, storage).await
    }

    /// Loads an existing bloom filter from database
    /// Returns error if database doesn't exist
    #[cfg(feature = "fjall")]
    pub async fn load(db_path: PathBuf) -> BloomResult<Self> {
        // Check if DB exists
        if !db_path.exists() {
            return Err(BloomError::StorageError(format!(
                "Database does not exist at {db_path:?}"
            )));
        }

        // Create Fjall backend for existing DB
        let backend = FjallBackend::new(db_path.clone()).await?;
        info!("Created Fjall backend for existing DB at {:?}", db_path);

        // Load config from DB
        let loaded_config = backend.load_config().await?;
        info!(
            "Loaded config from DB - capacity: {}, FPR: {:.3}%",
            loaded_config.capacity,
            loaded_config.false_positive_rate * 100.0
        );

        // Build filter with loaded config
        let mut filter = Self::build_filter(loaded_config, Some(backend)).await?;

        // Load snapshot data from DB

        if let Some(ref backend) = filter.storage {
            let chunks = backend.load_snapshot().await?;
            filter.reconstruct_from_chunks(&chunks)?;
            info!("Loaded {} chunks from database", chunks.len());
        }

        Ok(filter)
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
    }

    /// Internal helper to build the actual BloomFilter struct
    async fn build_filter(
        config: BloomFilterConfig,
        storage: Option<FjallBackend>,
    ) -> BloomResult<Self> {
        let bit_vector_size =
            optimal_bit_vector_size(config.capacity, config.false_positive_rate);
        let num_hashes = optimal_num_hashes(config.capacity, bit_vector_size);
        let bits = Arc::new(RwLock::new(bitvec![0; bit_vector_size]));

        // Setup chunking if persistence enabled
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
            bits,
            insert_count: AtomicUsize::new(0),
            #[cfg(feature = "fjall")]
            storage,
            chunk_size_bytes,
            dirty_chunks,
        })
    }

    pub async fn save_snapshot(&self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        if let Some(ref backend) = self.storage {
            // Extract all chunks (not just dirty ones for now - keep it simple)
            let chunks = self.extract_all_chunks();
            backend.save_snapshot(&chunks).await?;
            info!("Saved {} chunks to database", chunks.len());
        }
        Ok(())
    }

    fn extract_all_chunks(&self) -> Vec<(usize, Vec<u8>)> {
        let mut chunks = Vec::new();

        if self.chunk_size_bytes > 0 {
            let bits = self.bits.read().unwrap(); // Add this lock
            let chunk_size_bits = self.chunk_size_bytes * 8;
            let num_chunks = (self.bit_vector_size + chunk_size_bits - 1)
                .div_ceil(chunk_size_bits);

            for chunk_id in 0..num_chunks {
                let chunk_data = self.extract_chunk_bytes_with_bits(
                    &bits, // Pass the locked bits
                    chunk_id,
                    chunk_size_bits,
                );
                chunks.push((chunk_id, chunk_data));
            }

            debug!("Extracted {} chunks for snapshot", chunks.len());
        }

        chunks
    }

    pub fn extract_dirty_chunks(&self) -> Vec<(usize, Vec<u8>)> {
        let mut chunks = Vec::new();

        if let Some(ref dirty_chunks_arc) = self.dirty_chunks {
            let dirty_chunks = dirty_chunks_arc.read().unwrap();
            let bits = self.bits.read().unwrap();
            let chunk_size_bits = self.chunk_size_bytes * 8;

            for chunk_id in 0..dirty_chunks.len() {
                if dirty_chunks[chunk_id] {
                    let chunk_data = self.extract_chunk_bytes_with_bits(
                        &bits,
                        chunk_id,
                        chunk_size_bits,
                    );
                    chunks.push((chunk_id, chunk_data));
                }
            }
            debug!("Extracted {} dirty chunks for snapshot", chunks.len());
        }

        chunks
    }

    fn extract_chunk_bytes_with_bits(
        &self,
        bits: &BitVec<usize, Lsb0>,
        chunk_id: usize,
        chunk_size_bits: usize,
    ) -> Vec<u8> {
        let start_bit = chunk_id * chunk_size_bits;

        // Safety check: if start_bit >= bits.len(), return empty chunk
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

    /* fn extract_chunk_bytes(
        &self,
        chunk_id: usize,
        chunk_size_bits: usize,
    ) -> Vec<u8> {
        let start_bit = chunk_id * chunk_size_bits;
        let end_bit = std::cmp::min(start_bit + chunk_size_bits, self.bits.len());

        // Convert bit range to bytes
        let chunk_bits = &self.bits[start_bit..end_bit];
        let mut bytes = Vec::new();

        // Pack bits into bytes (8 bits per byte)
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
    } */

    fn reconstruct_from_chunks(
        &mut self,
        chunks: &[(usize, Vec<u8>)],
    ) -> BloomResult<()> {
        let chunk_size_bits = self.chunk_size_bytes * 8;

        // Get write lock for the entire reconstruction
        let mut bits = self.bits.write().unwrap();

        for (chunk_id, chunk_bytes) in chunks {
            let start_bit = chunk_id * chunk_size_bits;

            for (byte_idx, &byte) in chunk_bytes.iter().enumerate() {
                for bit_pos in 0..8 {
                    let bit_idx = start_bit + byte_idx * 8 + bit_pos;
                    if bit_idx < bits.len() {
                        // Use bits instead of self.bits
                        let bit_value = (byte & (1 << bit_pos)) != 0;
                        bits.set(bit_idx, bit_value); // Use bits instead of self.bits
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
        let words = binding.as_raw_slice(); // &[usize]
        // words.len() * std::mem::size_of::<usize>()
        std::mem::size_of_val(words)
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

impl BloomFilterOps for BloomFilter {
    fn insert(&self, item: &[u8]) -> BloomResult<()> {
        let indices =
            default_hash_function(item, self.num_hashes, self.bit_vector_size);

        // Get write locks
        let mut bits = self.bits.write().unwrap();

        for idx in indices {
            let idx = idx as usize;
            if idx >= self.bit_vector_size {
                return Err(BloomError::IndexOutOfBounds {
                    index: idx,
                    capacity: self.bit_vector_size,
                });
            }

            // Mark chunk as dirty when setting bits
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
