# Current Task Context

## Active Work: Phase 4b - Serialization Cleanup & Consistency

**Started**: 2025-01-17  
**Module**: `src/ebloom/`  
**Priority**: High - Fix platform-dependent serialization before persistence integration

## Current Issues Identified

### 1. Inconsistent Serialization Patterns
- **Config**: Uses `serde_json` (human‑readable, large overhead)
- **Current level**: Raw `usize` bytes (platform‑dependent, verbose indexing)
- **Metadata**: Hand‑rolled byte bashing (`[chunk[0], chunk[1], ...]`)
- **Core bloom**: Uses `bincode` (portable, compact, type‑safe)

### 2. Platform‑Dependent Assumptions
- `usize` serialized as 8 bytes (assumes 64‑bit target)
- `usize` → `u64` casts without validation
- Manual byte indexing instead of `try_into()`

### 3. Code Quality Issues
- Verbose `[level_bytes[0], level_bytes[1], ...]` patterns
- Missing error handling for malformed data
- Inconsistent error types (`serde_json::Error` vs `bincode::Error`)

## Proposed Solution: Bincode‑Only Serialization

### Core Decisions
1. **Remove `serde_json` entirely** from ebloom module
2. **Limit levels to 255** (`u8`) – realistic for all use cases
3. **Use `bincode` for everything** (config, metadata, level index)
4. **Fix platform‑dependent fields**:
   - `current_level: u8` (in‑memory as `usize`, on‑disk as `u8`)
   - `insert_count: u64` (was `usize`)
   - All timestamps remain `u64`

### Implementation Plan

#### Phase 1: Update Config Serialization (`src/ebloom/config.rs`)
```rust
// Add bincode derives
#[derive(Debug, Clone, Builder, Serialize, Deserialize, bincode::Decode, bincode::Encode)]
pub struct ExpiringFilterConfig { /* ... */ }

// Update to_bytes/from_bytes
pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
    bincode::encode_to_vec(self, bincode::config::standard())
}

pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
    bincode::decode_from_slice(bytes, bincode::config::standard()).map(|(config, _)| config)
}
```

#### Phase 2: Update LevelMetadata (`src/ebloom/config.rs`)
```rust
#[derive(Debug, Clone, Serialize, Deserialize, bincode::Decode, bincode::Encode)]
pub struct LevelMetadata {
    pub created_at: u64,
    pub insert_count: u64,  // was usize
    pub last_snapshot_at: u64,
}
```

#### Phase 3: Fix Current Level Storage (`src/ebloom/storage.rs`)
```rust
// Save: store as u8
let level_bytes = (current_level as u8).to_le_bytes();

// Load: read 1 byte
if level_bytes.len() >= 1 {
    Ok(level_bytes[0] as usize)  // u8 → usize safe
}
```

#### Phase 4: Fix Metadata Serialization (`src/ebloom/storage.rs`)
- Replace `serialize_metadata`/`deserialize_metadata` with `bincode::encode_to_vec`/`decode_from_slice`
- Remove manual byte‑bashing loops

#### Phase 5: Update Field Types & Validation
- Add validation: `num_levels <= 255`
- Update `ExpiringFilterConfigBuilder` to accept `u8` for `num_levels` (or keep `usize` with validation)
- Update `current_level` in filter struct (keep `AtomicUsize` for atomic ops, convert at boundaries)

#### Phase 6: Update Error Handling (`src/ebloom/error.rs`)
- Remove `From<serde_json::Error>`
- Add `From<bincode::error::EncodeError>` and `From<bincode::error::DecodeError>`

#### Phase 7: Update Tests
- Update config serialization tests
- Ensure all tests pass with new limits
- Add edge‑case tests for 255 levels

## Why These Changes?

### 1. Consistency with Core Bloom
- Core bloom already uses `bincode` – same library, same patterns
- Reduces cognitive load when switching between modules

### 2. Portability
- `bincode` handles `usize` portably (varint encoding)
- No 32‑bit vs 64‑bit compatibility issues
- Automatic endianness handling

### 3. Realistic Constraints
- 255 levels is ample (e.g., 255 hours ≈ 10 days, 255 days ≈ 8 months)
- Most real‑world uses: <10 levels
- Simplifies storage (1 byte per level index)

### 4. Code Quality
- Eliminates verbose manual byte indexing
- Type‑safe serialization/deserialization
- Clear error handling with proper error types

## Risk Assessment

### Breaking Changes
- **On‑disk format changes** – existing databases become incompatible
- **API changes** – `insert_count` becomes `u64`, may affect downstream code
- **Level limit** – filters with >255 levels will fail validation

### Mitigation
- Phase 4 (persistence) not yet implemented – no existing databases
- `insert_count` not exposed in public API (internal metric)
- 255‑level limit documented; users can adjust time window instead

## Success Criteria

- [ ] No `serde_json` usage in ebloom module
- [ ] All serialization uses `bincode`
- [ ] `current_level` stored as `u8` (1 byte)
- [ ] `LevelMetadata` uses `u64` for `insert_count`
- [ ] `num_levels <= 255` validation
- [ ] All tests pass with `cargo test --features fjall`
- [ ] Manual byte‑bashing patterns replaced with `try_into()` or `bincode`

## Next Steps After Cleanup

1. **Proceed with Phase 4** (Fjall persistence integration)
2. **Benchmark serialization overhead** (bincode vs raw bytes)
3. **Consider compression** for large metadata arrays (optional)
4. **Document serialization format** for future compatibility

## Files to Modify

| File | Changes |
|------|---------|
| `src/ebloom/config.rs` | Add bincode derives, change insert_count type, update serialization |
| `src/ebloom/storage.rs` | Replace manual serialization with bincode, fix current_level storage |
| `src/ebloom/error.rs` | Update error conversions |
| `src/ebloom/filter.rs` | Update insert_count handling, add validation |
| `tests/ebloom_tests.rs` | Update test data for new limits |
| `Cargo.toml` | Ensure bincode dependency (already present) |

## Open Questions

1. **Keep `num_levels` as `usize` with validation?**  
   → Yes, for API compatibility; validate `<= 255` in `config.validate()`

2. **Store `current_level` as bincode or raw byte?**  
   → Raw byte (1 byte) – simpler for single value

3. **Backward compatibility with existing tests?**  
   → No existing persistence tests; all in‑memory tests unaffected

4. **Performance impact of bincode vs raw bytes?**  
   → Minimal for small structs; benchmark if concerned
