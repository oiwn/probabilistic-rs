use super::BloomResult;
use async_trait::async_trait;

#[async_trait]
pub trait BloomFilterOps {
    async fn insert(&mut self, item: &[u8]) -> BloomResult<()>;
    async fn contains(&self, item: &[u8]) -> BloomResult<bool>;
    async fn clear(&mut self) -> BloomResult<()>;
}

pub trait BloomFilterStats {
    fn capacity(&self) -> usize;
    fn false_positive_rate(&self) -> f64;
    fn insert_count(&self) -> usize;
}

pub trait BulkBloomFilterOps {
    async fn insert_bulk(&mut self, items: &[&[u8]]) -> BloomResult<()>;
    async fn contains_bulk(&self, items: &[&[u8]]) -> BloomResult<Vec<bool>>;
}

pub trait PersistentBloomFilter {
    async fn save_snapshot(&self) -> BloomResult<()>;
    async fn load_from_storage(&mut self) -> BloomResult<()>;
    fn is_dirty(&self) -> bool;
}
