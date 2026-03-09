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
        # Data from previous level must still be visible
        assert ebloom_filter.contains(b"before_rotate") == True

    def test_insert_after_rotate(self, ebloom_filter):
        ebloom_filter.insert(b"level0")
        ebloom_filter.rotate_levels()
        ebloom_filter.insert(b"level1")
        assert ebloom_filter.contains(b"level0") == True
        assert ebloom_filter.contains(b"level1") == True

    def test_full_rotation_clears_oldest(self, ebloom_filter):
        """After rotating num_levels times, the oldest level is cleared."""
        num_levels = ebloom_filter.num_levels()  # 3
        ebloom_filter.insert(b"oldest")
        # Rotate enough times that the level holding "oldest" gets cleared
        for _ in range(num_levels):
            ebloom_filter.rotate_levels()
        assert ebloom_filter.contains(b"oldest") == False

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

    def test_total_insert_count_across_levels(self, ebloom_filter):
        ebloom_filter.insert_bulk([b"a", b"b", b"c"])
        ebloom_filter.rotate_levels()
        ebloom_filter.insert_bulk([b"d", b"e"])
        assert ebloom_filter.total_insert_count() == 5


class TestExpiringBloomFilterPersistence:
    def test_create_and_load(self, temp_dir):
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "ebloom.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.insert(b"data")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"data") == True

    def test_snapshot_on_drop(self, temp_dir):
        """Drop without explicit save_snapshot() — final snapshot must fire."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "drop.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.insert(b"saved_on_drop")
        del ebf  # no save_snapshot()

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"saved_on_drop") == True

    def test_snapshot_config_applied(self, temp_dir):
        """create() accepts SnapshotConfig and persists correctly."""
        from probabilistic_rs import ExpiringBloomFilter, SnapshotConfig

        db_path = str(temp_dir / "cfg.db")
        snap = SnapshotConfig(auto_snapshot=False)

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000, snapshot=snap)
        ebf.insert(b"cfg_item")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"cfg_item") == True

    def test_rotation_full_snapshot_survives_reload(self, temp_dir):
        """rotate_levels() writes a full snapshot — data survives without save_snapshot()."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "rotation_snap.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.insert(b"before_rotate")
        ebf.rotate_levels()  # full snapshot written here
        del ebf  # no explicit save_snapshot()

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"before_rotate") == True

    def test_persistence_with_rotation(self, temp_dir):
        """Data inserted before and after rotation both survive reload."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "rotation.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.insert(b"before")
        ebf.rotate_levels()
        ebf.insert(b"after")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"before") == True
        assert ebf2.contains(b"after") == True

    def test_multiple_rotations_all_levels_survive(self, temp_dir):
        """Insert into each level via rotation; all survive reload until wrapped."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "multi_rotate.db")
        num_levels = 3

        ebf = ExpiringBloomFilter.create(
            db_path, capacity_per_level=10_000, num_levels=num_levels
        )
        # Insert one item per level
        for i in range(num_levels - 1):
            ebf.insert(f"level{i}".encode())
            ebf.rotate_levels()
        ebf.insert(f"level{num_levels - 1}".encode())
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        for i in range(num_levels):
            assert ebf2.contains(f"level{i}".encode()) == True

    def test_full_circular_wrap_clears_oldest(self, temp_dir):
        """After rotating num_levels times, the original oldest level is cleared."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "wrap.db")
        num_levels = 3

        ebf = ExpiringBloomFilter.create(
            db_path, capacity_per_level=10_000, num_levels=num_levels
        )
        ebf.insert(b"oldest")
        # Rotate enough to cycle back and clear the level holding "oldest"
        for _ in range(num_levels):
            ebf.rotate_levels()
        assert ebf.contains(b"oldest") == False

        ebf.insert(b"newest")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"oldest") == False
        assert ebf2.contains(b"newest") == True

    def test_insert_count_trigger(self, temp_dir):
        """Insert-count trigger snapshots dirty state without explicit call."""
        from probabilistic_rs import ExpiringBloomFilter, SnapshotConfig
        import time

        db_path = str(temp_dir / "count.db")
        snap = SnapshotConfig(auto_snapshot=True, interval_secs=9999, after_inserts=5)

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000, snapshot=snap)
        for i in range(10):
            ebf.insert(f"item{i}".encode())
        time.sleep(0.2)
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        for i in range(10):
            assert ebf2.contains(f"item{i}".encode()) == True

    def test_load_nonexistent_raises(self, temp_dir):
        from probabilistic_rs import ExpiringBloomFilter

        with pytest.raises(Exception):
            ExpiringBloomFilter.load(str(temp_dir / "nonexistent.db"))

    def test_overwrite_existing_db(self, temp_dir):
        """create() on existing path removes old data."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "overwrite.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.insert(b"old_data")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf2.save_snapshot()
        del ebf2

        ebf3 = ExpiringBloomFilter.load(db_path)
        assert ebf3.contains(b"old_data") == False

    def test_post_rotation_snapshot_survives_reload(self, temp_dir):
        """After rotation, dirty-chunk snapshot of new level survives reload."""
        from probabilistic_rs import ExpiringBloomFilter

        db_path = str(temp_dir / "post_rotate_snap.db")

        ebf = ExpiringBloomFilter.create(db_path, capacity_per_level=10_000)
        ebf.rotate_levels()
        ebf.insert(b"new_level_item")
        ebf.save_snapshot()
        del ebf

        ebf2 = ExpiringBloomFilter.load(db_path)
        assert ebf2.contains(b"new_level_item") == True
