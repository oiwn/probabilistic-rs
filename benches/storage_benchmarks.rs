use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use probablistic_rs::{FilterStorage, InMemoryStorage};
use rand::{Rng, distr::Uniform};
use std::collections::HashSet;
use std::sync::{Arc, Barrier, RwLock};
use std::thread;

// Helper function to generate a set of random indices within capacity
fn generate_random_indices(count: usize, capacity: usize) -> Vec<usize> {
    let mut rng = rand::rng();
    let range = Uniform::new(0, capacity).unwrap();

    // Use HashSet to ensure uniqueness
    let mut indices_set = HashSet::with_capacity(count);
    while indices_set.len() < count {
        indices_set.insert(rng.sample(range));
    }

    indices_set.into_iter().collect()
}

fn bench_set_bits(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_set_bits");

    // Test different storage capacities
    for &capacity in &[1_000_000] {
        // Test different numbers of bits to set
        for &bit_count in &[100, 10000] {
            group.bench_with_input(
                BenchmarkId::new(format!("capacity_{capacity}"), bit_count),
                &(capacity, bit_count),
                |b, &(cap, bits)| {
                    b.iter_batched(
                        || {
                            // Setup: Create storage and random indices
                            let storage = InMemoryStorage::new(cap, 1).unwrap();
                            let indices = generate_random_indices(bits, cap);
                            (storage, indices)
                        },
                        |(mut storage, indices)| {
                            // Benchmark: Set bits operation
                            storage.set_bits(0, &indices).unwrap();
                        },
                        criterion::BatchSize::SmallInput,
                    )
                },
            );
        }
    }

    group.finish();
}

fn bench_get_bits(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_get_bits");

    for &capacity in &[1_000_000] {
        for &bit_count in &[1000, 10000] {
            group.bench_with_input(
                BenchmarkId::new(format!("capacity_{capacity}"), bit_count),
                &(capacity, bit_count),
                |b, &(cap, bits)| {
                    // Setup: Create storage, set some bits, then measure get performance
                    let mut storage = InMemoryStorage::new(cap, 1).unwrap();
                    let indices = generate_random_indices(bits, cap);

                    // Set the bits first
                    storage.set_bits(0, &indices).unwrap();

                    b.iter(|| {
                        storage.get_bits(0, &indices).unwrap();
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_concurrent_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_concurrent_access");
    group.sample_size(10); // Reduce sample size for complex threading benchmarks

    // Test parameters
    const NUM_THREADS: usize = 8;
    const OPERATIONS_PER_THREAD: usize = 1000;
    const NUM_LEVELS: usize = 5;

    // Test different storage capacities
    for &capacity in &[10_000, 100_000] {
        group.bench_with_input(
            BenchmarkId::new("concurrent_rw", capacity),
            &capacity,
            |b, &cap| {
                b.iter_batched(
                    || {
                        // Setup: Create storage wrapped in Arc+RwLock for thread sharing
                        let storage = Arc::new(RwLock::new(
                            InMemoryStorage::new(cap, NUM_LEVELS).unwrap(),
                        ));

                        // Pre-populate with some data
                        let indices = generate_random_indices(cap / 10, cap);
                        for level in 0..NUM_LEVELS {
                            storage
                                .write()
                                .unwrap()
                                .set_bits(level, &indices)
                                .unwrap();
                        }

                        storage
                    },
                    |storage| {
                        // Create a barrier to synchronize thread starts
                        let barrier = Arc::new(Barrier::new(NUM_THREADS + 1));

                        // Spawn threads with different operation patterns
                        let handles: Vec<_> = (0..NUM_THREADS)
                            .map(|id| {
                                let storage = storage.clone();
                                let barrier = barrier.clone();

                                thread::spawn(move || {
                                    // Wait for all threads to be ready
                                    barrier.wait();

                                    // Determine thread role
                                    match id % 3 {
                                        0 => {
                                            // Writer threads - write to current level
                                            for _ in 0..OPERATIONS_PER_THREAD {
                                                let indices =
                                                    generate_random_indices(
                                                        10, cap,
                                                    );
                                                let _ = storage
                                                    .write()
                                                    .unwrap()
                                                    .set_bits(0, &indices);
                                            }
                                        }
                                        1 => {
                                            // Reader threads - read from random levels
                                            let mut rng = rand::rng();
                                            for _ in 0..OPERATIONS_PER_THREAD {
                                                let level = rng
                                                    .random_range(0..NUM_LEVELS);
                                                let indices =
                                                    generate_random_indices(
                                                        10, cap,
                                                    );
                                                let _ = storage
                                                    .read()
                                                    .unwrap()
                                                    .get_bits(level, &indices);
                                            }
                                        }
                                        _ => {
                                            // Mixed operation threads - both read and clear
                                            let mut rng = rand::rng();
                                            for op_num in 0..OPERATIONS_PER_THREAD
                                            {
                                                if op_num % 10 == 0 {
                                                    // Occasionally clear a random old level
                                                    let level = rng.random_range(
                                                        1..NUM_LEVELS,
                                                    );
                                                    let _ = storage
                                                        .write()
                                                        .unwrap()
                                                        .clear_level(level);
                                                } else {
                                                    // Mostly read operations
                                                    let indices =
                                                        generate_random_indices(
                                                            5, cap,
                                                        );
                                                    for level in 0..NUM_LEVELS {
                                                        let _ = storage
                                                            .read()
                                                            .unwrap()
                                                            .get_bits(
                                                                level, &indices,
                                                            );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                })
                            })
                            .collect();

                        // Start all threads simultaneously
                        barrier.wait();

                        // Wait for all threads to finish
                        for handle in handles {
                            handle.join().unwrap();
                        }
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_set_bits,
    bench_get_bits,
    bench_concurrent_access
);
criterion_main!(benches);
