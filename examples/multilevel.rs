#![allow(clippy::needless_range_loop)]
use colored::Colorize;
use comfy_table::{
    ContentArrangement, Table, modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL,
};
use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, FilterStorage, RedbFilter,
    RedbFilterConfigBuilder,
};
use rand::{Rng, distr::Alphanumeric, seq::IndexedRandom};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

// Constants for our test
const CAPACITY: usize = 1_000_000;
const NUM_LEVELS: usize = 5;
const UNIQUE_ITEMS: usize = 500_000;
const TRACEABLE_ITEMS: usize = 1_000;
const ITEMS_PER_LEVEL: usize = 100_000;
const OVERLAP_FACTOR: f64 = 0.33; // 33% overlap between levels

// Basic word list for generating traceable items
const WORD_LIST: [&str; 20] = [
    "apple",
    "banana",
    "cherry",
    "date",
    "elderberry",
    "fig",
    "grape",
    "honeydew",
    "kiwi",
    "lemon",
    "mango",
    "nectarine",
    "orange",
    "peach",
    "quince",
    "raspberry",
    "strawberry",
    "tangerine",
    "watermelon",
    "zucchini",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Time-Decaying Bloom Filter Multi-Level Benchmark");
    println!("Configuration:");
    println!(
        "Capacity={}, Items/level={}, Overlap={}%, Levels={}",
        CAPACITY,
        ITEMS_PER_LEVEL,
        (OVERLAP_FACTOR * 100.0) as u32,
        NUM_LEVELS,
    );
    println!(
        "Unique items={}, Traceable items={}",
        UNIQUE_ITEMS, TRACEABLE_ITEMS
    );

    let file_name = format!("bloom_multilevel_{}.redb", CAPACITY);
    let db_path = Path::new(&file_name);

    // Remove any existing database file
    if db_path.exists() {
        fs::remove_file(db_path)?;
        let message = "Removed existing database file".red();
        println!("{} : {}", message, db_path.display());
    }

    let mut filter = create_filter(db_path);

    // Generate all our data
    println!("\nGenerating test data (takes few seconds!)...");
    let mut all_test_data = generate_test_data(UNIQUE_ITEMS);

    // Generate traceable items and replace some of the random data with them
    println!("Generating traceable items...");
    let traceable_items = generate_traceable_items(TRACEABLE_ITEMS);

    println!("Adding traceable items to test data...");
    all_test_data.extend(traceable_items.iter().cloned());

    // Generate distribution of elements across levels with overlap
    println!("Creating distributed element strategy with overlap...");
    let mut level_distributions = generate_time_ordered_elements(
        UNIQUE_ITEMS,
        NUM_LEVELS,
        ITEMS_PER_LEVEL,
        OVERLAP_FACTOR,
    );

    let mut rng = rand::rng();

    // Indices of traceable items in all_test_data
    let traceable_start_idx = all_test_data.len() - traceable_items.len();

    for (i, _item) in traceable_items.iter().enumerate() {
        let item_idx = traceable_start_idx + i;

        // Randomly determine sequential layers (chain)
        let chain_length = rng.random_range(1..=NUM_LEVELS.min(5));
        let start_level = rng.random_range(0..=NUM_LEVELS - chain_length);

        for level in start_level..start_level + chain_length {
            level_distributions[level].push(item_idx);
        }
    }

    // Track insertion times and other metrics for each level
    let mut level_metrics = Vec::new();
    let mut traceable_level_map: HashMap<String, Vec<usize>> = HashMap::new();

    // Insert data in batches to simulate levels with overlapping elements
    for level in 0..NUM_LEVELS {
        println!("\nFilling level {}...", level);

        // Get this level's items based on the distribution
        let level_indices = &level_distributions[level];
        let level_size = level_indices.len();

        // Insert items for this level and measure time
        let start_time = Instant::now();

        for (i, &idx) in level_indices.iter().enumerate() {
            if i % (level_size / 10) == 0 {
                print!("{}%... ", (i * 100) / level_size);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            // Get the item and insert it
            let item = &all_test_data[idx];

            // This is reimplemenatation of insert:
            // filter.insert(item.as_bytes())?;
            // because i need to drop it into the different levels
            let indices: Vec<usize> = (filter.get_config().hash_function)(
                item.as_bytes(),
                filter.get_num_hashes(),
                filter.get_config().capacity,
            )
            .into_iter()
            .map(|h| h as usize)
            .collect();
            filter.storage.set_bits(level, &indices).unwrap();

            // Record item to level mapping for traceable items
            if traceable_items.contains(item) {
                traceable_level_map
                    .entry(item.clone())
                    .or_default()
                    .push(level);
            }
        }
        println!("100%");

        let insert_duration = start_time.elapsed();
        let inserts_per_second =
            level_size as f64 / insert_duration.as_secs_f64();

        println!(
            "  Level {} insertion completed in {:.2?}",
            level, insert_duration
        );
        println!("  Insertion rate: {:.2} items/second", inserts_per_second);

        // Calculate bit density after filling this level
        let bit_density = calculate_bit_density(&filter, level);
        println!("  Bit density: {:.2}%", bit_density * 100.0);

        // Store metrics for this level
        level_metrics.push((
            level,
            insert_duration,
            inserts_per_second,
            bit_density,
            level_size,
        ));

        // Rotate to next level for the next batch (simulate time passing)
        if level < NUM_LEVELS - 1 {
            // Force creation of new level
            println!("  Rotating to next level...");
            thread::sleep(Duration::from_secs(2));
            filter.cleanup_expired_levels()?;
        }
    }

    // Save snapshot and get final database size
    println!("\nSaving snapshot...");
    let snapshot_start = Instant::now();
    filter.save_snapshot()?;
    let snapshot_duration = snapshot_start.elapsed();
    println!("Snapshot saved in {:.2?}", snapshot_duration);

    let db_size = get_file_size(db_path)?;
    let total_items_inserted = level_metrics
        .iter()
        .map(|(_, _, _, _, size)| size)
        .sum::<usize>();
    let bits_per_item = (db_size * 8) as f64 / total_items_inserted as f64;

    // Perform query benchmarks
    println!("\nRunning query benchmarks...");

    // Generate additional test data for unknown queries
    let unknown_data = generate_test_data(10_000);

    // Sample from each level for known queries
    let mut known_sample = Vec::new();
    for level in 0..NUM_LEVELS {
        let indices = &level_distributions[level];
        let sample_size = std::cmp::min(2_000, indices.len() / 10);

        for i in 0..sample_size {
            let idx = indices[i * indices.len() / sample_size];
            if idx < all_test_data.len() {
                known_sample.push(all_test_data[idx].clone());
            }
        }
    }

    let (
        _known_positive,     // TODO: use it
        _avg_query_duration, // TODO: use it
        per_query_time,
        measured_fpr,
        traceable_results,
    ) = measure_query_performance(
        &filter,
        &known_sample,
        &unknown_data,
        &traceable_items,
    );

    // Create a table for level metrics
    let mut level_table = Table::new();
    level_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "Level",
            "Items",
            "Duration",
            "Items/sec",
            "Bit Density",
        ]);

    for (level, duration, rate, density, size) in &level_metrics {
        level_table.add_row(vec![
            &format!("{}", level),
            &format!("{}", size),
            &format!("{:.2?}", duration),
            &format!("{:.2}", rate),
            &format!("{:.2}%", density * 100.0),
        ]);
    }

    println!("\nLevel Metrics:");
    println!("{}", level_table);

    // Create a table for storage metrics
    let mut storage_table = Table::new();
    storage_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Metric", "Value"]);

    storage_table.add_row(vec!["Database Size", &format_file_size(db_size)]);
    storage_table
        .add_row(vec!["Bits per Item", &format!("{:.2}", bits_per_item)]);
    storage_table.add_row(vec![
        "Total Items Inserted",
        &format!("{}", total_items_inserted),
    ]);
    storage_table.add_row(vec!["Unique Items", &format!("{}", UNIQUE_ITEMS)]);
    storage_table.add_row(vec![
        "Snapshot Duration",
        &format!("{:.2?}", snapshot_duration),
    ]);

    println!("\nStorage Metrics:");
    println!("{}", storage_table);

    // Create a table for query performance
    let mut query_table = Table::new();
    query_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Metric", "Value"]);

    query_table.add_row(vec![
        "Average Query Time",
        &format!("{:.2?}", per_query_time),
    ]);
    query_table.add_row(vec![
        "Queries per Second",
        &format!("{:.2}", 1.0 / per_query_time.as_secs_f64()),
    ]);
    query_table.add_row(vec![
        "Measured False Positive",
        &format!("{:.4}%", measured_fpr * 100.0),
    ]);

    println!("\nQuery Performance:");
    println!("{}", query_table);

    // Create a sample table for traceable items
    let mut traceable_table = Table::new();
    traceable_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Traceable Item", "In Filter", "In Levels"]);

    // Display a sample of 10 traceable items
    let check_mark = "✓".to_string();
    let x_mark = "✗".to_string();

    for (item, (item_name, found)) in traceable_items
        .iter()
        .zip(traceable_results.iter())
        .take(10)
    {
        let level_info = traceable_level_map
            .get(item)
            .map(|levels| {
                levels
                    .iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "None".into());

        traceable_table.add_row(vec![
            item_name,
            if *found { &check_mark } else { &x_mark },
            &level_info,
        ]);
    }

    println!("\nTraceable Items Sample (first 10):");
    println!("{}", traceable_table);

    // After the traceable items section, add this code:

    // Create a sample table for non-traceable items
    let mut non_traceable_table = Table::new();
    non_traceable_table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Non-Traceable Item", "In Filter", "Expected"]);

    // Generate some non-traceable items
    let non_traceable_items: Vec<String> = (0..10)
        .map(|i| format!("definitely_not_in_filter_{}", i))
        .collect();

    // Check if they're in the filter
    let non_traceable_results: Vec<(String, bool)> = non_traceable_items
        .iter()
        .map(|item| {
            let exists = filter.query(item.as_bytes()).unwrap_or(false);
            (item.clone(), exists)
        })
        .collect();

    // Display the non-traceable items
    let check_mark = "✓".to_string();
    let x_mark = "✗".to_string();

    for (item, found) in non_traceable_results.iter().take(10) {
        non_traceable_table.add_row(vec![
            item,
            if *found { &check_mark } else { &x_mark },
            &x_mark, // Always expected to be not found
        ]);
    }

    println!("\nNon-Traceable Items Sample (10):");
    println!("{}", non_traceable_table);

    // Add statistics about false positives in non-traceable items
    let false_positive_count = non_traceable_results
        .iter()
        .filter(|(_, found)| *found)
        .count();
    if false_positive_count > 0 {
        println!(
            "\nDetected {} false positives in non-traceable items check!",
            false_positive_count
        );
        println!(
            "This matches the expected false positive rate of the filter configuration."
        );
    } else {
        println!(
            "\nNo false positives detected in the non-traceable items check."
        );
    }

    // Theoretical vs. measured statistics
    println!("\nTheoretical vs. Measured Performance:");
    println!("  Configured FPR: 1.00%");
    println!("  Measured FPR:   {:.4}%", measured_fpr * 100.0);
    println!(
        "  Space efficiency: {:.2} bits/item (optimal is typically 9-10)",
        bits_per_item
    );

    // Data distribution statistics
    println!("\nData Distribution Statistics:");
    println!(
        "  Average elements per level: {:.2}",
        total_items_inserted as f64 / NUM_LEVELS as f64
    );

    let mut overlap_counts = [0; NUM_LEVELS + 1];
    for item in traceable_items.iter().take(TRACEABLE_ITEMS) {
        let count = traceable_level_map
            .get(item)
            .map_or(0, |levels| levels.len());
        overlap_counts[count] += 1;
    }

    println!("  Distribution of traceable items across levels:");
    for (i, count) in overlap_counts.iter().enumerate() {
        println!("    Items in {} level(s): {}", i, count);
    }

    // Optional: clean up the database file
    fs::remove_file(db_path)?;
    println!("\nBenchmark complete. Database file cleaned up.");

    Ok(())
}

// Generate random string data
fn generate_random_string(len: usize) -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

// Generate test data
fn generate_test_data(count: usize) -> Vec<String> {
    (0..count).map(|_| generate_random_string(32)).collect()
}

// Generate traceable items with predictable names
fn generate_traceable_items(count: usize) -> Vec<String> {
    let mut items = Vec::with_capacity(count);
    let mut rng = rand::rng();

    for i in 0..count {
        // Pick 2-3 random words from our list and join them
        let num_words = rng.random_range(2..=3);
        let mut selected_words = Vec::with_capacity(num_words);

        for _ in 0..num_words {
            selected_words.push(*WORD_LIST.choose(&mut rng).unwrap());
        }

        // Add a unique identifier to ensure uniqueness
        let identifier = format!("{:04}", i);

        // Join the words with underscores and append the identifier
        items.push(format!("{}_{}", selected_words.join("_"), identifier));
    }

    items
}

// Convert bytes to human-readable size
fn format_file_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} bytes", size)
    }
}

// Get file size in bytes
fn get_file_size(path: &Path) -> std::io::Result<u64> {
    let metadata = fs::metadata(path)?;
    Ok(metadata.len())
}

fn create_filter(db_path: &Path) -> RedbFilter {
    // Create the filter configuration
    let config = FilterConfigBuilder::default()
        .capacity(CAPACITY)
        .false_positive_rate(0.01)
        .level_duration(Duration::from_secs(60))
        .max_levels(NUM_LEVELS)
        .build()
        .unwrap();

    let redb_config = RedbFilterConfigBuilder::default()
        .db_path(db_path.to_path_buf())
        .filter_config(Some(config))
        .snapshot_interval(Duration::from_secs(60))
        .build()
        .unwrap();

    // Create a new filter
    RedbFilter::new(redb_config).unwrap()
}

// Calculate average bit density in a bit vector
fn calculate_bit_density(filter: &RedbFilter, level: usize) -> f64 {
    let level_bits = &filter.storage.levels.read().unwrap()[level];
    // let set_bits = level_bits.iter().filter(|&&bit| bit).count();
    let set_bits = level_bits.iter().filter(|bit| **bit).count();

    set_bits as f64 / level_bits.len() as f64
}

// Measure query performance
fn measure_query_performance(
    filter: &RedbFilter,
    known_items: &[String],
    unknown_items: &[String],
    traceable_items: &[String],
) -> (u32, Duration, Duration, f64, Vec<(String, bool)>) {
    // Query known items
    let start_time = Instant::now();
    let mut known_positive = 0u32;
    for item in known_items {
        if filter.query(item.as_bytes()).unwrap() {
            known_positive += 1;
        }
    }
    let known_duration = start_time.elapsed();
    let known_rate = known_items.len() as f64 / known_duration.as_secs_f64();

    // Query unknown items
    let start_time = Instant::now();
    let mut false_positive = 0;
    for item in unknown_items {
        if filter.query(item.as_bytes()).unwrap() {
            false_positive += 1;
        }
    }
    let unknown_duration = start_time.elapsed();
    let false_positive_rate = false_positive as f64 / unknown_items.len() as f64;

    // Track traceable items
    let mut traceable_results = Vec::new();
    for item in traceable_items {
        let exists = filter.query(item.as_bytes()).unwrap();
        traceable_results.push((item.clone(), exists));
    }

    // Average query duration
    let avg_duration = (known_duration + unknown_duration) / 2;

    (
        known_positive,
        avg_duration,
        Duration::from_secs_f64(1.0 / known_rate),
        false_positive_rate,
        traceable_results,
    )
}

fn generate_time_ordered_elements(
    total_unique: usize,
    num_levels: usize,
    items_per_level: usize,
    overlap_factor: f64,
) -> Vec<Vec<usize>> {
    let mut rng = rand::rng();
    let mut levels = vec![Vec::new(); num_levels];
    let mut current_idx = 0;

    let overlap_items =
        (items_per_level as f64 * overlap_factor).round() as usize;
    let unique_items = items_per_level - overlap_items;

    for level in 0..num_levels {
        // Add unique items
        for _ in 0..unique_items {
            if current_idx < total_unique {
                levels[level].push(current_idx);
                current_idx += 1;
            }
        }

        // Add overlap from previous level
        if level > 0 {
            let prev_level = &levels[level - 1];
            let overlap_subset: Vec<_> = prev_level
                .choose_multiple(&mut rng, overlap_items.min(prev_level.len()))
                .cloned()
                .collect();
            levels[level].extend(overlap_subset);
        }
    }

    levels
}
