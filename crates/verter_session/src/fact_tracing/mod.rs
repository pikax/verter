//! The single fact-tracing runtime: the per-thread tracer and recorder
//! stacks ([`tracing`]), the typed refusal-observation scopes
//! ([`refusal_scope`]), and the non-cacheable read fan-out that marks both.
//!
//! Every non-cacheability mark reaches the running thread's tracers by a
//! DIRECT call from here — there is no process-global receiver or callback.
//! Source-side code never calls into this module: it returns its refusal
//! evidence by value ([`SourceRead`]) and the first engine consumer applies
//! it with [`consume_source_read`].
//!
//! This module depends only on `verter_session_query`, the audit leaf and
//! std, so it carries no host, workspace, scheduler, parser or semantic
//! dependency.

pub(crate) mod refusal_scope;
pub(crate) mod tracing;

pub(crate) use refusal_scope::{replay_reuse_refusal, RefusalObservationScope};

use verter_session_query::facts::fact_read_set::NonCacheablePropagation;
use verter_session_query::facts::reuse::NonCacheableReadReason;
use verter_session_query::source::demand::SourceRead;

/// Mark every active tracer on the current thread's stack as having
/// consumed a NON-CACHEABLE read — the by-value rail enclosing traced cold
/// computes consult to refuse shared-cache admission — and record the typed
/// `reason` into every active [`RefusalObservationScope`], so a producer can
/// classify its result's reuse rail instead of inferring it from a boolean
/// that a fenced serve and a broken lease set identically. No-op when no
/// tracer, recorder or scope is active.
#[inline]
pub(crate) fn note_non_cacheable_read_fan_out(reason: NonCacheableReadReason) {
    refusal_scope::record_refusal(reason);
    note_non_cacheable_propagation(reason.propagation());
}

/// Apply an already-classified refusal to the active tracer stack. Cache
/// owners use this after a typed `ReturnOnly` result escapes its own tracing
/// scope; ordinary read sites use [`note_non_cacheable_read_fan_out`] so
/// their closed reason enum selects the propagation policy.
#[inline]
pub(crate) fn note_non_cacheable_propagation(propagation: NonCacheablePropagation) {
    tracing::apply_non_cacheable_propagation(propagation);
}

/// Take a source-side read's value, first applying the refusal evidence it
/// carries to the running thread's tracers and scopes. The first engine
/// consumer of a source read calls this before any early return, admission
/// decision or memo insertion.
#[inline]
pub(crate) fn consume_source_read<T>(read: SourceRead<T>) -> T {
    if let Some(reason) = read.refusal {
        note_non_cacheable_read_fan_out(reason);
    }
    read.value
}
