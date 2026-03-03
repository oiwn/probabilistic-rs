"""
Persistence examples for BloomFilter and ExpiringBloomFilter.
Run with: python python/examples/persistence_example.py
"""

import tempfile
import os
from pathlib import Path

from probabilistic_rs import BloomFilter, ExpiringBloomFilter


def main():
    print("=" * 50)
    print("Persistence Examples")
    print("=" * 50)

    # Use temp directory
    with tempfile.TemporaryDirectory() as tmpdir:
        tmpdir = Path(tmpdir)

        # BloomFilter persistence
        print("\n1. BloomFilter with persistence")
        bloom_path = str(tmpdir / "bloom.db")

        # Create and populate
        bf = BloomFilter.create(bloom_path, capacity=10_000, false_positive_rate=0.01)
        bf.insert(b"persistent_item_1")
        bf.insert(b"persistent_item_2")
        bf.insert_bulk([b"bulk_1", b"bulk_2"])
        bf.save_snapshot()
        print(f"   Created at: {bloom_path}")
        print(f"   Inserted 4 items")

        # Verify before closing
        print(f"   Before save - contains('persistent_item_1'): {bf.contains(b'persistent_item_1')}")

        # Release and reload
        del bf
        print("   Released DB...")

        bf2 = BloomFilter.load(bloom_path)
        print(f"   Loaded from: {bloom_path}")
        print(f"   After load - contains('persistent_item_1'): {bf2.contains(b'persistent_item_1')}")
        print(f"   After load - contains('bulk_1'): {bf2.contains(b'bulk_1')}")
        print(f"   After load - contains('unknown'): {bf2.contains(b'unknown')}")

        # ExpiringBloomFilter persistence
        print("\n2. ExpiringBloomFilter with persistence")
        ebloom_path = str(tmpdir / "expiring.db")

        # Create and populate
        ebf = ExpiringBloomFilter.create(
            ebloom_path,
            capacity_per_level=10_000,
            target_fpr=0.01,
            level_duration_secs=3600,
            num_levels=3,
        )
        ebf.insert(b"level0_item")
        ebf.save_snapshot()
        print(f"   Created at: {ebloom_path}")

        # Rotate and add more
        ebf.rotate_levels()
        ebf.insert(b"level1_item")
        ebf.save_snapshot()
        print("   Rotated and added more items")

        # Release and reload
        del ebf
        print("   Released DB...")

        ebf2 = ExpiringBloomFilter.load(ebloom_path)
        print(f"   Loaded from: {ebloom_path}")
        print(f"   After load - contains('level0_item'): {ebf2.contains(b'level0_item')}")
        print(f"   After load - contains('level1_item'): {ebf2.contains(b'level1_item')}")

        print("\n" + "=" * 50)
        print("All persistence examples completed!")
        print("=" * 50)


if __name__ == "__main__":
    main()
