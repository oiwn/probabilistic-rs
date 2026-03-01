mod fjall_ebloom_bench {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    use criterion::{
        BenchmarkId, Criterion, Throughput, black_box, criterion_group,
    };
    use probabilistic_rs::ebloom::config::{
        ExpiringFilterConfigBuilder, ExpiringPersistenceConfigBuilder,
    };
    use probabilistic_rs::ebloom::filter::ExpiringBloomFilter;
    use probabilistic_rs::ebloom::traits::ExpiringBloomFilterOps;
    use tokio::runtime::Runtime;

    const CAPACITY_PER_LEVEL: usize = 100_000;
    const NUM_LEVELS: usize = 3;
    const EXTRA_INSERTS: usize = CAPACITY_PER_LEVEL / 100; // 1% incremental writes
    const BATCH_SIZE: usize = 5_000;
    const LEVEL_DURATION: Duration = Duration::from_secs(3600);
    const SEED: u64 = 42;

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn ebloom_incremental_snapshot_benchmark(c: &mut Criterion) {
        let mut group = c.benchmark_group("ebloom_fjall_incremental_snapshot");
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(15));

        let runtime = Runtime::new()
            .expect("failed to create Tokio runtime for benchmarks");

        let scenarios = [("fill_10", 10_000usize), ("fill_50", 50_000usize)];

        for (label, baseline_count) in scenarios {
            let baseline = generate_permutation_payloads(
                baseline_count,
                SEED + baseline_count as u64,
            );
            let incremental = generate_dirty_payloads(EXTRA_INSERTS);

            let expected =
                compute_expected_stats(&runtime, label, &baseline, &incremental);

            println!(
                "scenario={label} baseline_items={} incremental_items={} dirty_chunks={} dirty_bytes={} seed={SEED}",
                baseline.len(),
                incremental.len(),
                expected.dirty_chunks,
                expected.dirty_bytes,
            );

            group.throughput(Throughput::Bytes(expected.dirty_bytes as u64));

            group.bench_with_input(
                BenchmarkId::new("incremental_save", label),
                &baseline_count,
                |b, &_baseline_count| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for iteration in 0..iters {
                            let ctx = setup_context(
                                &runtime,
                                label,
                                &baseline,
                                &incremental,
                                iteration,
                            );

                            let SnapshotBenchContext { filter, db_path } = ctx;

                            let start = Instant::now();
                            runtime
                                .block_on(filter.save_snapshot())
                                .expect("incremental snapshot failed");
                            total += start.elapsed();

                            drop(filter);
                            if let Err(err) = fs::remove_dir_all(&db_path) {
                                eprintln!(
                                    "cleanup warning ({}): {}",
                                    db_path.display(),
                                    err
                                );
                            }
                        }
                        total
                    });
                },
            );
        }
        group.finish();
    }

    pub fn ebloom_level_rotation_benchmark(c: &mut Criterion) {
        let mut group = c.benchmark_group("ebloom_fjall_rotation");
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(10));

        let runtime = Runtime::new()
            .expect("failed to create Tokio runtime for benchmarks");

        let scenarios = [
            ("levels_3_capacity_100k", 3usize, 100_000usize),
            ("levels_5_capacity_100k", 5, 100_000),
        ];

        for (label, num_levels, capacity) in scenarios {
            let test_data =
                generate_permutation_payloads(capacity / num_levels, SEED);

            group.bench_with_input(
                BenchmarkId::new("rotate_levels", label),
                &(num_levels, capacity),
                |b, &(levels, cap)| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for iteration in 0..iters {
                            let db_path = temp_db_path(
                                &format!("rotate_{label}"),
                                iteration,
                            );
                            let config =
                                build_config(db_path.clone(), levels, cap);

                            let filter = runtime
                                .block_on(ExpiringBloomFilter::create(config))
                                .expect("failed to create filter");

                            insert_in_batches(&filter, &test_data);

                            let start = Instant::now();
                            runtime
                                .block_on(filter.rotate_levels())
                                .expect("rotation failed");
                            total += start.elapsed();

                            drop(filter);
                            if let Err(err) = fs::remove_dir_all(&db_path) {
                                eprintln!(
                                    "cleanup warning ({}): {}",
                                    db_path.display(),
                                    err
                                );
                            }
                        }
                        total
                    });
                },
            );
        }
        group.finish();
    }

    pub fn ebloom_load_reconstruct_benchmark(c: &mut Criterion) {
        let mut group = c.benchmark_group("ebloom_fjall_load_reconstruct");
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(10));

        let runtime = Runtime::new()
            .expect("failed to create Tokio runtime for benchmarks");

        let scenarios = [
            ("levels_3_100k", 3usize, 100_000usize),
            ("levels_5_100k", 5, 100_000),
        ];

        for (label, num_levels, capacity) in scenarios {
            let test_data = generate_permutation_payloads(capacity, SEED);

            group.bench_with_input(
                BenchmarkId::new("load_from_storage", label),
                &(num_levels, capacity),
                |b, &(levels, cap)| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for iteration in 0..iters {
                            let db_path =
                                temp_db_path(&format!("load_{label}"), iteration);
                            let config =
                                build_config(db_path.clone(), levels, cap);

                            // Create and populate filter
                            let filter = runtime
                                .block_on(ExpiringBloomFilter::create(
                                    config.clone(),
                                ))
                                .expect("failed to create filter");

                            insert_in_batches(&filter, &test_data);

                            // Save snapshot
                            runtime
                                .block_on(filter.save_snapshot())
                                .expect("snapshot failed");

                            drop(filter);

                            // Measure load time
                            let start = Instant::now();
                            let loaded_filter = runtime
                                .block_on(ExpiringBloomFilter::load(
                                    db_path.clone(),
                                ))
                                .expect("failed to load filter");
                            total += start.elapsed();

                            // Verify data integrity
                            for item in test_data.iter().take(100) {
                                let found = loaded_filter
                                    .contains(item.as_slice())
                                    .expect("contains check failed");
                                assert!(found, "item should exist after load");
                            }

                            drop(loaded_filter);
                            if let Err(err) = fs::remove_dir_all(&db_path) {
                                eprintln!(
                                    "cleanup warning ({}): {}",
                                    db_path.display(),
                                    err
                                );
                            }
                        }
                        total
                    });
                },
            );
        }
        group.finish();
    }

    struct SnapshotBenchContext {
        filter: ExpiringBloomFilter,
        db_path: PathBuf,
    }

    struct SnapshotStats {
        dirty_chunks: usize,
        dirty_bytes: usize,
    }

    fn setup_context(
        runtime: &Runtime,
        label: &str,
        baseline: &[Vec<u8>],
        incremental: &[Vec<u8>],
        iteration: u64,
    ) -> SnapshotBenchContext {
        let db_path = temp_db_path(label, iteration);
        let config =
            build_config(db_path.clone(), NUM_LEVELS, CAPACITY_PER_LEVEL);
        let filter = runtime
            .block_on(async { ExpiringBloomFilter::create(config).await })
            .expect("failed to create ExpiringBloom filter");

        insert_in_batches(&filter, baseline);

        runtime
            .block_on(filter.save_snapshot())
            .expect("initial snapshot failed");

        // Reload the filter to reset dirty tracking before measuring incremental writes
        drop(filter);
        let filter = runtime
            .block_on(async { ExpiringBloomFilter::load(db_path.clone()).await })
            .expect("failed to reload filter for incremental phase");

        insert_in_batches(&filter, incremental);

        SnapshotBenchContext { filter, db_path }
    }

    fn compute_expected_stats(
        runtime: &Runtime,
        label: &str,
        baseline: &[Vec<u8>],
        incremental: &[Vec<u8>],
    ) -> SnapshotStats {
        let db_path = temp_db_path(label, u64::MAX);
        let config =
            build_config(db_path.clone(), NUM_LEVELS, CAPACITY_PER_LEVEL);
        let filter = runtime
            .block_on(async { ExpiringBloomFilter::create(config).await })
            .expect("failed to create filter for stats");

        insert_in_batches(&filter, baseline);
        runtime
            .block_on(filter.save_snapshot())
            .expect("initial snapshot failed during stats");

        drop(filter);
        let filter = runtime
            .block_on(async { ExpiringBloomFilter::load(db_path.clone()).await })
            .expect("failed to reload filter for stats phase");

        insert_in_batches(&filter, incremental);

        // For ebloom, we measure the dirty chunks from the current level
        // Since we don't have direct access to dirty_chunks, estimate based on inserts
        let dirty_chunks = (incremental.len() as f64 / CAPACITY_PER_LEVEL as f64
            * 100.0)
            .ceil() as usize;
        let dirty_bytes = dirty_chunks * 4096; // chunk_size_bytes

        drop(filter);

        if let Err(err) = fs::remove_dir_all(&db_path) {
            eprintln!(
                "cleanup warning (expected stats, {}): {}",
                db_path.display(),
                err
            );
        }

        SnapshotStats {
            dirty_chunks,
            dirty_bytes,
        }
    }

    fn build_config(
        db_path: PathBuf,
        num_levels: usize,
        capacity_per_level: usize,
    ) -> probabilistic_rs::ebloom::config::ExpiringFilterConfig {
        let persistence = ExpiringPersistenceConfigBuilder::default()
            .db_path(db_path)
            .chunk_size_bytes(4096)
            .build()
            .expect("failed to build persistence config");

        ExpiringFilterConfigBuilder::default()
            .capacity_per_level(capacity_per_level)
            .target_fpr(0.01)
            .level_duration(LEVEL_DURATION)
            .num_levels(num_levels)
            .persistence(Some(persistence))
            .build()
            .expect("failed to build ebloom filter config")
    }

    fn insert_in_batches(filter: &ExpiringBloomFilter, items: &[Vec<u8>]) {
        for chunk in items.chunks(BATCH_SIZE) {
            for item in chunk {
                filter
                    .insert(black_box(item.as_slice()))
                    .expect("failed to insert payload");
            }
        }
    }

    fn generate_permutation_payloads(total: usize, seed: u64) -> Vec<Vec<u8>> {
        const WORDS: [&str; 12] = [
            "aurora", "binary", "cosmos", "delta", "ember", "flux", "gamma",
            "helios", "ion", "jaguar", "kepler", "lyric",
        ];

        let word_count = WORDS.len();
        let mut results = Vec::with_capacity(total);
        let mut offset = seed as usize % word_count;

        for index in 0..total {
            let first_idx = (index + offset) % word_count;
            let mut second_idx = ((index / word_count) + offset) % word_count;
            if second_idx == first_idx {
                second_idx = (second_idx + 1) % word_count;
            }

            let mut third_idx =
                ((index / (word_count * word_count)) + offset) % word_count;
            let mut shift = 0;
            while third_idx == first_idx || third_idx == second_idx {
                shift += 1;
                third_idx = (third_idx + shift) % word_count;
            }

            let payload = format!(
                "{}-{}-{}-{:06}",
                WORDS[first_idx], WORDS[second_idx], WORDS[third_idx], index
            );

            results.push(payload.into_bytes());
            offset = (offset + 1) % word_count;
        }

        results
    }

    fn generate_dirty_payloads(count: usize) -> Vec<Vec<u8>> {
        const SENTINEL: &[u8] = b"dirty-sentinel";
        let mut results = Vec::with_capacity(count);
        for _ in 0..count {
            results.push(SENTINEL.to_vec());
        }
        results
    }

    fn temp_db_path(label: &str, iteration: u64) -> PathBuf {
        let id = if iteration == u64::MAX {
            DB_COUNTER.fetch_add(1, Ordering::Relaxed)
        } else {
            iteration + DB_COUNTER.fetch_add(1, Ordering::Relaxed)
        };
        std::env::temp_dir().join(format!("fjall_ebloom_{label}_{id}.fjall"))
    }

    criterion_group!(
        fjall_ebloom_bench_group,
        ebloom_incremental_snapshot_benchmark,
        ebloom_level_rotation_benchmark,
        ebloom_load_reconstruct_benchmark,
    );
}

use fjall_ebloom_bench::fjall_ebloom_bench_group;

criterion::criterion_main!(fjall_ebloom_bench_group);
