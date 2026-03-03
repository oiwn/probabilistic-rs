use pyo3::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::BloomError;
use crate::EbloomError;
use crate::bloom::{
    BloomFilter, BloomFilterConfigBuilder, BloomFilterOps, BloomFilterStats,
    BulkBloomFilterOps, PersistenceConfigBuilder,
};
use crate::ebloom::config::{
    ExpiringFilterConfigBuilder, ExpiringPersistenceConfigBuilder,
};
use crate::ebloom::filter::ExpiringBloomFilter;
use crate::ebloom::traits::{
    BulkExpiringBloomFilterOps, ExpiringBloomFilterOps, ExpiringBloomFilterStats,
};

fn get_runtime() -> Arc<Runtime> {
    std::sync::OnceLock::new()
        .get_or_init(|| Arc::new(Runtime::new().unwrap()))
        .clone()
}

impl From<BloomError> for PyErr {
    fn from(err: BloomError) -> PyErr {
        match err {
            BloomError::InvalidConfig(msg) => {
                pyo3::exceptions::PyValueError::new_err(msg)
            }
            BloomError::StorageError(msg) => {
                pyo3::exceptions::PyIOError::new_err(msg)
            }
            _ => pyo3::exceptions::PyRuntimeError::new_err(err.to_string()),
        }
    }
}

impl From<EbloomError> for PyErr {
    fn from(err: EbloomError) -> PyErr {
        match err {
            EbloomError::InvalidConfig(msg) => {
                pyo3::exceptions::PyValueError::new_err(msg)
            }
            EbloomError::StorageError(msg) => {
                pyo3::exceptions::PyIOError::new_err(msg)
            }
            _ => pyo3::exceptions::PyRuntimeError::new_err(err.to_string()),
        }
    }
}

#[pyclass(name = "BloomFilter")]
pub struct PyBloomFilter {
    inner: BloomFilter,
    rt: Arc<Runtime>,
}

#[pymethods]
impl PyBloomFilter {
    #[new]
    #[pyo3(signature = (capacity, false_positive_rate=0.01))]
    fn new(capacity: usize, false_positive_rate: f64) -> PyResult<Self> {
        let config = BloomFilterConfigBuilder::default()
            .capacity(capacity)
            .false_positive_rate(false_positive_rate)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let rt = get_runtime();
        let inner = rt.block_on(BloomFilter::create(config))?;

        Ok(Self { inner, rt })
    }

    #[staticmethod]
    fn create(
        db_path: &str,
        capacity: usize,
        false_positive_rate: f64,
    ) -> PyResult<Self> {
        let persistence = PersistenceConfigBuilder::default()
            .db_path(PathBuf::from(db_path))
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let config = BloomFilterConfigBuilder::default()
            .capacity(capacity)
            .false_positive_rate(false_positive_rate)
            .persistence(Some(persistence))
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let rt = get_runtime();
        let inner = rt.block_on(BloomFilter::create(config))?;

        Ok(Self { inner, rt })
    }

    #[staticmethod]
    fn load(db_path: &str) -> PyResult<Self> {
        let rt = get_runtime();
        let inner = rt.block_on(BloomFilter::load(PathBuf::from(db_path)))?;

        Ok(Self { inner, rt })
    }

    fn insert(&self, item: &[u8]) -> PyResult<()> {
        self.inner.insert(item)?;
        Ok(())
    }

    fn contains(&self, item: &[u8]) -> PyResult<bool> {
        Ok(self.inner.contains(item)?)
    }

    fn clear(&self) -> PyResult<()> {
        self.inner.clear()?;
        Ok(())
    }

    fn insert_bulk(&self, items: Vec<Vec<u8>>) -> PyResult<()> {
        let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();
        self.inner.insert_bulk(&refs)?;
        Ok(())
    }

    fn contains_bulk(&self, items: Vec<Vec<u8>>) -> PyResult<Vec<bool>> {
        let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();
        Ok(self.inner.contains_bulk(&refs)?)
    }

    fn save_snapshot(&self) -> PyResult<()> {
        self.rt.block_on(self.inner.save_snapshot())?;
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    fn false_positive_rate(&self) -> f64 {
        self.inner.false_positive_rate()
    }

    fn insert_count(&self) -> usize {
        self.inner.insert_count()
    }
}

#[pyclass(name = "ExpiringBloomFilter")]
pub struct PyExpiringBloomFilter {
    inner: ExpiringBloomFilter,
    rt: Arc<Runtime>,
}

#[pymethods]
impl PyExpiringBloomFilter {
    #[new]
    #[pyo3(signature = (capacity_per_level, target_fpr=0.01, level_duration_secs=3600, num_levels=3))]
    fn new(
        capacity_per_level: usize,
        target_fpr: f64,
        level_duration_secs: u64,
        num_levels: usize,
    ) -> PyResult<Self> {
        let config = ExpiringFilterConfigBuilder::default()
            .capacity_per_level(capacity_per_level)
            .target_fpr(target_fpr)
            .level_duration(std::time::Duration::from_secs(level_duration_secs))
            .num_levels(num_levels)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let rt = get_runtime();
        let inner = rt.block_on(ExpiringBloomFilter::create(config))?;

        Ok(Self { inner, rt })
    }

    #[staticmethod]
    fn create(
        db_path: &str,
        capacity_per_level: usize,
        target_fpr: f64,
        level_duration_secs: u64,
        num_levels: usize,
    ) -> PyResult<Self> {
        let persistence = ExpiringPersistenceConfigBuilder::default()
            .db_path(PathBuf::from(db_path))
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let config = ExpiringFilterConfigBuilder::default()
            .capacity_per_level(capacity_per_level)
            .target_fpr(target_fpr)
            .level_duration(std::time::Duration::from_secs(level_duration_secs))
            .num_levels(num_levels)
            .persistence(Some(persistence))
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(e.to_string())
            })?;

        let rt = get_runtime();
        let inner = rt.block_on(ExpiringBloomFilter::create(config))?;

        Ok(Self { inner, rt })
    }

    #[staticmethod]
    fn load(db_path: &str) -> PyResult<Self> {
        let rt = get_runtime();
        let inner =
            rt.block_on(ExpiringBloomFilter::load(PathBuf::from(db_path)))?;

        Ok(Self { inner, rt })
    }

    fn insert(&self, item: &[u8]) -> PyResult<()> {
        self.inner.insert(item)?;
        Ok(())
    }

    fn contains(&self, item: &[u8]) -> PyResult<bool> {
        Ok(self.inner.contains(item)?)
    }

    fn clear(&self) -> PyResult<()> {
        self.inner.clear()?;
        Ok(())
    }

    fn insert_bulk(&self, items: Vec<Vec<u8>>) -> PyResult<()> {
        let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();
        self.inner.insert_bulk(&refs)?;
        Ok(())
    }

    fn contains_bulk(&self, items: Vec<Vec<u8>>) -> PyResult<Vec<bool>> {
        let refs: Vec<&[u8]> = items.iter().map(|v| v.as_slice()).collect();
        Ok(self.inner.contains_bulk(&refs)?)
    }

    fn rotate_levels(&self) -> PyResult<()> {
        self.rt.block_on(self.inner.rotate_levels())?;
        Ok(())
    }

    fn cleanup_expired_levels(&self) -> PyResult<()> {
        self.rt.block_on(self.inner.cleanup_expired_levels())?;
        Ok(())
    }

    fn save_snapshot(&self) -> PyResult<()> {
        self.rt.block_on(self.inner.save_snapshot())?;
        Ok(())
    }

    fn capacity_per_level(&self) -> usize {
        self.inner.capacity_per_level()
    }

    fn target_fpr(&self) -> f64 {
        self.inner.target_fpr()
    }

    fn num_levels(&self) -> usize {
        self.inner.num_levels()
    }

    fn active_levels(&self) -> usize {
        self.inner.active_levels()
    }

    fn total_insert_count(&self) -> u64 {
        self.inner.total_insert_count()
    }
}

#[pymodule]
fn probabilistic_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyBloomFilter>()?;
    m.add_class::<PyExpiringBloomFilter>()?;
    Ok(())
}
