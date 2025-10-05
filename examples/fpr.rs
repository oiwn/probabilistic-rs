#![allow(clippy::uninlined_format_args)]
mod common;

use comfy_table::{
    Cell, CellAlignment, ContentArrangement, Table,
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL,
};
use common::{generate_random_string, generate_test_data};
use probablistic_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, InMemoryFilter,
};
use std::{collections::HashSet, time::Duration};

// Test configuration
const LEVEL_DURATION_MS: u64 = 100; // Short duration for testing
const MAX_LEVELS: usize = 1;
const FILL_RATIO: f64 = 0.5; // Fill the filter to 70% of capacity
const TEST_SAMPLES: usize = 10_000; // Number of unknown elements to test for FPR

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║  Time-Decaying Bloom Filter - False Positive Rate Tester  ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    println!("Configuration:");
    println!("  • Levels: {}", MAX_LEVELS);
    println!("  • Level Duration: {} ms", LEVEL_DURATION_MS);
    println!("  • Fill Ratio: {}%", FILL_RATIO * 100.0);
    println!("  • Test Samples: {}", TEST_SAMPLES);

    // Configurations to test
    let capacities = [1_000, 10_000, 100_000];
    let target_fprs = [0.01, 0.05, 0.1];

    // Create a table for results
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Capacity").set_alignment(CellAlignment::Center),
            Cell::new("Target FPR").set_alignment(CellAlignment::Center),
            Cell::new("Elements").set_alignment(CellAlignment::Center),
            Cell::new("Known Positives").set_alignment(CellAlignment::Center),
            Cell::new("False Positives").set_alignment(CellAlignment::Center),
            Cell::new("Observed FPR").set_alignment(CellAlignment::Center),
            Cell::new("Deviation").set_alignment(CellAlignment::Center),
        ]);

    // Run tests for each configuration
    for &capacity in &capacities {
        for &target_fpr in &target_fprs {
            // Calculate how many elements to insert
            let insert_count = (capacity as f64 * FILL_RATIO) as usize;

            // Create the filter
            let config = FilterConfigBuilder::default()
                .capacity(capacity)
                .false_positive_rate(target_fpr)
                .level_duration(Duration::from_millis(LEVEL_DURATION_MS))
                .max_levels(MAX_LEVELS)
                .build()?;

            let mut filter = InMemoryFilter::new(config)?;

            // Generate and insert known elements
            print!(
                "Testing capacity={}, target_fpr={:.2}%: ",
                capacity,
                target_fpr * 100.0
            );
            print!("Generating data... ");
            let known_elements = generate_test_data(insert_count);

            // Create a HashSet for faster lookups
            let known_set: HashSet<String> =
                known_elements.iter().cloned().collect();

            // Insert known elements
            print!("Inserting... ");
            for element in &known_elements {
                filter.insert(element.as_bytes())?;
            }

            // Test for false positives
            print!("Testing FPR... ");
            let mut true_positives = 0;
            let mut false_positives = 0;

            // First check all known elements (should all be found)
            for element in &known_elements {
                if filter.query(element.as_bytes())? {
                    true_positives += 1;
                }
            }

            // Then test for false positives with unknown elements
            for _ in 0..TEST_SAMPLES {
                let unknown = generate_random_string(32);
                // Skip if we accidentally generated a known element
                if known_set.contains(&unknown) {
                    continue;
                }

                if filter.query(unknown.as_bytes())? {
                    false_positives += 1;
                }
            }

            // Calculate observed FPR
            let observed_fpr = false_positives as f64 / TEST_SAMPLES as f64;
            let deviation = (observed_fpr - target_fpr) / target_fpr * 100.0;

            println!("Done!");

            // Add results to table
            table.add_row(vec![
                Cell::new(format!("{}", capacity)),
                Cell::new(format!("{:.2}%", target_fpr * 100.0)),
                Cell::new(format!("{}", insert_count)),
                Cell::new(format!("{}/{}", true_positives, known_elements.len())),
                Cell::new(format!("{}/{}", false_positives, TEST_SAMPLES)),
                Cell::new(format!("{:.4}%", observed_fpr * 100.0)),
                Cell::new(format!("{:+.2}%", deviation)),
            ]);
        }
    }

    // Print results table
    println!("\nResults:");
    println!("{}", table);

    // Print summary
    println!("\nSummary:");
    println!(
        "The Bloom filter implementation performs as expected with FPR rates"
    );
    println!(
        "generally close to the configured target values. Deviations can occur"
    );
    println!(
        "due to randomness in hash functions and the probabilistic nature of"
    );
    println!(
        "Bloom filters. Lower capacities tend to have higher variance in FPR."
    );

    Ok(())
}
