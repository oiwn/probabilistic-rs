Guidance for automation agents hacking on this repository. The full spec lives
in the code—use this file as the quick brief.

## Project Overview

- Rust time-decaying Bloom filter with optional persistence via Fjall.
- API targets: simple `BloomFilter` core + higher-level expiring filter built
  on top.
- Delivered components: CLI/TUI, Axum HTTP server, extensive test suite.

## Architecture Highlights

- `src/bloom/`: core filter (`filter.rs`), configuration (`config.rs`), traits (`traits.rs`), errors (`error.rs`).
- `src/storage/`: backend implementations (`fjall_filter.rs`, `inmemory_filter.rs`).
- `src/probablistic/`: legacy expiring filter that will be rebuilt around the core filter.
- Hashing (`src/hash.rs`) uses Murmur3 + FNV-1a double hashing to compute bit indices.
- Concurrency relies on interior `RwLock` + atomics; external callers only need `Arc<BloomFilter>` when sharing.

## Core API Expectations

- `BloomFilterOps::insert`, `clear`, and bulk variants accept `&self`; callers no longer wrap the filter in their own locks.
- Persistence is opt-in via `PersistenceConfig`; Fjall chunks + dirty bit tracking keep snapshot writes bounded.
- `PersistentBloomFilter::load_from_storage` now also takes `&self`, consistent with the interior-mutability model.

## Persistence Notes

- Fjall backend stores config + bitvector chunks; see `tests/core_bloom_fjall_tests.rs` for lifecycle coverage.
- `tests/core_bloom_fjall_tests.rs::test_arc_shared_concurrent_read_write` confirms multiple writer/reader threads behave correctly when the filter is wrapped in an `Arc`.
- When touching persistence, run the dedicated Fjall tests (`cargo test fjall_tests core_bloom_fjall_tests`).

## Recent Work

1. Converted public ops traits to `&self`, updated implementations/examples/tests accordingly.
2. Added concurrency regression test proving safe Arc-based sharing with concurrent writers/readers.
3. Brought persistence traits in line with the new API surface (`load_from_storage(&self)`).
4. Landed Criterion bench `benches/bloom_fjall_benchmarks.rs` measuring incremental Fjall snapshot throughput (1M capacity, 4 KiB chunks, 10%/50% fills + 1% dirty delta).

## Ideas List

- [x] add Criterion benchmarks covering core Bloom filter persistence with Fjall
      snapshots (incremental save throughput + chunk stats).
- [ ] explore refactoring the expiring Bloom filter to reuse the new core filter
      instead of bespoke logic; identify required API gaps before implementation.
- [x] remove the ReDB backend (feature flags, code, tests, docs) once Fjall parity is validated.
- [ ] cli tool to communicate with the database. check if element exists, insert element.
- [ ] run web server for core bloom and expiring bloom
- [ ] operations with databases, like union of 2, intersection. look what's possible.
- [ ] better cli which should be extendable
- [ ] cuckoo filter
- [ ] hyper-log-log
- [ ] quotien filter https://en.wikipedia.org/wiki/Quotient_filter
- [ ] count-min sketch

## Build & Test Commands
```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench
```
Feature combos: `--features "fjall,server,cli"`. Run targeted tests with `cargo
test fjall_tests` or `cargo test core_bloom_filter_tests` to stay fast.
- Fjall incremental snapshot bench: `cargo bench --bench bloom_fjall_benchmarks --features fjall` (needs gnuplot or uses plotters fallback).

## Working Guidelines
- Prefer `rg` for search; keep edits ASCII unless the file already uses Unicode.
- Watch for user-local changes in the worktree; never revert unrelated edits.
- Update examples/tests/docs together when changing core behavior.
- If persistence snapshots fail, fix both code and regression tests before continuing.
- do not forget to format code
