#[cfg(feature = "server")]
mod server {
    use axum::{
        Router,
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use probabilistic_rs::server::{AppState, create_router};
    use serde::{Deserialize, de::DeserializeOwned};
    use tower::ServiceExt;

    fn test_router() -> Router<()> {
        create_router(AppState::new())
    }

    async fn send_request(
        router: Router<()>,
        method: Method,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, Option<String>) {
        let req = if let Some(json) = body {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("Content-Type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        } else {
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap()
        };

        let response = router.oneshot(req).await.unwrap();
        let status = response.status();

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = if body_bytes.is_empty() {
            None
        } else {
            Some(String::from_utf8(body_bytes.to_vec()).unwrap())
        };

        (status, body_str)
    }

    fn parse_json<T: DeserializeOwned>(body: &str) -> T {
        serde_json::from_str(body).unwrap()
    }

    #[derive(Debug, Deserialize)]
    struct MessageResponse {
        message: String,
    }

    #[derive(Debug, Deserialize)]
    struct ContainsResponse {
        present: bool,
    }

    #[derive(Debug, Deserialize)]
    struct BulkContainsResponse {
        results: Vec<bool>,
    }

    #[derive(Debug, Deserialize)]
    struct FilterListResponse {
        filters: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct BloomStatsResponse {
        capacity: usize,
        false_positive_rate: f64,
        insert_count: usize,
    }

    #[derive(Debug, Deserialize)]
    struct EbloomStatsResponse {
        capacity_per_level: usize,
        target_fpr: f64,
        total_insert_count: u64,
        active_levels: usize,
        num_levels: usize,
    }

    #[derive(Debug, Deserialize)]
    struct ApiError {
        error: String,
    }

    // ============================================
    // Bloom Filter Tests
    // ============================================

    #[tokio::test]
    async fn create_bloom_filter() {
        let router = test_router();
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let resp: MessageResponse = parse_json(body.unwrap().as_str());
        assert!(resp.message.contains("created"));
    }

    #[tokio::test]
    async fn create_bloom_filter_duplicate() {
        let router = test_router();

        // First create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "dup_test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        // Try to create again
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "dup_test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        let err: ApiError = parse_json(body.unwrap().as_str());
        assert!(err.error.contains("already exists"));
    }

    #[tokio::test]
    async fn delete_bloom_filter() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "to_delete", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        // Delete
        let (status, body) =
            send_request(router, Method::DELETE, "/api/v1/bloom/to_delete", None)
                .await;

        assert_eq!(status, StatusCode::OK);
        let resp: MessageResponse = parse_json(body.unwrap().as_str());
        assert!(resp.message.contains("deleted"));
    }

    #[tokio::test]
    async fn delete_bloom_filter_not_found() {
        let router = test_router();

        let (status, body) = send_request(
            router,
            Method::DELETE,
            "/api/v1/bloom/nonexistent",
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let err: ApiError = parse_json(body.unwrap().as_str());
        assert!(err.error.contains("not found"));
    }

    #[tokio::test]
    async fn insert_and_contains() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "insert_test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        // Insert
        let (status, _) = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/insert_test/insert",
            Some(r#"{"item": "hello"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Contains - should be true
        let (status, body) = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/insert_test/contains",
            Some(r#"{"item": "hello"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: ContainsResponse = parse_json(body.unwrap().as_str());
        assert!(resp.present);

        // Contains - should be false
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom/insert_test/contains",
            Some(r#"{"item": "world"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: ContainsResponse = parse_json(body.unwrap().as_str());
        assert!(!resp.present);
    }

    #[tokio::test]
    async fn bulk_insert_and_contains() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "bulk_test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;

        // Bulk insert
        let (status, body) = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/bulk_test/bulk/insert",
            Some(r#"{"items": ["a", "b", "c"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: MessageResponse = parse_json(body.unwrap().as_str());
        assert!(resp.message.contains("3 items"));

        // Bulk contains
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom/bulk_test/bulk/contains",
            Some(r#"{"items": ["a", "b", "x"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: BulkContainsResponse = parse_json(body.unwrap().as_str());
        assert_eq!(resp.results, vec![true, true, false]);
    }

    #[tokio::test]
    async fn clear_filter() {
        let router = test_router();

        // Create and insert
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "clear_test", "capacity": 10000, "fpr": 0.01}"#),
        )
        .await;
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/clear_test/insert",
            Some(r#"{"item": "hello"}"#),
        )
        .await;

        // Clear
        let (status, _) = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/clear_test/clear",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // Contains should be false now
        let (_, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom/clear_test/contains",
            Some(r#"{"item": "hello"}"#),
        )
        .await;
        let resp: ContainsResponse = parse_json(body.unwrap().as_str());
        assert!(!resp.present);
    }

    #[tokio::test]
    async fn list_filters() {
        let router = test_router();

        // Create multiple filters
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "filter1", "capacity": 1000, "fpr": 0.01}"#),
        )
        .await;
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "filter2", "capacity": 1000, "fpr": 0.01}"#),
        )
        .await;

        // List
        let (status, body) =
            send_request(router, Method::GET, "/api/v1/bloom/list", None).await;
        assert_eq!(status, StatusCode::OK);
        let resp: FilterListResponse = parse_json(body.unwrap().as_str());
        assert!(resp.filters.contains(&"filter1".to_string()));
        assert!(resp.filters.contains(&"filter2".to_string()));
    }

    #[tokio::test]
    async fn filter_stats() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "stats_test", "capacity": 5000, "fpr": 0.05}"#),
        )
        .await;

        // Insert 3 items
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/stats_test/insert",
            Some(r#"{"item": "a"}"#),
        )
        .await;
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/stats_test/insert",
            Some(r#"{"item": "b"}"#),
        )
        .await;
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/bloom/stats_test/insert",
            Some(r#"{"item": "c"}"#),
        )
        .await;

        // Stats
        let (status, body) = send_request(
            router,
            Method::GET,
            "/api/v1/bloom/stats_test/stats",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: BloomStatsResponse = parse_json(body.unwrap().as_str());
        assert_eq!(resp.capacity, 5000);
        assert!((resp.false_positive_rate - 0.05).abs() < 0.001);
        assert_eq!(resp.insert_count, 3);
    }

    #[tokio::test]
    async fn operation_on_nonexistent_filter() {
        let router = test_router();

        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom/nonexistent/insert",
            Some(r#"{"item": "test"}"#),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        let err: ApiError = parse_json(body.unwrap().as_str());
        assert!(err.error.contains("not found"));
    }

    // ============================================
    // Expiring Bloom Filter Tests
    // ============================================

    #[tokio::test]
    async fn create_ebloom_filter() {
        let router = test_router();
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "expiring_test", "capacity": 10000, "fpr": 0.01, "ttl_secs": 60, "num_levels": 3}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let resp: MessageResponse = parse_json(body.unwrap().as_str());
        assert!(resp.message.contains("created"));
    }

    #[tokio::test]
    async fn create_ebloom_filter_duplicate() {
        let router = test_router();

        // First create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "dup_ebloom", "capacity": 10000, "fpr": 0.01, "ttl_secs": 60, "num_levels": 3}"#,
            ),
        )
        .await;

        // Try again
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "dup_ebloom", "capacity": 10000, "fpr": 0.01, "ttl_secs": 60, "num_levels": 3}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);
        let err: ApiError = parse_json(body.unwrap().as_str());
        assert!(err.error.contains("already exists"));
    }

    #[tokio::test]
    async fn ebloom_insert_and_contains() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "ebloom_insert", "capacity": 10000, "fpr": 0.01, "ttl_secs": 60, "num_levels": 3}"#,
            ),
        )
        .await;

        // Insert
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom/ebloom_insert/insert",
            Some(r#"{"item": "hello"}"#),
        )
        .await;

        // Contains
        let (status, body) = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom/ebloom_insert/contains",
            Some(r#"{"item": "hello"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: ContainsResponse = parse_json(body.unwrap().as_str());
        assert!(resp.present);

        // Not present
        let (_, body) = send_request(
            router,
            Method::POST,
            "/api/v1/ebloom/ebloom_insert/contains",
            Some(r#"{"item": "world"}"#),
        )
        .await;
        let resp: ContainsResponse = parse_json(body.unwrap().as_str());
        assert!(!resp.present);
    }

    #[tokio::test]
    async fn ebloom_bulk_operations() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "ebloom_bulk", "capacity": 10000, "fpr": 0.01, "ttl_secs": 60, "num_levels": 3}"#,
            ),
        )
        .await;

        // Bulk insert
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom/ebloom_bulk/bulk/insert",
            Some(r#"{"items": ["x", "y", "z"]}"#),
        )
        .await;

        // Bulk contains
        let (status, body) = send_request(
            router,
            Method::POST,
            "/api/v1/ebloom/ebloom_bulk/bulk/contains",
            Some(r#"{"items": ["x", "y", "not_there"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: BulkContainsResponse = parse_json(body.unwrap().as_str());
        assert_eq!(resp.results, vec![true, true, false]);
    }

    #[tokio::test]
    async fn ebloom_stats() {
        let router = test_router();

        // Create
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom",
            Some(
                r#"{"name": "ebloom_stats", "capacity": 5000, "fpr": 0.02, "ttl_secs": 120, "num_levels": 5}"#,
            ),
        )
        .await;

        // Insert
        let _ = send_request(
            router.clone(),
            Method::POST,
            "/api/v1/ebloom/ebloom_stats/insert",
            Some(r#"{"item": "test"}"#),
        )
        .await;

        // Stats
        let (status, body) = send_request(
            router,
            Method::GET,
            "/api/v1/ebloom/ebloom_stats/stats",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let resp: EbloomStatsResponse = parse_json(body.unwrap().as_str());
        assert_eq!(resp.capacity_per_level, 5000);
        assert!((resp.target_fpr - 0.02).abs() < 0.001);
        assert_eq!(resp.num_levels, 5);
        assert_eq!(resp.active_levels, 5);
        assert!(resp.total_insert_count >= 1);
    }

    // ============================================
    // Error Handling Tests
    // ============================================

    #[tokio::test]
    async fn invalid_capacity_zero() {
        let router = test_router();

        let (status, _) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "bad_capacity", "capacity": 0, "fpr": 0.01}"#),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn invalid_fpr_out_of_range() {
        let router = test_router();

        let (status, _) = send_request(
            router,
            Method::POST,
            "/api/v1/bloom",
            Some(r#"{"name": "bad_fpr", "capacity": 1000, "fpr": 2.0}"#),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
