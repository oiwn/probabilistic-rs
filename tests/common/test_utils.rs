use std::{fs, path::PathBuf};

/// Structure to manage temporary test databases that are automatically cleaned up
pub struct TestDb {
    path: PathBuf,
}

impl TestDb {
    /// Create a new test database with a name based on the test name
    pub fn new(test_name: &str) -> Self {
        let path = format!("test_db_{}.redb", test_name).into();
        Self { path }
    }

    /// Get a clone of the database path
    /// TODO: btw this is detected as dead code while it's not
    #[allow(dead_code)]
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Get the database path as a string
    #[allow(dead_code)]
    pub fn path_string(&self) -> String {
        self.path.to_string_lossy().to_string()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}
