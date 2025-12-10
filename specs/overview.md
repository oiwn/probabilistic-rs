# Project Overview

## Core Purpose

Rust library for probabilistic data structures with optional persistence.

## Data Structures

### ✅ Core Bloom Filter (`src/bloom/`)
- Standard Bloom filter implementation
- Configurable false positive rate
- Bulk operations (`insert_bulk`, `contains_bulk`)
- Persistence backends: In-memory, Fjall

### 🚧 Expiring Bloom Filter (`src/ebloom/`)
- Time-decaying multi-level Bloom filter
- CURRENTLY: Cleanup and reorganization phase
- Goal: Automatic expiration of old entries

### 📋 Planned Data Structures
- Cuckoo filters (next after expiring bloom)
- HyperLogLog
- Count-min sketch
- Quotient filters

## Architecture

### Core Components
- `src/bloom/` - Standard Bloom filter
- `src/ebloom/` - Expiring Bloom filter
- `src/hash.rs` - Murmur3 + FNV-1a double hashing
- `src/common.rs` - Shared utilities

### Persistence Layer
- `src/bloom/storage.rs` - Core filter backends
- Fjall for disk-based persistence
- In-memory backend for testing/ephemeral use

### Applications
- CLI/TUI for data structure interaction
- Axum HTTP server for remote access
- Comprehensive examples

## Concurrency Model

- Interior `RwLock` + atomics
- `Arc<T>` for cross-thread sharing
- All operations accept `&self`

## Current Implementation Status

### ✅ Working
- Core Bloom filter with full API
- Fjall and in-memory persistence
- Bulk operations and optimizations
- Test suite and benchmarks
- CLI, TUI, and HTTP server

### 🚧 In Progress  
- Expiring Bloom filter cleanup and reorganization
- Multi-level Fjall persistence for expiring filter

### 📋 Next Up
- Complete expiring filter implementation
- Cuckoo filter implementation
- HyperLogLog implementation

## Build & Test Commands

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench
```

Feature flags: `--features "fjall,server,cli"`

Targeted testing:
- `cargo test core_bloom_filter_tests` - Core bloom filter
- `cargo test ebloom_tests` - Exiring bloom filter
- `cargo test fjall_tests` - Persistence tests