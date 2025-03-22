mod common;

use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, FilterStorage, InMemoryFilter,
    common::format_memory_usage, optimal_bit_vector_size, optimal_num_hashes,
};
use rand::seq::IndexedRandom;
use std::{collections::HashSet, time::Duration};
use tokio::io::AsyncWriteExt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration
    let capacity = 1_000_000;
    let false_positive_rate = 0.01;
    let max_levels = 3;
    let level_duration = Duration::from_secs(60);
    let fill_percentage = 99.0;

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

    inserted_items = populate_filter_with_realistic_distribution(
        &mut filter,
        insert_count,
        0.5,
    )?;

    // for _ in 0..insert_count {
    //     let item = common::generate_random_string(32);
    //     filter.insert(item.as_bytes())?;
    //     inserted_items.insert(item);
    // }

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

// Modified function to return all inserted items
fn populate_filter_with_realistic_distribution(
    filter: &mut InMemoryFilter,
    total_items: usize,
    overlap_factor: f64,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let num_levels = filter.config.max_levels;
    let items_per_level = total_items / num_levels;
    let mut inserted_items = HashSet::with_capacity(total_items);

    println!("Populating filter with realistic time-based distribution:");
    println!("  • Total items: {}", total_items);
    println!("  • Items per level: ~{}", items_per_level);
    println!("  • Overlap factor: {:.1}%", overlap_factor * 100.0);

    // Generate all test data first
    let all_test_data = common::generate_test_data(total_items);

    // Create distribution with controlled overlap between levels
    let mut current_idx = 0;

    for level in 0..num_levels {
        println!("\nFilling level {}...", level);

        // Calculate how many unique vs overlapped items for this level
        let overlap_items = if level > 0 {
            (items_per_level as f64 * overlap_factor).round() as usize
        } else {
            0
        };

        let unique_items = items_per_level - overlap_items;

        // Insert unique items for this level
        for i in 0..unique_items {
            if current_idx >= all_test_data.len() {
                break;
            }

            // Directly insert into the specific level (bypass normal insert)
            let item = &all_test_data[current_idx];
            insert_directly_into_level(filter, level, item.as_bytes())?;
            inserted_items.insert(item.clone()); // Track in our HashSet
            current_idx += 1;

            if i % (unique_items / 10).max(1) == 0 {
                print!("{}%... ", (i * 100) / unique_items);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
        }

        // Add overlap from previous level if needed
        if level > 0 && overlap_items > 0 {
            let mut rng = rand::rng();
            // Get items from previous level to reuse
            let prev_level_start = current_idx - unique_items - items_per_level;
            let prev_level_end = prev_level_start + items_per_level;
            let prev_level_range = prev_level_start..prev_level_end;

            // Randomly choose items from previous level
            let overlap_indices: Vec<_> = prev_level_range
                .collect::<Vec<_>>()
                .choose_multiple(&mut rng, overlap_items)
                .cloned()
                .collect();

            println!(
                "\nAdding {} overlapping items from previous level",
                overlap_items
            );

            for idx in overlap_indices {
                if idx < all_test_data.len() {
                    let item = &all_test_data[idx];
                    insert_directly_into_level(filter, level, item.as_bytes())?;
                    // Note: No need to add to inserted_items again since these are already tracked
                }
            }
        }

        // Set timestamp for this level to simulate time passage
        let now = std::time::SystemTime::now();
        let level_age =
            filter.config.level_duration * (num_levels - 1 - level) as u32 / 2;
        let level_time = now - level_age;
        filter.storage.set_timestamp(level, level_time)?;

        println!(
            "100% - Level {} complete with {}ms age",
            level,
            level_age.as_millis()
        );
    }

    println!(
        "\nFilter populated with {} unique items across {} levels",
        inserted_items.len(),
        num_levels
    );
    Ok(inserted_items)
}

// Function to distribute elements across levels with controlled overlap
fn populate_filter_with_realistic_distribution_old(
    filter: &mut InMemoryFilter,
    total_items: usize,
    overlap_factor: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    let num_levels = filter.config.max_levels;
    let items_per_level = total_items / num_levels;

    println!("Populating filter with realistic time-based distribution:");
    println!("  • Total items: {}", total_items);
    println!("  • Items per level: ~{}", items_per_level);
    println!("  • Overlap factor: {:.1}%", overlap_factor * 100.0);

    // Generate all test data first
    let all_test_data = common::generate_test_data(total_items);

    // Create distribution with controlled overlap between levels
    let mut current_idx = 0;

    for level in 0..num_levels {
        println!("\nFilling level {}...", level);

        // Calculate how many unique vs overlapped items for this level
        let overlap_items = if level > 0 {
            (items_per_level as f64 * overlap_factor).round() as usize
        } else {
            0
        };

        let unique_items = items_per_level - overlap_items;

        // Insert unique items for this level
        for i in 0..unique_items {
            if current_idx >= all_test_data.len() {
                break;
            }

            // Directly insert into the specific level (bypass normal insert)
            let item = &all_test_data[current_idx];
            insert_directly_into_level(filter, level, item.as_bytes())?;
            current_idx += 1;

            if i % (unique_items / 10).max(1) == 0 {
                print!("{}%... ", (i * 100) / unique_items);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
        }

        // Add overlap from previous level if needed
        if level > 0 && overlap_items > 0 {
            let mut rng = rand::rng();
            // Get items from previous level to reuse
            let prev_level_start = current_idx - unique_items - items_per_level;
            let prev_level_end = prev_level_start + items_per_level;
            let prev_level_range = prev_level_start..prev_level_end;

            // Randomly choose items from previous level
            let overlap_indices: Vec<_> = prev_level_range
                .collect::<Vec<_>>()
                .choose_multiple(&mut rng, overlap_items)
                .cloned()
                .collect();

            println!(
                "\nAdding {} overlapping items from previous level",
                overlap_items
            );

            for idx in overlap_indices {
                if idx < all_test_data.len() {
                    let item = &all_test_data[idx];
                    insert_directly_into_level(filter, level, item.as_bytes())?;
                }
            }
        }

        // Set timestamp for this level to simulate time passage
        let now = std::time::SystemTime::now();
        let level_age =
            filter.config.level_duration * (num_levels - 1 - level) as u32 / 2;
        let level_time = now - level_age;
        filter.storage.set_timestamp(level, level_time)?;

        println!(
            "100% - Level {} complete with {}ms age",
            level,
            level_age.as_millis()
        );
    }

    println!(
        "\nFilter populated with realistic distribution across {} levels",
        num_levels
    );
    Ok(())
}

// Helper function to insert directly into a specific level
fn insert_directly_into_level(
    filter: &mut InMemoryFilter,
    level: usize,
    item: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let indices: Vec<usize> = (filter.config.hash_function)(
        item,
        filter.num_hashes,
        filter.config.capacity,
    )
    .into_iter()
    .map(|h| h as usize)
    .collect();

    filter.storage.set_bits(level, &indices)?;
    Ok(())
}
