# Current Task Context

Working to bring this crate to the release.

## Checklist

- [x] python bindings
- [x] rest api
- [x] integration tests
- [x] python package
- [ ] benchmarks like how many inserts per second and how many queries

## Python Package Publishing Plan

### Trusted Publisher (DONE)
PyPI pending publisher configured:
- Project: `probabilistic-rs`
- Repository: `oiwn/probabilistic-rs`
- Workflow: `publish-pypi.yml`
- Environment: `pypi`

### Remaining Steps (DONE)

- [x] Created `.github/workflows/publish-pypi.yml` - builds wheels for linux/macos/windows
- [x] Updated `pyproject.toml` with PyPI metadata

### To Publish

1. Optional: Create GitHub Environment `pypi` (Repo → Settings → Environments)
2. Bump version in `Cargo.toml` → merge to `main` → auto-publishes to PyPI

### Installation (after publish)

**Rust:**
```bash
cargo add probabilistic-rs
```

**Python:**
```bash
pip install probabilistic-rs
```

