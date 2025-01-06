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

pub fn optimal_bit_vector_size(n: usize, fpr: f64) -> usize {
    let ln2 = std::f64::consts::LN_2;
    ((-(n as f64) * fpr.ln()) / (ln2 * ln2)).ceil() as usize
}

pub fn optimal_num_hashes(n: usize, m: usize) -> usize {
    ((m as f64 / n as f64) * std::f64::consts::LN_2).round() as usize
}
