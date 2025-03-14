use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use expiring_bloom_rs::{FilterStorage, InMemoryStorage};
use rand::{Rng, distr::Uniform};
use std::collections::HashSet;

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
    for &capacity in &[100_000, 1_000_000] {
        // Test different numbers of bits to set
        for &bit_count in &[10, 1000] {
            // Skip configurations that don't make sense
            if bit_count > capacity {
                continue;
            }

            group.bench_with_input(
                BenchmarkId::new(format!("capacity_{}", capacity), bit_count),
                &(capacity, bit_count),
                |b, &(cap, bits)| {
                    b.iter_batched(
                        || {
                            // Setup: Create storage and random indices
                            let storage = InMemoryStorage::new(cap, 3).unwrap();
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

    for &capacity in &[100_000, 1_000_000] {
        for &bit_count in &[100, 1000] {
            if bit_count > capacity {
                continue;
            }

            group.bench_with_input(
                BenchmarkId::new(format!("capacity_{}", capacity), bit_count),
                &(capacity, bit_count),
                |b, &(cap, bits)| {
                    // Setup: Create storage, set some bits, then measure get performance
                    let mut storage = InMemoryStorage::new(cap, 3).unwrap();
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

criterion_group!(benches, bench_set_bits, bench_get_bits,);
criterion_main!(benches);
