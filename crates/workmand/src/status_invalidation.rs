//! Coalesced invalidation for the desktop process-status snapshot.

use std::sync::{
    Arc,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

const NO_DEADLINE: i64 = i64::MAX;

#[derive(Debug)]
struct StatusInvalidationState {
    version: AtomicU64,
    deadline: AtomicI64,
}

/// A thread-safe dirty version plus the next wall-clock-derived status edge.
///
/// Producers may invalidate from PTY worker threads. Status subscribers sample the
/// version on their existing 500 ms delivery tick, preserving the established
/// coalescing/latency ceiling without rebuilding an unchanged snapshot.
#[derive(Clone, Debug)]
pub(crate) struct StatusInvalidationHub {
    state: Arc<StatusInvalidationState>,
}

impl Default for StatusInvalidationHub {
    fn default() -> Self {
        Self {
            state: Arc::new(StatusInvalidationState {
                version: AtomicU64::new(0),
                deadline: AtomicI64::new(NO_DEADLINE),
            }),
        }
    }
}

impl StatusInvalidationHub {
    pub(crate) fn invalidate(&self) {
        self.state.version.fetch_add(1, Ordering::AcqRel);
    }

    /// Arm the earliest known wall-clock edge, such as attention becoming idle.
    pub(crate) fn arm_deadline(&self, at: i64) {
        self.state.deadline.fetch_min(at, Ordering::AcqRel);
    }

    /// Return the current version, first promoting a due wall-clock edge to dirty.
    pub(crate) fn version_at(&self, now: i64) -> u64 {
        loop {
            let deadline = self.state.deadline.load(Ordering::Acquire);
            if deadline == NO_DEADLINE || deadline > now {
                break;
            }
            if self
                .state
                .deadline
                .compare_exchange(deadline, NO_DEADLINE, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.invalidate();
                break;
            }
        }
        self.state.version.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidations_advance_the_shared_version() {
        let hub = StatusInvalidationHub::default();
        let other = hub.clone();
        assert_eq!(hub.version_at(10), 0);
        other.invalidate();
        assert_eq!(hub.version_at(10), 1);
    }

    #[test]
    fn only_due_deadlines_promote_a_dirty_version() {
        let hub = StatusInvalidationHub::default();
        hub.arm_deadline(20);
        hub.arm_deadline(30);
        assert_eq!(hub.version_at(19), 0);
        assert_eq!(hub.version_at(20), 1);
        assert_eq!(hub.version_at(30), 1);
    }
}
