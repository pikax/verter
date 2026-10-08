//! The fact a flow-return read establishes for its consumer, and the
//! scoped observation of a flow read whose consumer may still decline it.
//!
//! # Producer guarantees
//!
//! A flow-return demand answers through [`FlowReturnStep`]; its consumer
//! reads that step together with the completeness its read observed as ONE
//! [`FactResult`] ([`ObservedFlowRead::fact`]):
//!
//! - **Complete** — an undegraded evaluated return whose read observed no
//!   partiality: the exact return, warm-capable.
//! - **Approximate** — a USABLE UNFINISHED value. The documented
//!   interpretation is the evaluated return itself: a degraded success
//!   keeps every modelled position exact and carries the typed unresolved
//!   marker at a position it could not type ([`PartialReason::FlowReturnUninferred`](crate::semantic_query::PartialReason::FlowReturnUninferred)),
//!   or its member set is complete while a member's type may be unverified
//!   ([`PartialReason::FlowReturnUnverified`](crate::semantic_query::PartialReason::FlowReturnUnverified)), or it is the checker's
//!   recovery after an operation exhausted its allowance
//!   ([`PartialReason::OperationBudget`](crate::semantic_query::PartialReason::OperationBudget)). An undegraded value whose read
//!   observed partiality (a withheld proof, an unverified dependency) is
//!   approximate by the read's own classes. An approximation is never an
//!   exact answer and never warms.
//! - **Unavailable** — NO VALUE: a typed [`FlowReturnFailure`] or an
//!   in-flight recursive hold surfacing at its consumer. Always carries
//!   [`PartialReason::FlowReturnNoSurface`](crate::semantic_query::PartialReason::FlowReturnNoSurface), plus whatever the read
//!   observed.
//! - **Aborted** — a read that observed cancellation or a superseded/torn
//!   view is not a fact at all ([`ExecutionAbort`]): the consumer publishes
//!   nothing from it.
//!
//! The arm decides; the causes explain. A consumer never re-derives
//! availability from the reason classes.
//!
//! # Discarded probes
//!
//! A read the consumer may DECLINE — a probe with a fall-through route
//! that re-derives the authoritative answer and its own rails — runs under
//! [`ProjectSemanticDispatch::observe_flow_read`]. Its observations (the
//! per-cold-compute completeness, the build-local taint frame and the
//! deferred request sticky) are held aside, and the consumer states its
//! choice by type: [`ObservedFlowRead::adopt`] joins them into the
//! enclosing build exactly as an unscoped read would have, and
//! [`ObservedFlowRead::discard`] drops them because the fall-through
//! route owns the answer. A failed probe therefore never marks the
//! enclosing composition partial, and an adopted one never hides its
//! partiality.

use super::flow_return::{degradation_reason, NO_VALUE_REASON};
use super::{BuildLocalTaint, BuildLocalTaintGuard, ProjectSemanticDispatch};
use crate::semantic_query::surface_resolution::NonEmptyReasons;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FlowReturnResult, FlowReturnStep, ResultCompleteness,
};

/// One flow read run under private observation rails, awaiting its
/// consumer's adopt-or-discard decision.
#[must_use = "a scoped flow read must be adopted or discarded"]
pub(super) struct ObservedFlowRead<T> {
    value: T,
    completeness: ResultCompleteness,
    frame: BuildLocalTaint,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Run `read` with its observations held aside: the request sticky is
    /// deferred, and the per-cold-compute completeness and build-local
    /// taint the read produces are captured instead of joining the
    /// enclosing build. The returned read states, by
    /// [`ObservedFlowRead::adopt`] or [`ObservedFlowRead::discard`],
    /// whether they join it.
    pub(super) fn observe_flow_read<T>(&self, read: impl FnOnce() -> T) -> ObservedFlowRead<T> {
        let deferred_sticky = crate::request_context::DeferredPartialStickyScope::enter();
        let completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
        let frame = BuildLocalTaintGuard::push(&self.build_local_taint);
        let value = read();
        let frame = frame.finish();
        let completeness = crate::request_context::current_cold_compute_completeness();
        completeness_scope.discard();
        drop(deferred_sticky);
        ObservedFlowRead {
            value,
            completeness,
            frame,
        }
    }
}

impl<T> ObservedFlowRead<T> {
    /// The consumer USES the value: the read's observations join the
    /// enclosing build and the request exactly as an unscoped read's would.
    pub(super) fn adopt<C: crate::resolver_core::ResolverCapabilities>(
        self,
        dispatch: &ProjectSemanticDispatch<'_, C>,
    ) -> T {
        crate::request_context::fold_result_completeness(self.completeness);
        dispatch.fold_observed_frame_into_top(&self.frame);
        self.value
    }

    /// The consumer DECLINES the value and a fall-through route owns the
    /// answer: the read's observations are dropped.
    pub(super) fn discard(self) -> T {
        self.value
    }
}

impl ObservedFlowRead<FlowReturnStep> {
    /// The fact this flow-return read established. See the module
    /// documentation for each arm's guarantee.
    pub(super) fn fact(&self) -> Result<FactResult<&FlowReturnResult>, ExecutionAbort> {
        flow_return_fact(&self.value, self.completeness)
    }
}

/// The fact a flow-return step establishes under the completeness its read
/// observed. The ONE mapping from the flow producer's typed outcome to its
/// consumer-facing quality.
pub(super) fn flow_return_fact(
    step: &FlowReturnStep,
    observed: ResultCompleteness,
) -> Result<FactResult<&FlowReturnResult>, ExecutionAbort> {
    if let Some(abort) = ExecutionAbort::observed_in(observed) {
        return Err(abort);
    }
    let observed_reasons = NonEmptyReasons::new(observed.reasons());
    Ok(match step {
        FlowReturnStep::Complete(result) => {
            let own = result
                .degradation()
                .map(|degradation| NonEmptyReasons::of(degradation_reason(degradation)));
            match join(own, observed_reasons) {
                None => FactResult::complete(result),
                Some(causes) => FactResult::approximate(result, causes),
            }
        }
        FlowReturnStep::NoValue(_) | FlowReturnStep::Hold(_) => {
            let no_value = NonEmptyReasons::of(NO_VALUE_REASON);
            FactResult::unavailable(match observed_reasons {
                Some(observed) => no_value.union(observed),
                None => no_value,
            })
        }
    })
}

fn join(left: Option<NonEmptyReasons>, right: Option<NonEmptyReasons>) -> Option<NonEmptyReasons> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.union(right)),
        (one, None) | (None, one) => one,
    }
}

#[cfg(test)]
#[path = "flow_return_fact_tests.rs"]
mod flow_return_fact_tests;
