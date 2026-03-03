"""
ExpiringBloomFilter usage examples.
Run with: python python/examples/expiring_bloom_filter_example.py
"""

from probabilistic_rs import ExpiringBloomFilter


def main():
    print("=" * 50)
    print("ExpiringBloomFilter Examples")
    print("=" * 50)

    # Create filter with 3 levels, 1 hour per level
    print("\n1. Create ExpiringBloomFilter")
    ebf = ExpiringBloomFilter(
        capacity_per_level=100_000,
        target_fpr=0.01,
        level_duration_secs=3600,  # 1 hour
        num_levels=3,
    )
    print(f"   Capacity per level: {ebf.capacity_per_level():,}")
    print(f"   Target FPR: {ebf.target_fpr()}")
    print(f"   Num levels: {ebf.num_levels()}")
    print(f"   Active levels: {ebf.active_levels()}")

    # Insert items
    print("\n2. Insert items")
    ebf.insert(b"temp_data_1")
    ebf.insert(b"temp_data_2")
    ebf.insert(b"temp_data_3")
    print(f"   Inserted 3 items, total count: {ebf.total_insert_count()}")

    # Check existence
    print("\n3. Check existence")
    print(f"   contains('temp_data_1'): {ebf.contains(b'temp_data_1')}")
    print(f"   contains('unknown'): {ebf.contains(b'unknown')}")

    # Bulk operations
    print("\n4. Bulk operations")
    items = [b"a", b"b", b"c"]
    ebf.insert_bulk(items)
    print(f"   Inserted {len(items)} items, total count: {ebf.total_insert_count()}")

    results = ebf.contains_bulk([b"temp_data_1", b"a", b"unknown"])
    print(f"   Bulk check: {results}")

    # Rotate levels
    print("\n5. Rotate levels (simulates time passing)")
    ebf.insert(b"before_rotation")
    print(f"   Inserted 'before_rotation'")
    print(f"   Total count before rotation: {ebf.total_insert_count()}")

    ebf.rotate_levels()
    print("   Rotated!")

    ebf.insert(b"after_rotation")
    print(f"   Inserted 'after_rotation'")

    print(f"   contains('before_rotation'): {ebf.contains(b'before_rotation')}")
    print(f"   contains('after_rotation'): {ebf.contains(b'after_rotation')}")

    # Clear
    print("\n6. Clear filter")
    ebf.clear()
    print(f"   After clear, contains('temp_data_1'): {ebf.contains(b'temp_data_1')}")
    print(f"   Total insert count: {ebf.total_insert_count()}")


if __name__ == "__main__":
    main()
