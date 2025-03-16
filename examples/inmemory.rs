mod common;

use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, InMemoryFilter,
    common::format_memory_usage, optimal_bit_vector_size, optimal_num_hashes,
};
use std::{collections::HashSet, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let capacity = 1_000_000;
    let false_positive_rate = 0.01;
    let max_levels = 2;
    let level_duration = Duration::from_secs(60);
    let fill_percentage = 10.0;

    // Print theoretical optimal parameters
    let optimal_size = optimal_bit_vector_size(capacity, false_positive_rate);
    let optimal_hashes = optimal_num_hashes(capacity, optimal_size);

    println!("Bloom Filter Configuration:");
    println!(
        "  Capacity: {}, Max Levels: {} Level Duration: {:?}",
        capacity, max_levels, level_duration
    );
    println!(
        "  Target False Positive Rate: {:.4}%",
        false_positive_rate * 100.0
    );
    println!(
        "  Theoretical Optimal Bit Vector Size: {} bits",
        optimal_size
    );
    println!("  Theoretical Optimal Hash Functions: {}", optimal_hashes);

    // Create the filter
    let config = FilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(false_positive_rate)
        .level_duration(level_duration)
        .max_levels(max_levels)
        .build()?;

    let mut filter = InMemoryFilter::new(config)?;

    // Print actual values used in the filter
    println!("\nActual Bloom Filter Parameters:");
    println!(
        "  Bit Vector Size: {} bits",
        filter.storage.bit_vector_len()
    );
    println!("  Hash Functions: {}", filter.num_hashes());
    println!(
        "  Bits per Element Ratio: {:.2}",
        filter.storage.bit_vector_len() as f64 / capacity as f64
    );

    // Simulation parameters
    let insert_count = (capacity as f64 * fill_percentage / 100.0) as usize;

    println!("\nStarting insertion of elements:");
    println!(
        "  {} total inserts ({:.1}% of capacity)",
        insert_count, fill_percentage
    );

    // Track all inserted items for false positive testing
    let mut inserted_items = HashSet::with_capacity(insert_count);
    let start_time = std::time::Instant::now();

    for _ in 0..insert_count {
        let item = common::generate_random_string(32);
        filter.insert(item.as_bytes())?;
        inserted_items.insert(item);
    }

    let insert_duration = start_time.elapsed();
    println!("\nInsertion complete in {:.2?}", insert_duration);
    println!(
        "Average insertion rate: {:.2} items/second",
        insert_count as f64 / insert_duration.as_secs_f64()
    );

    // Test for false positives
    let mut false_positives = 0;
    let test_count = 100_000;

    println!(
        "\nTesting for false positives with {} unknown items...",
        test_count
    );
    let test_start = std::time::Instant::now();

    let inserted_set = inserted_items.clone();
    drop(inserted_items);

    let mut tested_count = 0;
    while tested_count < test_count {
        let test_item = common::generate_random_string(32);

        // Skip if we accidentally generated a known item
        if inserted_set.contains(&test_item) {
            continue;
        }

        if filter.query(test_item.as_bytes()).unwrap() {
            false_positives += 1;
        }

        tested_count += 1;
    }

    let test_duration = test_start.elapsed();
    let measured_fpr = false_positives as f64 / test_count as f64;

    println!("False positive test complete in {:.2?}", test_duration);
    println!(
        "Found {} false positives out of {} tests",
        false_positives, test_count
    );
    println!("Measured false positive rate: {:.4}%", measured_fpr * 100.0);
    println!(
        "Target false positive rate: {:.4}%",
        false_positive_rate * 100.0
    );
    println!(
        "Ratio (measured/target): {:.2}x",
        measured_fpr / false_positive_rate
    );

    // Print bitvector details - analyze bit distribution
    let bit_density = calculate_bit_density(&filter);

    println!("\nBit Vector Analysis:");
    for level in 0..filter.config.max_levels {
        println!(
            "  Level {}: {:.2}% of bits set ({} bits)",
            level,
            bit_density[level] * 100.0,
            (filter.config.capacity as f64 * bit_density[level]) as usize
        );
    }

    println!("\nMemory Usage Analysis:");
    println!(
        "  Total memory used by storage: {}",
        format_memory_usage(filter.storage.approx_memory_usage())
    );
    println!(
        "  Memory per level: {:.2} KB",
        filter.storage.approx_memory_usage() as f64
            / (filter.config.max_levels as f64 * 1024.0)
    );
    println!(
        "  Memory per element: {:.2} bytes/element",
        filter.storage.approx_memory_usage() as f64 / insert_count as f64
    );
    println!(
        "  Theoretical optimal memory: {:.2} bytes/element (based on optimal bit vector size)",
        optimal_size as f64 / 8.0 / capacity as f64
    );

    Ok(())
}

// Helper function to calculate bit density per level
fn calculate_bit_density(filter: &InMemoryFilter) -> Vec<f64> {
    let mut densities = Vec::with_capacity(filter.config.max_levels);

    for level in 0..filter.config.max_levels {
        let level_bits = &filter.storage.levels.read().unwrap()[level];
        let set_bits = level_bits.iter().filter(|bit| **bit).count();
        // let set_bits = level_bits.iter().filter(|&bit| bit).count();
        let density = set_bits as f64 / level_bits.len() as f64;
        densities.push(density);
    }

    densities
}
