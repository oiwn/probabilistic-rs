# Current Task Context

## Auto Snapshot Persistence Plan

### Goal

Make snapshot persistence behavior real and consistent across both persisted filter types:
- `BloomFilter`
- `ExpiringBloomFilter`

Today, snapshot persistence is only partially implemented:
- `BloomFilter` exposes snapshot config fields that do nothing
- `ExpiringBloomFilter` rotates levels with `save_full_snapshot()`, but dirty-chunk `save_snapshot()` is still manual-only

The result is misleading configuration and incomplete persistence behavior.

### Required Behavior

#### Snapshot Triggers

Auto snapshot uses OR logic:
- time-based trigger: snapshot every `snapshot_interval`
- insert-count trigger: snapshot after `snapshot_after_inserts` new inserts since the last successful snapshot

If either trigger fires, attempt a snapshot.

#### Supported Modes

Two persistence modes remain supported:
- automatic snapshot via background task
- explicit manual `save_snapshot()` calls

Manual snapshot remains available even when auto snapshot is enabled.

#### Applies To

`BloomFilter`
- implement background auto snapshot for persisted filters
- snapshot operation is the existing dirty-chunk snapshot path

`ExpiringBloomFilter`
- implement background auto snapshot for dirty chunks
- keep existing `save_full_snapshot()` behavior on level rotation
- full rotation snapshot and background dirty snapshot are separate mechanisms

#### Clean Shutdown

Snapshot on clean shutdown is required.
- do not keep `snapshot_on_drop` as a config option
- perform a final snapshot attempt during drop/cleanup

#### Failure Semantics

Snapshot write failures are hard failures.
- snapshot I/O errors are not ignored
- background snapshot tasks must not silently continue after a failed write
- a failed background snapshot poisons the filter for future writes
- subsequent mutating operations must return the stored persistence error
- read operations may continue to use the in-memory state
- manual `save_snapshot()` must also return the stored persistence error once poisoned

This is an I/O correctness issue, not best-effort background maintenance.

#### Failure Model

Use write-poisoning.

After the first background snapshot write failure:
- store the persistence error in shared state
- mark the filter as poisoned for writes
- reject future mutating operations with that stored error
- allow read-only operations to continue against in-memory state
- keep the error inspectable through an explicit health or last-error API if needed

### Runtime / Python Notes

#### Shared Runtime Bug

`get_runtime()` currently uses a non-static `OnceLock`, which means each filter instance may create its own Tokio runtime.

That must be fixed first or as part of the same change:
- use one shared static runtime
- use that runtime for all background snapshot tasks

#### Python Surface

Python constructors must expose the same auto snapshot configuration that Rust users can set.

Meaning:
- Python users should be able to enable auto snapshot
- Python users should be able to set interval and insert-count thresholds
- Python `save_snapshot()` remains callable manually

### Configuration Shape

Keep config surface minimal and real:
- `auto_snapshot: bool`
- `snapshot_interval: Duration`
- `snapshot_after_inserts: usize` where `0` disables the count trigger

Remove dead or misleading config.
If clean-shutdown snapshot is mandatory, it should not be user-configurable.

### Documentation Requirements

Document exact semantics, not just field meanings.

Required docs:
- field docs for `PersistenceConfig` and `ExpiringPersistenceConfig`
- module-level docs explaining manual vs automatic snapshot behavior
- docs stating that `ExpiringBloomFilter` does dirty snapshots in background and full snapshots on rotation
- Python docstrings / constructor docs for the persistence options
- docs stating that snapshot errors are hard failures

### Implementation Plan

#### Step 1: Fix runtime ownership

Implement one shared Tokio runtime for snapshot work.

Tests:
- runtime accessor returns the same shared runtime across multiple filter constructions
- creating multiple persisted filters does not create per-instance runtime state

#### Step 2: Make config truthful

Update persistence config definitions to match real supported behavior.
- keep `auto_snapshot`
- keep `snapshot_interval`
- keep `snapshot_after_inserts`
- remove `snapshot_on_drop` if shutdown snapshot is unconditional
- add docs for each field

Tests:
- config defaults match documented behavior
- count trigger disabled when `snapshot_after_inserts == 0`

#### Step 3: Add insert tracking for snapshot thresholds

Track inserts since last successful snapshot using shared atomic state.

Tests:
- insert counter increments on writes
- successful snapshot resets the counter
- failed snapshot does not reset the counter
- poisoned state retains the first persistence error for later write failures

#### Step 4: Implement `BloomFilter` background auto snapshot

When persistence is enabled and `auto_snapshot` is true:
- spawn a background task on the shared runtime
- trigger snapshot by time or insert count
- skip work when nothing is dirty
- stop the task during shutdown
- attempt final snapshot during cleanup/drop

Tests:
- interval trigger causes snapshot without manual calls
- insert-count trigger causes snapshot after threshold is reached
- no snapshot is written when nothing is dirty
- final cleanup/drop attempts a snapshot
- injected snapshot I/O failure poisons future writes and is not swallowed
- reads still work after write poisoning

#### Step 5: Implement `ExpiringBloomFilter` background dirty snapshot

Apply the same background trigger model to dirty-chunk snapshots.
Keep level-rotation full snapshot behavior unchanged.

Tests:
- interval trigger snapshots dirty state
- insert-count trigger snapshots dirty state
- level rotation still performs full snapshot
- background dirty snapshot and rotation full snapshot do not conflict semantically
- injected snapshot I/O failure poisons future writes and is not swallowed
- reads still work after write poisoning

#### Step 6: Expose config in Python bindings

Thread the persistence options through Python constructors and docs.

Tests:
- Python constructors accept auto snapshot settings
- Python-created filters use configured interval/count behavior
- Python manual `save_snapshot()` still works

#### Step 7: Document exact persistence semantics

Update Rust and Python docs to describe:
- when snapshots happen
- what each filter saves
- what happens on clean shutdown
- what happens on snapshot failure

Tests:
- doc examples compile where applicable
- public API docs reflect current config and behavior

### Acceptance Criteria

The work is complete when:
- no persistence config field is dead
- both filter types support background auto snapshot
- both filter types still support manual snapshot
- `ExpiringBloomFilter` retains full snapshot on rotation
- clean shutdown performs a final snapshot attempt
- snapshot write failures poison future writes while allowing reads
- Python bindings expose the same persistence controls
- docs describe the real behavior precisely
