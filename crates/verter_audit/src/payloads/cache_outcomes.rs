#![deny(missing_docs)]
//! Cache-outcome discriminator used by structured events and
//! per-cache attribution counters.

use serde::{Deserialize, Serialize};

/// Cache-outcome discriminator for per-event tallies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum CacheOutcomeKind {
    /// Warm cache hit.
    Hit,
    /// Cache miss (no entry present).
    Miss,
    /// Joined a peer's in-flight slot and waited.
    JoinedWait,
    /// Observed a sentinel (placeholder) entry.
    Sentinel,
    /// Performed a cold build from source.
    ColdBuild,
    /// Retry loop after an in-flight slot was aborted.
    InflightAbortedRetry,
    /// Cold entry reaped during generation reconciliation.
    ColdAbortSwept,
    /// Path-dependent outcome — the materialiser's depth fuse
    /// tripped, the owner scope was unloaded mid-compute, or a
    /// dispatch sub-call returned `Recursive`. Non-cacheable.
    Tainted,
}
