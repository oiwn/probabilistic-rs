use crate::RedbSlidingBloomFilter;
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
    #[builder(default = "10000")]
    pub bloom_capacity: usize,
    #[builder(default = "0.01")]
    pub bloom_false_positive_rate: f64,
    #[builder(default = "Duration::from_secs(60)")]
    pub bloom_level_duration: Duration,
    #[builder(default = "3")]
    pub bloom_max_levels: usize,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, String> {
        dotenvy::dotenv().ok();

        Ok(Self {
            server_host: std::env::var("SERVER_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            server_port: std::env::var("SERVER_PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .map_err(|_| "Invalid SERVER_PORT")?,
            bloom_db_path: std::env::var("BLOOM_DB_PATH")
                .unwrap_or_else(|_| "bloom.redb".to_string()),
            bloom_capacity: std::env::var("BLOOM_CAPACITY")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .map_err(|_| "Invalid BLOOM_CAPACITY")?,
            bloom_false_positive_rate: std::env::var("BLOOM_FALSE_POSITIVE_RATE")
                .unwrap_or_else(|_| "0.01".to_string())
                .parse()
                .map_err(|_| "Invalid BLOOM_FALSE_POSITIVE_RATE")?,
            bloom_level_duration: Duration::from_secs(
                std::env::var("BLOOM_LEVEL_DURATION_SECS")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()
                    .map_err(|_| "Invalid BLOOM_LEVEL_DURATION_SECS")?,
            ),
            bloom_max_levels: std::env::var("BLOOM_MAX_LEVELS")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .map_err(|_| "Invalid BLOOM_MAX_LEVELS")?,
        })
    }
}
