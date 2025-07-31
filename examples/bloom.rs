#![allow(clippy::uninlined_format_args)]
// mod common;
use expiring_bloom_rs::bloom::{
    BloomFilter, BloomFilterConfigBuilder, BloomFilterOps, BloomFilterStats,
};
use expiring_bloom_rs::common::bits2hr;
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌸 Bloom Filter Basic Example");
    println!("==============================");

    // Demonstrate basic usage
    basic_workflow_example().await?;

    // Demonstrate false positive rate measurement
    false_positive_rate_example().await?;

    // Demonstrate capacity limits
    capacity_limits_example().await?;

    Ok(())
}

async fn basic_workflow_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📝 Basic Workflow Example");
    println!("---------------------------");

    // Create a bloom filter configuration
    let config = BloomFilterConfigBuilder::default()
        .capacity(1_000_000)
        .false_positive_rate(0.01) // 1%
        .build()?;

    let mut filter = BloomFilter::new(config)?;

    println!("Created bloom filter:");
    println!("  Capacity: {}", filter.capacity());
    println!("  Target FPR: {:.2}%", filter.false_positive_rate() * 100.0);
    println!("  Bit vector size: {} bits", filter.bit_vector_size);
    println!(
        "  Bit vector size in memory: {}",
        filter.approx_memory_bits()
    );
    println!("  Bits per item: {:.2}", filter.bits_per_item());
    println!("  Bit vector size: {}", bits2hr(filter.bit_vector_size));
    println!("  Hash functions: {}", filter.num_hashes);

    // Insert some items
    let items = vec!["apple", "banana", "cherry", "date", "elderberry"];

    for item in &items {
        filter.insert(item.as_bytes()).await?;
        println!("  ✅ Inserted: {}", item);
    }

    println!("\nQuerying items:");
    // Query the items we inserted
    for item in &items {
        let exists = filter.contains(item.as_bytes()).await?;
        println!("  {} exists: {}", item, if exists { "✅" } else { "❌" });
    }

    // Query items we didn't insert
    let test_items = vec!["grape", "kiwi", "mango"];
    for item in &test_items {
        let exists = filter.contains(item.as_bytes()).await?;
        println!(
            "  {} exists: {}",
            item,
            if exists {
                "🟡 (false positive?)"
            } else {
                "❌"
            }
        );
    }

    println!("  Inserted elements inserted: {}", filter.insert_count());

    Ok(())
}

async fn false_positive_rate_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📊 False Positive Rate Measurement");
    println!("-----------------------------------");

    let config = BloomFilterConfigBuilder::default()
        .capacity(1_000)
        .false_positive_rate(0.05) // 5%
        .build()?;

    let mut filter = BloomFilter::new(config)?;

    // Insert 500 known items (50% capacity)
    let mut inserted_items = HashSet::new();
    for i in 0..500 {
        let item = format!("item_{:04}", i);
        filter.insert(item.as_bytes()).await?;
        inserted_items.insert(item);
    }

    println!("Inserted {} items into filter", inserted_items.len());

    // Test for false positives with 1000 random items
    let mut false_positives = 0;
    let test_count = 1000;

    for i in 1000..1000 + test_count {
        let test_item = format!("test_{:04}", i);
        if filter.contains(test_item.as_bytes()).await? {
            // Make sure this isn't actually a known item
            if !inserted_items.contains(&test_item) {
                false_positives += 1;
            }
        }
    }

    let measured_fpr = false_positives as f64 / test_count as f64;
    let target_fpr = filter.false_positive_rate();

    println!("False positive rate analysis:");
    println!(
        "  Target FPR: {:.4}% ({:.4})",
        target_fpr * 100.0,
        target_fpr
    );
    println!(
        "  Measured FPR: {:.4}% ({:.4})",
        measured_fpr * 100.0,
        measured_fpr
    );
    println!(
        "  Ratio (measured/target): {:.2}x",
        measured_fpr / target_fpr
    );
    println!(
        "  False positives found: {}/{}",
        false_positives, test_count
    );

    Ok(())
}

async fn capacity_limits_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n⚠️  Capacity Limits & Performance");
    println!("----------------------------------");

    let config = BloomFilterConfigBuilder::default()
        .capacity(100) // Small capacity for demonstration
        .false_positive_rate(0.01)
        .build()?;

    let _ = BloomFilter::new(config)?;

    println!("Testing with small capacity filter (100 items):");

    // Insert items up to and beyond capacity
    for fill_percentage in [25, 50, 75, 100, 150, 200] {
        let mut test_filter = BloomFilter::new(
            BloomFilterConfigBuilder::default()
                .capacity(100)
                .false_positive_rate(0.01)
                .build()?,
        )?;

        let items_to_insert = fill_percentage;

        // Insert items
        for i in 0..items_to_insert {
            let item = format!("load_test_{:03}", i);
            test_filter.insert(item.as_bytes()).await?;
        }

        // Measure false positive rate
        let mut fps = 0;
        let tests = 100;
        for i in 1000..(1000 + tests) {
            let test_item = format!("fp_test_{:03}", i);
            if test_filter.contains(test_item.as_bytes()).await? {
                fps += 1;
            }
        }

        let measured_fpr = fps as f64 / tests as f64;
        println!(
            "  {}% fill ({} items): FPR = {:.4}%",
            fill_percentage,
            items_to_insert,
            measured_fpr * 100.0
        );
    }

    println!(
        "\n💡 Observation: FPR increases significantly when exceeding capacity!"
    );

    Ok(())
}
