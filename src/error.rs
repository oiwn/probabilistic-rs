use thiserror::Error;

pub type Result<T> = std::result::Result<T, BloomError>;

#[derive(Error, Debug)]
pub enum BloomError {
    #[error("Storage operation failed: {0}")]
    StorageError(String),

    #[error("Index out of bounds: {index} >= {capacity}")]
    IndexOutOfBounds { index: usize, capacity: usize },

    #[error("Invalid level: {level} >= {max_levels}")]
    InvalidLevel { level: usize, max_levels: usize },

    #[error("SystemTime error: {0}")]
    SystemTimeError(#[from] std::time::SystemTimeError),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Failed to parse environment variable {var_name}: value '{value}' - {error}")]
    EnvParseError {
        var_name: String,
        value: String,
        error: String,
    },

    #[cfg(feature = "redb")]
    #[error("ReDB error: {0}")]
    RedbError(#[from] redb::Error),
}
