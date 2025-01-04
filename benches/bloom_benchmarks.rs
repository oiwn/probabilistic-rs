use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use dotenvy::dotenv;
use expiring_bloom_rs::{
    default_hash_function, inmemory_storage::InMemoryStorage,
    redb_storage::RedbStorage, redis_storage::RedisStorage, BloomFilterStorage,
    SlidingBloomFilter,
};
use rand::{distributions::Alphanumeric, Rng};
use std::{env, fs, path::PathBuf, time::Duration, time::SystemTime};

// Helper function to generate random string data
fn generate_random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn get_redis_url() -> String {
    dotenv().ok();
    env::var("REDIS_URI").unwrap_or_else(|_| "redis://127.0.0.1/".to_string())
}

// Helper to create test data
fn generate_test_data(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_random_string(32)).collect()
}

// Helper to create "expired" timestamps
// see: https://github.com/rust-lang/rust/issues/100141
// to figure why this crap is so complicated.
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

// Helper to create a bloom filter with given storage
fn create_bloom_filter<S: BloomFilterStorage>(
    storage: S,
    capacity: usize,
    fpr: f64,
) -> Result<SlidingBloomFilter<S>, Box<dyn std::error::Error>> {
    Ok(SlidingBloomFilter::new(
        storage,
        capacity,
        fpr,
        Duration::from_secs(1),
        5,
        default_hash_function,
    )?)
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_operations");

    // Test different capacities
    for capacity in [10, 100, 1_000] {
        let test_data = generate_test_data(capacity);

        // Benchmark in-memory storage
        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_batched(
                    || {
                        create_bloom_filter(
                            InMemoryStorage::new(*cap, 5)
                                .expect("Failed to create InMemory storage"),
                            *cap,
                            0.01,
                        )
                        .expect("Failed to create Bloom filter")
                    },
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

        // Benchmark ReDB storage
        group.bench_with_input(
            BenchmarkId::new("redb", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_batched(
                    || {
                        let path =
                            format!("bench_db_{}.redb", rand::random::<u64>());
                        let storage =
                            RedbStorage::open(&path.clone().into(), *cap, 5)
                                .expect("Failed to create ReDB storage");
                        let filter = create_bloom_filter(storage, *cap, 0.01)
                            .expect("Failed to create Bloom filter");
                        (filter, path)
                    },
                    |(mut filter, _path)| {
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

        // Benchmark Redis storage
        group.bench_with_input(
            BenchmarkId::new("redis", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_batched(
                    || {
                        let redis_url = get_redis_url();
                        let prefix =
                            format!("bench_filter_{}", rand::random::<u64>());
                        let storage =
                            RedisStorage::new(&redis_url, *cap, 5, &prefix)
                                .expect("Failed to create Redis storage");
                        create_bloom_filter(storage, *cap, 0.01)
                            .expect("Failed to create Bloom filter")
                    },
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

/* fn bench_query(c: &mut Criterion) {
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
                let mut filter = create_bloom_filter(
                    InMemoryStorage::new(*cap, 5).unwrap(),
                    *cap,
                    0.01,
                );

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

        // Benchmark ReDB queries
        /* group.bench_with_input(
            BenchmarkId::new("redb", capacity),
            &(capacity, &known_data, &unknown_data),
            |b, (cap, known, unknown)| {
                let path = format!("bench_db_{}.redb", rand::random::<u64>());
                let mut filter = create_bloom_filter(
                    RedbStorage::open(&path, *cap, 5).unwrap(),
                    *cap,
                    0.01,
                );

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

                // Cleanup
                let _ = fs::remove_file(path);
            },
        ); */
    }
    group.finish();
}

fn bench_cleanup(c: &mut Criterion) {
    let mut group = c.benchmark_group("cleanup_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        let test_data = generate_test_data(capacity);

        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_with_setup(
                    || {
                        // Setup: Create filter and insert data with artificially expired timestamps
                        let mut filter = SlidingBloomFilter::new(
                            InMemoryStorage::new(*cap, 5).unwrap(),
                            *cap,
                            0.01,
                            Duration::from_secs(1),
                            5,
                            default_hash_function,
                        )
                        .unwrap();

                        // Insert test data
                        for item in data.iter() {
                            filter.insert(item.as_bytes()).unwrap();
                        }

                        // Artificially expire the timestamps
                        let expired_timestamps =
                            create_expired_timestamps(5, Duration::from_secs(6));
                        for (level, timestamp) in
                            expired_timestamps.iter().enumerate()
                        {
                            filter
                                .storage
                                .set_timestamp(level, *timestamp)
                                .unwrap();
                        }

                        filter
                    },
                    |mut filter| {
                        // Benchmark just the cleanup operation
                        filter.cleanup_expired_levels().unwrap();
                    },
                );
            },
        );

        // Benchmark ReDB cleanup
        /* group.bench_with_input(
            BenchmarkId::new("redb", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                let path = format!("bench_db_{}.redb", rand::random::<u64>());
                let mut filter = create_bloom_filter(
                    RedbStorage::open(&path, *cap, 5).unwrap(),
                    *cap,
                    0.01,
                );

                // Insert data
                for item in data.iter() {
                    filter.insert(item.as_bytes()).unwrap();
                }

                // Wait for elements to expire
                std::thread::sleep(Duration::from_secs(6));

                b.iter(|| {
                    filter.cleanup_expired_levels().unwrap();
                });

                // Cleanup
                let _ = fs::remove_file(path);
            },
        ); */
    }
    group.finish();
} */

// criterion_group!(benches, bench_insert, bench_query, bench_cleanup);
criterion_group!(benches, bench_insert);
criterion_main!(benches);
