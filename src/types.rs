use crate::{BloomError, FilterConfig, RedbSlidingBloomFilter, Result};
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::Mutex;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InsertRequest {
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct QueryResponse {
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub message: String,
}

pub struct AppState {
    pub filter: Mutex<RedbSlidingBloomFilter>,
}

#[derive(Builder, Clone)]
#[builder(pattern = "owned")]
pub struct ServerConfig {
    #[builder(default = "\"127.0.0.1\".to_string()")]
    pub server_host: String,
    #[builder(default = "3000")]
    pub server_port: u16,
    #[builder(default = "\"bloom.redb\".to_string()")]
    pub bloom_db_path: String,
    #[builder(default = "1_000_000")]
    pub bloom_capacity: usize,
    #[builder(default = "0.01")]
    pub bloom_false_positive_rate: f64,
    #[builder(default = "Duration::from_secs(60)")]
    pub bloom_level_duration: Duration,
    #[builder(default = "3")]
    pub bloom_max_levels: usize,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            server_host: std::env::var("SERVER_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            server_port: std::env::var("SERVER_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .map_err(|e: std::num::ParseIntError| {
                    BloomError::EnvParseError {
                        var_name: "SERVER_PORT".into(),
                        value: std::env::var("SERVER_PORT").unwrap_or_default(),
                        error: e.to_string(),
                    }
                })?,
            bloom_db_path: std::env::var("BLOOM_DB_PATH")
                .unwrap_or_else(|_| "bloom.redb".to_string()),
            bloom_capacity: std::env::var("BLOOM_CAPACITY")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .map_err(|e: std::num::ParseIntError| {
                    BloomError::EnvParseError {
                        var_name: "BLOOM_CAPACITY".into(),
                        value: std::env::var("BLOOM_CAPACITY")
                            .unwrap_or_default(),
                        error: e.to_string(),
                    }
                })?,
            bloom_false_positive_rate: std::env::var("BLOOM_FALSE_POSITIVE_RATE")
                .unwrap_or_else(|_| "0.01".to_string())
                .parse()
                .map_err(|e: std::num::ParseFloatError| {
                    BloomError::EnvParseError {
                        var_name: "BLOOM_FALSE_POSITIVE_RATE".into(),
                        value: std::env::var("BLOOM_FALSE_POSITIVE_RATE")
                            .unwrap_or_default(),
                        error: e.to_string(),
                    }
                })?,
            bloom_level_duration: Duration::from_secs(
                std::env::var("BLOOM_LEVEL_DURATION_SECS")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()
                    .map_err(|e: std::num::ParseIntError| {
                        BloomError::EnvParseError {
                            var_name: "BLOOM_LEVEL_DURATION_SECS".into(),
                            value: std::env::var("BLOOM_LEVEL_DURATION_SECS")
                                .unwrap_or_default(),
                            error: e.to_string(),
                        }
                    })?,
            ),
            bloom_max_levels: std::env::var("BLOOM_MAX_LEVELS")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .map_err(|e: std::num::ParseIntError| {
                    BloomError::EnvParseError {
                        var_name: "BLOOM_MAX_LEVELS".into(),
                        value: std::env::var("BLOOM_MAX_LEVELS")
                            .unwrap_or_default(),
                        error: e.to_string(),
                    }
                })?,
        })
    }
}

impl TryFrom<ServerConfig> for FilterConfig {
    type Error = BloomError;

    fn try_from(server_config: ServerConfig) -> Result<Self> {
        // Validate capacity
        if server_config.bloom_capacity == 0 {
            return Err(BloomError::InvalidConfig(
                "capacity must be greater than 0".into(),
            ));
        }

        // Validate false positive rate
        if server_config.bloom_false_positive_rate <= 0.0
            || server_config.bloom_false_positive_rate >= 1.0
        {
            return Err(BloomError::InvalidConfig(
                "false positive rate must be between 0 and 1".into(),
            ));
        }

        // Validate max levels
        if server_config.bloom_max_levels == 0 {
            return Err(BloomError::InvalidConfig(
                "max levels must be greater than 0".into(),
            ));
        }

        // Validate level duration
        if server_config.bloom_level_duration.as_secs() == 0 {
            return Err(BloomError::InvalidConfig(
                "level duration must be greater than 0 seconds".into(),
            ));
        }

        // Create FilterConfig with validated values
        Ok(FilterConfig {
            capacity: server_config.bloom_capacity,
            false_positive_rate: server_config.bloom_false_positive_rate,
            max_levels: server_config.bloom_max_levels,
            level_duration: server_config.bloom_level_duration,
            hash_function: crate::hash::default_hash_function,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_valid_conversion() {
        let server_config = ServerConfig {
            server_host: "localhost".to_string(),
            server_port: 3000,
            bloom_db_path: "test.db".to_string(),
            bloom_capacity: 1000,
            bloom_false_positive_rate: 0.01,
            bloom_level_duration: Duration::from_secs(60),
            bloom_max_levels: 3,
        };

        let filter_config = FilterConfig::try_from(server_config);
        assert!(filter_config.is_ok());
        let config = filter_config.unwrap();
        assert_eq!(config.capacity, 1000);
        assert_eq!(config.false_positive_rate, 0.01);
        assert_eq!(config.max_levels, 3);
        assert_eq!(config.level_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_invalid_capacity() {
        let server_config = ServerConfig {
            server_host: "localhost".to_string(),
            server_port: 3000,
            bloom_db_path: "test.db".to_string(),
            bloom_capacity: 0, // Invalid capacity
            bloom_false_positive_rate: 0.01,
            bloom_level_duration: Duration::from_secs(60),
            bloom_max_levels: 3,
        };

        let result = FilterConfig::try_from(server_config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BloomError::InvalidConfig(_)));
    }

    #[test]
    fn test_invalid_false_positive_rate() {
        let server_config = ServerConfig {
            server_host: "localhost".to_string(),
            server_port: 3000,
            bloom_db_path: "test.db".to_string(),
            bloom_capacity: 1000,
            bloom_false_positive_rate: 1.5, // Invalid rate
            bloom_level_duration: Duration::from_secs(60),
            bloom_max_levels: 3,
        };

        let result = FilterConfig::try_from(server_config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BloomError::InvalidConfig(_)));
    }

    #[test]
    fn test_invalid_max_levels() {
        let server_config = ServerConfig {
            server_host: "localhost".to_string(),
            server_port: 3000,
            bloom_db_path: "test.db".to_string(),
            bloom_capacity: 1000,
            bloom_false_positive_rate: 0.01,
            bloom_level_duration: Duration::from_secs(60),
            bloom_max_levels: 0, // Invalid levels
        };

        let result = FilterConfig::try_from(server_config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BloomError::InvalidConfig(_)));
    }

    #[test]
    fn test_invalid_duration() {
        let server_config = ServerConfig {
            server_host: "localhost".to_string(),
            server_port: 3000,
            bloom_db_path: "test.db".to_string(),
            bloom_capacity: 1000,
            bloom_false_positive_rate: 0.01,
            bloom_level_duration: Duration::from_secs(0), // Invalid duration
            bloom_max_levels: 3,
        };

        let result = FilterConfig::try_from(server_config);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BloomError::InvalidConfig(_)));
    }
}
