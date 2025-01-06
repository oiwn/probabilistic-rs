use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use expiring_bloom_rs::{
    FilterConfigBuilder, InMemorySlidingBloomFilter, SlidingBloomFilter,
};
use rand::{distributions::Alphanumeric, Rng};
use std::{time::Duration, time::SystemTime};

// Helper function to generate random string data
fn generate_random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

// Helper to create test data
fn generate_test_data(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_random_string(32)).collect()
}

fn create_test_filter(capacity: usize) -> InMemorySlidingBloomFilter {
    let config = FilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(1))
        .max_levels(5)
        .build()
        .expect("Failed to create config");

    InMemorySlidingBloomFilter::new(config)
        .expect("Failed to create Bloom filter")
}

// Helper to create "expired" timestamps
// see: https://github.com/rust-lang/rust/issues/100141
// to figure why this crap is so complicated.
#[allow(dead_code)]
fn create_expired_timestamps(
    num_levels: usize,
    expiration_duration: Duration,
) -> Vec<SystemTime> {
    let now = SystemTime::now();
    let past =
        match now.checked_sub(expiration_duration * (num_levels as u32 + 1)) {
            Some(time) => time,
            None => SystemTime::UNIX_EPOCH, // Fallback to epoch if overflow
        };
    vec![past; num_levels]
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_operations");

    // Test different capacities
    for capacity in [1000, 100_000, 1_000_000] {
        let test_data = generate_test_data(capacity);

        // Benchmark in-memory storage

        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_batched(
                    || create_test_filter(*cap),
                    |mut filter| {
                        for item in data.iter() {
                            if let Err(e) = filter.insert(item.as_bytes()) {
                                eprintln!("Insert error (continuing): {}", e);
                                continue;
                            }
                        }
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        // Generate test data with 80% known elements
        let known_data = generate_test_data((capacity as f64 * 0.8) as usize);
        let unknown_data = generate_test_data((capacity as f64 * 0.2) as usize);

        // Benchmark in-memory queries
        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &known_data, &unknown_data),
            |b, (cap, known, unknown)| {
                let mut filter = create_test_filter(*cap);

                // Insert known data
                for item in known.iter() {
                    filter.insert(item.as_bytes()).unwrap();
                }

                b.iter(|| {
                    // Query mix of known and unknown
                    for item in known.iter() {
                        filter.query(item.as_bytes()).unwrap();
                    }
                    for item in unknown.iter() {
                        filter.query(item.as_bytes()).unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_query);
criterion_main!(benches);
