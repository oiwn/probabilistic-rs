# Ideas

## 1. Figure out ribbon filter.

## 2. Platform‑Dependent Assumptions
- `usize` serialized as 8 bytes (assumes 64‑bit target)
- `usize` → `u64` casts without validation
- Manual byte indexing instead of `try_into()`

