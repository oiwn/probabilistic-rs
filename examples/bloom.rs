#![allow(clippy::uninlined_format_args)]
// mod common;
use expiring_bloom_rs::bloom::{
    BloomFilter, BloomFilterConfigBuilder, BloomFilterOps, BloomFilterStats,
    PersistenceConfigBuilder, SnapshotConfig,
};
use expiring_bloom_rs::common::bits2hr;
use std::collections::HashSet;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("🌸 Core Bloom Filter Example");
    println!("==============================");

    // Demonstrate basic usage
    basic_workflow_example().await?;

    // Demonstrate false positive rate measurement
    false_positive_rate_example().await?;

    // Demonstrate capacity limits
    capacity_limits_example().await?;

    // Demonstrate database persistence
    persistence_example().await?;

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

    let mut filter = BloomFilter::create(config).await?;

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

    let mut filter = BloomFilter::create(config).await?;

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

    let _ = BloomFilter::create(config).await?;

    println!("Testing with small capacity filter (100 items):");

    // Insert items up to and beyond capacity
    for fill_percentage in [25, 50, 75, 100, 150, 200] {
        let mut test_filter = BloomFilter::create(
            BloomFilterConfigBuilder::default()
                .capacity(100)
                .false_positive_rate(0.01)
                .build()?,
        )
        .await?;

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

async fn persistence_example() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    println!("\n🗄️  Database Persistence Example");
    println!("----------------------------------");

    // Ensure tmp directory exists
    fs::create_dir_all("tmp")?;
    let db_path = PathBuf::from("tmp/test_bloom_db.fjall");

    // Step 1: Create database (deleting previous if exists)
    println!("Step 1: Creating new database at {:?}", db_path);

    let persistence_config = PersistenceConfigBuilder::default()
        .db_path(db_path.clone())
        .chunk_size_bytes(1024) // 1KB chunks
        .snapshot_config(SnapshotConfig::default())
        .build()?;

    let original_config = BloomFilterConfigBuilder::default()
        .capacity(50000)
        .false_positive_rate(0.02) // 2% FPR
        .persistence(Some(persistence_config))
        .build()?;

    println!(
        "  Original config - capacity: {}, FPR: {:.3}%",
        original_config.capacity,
        original_config.false_positive_rate * 100.0
    );

    let mut filter = BloomFilter::create(original_config.clone()).await?;

    // Insert some test data
    let test_items = ["apple", "banana", "cherry", "date", "elderberry"];
    for item in &test_items {
        filter.insert(item.as_bytes()).await?;
    }
    println!("  Inserted {} test items", test_items.len());

    // Save snapshot (when implemented)
    println!("  Saving snapshot to database...");
    // filter.save_snapshot().await?;

    drop(filter); // Ensure everything is written

    // Step 2: Check database folder size and file count
    println!("\nStep 2: Analyzing database folder");
    if db_path.exists() {
        let metadata = fs::metadata(&db_path)?;
        if metadata.is_dir() {
            let entries: Vec<_> =
                fs::read_dir(&db_path)?.collect::<Result<Vec<_>, _>>()?;

            let total_size: u64 = entries
                .iter()
                .filter_map(|entry| entry.metadata().ok().map(|m| m.len()))
                .sum();

            println!("  Database folder: {:?}", db_path);
            println!("  File count: {}", entries.len());
            println!(
                "  Total size: {} bytes ({:.2} KB)",
                total_size,
                total_size as f64 / 1024.0
            );

            for entry in entries {
                let metadata = entry.metadata()?;
                println!(
                    "    - {}: {} bytes",
                    entry.file_name().to_string_lossy(),
                    metadata.len()
                );
            }
        }
    } else {
        println!(
            "  ⚠️  Database folder not found (expected for dummy implementation)"
        );
    }

    // Step 3: Load from database and verify config
    println!("\nStep 3: Loading from database and verifying config");

    let loaded_filter = BloomFilter::load(db_path.clone()).await?;
    let loaded_config = loaded_filter.config();

    println!(
        "  Loaded config - capacity: {}, FPR: {:.3}%",
        loaded_config.capacity,
        loaded_config.false_positive_rate * 100.0
    );

    // Verify parameters match
    let capacity_match = original_config.capacity == loaded_config.capacity;
    let fpr_match = (original_config.false_positive_rate
        - loaded_config.false_positive_rate)
        .abs()
        < f64::EPSILON;

    println!("  Config verification:");
    println!(
        "    Capacity match: {} ({} == {})",
        if capacity_match { "✅" } else { "❌" },
        original_config.capacity,
        loaded_config.capacity
    );
    println!(
        "    FPR match: {} ({:.4} == {:.4})",
        if fpr_match { "✅" } else { "❌" },
        original_config.false_positive_rate,
        loaded_config.false_positive_rate
    );

    if capacity_match && fpr_match {
        println!("  🎉 All parameters match successfully!");
    } else {
        println!("  ❌ Parameter mismatch detected!");
    }

    // Test create_or_load behavior
    println!("\nStep 4: Testing create_or_load behavior");

    // Should load existing
    println!("  Testing create_or_load with existing DB...");
    let reloaded_filter =
        BloomFilter::create_or_load(original_config.clone()).await?;
    println!(
        "    Loaded existing - capacity: {}",
        reloaded_filter.capacity()
    );

    // Remove DB and test creation
    if db_path.exists() {
        fs::remove_dir_all(&db_path)?;
        println!("  Removed database for create test");
    }

    println!("  Testing create_or_load with missing DB...");
    let new_filter = BloomFilter::create_or_load(original_config.clone()).await?;
    println!("    Created new - capacity: {}", new_filter.capacity());

    println!("\n✅ Persistence example completed!");
    Ok(())
}
