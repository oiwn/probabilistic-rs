use thiserror::Error;

pub type BloomResult<T> = std::result::Result<T, BloomError>;

#[derive(Error, Debug)]
pub enum BloomError {
    #[error("Index out of bounds: {index} >= {capacity}")]
    IndexOutOfBounds { index: usize, capacity: usize },

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Capacity must be greater than 0")]
    ZeroCapacity,

    #[error("False positive rate must be between 0 and 1, got {rate}")]
    InvalidFalsePositiveRate { rate: f64 },

    #[error("Storage backend error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("No configuration found in storage")]
    ConfigNotFound,

    #[error("No snapshot data found in storage")]
    SnapshotNotFound,

    #[cfg(feature = "fjall")]
    #[error("Fjall error: {0}")]
    FjallError(#[from] Box<fjall::Error>),
}
