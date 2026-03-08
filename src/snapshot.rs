use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

/// Shared state for snapshot tracking.
///
/// Tracks inserts since the last successful snapshot and holds a write-poison
/// slot. Wrapped in `Arc` so the filter and a future background snapshot task
/// can share it without copying.
pub struct SnapshotState {
    /// Number of inserts since the last successful snapshot.
    inserts_since_snapshot: AtomicUsize,
    /// First snapshot failure message. Once set, the filter is poisoned for writes.
    poison: Mutex<Option<String>>,
}

impl SnapshotState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inserts_since_snapshot: AtomicUsize::new(0),
            poison: Mutex::new(None),
        })
    }

    /// Record `count` new inserts.
    pub fn record_inserts(&self, count: usize) {
        self.inserts_since_snapshot
            .fetch_add(count, Ordering::Relaxed);
    }

    /// Number of inserts since the last successful snapshot.
    pub fn inserts_since_snapshot(&self) -> usize {
        self.inserts_since_snapshot.load(Ordering::Relaxed)
    }

    /// Call after a successful snapshot. Resets the insert counter.
    pub fn on_snapshot_success(&self) {
        self.inserts_since_snapshot.store(0, Ordering::Relaxed);
    }

    /// Call after a snapshot failure. Stores the first error; subsequent calls are no-ops.
    pub fn on_snapshot_failure(&self, error: &str) {
        let mut guard = self.poison.lock().unwrap();
        if guard.is_none() {
            *guard = Some(error.to_owned());
        }
    }

    /// Returns the stored poison error message, if any.
    pub fn check_poison(&self) -> Option<String> {
        self.poison.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_starts_at_zero() {
        let state = SnapshotState::new();
        assert_eq!(state.inserts_since_snapshot(), 0);
    }

    #[test]
    fn counter_increments_on_record_inserts() {
        let state = SnapshotState::new();
        state.record_inserts(1);
        assert_eq!(state.inserts_since_snapshot(), 1);
        state.record_inserts(5);
        assert_eq!(state.inserts_since_snapshot(), 6);
    }

    #[test]
    fn successful_snapshot_resets_counter() {
        let state = SnapshotState::new();
        state.record_inserts(100);
        state.on_snapshot_success();
        assert_eq!(state.inserts_since_snapshot(), 0);
    }

    #[test]
    fn failed_snapshot_does_not_reset_counter() {
        let state = SnapshotState::new();
        state.record_inserts(42);
        state.on_snapshot_failure("disk full");
        assert_eq!(state.inserts_since_snapshot(), 42);
    }

    #[test]
    fn no_poison_initially() {
        let state = SnapshotState::new();
        assert!(state.check_poison().is_none());
    }

    #[test]
    fn poison_stores_first_error_only() {
        let state = SnapshotState::new();
        state.on_snapshot_failure("first error");
        state.on_snapshot_failure("second error");
        assert_eq!(state.check_poison().unwrap(), "first error");
    }

    #[test]
    fn poison_is_retained_after_multiple_checks() {
        let state = SnapshotState::new();
        state.on_snapshot_failure("boom");
        assert_eq!(state.check_poison().unwrap(), "boom");
        assert_eq!(state.check_poison().unwrap(), "boom");
    }
}
