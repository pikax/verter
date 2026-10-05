//! Outcome of a per-symbol body demand: a completed run, or a broken lease that
//! lowered nothing.

use std::sync::Arc;

/// Outcome of a per-symbol body DEMAND ([`DeclBodyMemo::demand_and_commit`])
/// as seen by a caller that needs to DISTINGUISH the two `None`-shaped miss
/// classes (the locator-deref path, which must not collapse a transient
/// ReturnOnly into a cacheable resolution result):
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some` is the
///   demanded decl; `None` is a GENUINE, cacheable miss (the symbol is not
///   inventoried, or the run produced a fatal-parse empty).
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken: the demand
///   ran NOTHING and committed NOTHING (`ReturnOnly`). A caller must route
///   this to a no-warm signal, never treat it as a genuine miss.
pub enum DemandOutcome<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

impl<D> DemandOutcome<D> {
    /// Collapse to the plain `Option` API: a lease-miss reads as `None`. Used
    /// by the broad `Option`-returning demand accessors whose consumers do
    /// NOT distinguish the transient ReturnOnly from a genuine miss (the
    /// per-symbol demand cell already fails closed by evicting the poisoned
    /// cell, so a later demand under a live lease recovers).
    ///
    /// The `LeaseMiss` arm marks the generalized non-cacheability rail: this
    /// is the ONE central collapse point for the plain type / value /
    /// augmentation decl-body accessors, so a transient broken-lease read
    /// consumed by an enclosing traced compute refuses that compute's
    /// shared-cache admission (structural, not per-name). A `Ready(None)`
    /// genuine absence stays cacheable and marks nothing.
    pub fn into_option(self) -> Option<Arc<D>> {
        match self {
            DemandOutcome::Ready(value) => value,
            DemandOutcome::LeaseMiss => {
                crate::facts::reuse::note_non_cacheable_read_fan_out(
                    crate::facts::reuse::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
        }
    }
}
