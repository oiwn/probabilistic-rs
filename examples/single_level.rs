#![allow(clippy::uninlined_format_args)]
mod common;
use comfy_table::{
    Cell, CellAlignment, ContentArrangement, Table,
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL,
};
use common::generate_test_data;
use expiring_bloom_rs::{
    ExpiringBloomFilter, FilterConfigBuilder, InMemoryFilter,
};
use std::time::Duration;

// Test configuration
const CAPACITY: usize = 10_000;
const LEVEL_DURATION_MS: u64 = 100; // Short duration for testing
const MAX_LEVELS: usize = 1; // Single level for this test
const TARGET_FPR: f64 = 0.01; // 1% target FPR
const TEST_SAMPLES: usize = 10_000; // Number of unknown elements to test for FPR
const FILL_STEPS: usize = 10; // Test 10 different fill ratios (10%, 20%, ..., 100%)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║  Bloom Filter FPR Growth Analysis with Increasing Elements  ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    println!("Configuration:");
    println!("  • Capacity: {}", CAPACITY);
    println!("  • Target FPR: {}%", TARGET_FPR * 100.0);
    println!("  • Single-level filter (no time decay)");
    println!("  • Test samples for FPR measurement: {}", TEST_SAMPLES);

    // Create the filter once
    let config = FilterConfigBuilder::default()
        .capacity(CAPACITY)
        .false_positive_rate(TARGET_FPR)
        .level_duration(Duration::from_millis(LEVEL_DURATION_MS))
        .max_levels(MAX_LEVELS)
        .build()?;

    let mut filter = InMemoryFilter::new(config)?;

    // Create a table for results
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Fill Ratio").set_alignment(CellAlignment::Center),
            Cell::new("Elements").set_alignment(CellAlignment::Center),
            Cell::new("Theoretical FPR").set_alignment(CellAlignment::Center),
            Cell::new("Measured FPR").set_alignment(CellAlignment::Center),
            Cell::new("FPR Ratio").set_alignment(CellAlignment::Center),
        ]);

    // Generate a large set of unique elements
    println!("Generating test data...");
    let all_elements = generate_test_data(CAPACITY);
    let test_elements = generate_test_data(TEST_SAMPLES);

    println!("Running tests for different fill ratios...");

    // Track elements already inserted
    let mut inserted_count = 0;

    // Test different fill ratios
    for step in 1..=FILL_STEPS {
        let fill_ratio = step as f64 / FILL_STEPS as f64;
        let total_elements = (CAPACITY as f64 * fill_ratio) as usize;
        let elements_to_add = total_elements - inserted_count;

        println!(
            "Testing fill ratio: {:.0}% ({} elements)...",
            fill_ratio * 100.0,
            total_elements
        );

        // Insert additional elements
        for i in 0..elements_to_add {
            let index = inserted_count + i;
            if index < all_elements.len() {
                filter.insert(all_elements[index].as_bytes())?;
            }
        }
        inserted_count = total_elements;

        // Calculate theoretical FPR at this fill ratio
        // Formula: (1 - e^(-k*n/m))^k where k is num_hashes, n is elements, m is capacity
        let k = filter.num_hashes;
        let n = total_elements;
        let m = CAPACITY;
        let theoretical_fpr = (1.0
            - std::f64::consts::E.powf(-(k as f64 * n as f64) / m as f64))
        .powi(k as i32);

        // Measure actual FPR
        let mut false_positives = 0;
        for element in &test_elements {
            // Skip if we accidentally test an element we already inserted
            if all_elements[0..total_elements].contains(element) {
                continue;
            }

            if filter.query(element.as_bytes())? {
                false_positives += 1;
            }
        }

        // Calculate measured FPR
        let measured_fpr = false_positives as f64 / test_elements.len() as f64;

        // Calculate ratio between measured and theoretical
        let fpr_ratio = if theoretical_fpr > 0.0 {
            measured_fpr / theoretical_fpr
        } else {
            0.0
        };

        // Add results to table
        table.add_row(vec![
            Cell::new(format!("{:.0}%", fill_ratio * 100.0)),
            Cell::new(format!("{}", total_elements)),
            Cell::new(format!("{:.6}%", theoretical_fpr * 100.0)),
            Cell::new(format!("{:.6}%", measured_fpr * 100.0)),
            Cell::new(format!("{:.2}x", fpr_ratio)),
        ]);
    }

    // Print results table
    println!("\nResults:");
    println!("{}", table);

    // Print summary
    println!("\nAnalysis:");
    println!(
        "1. As the fill ratio increases, the false positive rate grows exponentially"
    );
    println!(
        "2. Once the filter approaches 50% capacity, the FPR begins to rise quickly"
    );
    println!(
        "3. At very high fill ratios (>70%), the FPR may become unacceptably high"
    );
    println!(
        "4. The theoretical model generally predicts the actual behavior well"
    );
    println!(
        "\nRecommendation: For optimal performance, keep fill ratio below 50%"
    );

    Ok(())
}
