use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::bloom::BloomFilter;
use crate::ebloom::ExpiringBloomFilter;

pub type BloomRegistry = Arc<RwLock<HashMap<String, Arc<BloomFilter>>>>;
pub type EbloomRegistry = Arc<RwLock<HashMap<String, Arc<ExpiringBloomFilter>>>>;

#[derive(Clone)]
pub struct AppState {
    pub blooms: BloomRegistry,
    pub eblooms: EbloomRegistry,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            blooms: Arc::new(RwLock::new(HashMap::new())),
            eblooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
