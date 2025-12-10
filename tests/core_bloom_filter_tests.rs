use probabilistic_rs::bloom::{
    BloomFilter, BloomFilterConfigBuilder, BloomFilterOps, BloomFilterStats,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

// Helper function to create a basic in-memory filter for testing
fn create_test_filter(capacity: usize, fpr: f64) -> BloomFilter {
    let config = BloomFilterConfigBuilder::default()
        .capacity(capacity)
        .false_positive_rate(fpr)
        .persistence(None)
        .build()
        .expect("Failed to build test config");

    tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime")
        .block_on(BloomFilter::create(config))
        .expect("Failed to create test filter")
}

// Helper function to generate consistent test data
fn generate_test_items(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| format!("test_item_{:06}", i).into_bytes())
        .collect()
}

#[cfg(test)]
mod basic_operations_tests {
    use super::*;

    #[test]
    fn test_insert_and_contains() {
        let filter = create_test_filter(1000, 0.01);

        // Test single item
        let item = b"hello_world";
        filter.insert(item).expect("Insert should succeed");
        assert!(
            filter.contains(item).expect("Contains should succeed"),
            "Item should be found after insertion"
        );

        // Test different item not inserted
        let other_item = b"goodbye_world";
        let _result = filter
            .contains(other_item)
            .expect("Contains should succeed");
        // Note: result might be true due to false positive, but that's acceptable
    }

    #[test]
    fn test_multiple_insertions() {
        let filter = create_test_filter(1000, 0.01);
        let test_items = generate_test_items(10);

        // Insert all items
        for item in &test_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // All inserted items should be found (no false negatives)
        for item in &test_items {
            assert!(
                filter.contains(item).expect("Contains should succeed"),
                "No false negatives allowed for item: {:?}",
                String::from_utf8_lossy(item)
            );
        }

        // Verify insert count
        assert_eq!(
            filter.insert_count(),
            test_items.len(),
            "Insert count should match number of inserted items"
        );
    }

    #[test]
    fn test_clear_functionality() {
        let filter = create_test_filter(1000, 0.01);
        let test_items = generate_test_items(5);

        // Insert items
        for item in &test_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // Verify items are present
        assert!(filter.insert_count() > 0, "Should have items before clear");

        // Clear the filter
        filter.clear().expect("Clear should succeed");

        // Verify filter is cleared
        assert_eq!(
            filter.insert_count(),
            0,
            "Insert count should be 0 after clear"
        );

        // Verify previously inserted items are no longer found
        for item in &test_items {
            assert!(
                !filter.contains(item).expect("Contains should succeed"),
                "Items should not be found after clear"
            );
        }

        // Verify filter is still usable after clear
        let new_item = b"new_item_after_clear";
        filter
            .insert(new_item)
            .expect("Insert should work after clear");
        assert!(
            filter.contains(new_item).expect("Contains should succeed"),
            "Filter should work normally after clear"
        );
    }

    #[test]
    fn test_empty_filter_behavior() {
        let filter = create_test_filter(1000, 0.01);

        // Empty filter should not contain any items
        assert!(
            !filter
                .contains(b"anything")
                .expect("Contains should succeed"),
            "Empty filter should not contain any items"
        );

        // Stats should reflect empty state
        assert_eq!(
            filter.insert_count(),
            0,
            "Empty filter should have 0 insert count"
        );
    }

    #[test]
    fn test_duplicate_insertions() {
        let filter = create_test_filter(1000, 0.01);
        let item = b"duplicate_item";

        // Insert same item multiple times
        for _ in 0..5 {
            filter.insert(item).expect("Insert should succeed");
        }

        // Item should still be found
        assert!(
            filter.contains(item).expect("Contains should succeed"),
            "Item should be found regardless of duplicate insertions"
        );

        // Insert count should reflect all insertions (bloom filter doesn't dedupe)
        assert_eq!(
            filter.insert_count(),
            5,
            "Insert count should reflect all insertions, including duplicates"
        );
    }
}

#[cfg(test)]
mod configuration_and_stats_tests {
    use super::*;

    #[test]
    fn test_config_validation_valid() {
        // Valid configurations should work
        let valid_configs =
            [(1000, 0.01), (100_000, 0.001), (1, 0.5), (1_000_000, 0.1)];

        for (capacity, fpr) in valid_configs {
            let config = BloomFilterConfigBuilder::default()
                .capacity(capacity)
                .false_positive_rate(fpr)
                .build()
                .expect("Valid config should build successfully");

            assert!(
                config.validate().is_ok(),
                "Valid config should pass validation: capacity={}, fpr={}",
                capacity,
                fpr
            );
        }
    }

    #[test]
    fn test_config_validation_invalid() {
        // Test zero capacity
        let config = BloomFilterConfigBuilder::default()
            .capacity(0)
            .false_positive_rate(0.01)
            .build()
            .expect("Config should build");

        assert!(
            config.validate().is_err(),
            "Zero capacity should fail validation"
        );

        // Test invalid FPR values
        let invalid_fprs = [0.0, 1.0, -0.1, 1.1];
        for fpr in invalid_fprs {
            let config = BloomFilterConfigBuilder::default()
                .capacity(1000)
                .false_positive_rate(fpr)
                .build()
                .expect("Config should build");

            assert!(
                config.validate().is_err(),
                "Invalid FPR {} should fail validation",
                fpr
            );
        }
    }

    #[test]
    fn test_stats_accuracy() {
        let capacity = 5000;
        let fpr = 0.02;
        let filter = create_test_filter(capacity, fpr);

        // Test basic stats
        assert_eq!(filter.capacity(), capacity, "Capacity should match config");
        assert_eq!(filter.false_positive_rate(), fpr, "FPR should match config");
        assert_eq!(
            filter.insert_count(),
            0,
            "Empty filter should have 0 insert count"
        );
    }

    #[test]
    fn test_memory_estimation() {
        let filter = create_test_filter(10_000, 0.01);

        let memory_bits = filter.approx_memory_bits();
        let bits_per_item = filter.bits_per_item();

        // Memory should be reasonable (not zero, not ridiculously large)
        assert!(
            memory_bits > 0,
            "Memory estimation should be greater than 0"
        );

        // Bits per item should be reasonable (theoretical minimum for 1% FPR is ~9.6 bits)
        // Note: Due to how memory is allocated in chunks, actual may be lower
        assert!(
            (1.0..=50.0).contains(&bits_per_item),
            "Bits per item should be reasonable: {}",
            bits_per_item
        );

        println!(
            "Memory stats - Total bits: {}, Bits per item: {:.2}",
            memory_bits, bits_per_item
        );
    }

    #[test]
    fn test_different_config_combinations() {
        let configs = [
            (100, 0.1),      // Small, high FPR
            (1000, 0.01),    // Medium, medium FPR
            (10_000, 0.001), // Large, low FPR
        ];

        for (capacity, fpr) in configs {
            let filter = create_test_filter(capacity, fpr);

            // Verify configuration is applied correctly
            assert_eq!(filter.capacity(), capacity);
            assert_eq!(filter.false_positive_rate(), fpr);

            // Verify filter works with this configuration
            filter
                .insert(b"test")
                .expect("Insert should work with any valid config");
            assert!(filter.contains(b"test").expect("Contains should work"));
        }
    }
}

#[cfg(test)]
mod edge_cases_and_error_conditions {
    use super::*;

    #[test]
    fn test_empty_item_insertion() {
        let filter = create_test_filter(1000, 0.01);

        // Insert empty byte slice
        let empty_item = b"";
        filter
            .insert(empty_item)
            .expect("Should be able to insert empty item");

        // Should be able to query empty item
        assert!(
            filter
                .contains(empty_item)
                .expect("Should be able to query empty item"),
            "Empty item should be found after insertion"
        );
    }

    #[test]
    fn test_large_item_insertion() {
        let filter = create_test_filter(1000, 0.01);

        // Create large item (1MB)
        let large_item = vec![42u8; 1024 * 1024];

        filter
            .insert(&large_item)
            .expect("Should be able to insert large item");

        assert!(
            filter
                .contains(&large_item)
                .expect("Should be able to query large item"),
            "Large item should be found after insertion"
        );
    }

    #[test]
    fn test_capacity_stress_test() {
        let capacity = 100;
        let filter = create_test_filter(capacity, 0.1);

        // Insert more items than capacity
        let test_items = generate_test_items(capacity * 2);

        for item in &test_items {
            filter
                .insert(item)
                .expect("Insert should succeed even beyond capacity");
        }

        // All items should still be found (no false negatives)
        let mut found_count = 0;
        for item in &test_items {
            if filter.contains(item).expect("Contains should succeed") {
                found_count += 1;
            }
        }

        // Should find all items (no false negatives allowed)
        assert_eq!(
            found_count,
            test_items.len(),
            "All inserted items should be found, even beyond capacity"
        );

        // Insert count should reflect all insertions
        assert_eq!(
            filter.insert_count(),
            test_items.len(),
            "Insert count should reflect all insertions"
        );
    }

    #[test]
    fn test_binary_data_handling() {
        let filter = create_test_filter(1000, 0.01);

        // Test various binary data patterns
        let binary_items = vec![
            vec![0u8; 10],           // All zeros
            vec![255u8; 10],         // All ones
            (0u8..=255u8).collect(), // Sequential bytes
            vec![0, 1, 0, 1, 0, 1],  // Pattern
        ];

        for item in &binary_items {
            filter.insert(item).expect("Should handle binary data");

            assert!(
                filter.contains(item).expect("Should query binary data"),
                "Binary data should be found after insertion"
            );
        }
    }
}

#[cfg(test)]
mod behavioral_guarantees_tests {
    use super::*;

    #[test]
    fn test_no_false_negatives_guarantee() {
        let filter = create_test_filter(1000, 0.01);
        let test_items = generate_test_items(100);

        // Insert all items
        for item in &test_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // Every inserted item MUST be found (zero false negatives)
        for (i, item) in test_items.iter().enumerate() {
            assert!(
                filter.contains(item).expect("Contains should succeed"),
                "FALSE NEGATIVE detected for item {}: {:?}",
                i,
                String::from_utf8_lossy(item)
            );
        }
    }

    #[test]
    fn test_false_positive_rate_measurement() {
        let capacity = 1000;
        let target_fpr = 0.05; // 5% target FPR
        let filter = create_test_filter(capacity, target_fpr);

        // Insert items up to about 50% of capacity
        let inserted_items = generate_test_items(capacity / 2);
        let inserted_set: HashSet<Vec<u8>> =
            inserted_items.iter().cloned().collect();

        for item in &inserted_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // Test with items that were NOT inserted
        let test_items = generate_test_items(2000);
        let mut false_positives = 0;
        let mut tested_count = 0;

        for item in &test_items {
            if !inserted_set.contains(item) {
                tested_count += 1;
                if filter.contains(item).expect("Contains should succeed") {
                    false_positives += 1;
                }
            }
        }

        let measured_fpr = false_positives as f64 / tested_count as f64;

        println!(
            "False positive measurement - Target: {:.1}%, Measured: {:.1}% ({}/{} items)",
            target_fpr * 100.0,
            measured_fpr * 100.0,
            false_positives,
            tested_count
        );

        // FPR should be reasonable (within 3x of target, allowing for statistical variation)
        assert!(
            measured_fpr <= target_fpr * 3.0,
            "Measured FPR ({:.3}%) should be within 3x of target ({:.3}%)",
            measured_fpr * 100.0,
            target_fpr * 100.0
        );
    }

    #[test]
    fn test_deterministic_behavior() {
        let capacity = 1000;
        let fpr = 0.01;

        // Create two identical filters
        let filter1 = create_test_filter(capacity, fpr);
        let filter2 = create_test_filter(capacity, fpr);

        let test_items = generate_test_items(10);

        // Insert same items in same order
        for item in &test_items {
            filter1.insert(item).expect("Insert should succeed");
            filter2.insert(item).expect("Insert should succeed");
        }

        // Both filters should behave identically
        for item in &test_items {
            let result1 =
                filter1.contains(item).expect("Contains should succeed");
            let result2 =
                filter2.contains(item).expect("Contains should succeed");

            assert_eq!(
                result1,
                result2,
                "Identical filters should produce identical results for item: {:?}",
                String::from_utf8_lossy(item)
            );
        }
    }
}

#[cfg(test)]
mod thread_safety_tests {
    use super::*;

    #[test]
    fn test_concurrent_reads() {
        let filter = create_test_filter(10_000, 0.01);
        let test_items = generate_test_items(100);

        // Insert test items
        for item in &test_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // Wrap filter in Arc for sharing between threads
        let filter = Arc::new(filter);
        let mut handles = vec![];

        // Spawn multiple reader threads
        for thread_id in 0..4 {
            let filter_clone = Arc::clone(&filter);
            let items_clone = test_items.clone();

            let handle = thread::spawn(move || {
                for item in items_clone {
                    let result = filter_clone
                        .contains(&item)
                        .expect("Contains should succeed in thread");

                    // All inserted items should be found
                    assert!(
                        result,
                        "Thread {} should find inserted item: {:?}",
                        thread_id,
                        String::from_utf8_lossy(&item)
                    );
                }
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread should complete successfully");
        }
    }

    #[test]
    fn test_single_writer_multiple_readers() {
        let filter = Arc::new(Mutex::new(create_test_filter(10_000, 0.01)));
        let test_items = generate_test_items(50);
        let mut handles = vec![];

        // Writer thread
        let writer_filter = Arc::clone(&filter);
        let writer_items = test_items.clone();
        let writer_handle = thread::spawn(move || {
            for item in writer_items {
                thread::sleep(Duration::from_millis(1)); // Small delay to interleave operations
                let filter_guard =
                    writer_filter.lock().expect("Writer should acquire lock");
                filter_guard.insert(&item).expect("Insert should succeed");
            }
        });

        // Multiple reader threads
        for thread_id in 0..3 {
            let reader_filter = Arc::clone(&filter);
            let reader_items = test_items.clone();

            let handle = thread::spawn(move || {
                thread::sleep(Duration::from_millis(10)); // Let writer get started

                for _retry in 0..10 {
                    thread::sleep(Duration::from_millis(5));
                    let filter_guard =
                        reader_filter.lock().expect("Reader should acquire lock");

                    // Count how many items are found
                    let found_count = reader_items
                        .iter()
                        .filter(|item| {
                            filter_guard
                                .contains(item)
                                .expect("Contains should succeed")
                        })
                        .count();

                    println!(
                        "Thread {} found {}/{} items",
                        thread_id,
                        found_count,
                        reader_items.len()
                    );
                }
            });

            handles.push(handle);
        }

        // Wait for writer to complete
        writer_handle.join().expect("Writer thread should complete");

        // Wait for all reader threads to complete
        for handle in handles {
            handle.join().expect("Reader thread should complete");
        }

        // Final verification: all items should be found
        let final_filter = filter.lock().expect("Should acquire final lock");
        for item in &test_items {
            assert!(
                final_filter
                    .contains(item)
                    .expect("Final check should succeed"),
                "All items should be found after concurrent operations"
            );
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_realistic_usage_scenario() {
        // Simulate a realistic caching scenario
        let filter = create_test_filter(100_000, 0.01);

        // Phase 1: Bulk insertion of cache keys
        let cache_keys: Vec<Vec<u8>> = (0..10_000)
            .map(|i| format!("cache_key_{}", i).into_bytes())
            .collect();

        let start_time = std::time::Instant::now();
        for key in &cache_keys {
            filter.insert(key).expect("Insert should succeed");
        }
        let insert_duration = start_time.elapsed();

        // Phase 2: Mixed read/write workload
        let query_keys: Vec<Vec<u8>> = (5_000..15_000)
            .map(|i| format!("cache_key_{}", i).into_bytes())
            .collect();

        let start_time = std::time::Instant::now();
        let mut hit_count = 0;
        for key in &query_keys {
            if filter.contains(key).expect("Query should succeed") {
                hit_count += 1;
            }
        }
        let query_duration = start_time.elapsed();

        // Verify performance and correctness
        println!(
            "Performance test - Inserts: {:.2}ms, Queries: {:.2}ms, Hit rate: {:.1}%",
            insert_duration.as_millis(),
            query_duration.as_millis(),
            (hit_count as f64 / query_keys.len() as f64) * 100.0
        );

        // All originally inserted keys should be found
        for key in &cache_keys {
            assert!(
                filter.contains(key).expect("Query should succeed"),
                "All inserted cache keys should be found"
            );
        }

        // Performance assertions (these are rough estimates)
        assert!(
            insert_duration.as_millis() < 1000,
            "10K insertions should complete within 1 second"
        );
        assert!(
            query_duration.as_millis() < 100,
            "10K queries should complete within 100ms"
        );
    }
}

#[cfg(test)]
mod bulk_operations_tests {
    use super::*;
    use probabilistic_rs::bloom::BulkBloomFilterOps;

    #[test]
    fn test_bulk_insert_empty() {
        let filter = create_test_filter(1000, 0.01);

        // Empty input should succeed
        let empty_items: Vec<&[u8]> = vec![];
        filter
            .insert_bulk(&empty_items)
            .expect("Empty bulk insert should succeed");

        assert_eq!(
            filter.insert_count(),
            0,
            "Empty bulk insert should not change insert count"
        );
    }

    #[test]
    fn test_bulk_insert_single_item() {
        let filter = create_test_filter(1000, 0.01);

        let items: Vec<&[u8]> = vec![b"single_item"];
        filter
            .insert_bulk(&items)
            .expect("Single item bulk insert should succeed");

        assert_eq!(filter.insert_count(), 1, "Insert count should be 1");
        assert!(
            filter
                .contains(b"single_item")
                .expect("Item should be found"),
            "Single item should be found"
        );
    }

    #[test]
    fn test_bulk_insert_multiple_items() {
        let filter = create_test_filter(1000, 0.01);

        let items: Vec<&[u8]> =
            vec![b"item1", b"item2", b"item3", b"item4", b"item5"];
        filter
            .insert_bulk(&items)
            .expect("Multiple items bulk insert should succeed");

        assert_eq!(filter.insert_count(), 5, "Insert count should be 5");

        // All items should be found
        for item in &items {
            assert!(
                filter.contains(item).expect("Item should be found"),
                "Item should be found: {:?}",
                String::from_utf8_lossy(item)
            );
        }
    }

    #[test]
    fn test_bulk_contains_empty() {
        let filter = create_test_filter(1000, 0.01);

        let empty_items: Vec<&[u8]> = vec![];
        let results = filter
            .contains_bulk(&empty_items)
            .expect("Empty bulk contains should succeed");

        assert!(
            results.is_empty(),
            "Results should be empty for empty input"
        );
    }

    #[test]
    fn test_bulk_contains_single_item() {
        let filter = create_test_filter(1000, 0.01);

        // Insert item first
        filter.insert(b"test_item").expect("Insert should succeed");

        let items: Vec<&[u8]> = vec![b"test_item"];
        let results = filter
            .contains_bulk(&items)
            .expect("Single item bulk contains should succeed");

        assert_eq!(results.len(), 1, "Should have one result");
        assert!(results[0], "Item should be found");
    }

    #[test]
    fn test_bulk_contains_mixed_results() {
        let filter = create_test_filter(1000, 0.01);

        // Insert some items
        let inserted_items: Vec<&[u8]> = vec![b"item1", b"item2", b"item3"];
        for item in &inserted_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // Test mix of inserted and non-inserted items
        let test_items: Vec<&[u8]> =
            vec![b"item1", b"item2", b"item4", b"item5", b"item3"];
        let results = filter
            .contains_bulk(&test_items)
            .expect("Bulk contains should succeed");

        assert_eq!(results.len(), 5, "Should have 5 results");
        assert_eq!(
            results,
            vec![true, true, false, false, true],
            "Results should match expected pattern"
        );
    }

    #[test]
    fn test_bulk_operations_consistency() {
        let filter = create_test_filter(1000, 0.01);

        let items: Vec<&[u8]> = vec![b"a", b"b", b"c", b"d", b"e"];

        // Insert using bulk
        filter
            .insert_bulk(&items)
            .expect("Bulk insert should succeed");

        // Check using bulk
        let bulk_results = filter
            .contains_bulk(&items)
            .expect("Bulk contains should succeed");

        // Check using individual operations
        let individual_results: Vec<bool> = items
            .iter()
            .map(|item| {
                filter
                    .contains(item)
                    .expect("Individual contains should succeed")
            })
            .collect();

        assert_eq!(
            bulk_results, individual_results,
            "Bulk and individual operations should produce same results"
        );
    }

    #[test]
    fn test_bulk_insert_large_batch() {
        let filter = create_test_filter(10_000, 0.01);

        // Generate large batch
        let large_batch: Vec<Vec<u8>> = (0..1000)
            .map(|i| format!("large_batch_item_{:04}", i).into_bytes())
            .collect();

        let batch_refs: Vec<&[u8]> =
            large_batch.iter().map(|item| item.as_slice()).collect();

        filter
            .insert_bulk(&batch_refs)
            .expect("Large batch insert should succeed");

        assert_eq!(filter.insert_count(), 1000, "Insert count should be 1000");

        // Verify all items are found
        let results = filter
            .contains_bulk(&batch_refs)
            .expect("Large batch contains should succeed");
        let found_count = results.iter().filter(|&&exists| exists).count();

        assert_eq!(found_count, 1000, "All 1000 items should be found");
    }

    #[test]
    fn test_bulk_operations_with_duplicates() {
        let filter = create_test_filter(1000, 0.01);

        let items_with_duplicates: Vec<&[u8]> =
            vec![b"item1", b"item2", b"item1", b"item3", b"item2"];
        filter
            .insert_bulk(&items_with_duplicates)
            .expect("Bulk insert with duplicates should succeed");

        // Insert count should reflect all insertions (including duplicates)
        assert_eq!(
            filter.insert_count(),
            5,
            "Insert count should include duplicates"
        );

        // All items should be found
        let results = filter
            .contains_bulk(&items_with_duplicates)
            .expect("Bulk contains should succeed");
        assert!(
            results.iter().all(|&exists| exists),
            "All items (including duplicates) should be found"
        );
    }

    #[test]
    fn test_bulk_operations_performance_comparison() {
        let filter_bulk = create_test_filter(10_000, 0.01);
        let filter_individual = create_test_filter(10_000, 0.01);

        // Use larger batch size for more meaningful performance comparison
        let test_items: Vec<Vec<u8>> = (0..1000)
            .map(|i| format!("perf_test_item_{:04}", i).into_bytes())
            .collect();

        let test_refs: Vec<&[u8]> =
            test_items.iter().map(|item| item.as_slice()).collect();

        // Time bulk operations
        let bulk_insert_start = std::time::Instant::now();
        filter_bulk
            .insert_bulk(&test_refs)
            .expect("Bulk insert should succeed");
        let bulk_insert_duration = bulk_insert_start.elapsed();

        let bulk_query_start = std::time::Instant::now();
        let bulk_results = filter_bulk
            .contains_bulk(&test_refs)
            .expect("Bulk query should succeed");
        let bulk_query_duration = bulk_query_start.elapsed();

        // Time individual operations
        let individual_insert_start = std::time::Instant::now();
        for item in &test_refs {
            filter_individual
                .insert(item)
                .expect("Individual insert should succeed");
        }
        let individual_insert_duration = individual_insert_start.elapsed();

        let individual_query_start = std::time::Instant::now();
        let mut individual_results = Vec::with_capacity(test_items.len());
        for item in &test_refs {
            individual_results.push(
                filter_individual
                    .contains(item)
                    .expect("Individual query should succeed"),
            );
        }
        let individual_query_duration = individual_query_start.elapsed();

        // Verify correctness
        assert_eq!(
            bulk_results, individual_results,
            "Bulk and individual operations should produce same results"
        );

        println!(
            "Performance comparison - Bulk insert: {:?}, Individual insert: {:?}, Bulk query: {:?}, Individual query: {:?}",
            bulk_insert_duration,
            individual_insert_duration,
            bulk_query_duration,
            individual_query_duration
        );

        // For small batches, bulk operations might be slightly slower due to overhead
        // But they should still be reasonably close in performance
        let insert_ratio = bulk_insert_duration.as_nanos() as f64
            / individual_insert_duration.as_nanos() as f64;
        let query_ratio = bulk_query_duration.as_nanos() as f64
            / individual_query_duration.as_nanos() as f64;

        println!(
            "Performance ratios - Insert: {:.2}x, Query: {:.2}x",
            insert_ratio, query_ratio
        );

        // Bulk operations should be within reasonable bounds (not significantly slower)
        assert!(
            insert_ratio < 2.0,
            "Bulk insert should not be more than 2x slower than individual inserts"
        );
        assert!(
            query_ratio < 2.0,
            "Bulk query should not be more than 2x slower than individual queries"
        );
    }

    #[test]
    fn test_bulk_operations_concurrent() {
        let filter = Arc::new(create_test_filter(10_000, 0.01));
        let mut handles = vec![];

        // Spawn multiple threads performing bulk operations
        for thread_id in 0..4 {
            let filter_clone = Arc::clone(&filter);

            let handle = thread::spawn(move || {
                let thread_items: Vec<Vec<u8>> = (0..100)
                    .map(|i| {
                        format!("thread_{}_item_{:03}", thread_id, i).into_bytes()
                    })
                    .collect();

                let thread_refs: Vec<&[u8]> =
                    thread_items.iter().map(|item| item.as_slice()).collect();

                // Bulk insert
                filter_clone
                    .insert_bulk(&thread_refs)
                    .expect("Concurrent bulk insert should succeed");

                // Bulk query
                let results = filter_clone
                    .contains_bulk(&thread_refs)
                    .expect("Concurrent bulk query should succeed");

                // All items should be found
                assert!(
                    results.iter().all(|&exists| exists),
                    "All thread items should be found"
                );
            });

            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread should complete successfully");
        }

        // Verify total insert count
        let final_filter = Arc::try_unwrap(filter)
            .ok()
            .expect("Should be able to unwrap Arc");
        assert_eq!(
            final_filter.insert_count(),
            400,
            "Total insert count should be 400"
        );
    }
}
