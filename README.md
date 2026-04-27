# probabilistic-rs

[![Crates.io](https://img.shields.io/crates/v/probabilistic-rs.svg)](https://crates.io/crates/probabilistic-rs)
[![PyPI](https://img.shields.io/pypi/v/probabilistic-rs.svg)](https://pypi.org/project/probabilistic-rs/)
[![Documentation](https://docs.rs/probabilistic-rs/badge.svg)](https://docs.rs/probabilistic-rs)
[![codecov](https://codecov.io/gh/oiwn/probabilistic-rs/graph/badge.svg?token=5JMM0V5RFO)](https://codecov.io/gh/oiwn/probabilistic-rs)

Probabilistic data structures in Rust with Python bindings and HTTP API.

## Features

- **Bloom Filter** — fast membership testing with bulk ops and optional Fjall persistence
- **Expiring Bloom Filter** — auto-expires elements via sliding time windows
- **HTTP API** — REST server with Swagger UI, managing multiple named filters
- **Python Bindings** — native wheels via PyO3/maturin
- **CLI + TUI** — interactive terminal interface

## Installation

**Rust:**
```bash
cargo add probabilistic-rs
```

**Python:**
```bash
pip install probabilistic-rs
```

## Quick Start

### Rust — Bloom Filter

```rust
use probabilistic_rs::bloom::{BloomFilter, BloomFilterConfigBuilder, BloomFilterOps};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BloomFilterConfigBuilder::default()
        .capacity(10000)
        .false_positive_rate(0.01)
        .build()?;

    let filter = BloomFilter::create(config).await?;
    filter.insert(b"item1")?;
    assert!(filter.contains(b"item1")?);
    Ok(())
}
```

### Rust — Expiring Bloom Filter

```rust
use probabilistic_rs::ebloom::{
    ExpiringBloomFilter, ExpiringFilterConfigBuilder, ExpiringBloomFilterOps,
};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ExpiringFilterConfigBuilder::default()
        .capacity(1000)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(60))
        .max_levels(3)
        .build()?;

    let mut filter = ExpiringBloomFilter::new(config)?;
    filter.insert(b"test_item")?;
    assert!(filter.query(b"test_item")?);
    Ok(())
}
```

### Python

```python
from probabilistic_rs import BloomFilter, ExpiringBloomFilter

bf = BloomFilter(capacity=10000, false_positive_rate=0.01)
bf.insert(b"item1")
assert bf.contains(b"item1")

ebf = ExpiringBloomFilter(capacity=1000, false_positive_rate=0.01, ttl_seconds=60)
ebf.insert(b"temp_item")
assert ebf.query(b"temp_item")
```

## HTTP API

```bash
# Start server (default: localhost:3000)
probabilistic-server

# Create a filter, insert, query
curl -X POST http://localhost:3000/api/v1/bloom/create \
  -H "Content-Type: application/json" \
  -d '{"name":"my-filter","capacity":10000,"false_positive_rate":0.01}'

curl -X POST http://localhost:3000/api/v1/bloom/insert \
  -H "Content-Type: application/json" \
  -d '{"name":"my-filter","item":"hello"}'

curl -X POST http://localhost:3000/api/v1/bloom/contains \
  -H "Content-Type: application/json" \
  -d '{"name":"my-filter","item":"hello"}'
```

Endpoints: `create`, `delete`, `insert`, `contains`, `bulk_insert`, `bulk_contains`, `clear`, `stats`, `list` — available for both `/api/v1/bloom` and `/api/v1/ebloom`. Swagger UI at `/swagger-ui`.

## CLI

```bash
# Create filter
expblf create --db-path myfilter.fjall --capacity 10000 --fpr 0.01

# Operations
expblf load --db-path myfilter.fjall insert --element "key"
expblf load --db-path myfilter.fjall check --element "key"

# Interactive TUI
expblf tui --db-path myfilter.fjall
```

## Benchmarks

Measured on Apple M-series via `cargo bench` (criterion, 100 samples). Times are total for N operations.

**Bloom Filter (in-memory)**

| Operation | 1K elements | 100K elements | 1M elements |
|-----------|-------------|---------------|-------------|
| Insert    | 60.2 µs     | 6.15 ms       | 64.1 ms     |
| Query     | 61.3 µs     | 6.17 ms       | 63.1 ms     |

**Expiring Bloom Filter (in-memory, 3 levels — 5 levels nearly identical)**

| Operation    | 1K elements | 100K elements | 1M elements |
|--------------|-------------|---------------|-------------|
| Insert       | 63.8 µs     | 6.61 ms       | 68.0 ms     |
| Query        | 63.4 µs     | 6.52 ms       | 67.0 ms     |
| Bulk insert  | 59.7 µs     | 6.16 ms       | 63.8 ms     |
| Bulk query   | 62.3 µs     | 6.41 ms       | 65.8 ms     |
| Level rotate | 224 µs      | 255 µs        | —           |

Both filters sustain ~15–17M ops/s (60–65 ns/op) across all dataset sizes. The expiring filter adds ~5–10% overhead over plain bloom due to multi-level bookkeeping. Bulk operations match or slightly outperform single-item ops. Level rotation (TTL expiry) takes ~250 µs regardless of filter size.

## Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `capacity` | Max elements | 1,000,000 |
| `false_positive_rate` | Desired FPR | 0.01 |
| `level_duration` | TTL per level (expiring) | 60s |
| `max_levels` | Filter levels (expiring) | 3 |

## License

MIT
