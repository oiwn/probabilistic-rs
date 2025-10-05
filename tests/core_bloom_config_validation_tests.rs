use probablistic_rs::bloom::{
    config::{
        BloomFilterConfig, BloomFilterConfigBuilder, PersistenceConfigBuilder,
    },
    error::BloomError,
};
use std::{fs, path::PathBuf, time::Duration};

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(test_name: &str) -> Self {
        let path =
            PathBuf::from(format!("test_config_validation_{}.fjall", test_name));
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod capacity_validation_tests {
    use super::*;

    #[test]
    fn test_zero_capacity_fails() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(0)
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("Capacity must be > 0"));
            }
            _ => panic!("Expected InvalidConfig error for zero capacity"),
        }
    }

    #[test]
    fn test_minimum_valid_capacity() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1)
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_large_capacity_succeeds() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(100_000_000)
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_very_large_capacity_succeeds() {
        // Test with usize::MAX / 2 to avoid potential overflow issues
        let large_capacity = usize::MAX / 2;
        let config = BloomFilterConfigBuilder::default()
            .capacity(large_capacity)
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        assert!(config.validate().is_ok());
    }
}

#[cfg(test)]
mod false_positive_rate_validation_tests {
    use super::*;

    #[test]
    fn test_zero_fpr_fails() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.0)
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("FPR must be between 0 and 1"));
            }
            _ => panic!("Expected InvalidConfig error for zero FPR"),
        }
    }

    #[test]
    fn test_one_fpr_fails() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(1.0)
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("FPR must be between 0 and 1"));
            }
            _ => panic!("Expected InvalidConfig error for FPR = 1.0"),
        }
    }

    #[test]
    fn test_negative_fpr_fails() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(-0.1)
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("FPR must be between 0 and 1"));
            }
            _ => panic!("Expected InvalidConfig error for negative FPR"),
        }
    }

    #[test]
    fn test_greater_than_one_fpr_fails() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(1.5)
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("FPR must be between 0 and 1"));
            }
            _ => panic!("Expected InvalidConfig error for FPR > 1.0"),
        }
    }

    #[test]
    fn test_boundary_fpr_values_succeed() {
        // Test very small positive FPR
        let config1 = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.000001)
            .build()
            .unwrap();
        assert!(config1.validate().is_ok());

        // Test close to 1 but not 1
        let config2 = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.999999)
            .build()
            .unwrap();
        assert!(config2.validate().is_ok());
    }

    #[test]
    fn test_common_valid_fpr() {
        let config = BloomFilterConfigBuilder::default()
            .capacity(1000)
            .false_positive_rate(0.01)
            .build()
            .unwrap();

        assert!(config.validate().is_ok());
    }
}

#[cfg(test)]
mod configuration_combinations_tests {
    use super::*;

    #[test]
    fn test_valid_combinations() {
        let test_cases =
            vec![(1_000, 0.01), (10_000, 0.001), (100_000, 0.1), (1, 0.5)];

        for (capacity, fpr) in test_cases {
            let config = BloomFilterConfigBuilder::default()
                .capacity(capacity)
                .false_positive_rate(fpr)
                .build()
                .unwrap();

            assert!(
                config.validate().is_ok(),
                "Valid combination should pass: capacity={}, fpr={}",
                capacity,
                fpr
            );
        }
    }

    #[test]
    fn test_extreme_but_valid_combinations() {
        // Very large capacity with very small FPR
        let config1 = BloomFilterConfigBuilder::default()
            .capacity(10_000_000)
            .false_positive_rate(0.0001)
            .build()
            .unwrap();
        assert!(config1.validate().is_ok());

        // Small capacity with large FPR
        let config2 = BloomFilterConfigBuilder::default()
            .capacity(10)
            .false_positive_rate(0.9)
            .build()
            .unwrap();
        assert!(config2.validate().is_ok());
    }
}

#[cfg(test)]
mod builder_error_tests {
    use super::*;

    #[test]
    fn test_bloom_filter_config_builder_defaults() {
        // Test that builder works with all defaults
        let config = BloomFilterConfigBuilder::default().build().unwrap();

        assert_eq!(config.capacity, 1_000_000);
        assert_eq!(config.false_positive_rate, 0.01);
        assert!(config.persistence.is_none());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_persistence_config_builder_requires_db_path() {
        // db_path is required, so builder should require it
        let result = PersistenceConfigBuilder::default().build();

        assert!(
            result.is_err(),
            "Builder should fail when required db_path is missing"
        );
    }

    #[test]
    fn test_persistence_config_builder_with_valid_values() {
        let test_db = TestDb::new("builder_valid");

        let config = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .build()
            .unwrap();

        assert_eq!(config.db_path, *test_db.path());
        assert_eq!(config.snapshot_interval, Duration::from_secs(60));
        assert_eq!(config.chunk_size_bytes, 4096);
        assert!(!config.auto_snapshot);
    }

    #[test]
    fn test_builder_with_custom_values() {
        let test_db = TestDb::new("builder_custom");

        let persistence = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .snapshot_interval(Duration::from_secs(30))
            .chunk_size_bytes(8192)
            .auto_snapshot(true)
            .build()
            .unwrap();

        let config = BloomFilterConfigBuilder::default()
            .capacity(50_000)
            .false_positive_rate(0.005)
            .persistence(Some(persistence))
            .build()
            .unwrap();

        assert_eq!(config.capacity, 50_000);
        assert_eq!(config.false_positive_rate, 0.005);
        assert!(config.persistence.is_some());

        let p = config.persistence.as_ref().unwrap();
        assert_eq!(p.snapshot_interval, Duration::from_secs(30));
        assert_eq!(p.chunk_size_bytes, 8192);
        assert!(p.auto_snapshot);

        assert!(config.validate().is_ok());
    }
}

#[cfg(test)]
mod persistence_config_validation_tests {
    use super::*;

    #[test]
    fn test_empty_path_validation() {
        let config = PersistenceConfigBuilder::default()
            .db_path(PathBuf::from(""))
            .build()
            .unwrap();

        // Empty path should be valid at config level (validation happens during use)
        assert_eq!(config.db_path, PathBuf::from(""));
    }

    #[test]
    fn test_minimum_chunk_size() {
        let test_db = TestDb::new("min_chunk");

        // Test minimum chunk size (512 bytes as per spec)
        let config = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .chunk_size_bytes(512)
            .build()
            .unwrap();

        assert_eq!(config.chunk_size_bytes, 512);
    }

    #[test]
    fn test_small_chunk_size() {
        let test_db = TestDb::new("small_chunk");

        // Very small chunk size should be allowed at config level
        let config = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .chunk_size_bytes(256)
            .build()
            .unwrap();

        assert_eq!(config.chunk_size_bytes, 256);
    }

    #[test]
    fn test_large_chunk_size() {
        let test_db = TestDb::new("large_chunk");

        // Large chunk size should work
        let config = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .chunk_size_bytes(1_048_576) // 1MB
            .build()
            .unwrap();

        assert_eq!(config.chunk_size_bytes, 1_048_576);
    }

    #[test]
    fn test_various_paths() {
        // Test relative path
        let config1 = PersistenceConfigBuilder::default()
            .db_path(PathBuf::from("./test.db"))
            .build()
            .unwrap();
        assert_eq!(config1.db_path, PathBuf::from("./test.db"));

        // Test absolute path
        let config2 = PersistenceConfigBuilder::default()
            .db_path(PathBuf::from("/tmp/test.db"))
            .build()
            .unwrap();
        assert_eq!(config2.db_path, PathBuf::from("/tmp/test.db"));

        // Test path with special characters
        let config3 = PersistenceConfigBuilder::default()
            .db_path(PathBuf::from("test-db_2024.db"))
            .build()
            .unwrap();
        assert_eq!(config3.db_path, PathBuf::from("test-db_2024.db"));
    }

    #[test]
    fn test_snapshot_interval_values() {
        let test_db = TestDb::new("intervals");

        // Very short interval
        let config1 = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .snapshot_interval(Duration::from_millis(1))
            .build()
            .unwrap();
        assert_eq!(config1.snapshot_interval, Duration::from_millis(1));

        // Very long interval
        let config2 = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .snapshot_interval(Duration::from_secs(3600)) // 1 hour
            .build()
            .unwrap();
        assert_eq!(config2.snapshot_interval, Duration::from_secs(3600));

        // Zero duration
        let config3 = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .snapshot_interval(Duration::from_secs(0))
            .build()
            .unwrap();
        assert_eq!(config3.snapshot_interval, Duration::from_secs(0));
    }

    #[test]
    fn test_auto_snapshot_combinations() {
        let test_db = TestDb::new("auto_snap");

        // Auto snapshot enabled with short interval
        let config1 = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .auto_snapshot(true)
            .snapshot_interval(Duration::from_secs(5))
            .build()
            .unwrap();

        assert!(config1.auto_snapshot);
        assert_eq!(config1.snapshot_interval, Duration::from_secs(5));

        // Auto snapshot disabled with long interval
        let config2 = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .auto_snapshot(false)
            .snapshot_interval(Duration::from_secs(3600))
            .build()
            .unwrap();

        assert!(!config2.auto_snapshot);
        assert_eq!(config2.snapshot_interval, Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;

    #[test]
    fn test_basic_config_serialization_round_trip() {
        let original = BloomFilterConfigBuilder::default()
            .capacity(50_000)
            .false_positive_rate(0.005)
            .build()
            .unwrap();

        // Serialize to bytes
        let bytes = original.to_bytes().unwrap();
        assert!(!bytes.is_empty());

        // Deserialize back
        let deserialized = BloomFilterConfig::from_bytes(&bytes).unwrap();

        // Verify round-trip accuracy
        assert_eq!(deserialized.capacity, original.capacity);
        assert_eq!(
            deserialized.false_positive_rate,
            original.false_positive_rate
        );
        assert_eq!(
            deserialized.persistence.is_none(),
            original.persistence.is_none()
        );
        assert!(deserialized.validate().is_ok());
    }

    #[test]
    fn test_config_with_persistence_serialization() {
        let test_db = TestDb::new("persist_serial");

        let persistence = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .snapshot_interval(Duration::from_secs(30))
            .chunk_size_bytes(8192)
            .auto_snapshot(true)
            .build()
            .unwrap();

        let original = BloomFilterConfigBuilder::default()
            .capacity(100_000)
            .false_positive_rate(0.001)
            .persistence(Some(persistence))
            .build()
            .unwrap();

        // Serialize and deserialize
        let bytes = original.to_bytes().unwrap();
        let deserialized = BloomFilterConfig::from_bytes(&bytes).unwrap();

        // Verify main config
        assert_eq!(deserialized.capacity, original.capacity);
        assert_eq!(
            deserialized.false_positive_rate,
            original.false_positive_rate
        );

        // Verify persistence config
        assert!(deserialized.persistence.is_some());
        let orig_p = original.persistence.as_ref().unwrap();
        let deser_p = deserialized.persistence.as_ref().unwrap();

        assert_eq!(deser_p.db_path, orig_p.db_path);
        assert_eq!(deser_p.snapshot_interval, orig_p.snapshot_interval);
        assert_eq!(deser_p.chunk_size_bytes, orig_p.chunk_size_bytes);
        assert_eq!(deser_p.auto_snapshot, orig_p.auto_snapshot);

        assert!(deserialized.validate().is_ok());
    }

    #[test]
    fn test_invalid_serialization_data() {
        // Test with completely invalid data
        let invalid_bytes = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result = BloomFilterConfig::from_bytes(&invalid_bytes);

        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::SerializationError(_) => {}
            _ => panic!("Expected SerializationError for invalid data"),
        }
    }

    #[test]
    fn test_empty_serialization_data() {
        let result = BloomFilterConfig::from_bytes(&[]);

        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::SerializationError(_) => {}
            _ => panic!("Expected SerializationError for empty data"),
        }
    }

    #[test]
    fn test_extreme_values_serialization() {
        let original = BloomFilterConfigBuilder::default()
            .capacity(usize::MAX / 2)
            .false_positive_rate(0.000001)
            .build()
            .unwrap();

        let bytes = original.to_bytes().unwrap();
        let deserialized = BloomFilterConfig::from_bytes(&bytes).unwrap();

        assert_eq!(deserialized.capacity, original.capacity);
        assert_eq!(
            deserialized.false_positive_rate,
            original.false_positive_rate
        );
        assert!(deserialized.validate().is_ok());
    }

    #[test]
    fn test_json_serialization_compatibility() {
        use serde_json;

        let test_db = TestDb::new("json_serial");

        let persistence = PersistenceConfigBuilder::default()
            .db_path(test_db.path().clone())
            .build()
            .unwrap();

        let original = BloomFilterConfigBuilder::default()
            .capacity(10_000)
            .false_positive_rate(0.02)
            .persistence(Some(persistence))
            .build()
            .unwrap();

        // Test JSON serialization
        let json_str = serde_json::to_string(&original).unwrap();
        assert!(!json_str.is_empty());

        // Test JSON deserialization
        let deserialized: BloomFilterConfig =
            serde_json::from_str(&json_str).unwrap();

        assert_eq!(deserialized.capacity, original.capacity);
        assert_eq!(
            deserialized.false_positive_rate,
            original.false_positive_rate
        );
        assert!(deserialized.persistence.is_some());
        assert!(deserialized.validate().is_ok());
    }
}

#[cfg(test)]
mod error_recovery_tests {
    use super::*;

    #[test]
    fn test_multiple_validation_errors_reported() {
        // Create config with multiple validation issues
        let config = BloomFilterConfigBuilder::default()
            .capacity(0) // Invalid
            .false_positive_rate(-0.5) // Invalid
            .build()
            .unwrap();

        let result = config.validate();
        assert!(result.is_err());

        // Should fail on the first error encountered (capacity)
        match result.unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("Capacity must be > 0"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[test]
    fn test_config_creation_with_invalid_values() {
        // Test that invalid values can be stored in config but fail validation
        let config = BloomFilterConfig {
            capacity: 0,
            false_positive_rate: 2.0,
            persistence: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_error_message_quality() {
        // Test that error messages are helpful
        let config1 = BloomFilterConfig {
            capacity: 0,
            false_positive_rate: 0.5,
            persistence: None,
        };

        match config1.validate().unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("Capacity"));
                assert!(msg.contains("0"));
            }
            _ => panic!("Expected InvalidConfig with helpful message"),
        }

        let config2 = BloomFilterConfig {
            capacity: 1000,
            false_positive_rate: 1.5,
            persistence: None,
        };

        match config2.validate().unwrap_err() {
            BloomError::InvalidConfig(msg) => {
                assert!(msg.contains("FPR"));
                assert!(msg.contains("between 0 and 1"));
            }
            _ => panic!("Expected InvalidConfig with helpful message"),
        }
    }

    #[test]
    fn test_graceful_handling_of_extreme_values() {
        // Test config with extreme but technically valid values
        let config = BloomFilterConfig {
            capacity: 1,
            false_positive_rate: 0.99999,
            persistence: None,
        };

        // Should validate successfully despite being impractical
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_serialization_error_recovery() {
        // Create a valid config
        let original = BloomFilterConfigBuilder::default().build().unwrap();

        // Serialize it
        let mut bytes = original.to_bytes().unwrap();

        // Corrupt the serialized data
        if !bytes.is_empty() {
            bytes[0] = bytes[0].wrapping_add(1); // Change first byte
        }

        // Attempt to deserialize
        let result = BloomFilterConfig::from_bytes(&bytes);

        // Should fail gracefully with SerializationError
        assert!(result.is_err());
        match result.unwrap_err() {
            BloomError::SerializationError(msg) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected SerializationError for corrupted data"),
        }
    }

    #[test]
    fn test_default_fallback_behavior() {
        // Test that defaults work as expected
        let config = BloomFilterConfigBuilder::default().build().unwrap();

        // Verify defaults are sensible
        assert!(config.capacity > 0);
        assert!(
            config.false_positive_rate > 0.0 && config.false_positive_rate < 1.0
        );
        assert!(config.persistence.is_none()); // Default to no persistence

        assert!(config.validate().is_ok());
    }
}
