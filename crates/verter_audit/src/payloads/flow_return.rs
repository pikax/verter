#![deny(missing_docs)]
//! [`FlowReturnInferencePayload`] — typed payload for
//! [`crate::record::RequestKind::FlowReturnInference`] records.
//!
//! Populated by the session-side flow-return audited entry-point from
//! per-request counters bumped at the cold-path emission sites. Every
//! field is producer-populated per request; none is a reserved slot.

use serde::{Deserialize, Serialize};

/// Per-request counters for one flow-return inference request.
///
/// The three counters mirror the cold-path structured events one to
/// one: each cold whole-function evaluation bumps `cold_computes`
/// (paired with `FlowReturnStarted`), each flow-slice budget refusal
/// bumps `budget_exceeded_events` (paired with
/// `FlowSliceBudgetExceeded`), and each coinductive re-entry hold on
/// the obligation runtime bumps `cycle_reentry_holds` (paired with
/// `FlowCycleSentinelHit`). A warm family hit bumps nothing — the
/// cold-vs-warm audit contract's counter-side witness is
/// `cold_computes == 0`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct FlowReturnInferencePayload {
    /// Demanded function's symbol name (display attribution only —
    /// the record's `target_identity` carries the canonical file).
    pub function_symbol: String,
    /// Number of cold whole-function flow evaluations this request
    /// ran (root plus nested inline frames). `0` on a pure warm hit.
    pub cold_computes: u32,
    /// Number of flow-slice budget refusals observed (each one is a
    /// typed `Budget` failure routed through `ReturnOnly`).
    pub budget_exceeded_events: u32,
    /// Number of coinductive re-entry holds recorded on the shared
    /// obligation runtime (the flow-cycle sentinel).
    pub cycle_reentry_holds: u32,
}
