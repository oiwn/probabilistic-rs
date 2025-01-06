use crate::RedbSlidingBloomFilter;
use serde::{Deserialize, Serialize};
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
