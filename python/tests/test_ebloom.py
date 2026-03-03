import pytest


class TestExpiringBloomFilterInMemory:
    def test_insert_and_contains(self, ebloom_filter):
        ebloom_filter.insert(b"temp")
        assert ebloom_filter.contains(b"temp") == True

    def test_insert_bulk(self, ebloom_filter):
        items = [b"a", b"b", b"c"]
        ebloom_filter.insert_bulk(items)
        results = ebloom_filter.contains_bulk(items)
        assert results == [True, True, True]

    def test_rotate_levels(self, ebloom_filter):
        ebloom_filter.insert(b"before_rotate")
        ebloom_filter.rotate_levels()
        # Data should still exist (in previous level)
        assert ebloom_filter.contains(b"before_rotate") == True

    def test_clear(self, ebloom_filter):
        ebloom_filter.insert(b"test")
        assert ebloom_filter.contains(b"test") == True
        ebloom_filter.clear()
        assert ebloom_filter.contains(b"test") == False

    def test_stats(self, ebloom_filter):
        assert ebloom_filter.capacity_per_level() == 10_000
        assert ebloom_filter.target_fpr() == 0.01
        assert ebloom_filter.num_levels() == 3
        assert ebloom_filter.active_levels() == 3
        ebloom_filter.insert(b"x")
        assert ebloom_filter.total_insert_count() == 1


class TestExpiringBloomFilterPersistence:
    def test_create_and_load(self, temp_dir):
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "ebloom.db")

        ebf = ExpiringBloomFilter.create(
            db_path,
            capacity_per_level=10_000,
            target_fpr=0.01,
            level_duration_secs=3600,
            num_levels=3,
        )
        ebf.insert(b"data")
        ebf.save_snapshot()
        del ebf  # Release DB lock

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"data") == True

    def test_persistence_with_rotation(self, temp_dir):
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "rotation.db")

        ebf = ExpiringBloomFilter.create(
            db_path,
            capacity_per_level=10_000,
            target_fpr=0.01,
            level_duration_secs=3600,
            num_levels=3,
        )
        ebf.insert(b"before")
        ebf.rotate_levels()
        ebf.insert(b"after")
        ebf.save_snapshot()
        del ebf  # Release DB lock

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"before") == True
        assert ebf2.contains(b"after") == True
