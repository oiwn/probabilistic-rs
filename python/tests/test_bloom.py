import pytest


class TestBloomFilterInMemory:
    """In-memory BloomFilter tests."""

    def test_insert_and_contains(self, bloom_filter):
        bloom_filter.insert(b"hello")
        assert bloom_filter.contains(b"hello") == True
        assert bloom_filter.contains(b"world") == False

    def test_insert_bulk(self, bloom_filter):
        items = [b"a", b"b", b"c"]
        bloom_filter.insert_bulk(items)
        results = bloom_filter.contains_bulk(items)
        assert results == [True, True, True]

    def test_contains_bulk_mixed(self, bloom_filter):
        bloom_filter.insert_bulk([b"a", b"b"])
        results = bloom_filter.contains_bulk([b"a", b"b", b"c"])
        assert results == [True, True, False]

    def test_clear(self, bloom_filter):
        bloom_filter.insert(b"test")
        assert bloom_filter.contains(b"test") == True
        bloom_filter.clear()
        assert bloom_filter.contains(b"test") == False

    def test_stats(self, bloom_filter):
        assert bloom_filter.capacity() == 10_000
        assert bloom_filter.false_positive_rate() == 0.01
        bloom_filter.insert(b"x")
        assert bloom_filter.insert_count() == 1

    def test_insert_empty_bytes(self, bloom_filter):
        bloom_filter.insert(b"")
        assert bloom_filter.contains(b"") == True


class TestBloomFilterPersistence:
    """Persistence tests (requires fjall feature)."""

    def test_create_and_load(self, temp_dir):
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "test.db")

        # Create and insert
        bf = BloomFilter.create(
            db_path, capacity=10_000, false_positive_rate=0.01
        )
        bf.insert(b"persistent")
        bf.save_snapshot()
        del bf  # Release DB lock

        # Load and verify
        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"persistent") == True

    def test_load_nonexistent_raises(self, temp_dir):
        from probabilistic_rs import BloomFilter

        with pytest.raises(Exception):
            BloomFilter.load(str(temp_dir / "nonexistent.db"))

    def test_persistence_roundtrip(self, temp_dir):
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "roundtrip.db")

        bf = BloomFilter.create(
            db_path, capacity=10_000, false_positive_rate=0.01
        )
        items = [b"item1", b"item2", b"item3"]
        bf.insert_bulk(items)
        bf.save_snapshot()
        del bf  # Release DB lock

        bf2 = BloomFilter.load(db_path)
        for item in items:
            assert bf2.contains(item) == True
