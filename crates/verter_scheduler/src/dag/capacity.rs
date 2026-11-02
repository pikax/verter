//! Capacity types — child module of `dag`.
//!
//! Houses the admission-budget plumbing that the DAG consumes
//! during dispatch: the split CPU / I/O resource budget, the
//! per-`WorkKind` resource class, and the typed reservation that
//! decrements counters exactly once on release. The types live
//! here so the main `dag.rs` file stays focused on the readiness
//! DAG (admission, dedup, gating, fan-out) rather than the
//! capacity accounting it consumes.
//!
//! Anti-starvation is handled by the weighted-credit lane selector
//! in `dag.rs`, not by a time-based promotion config — the ledger
//! here governs ADMISSION only.
//!
//! Visibility note: every type is `pub` so the existing
//! `crate::dag::*` re-exports continue to work without callers
//! needing to know about the submodule split.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::WorkKind;

/// Admission-time capacity budget, split into two resource classes.
///
/// `cpu` covers CPU-bound stages (Parse / Analysis / Artifact) and, on a
/// ledger of its own, CacheNode producers; `io` covers I/O-bound work (Load).
/// The DAG admits at most `cpu` concurrent stages, at most `cpu` concurrent
/// producers and at most `io` concurrent I/O jobs. Defaults mirror the
/// scheduler's CPU / IO pool sizing.
#[derive(Clone, Copy, Debug)]
pub struct DagCapacityBudget {
    /// Maximum concurrent CPU-bound stages, and separately of producers.
    pub cpu: u32,
    /// Maximum concurrent I/O-bound jobs.
    pub io: u32,
}

impl Default for DagCapacityBudget {
    fn default() -> Self {
        Self { cpu: 8, io: 8 }
    }
}

/// Resource class a `WorkKind` consumes for admission accounting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceClass {
    /// CPU-bound stages: Parse, Analysis, Artifact.
    Cpu,
    /// I/O-bound: Load.
    Io,
    /// CacheNode producers. A producer may block on a dependency, a flight or
    /// a host lock whose holder is waiting for a stage. Stages block on none
    /// of those, so a producer never holds a permit a stage needs.
    Producer,
}

impl ResourceClass {
    /// Resource class consumed by `kind`.
    pub fn for_work_kind(kind: WorkKind) -> Self {
        match kind {
            WorkKind::Load => ResourceClass::Io,
            WorkKind::Parse | WorkKind::Analysis | WorkKind::Artifact => ResourceClass::Cpu,
            WorkKind::CacheNode => ResourceClass::Producer,
        }
    }
}

/// A single accounting source for admission permits.
///
/// Releases its permit count exactly once. If the holder calls
/// [`Self::release`], the inner counters are consumed and a subsequent
/// `Drop` is a no-op. If the holder drops without calling `release`,
/// the destructor releases the permits. Calling `release` more than
/// once is impossible at the type level — the method takes `self` by
/// value.
///
/// The reservation carries the [`ResourceClass`] it was taken
/// against so multi-class admission accounting stays type-safe — the
/// holder cannot accidentally release against the wrong pool. The
/// aggregate in-flight count is derived from the per-class counters on
/// read, so a reservation owns exactly one counter.
pub struct DagCapacityReservation {
    pub(super) permits: u32,
    pub(super) class: ResourceClass,
    /// Per-class counter (cpu or io) — decremented on release; taken
    /// (`None`) once the permits are given back.
    pub(super) class_counter: Option<Arc<AtomicU64>>,
    /// The ledger's reset epoch and its value when this reservation was
    /// taken. [`crate::dag::SchedulerDag::clear`] replaces the counters and
    /// bumps the epoch. An already-selected release debits its old counter;
    /// a release selecting after reset gives nothing back and does not wake.
    pub(super) epoch: Option<(Arc<AtomicU64>, u64)>,
    /// Notifies readiness after the final shared permit returns, including
    /// releases outside a DAG operation. The scheduler defers callbacks across
    /// its lifecycle locks; standalone DAGs can omit the callback.
    pub(super) progress: Option<std::task::Waker>,
}

impl DagCapacityReservation {
    /// Number of permits held by this reservation.
    pub fn permits(&self) -> u32 {
        self.permits
    }

    /// Resource class this reservation was taken against.
    pub fn class(&self) -> ResourceClass {
        self.class
    }

    /// Release the held permits. Consumes the reservation; further
    /// release is statically impossible (the method takes `self`).
    pub fn release(mut self) {
        self.give_back();
    }

    /// Return the permits to the class counter exactly once (subsequent
    /// calls find the counter taken), unless the ledger was reset since the
    /// reservation was taken. Saturating, so a straggler can never wrap a
    /// counter.
    fn give_back(&mut self) {
        let current = self.release_epoch_is_current();
        self.return_capacity(current);
    }

    fn release_epoch_is_current(&self) -> bool {
        self.epoch
            .as_ref()
            .is_none_or(|(epoch, taken_at)| epoch.load(Ordering::Acquire) == *taken_at)
    }

    /// The eligibility read can precede a reset; this reservation's counter
    /// must therefore remain an independent ledger after that reset.
    fn return_capacity(&mut self, current: bool) {
        let permits = self.permits as u64;
        let saturating_sub = |counter: &AtomicU64| {
            let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                Some(value.saturating_sub(permits))
            });
        };
        if let Some(counter) = self.class_counter.take() {
            if current {
                saturating_sub(&counter);
                if let Some(progress) = &self.progress {
                    progress.wake_by_ref();
                }
            }
        }
    }
}

impl std::fmt::Debug for DagCapacityReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DagCapacityReservation")
            .field("permits", &self.permits)
            .field("class", &self.class)
            .field("released", &self.class_counter.is_none())
            .finish()
    }
}

impl Drop for DagCapacityReservation {
    fn drop(&mut self) {
        self.give_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::SchedulerDag;
    use std::sync::atomic::AtomicUsize;
    use std::task::{Wake, Waker};

    struct Progress(AtomicUsize);

    impl Wake for Progress {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_release_selected_before_reset_debits_only_its_original_ledger() {
        for class in [
            ResourceClass::Cpu,
            ResourceClass::Io,
            ResourceClass::Producer,
        ] {
            for loan in [false, true] {
                let mut dag = SchedulerDag::with_budget(DagCapacityBudget { cpu: 2, io: 2 });
                let probe = Arc::new(Progress(AtomicUsize::new(0)));
                dag.set_progress_waker(Waker::from(Arc::clone(&probe)));
                let reserve = |dag: &SchedulerDag| {
                    if loan {
                        dag.loan_capacity_for_class(class).unwrap()
                    } else {
                        dag.try_reserve_for_class(class).unwrap()
                    }
                };
                let mut selected = reserve(&dag);
                let late = reserve(&dag);
                let eligible = selected.release_epoch_is_current();
                assert!(eligible);
                dag.clear();
                let replacement = reserve(&dag);
                let in_flight = |dag: &SchedulerDag| match class {
                    ResourceClass::Cpu => dag.in_flight_cpu_permits(),
                    ResourceClass::Io => dag.in_flight_io_permits(),
                    ResourceClass::Producer => dag.in_flight_producer_permits(),
                };
                assert_eq!(in_flight(&dag), 1);

                // Resume the actual debit after a replacement admission. The
                // pre-reset eligibility decision can still select an advisory wake.
                selected.return_capacity(eligible);
                assert_eq!(
                    in_flight(&dag),
                    1,
                    "{class:?}, loan={loan}: old eligibility cannot debit the new ledger"
                );
                assert_eq!(probe.0.load(Ordering::SeqCst), 1);
                drop(selected);
                assert_eq!(in_flight(&dag), 1, "debit consumes the counter before Drop");
                assert_eq!(probe.0.load(Ordering::SeqCst), 1);

                assert!(!late.release_epoch_is_current());
                drop(late);
                assert_eq!(in_flight(&dag), 1);
                assert_eq!(
                    probe.0.load(Ordering::SeqCst),
                    1,
                    "post-reset selection does not wake"
                );
                drop(replacement);
                assert_eq!(in_flight(&dag), 0);
                assert_eq!(probe.0.load(Ordering::SeqCst), 2);
            }
        }
    }
}
