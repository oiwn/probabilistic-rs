use super::CuckooResult;

pub trait CuckooFilterOps {
    fn insert(&self, item: &[u8]) -> CuckooResult<()>;
    fn contains(&self, item: &[u8]) -> CuckooResult<bool>;
    fn delete(&self, item: &[u8]) -> CuckooResult<()>;
    fn clear(&self) -> CuckooResult<()>;
}

pub trait CuckooFilterStats {
    fn capacity(&self) -> usize;
    fn fingerprint_bits(&self) -> usize;
    fn entries_per_bucket(&self) -> usize;
    fn insert_count(&self) -> usize;
    fn load_factor(&self) -> f64;
}

pub trait BulkCuckooFilterOps {
    fn insert_bulk(&self, items: &[&[u8]]) -> CuckooResult<()>;
    fn contains_bulk(&self, items: &[&[u8]]) -> CuckooResult<Vec<bool>>;
    fn delete_bulk(&self, items: &[&[u8]]) -> CuckooResult<()>;
}
