use expiring_bloom_rs::api::create_router;
use expiring_bloom_rs::types::AppState;
use expiring_bloom_rs::{FilterConfig, RedbSlidingBloomFilter, ServerConfig};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    // load configuration from environment variables
    let server_config =
        ServerConfig::from_env().expect("Unable to load server config from env.");
    let filter_config = FilterConfig::try_from(server_config.clone())
        .expect("Unable to convert ServerConfig into FilterConfig");

    // Store the db path before config is moved
    let db_path = server_config.bloom_db_path.clone();

    let filter = RedbSlidingBloomFilter::new(
        filter_config.clone(),
        server_config.bloom_db_path.into(),
    )
    .expect("Failed to create filter");

    // Create application state
    let state = Arc::new(AppState {
        filter: tokio::sync::Mutex::new(filter),
    });

    // Create router
    let app = create_router(state.clone());

    // Build address string
    let addr = format!(
        "{}:{}",
        server_config.server_host, server_config.server_port
    );
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    // Calculate the memory usage estimation
    let bits_per_level = filter_config.capacity;
    let total_bits = bits_per_level * filter_config.max_levels;
    let estimated_memory_kb = (total_bits as f64 / 8.0 / 1024.0).ceil();

    info!(
        r#"
    🦀 Time-Decaying Bloom Filter Server Starting! 🚀

         ,~~.
        (  6 )-_,
         (\_/)\  )\  /     < Ready to filter with blazing speed! >
          \  |  ) |
          ^^ ^^   ^^
    
    📊 Filter Configuration:
       • Capacity: {:>16} items
       • False Positive Rate: {:>8.4}%
       • Levels: {:>18}
       • Level Duration: {:>12?}
       • Estimated Memory: {:>8.2} KB
       • Database Path: {:>14}

    🌐 Server Information:
       • Listening on:  http://{}
       • Swagger UI:    http://{}/swagger-ui/
       • Health Check:  http://{}/health
    
    🎯 API Endpoints:
       • POST   /items          - Insert item
       • GET    /items/:value   - Query item
       • POST   /cleanup        - Cleanup expired items
       
    🔧 Performance Mode: {}
    "#,
        filter_config.capacity,
        filter_config.false_positive_rate * 100.0,
        filter_config.max_levels,
        filter_config.level_duration,
        estimated_memory_kb,
        db_path,
        addr,
        addr,
        addr,
        if cfg!(debug_assertions) {
            "DEBUG"
        } else {
            "RELEASE"
        }
    );

    info!("Starting server on {}", addr);
    axum::serve(listener, app).await.unwrap();
}
