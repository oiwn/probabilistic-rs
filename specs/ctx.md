# Current Task Context

## Active Work: Phase 4 - Fjall Persistence Integration for Expiring Bloom Filter

**Started**: 2025-01-16
**Module**: `src/ebloom/`
**Priority**: High - Enable multi-level persistence with chunked storage

## Current Status

### ✅ Completed (Phase 1-3)

**Core Implementation:**
- ✅ Complete separation from core bloom filter (independent module)
- ✅ Multi-level in-memory filter implementation
- ✅ Independent trait system (`ExpiringBloomFilterOps`, `ExpiringBloomFilterStats`)
- ✅ Clean error handling with `EbloomError` (no mixing with `BloomError`)
- ✅ Level rotation and time-based expiration logic
- ✅ Configuration with proper field names (`capacity_per_level`, `target_fpr`, `num_levels`, `level_duration`)

**Testing:**
- ✅ Comprehensive test suite (30+ tests in `tests/ebloom_tests.rs`)
- ✅ Core bloom filter tests passing (86 total tests)
- ✅ Old Fjall architecture tests in `tmp/fjall_tests.rs` (ready to adapt)

**Architecture:**
- ✅ `src/ebloom/config.rs` - Configuration with builder pattern
- ✅ `src/ebloom/error.rs` - Independent error types
- ✅ `src/ebloom/filter.rs` - Main filter implementation (in-memory)
- ✅ `src/ebloom/traits.rs` - Independent trait definitions
- ✅ `src/ebloom/storage.rs` - Storage backends (ready for Fjall integration)

### 🎯 Phase 4 Goal: FJALL PERSISTENCE WITH CHUNKED STORAGE

**Key Principle**: Write-Once Per Rotation

```
Timeline:
Time ──────────────────────────────────────────────────>

Level 0: [WRITE] ─────> [FROZEN] ─────> [FROZEN] ─────> [CLEAR & WRITE] ──>
Level 1: [FROZEN] ─────> [CLEAR & WRITE] ─> [FROZEN] ─────> [FROZEN] ─────>
Level 2: [FROZEN] ─────> [FROZEN] ─────> [CLEAR & WRITE] ─> [FROZEN] ─────>
         rotation 0     rotation 1      rotation 2      rotation 3
```

**At any moment:**
- **1 level is ACTIVE** (current_level) - receives ALL writes
- **N-1 levels are FROZEN** - read-only, already persisted to DB

**On rotation:**
1. Save current level's final snapshot → DB (freeze it forever)
2. Move to next level: `current_level = (current_level + 1) % num_levels`
3. Clear new current level in memory
4. Delete new current level's old chunks from DB (oldest data expires)
5. Clear dirty chunks tracker (it's for the new current level now)
6. Update metadata (timestamps, current_level pointer) in DB

### Core Bloom Pattern (Reference)
```
┌─────────────────────────────────────────┐
│ BloomFilter                             │
│ ├─ bits: Arc<RwLock<BitVec>>           │  ← Single bit vector
│ ├─ dirty_chunks: Arc<RwLock<BitVec>>   │  ← Dirty tracking
│ ├─ storage: FjallBackend               │
│ └─ chunk_size_bytes: usize             │
└─────────────────────────────────────────┘
```

### Expiring Bloom Target (Multi-Level)
```
┌─────────────────────────────────────────┐
│ ExpiringBloomFilter                     │
│ ├─ levels: Arc<RwLock<Vec<BitVec>>>    │  ← N bit vectors in memory
│ ├─ metadata: Arc<RwLock<Vec<LevelMetadata>>> │  ← Already exists!
│ ├─ current_level: AtomicUsize           │  ← Active level index
│ │                                        │
│ │ NEW FIELDS:                            │
│ ├─ storage: Option<FjallExpiringBackend> │  ← Backend
│ ├─ chunk_size_bytes: usize              │  ← Chunk size
│ └─ dirty_chunks: Option<Arc<RwLock<BitVec>>> │  ← ONE tracker for CURRENT level!
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────┐
│ FjallExpiringBackend (already implemented!)         │
│ Partitions for ALL N levels:                        │
│   • expiring_config → ExpiringFilterConfig          │
│   • current_level   → usize (active level index)    │
│   • level_metadata  → Vec<LevelMetadata>            │
│   • level_0_chunks  → Full snapshots                │
│   • level_0_dirty   → Incremental dirty chunks      │
│   • level_1_chunks  → Full snapshots (frozen)       │
│   • level_1_dirty   → Incremental dirty chunks      │
│   • level_2_chunks  → Full snapshots (frozen)       │
│   • level_2_dirty   → Incremental dirty chunks      │
│                                                      │
│ NOTE: Only CURRENT level's partitions are written!  │
│       Other levels are FROZEN (read-only).          │
│                                                      │
│ Two persistence modes:                              │
│ 1. Incremental: save_dirty_chunks(current_level)    │
│    - For crash recovery during active writes        │
│ 2. Full snapshot: save_level_chunks(current_level)  │
│    - On rotation to freeze the level                │
└─────────────────────────────────────────────────────┘
```

## Implementation Tasks

### Task 1: Add Persistence Config
**File**: `src/ebloom/config.rs`

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct ExpiringPersistenceConfig {
    pub db_path: PathBuf,
    #[builder(default = "4096")]  // 4KB chunks
    pub chunk_size_bytes: usize,
}

// Add to ExpiringFilterConfig:
#[builder(default = "None")]
pub persistence: Option<ExpiringPersistenceConfig>,
```

### Task 2: Add Persistence Fields to Filter
**File**: `src/ebloom/filter.rs`

```rust
pub struct ExpiringBloomFilter {
    // ... existing fields ...

    // Persistence support
    #[cfg(feature = "fjall")]
    storage: Option<FjallExpiringBackend>,

    chunk_size_bytes: usize,

    // Dirty chunk tracking for CURRENT level only!
    dirty_chunks: Option<Arc<RwLock<BitVec<usize, Lsb0>>>>,
}
```

### Task 3: Implement create/load/build Methods
**File**: `src/ebloom/filter.rs`

```rust
impl ExpiringBloomFilter {
    /// Create new filter (overwrites existing DB)
    pub async fn create(config: ExpiringFilterConfig) -> Result<Self>

    /// Load existing filter from DB
    #[cfg(feature = "fjall")]
    pub async fn load(db_path: PathBuf) -> Result<Self>

    /// Create or load (convenience)
    pub async fn create_or_load(config: ExpiringFilterConfig) -> Result<Self>

    /// Internal builder
    async fn build_filter(
        config: ExpiringFilterConfig,
        storage: Option<FjallExpiringBackend>,
    ) -> Result<Self>
}
```

### Task 4: Implement Snapshot Methods
**File**: `src/ebloom/filter.rs`

```rust
impl ExpiringBloomFilter {
    /// Save incremental dirty chunks for CURRENT level (crash recovery)
    pub async fn save_snapshot(&self) -> Result<()> {
        // Extract only dirty chunks for current level
        // Save to storage.save_dirty_chunks(current_level, ...)
    }

    /// Save full snapshot of CURRENT level (called on rotation)
    async fn save_full_snapshot(&self) -> Result<()> {
        // Extract all chunks for current level
        // Save to storage.save_level_chunks(current_level, ...)
    }

    /// Extract dirty chunks for current level only
    fn extract_dirty_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>>

    /// Extract all chunks for current level only
    fn extract_all_chunks(&self) -> Result<Vec<(usize, Vec<u8>)>>

    /// Reconstruct all N levels from storage (on load)
    async fn reconstruct_from_storage(&mut self) -> Result<()> {
        // For each level, try dirty chunks first, fall back to full chunks
    }
}

// Helper functions:
fn extract_chunk_bytes(bits: &BitVec, chunk_id: usize, chunk_size_bits: usize) -> Vec<u8>
fn reconstruct_level_from_chunks(level_bits: &mut BitVec, chunks: &[(usize, Vec<u8>)], chunk_size_bytes: usize) -> Result<()>
```

### Task 5: Update Insert to Track Dirty Chunks
**File**: `src/ebloom/filter.rs`

```rust
fn insert(&self, item: &[u8]) -> Result<()> {
    // ... calculate indices ...

    // Mark dirty chunks (current level only)
    if let Some(ref dirty_arc) = self.dirty_chunks {
        let mut dirty = dirty_arc.write()?;
        for &idx in &indices {
            let chunk_id = (idx as usize) / (self.chunk_size_bytes * 8);
            if chunk_id < dirty.len() {
                dirty.set(chunk_id, true);
            }
        }
    }

    // ... rest of insert ...
}
```

### Task 6: Update Rotation to Persist
**File**: `src/ebloom/filter.rs`

```rust
pub async fn rotate_levels(&self) -> Result<()> {
    let current_idx = self.current_level.load(Ordering::Relaxed);
    let new_current_idx = (current_idx + 1) % self.config.num_levels;

    // 1. Save FULL snapshot of current level (freeze it forever)
    self.save_full_snapshot().await?;

    // 2. Get write locks
    let mut levels = self.levels.write()?;
    let mut metadata = self.metadata.write()?;

    // 3. Clear new current level in memory (oldest data expires)
    levels[new_current_idx].fill(false);

    // 4. Delete new current level's old data from DB (both chunks AND dirty)
    #[cfg(feature = "fjall")]
    if let Some(ref backend) = self.storage {
        backend.delete_level(new_current_idx).await?;
    }

    // 5. Update metadata for new current level
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    metadata[new_current_idx] = LevelMetadata {
        created_at: now_ms,
        insert_count: 0,
        last_snapshot_at: 0,
    };

    // 6. Save metadata and current level pointer to DB
    #[cfg(feature = "fjall")]
    if let Some(ref backend) = self.storage {
        backend.save_level_metadata(&metadata).await?;
        backend.save_current_level(new_current_idx).await?;
    }

    drop(levels);
    drop(metadata);

    // 7. Update current level pointer in memory
    self.current_level.store(new_current_idx, Ordering::Relaxed);

    // 8. Clear dirty chunks tracker (for new current level)
    if let Some(ref dirty_arc) = self.dirty_chunks {
        dirty_arc.write()?.fill(false);
    }

    Ok(())
}
```

### Task 7: Create Comprehensive Tests
**File**: `tests/ebloom_fjall_tests.rs` (new)

```rust
#[cfg(feature = "fjall")]
mod tests {
    #[tokio::test]
    async fn test_create_and_save_incremental() {
        // Create filter, insert items
        // Save dirty chunks (incremental)
        // Verify dirty partitions have data
    }

    #[tokio::test]
    async fn test_load_from_dirty_chunks() {
        // Create filter, insert, save dirty chunks
        // Drop filter
        // Load from DB - should reconstruct from dirty chunks
    }

    #[tokio::test]
    async fn test_rotation_saves_full_snapshot() {
        // Insert to level 0
        // Rotate - should save full snapshot to chunks partition
        // Verify level 0 frozen in chunks partition
    }

    #[tokio::test]
    async fn test_rotation_deletes_old_level() {
        // Fill all 3 levels
        // Rotate to level 0 again
        // Verify level 0's old chunks AND dirty partitions deleted
    }

    #[tokio::test]
    async fn test_create_or_load() {
        // First call creates, second call loads
    }

    #[tokio::test]
    async fn test_full_rotation_cycle_with_persistence() {
        // Complete rotation cycle
        // Verify each level frozen correctly
        // Verify dirty chunks only for current level
    }

    #[tokio::test]
    async fn test_crash_recovery_from_dirty_chunks() {
        // Insert to current level, save dirty
        // Simulate crash (don't save full snapshot)
        // Reload - should recover from dirty chunks
    }
}
```

## Implementation Checklist

- [ ] Add `ExpiringPersistenceConfig` to config.rs
- [ ] Add persistence field to `ExpiringFilterConfig`
- [ ] Add storage, chunk_size_bytes, dirty_chunks to `ExpiringBloomFilter` struct
- [ ] Implement `create()` - initialize DB, save config
- [ ] Implement `load()` - open DB, load config, reconstruct all N levels
- [ ] Implement `create_or_load()` - convenience wrapper
- [ ] Implement `build_filter()` - internal builder
- [ ] Implement `save_snapshot()` - save dirty chunks (incremental, current level)
- [ ] Implement `save_full_snapshot()` - save all chunks (on rotation, current level)
- [ ] Implement `extract_dirty_chunks()` - extract only dirty chunks from current level
- [ ] Implement `extract_all_chunks()` - extract all chunks from current level
- [ ] Implement `reconstruct_from_storage()` - load all N levels (dirty first, fallback to chunks)
- [ ] Implement helpers: `extract_chunk_bytes()`, `reconstruct_level_from_chunks()`
- [ ] Update `insert()` to mark dirty chunks (current level only)
- [ ] Update `rotate_levels()` to call save_full_snapshot, delete old level, persist metadata
- [ ] Create test file with 7 comprehensive tests
- [ ] Verify all tests pass: `cargo test --features fjall`

## Success Criteria for Phase 4

- [ ] ExpiringBloomFilter supports optional Fjall persistence
- [ ] Two-tier persistence working:
  - [ ] Incremental saves (dirty chunks) for crash recovery
  - [ ] Full snapshots (all chunks) on rotation to freeze levels
- [ ] Only CURRENT level's partitions are written to
- [ ] All N levels can be reconstructed from storage (dirty or chunks)
- [ ] Level rotation saves full snapshot, deletes old level data (chunks + dirty)
- [ ] Dirty chunk tracking (single tracker for current level) works correctly
- [ ] All tests pass with `cargo test --features fjall`
- [ ] At least 7 comprehensive Fjall integration tests
- [ ] Crash recovery from dirty chunks verified
- [ ] Rotation cycle verified: write → incremental save → freeze → delete

## Why Phase 4 Matters

- **Durability**: Expiring filters can survive process restarts
- **Large Scale**: Support filters larger than available memory (future optimization)
- **Production Ready**: Critical for real-world deployments
- **Foundation**: Enables Phase 5 (background cleanup) to be crash-resistant

### 📋 Future Phases (After Phase 4)

**Phase 5**: Background Cleanup with Persistence
- Tokio-based periodic cleanup task that persists state
- Graceful shutdown with snapshot save
- Configurable cleanup intervals
- Integration with persistence layer

**Phase 6**: Performance & Optimization
- Benchmarks comparing in-memory vs persisted
- Optimize dirty chunk tracking strategy
- Memory-mapped chunks for large datasets (optional)
- Compression for chunk storage (optional)

**Phase 7**: Server/CLI Integration (Optional)
- Update HTTP API (in `tmp/bin/server.rs`) to use new ebloom
- Update TUI application (in `tmp/bin/cli.rs`) to use new ebloom
- Add persistence configuration via API/CLI

## Key Files Reference

### Implementation Files
- `src/ebloom/filter.rs` - Main implementation (to be modified)
- `src/ebloom/config.rs` - Config structures (add PersistenceConfig)
- `src/ebloom/storage.rs` - Backend trait & Fjall implementation (already done!)
- `src/ebloom/error.rs` - Error types (already sufficient)

### Test Files
- `tests/ebloom_tests.rs` - Existing 30+ in-memory tests
- `tests/ebloom_fjall_tests.rs` - NEW: Fjall persistence tests
- `tmp/fjall_tests.rs` - Reference for test patterns (old architecture)

### Reference Implementation
- `src/bloom/filter.rs` - Core bloom with persistence (reference implementation)
- `src/bloom/storage.rs` - Single-level Fjall backend (pattern to follow)

## Notes for Implementation

1. **Follow Core Bloom Pattern**: The core bloom filter has excellent persistence - use it as a template
2. **Multi-Level Complexity**: Key difference is managing N bit vectors instead of 1
3. **Dirty Tracking**: Each level needs its own dirty chunk BitVec
4. **Rotation is Critical**: Must properly delete old level's data from DB during rotation
5. **Async Throughout**: All storage operations are async (use tokio runtime in tests)
6. **Feature Flag**: All Fjall code should be behind `#[cfg(feature = "fjall")]`
7. **Error Handling**: Use `EbloomError` consistently (no panic on storage errors)
8. **Metadata Sync**: Keep LevelMetadata synchronized between memory and storage