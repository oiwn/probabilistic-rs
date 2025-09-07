use super::{BloomFilterConfig, BloomResult};
use async_trait::async_trait;

pub trait BloomFilterOps {
    fn insert(&mut self, item: &[u8]) -> BloomResult<()>;
    fn contains(&self, item: &[u8]) -> BloomResult<bool>;
    fn clear(&mut self) -> BloomResult<()>;
}

pub trait BloomFilterStats {
    fn capacity(&self) -> usize;
    fn false_positive_rate(&self) -> f64;
    fn insert_count(&self) -> usize;
}

pub trait BulkBloomFilterOps {
    fn insert_bulk(&mut self, items: &[&[u8]]) -> BloomResult<()>;
    fn contains_bulk(&self, items: &[&[u8]]) -> BloomResult<Vec<bool>>;
}

#[async_trait]
pub trait PersistentBloomFilter {
    async fn save_snapshot(&self) -> BloomResult<()>;
    async fn load_from_storage(&mut self) -> BloomResult<()>;
    fn is_dirty(&self) -> bool;
}

#[async_trait]
pub trait StorageBackend {
    async fn save_config(&self, config: &BloomFilterConfig) -> BloomResult<()>;
    async fn load_config(&self) -> BloomResult<BloomFilterConfig>;
    async fn save_snapshot(&self, chunks: &[(usize, Vec<u8>)])
    -> BloomResult<()>;
    async fn load_snapshot(&self) -> BloomResult<Vec<(usize, Vec<u8>)>>;
}
