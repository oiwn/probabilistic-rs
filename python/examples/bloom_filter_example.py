"""
Basic BloomFilter usage examples.
Run with: python python/examples/bloom_filter_example.py
"""

from probabilistic_rs import BloomFilter


def main():
    print("=" * 50)
    print("BloomFilter Examples")
    print("=" * 50)

    # Create in-memory filter
    print("\n1. Create in-memory BloomFilter")
    bf = BloomFilter(capacity=1_000_000, false_positive_rate=0.01)
    print(f"   Capacity: {bf.capacity():,}")
    print(f"   FPR: {bf.false_positive_rate()}")

    # Insert items
    print("\n2. Insert items")
    bf.insert(b"hello")
    bf.insert(b"world")
    bf.insert(b"rust")
    print(f"   Inserted 3 items, count: {bf.insert_count()}")

    # Check existence
    print("\n3. Check existence")
    print(f"   contains('hello'): {bf.contains(b'hello')}")
    print(f"   contains('python'): {bf.contains(b'python')}")

    # Bulk operations
    print("\n4. Bulk operations")
    items = [b"item1", b"item2", b"item3", b"item4", b"item5"]
    bf.insert_bulk(items)
    print(f"   Inserted {len(items)} items, count: {bf.insert_count()}")

    check_items = [b"hello", b"item1", b"unknown1", b"unknown2"]
    results = bf.contains_bulk(check_items)
    print(f"   Bulk check: {list(zip([i.decode() for i in check_items], results))}")

    # Clear
    print("\n5. Clear filter")
    bf.clear()
    print(f"   After clear, contains('hello'): {bf.contains(b'hello')}")
    print(f"   Insert count: {bf.insert_count()}")


if __name__ == "__main__":
    main()
