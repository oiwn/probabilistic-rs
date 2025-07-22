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
    #[cfg(feature = "redb")]
    #[error("ReDB error: {0}")]
    RedbError(#[from] redb::Error),
}
