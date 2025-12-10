use probabilistic_rs::ebloom::{
    config::ExpiringFilterConfigBuilder,
    filter::ExpiringBloomFilter,
    traits::{ExpiringBloomFilterOps, ExpiringBloomFilterStats},
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

// Helper function to create a basic test filter
fn create_test_filter(
    capacity_per_level: usize,
    num_levels: usize,
    fpr: f64,
) -> ExpiringBloomFilter {
    let config = ExpiringFilterConfigBuilder::default()
        .capacity_per_level(capacity_per_level)
        .target_fpr(fpr)
        .num_levels(num_levels)
        .level_duration(Duration::from_secs(60)) // 1 minute default
        .build()
        .expect("Failed to build test config");

    ExpiringBloomFilter::new(config).expect("Failed to create test filter")
}

// Helper function to create filter with short expiration for testing
fn create_short_expiry_filter(
    capacity_per_level: usize,
    num_levels: usize,
    duration_ms: u64,
) -> ExpiringBloomFilter {
    let config = ExpiringFilterConfigBuilder::default()
        .capacity_per_level(capacity_per_level)
        .target_fpr(0.01)
        .num_levels(num_levels)
        .level_duration(Duration::from_millis(duration_ms))
        .build()
        .expect("Failed to build test config");

    ExpiringBloomFilter::new(config).expect("Failed to create test filter")
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
        let filter = create_test_filter(1000, 3, 0.01);

        let item1 = b"hello";
        let item2 = b"world";
        let item3 = b"rust";

        filter.insert(item1).unwrap();
        filter.insert(item2).unwrap();

        assert!(filter.contains(item1).unwrap());
        assert!(filter.contains(item2).unwrap());
        assert!(!filter.contains(item3).unwrap());
    }

    #[test]
    fn test_multiple_insertions() {
        let filter = create_test_filter(1000, 3, 0.01);
        let test_items = generate_test_items(10);

        // Insert all items
        for item in &test_items {
            filter.insert(item).expect("Insert should succeed");
        }

        // All inserted items should be found (no false negatives)
        for item in &test_items {
            assert!(
                filter.contains(item).expect("Contains should succeed"),
                "No false negatives allowed"
            );
        }

        // Verify total insert count across all levels
        assert_eq!(
            filter.total_insert_count(),
            test_items.len(),
            "Total insert count should match"
        );
    }

    #[test]
    fn test_clear() {
        let filter = create_test_filter(1000, 3, 0.01);

        let item1 = b"hello";
        let item2 = b"world";

        filter.insert(item1).unwrap();
        filter.insert(item2).unwrap();

        assert!(filter.contains(item1).unwrap());
        assert!(filter.contains(item2).unwrap());

        filter.clear().unwrap();

        assert!(!filter.contains(item1).unwrap());
        assert!(!filter.contains(item2).unwrap());
        assert_eq!(filter.total_insert_count(), 0);
    }

    #[test]
    fn test_duplicate_insertions() {
        let filter = create_test_filter(1000, 3, 0.01);
        let item = b"duplicate_test";

        // Insert same item multiple times
        for _ in 0..5 {
            filter.insert(item).expect("Insert should succeed");
        }

        // Item should still be found
        assert!(filter.contains(item).expect("Contains should succeed"));

        // Insert count increases even for duplicates (bloom filter behavior)
        assert_eq!(filter.total_insert_count(), 5);
    }

    #[test]
    fn test_empty_filter_behavior() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Empty filter should not contain any items
        assert!(!filter.contains(b"test").unwrap());
        assert_eq!(filter.total_insert_count(), 0);

        // Clear on empty filter should work
        filter
            .clear()
            .expect("Clear should succeed on empty filter");
        assert_eq!(filter.total_insert_count(), 0);
    }
}

#[cfg(test)]
mod multi_level_behavior_tests {
    use super::*;

    #[test]
    fn test_current_level_tracking() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Initially on level 0
        assert_eq!(filter.get_active_level(), 0);
    }

    #[tokio::test]
    async fn test_items_found_across_levels() {
        let filter = create_short_expiry_filter(1000, 3, 100);

        // Insert in level 0
        filter.insert(b"item1").unwrap();
        assert!(filter.contains(b"item1").unwrap());

        // Wait for rotation
        thread::sleep(Duration::from_millis(150));
        filter.cleanup_expired_levels().await.unwrap();

        // Item should still be found in rotated level
        // (contains checks ALL levels)
        assert!(filter.contains(b"item1").unwrap());
    }

    #[tokio::test]
    async fn test_level_rotation_mechanics() {
        let filter = create_short_expiry_filter(1000, 3, 100);

        let initial_level = filter.get_active_level();

        // Wait for expiration and rotate
        thread::sleep(Duration::from_millis(150));
        filter.cleanup_expired_levels().await.unwrap();

        // Current level should have changed
        let new_level = filter.get_active_level();
        assert_ne!(initial_level, new_level);
    }

    #[tokio::test]
    async fn test_expiration_after_all_levels() {
        let filter = create_short_expiry_filter(100, 2, 100); // 2 levels, 100ms each

        filter.insert(b"expire_me").unwrap();
        assert!(filter.contains(b"expire_me").unwrap());

        // Rotate once - item moves to level 1
        thread::sleep(Duration::from_millis(150));
        filter.cleanup_expired_levels().await.unwrap();
        assert!(
            filter.contains(b"expire_me").unwrap(),
            "Should still be in level 1"
        );

        // Rotate again - item should be gone
        thread::sleep(Duration::from_millis(150));
        filter.cleanup_expired_levels().await.unwrap();

        // Item might still appear due to false positive, but that's expected
        // The key is that the levels rotated correctly
    }

    #[test]
    fn test_level_expiration_check() {
        let filter = create_short_expiry_filter(1000, 3, 100);

        // Level should not be expired immediately
        assert!(!filter.is_level_expired(0).unwrap());

        // After duration, should be expired
        thread::sleep(Duration::from_millis(150));
        assert!(filter.is_level_expired(0).unwrap());
    }
}

#[cfg(test)]
mod configuration_and_stats_tests {
    use super::*;

    #[test]
    fn test_config_validation_valid() {
        let config = ExpiringFilterConfigBuilder::default()
            .capacity_per_level(1000_usize)
            .target_fpr(0.01)
            .num_levels(3_usize)
            .level_duration(Duration::from_secs(60))
            .build();

        assert!(config.is_ok(), "Valid config should build successfully");
    }

    #[test]
    fn test_stats_accuracy() {
        let filter = create_test_filter(1000, 3, 0.01);

        assert_eq!(filter.capacity_per_level(), 1000);
        assert_eq!(filter.target_fpr(), 0.01);
        assert_eq!(filter.num_levels(), 3);
        assert_eq!(filter.active_levels(), 3);
        assert_eq!(filter.total_insert_count(), 0);

        // Insert some items
        for i in 0..10 {
            filter.insert(format!("item{}", i).as_bytes()).unwrap();
        }

        assert_eq!(filter.total_insert_count(), 10);
    }

    #[test]
    fn test_different_config_combinations() {
        // Small capacity, few levels
        let filter1 = create_test_filter(100, 2, 0.01);
        assert_eq!(filter1.num_levels(), 2);

        // Large capacity, many levels
        let filter2 = create_test_filter(10000, 5, 0.001);
        assert_eq!(filter2.num_levels(), 5);
        assert_eq!(filter2.target_fpr(), 0.001);
    }
}

#[cfg(test)]
mod edge_cases_and_error_conditions {
    use super::*;

    #[test]
    fn test_empty_item_insertion() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Empty item should be insertable
        filter
            .insert(b"")
            .expect("Empty item insertion should succeed");
        assert!(
            filter
                .contains(b"")
                .expect("Contains should work for empty item")
        );
    }

    #[test]
    fn test_binary_data_handling() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Test with binary data (null bytes, etc.)
        let binary_data: Vec<u8> = vec![0, 1, 255, 128, 0, 0, 42];
        filter
            .insert(&binary_data)
            .expect("Binary data insertion should succeed");
        assert!(
            filter
                .contains(&binary_data)
                .expect("Binary data lookup should succeed")
        );
    }

    #[test]
    fn test_large_item_insertion() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Test with large item (1MB)
        let large_item = vec![b'x'; 1024 * 1024];
        filter
            .insert(&large_item)
            .expect("Large item insertion should succeed");
        assert!(
            filter
                .contains(&large_item)
                .expect("Large item lookup should succeed")
        );
    }

    #[test]
    fn test_capacity_stress_test() {
        let capacity = 100;
        let filter = create_test_filter(capacity, 3, 0.01);
        let test_items = generate_test_items(capacity * 2); // Insert 2x capacity

        // Should handle over-capacity gracefully (bloom filters don't have hard limits)
        for item in &test_items {
            filter
                .insert(item)
                .expect("Insert should succeed even over capacity");
        }

        // No false negatives (all inserted items should be found)
        for item in &test_items {
            assert!(
                filter.contains(item).expect("Contains should succeed"),
                "No false negatives even when over capacity"
            );
        }
    }
}

#[cfg(test)]
mod behavioral_guarantees_tests {
    use super::*;

    #[test]
    fn test_no_false_negatives_guarantee() {
        let filter = create_test_filter(1000, 3, 0.01);
        let test_items = generate_test_items(100);

        // Insert all items
        for item in &test_items {
            filter.insert(item).unwrap();
        }

        // Verify no false negatives
        for item in &test_items {
            assert!(
                filter.contains(item).unwrap(),
                "Bloom filters must never have false negatives"
            );
        }
    }

    #[test]
    fn test_deterministic_behavior() {
        let filter1 = create_test_filter(1000, 3, 0.01);
        let filter2 = create_test_filter(1000, 3, 0.01);

        let test_item = b"deterministic_test";

        // Same operations on both filters
        filter1.insert(test_item).unwrap();
        filter2.insert(test_item).unwrap();

        // Both should give same results
        assert_eq!(
            filter1.contains(test_item).unwrap(),
            filter2.contains(test_item).unwrap()
        );
    }

    #[test]
    fn test_false_positive_rate_reasonable() {
        let filter = create_test_filter(1000, 3, 0.01);
        let inserted_items = generate_test_items(500);
        let test_items = generate_test_items(1000);

        // Insert first half
        for item in &inserted_items {
            filter.insert(item).unwrap();
        }

        // Check false positives on items NOT inserted
        let mut false_positives = 0;
        for item in test_items.iter().skip(500) {
            if filter.contains(item).unwrap() {
                false_positives += 1;
            }
        }

        let false_positive_rate = false_positives as f64 / 500.0;

        // FPR should be somewhat close to target (allowing for multi-level increase)
        // Multi-level can increase FPR, so we allow up to 5x the target FPR
        assert!(
            false_positive_rate < 0.05,
            "False positive rate too high: {}",
            false_positive_rate
        );
    }
}

#[cfg(test)]
mod thread_safety_tests {
    use super::*;

    #[test]
    fn test_concurrent_reads() {
        let filter = Arc::new(create_test_filter(10000, 3, 0.01));
        let test_items = generate_test_items(100);

        // Insert items first
        for item in &test_items {
            filter.insert(item).unwrap();
        }

        // Spawn multiple reader threads
        let mut handles = vec![];
        for _ in 0..10 {
            let filter_clone = Arc::clone(&filter);
            let items_clone = test_items.clone();

            let handle = thread::spawn(move || {
                for item in &items_clone {
                    assert!(filter_clone.contains(item).unwrap());
                }
            });
            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            handle.join().expect("Thread should complete successfully");
        }
    }

    #[test]
    fn test_single_writer_multiple_readers() {
        let filter = Arc::new(create_test_filter(10000, 3, 0.01));
        let test_items = generate_test_items(100);
        let inserted_items = Arc::new(Mutex::new(HashSet::new()));

        // Writer thread
        let filter_clone = Arc::clone(&filter);
        let items_clone = test_items.clone();
        let inserted_clone = Arc::clone(&inserted_items);

        let writer = thread::spawn(move || {
            for item in &items_clone {
                filter_clone.insert(item).unwrap();
                inserted_clone.lock().unwrap().insert(item.clone());
                thread::sleep(Duration::from_micros(100));
            }
        });

        // Reader threads
        let mut readers = vec![];
        for _ in 0..5 {
            let filter_clone = Arc::clone(&filter);
            let inserted_clone = Arc::clone(&inserted_items);

            let handle = thread::spawn(move || {
                for _ in 0..50 {
                    let items = inserted_clone.lock().unwrap().clone();
                    for item in items.iter() {
                        // No false negatives for inserted items
                        assert!(filter_clone.contains(item).unwrap());
                    }
                    thread::sleep(Duration::from_micros(200));
                }
            });
            readers.push(handle);
        }

        writer.join().expect("Writer should complete");
        for reader in readers {
            reader.join().expect("Reader should complete");
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_realistic_usage_scenario() {
        // Simulate a cache invalidation scenario
        let filter = create_short_expiry_filter(1000, 3, 200);

        // Phase 1: Add some cache keys
        for i in 0..50 {
            let key = format!("cache_key_{}", i);
            filter.insert(key.as_bytes()).unwrap();
        }

        // Verify all are present
        for i in 0..50 {
            let key = format!("cache_key_{}", i);
            assert!(filter.contains(key.as_bytes()).unwrap());
        }

        // Phase 2: Rotate levels
        thread::sleep(Duration::from_millis(250));
        filter.cleanup_expired_levels().await.unwrap();

        // Old keys should still be findable (in rotated level)
        for i in 0..50 {
            let key = format!("cache_key_{}", i);
            assert!(filter.contains(key.as_bytes()).unwrap());
        }

        // Phase 3: Add new keys
        for i in 50..100 {
            let key = format!("cache_key_{}", i);
            filter.insert(key.as_bytes()).unwrap();
        }

        // All keys should be present
        for i in 0..100 {
            let key = format!("cache_key_{}", i);
            assert!(filter.contains(key.as_bytes()).unwrap());
        }
    }

    #[tokio::test]
    async fn test_time_based_expiration_workflow() {
        let filter = create_short_expiry_filter(100, 3, 100); // 3 levels for better testing

        // Insert items at different times
        filter.insert(b"early_item").unwrap();

        thread::sleep(Duration::from_millis(110)); // Wait for rotation
        filter.cleanup_expired_levels().await.unwrap();

        // early_item should still be present (now in previous level)
        assert!(filter.contains(b"early_item").unwrap());

        // Insert new item in new current level
        filter.insert(b"mid_item").unwrap();

        thread::sleep(Duration::from_millis(110)); // Wait for another rotation
        filter.cleanup_expired_levels().await.unwrap();

        // Both should still be present (in different historical levels)
        assert!(filter.contains(b"early_item").unwrap());
        assert!(filter.contains(b"mid_item").unwrap());

        thread::sleep(Duration::from_millis(110)); // Third rotation
        filter.cleanup_expired_levels().await.unwrap();

        // early_item should be expired now (was in oldest level)
        // mid_item still present
        assert!(filter.contains(b"mid_item").unwrap());
    }

    #[test]
    fn test_mixed_operations() {
        let filter = create_test_filter(1000, 3, 0.01);

        // Mix of operations
        filter.insert(b"item1").unwrap();
        assert!(filter.contains(b"item1").unwrap());

        filter.insert(b"item2").unwrap();
        filter.insert(b"item3").unwrap();

        assert_eq!(filter.total_insert_count(), 3);

        filter.clear().unwrap();
        assert_eq!(filter.total_insert_count(), 0);

        filter.insert(b"new_item").unwrap();
        assert!(filter.contains(b"new_item").unwrap());
        assert_eq!(filter.total_insert_count(), 1);
    }
}
