# Current Task Context

## Goal: Cuckoo Filter Implementation ✅

Implement a Cuckoo filter (`src/cuckoo/`) following the existing module patterns
from `src/bloom/` and `src/ebloom/`.

### Status

Cuckoo filter v1 implemented and passing:
- 12 unit tests cover insert, contains, delete, clear, stats, bulk ops, concurrency, FPR
- 160 total tests pass (was 148 before cuckoo)
- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` pass

### File Structure

```
src/cuckoo/
├── config.rs    — CuckooFilterConfig + derive_builder + Default + validate + postcard
├── error.rs     — CuckooError (thiserror), CuckooResult<T>
├── filter.rs    — CuckooFilter struct, insert/contains/delete/clear, 12 unit tests
└── traits.rs    — CuckooFilterOps, CuckooFilterStats, BulkCuckooFilterOps
src/cuckoo.rs    — module root + re-exports
```

### Design Recap

- Buckets: `[u16; 4]` with 0 sentinel for empty slots
- Fingerprints: 8-bit default, 4–16 bit range, non-zero guarantee
- Partial-key cuckoo: alternate bucket = current XOR hash(fingerprint)
- num_buckets is power-of-two for XOR symmetry
- Eviction: random slot pick per kick, up to max_kicks (500 default)
- All ops accept `&self` via `Arc<RwLock<Vec<Bucket>>>`

### Out of Scope (deferred)

- Fjall persistence (v2)
- Python bindings (v3)
- HTTP server routes (v4)
- Dynamic resizing on CapacityExceeded
