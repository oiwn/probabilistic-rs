use super::{
    BloomError, BloomFilter, BloomFilterConfig, BloomResult, PersistenceConfig,
    PersistentBloomFilter, StorageBackend,
};
use async_trait::async_trait;
use std::sync::Arc;

#[cfg(feature = "fjall")]
pub struct FjallBackend {
    keyspace: Arc<fjall::Keyspace>,
    config_partition: Arc<fjall::Partition>,
    chunks_partition: Arc<fjall::Partition>,
}

#[async_trait]
impl PersistentBloomFilter for BloomFilter {
    async fn save_snapshot(&self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        println!("TODO: need to save snapshot!");
        Ok(())
    }

    async fn load_from_storage(&mut self) -> BloomResult<()> {
        #[cfg(feature = "fjall")]
        println!("TODO: need to load filter data from storage!");
        Ok(())
    }

    fn is_dirty(&self) -> bool {
        self.dirty_chunks
            .as_ref()
            .map(|chunks| chunks.any())
            .unwrap_or(false)
    }
}
