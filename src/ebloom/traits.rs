use crate::ebloom::error::Result;

use async_trait::async_trait;

/// Core operations for expiring bloom filter
#[async_trait]
pub trait ExpiringBloomFilterOps {
    /// Insert an item into the current level
    fn insert(&self, item: &[u8]) -> Result<()>;

    /// Check if an item exists in any active level
    fn contains(&self, item: &[u8]) -> Result<bool>;

    /// Clear all levels
    fn clear(&self) -> Result<()>;

    /// Clean up expired levels by rotating when needed
    async fn cleanup_expired_levels(&self) -> Result<()>;
}

/// Bulk operations for expiring bloom filter
pub trait BulkExpiringBloomFilterOps {
    fn insert_bulk(&self, items: &[&[u8]]) -> Result<()>;
    fn contains_bulk(&self, items: &[&[u8]]) -> Result<Vec<bool>>;
}

/// Statistics for expiring bloom filter
pub trait ExpiringBloomFilterStats {
    fn capacity_per_level(&self) -> usize;
    fn target_fpr(&self) -> f64;
    fn total_insert_count(&self) -> usize;
    fn active_levels(&self) -> usize;
    fn num_levels(&self) -> usize;
}
