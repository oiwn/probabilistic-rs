# expiring-bloom-rs

[![Crates.io](https://img.shields.io/crates/v/expiring-bloom-rs.svg)](https://crates.io/crates/expiring-bloom-rs)
[![Documentation](https://docs.rs/expiring-bloom-rs/badge.svg)](https://docs.rs/expiring-bloom-rs)
[![codecov](https://codecov.io/gh/oiwn/expiring-bloom-rs/graph/badge.svg?token=5JMM0V5RFO)](https://codecov.io/gh/oiwn/expiring-bloom-rs)
[![dependency status](https://deps.rs/repo/github/oiwn/expiring-bloom-rs/status.svg)](https://deps.rs/repo/github/oiwn/expiring-bloom-rs)

# Time-Decaying Bloom Filter

A Rust implementation of a time-decaying Bloom filter with multiple storage
backends and a high-performance HTTP API server.

## Overview

This crate provides a Bloom filter implementation that automatically expires
elements after a configurable time period using a sliding window approach. It's
particularly useful for rate limiting, caching, and tracking recently seen items
where older data becomes less relevant over time.

### Key Features

- Time-based automatic element expiration
- Multiple storage backends (In-memory and ReDB persistence)
- Configurable false positive rate
- Multi-level sliding window design
- High-performance HTTP API server with Swagger UI
- Comprehensive benchmarking suite
- Thread-safe concurrent operations

## How It Works

The time-decaying Bloom filter uses a sliding window approach with the following
characteristics:

1. **Sub-Filters**: The main Bloom filter is divided into N sub-filters (BF_1, BF_2, …, BF_N)
2. **Time Windows**: Each sub-filter corresponds to a fixed time window T (e.g., 1 minute)
3. **Rotation Mechanism**: Sub-filters are rotated in a circular manner to represent sliding time intervals

### Operations

- **Insertion**: Elements are added to the current active sub-filter with timestamps
- **Query**: Checks for element presence across all non-expired sub-filters
- **Cleanup**: Automatically removes expired elements based on configured time windows

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
expiring-bloom-rs = "0.1"
```

### Basic Example

```rust
use expiring_bloom_rs::{FilterConfigBuilder, InMemorySlidingBloomFilter, SlidingBloomFilter};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure the filter
    let config = FilterConfigBuilder::default()
        .capacity(1000)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(60))
        .max_levels(3)
        .build()?;

    // Create an in-memory filter
    let mut filter = InMemorySlidingBloomFilter::new(config)?;

    // Insert and query items
    filter.insert(b"test_item")?;
    assert!(filter.query(b"test_item")?);
    
    Ok(())
}
```

### Using the HTTP Server

```rust
use expiring_bloom_rs::{ServerConfigBuilder, FilterConfigBuilder};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure the server
    let server_config = ServerConfigBuilder::default()
        .server_host("127.0.0.1".to_string())
        .server_port(3000)
        .bloom_db_path("bloom.redb".to_string())
        .bloom_capacity(10000)
        .bloom_false_positive_rate(0.01)
        .bloom_level_duration(Duration::from_secs(60))
        .bloom_max_levels(3)
        .build()?;

    // Start the server
    expiring_bloom_rs::run_server(server_config).await?;
    
    Ok(())
}
```

## API Endpoints

The HTTP server provides the following REST endpoints:

- `GET /health` - Health check endpoint
- `POST /items` - Insert an item into the filter
- `GET /items/{value}` - Query if an item exists in the filter
- `POST /cleanup` - Manually trigger cleanup of expired items
- `/swagger-ui` - Interactive API documentation

## Configuration

The filter can be configured with the following parameters:

- `capacity`: Maximum number of elements (default: 1000)
- `false_positive_rate`: Desired false positive rate (default: 0.01)
- `level_duration`: Duration after which entries in a level expire (default: 60s)
- `max_levels`: Number of filter levels for time-based expiration (default: 3)

## Performance

Bro, it's 🦀🦀🦀 RUST 🦀🦀🦀 and its BLAZINGLY FAST 🚀🚀🚀

### Memory Usage

Memory usage is calculated as:

```
total_bits = capacity * max_levels
memory_bytes = total_bits * 8
```

Since i use `u8` to store `bool`.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
