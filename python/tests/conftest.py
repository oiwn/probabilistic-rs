import pytest
import tempfile
import shutil
from pathlib import Path


@pytest.fixture
def temp_dir():
    """Temporary directory for persistence tests."""
    d = tempfile.mkdtemp()
    yield Path(d)
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def bloom_filter():
    """In-memory BloomFilter for quick tests."""
    from probabilistic_rs import BloomFilter

    return BloomFilter(capacity=10_000, false_positive_rate=0.01)


@pytest.fixture
def ebloom_filter():
    """In-memory ExpiringBloomFilter for quick tests."""
    from probabilistic_rs import ExpiringBloomFilter

    return ExpiringBloomFilter(
        capacity_per_level=10_000,
        target_fpr=0.01,
        level_duration_secs=3600,
        num_levels=3,
    )
