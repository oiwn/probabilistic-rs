use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::Arc;
use tracing::debug;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::filter::SlidingBloomFilter;
use crate::types::{AppState, ErrorResponse, InsertRequest, QueryResponse};

#[derive(OpenApi)]
#[openapi(
    paths(
        health_check,
        insert_item,
        query_item,
        cleanup_expired,
    ),
    components(
        schemas(InsertRequest, QueryResponse, ErrorResponse)
    ),
    tags(
        (name = "bloom-filter", description = "Time-Decaying Bloom Filter API")
    )
)]
struct ApiDoc;

/// Check API health
#[utoipa::path(
    get,
    path = "/health",
    tag = "bloom-filter",
    responses(
        (status = 200, description = "API is healthy")
    )
)]
async fn health_check() -> impl IntoResponse {
    debug!("Health check");
    StatusCode::OK
}

/// Insert an item into the Bloom filter
#[utoipa::path(
    post,
    path = "/items",
    tag = "bloom-filter",
    request_body = InsertRequest,
    responses(
        (status = 200, description = "Item inserted successfully"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn insert_item(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InsertRequest>,
) -> impl IntoResponse {
    debug!("Inserting item: {}", &request.value);
    let mut filter = state.filter.lock().await;
    match filter.insert(request.value.as_bytes()) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Query if an item exists in the Bloom filter
#[utoipa::path(
    get,
    path = "/items/{value}",
    tag = "bloom-filter",
    params(
        ("value" = String, Path, description = "Value to query")
    ),
    responses(
        (status = 200, description = "Query successful", body = QueryResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn query_item(
    State(state): State<Arc<AppState>>,
    Path(value): Path<String>,
) -> impl IntoResponse {
    debug!("Querying item: {}", &value);
    let filter = state.filter.lock().await;
    match filter.query(value.as_bytes()) {
        Ok(exists) => {
            (StatusCode::OK, Json(QueryResponse { exists })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Clean up expired items
#[utoipa::path(
    post,
    path = "/cleanup",
    tag = "bloom-filter",
    responses(
        (status = 200, description = "Cleanup successful"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
async fn cleanup_expired(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let mut filter = state.filter.lock().await;
    match filter.cleanup_expired_levels() {
        Ok(_) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

pub fn create_router(state: Arc<AppState>) -> Router {
    let openapi = ApiDoc::openapi();

    Router::new()
        .merge(
            SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi),
        )
        .route("/health", get(health_check))
        .route("/items", post(insert_item))
        .route("/items/{value}", get(query_item))
        .route("/cleanup", post(cleanup_expired))
        .with_state(state)
}
