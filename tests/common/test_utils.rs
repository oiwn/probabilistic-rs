use axum::Router;
use probablistic_rs::{
    AppState, FilterConfigBuilder, FjallFilter, FjallFilterConfigBuilder,
    server::api::create_router,
};
use std::{fs, path::PathBuf};
use std::{sync::Arc, time::Duration};

/// Structure to manage temporary test databases that are automatically cleaned up
pub struct TestDb {
    path: PathBuf,
}

impl TestDb {
    /// Create a new test database with a name based on the test name
    pub fn new(test_name: &str) -> Self {
        let path = format!("test_db_{test_name}.fjall").into();
        Self { path }
    }

    /// Get a clone of the database path
    // FIXME: btw this is detected as dead code while it's not
    #[allow(dead_code)]
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Get the database path as a string
    pub fn path_string(&self) -> String {
        self.path.to_string_lossy().to_string()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if self.path.exists() {
            if self.path.is_dir() {
                let _ = fs::remove_dir_all(&self.path);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

pub fn setup_test_fjall(
    db_path: &str,
    capacity: usize,
    level_duration: Duration,
    max_levels: usize,
    snapshot_interval: Duration,
) -> FjallFilter {
    let filter_config = FilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(0.01)
        .level_duration(level_duration)
        .max_levels(max_levels)
        .build()
        .unwrap();

    let fjall_config = FjallFilterConfigBuilder::default()
        .db_path(PathBuf::from(db_path))
        .filter_config(Some(filter_config))
        .snapshot_interval(snapshot_interval)
        .build()
        .unwrap();

    FjallFilter::new(fjall_config).expect("Unable to create filter...")
}

// FIXME: btw this is detected as dead code while it's not
#[allow(dead_code)]
pub async fn setup_test_app(test_name: &str, capacity: usize) -> Router {
    let test_db = TestDb::new(&format!("server_test_{test_name}"));

    let filter = setup_test_fjall(
        &test_db.path_string(),
        capacity,
        Duration::from_secs(1),
        3,
        Duration::from_secs(60),
    );

    let state = Arc::new(AppState {
        filter: tokio::sync::Mutex::new(filter),
    });

    create_router(state)
}
