mod error;
mod routes;
mod state;

pub use error::ApiError;
pub use state::AppState;

use axum::Router;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::bloom::create_filter,
        routes::bloom::delete_filter,
        routes::bloom::insert,
        routes::bloom::contains,
        routes::bloom::bulk_insert,
        routes::bloom::bulk_contains,
        routes::bloom::clear,
        routes::bloom::stats,
        routes::bloom::list_filters,
        routes::ebloom::create_filter,
        routes::ebloom::delete_filter,
        routes::ebloom::insert,
        routes::ebloom::contains,
        routes::ebloom::bulk_insert,
        routes::ebloom::bulk_contains,
        routes::ebloom::clear,
        routes::ebloom::stats,
        routes::ebloom::list_filters,
    ),
    components(schemas(
        routes::bloom::CreateBloomRequest,
        routes::bloom::ItemRequest,
        routes::bloom::BulkItemRequest,
        routes::bloom::ContainsResponse,
        routes::bloom::BulkContainsResponse,
        routes::bloom::BloomStatsResponse,
        routes::bloom::FilterListResponse,
        routes::ebloom::CreateEbloomRequest,
        routes::ebloom::ItemRequest,
        routes::ebloom::BulkItemRequest,
        routes::ebloom::ContainsResponse,
        routes::ebloom::BulkContainsResponse,
        routes::ebloom::EbloomStatsResponse,
        routes::ebloom::FilterListResponse,
        error::ApiError,
    ))
)]
pub struct ApiDoc;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .nest("/api/v1/bloom", routes::bloom::router())
        .nest("/api/v1/ebloom", routes::ebloom::router())
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .with_state(state)
}

pub async fn run_server(state: AppState, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = create_router(state);
    axum::serve(listener, app).await?;
    Ok(())
}
