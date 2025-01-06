use expiring_bloom_rs::api::create_router;
use expiring_bloom_rs::types::AppState;
use expiring_bloom_rs::{FilterConfigBuilder, RedbSlidingBloomFilter};
use std::{sync::Arc, time::Duration};

#[tokio::main]
async fn main() {
    // Initialize the Bloom filter
    let config = FilterConfigBuilder::default()
        .capacity(1000)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(60))
        .max_levels(3)
        .build()
        .expect("Failed to build filter config");

    let filter = RedbSlidingBloomFilter::new(config, "bloom.redb".into())
        .expect("Failed to create filter");

    // Create application state
    let state = Arc::new(AppState {
        filter: tokio::sync::Mutex::new(filter),
    });

    // Create router
    let app = create_router(state);

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server running on http://localhost:3000");
    println!("API documentation available at http://localhost:3000/swagger-ui/");

    axum::serve(listener, app).await.unwrap();
}
