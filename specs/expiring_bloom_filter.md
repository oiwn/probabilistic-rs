# Expiring Bloom Filter Specification

## Overview

Time-decaying multi-level Bloom filter that automatically expires old entries.
Built on top of the core Bloom filter infrastructure.

## Architecture

### Multi-Level Design

- **Levels**: Multiple Bloom filters, each representing a time window
- **Rotation**: Old levels expire, new levels are created
- **Current Level**: Active level for insertions
- **Cleanup**: Automatic removal of expired levels
- **Access Pattern**: Only current level accepts writes and reads; other levels are read-only for historical lookups.

### Data Structure

```rust
pub struct ExpiringBloomFilter {
    config: ExpiringFilterConfig,
    bit_vector_size: usize,
    num_hashes: usize,
    insert_count: AtomicUsize,
    levels: Arc<RwLock<Vec<Level>>>,
    current_level: AtomicUsize,
    cleanup_task: Option<JoinHandle<()>>,
}
```

Note: The Level struct may be simplified to avoid nested BloomFilter complexity.
Consider storing BloomFilter data directly as fields for better performance and
simpler synchronization. This requires careful design of read/write patterns.

## API Design

### Core Operations
- `insert(&self, item: &[u8]) -> BloomResult<()>` - Insert into current level
- `contains(&self, item: &[u8]) -> BloomResult<bool>` - Check all active levels  
- `insert_bulk(&self, items: &[&[u8]]) -> BloomResult<()>` - Batch insert
- `contains_bulk(&self, items: &[&[u8]]) -> BloomResult<Vec<bool>>` - Batch check

These operations implement the standard `BloomFilterOps` and `BulkBloomFilterOps` traits.

### Configuration

```rust
#[derive(Clone, Debug, Builder)]
pub struct ExpiringFilterConfig {
    // Base config - mirrors BloomFilterConfig
    #[builder(default = "1_000_000")]
    pub capacity_per_level: usize,

    #[builder(default = "0.01")]  
    pub target_fpr: f64,

    // Expiring specific additions
    pub level_duration: Duration,
    pub num_levels: usize,

    // Persistence
    #[builder(default = "None")]
    pub persistence: Option<PersistenceConfig>,
}
```

Note: `level_duration` is a proper `Duration` type instead of seconds.

### State Machine

**Level Lifecycle:**
- Level 0 (current): Accepts writes and reads, grows until full or time expires
- Level N (historical): Read-only, immutable after creation (by this mean when new level generated the previously level 0 become level N and immutable) until expiration
- Expiration: Oldest level drops, all levels shift down, new level becomes current.
^^^ What does it mean "shift down" we store them in memory and in fjall.
- Capacity: Each level sized for `capacity_per_level` items at `target_fpr`


**States:**
- Normal: Current level grows, older levels persist
- Rotating: New level added, oldest level removed if > num_levels
- Cleanup: Periodic removal of expired levels based on `level_duration`

**Example with 3 levels, 10s duration:**
- T=00s: Level 0 starts
- T=05s: Level 0 active, capacity grows
- T=10s: Level 0 becomes Level 1, New Level starts as current (Level 0)
- T=15s: Level 0 active, Level 1 historical
- T=20s: Level 1 becomes Level 2, Level 0 becomes Level 1, New Level 0 starts
- T=30s: Level 3 expire and drop. Level 2 becomes level 3, Level 1 becomes

^^^ i think it work like this, i made edits

### Level Rotation
- `rotate_levels(&self)` - Shift levels, drop oldest, create new
- `cleanup_expired_levels(&self)` - Check timestamps and rotate
- Current level tracking with `AtomicUsize`

### Automatic Cleanup
- Periodic background task using tokio
- Configurable cleanup intervals
- Graceful shutdown support

## Persistence (Fjall)

### Multi-Level Storage
- Separate keys for each level
- Level metadata (creation time, level number)
- Current level index
- Configuration persistence

### Storage Layout
- `config` - Filter configuration
- `level_{n}_metadata` - Level metadata
- `level_{n}_chunks` - Bit vector chunks
- `current_level` - Active level index
- `dirty_chunks_{n}` - Modified chunk tracking

### Operations
- `save_snapshot(&self)` - Persist all levels
- `load()` constructor - Restore from storage
- `create_or_load()` - Open existing or create new

## Implementation Status

### ✅ Completed
- Basic multi-level structure
- Core expiring filter API
- In-memory level management
- Configuration and error handling

## Concurrency

- Interior `RwLock` protects levels vector
- `AtomicUsize` for current level tracking
- Background cleanup task coordination
- Arc for cross-thread sharing

## Error Handling

- `EbloomError` for expiring filter errors
- Conversion from `FilterError` where needed
- Storage operation error propagation
- Graceful degradation for cleanup failures

## Usage Examples

```rust
let config = ExpiringFilterConfig::new(
    1000,  // max_items_per_level
    0.01,  // target_fpr
    3600,  // level_duration_seconds (1 hour)
    4,     // num_levels (4 hours total)
);

let filter = ExpiringBloomFilter::new(config);

filter.insert("item");
let exists = filter.contains("item"); // Checks all active levels
```

## Testing Strategy

- Unit tests for level management
- Integration tests for time-based expiration
- Concurrency tests with background cleanup
- Persistence lifecycle tests
- Performance benchmarks for rotation
