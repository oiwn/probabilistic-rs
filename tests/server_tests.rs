#[cfg(test)]
mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_utils::TestDb;
    use axum::{
        Router,
        body::{self, Body},
        http::{Request, StatusCode},
    };
    use expiring_bloom_rs::api::create_router;
    use expiring_bloom_rs::{
        AppState, FilterConfigBuilder, RedbFilter, RedbFilterConfigBuilder,
        ServerConfigBuilder,
    };
    use serde_json::json;
    use std::{sync::Arc, time::Duration};
    use tower::util::ServiceExt;

    async fn setup_test_app(test_name: &str) -> Router {
        let test_db = TestDb::new(&format!("server_test_{}", test_name));

        // NOTE: Yes, it's ugly, but why not?
        let port = match test_name {
            "health_check" => 50001,
            "insert_and_query" => 50002,
            "cleanup" => 50003,
            "expiration" => 50004,
            _ => 50000, // Default port for any other tests
        };

        let test_config = ServerConfigBuilder::default()
            .server_port(port)
            .bloom_db_path(test_db.path_string())
            .build()
            .unwrap();

        let filter_config = FilterConfigBuilder::default()
            .capacity(100)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(1))
            .max_levels(3)
            .build()
            .unwrap();

        let redb_config = RedbFilterConfigBuilder::default()
            .db_path(test_config.bloom_db_path.into())
            .filter_config(Some(filter_config))
            .snapshot_interval(Duration::from_secs(10))
            .build()
            .unwrap();

        let filter = RedbFilter::new(redb_config).unwrap();

        let state = Arc::new(AppState {
            filter: tokio::sync::Mutex::new(filter),
        });

        create_router(state)
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = setup_test_app("health").await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_insert_and_query() {
        let app = setup_test_app("insert_and_query").await;
        let test_value = "test_item";

        // Test insert
        let insert_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/items")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "value": test_value }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(insert_response.status(), StatusCode::OK);

        // Test query for inserted item
        let query_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/items/{}", test_value))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(query_response.status(), StatusCode::OK);

        let body_bytes = body::to_bytes(query_response.into_body(), 100)
            .await
            .unwrap();
        let response: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(response["exists"], true);

        // Test query for non-existent item
        let query_response = app
            .oneshot(
                Request::builder()
                    .uri("/items/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(query_response.status(), StatusCode::OK);

        let body_bytes = body::to_bytes(query_response.into_body(), 100)
            .await
            .unwrap();
        let response: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(response["exists"], false);
    }

    #[tokio::test]
    async fn test_cleanup() {
        let app = setup_test_app("cleanup").await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/cleanup")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_expiration() {
        let app = setup_test_app("expiration").await;
        let test_value = "expiring_item";

        // Insert item
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/items")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "value": test_value }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(4)).await;

        // Trigger cleanup
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/cleanup")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Query expired item
        let query_response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/items/{}", test_value))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body_bytes = body::to_bytes(query_response.into_body(), 100)
            .await
            .unwrap();
        let response: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(response["exists"], false);
    }
}
