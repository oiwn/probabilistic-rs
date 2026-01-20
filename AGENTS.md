# Agent Development Guide

## Quick Start

This repository contains probabilistic data structures implementations in Rust.

**IMPORTANT**: Always read and update `specs/ctx.md` for current task context, implementation plans, and progress tracking. This file contains only development environment and workflow guidance.

## Development Environment

### Build Commands
```bash
cargo fmt --all                    # Format code
cargo clippy --all-targets -- -D warnings  # Lint with strict warnings
cargo test                         # Run all tests
cargo bench                        # Run benchmarks
```

### Feature Flags
- `--features "fjall,server,cli"` for full feature set
- Individual features: `fjall` (persistence), `server` (HTTP), `cli` (TUI)

### Testing Strategy
- Run targeted tests to stay fast: `cargo test core_bloom_filter_tests`
- Persistence tests: `cargo test fjall_tests`
- Expiring filter tests: `cargo test ebloom_tests`

### File Organization
- `src/bloom/` - Core Bloom filter implementation
- `src/ebloom/` - Expiring Bloom filter (WORK IN PROGRESS)
- `tests/` - Test suites organized by component
- `specs/` - Detailed module specifications
- `benches/` - Criterion benchmarks

## Working Guidelines

### Code Style
- Use `rg` for code searches
- Keep edits ASCII unless file already uses Unicode
- All operations accept `&self` for interior mutability
- Use `Arc<T>` for cross-thread sharing

### Development Workflow
- Read existing files before editing
- Match existing code patterns exactly
- Update examples/tests/docs together
- Never revert unrelated user changes
- Always run `cargo fmt` before commits

### Persistence Notes
- Fjall backend uses chunked storage with dirty tracking
- When touching persistence, run `cargo test --features fjall`
- Test with both in-memory and Fjall backends

## Context Management

**All task context, implementation plans, and progress tracking live in `specs/ctx.md`.**

Before starting work:
1. Read `specs/ctx.md` to understand current task and architecture
2. Update `specs/ctx.md` with your implementation plan
3. Track progress and document decisions in `specs/ctx.md`
4. Keep `specs/ctx.md` updated as the single source of truth for current work
