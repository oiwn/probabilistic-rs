use thiserror::Error;

pub type CuckooResult<T> = std::result::Result<T, CuckooError>;

#[derive(Error, Debug)]
pub enum CuckooError {
    #[error("Filter is at capacity — resize to insert more items")]
    CapacityExceeded,

    #[error(
        "All fingerprints collided to zero — unable to find valid fingerprint"
    )]
    ZeroFingerprint,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Capacity must be greater than 0")]
    ZeroCapacity,

    #[error("Fingerprint bits must be between 4 and 16, got {0}")]
    InvalidFingerprintBits(usize),

    #[error("Entries per bucket must be between 2 and 8, got {0}")]
    InvalidEntriesPerBucket(usize),

    #[error("Max kicks must be greater than 0")]
    ZeroMaxKicks,

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Storage backend error: {0}")]
    StorageError(String),

    #[error("No configuration found in storage")]
    ConfigNotFound,

    #[error("No snapshot data found in storage")]
    SnapshotNotFound,

    #[cfg(feature = "fjall")]
    #[error("Fjall error: {0}")]
    FjallError(#[from] Box<fjall::Error>),
}
