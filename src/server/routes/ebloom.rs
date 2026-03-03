use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use utoipa::ToSchema;

use crate::ebloom::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilter, ExpiringBloomFilterOps,
    ExpiringBloomFilterStats, ExpiringFilterConfigBuilder,
};
use crate::server::error::{ApiError, ApiResult};
use crate::server::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEbloomRequest {
    pub name: String,
    #[schema(default = 1_000_000)]
    pub capacity: usize,
    #[schema(default = 0.01)]
    pub fpr: f64,
    #[schema(default = 3600)]
    pub ttl_secs: u64,
    #[schema(default = 3)]
    pub num_levels: usize,
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
pub struct EbloomStatsResponse {
    pub capacity_per_level: usize,
    pub target_fpr: f64,
    pub total_insert_count: u64,
    pub active_levels: usize,
    pub num_levels: usize,
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
    path = "/api/v1/ebloom",
    request_body = CreateEbloomRequest,
    responses(
        (status = 201, description = "Filter created", body = MessageResponse),
        (status = 409, description = "Filter already exists", body = ApiError),
        (status = 500, description = "Internal error", body = ApiError)
    )
)]
pub async fn create_filter(
    State(state): State<AppState>,
    Json(req): Json<CreateEbloomRequest>,
) -> ApiResult<Json<MessageResponse>> {
    let mut eblooms = state.eblooms.write().await;

    if eblooms.contains_key(&req.name) {
        return Err(ApiError::already_exists("Expiring bloom filter", &req.name));
    }

    let config = ExpiringFilterConfigBuilder::default()
        .capacity_per_level(req.capacity)
        .target_fpr(req.fpr)
        .level_duration(Duration::from_secs(req.ttl_secs / req.num_levels as u64))
        .num_levels(req.num_levels)
        .build()
        .map_err(|e| ApiError::with_details("Invalid config", e.to_string()))?;

    let filter = ExpiringBloomFilter::create(config)
        .await
        .map_err(ApiError::from)?;
    eblooms.insert(req.name.clone(), Arc::new(filter));

    Ok(Json(MessageResponse {
        message: format!("Expiring bloom filter '{}' created", req.name),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/ebloom/{name}",
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
    let mut eblooms = state.eblooms.write().await;

    if eblooms.remove(&name).is_none() {
        return Err(ApiError::not_found("Expiring bloom filter", &name));
    }

    Ok(Json(MessageResponse {
        message: format!("Expiring bloom filter '{}' deleted", name),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ebloom/{name}/insert",
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
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    filter.insert(req.item.as_bytes()).map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: "Item inserted".to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ebloom/{name}/contains",
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
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    let present = filter
        .contains(req.item.as_bytes())
        .map_err(ApiError::from)?;

    Ok(Json(ContainsResponse { present }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ebloom/{name}/bulk/insert",
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
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    let items: Vec<&[u8]> = req.items.iter().map(|s| s.as_bytes()).collect();
    filter.insert_bulk(&items).map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: format!("{} items inserted", req.items.len()),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ebloom/{name}/bulk/contains",
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
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    let items: Vec<&[u8]> = req.items.iter().map(|s| s.as_bytes()).collect();
    let results = filter.contains_bulk(&items).map_err(ApiError::from)?;

    Ok(Json(BulkContainsResponse { results }))
}

#[utoipa::path(
    post,
    path = "/api/v1/ebloom/{name}/clear",
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
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    filter.clear().map_err(ApiError::from)?;

    Ok(Json(MessageResponse {
        message: format!("Expiring bloom filter '{}' cleared", name),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ebloom/{name}/stats",
    params(
        ("name" = String, Path, description = "Filter name")
    ),
    responses(
        (status = 200, description = "Filter statistics", body = EbloomStatsResponse),
        (status = 404, description = "Filter not found", body = ApiError)
    )
)]
pub async fn stats(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<EbloomStatsResponse>> {
    let eblooms = state.eblooms.read().await;

    let filter = eblooms
        .get(&name)
        .ok_or_else(|| ApiError::not_found("Expiring bloom filter", &name))?;

    Ok(Json(EbloomStatsResponse {
        capacity_per_level: filter.capacity_per_level(),
        target_fpr: filter.target_fpr(),
        total_insert_count: filter.total_insert_count(),
        active_levels: filter.active_levels(),
        num_levels: filter.num_levels(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/ebloom/list",
    responses(
        (status = 200, description = "List of filters", body = FilterListResponse)
    )
)]
pub async fn list_filters(
    State(state): State<AppState>,
) -> Json<FilterListResponse> {
    let eblooms = state.eblooms.read().await;
    let filters: Vec<String> = eblooms.keys().cloned().collect();
    Json(FilterListResponse { filters })
}
