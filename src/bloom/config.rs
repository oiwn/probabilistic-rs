use crate::hash::{HashFunction, default_hash_function};
use derive_builder::Builder;

#[derive(Clone, Debug, Builder)]
#[builder(pattern = "owned")]
pub struct BloomFilterConfig {
    #[builder(default = "1_000_000")]
    pub capacity: usize,

    #[builder(default = "0.01")]
    pub false_positive_rate: f64,

    #[builder(default = "default_hash_function")]
    pub hash_function: HashFunction,
}

impl BloomFilterConfig {
    pub fn validate(&self) -> super::BloomResult<()> {
        if self.capacity == 0 {
            return Err(super::BloomError::InvalidConfig(
                "Capacity must be > 0".into(),
            ));
        }
        if self.false_positive_rate <= 0.0 || self.false_positive_rate >= 1.0 {
            return Err(super::BloomError::InvalidConfig(
                "FPR must be between 0 and 1".into(),
            ));
        }
        Ok(())
    }
}
