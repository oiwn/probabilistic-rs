use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use probabilistic_rs::bloom::config::BloomFilterConfigBuilder;
use probabilistic_rs::bloom::filter::BloomFilter;
use probabilistic_rs::bloom::traits::BloomFilterOps;
use rand::{Rng, distr::Alphanumeric};

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

fn create_test_filter(capacity: usize) -> BloomFilter {
    let config = BloomFilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(0.01)
        .build()
        .expect("Failed to create config");

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(BloomFilter::create(config))
        .expect("Failed to create Bloom filter")
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_operations");

    for capacity in [1000, 100_000, 1_000_000] {
        let test_data = generate_test_data(capacity);

        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &test_data),
            |b, (cap, data)| {
                b.iter_batched(
                    || create_test_filter(*cap),
                    |filter: BloomFilter| {
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
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_operations");

    for capacity in [1_000, 100_000, 1_000_000] {
        let known_data = generate_test_data((capacity as f64 * 0.8) as usize);
        let unknown_data = generate_test_data((capacity as f64 * 0.2) as usize);

        group.bench_with_input(
            BenchmarkId::new("inmemory", capacity),
            &(capacity, &known_data, &unknown_data),
            |b, (cap, known, unknown)| {
                let filter = create_test_filter(*cap);

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
    group.finish();
}

criterion_group!(benches, bench_insert, bench_query);
criterion_main!(benches);
