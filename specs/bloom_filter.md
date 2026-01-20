# Core Bloom Filter Specification

## Overview

Standard Bloom filter implementation with configurable false positive rate and optional persistence.

## API Design

### Core Traits
- `BloomFilterOps` - Basic operations (insert, contains, clear)
- `PersistentBloomFilterOps` - Persistence operations (save, load)
- All operations accept `&self` for interior mutability

### Key Operations
- `insert(&self, item: T)` - Insert single item
- `insert_bulk(&self, items: &[T])` - Batch insert
- `contains(&self, item: &T) -> bool` - Membership test
- `contains_bulk(&self, items: &[&T]) -> Vec<bool>` - Batch test
- `clear(&self)` - Reset filter

### Configuration
```rust
pub struct BloomFilterConfig {
    pub expected_items: usize,
    pub target_fp_rate: f64,
    pub max_fp_rate: f64,
    pub persistence: Option<PersistenceConfig>,
}
```

## Storage Backends

### In-Memory
- Simple bit vector storage
- Used for testing and ephemeral applications
- No persistence

### Fjall Persistence
- Chunked bit vector storage
- Dirty chunk tracking for incremental snapshots
- Configurable chunk sizes
- Atomic writes with crash recovery

## Hashing

- Double hashing: Murmur3 + FNV-1a
- Computed bit indices for uniform distribution
- Deterministic across restarts


Instead of K hash function can use composite hash function which will create K
different hashes using so called 2 hash trick:

```python
def get_hashes(key, k, m):
    h1 = hash1(key)
    h2 = hash2(key)
    for i in range(k):
        yield (h1 + i * h2) % m
```

## Concurrency

- Interior `RwLock` protects bit vector
- Atomic counters for statistics
- Arc for cross-thread sharing

## Performance

- Optimized bit vector operations
- Bulk operations reduce lock contention
- Chunked persistence for large filters
- Benchmarked under various load patterns

## Usage Examples

```rust
// Simple in-memory filter
let config = BloomFilterConfig::new(1000, 0.01);
let filter = BloomFilter::new(config);

filter.insert("item");
let exists = filter.contains("item");

// Persistent filter
let persistence = PersistenceConfig::fjall("path/to/db")
    .with_chunk_size(4096);
let config = BloomFilterConfig::with_persistence(1000, 0.01, persistence);
let filter = PersistentBloomFilter::new(config);
```

## Testing

- Unit tests for all operations
- Concurrency regression tests
- Persistence lifecycle tests
- Bulk operation validation
- Error handling coverage

# Useful links

[https://fjall-rs.github.io/post/bloom-filter-hash-sharing/] - general description of bloom filters
[https://docs.rs/seahash/latest/seahash/] - claims superiority over "xxHash"
