//! Capacity / aging types — child module of `dag`.
//!
//! Houses the admission-budget plumbing that the DAG consumes
//! during dispatch: the priority-tier aging configuration, the
//! split CPU / I/O resource budget, the per-`WorkKind` resource
//! class, and the typed reservation that decrements counters
//! exactly once on release. The types live here so the main
//! `dag.rs` file stays focused on the readiness DAG (admission,
//! dedup, gating, fan-out) rather than the capacity accounting
//! it consumes.
//!
//! Visibility note: every type is `pub` so the existing
//! `crate::dag::*` re-exports continue to work without callers
//! needing to know about the submodule split.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::WorkKind;

/// Configuration for priority-tier aging in the DAG.
#[derive(Clone, Debug)]
pub struct DagAgingConfig {
    /// Background entries older than this promote to Interactive.
    pub background_to_interactive: Duration,
    /// Maintenance entries older than this promote to Background.
    pub maintenance_to_background: Duration,
}

impl Default for DagAgingConfig {
    fn default() -> Self {
        Self {
            background_to_interactive: Duration::from_secs(10),
            maintenance_to_background: Duration::from_secs(30),
        }
    }
}

/// Admission-time capacity budget, split into two resource classes.
///
/// `cpu` covers CPU-bound work (Parse / Analysis / Artifact / CacheNode);
/// `io` covers I/O-bound work (Load). The DAG admits at most `cpu`
/// concurrent CPU jobs and at most `io` concurrent I/O jobs. Defaults
/// mirror the scheduler's CPU / IO pool sizing.
#[derive(Clone, Copy, Debug)]
pub struct DagCapacityBudget {
    /// Maximum concurrent CPU-bound jobs.
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
    /// CPU-bound: Parse, Analysis, Artifact, CacheNode.
    Cpu,
    /// I/O-bound: Load.
    Io,
}

impl ResourceClass {
    /// Resource class consumed by `kind`.
    pub fn for_work_kind(kind: WorkKind) -> Self {
        match kind {
            WorkKind::Load => ResourceClass::Io,
            WorkKind::Parse | WorkKind::Analysis | WorkKind::Artifact | WorkKind::CacheNode => {
                ResourceClass::Cpu
            }
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
/// holder cannot accidentally release against the wrong pool. Typed
/// reservations decrement BOTH the per-class counter and the aggregate
/// counter atomically (independent fetches, but consumed by the same
/// `release`/`Drop`).
pub struct DagCapacityReservation {
    pub(super) permits: u32,
    pub(super) class: ResourceClass,
    /// Per-class counter (cpu or io) — decremented on release.
    /// `None` for untyped reservations taken via the diagnostic
    /// `reserve_capacity` entry point that bypasses class accounting.
    pub(super) class_counter: Option<Arc<AtomicU64>>,
    /// Aggregate in-flight counter — always decremented on release.
    pub(super) counter: Option<Arc<AtomicU64>>,
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
        let permits = self.permits as u64;
        if let Some(counter) = self.class_counter.take() {
            counter.fetch_sub(permits, Ordering::AcqRel);
        }
        if let Some(counter) = self.counter.take() {
            counter.fetch_sub(permits, Ordering::AcqRel);
        }
    }
}

impl std::fmt::Debug for DagCapacityReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DagCapacityReservation")
            .field("permits", &self.permits)
            .field("class", &self.class)
            .field("released", &self.counter.is_none())
            .finish()
    }
}

impl Drop for DagCapacityReservation {
    fn drop(&mut self) {
        let permits = self.permits as u64;
        if let Some(counter) = self.class_counter.take() {
            counter.fetch_sub(permits, Ordering::AcqRel);
        }
        if let Some(counter) = self.counter.take() {
            counter.fetch_sub(permits, Ordering::AcqRel);
        }
    }
}
