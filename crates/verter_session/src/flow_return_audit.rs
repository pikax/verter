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

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_audit::{FlowSliceBudgetAxisTag, StructuredAuditEvent};

use verter_semantic::analysis::flow::peeker::FlowSliceBudgetExceeded;

/// `true` when a structured-event accumulator is installed for the
/// current request. The event-construction gate: all payload
/// allocation for the three flow events happens strictly behind it.
fn accumulator_installed() -> bool {
    crate::request_context::current_accumulator().is_some()
}

/// Record one cold whole-function flow evaluation: bump the
/// per-request `flow_return_cold_computes` counter and, when an
/// accumulator is installed, push a
/// [`StructuredAuditEvent::FlowReturnStarted`].
///
/// COLD-PATH ONLY caller contract — the warm family hit must return
/// before reaching this helper.
pub fn record_flow_return_started(canonical_id: &Arc<str>, function_symbol: &Arc<str>) {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.flow_return_cold_computes
            .fetch_add(1, Ordering::Relaxed);
    }
    if !accumulator_installed() {
        return;
    }
    crate::host_manage::push_structured_event(StructuredAuditEvent::FlowReturnStarted {
        canonical_id: Arc::clone(canonical_id),
        function_symbol: Arc::clone(function_symbol),
    });
}

/// Record one flow-slice budget refusal: bump the per-request
/// `flow_return_budget_exceeded` counter and, when an accumulator is
/// installed, push a
/// [`StructuredAuditEvent::FlowSliceBudgetExceeded`].
pub fn record_flow_slice_budget_exceeded(exceeded: &FlowSliceBudgetExceeded) {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.flow_return_budget_exceeded
            .fetch_add(1, Ordering::Relaxed);
    }
    if !accumulator_installed() {
        return;
    }
    crate::host_manage::push_structured_event(StructuredAuditEvent::FlowSliceBudgetExceeded {
        axis: match exceeded.axis {
            verter_semantic::analysis::flow::peeker::FlowSliceBudgetAxis::ReturnSites => {
                FlowSliceBudgetAxisTag::ReturnSites
            }
            verter_semantic::analysis::flow::peeker::FlowSliceBudgetAxis::SelectedNodes => {
                FlowSliceBudgetAxisTag::SelectedNodes
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
pub fn record_flow_cycle_reentry(cycle_id: u32, function_symbol: &Arc<str>) {
    if let Some(ctx) = crate::request_context::current_request_context() {
        ctx.flow_return_cycle_reentries
            .fetch_add(1, Ordering::Relaxed);
    }
    if !accumulator_installed() {
        return;
    }
    crate::host_manage::push_structured_event(StructuredAuditEvent::FlowCycleSentinelHit {
        cycle_id,
        function_symbol: Arc::clone(function_symbol),
    });
}
