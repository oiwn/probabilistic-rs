#![allow(clippy::uninlined_format_args)]
mod common;

use common::{format_file_size, generate_random_string};
use probablistic_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, FjallFilter,
    FjallFilterConfigBuilder,
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

// Replace the existing get_file_size function with this:
fn get_directory_size(path: &Path) -> std::io::Result<u64> {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len());
    }

    let mut total_size = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() {
                total_size += metadata.len();
            } else if metadata.is_dir() {
                total_size += get_directory_size(&entry.path())?;
            }
        }
    }
    Ok(total_size)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Define the capacities to test
    let capacities = [100_000, 1_000_000, 10_000_000];
    let mut results = Vec::new();

    // First, create and fill all the databases
    for &capacity in &capacities {
        let db_name = format!("bloom_fjall_size_{}.fjall", capacity);
        let db_path = Path::new(&db_name);

        // Remove existing directory if it exists
        if db_path.exists() {
            fs::remove_dir_all(db_path)?;
        }

        let fill_count = (capacity * 75) / 100;

        println!(
            "Creating and filling Fjall filter with capacity {} ({}% = {} items)...",
            capacity, 75, fill_count
        );

        // Create the filter configuration
        let config = FilterConfigBuilder::default()
            .capacity(capacity)
            .false_positive_rate(0.01)
            .level_duration(Duration::from_secs(60))
            .max_levels(3)
            .build()?;

        let fjall_config = FjallFilterConfigBuilder::default()
            .db_path(db_path.to_path_buf())
            .filter_config(Some(config))
            .snapshot_interval(Duration::from_secs(60))
            .build()?;

        // Create a new filter
        let mut filter = FjallFilter::new(fjall_config)?;

        let start_time = Instant::now();
        // Fill the filter with random data
        for i in 0..fill_count {
            if i % (fill_count / 10) == 0 {
                print!("{}%... ", (i * 100) / fill_count);
                std::io::Write::flush(&mut std::io::stdout())?;
            }

            let item = generate_random_string(32);
            filter.insert(item.as_bytes())?;
        }
        println!("100%");

        let fill_duration = start_time.elapsed();
        println!("\t(Completed in {:.2?})", fill_duration);

        println!("Saving snapshot...");
        let start_time = Instant::now();
        filter.save_snapshot()?;

        let save_duration = start_time.elapsed();
        println!("\t(Completed in {:.2?})", save_duration);

        // Drop the filter to ensure all data is flushed
        drop(filter);

        // Get the directory size
        let size = get_directory_size(db_path)?;

        // Store results for later display
        results.push((capacity, fill_count, size));
    }

    // Print the results table (same as in sizes.rs)
    println!("\n┌{:─^60}┐", "");
    println!("│{:^60}│", "Fjall Bloom Filter Size Measurement");
    println!("├{:─^15}┬{:─^15}┬{:─^15}┬{:─^12}┤", "", "", "", "");
    println!(
        "│{:^15}│{:^15}│{:^15}│{:^12}│",
        "Capacity", "Fill Count", "Fill Ratio", "File Size"
    );
    println!("├{:─^15}┼{:─^15}┼{:─^15}┼{:─^12}┤", "", "", "", "");

    for (capacity, fill_count, size) in &results {
        let formatted_size = format_file_size(*size);
        println!(
            "│{:^15}│{:^15}│{:^15}│{:^12}│",
            capacity, fill_count, "75%", formatted_size
        );
    }

    println!("└{:─^15}┴{:─^15}┴{:─^15}┴{:─^12}┘", "", "", "", "");

    println!("\nDetailed file information:");
    for (capacity, fill_count, size) in &results {
        let db_path = format!("bloom_size_{}.fjall", capacity);
        let bits_per_item = (size * 8) as f64 / *fill_count as f64;

        println!("File: {}", db_path);
        println!("  - Raw size:      {} bytes", size);
        println!("  - Human size:    {}", format_file_size(*size));
        println!("  - Bits per item: {:.2} bits/item", bits_per_item);
        println!();
    }

    println!("\nCleaning up database files...");
    for (capacity, _, _) in &results {
        let file_name = format!("bloom_size_{}.fjall", capacity);
        let db_path = Path::new(&file_name);
        if db_path.exists() {
            let _ = fs::remove_file(db_path);
        }
    }

    Ok(())
}
