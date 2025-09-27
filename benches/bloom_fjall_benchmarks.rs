#[cfg(not(feature = "fjall"))]
fn main() {
    eprintln!(
        "`bloom_fjall_benchmarks` requires the `fjall` feature. Run with `cargo bench --bench bloom_fjall_benchmarks --features fjall`."
    );
}

#[cfg(feature = "fjall")]
mod fjall_core_bench {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    use criterion::{
        BenchmarkId, Criterion, Throughput, black_box, criterion_group,
    };
    use expiring_bloom_rs::bloom::{
        BloomFilter, BloomFilterConfigBuilder, BloomFilterOps,
        PersistenceConfigBuilder,
    };
    use tokio::runtime::Runtime;

    const CAPACITY: usize = 1_000_000;
    const EXTRA_INSERTS: usize = CAPACITY / 100; // 1% incremental writes
    const BATCH_SIZE: usize = 5_000;
    const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);
    const SEED: u64 = 42;

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    pub fn fjall_incremental_benchmark(c: &mut Criterion) {
        let mut group = c.benchmark_group("fjall_core_incremental");
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(15));

        let runtime = Runtime::new()
            .expect("failed to create Tokio runtime for benchmarks");

        let scenarios = [("fill_10", 100_000usize), ("fill_50", 500_000usize)];

        for (label, baseline_count) in scenarios {
            let baseline = generate_permutation_payloads(
                baseline_count,
                SEED + baseline_count as u64,
            );
            let incremental = generate_dirty_payloads(EXTRA_INSERTS);

            let expected =
                compute_expected_stats(&runtime, label, &baseline, &incremental);

            println!(
                "scenario={label} baseline_items={} incremental_items={} dirty_chunks={} total_chunks={} dirty_bytes={} seed={SEED}",
                baseline.len(),
                incremental.len(),
                expected.dirty_chunks,
                expected.total_chunks,
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

    struct SnapshotBenchContext {
        filter: BloomFilter,
        db_path: PathBuf,
    }

    struct SnapshotStats {
        dirty_chunks: usize,
        dirty_bytes: usize,
        total_chunks: usize,
    }

    fn setup_context(
        runtime: &Runtime,
        label: &str,
        baseline: &[Vec<u8>],
        incremental: &[Vec<u8>],
        iteration: u64,
    ) -> SnapshotBenchContext {
        let db_path = temp_db_path(label, iteration);
        let config = build_config(db_path.clone());
        let filter = runtime
            .block_on(async { BloomFilter::create(config).await })
            .expect("failed to create Bloom filter");

        insert_in_batches(&filter, baseline);

        runtime
            .block_on(filter.save_snapshot())
            .expect("initial snapshot failed");

        // Reload the filter to reset dirty tracking before measuring incremental writes
        drop(filter);
        let filter = runtime
            .block_on(async { BloomFilter::load(db_path.clone()).await })
            .expect("failed to reload Bloom filter for incremental phase");

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
        let config = build_config(db_path.clone());
        let filter = runtime
            .block_on(async { BloomFilter::create(config).await })
            .expect("failed to create Bloom filter for stats");

        insert_in_batches(&filter, baseline);
        runtime
            .block_on(filter.save_snapshot())
            .expect("initial snapshot failed during stats");

        drop(filter);
        let filter = runtime
            .block_on(async { BloomFilter::load(db_path.clone()).await })
            .expect("failed to reload Bloom filter for stats phase");

        insert_in_batches(&filter, incremental);

        let dirty_chunks = filter.extract_dirty_chunks();
        let dirty_bytes: usize =
            dirty_chunks.iter().map(|(_, bytes)| bytes.len()).sum();
        let chunk_size_bits = 4096 * 8;
        let total_chunks =
            (filter.bit_vector_size + chunk_size_bits - 1) / chunk_size_bits;
        let stats = SnapshotStats {
            dirty_chunks: dirty_chunks.len(),
            dirty_bytes,
            total_chunks,
        };

        drop(dirty_chunks);
        drop(filter);

        if let Err(err) = fs::remove_dir_all(&db_path) {
            eprintln!(
                "cleanup warning (expected stats, {}): {}",
                db_path.display(),
                err
            );
        }

        stats
    }

    fn build_config(
        db_path: PathBuf,
    ) -> expiring_bloom_rs::bloom::BloomFilterConfig {
        let persistence = PersistenceConfigBuilder::default()
            .db_path(db_path)
            .chunk_size_bytes(4096)
            .snapshot_interval(SNAPSHOT_INTERVAL)
            .auto_snapshot(false)
            .build()
            .expect("failed to build persistence config");

        BloomFilterConfigBuilder::default()
            .capacity(CAPACITY)
            .false_positive_rate(0.01)
            .persistence(Some(persistence))
            .build()
            .expect("failed to build bloom filter config")
    }

    fn insert_in_batches(filter: &BloomFilter, items: &[Vec<u8>]) {
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
        std::env::temp_dir().join(format!("fjall_core_{label}_{id}.fjall"))
    }

    criterion_group!(fjall_core_bench_group, fjall_incremental_benchmark);
}

#[cfg(feature = "fjall")]
use fjall_core_bench::fjall_core_bench_group;

#[cfg(feature = "fjall")]
criterion::criterion_main!(fjall_core_bench_group);
