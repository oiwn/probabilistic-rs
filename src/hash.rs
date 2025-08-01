use fnv::FnvHasher;
use murmur3::murmur3_32;
use std::hash::Hasher;
use std::io::Cursor;

/// A type alias for the hash function used in the Bloom filter.
///
/// This function takes an input item and computes multiple hash indices
/// for the Bloom filter's bit vector.
///
/// **Parameters:**
///
/// - `item: &[u8]`
///   - A byte slice representing the item to be hashed.
/// - `num_hashes: usize`
///   - The number of hash values to compute for the item.
/// - `capacity: usize`
///   - The size of the Bloom filter's bit vector. This ensures that
///     the generated hash indices are within valid bounds.
///
/// **Returns:**
///
/// - `Vec<u32>`
///   - A vector of hash indices corresponding to positions in the bit vector.
///
/// **Usage:**
///
/// The hash function computes `num_hashes` hash indices for the given `item`,
/// ensuring each index is within the range `[0, capacity)`. These indices are
/// used to set or check bits in the Bloom filter's bit vector.
pub type HashFunction = fn(&[u8], usize, usize) -> Vec<u32>;

pub(crate) fn hash_murmur32(key: &[u8]) -> u32 {
    let mut cursor = Cursor::new(key);
    murmur3_32(&mut cursor, 0).expect("Failed to compute Murmur3 hash")
}

pub(crate) fn hash_fnv32(key: &[u8]) -> u32 {
    let mut hasher = FnvHasher::default();
    hasher.write(key);
    hasher.finish() as u32
}

/// Implements the default double-hashing scheme for Bloom filters.
///
/// This function uses a technique called "double hashing" to generate multiple hash values
/// from just two independent hash functions. It's more efficient than computing
/// completely separate hash functions for each index.
///
/// The formula used is: h(i) = (h1 + i * h2) mod capacity
/// Where:
/// - h1 is the first hash value (Murmur3)
/// - h2 is the second hash value (FNV)
/// - i ranges from 0 to num_hashes-1
/// - capacity is the size of the bit vector
///
/// This approach provides good distribution while being computationally efficient.
/// The wrapping operations prevent integer overflow on large values.
///
/// Parameters:
/// - `item`: The byte slice to hash
/// - `num_hashes`: The number of hash values to generate
/// - `capacity`: The size of the bit vector (used for modulo)
///
/// Returns:
/// A vector of `num_hashes` hash values, each in the range [0, capacity-1]
pub fn default_hash_function(
    item: &[u8],
    num_hashes: usize,
    capacity: usize,
) -> Vec<u32> {
    let h1 = hash_murmur32(item);
    let h2 = hash_fnv32(item);
    (0..num_hashes)
        .map(|i| h1.wrapping_add((i as u32).wrapping_mul(h2)) % capacity as u32)
        .collect()
}

/// Calculates the optimal bit vector size for a Bloom filter.
///
/// This function determines the ideal size of the bit array to achieve the target
/// false positive rate (FPR) for a given number of elements.
///
/// The formula used is: m = -n * ln(fpr) / (ln(2)^2)
/// Where:
/// - m is the optimal bit vector size
/// - n is the expected number of elements
/// - fpr is the target false positive rate (between 0 and 1)
/// - ln(2) is the natural logarithm of 2
///
/// Mathematical derivation:
/// 1. The optimal false positive rate for a Bloom filter is (1 - e^(-kn/m))^k
/// 2. When k = (m/n)*ln(2), this expression is minimized
/// 3. At this optimal k, the false positive rate is approximately (0.6185)^(m/n)
/// 4. Solving for m gives us the formula implemented here
///
/// Parameters:
/// - `n`: The expected number of elements to be inserted
/// - `fpr`: The target false positive rate (e.g., 0.01 for 1%)
///
/// Returns:
/// The optimal size of the bit vector as a number of bits
pub fn optimal_bit_vector_size(n: usize, fpr: f64) -> usize {
    let ln2 = std::f64::consts::LN_2; // Natural logarithm of 2 (≈0.693)
    ((-(n as f64) * fpr.ln()) / (ln2 * ln2)).ceil() as usize
}

/// Calculates the optimal number of hash functions for a Bloom filter.
///
/// This function determines the ideal number of hash functions to minimize
/// the false positive rate for given bit vector size and element count.
///
/// The formula used is: k = (m/n) * ln(2)
/// Where:
/// - k is the optimal number of hash functions
/// - m is the bit vector size
/// - n is the expected number of elements
/// - ln(2) is the natural logarithm of 2
///
/// Mathematical basis:
/// 1. Each hash function sets one bit in the vector
/// 2. The probability of a bit still being 0 after all n elements are inserted is (1 - 1/m)^(k*n)
/// 3. This approximates to e^(-kn/m) for large m
/// 4. The false positive probability is minimized when k = (m/n)*ln(2)
///
/// Parameters:
/// - `n`: The expected number of elements to be inserted
/// - `m`: The size of the bit vector
///
/// Returns:
/// The optimal number of hash functions to use
pub fn optimal_num_hashes(n: usize, m: usize) -> usize {
    ((m as f64 / n as f64) * std::f64::consts::LN_2).round() as usize
}

/// Calculates the per-level false positive rate needed to achieve the target
/// overall false positive rate in a multi-level Bloom filter.
///
/// In a multi-level filter where a query returns true if any level reports true,
/// the overall false positive rate is higher than the per-level rate.
///
/// Parameters:
/// - `target_fpr`: The desired overall false positive rate
/// - `num_levels`: The maximum number of levels in the filter
/// - `active_ratio`: Estimated fraction of levels that are typically active (0.0-1.0)
///
/// Returns:
/// The adjusted per-level false positive rate
pub fn calculate_level_fpr(
    target_fpr: f64,
    num_levels: usize,
    active_ratio: f64,
) -> f64 {
    // Calculate effective number of levels based on typical activity
    let effective_levels = 1.0 + (num_levels - 1) as f64 * active_ratio;
    // Calculate adjusted level FPR
    1.0 - (1.0 - target_fpr).powf(1.0 / effective_levels)
}

/// Calculates optimal Bloom filter parameters based on capacity and target FPR.
///
/// This function handles the common initialization logic for Bloom filters,
/// including FPR adjustment for multi-level filters.
///
/// Parameters:
/// - `capacity`: The maximum number of elements the filter should hold
/// - `target_fpr`: The desired overall false positive rate
/// - `num_levels`: The maximum number of levels in the filter
/// - `active_ratio`: Estimated fraction of levels that are typically active (0.0-1.0)
///
/// Returns:
/// A tuple containing (adjusted_level_fpr, optimal_bit_vector_size, optimal_num_hashes)
pub fn calculate_optimal_params(
    capacity: usize,
    target_fpr: f64,
    num_levels: usize,
    active_ratio: f64,
) -> (f64, usize, usize) {
    // Adjust FPR for multi-level filter
    let level_fpr = calculate_level_fpr(target_fpr, num_levels, active_ratio);

    // Calculate optimal bit vector size for the adjusted FPR
    let bit_vector_size = optimal_bit_vector_size(capacity, level_fpr);

    // Calculate optimal number of hash functions
    let num_hashes = optimal_num_hashes(capacity, bit_vector_size);

    // TODO: probably need to remove level_fpr
    (level_fpr, bit_vector_size, num_hashes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimal_bit_vector_size() {
        // Test with known values from literature
        // For 10,000 items with 1% FPR, optimal size should be around 95,850 bits
        let m = optimal_bit_vector_size(10_000, 0.01);
        assert!(
            m > 90_000 && m < 100_000,
            "Optimal size outside expected range: {m}"
        );

        // For 1,000 items with 0.1% FPR, optimal size should be around 14,400 bits
        let m = optimal_bit_vector_size(1_000, 0.001);
        assert!(
            m > 13_000 && m < 16_000,
            "Optimal size outside expected range: {m}"
        );

        // Test boundary values
        assert!(
            optimal_bit_vector_size(1, 0.5) > 0,
            "Even small values should produce positive bit size"
        );

        // Test scaling property - 10x items should need ~10x space for same FPR
        let m1 = optimal_bit_vector_size(1_000, 0.01);
        let m2 = optimal_bit_vector_size(10_000, 0.01);
        let ratio = m2 as f64 / m1 as f64;
        assert!(
            ratio > 9.0 && ratio < 11.0,
            "Bit vector size should scale linearly with item count"
        );
    }

    #[test]
    fn test_optimal_num_hashes() {
        // Test with known values from literature
        // For m/n = 10, optimal k should be around 7
        let k = optimal_num_hashes(1_000, 10_000);
        assert!(
            (6..=8).contains(&k),
            "Optimal hash count outside expected range: {k}"
        );

        // Test scaling property - doubling m/n should roughly double k
        let k1 = optimal_num_hashes(1_000, 10_000);
        let k2 = optimal_num_hashes(1_000, 20_000);
        let ratio = k2 as f64 / k1 as f64;
        assert!(
            ratio > 1.8 && ratio < 2.2,
            "Hash count should scale with m/n ratio"
        );
    }

    #[test]
    fn test_hash_functions_distribution() {
        let capacity = 10000;
        let num_samples = 1000;
        let mut distribution = vec![0; capacity];

        // Generate random test data
        let test_data: Vec<Vec<u8>> = (0..num_samples)
            .map(|i| format!("test_data_{i}").into_bytes())
            .collect();

        // Generate hash values using default_hash_function
        for data in test_data {
            let hashes = default_hash_function(&data, 1, capacity);
            for hash in hashes {
                distribution[hash as usize] += 1;
            }
        }

        // Calculate statistics to verify distribution
        let mean = distribution.iter().sum::<i32>() as f64 / capacity as f64;

        // Count non-zero buckets (should be reasonably high for good distribution)
        let non_zero = distribution.iter().filter(|&&x| x > 0).count();
        let coverage = non_zero as f64 / capacity as f64;

        // For good hash functions with 1000 samples in 10000 buckets, we expect roughly 10% coverage
        assert!(
            coverage > 0.05,
            "Hash distribution coverage too low: {coverage}"
        );

        // Verify the mean is reasonable (should be close to num_samples/capacity)
        let expected_mean = num_samples as f64 / capacity as f64;
        let mean_ratio = mean / expected_mean;
        assert!(
            mean_ratio > 0.8 && mean_ratio < 1.2,
            "Mean distribution ratio outside expected range: {mean_ratio}"
        );
    }
}
