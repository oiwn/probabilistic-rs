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

        bf = BloomFilter.create(db_path, capacity=10_000, false_positive_rate=0.01)
        bf.insert(b"persistent")
        bf.save_snapshot()
        del bf

        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"persistent") == True

    def test_load_nonexistent_raises(self, temp_dir):
        from probabilistic_rs import BloomFilter

        with pytest.raises(Exception):
            BloomFilter.load(str(temp_dir / "nonexistent.db"))

    def test_persistence_roundtrip(self, temp_dir):
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "roundtrip.db")

        bf = BloomFilter.create(db_path, capacity=10_000, false_positive_rate=0.01)
        items = [b"item1", b"item2", b"item3"]
        bf.insert_bulk(items)
        bf.save_snapshot()
        del bf

        bf2 = BloomFilter.load(db_path)
        for item in items:
            assert bf2.contains(item) == True

    def test_snapshot_on_drop(self, temp_dir):
        """Drop without explicit save_snapshot() — final snapshot must fire."""
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "drop.db")

        bf = BloomFilter.create(db_path, capacity=10_000)
        bf.insert(b"saved_on_drop")
        del bf  # no save_snapshot()

        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"saved_on_drop") == True

    def test_snapshot_config_defaults(self):
        """SnapshotConfig constructs with expected defaults."""
        from probabilistic_rs import SnapshotConfig

        cfg = SnapshotConfig()
        assert cfg.auto_snapshot == True
        assert cfg.interval_secs == 300
        assert cfg.after_inserts == 0

    def test_snapshot_config_custom(self):
        """SnapshotConfig accepts custom values."""
        from probabilistic_rs import SnapshotConfig

        cfg = SnapshotConfig(auto_snapshot=False, interval_secs=60, after_inserts=100)
        assert cfg.auto_snapshot == False
        assert cfg.interval_secs == 60
        assert cfg.after_inserts == 100

    def test_create_with_snapshot_config(self, temp_dir):
        """create() accepts a SnapshotConfig object."""
        from probabilistic_rs import BloomFilter, SnapshotConfig

        db_path = str(temp_dir / "cfg.db")
        snap = SnapshotConfig(auto_snapshot=False)

        bf = BloomFilter.create(db_path, capacity=10_000, snapshot=snap)
        bf.insert(b"hello")
        bf.save_snapshot()
        del bf

        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"hello") == True

    def test_create_without_snapshot_config_uses_defaults(self, temp_dir):
        """create() without snapshot= uses default SnapshotConfig."""
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "defaults.db")

        bf = BloomFilter.create(db_path, capacity=10_000)
        bf.insert(b"default_snap")
        bf.save_snapshot()
        del bf

        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"default_snap") == True

    def test_insert_count_trigger(self, temp_dir):
        """Insert-count trigger: snapshot fires after threshold without explicit call."""
        from probabilistic_rs import BloomFilter, SnapshotConfig
        import time

        db_path = str(temp_dir / "count_trigger.db")
        # Trigger snapshot after every 5 inserts; auto_snapshot must be enabled
        snap = SnapshotConfig(auto_snapshot=True, interval_secs=9999, after_inserts=5)

        bf = BloomFilter.create(db_path, capacity=10_000, snapshot=snap)
        for i in range(10):
            bf.insert(f"item{i}".encode())
        # Give background task a moment to flush
        time.sleep(0.2)
        del bf  # also fires final snapshot

        bf2 = BloomFilter.load(db_path)
        for i in range(10):
            assert bf2.contains(f"item{i}".encode()) == True

    def test_manual_snapshot_no_auto(self, temp_dir):
        """With auto_snapshot=False, manual save_snapshot() still works."""
        from probabilistic_rs import BloomFilter, SnapshotConfig

        db_path = str(temp_dir / "manual.db")
        snap = SnapshotConfig(auto_snapshot=False)

        bf = BloomFilter.create(db_path, capacity=10_000, snapshot=snap)
        bf.insert(b"manual_item")
        bf.save_snapshot()
        del bf

        bf2 = BloomFilter.load(db_path)
        assert bf2.contains(b"manual_item") == True

    def test_overwrite_existing_db(self, temp_dir):
        """create() on existing path removes old data."""
        from probabilistic_rs import BloomFilter

        db_path = str(temp_dir / "overwrite.db")

        bf = BloomFilter.create(db_path, capacity=10_000)
        bf.insert(b"old_data")
        bf.save_snapshot()
        del bf

        # Re-create overwrites
        bf2 = BloomFilter.create(db_path, capacity=10_000)
        bf2.save_snapshot()
        del bf2

        bf3 = BloomFilter.load(db_path)
        assert bf3.contains(b"old_data") == False
