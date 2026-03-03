use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use probabilistic_rs::ebloom::config::ExpiringFilterConfigBuilder;
use probabilistic_rs::ebloom::filter::ExpiringBloomFilter;
use probabilistic_rs::ebloom::traits::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilterOps,
};
use rand::{Rng, distr::Alphanumeric};
use std::time::Duration;

fn generate_random_string(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn generate_test_data(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_random_string(32)).collect()
}

fn create_test_filter(capacity: usize, num_levels: usize) -> ExpiringBloomFilter {
    let config = ExpiringFilterConfigBuilder::default()
        .capacity_per_level(capacity)
        .target_fpr(0.01)
        .level_duration(Duration::from_secs(3600))
        .num_levels(num_levels)
        .build()
        .expect("Failed to create config");

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(ExpiringBloomFilter::create(config))
        .expect("Failed to create ExpiringBloom filter")
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("ebloom_insert_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        for num_levels in [3, 5] {
            let test_data = generate_test_data(capacity);

            group.bench_with_input(
                BenchmarkId::new(
                    format!("inmemory_levels_{num_levels}"),
                    capacity,
                ),
                &(capacity, num_levels, &test_data),
                |b, (cap, levels, data)| {
                    b.iter_batched(
                        || create_test_filter(*cap, *levels),
                        |filter: ExpiringBloomFilter| {
                            for item in data.iter() {
                                if let Err(e) = filter.insert(item.as_bytes()) {
                                    eprintln!("Insert error (continuing): {e}");
                                    continue;
                                }
                            }
                        },
                        criterion::BatchSize::SmallInput,
                    )
                },
            );
        }
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("ebloom_query_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        for num_levels in [3, 5] {
            let known_data = generate_test_data((capacity as f64 * 0.8) as usize);
            let unknown_data =
                generate_test_data((capacity as f64 * 0.2) as usize);

            group.bench_with_input(
                BenchmarkId::new(
                    format!("inmemory_levels_{num_levels}"),
                    capacity,
                ),
                &(capacity, num_levels, &known_data, &unknown_data),
                |b, (cap, levels, known, unknown)| {
                    let filter = create_test_filter(*cap, *levels);

                    for item in known.iter() {
                        filter.insert(item.as_bytes()).unwrap();
                    }

                    b.iter(|| {
                        for item in known.iter() {
                            filter.contains(item.as_bytes()).unwrap();
                        }
                        for item in unknown.iter() {
                            filter.contains(item.as_bytes()).unwrap();
                        }
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_insert_bulk(c: &mut Criterion) {
    let mut group = c.benchmark_group("ebloom_insert_bulk_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        for num_levels in [3, 5] {
            let test_data: Vec<Vec<u8>> = generate_test_data(capacity)
                .into_iter()
                .map(|s| s.into_bytes())
                .collect();
            let test_refs: Vec<&[u8]> =
                test_data.iter().map(|v| v.as_slice()).collect();

            group.bench_with_input(
                BenchmarkId::new(
                    format!("inmemory_levels_{num_levels}"),
                    capacity,
                ),
                &(capacity, num_levels, &test_refs),
                |b, (cap, levels, data)| {
                    b.iter_batched(
                        || create_test_filter(*cap, *levels),
                        |filter: ExpiringBloomFilter| {
                            if let Err(e) = filter.insert_bulk(data) {
                                eprintln!("Bulk insert error (continuing): {e}");
                            }
                        },
                        criterion::BatchSize::SmallInput,
                    )
                },
            );
        }
    }
    group.finish();
}

fn bench_contains_bulk(c: &mut Criterion) {
    let mut group = c.benchmark_group("ebloom_contains_bulk_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        for num_levels in [3, 5] {
            let known_data: Vec<Vec<u8>> =
                generate_test_data((capacity as f64 * 0.8) as usize)
                    .into_iter()
                    .map(|s| s.into_bytes())
                    .collect();
            let unknown_data: Vec<Vec<u8>> =
                generate_test_data((capacity as f64 * 0.2) as usize)
                    .into_iter()
                    .map(|s| s.into_bytes())
                    .collect();

            let known_refs: Vec<&[u8]> =
                known_data.iter().map(|v| v.as_slice()).collect();
            let unknown_refs: Vec<&[u8]> =
                unknown_data.iter().map(|v| v.as_slice()).collect();

            group.bench_with_input(
                BenchmarkId::new(
                    format!("inmemory_levels_{num_levels}"),
                    capacity,
                ),
                &(capacity, num_levels, &known_refs, &unknown_refs),
                |b, (cap, levels, known, unknown)| {
                    let filter = create_test_filter(*cap, *levels);

                    filter.insert_bulk(known).unwrap();

                    let all_refs: Vec<&[u8]> =
                        known.iter().chain(unknown.iter()).copied().collect();

                    b.iter(|| {
                        filter.contains_bulk(&all_refs).unwrap();
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_rotate_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("ebloom_rotate_operations");
    group.sample_size(20);

    for capacity in [1_000, 100_000] {
        for num_levels in [3, 5] {
            let test_data = generate_test_data(capacity / num_levels);

            group.bench_with_input(
                BenchmarkId::new(format!("levels_{num_levels}"), capacity),
                &(capacity, num_levels, &test_data),
                |b, (cap, levels, data)| {
                    b.iter_batched(
                        || {
                            let filter = create_test_filter(*cap, *levels);
                            for item in data.iter() {
                                filter.insert(item.as_bytes()).unwrap();
                            }
                            filter
                        },
                        |filter: ExpiringBloomFilter| {
                            tokio::runtime::Runtime::new()
                                .unwrap()
                                .block_on(filter.rotate_levels())
                                .expect("rotate failed");
                        },
                        criterion::BatchSize::SmallInput,
                    )
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_query,
    bench_insert_bulk,
    bench_contains_bulk,
    bench_rotate_levels,
);
criterion_main!(benches);
