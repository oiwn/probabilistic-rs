use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// Returns a reference to the shared Tokio runtime.
///
/// All background snapshot tasks and blocking async calls share this single runtime,
/// ensuring that multiple filter instances do not each spin up their own executor.
pub fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Runtime::new().expect("Failed to create shared Tokio runtime")
    })
}
