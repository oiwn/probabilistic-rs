use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ApiError {
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: None,
        }
    }

    pub fn with_details(error: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            details: Some(details.into()),
        }
    }

    pub fn not_found(resource: &str, name: &str) -> Self {
        Self::new(format!("{} '{}' not found", resource, name))
    }

    pub fn already_exists(resource: &str, name: &str) -> Self {
        Self::new(format!("{} '{}' already exists", resource, name))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.error.as_str() {
            e if e.contains("not found") => StatusCode::NOT_FOUND,
            e if e.contains("already exists") => StatusCode::CONFLICT,
            e if e.contains("invalid") || e.contains("missing") => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

impl From<crate::BloomError> for ApiError {
    fn from(err: crate::BloomError) -> Self {
        Self::with_details("Bloom filter error", err.to_string())
    }
}

impl From<crate::EbloomError> for ApiError {
    fn from(err: crate::EbloomError) -> Self {
        Self::with_details("Expiring bloom filter error", err.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
