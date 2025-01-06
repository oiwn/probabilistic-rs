use expiring_bloom_rs::api::create_router;
use expiring_bloom_rs::types::AppState;
use expiring_bloom_rs::{
    FilterConfigBuilder, RedbSlidingBloomFilter, ServerConfig,
};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .pretty()
        .init();

    // load configuration from environment variables
    let config = ServerConfig::from_env().expect("Failed to load configuration");

    // Initialize the Bloom filter with configuration
    let filter_config = FilterConfigBuilder::default()
        .capacity(config.bloom_capacity)
        .false_positive_rate(config.bloom_false_positive_rate)
        .level_duration(config.bloom_level_duration)
        .max_levels(config.bloom_max_levels)
        .build()
        .expect("Failed to build filter config");

    let filter = RedbSlidingBloomFilter::new(
        filter_config.clone(),
        config.bloom_db_path.clone().into(),
    )
    .expect("Failed to create filter");

    // Create application state
    let state = Arc::new(AppState {
        filter: tokio::sync::Mutex::new(filter),
    });

    // Create router with logging middleware
    let app = create_router(state.clone()).layer(
        tower_http::trace::TraceLayer::new_for_http()
            .make_span_with(|request: &axum::http::Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    version = ?request.version(),
                )
            })
            .on_response(
                |response: &axum::http::Response<_>,
                 latency: std::time::Duration,
                 _span: &tracing::Span| {
                    tracing::info!(
                        status = %response.status(),
                        latency = ?latency,
                        "response generated"
                    );
                },
            ),
    );

    // Build address string
    let addr = format!("{}:{}", config.server_host, config.server_port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    // Calculate the memory usage estimation
    let bits_per_level = filter_config.capacity;
    let total_bits = bits_per_level * filter_config.max_levels;
    let estimated_memory_kb = (total_bits as f64 / 8.0 / 1024.0).ceil();

    let level_duration = filter_config.level_duration;
    let max_levels = filter_config.max_levels;
    let false_positive_rate = filter_config.false_positive_rate.clone();

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
        bits_per_level,
        false_positive_rate * 100.0,
        max_levels,
        level_duration,
        estimated_memory_kb,
        &config.bloom_db_path,
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
