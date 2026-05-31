#![deny(missing_docs)]
//! [`WaitAudit`] — per-request lock + queue contention attribution.
//!
//! Populated only when the host's `audit_timing_capture` flag is on.
//! When the flag is off the [`crate::record::RequestAuditRecord::waits`]
//! envelope field stays `None` and producers short-circuit before
//! their `Instant::now()` capture so the zero-cost path is preserved.

use serde::{Deserialize, Serialize};

use crate::record::u64_as_decimal_string;

/// Per-request wall-clock attribution for the locks and scheduler
/// queues the request blocked on.
///
/// The fields are nanosecond totals across the full audited request
/// window. They aggregate across the multiple shard / canonical
/// mutex acquisitions a single request can perform — `lock_wait_ns`
/// is the SUM of every observed lock-wait, not the maximum, because
/// the audit consumer wants to know how much wall-clock the request
/// spent waiting on contention regardless of which lock contributed
/// it.
///
/// `queue_wait_ns` is derived from the per-dispatch
/// `SchedulerAudit::queue_dwell_ms` observations summed for this
/// request — every dispatch contributes its dwell, so retries that
/// re-queue add their own wait-time. Native-only; on WASM there is
/// no scheduler so the substrate-level
/// [`crate::record::RequestAuditRecord::waits`] field is `None`
/// regardless of the timing flag.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct WaitAudit {
    /// Cumulative wall-clock spent acquiring per-cache shard / canonical
    /// mutexes during the audited window, in nanoseconds. Producers
    /// time the `lock()` call with `Instant::now()` only when the
    /// timing flag is on; otherwise this counter is never incremented.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub lock_wait_ns: u64,
    /// Cumulative scheduler queue dwell time for the audited request,
    /// in nanoseconds. Derived from the sum of every
    /// `SchedulerAudit::queue_dwell_ms` observed at every dispatch
    /// site (initial dispatch plus any retries). Always `0` on WASM.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub queue_wait_ns: u64,
    /// Total number of lock acquisitions observed for the audited
    /// request. Bumped exactly once per acquisition through the
    /// session-side helper, regardless of which shard or canonical
    /// owned the mutex.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub lock_acquisitions: u64,
}
