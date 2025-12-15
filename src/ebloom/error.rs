use thiserror::Error;

use bincode::error::{DecodeError, EncodeError};

pub type Result<T> = std::result::Result<T, EbloomError>;
pub type EbloomResult<T> = Result<T>; // Alias for backward compatibility

#[derive(Error, Debug, PartialEq)]
pub enum EbloomError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Index out of bounds: index {index}, capacity {capacity}")]
    IndexOutOfBounds { index: usize, capacity: usize },

    #[error("Invalid level: level {level}, max levels {max_levels}")]
    InvalidLevel { level: usize, max_levels: usize },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Lock error: {0}")]
    LockError(String),

    #[error("Time error: {0}")]
    TimeError(String),
}

// Conversion from String to EbloomError (for validation errors)
impl From<String> for EbloomError {
    fn from(msg: String) -> Self {
        EbloomError::InvalidConfig(msg)
    }
}

// Conversion from serde_json::Error to EbloomError
impl From<serde_json::Error> for EbloomError {
    fn from(err: serde_json::Error) -> Self {
        EbloomError::SerializationError(err.to_string())
    }
}

// Conversion from bincode::error::EncodeError to EbloomError
impl From<EncodeError> for EbloomError {
    fn from(err: EncodeError) -> Self {
        EbloomError::SerializationError(err.to_string())
    }
}

// Conversion from bincode::error::DecodeError to EbloomError
impl From<DecodeError> for EbloomError {
    fn from(err: DecodeError) -> Self {
        EbloomError::SerializationError(err.to_string())
    }
}
