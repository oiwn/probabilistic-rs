# Current Task Context

Working to bring this crate to the release.

## Checklist

- [x] python bindings
- [x] rest api
- [ ] python package
- [ ] integration tests

---

# REST API Plan

## Goal
Axum-based REST API for Bloom and Expiring Bloom filters with Swagger UI documentation.

## Architecture

```
src/server/
├── mod.rs           # Server setup, router, main entry
├── routes/
│   ├── mod.rs
│   ├── bloom.rs     # /bloom endpoints
│   └── ebloom.rs    # /ebloom endpoints
├── handlers/
│   ├── mod.rs
│   ├── bloom.rs     # Bloom filter handlers
│   └── ebloom.rs    # Expiring bloom handlers
├── state.rs         # AppState with filter managers
└── error.rs         # API error types
```

## Endpoints

### Bloom Filter (`/api/v1/bloom`)
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/` | Create new filter |
| DELETE | `/{name}` | Delete filter |
| POST | `/{name}/insert` | Insert item |
| POST | `/{name}/contains` | Check item |
| POST | `/{name}/bulk/insert` | Bulk insert |
| POST | `/{name}/bulk/contains` | Bulk check |
| POST | `/{name}/clear` | Clear filter |
| GET | `/{name}/stats` | Get statistics |
| GET | `/list` | List all filters |

### Expiring Bloom Filter (`/api/v1/ebloom`)
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/` | Create new expiring filter |
| DELETE | `/{name}` | Delete filter |
| POST | `/{name}/insert` | Insert item |
| POST | `/{name}/contains` | Check item |
| POST | `/{name}/bulk/insert` | Bulk insert |
| POST | `/{name}/bulk/contains` | Bulk check |
| POST | `/{name}/clear` | Clear filter |
| GET | `/{name}/stats` | Get statistics |
| GET | `/list` | List all filters |

## Request/Response Models

```rust
// Create filter
struct CreateBloomRequest { name: String, capacity: usize, fpr: f64 }
struct CreateEbloomRequest { name: String, capacity: usize, fpr: f64, ttl_secs: u64 }

// Operations
struct ItemRequest { item: String }
struct BulkItemRequest { items: Vec<String> }
struct ContainsResponse { present: bool }
struct BulkContainsResponse { results: Vec<bool> }

// Stats
struct BloomStats { capacity, fpr, insert_count }
struct EbloomStats { capacity_per_level, fpr, total_inserts, active_levels, num_levels }
```

## Implementation Steps

- [x] Create `src/server/mod.rs` with basic Axum router setup
- [x] Create `src/server/state.rs` with AppState (HashMap<String, Filter> with RwLock)
- [x] Create `src/server/error.rs` with API error handling
- [x] Implement Bloom routes/handlers in `src/server/routes/bloom.rs`
- [x] Implement Ebloom routes/handlers in `src/server/routes/ebloom.rs`
- [x] Add Swagger UI with utoipa
- [x] Add server binary entry point
- [ ] Test all endpoints

---

# Integration Tests Plan

## Location
`tests/server_integration.rs`

## Test Structure
Use `tower::ServiceExt` for one-shot testing without spawning actual server.

## Test Categories

### 1. Bloom Filter Tests
| Test | Description |
|------|-------------|
| `create_bloom_filter` | Create filter, expect 200 |
| `create_bloom_filter_duplicate` | Create same filter twice, expect 409 |
| `delete_bloom_filter` | Create then delete, expect 200 |
| `delete_bloom_filter_not_found` | Delete non-existent, expect 404 |
| `insert_and_contains` | Insert item, check contains returns true |
| `contains_not_present` | Check item not inserted, expect false |
| `bulk_insert_and_contains` | Bulk insert, bulk check |
| `clear_filter` | Insert, clear, check contains returns false |
| `list_filters` | Create multiple, list all |
| `filter_stats` | Create, insert, check stats |

### 2. Expiring Bloom Filter Tests
| Test | Description |
|------|-------------|
| `create_ebloom_filter` | Create expiring filter |
| `create_ebloom_filter_duplicate` | Duplicate, expect 409 |
| `ebloom_insert_and_contains` | Basic insert/contains |
| `ebloom_bulk_operations` | Bulk insert/contains |
| `ebloom_stats` | Check stats response |

### 3. Error Handling Tests
| Test | Description |
|------|-------------|
| `invalid_capacity_zero` | capacity=0, expect 500 |
| `invalid_fpr_out_of_range` | fpr=2.0, expect 500 |
| `operation_on_nonexistent_filter` | Insert to missing filter, expect 404 |

## Implementation Steps
1. [x] Create `tests/server_integration.rs`
2. [x] Add test helper for creating test router
3. [x] Implement bloom filter tests
4. [x] Implement ebloom filter tests
5. [x] Implement error handling tests
6. [x] Run with `cargo test --features server --test server_integration`
