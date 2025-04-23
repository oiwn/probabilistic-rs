use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, FjallFilter,
    FjallFilterConfigBuilder,
};
use rand::{Rng, distr::Alphanumeric};
use std::{fs, path::PathBuf, time::Duration};

// Helper function to generate random string data
fn generate_random_string(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

// Helper to create test data
fn generate_test_data(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_random_string(32)).collect()
}

// Helper to create a temporary database path
fn temp_db_path(prefix: &str) -> PathBuf {
    use rand::random;
    PathBuf::from(format!("{}_{}.fjall", prefix, random::<u64>()))
}

// Helper to clean up database files - fjall uses directories
fn cleanup_db(path: &PathBuf) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

// Create a configured filter
fn create_test_filter(db_path: PathBuf, capacity: usize) -> FjallFilter {
    let config = FilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(1))
        .max_levels(5)
        .build()
        .expect("Failed to create config");

    let fjall_config = FjallFilterConfigBuilder::default()
        .db_path(db_path)
        .filter_config(Some(config))
        .snapshot_interval(Duration::from_secs(60)) // Set long interval to control snapshot timing
        .build()
        .expect("Failed to build FjallFilterConfig");

    FjallFilter::new(fjall_config).expect("Failed to create FjallFilter")
}

fn bench_tricky_issue(c: &mut Criterion) {
    let mut group = c.benchmark_group("fjall_tricky");
    group.sample_size(10); // Reduce sample size for disk operations
    group.measurement_time(Duration::from_secs(15));

    // In benches/fjall_snapshot_benchmarks.rs

    for capacity in [10_000, 100_000] {
        let db_path = temp_db_path(&format!("fjall_tricky_bench_{}", capacity));
        let test_data = generate_test_data(capacity);

        group.bench_with_input(
            BenchmarkId::new("snapshot", capacity),
            &(capacity, &test_data, db_path.clone()),
            |b, (cap, data, path)| {
                // Create the filter OUTSIDE the measurement loop
                let mut filter = create_test_filter(path.clone(), *cap);
                for item in data.iter() {
                    let _ = filter.insert(item.as_bytes());
                }

                // Now ONLY measure the snapshot operation
                b.iter(|| {
                    // This is all that will be measured
                    filter.save_snapshot().unwrap()
                });

                // Manually clean up after benchmark
                drop(filter);
            },
        );
    }
}

// Benchmark snapshot performance
fn bench_fjall_snapshots(c: &mut Criterion) {
    let mut group = c.benchmark_group("fjall_snapshot_operations");
    group.sample_size(10); // Reduce sample size for disk operations
    group.measurement_time(Duration::from_secs(15));

    // Test different capacities
    for capacity in [10_000, 100_000] {
        // Create database path for this benchmark
        let db_path = temp_db_path(&format!("fjall_snapshot_bench_{}", capacity));
        let test_data = generate_test_data(capacity);

        // Setup benchmark that measures snapshot time after inserting elements
        group.bench_with_input(
            BenchmarkId::new("snapshot", capacity),
            &(capacity, &test_data, db_path.clone()),
            |b, (cap, data, path)| {
                b.iter_with_setup(
                    || {
                        // Setup: Create filter and insert data
                        let mut filter = create_test_filter(path.clone(), *cap);
                        for item in data.iter() {
                            if let Err(e) = filter.insert(item.as_bytes()) {
                                eprintln!("Insert error (continuing): {}", e);
                            }
                        }
                        filter
                    },
                    |filter| {
                        // Measure: Time the snapshot operation directly
                        // if let Err(e) = filter.save_snapshot() {
                        //     eprintln!("Snapshot error: {}", e);
                        // }
                    },
                )
            },
        );

        // Cleanup database after this benchmark
        cleanup_db(&db_path);
    }
    group.finish();
}

// Benchmark snapshot with increasing fill levels
fn bench_fjall_snapshot_fill_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("fjall_snapshot_fill_levels");
    group.sample_size(10);

    // Fixed capacity, vary fill percentage
    let capacity = 100_000;

    for fill_percentage in [25, 50, 75] {
        let fill_count = (capacity * fill_percentage) / 100;
        let db_path =
            temp_db_path(&format!("fjall_snapshot_fill_{}", fill_percentage));
        let test_data = generate_test_data(fill_count);

        group.bench_with_input(
            BenchmarkId::new("fill", fill_percentage),
            &(capacity, &test_data, db_path.clone()),
            |b, (cap, data, path)| {
                b.iter_with_setup(
                    || {
                        // Setup: Create filter and insert data
                        let mut filter = create_test_filter(path.clone(), *cap);
                        for item in data.iter() {
                            if let Err(e) = filter.insert(item.as_bytes()) {
                                eprintln!("Insert error (continuing): {}", e);
                            }
                        }
                        filter
                    },
                    |filter| {
                        // Measure: Time the snapshot operation
                        if let Err(e) = filter.save_snapshot() {
                            eprintln!("Snapshot error: {}", e);
                        }
                    },
                )
            },
        );

        cleanup_db(&db_path);
    }
    group.finish();
}

// Benchmark multi-level effects on snapshots
fn bench_fjall_multi_level_snapshots(c: &mut Criterion) {
    let mut group = c.benchmark_group("fjall_multi_level_snapshots");
    group.sample_size(10);

    let capacity = 100_000;
    let item_count = 50_000;

    for level_count in [1, 3, 5] {
        let db_path =
            temp_db_path(&format!("fjall_snapshot_levels_{}", level_count));
        let test_data = generate_test_data(item_count);

        group.bench_with_input(
            BenchmarkId::new("levels", level_count),
            &(capacity, level_count, &test_data, db_path.clone()),
            |b, (cap, levels, data, path)| {
                b.iter_with_setup(
                    || {
                        // Create a special filter with the specified number of levels
                        let config = FilterConfigBuilder::default()
                            .capacity(*cap)
                            .false_positive_rate(0.01)
                            .level_duration(Duration::from_millis(1))
                            .max_levels(*levels)
                            .build()
                            .expect("Failed to create config");

                        let fjall_config = FjallFilterConfigBuilder::default()
                            .db_path(path.clone())
                            .filter_config(Some(config))
                            .snapshot_interval(Duration::from_secs(60))
                            .build()
                            .expect("Failed to build FjallFilterConfig");

                        let mut filter = FjallFilter::new(fjall_config)
                            .expect("Failed to create FjallFilter");

                        // Fill multiple levels by inserting and waiting
                        for chunk in data.chunks(data.len() / *levels) {
                            for item in chunk {
                                if let Err(e) = filter.insert(item.as_bytes()) {
                                    eprintln!("Insert error: {}", e);
                                }
                            }
                            // Force level rotation
                            std::thread::sleep(Duration::from_millis(2));
                        }

                        filter
                    },
                    |filter| {
                        // Measure the snapshot operation
                        if let Err(e) = filter.save_snapshot() {
                            eprintln!("Snapshot error: {}", e);
                        }
                    },
                )
            },
        );

        cleanup_db(&db_path);
    }
    group.finish();
}

// Compare fjall vs redb
fn bench_backend_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("fjall_backend_comparison");
    group.sample_size(10);

    let capacity = 10_000;
    let test_data = generate_test_data(capacity);

    // Benchmark Fjall
    let fjall_path = temp_db_path("backend_comparison_fjall");
    group.bench_with_input(
        BenchmarkId::new("fjall", capacity),
        &(capacity, &test_data, fjall_path.clone()),
        |b, (cap, data, path)| {
            b.iter_with_setup(
                || {
                    let mut filter = create_test_filter(path.clone(), *cap);
                    for item in data.iter() {
                        let _ = filter.insert(item.as_bytes());
                    }
                    filter
                },
                |filter| {
                    let _ = filter.save_snapshot();
                },
            )
        },
    );
    cleanup_db(&fjall_path);

    // Benchmark ReDB if available
    #[cfg(feature = "redb")]
    {
        use expiring_bloom_rs::{RedbFilter, RedbFilterConfigBuilder};

        let redb_path = PathBuf::from(format!(
            "backend_comparison_redb_{}.redb",
            rand::random::<u64>()
        ));

        group.bench_with_input(
            BenchmarkId::new("redb", capacity),
            &(capacity, &test_data, redb_path.clone()),
            |b, (cap, data, path)| {
                b.iter_with_setup(
                    || {
                        let config = FilterConfigBuilder::default()
                            .capacity(*cap)
                            .false_positive_rate(0.01)
                            .level_duration(Duration::from_secs(1))
                            .max_levels(5)
                            .build()
                            .expect("Failed to create config");

                        let redb_config = RedbFilterConfigBuilder::default()
                            .db_path(path.clone())
                            .filter_config(Some(config))
                            .snapshot_interval(Duration::from_secs(60))
                            .build()
                            .expect("Failed to build RedbFilterConfig");

                        let mut filter = RedbFilter::new(redb_config)
                            .expect("Failed to create RedbFilter");

                        for item in data.iter() {
                            let _ = filter.insert(item.as_bytes());
                        }
                        filter
                    },
                    |filter| {
                        let _ = filter.save_snapshot();
                    },
                )
            },
        );
        let _ = fs::remove_file(&redb_path);
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_tricky_issue,
    bench_fjall_snapshots,
    bench_fjall_snapshot_fill_levels,
    bench_fjall_multi_level_snapshots,
    bench_backend_comparison
);
criterion_main!(benches);
