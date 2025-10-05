use probablistic_rs::server::api::create_router;
use probablistic_rs::server::types::AppState;
use probablistic_rs::{
    FilterConfig, FjallFilter, FjallFilterConfigBuilder, ServerConfig,
};
use std::{path::PathBuf, sync::Arc};
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

    // Load configuration from environment variables
    let server_config =
        ServerConfig::from_env().expect("Unable to load server config from env.");

    // Convert to FilterConfig for potential use
    let env_filter_config = FilterConfig::try_from(server_config.clone())
        .expect("Unable to convert ServerConfig into FilterConfig");

    // Store the db path before config is moved
    let db_path: PathBuf = server_config.bloom_db_path.clone().into();

    // Determine if database already exists
    let db_exists = db_path.exists();

    // Initialize the filter based on database existence
    let filter = if db_exists {
        // Database exists, load configuration from it
        info!(
            "Opening existing Bloom filter database: {}",
            db_path.display()
        );

        let fjall_config = FjallFilterConfigBuilder::default()
            .db_path(db_path.clone())
            .snapshot_interval(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to create FjallFilterConfig");
        FjallFilter::new(fjall_config)
    } else {
        // No database, create new one with config from environment
        info!("Creating new Bloom filter database: {}", db_path.display());
        let fjall_config = FjallFilterConfigBuilder::default()
            .db_path(db_path.clone())
            .filter_config(Some(env_filter_config.clone()))
            .snapshot_interval(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to create FjallFilterConfig");

        FjallFilter::new(fjall_config)
    }
    .expect("Failed to initialize Bloom filter");

    // Get the actual configuration (from DB or env)
    let active_config = filter.config().clone();

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

    // Calculate the memory usage estimation based on the active configuration
    let bits_per_level = active_config.capacity;
    let total_bits = bits_per_level * active_config.max_levels;
    let estimated_memory_kb = (total_bits as f64 * 8.0 / 1024.0).ceil();

    info!(
        r#"
    🦀 Time-Decaying Bloom Filter Server Starting! 🚀

         ,~~.
        (  6 )-_,
         (\_/)\  )\  / < Ready to filter with blazing speed! >
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
        active_config.capacity,
        active_config.false_positive_rate * 100.0,
        active_config.max_levels,
        active_config.level_duration,
        estimated_memory_kb,
        db_path.display(),
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
