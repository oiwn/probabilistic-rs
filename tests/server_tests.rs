mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_utils::setup_test_app;
    use axum::{
        body::{self, Body},
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use std::time::Duration;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_health_check() {
        let app = setup_test_app("health", 1000).await;

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
        let app = setup_test_app("insert_and_query", 1000).await;
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
                    .uri(format!("/items/{test_value}"))
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
    // FIXME: what it testing? need to check clenup
    async fn test_cleanup() {
        let app = setup_test_app("cleanup", 100).await;

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
        let app = setup_test_app("expiration", 1000).await;
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
                    .uri(format!("/items/{test_value}"))
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
