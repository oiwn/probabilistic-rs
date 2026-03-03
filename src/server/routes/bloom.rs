use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::bloom::{BloomFilter, BloomFilterConfigBuilder, BloomFilterOps, BloomFilterStats, BulkBloomFilterOps};
use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateBloomRequest {
    pub name: String,
    #[schema(default = 1_000_000)]
    pub capacity: usize,
    #[schema(default = 0.01)]
    pub fpr: f64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ItemRequest {
    pub item: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkItemRequest {
    pub items: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContainsResponse {
    pub present: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkContainsResponse {
    pub results: Vec<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BloomStatsResponse {
    pub capacity: usize,
    pub false_positive_rate: f64,
    pub insert_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FilterListResponse {
    pub filters: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_filter))
        .route("/list", get(list_filters))
        .route("/{name}", delete(delete_filter))
        .route("/{name}/insert", post(insert))
        .route("/{name}/contains", post(contains))
        .route("/{name}/bulk/insert", post(bulk_insert))
        .route("/{name}/bulk/contains", post(bulk_contains))
        .route("/{name}/clear", post(clear))
        .route("/{name}/stats", get(stats))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom",
    request_body = CreateBloomRequest,
    responses(
        (status = 201, description = "Filter created", body = MessageResponse),
        (status = 409, description = "Filter already exists", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub async fn create_filter(
    State(state): State<AppState>,
    Json(req): Json<CreateBloomRequest>,
) -> ApiResult<Json<MessageResponse>> {
    let mut blooms = state.blooms.write().await;

    if blooms.contains_key(&req.name) {
        return Err(ApiError::already_exists("Bloom filter", &req.name));
    }

    let config = BloomFilterConfigBuilder::default()
        .capacity(req.capacity)
        .false_positive_rate(req.fpr)
        .build()
        .map_err(|e| ApiError::with_details("Invalid config", e.to_string()))?;

    let filter = BloomFilter::create(config).await.map_err(ApiError::from)?;
    blooms.insert(req.name.clone(), Arc::new(filter));

    Ok(Json(MessageResponse {
        message: format!("Bloom filter '{}' created", req.name),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/bloom/{name}",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    responses(
        (status = 200, description = "Filter deleted", body = MessageResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn delete_filter(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<MessageResponse>> {
    let mut blooms = state.blooms.write().await;

    if blooms.remove(&name).is_none() {
        return Err(ApiError::not_found("Bloom filter", &name));
    }

    Ok(Json(MessageResponse {
        message: format!("Bloom filter '{}' deleted", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom/{name}/insert",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    request_body = ItemRequest,
    responses(
        (status = 200, description = "Item inserted", body = MessageResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn insert(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ItemRequest>,
) -> ApiResult<Json<MessageResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    filter.insert(req.item.as_bytes()).map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: "Item inserted".to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom/{name}/contains",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    request_body = ItemRequest,
    responses(
        (status = 200, description = "Check result", body = ContainsResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn contains(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ItemRequest>,
) -> ApiResult<Json<ContainsResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    let present = filter.contains(req.item.as_bytes()).map_err(ApiError::from)?;

    Ok(Json(ContainsResponse { present }))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom/{name}/bulk/insert",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    request_body = BulkItemRequest,
    responses(
        (status = 200, description = "Items inserted", body = MessageResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn bulk_insert(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<BulkItemRequest>,
) -> ApiResult<Json<MessageResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    let items: Vec<&[u8]> = req.items.iter().map(|s| s.as_bytes()).collect();
    filter.insert_bulk(&items).map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: format!("{} items inserted", req.items.len()),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom/{name}/bulk/contains",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    request_body = BulkItemRequest,
    responses(
        (status = 200, description = "Check results", body = BulkContainsResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn bulk_contains(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<BulkItemRequest>,
) -> ApiResult<Json<BulkContainsResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    let items: Vec<&[u8]> = req.items.iter().map(|s| s.as_bytes()).collect();
    let results = filter.contains_bulk(&items).map_err(ApiError::from)?;

    Ok(Json(BulkContainsResponse { results }))
}

#[utoipa::path(
    post,
    path = "/api/v1/bloom/{name}/clear",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    responses(
        (status = 200, description = "Filter cleared", body = MessageResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn clear(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<MessageResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    filter.clear().map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: format!("Bloom filter '{}' cleared", name),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/bloom/{name}/stats",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    responses(
        (status = 200, description = "Filter statistics", body = BloomStatsResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<BloomStatsResponse>> {
    let blooms = state.blooms.read().await;

    let filter = blooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Bloom filter", &name))?;

    Ok(Json(BloomStatsResponse {
        capacity: filter.capacity(),
        false_positive_rate: filter.false_positive_rate(),
        insert_count: filter.insert_count(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/bloom/list",
    responses(
        (status = 200, description = "List of filters", body = FilterListResponse)
    )
)]
pub async fn list_filters(State(state): State<AppState>) -> Json<FilterListResponse> {
    let blooms = state.blooms.read().await;
    let filters: Vec<String> = blooms.keys().cloned().collect();
    Json(FilterListResponse { filters })
}
