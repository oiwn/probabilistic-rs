# Ideas

## 1. Figure out ribbon filter.

## 3. Trusted Publishing (crates.io)
Use `rust-lang/crates-io-auth-action` + OIDC instead of `CRATES_IO_TOKEN` secret.
Configure on crates.io crate settings → Trusted Publishing (owner, repo, workflow file).

## 2. Platform‑Dependent Assumptions
- `usize` serialized as 8 bytes (assumes 64‑bit target)
- `usize` → `u64` casts without validation
- Manual byte indexing instead of `try_into()`

