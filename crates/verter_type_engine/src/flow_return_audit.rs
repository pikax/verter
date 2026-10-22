#![deny(missing_docs)]
//! Cold-path flow-return audit emission helpers.
//!
//! The three helpers pair a per-request counter bump (the
//! [`crate::request_context::RequestContext`] atomics the
//! `FlowReturnInference` payload snapshots at finalize) with a typed
//! [`StructuredAuditEvent`] push onto the per-request accumulator.
//!
//! Cold-vs-warm audit contract (the block's exit-acceptance property):
//! every helper is called from a COLD-path site only — the warm family
//! hit in `execute_flow_return` returns before any of them — and the
//! event payload is constructed ONLY when an accumulator is installed.
//! Without an accumulator the helpers bump the (allocation-free)
//! atomics when a request context exists and construct NOTHING; the
//! `flow_return_audit_emission` allocator canary in
//! `crates/verter_session/tests/allocator_canaries.rs` pins the
//! zero-allocation half, and the warm-hit behavioral test pins the
//! no-`FlowReturnStarted` half.

//!
//! All three are optional capture: without `semantic-observe` they read no
//! request context and build no event.

#[cfg(feature = "semantic-observe")]
use std::sync::atomic::Ordering;
#[cfg(feature = "semantic-observe")]
use std::sync::Arc;

#[cfg(feature = "semantic-observe")]
use verter_audit::{FlowSliceBudgetAxisTag, StructuredAuditEvent};

use verter_session_query::flow::peeker::FlowSliceBudgetExceeded;

/// Record one cold whole-function flow evaluation: bump the
/// per-request `flow_return_cold_computes` counter and, when an
/// accumulator is installed, push a
/// [`StructuredAuditEvent::FlowReturnStarted`].
///
/// COLD-PATH ONLY caller contract — the warm family hit must return
/// before reaching this helper.
#[inline(always)]
pub fn record_flow_return_started<M: verter_audit::observe::ObserveExec>(
    observation: &crate::request_observers::CaptureTargets,
    canonical_id: &str,
    function_symbol: &str,
) {
    let _ = observation;
    #[cfg(not(feature = "semantic-observe"))]
    let _ = (canonical_id, function_symbol);
    #[cfg(feature = "semantic-observe")]
    if M::CAPTURED {
        record_flow_return_started_captured(observation, canonical_id, function_symbol);
    }
}

#[cfg(feature = "semantic-observe")]
fn record_flow_return_started_captured(
    observation: &crate::request_observers::CaptureTargets,
    canonical_id: &str,
    function_symbol: &str,
) {
    let _ = observation;
    if let Some(ctx) = observation.request.as_deref() {
        ctx.flow_return_cold_computes
            .fetch_add(1, Ordering::Relaxed);
    }
    let Some(accumulator) = observation.accumulator.as_deref() else {
        return;
    };
    accumulator.push_structured_event(StructuredAuditEvent::FlowReturnStarted {
        canonical_id: Arc::from(canonical_id),
        function_symbol: Arc::from(function_symbol),
    });
}

/// Record one flow-slice budget refusal: bump the per-request
/// `flow_return_budget_exceeded` counter and, when an accumulator is
/// installed, push a
/// [`StructuredAuditEvent::FlowSliceBudgetExceeded`].
#[inline(always)]
pub fn record_flow_slice_budget_exceeded<M: verter_audit::observe::ObserveExec>(
    observation: &crate::request_observers::CaptureTargets,
    exceeded: &FlowSliceBudgetExceeded,
) {
    let _ = observation;
    #[cfg(not(feature = "semantic-observe"))]
    let _ = exceeded;
    #[cfg(feature = "semantic-observe")]
    if M::CAPTURED {
        record_flow_slice_budget_exceeded_captured(observation, exceeded);
    }
}

#[cfg(feature = "semantic-observe")]
fn record_flow_slice_budget_exceeded_captured(
    observation: &crate::request_observers::CaptureTargets,
    exceeded: &FlowSliceBudgetExceeded,
) {
    let _ = observation;
    if let Some(ctx) = observation.request.as_deref() {
        ctx.flow_return_budget_exceeded
            .fetch_add(1, Ordering::Relaxed);
    }
    let Some(accumulator) = observation.accumulator.as_deref() else {
        return;
    };
    accumulator.push_structured_event(StructuredAuditEvent::FlowSliceBudgetExceeded {
        axis: match exceeded.axis {
            verter_session_query::flow::peeker::FlowSliceBudgetAxis::SelectedNodes => {
                FlowSliceBudgetAxisTag::SelectedNodes
            }
            verter_session_query::flow::peeker::FlowSliceBudgetAxis::ValueStates => {
                FlowSliceBudgetAxisTag::ValueStates
            }
        },
        limit: exceeded.limit,
        observed: exceeded.observed,
    });
}

/// Record one coinductive flow-cycle re-entry hold: bump the
/// per-request `flow_return_cycle_reentries` counter and, when an
/// accumulator is installed, push a
/// [`StructuredAuditEvent::FlowCycleSentinelHit`].
///
/// `cycle_id` is the request-scoped in-flight obligation frame index
/// the re-entry targeted.
#[inline(always)]
pub fn record_flow_cycle_reentry<M: verter_audit::observe::ObserveExec>(
    observation: &crate::request_observers::CaptureTargets,
    cycle_id: u32,
    function_symbol: &str,
) {
    let _ = observation;
    #[cfg(not(feature = "semantic-observe"))]
    let _ = (cycle_id, function_symbol);
    #[cfg(feature = "semantic-observe")]
    if M::CAPTURED {
        record_flow_cycle_reentry_captured(observation, cycle_id, function_symbol);
    }
}

#[cfg(feature = "semantic-observe")]
fn record_flow_cycle_reentry_captured(
    observation: &crate::request_observers::CaptureTargets,
    cycle_id: u32,
    function_symbol: &str,
) {
    let _ = observation;
    if let Some(ctx) = observation.request.as_deref() {
        ctx.flow_return_cycle_reentries
            .fetch_add(1, Ordering::Relaxed);
    }
    let Some(accumulator) = observation.accumulator.as_deref() else {
        return;
    };
    accumulator.push_structured_event(StructuredAuditEvent::FlowCycleSentinelHit {
        cycle_id,
        function_symbol: Arc::from(function_symbol),
    });
}
