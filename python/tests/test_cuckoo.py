class TestCuckooFilterInMemory:
    def test_insert_and_contains(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100)
        cf.insert(b"hello")
        cf.insert(b"world")

        assert cf.contains(b"hello")
        assert cf.contains(b"world")
        assert not cf.contains(b"other")

    def test_delete(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100)
        cf.insert(b"hello")
        assert cf.contains(b"hello")

        cf.delete(b"hello")
        assert not cf.contains(b"hello")

    def test_delete_nonexistent_does_not_error(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100)
        cf.delete(b"nope")

    def test_clear(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100)
        cf.insert(b"hello")
        cf.clear()
        assert not cf.contains(b"hello")

    def test_bulk_operations(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=1000)
        items = [f"item{i}".encode() for i in range(100)]

        cf.insert_bulk(items)
        results = cf.contains_bulk(items)
        assert all(results)

        cf.delete_bulk(items[:10])
        removed = cf.contains_bulk(items[:10])
        assert not any(removed)

    def test_stats(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100, fingerprint_bits=8)
        assert cf.capacity() == 100
        assert cf.fingerprint_bits() == 8
        assert cf.entries_per_bucket() == 4
        assert cf.insert_count() == 0

        cf.insert(b"a")
        cf.insert(b"b")
        assert cf.insert_count() == 2
        assert cf.load_factor() > 0.0

    def test_fingerprint_bits_variants(self):
        from probabilistic_rs import CuckooFilter

        for bits in (4, 8, 12, 16):
            cf = CuckooFilter(capacity=100, fingerprint_bits=bits)
            cf.insert(b"test")
            assert cf.contains(b"test")

    def test_duplicate_insertions(self):
        from probabilistic_rs import CuckooFilter

        cf = CuckooFilter(capacity=100)
        cf.insert(b"dup")
        cf.insert(b"dup")
        assert cf.contains(b"dup")


class TestCuckooFilterPersistence:
    def test_create_and_load(self, temp_dir):
        from probabilistic_rs import CuckooFilter

        db_path = str(temp_dir / "cuckoo.db")
        cf = CuckooFilter.create(db_path, capacity=100)
        cf.insert(b"alpha")
        cf.insert(b"beta")
        cf.save_snapshot()
        del cf

        cf2 = CuckooFilter.load(db_path)
        assert cf2.contains(b"alpha")
        assert cf2.contains(b"beta")
        assert not cf2.contains(b"gamma")

    def test_load_nonexistent_raises(self):
        from probabilistic_rs import CuckooFilter
        import pytest

        with pytest.raises(Exception):
            CuckooFilter.load("/nonexistent/cuckoo/path")

    def test_snapshot_on_drop(self, temp_dir):
        from probabilistic_rs import CuckooFilter

        db_path = str(temp_dir / "drop.db")
        cf = CuckooFilter.create(db_path, capacity=100)
        cf.insert(b"delta")
        del cf

        cf2 = CuckooFilter.load(db_path)
        assert cf2.contains(b"delta")

    def test_snapshot_config_defaults(self, temp_dir):
        from probabilistic_rs import CuckooFilter, SnapshotConfig

        db_path = str(temp_dir / "snap_config.db")
        snap = SnapshotConfig(auto_snapshot=True, interval_secs=9999, after_inserts=0)
        cf = CuckooFilter.create(db_path, capacity=100, snapshot=snap)
        cf.insert(b"epsilon")
        cf.save_snapshot()
        del cf

        cf2 = CuckooFilter.load(db_path)
        assert cf2.contains(b"epsilon")

    def test_insert_count_trigger(self, temp_dir):
        from probabilistic_rs import CuckooFilter, SnapshotConfig
        import time

        db_path = str(temp_dir / "count_trigger.db")
        snap = SnapshotConfig(
            auto_snapshot=True, interval_secs=9999, after_inserts=5
        )
        cf = CuckooFilter.create(db_path, capacity=1000, snapshot=snap)
        for i in range(10):
            cf.insert(f"item_{i}".encode())
        time.sleep(0.2)
        del cf

        cf2 = CuckooFilter.load(db_path)
        for i in range(10):
            assert cf2.contains(f"item_{i}".encode())

    def test_delete_persists(self, temp_dir):
        from probabilistic_rs import CuckooFilter

        db_path = str(temp_dir / "delete_persist.db")
        cf = CuckooFilter.create(db_path, capacity=100)
        cf.insert(b"to_delete")
        cf.insert(b"to_keep")
        cf.save_snapshot()
        cf.delete(b"to_delete")
        cf.save_snapshot()
        del cf

        cf2 = CuckooFilter.load(db_path)
        assert not cf2.contains(b"to_delete")
        assert cf2.contains(b"to_keep")
