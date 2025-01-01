use std::time::SystemTime;
use thiserror::Error;

pub use crate::inmemory_storage::InMemoryStorage;

pub type Result<T> = std::result::Result<T, BloomError>;

#[derive(Error, Debug)]
pub enum BloomError {
    #[error("Storage operation failed: {0}")]
    StorageError(String),

    #[error("Index out of bounds: {index} >= {capacity}")]
    IndexOutOfBounds { index: usize, capacity: usize },

    #[error("Invalid level: {level} >= {max_levels}")]
    InvalidLevel { level: usize, max_levels: usize },

    #[cfg(feature = "redis")]
    #[error("Redis error: {0}")]
    RedisError(#[from] rustis::Error),

    #[cfg(feature = "redb")]
    #[error("ReDB error: {0}")]
    RedbError(#[from] redb::Error),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

// Trait for the storage backend
pub trait BloomFilterStorage {
    /// Creates a new storage instance
    fn new(capacity: usize, max_levels: usize) -> Result<Self>
    where
        Self: Sized;
    /// Sets a bit at the specified level and index
    fn set_bit(&mut self, level: usize, index: usize) -> Result<()>;
    /// Gets a bit value at the specified level and index
    fn get_bit(&self, level: usize, index: usize) -> Result<bool>;
    /// Clears all bits in the specified level
    fn clear_level(&mut self, level: usize) -> Result<()>;
    /// Sets the timestamp for a level
    fn set_timestamp(
        &mut self,
        level: usize,
        timestamp: SystemTime,
    ) -> Result<()>;
    /// Gets the timestamp for a level
    fn get_timestamp(&self, level: usize) -> Result<Option<SystemTime>>;
    /// Returns the number of levels in the storage
    fn num_levels(&self) -> usize;
}
