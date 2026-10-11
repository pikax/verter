//! The fact a projection build establishes for its consumer.
//!
//! A projection build's value and the completeness its read observed are
//! read together as ONE [`FactResult`] ([`QueryBuildOutput::projection_fact`]).
//! The arm decides; the causes explain.
//!
//! # Per-projection interpretation
//!
//! Whether a partial projection is usable depends on what the projection
//! answers, not on the partial flag:
//!
//! | Projection | Partial read with a value |
//! | --- | --- |
//! | `ProjectPath` ending in `Shallow` (a one-level member surface) | **Approximate**: a presence-only member subset. Every member the build produced was proven under the projection's own rules; an omission proves nothing. |
//! | `KeyOf` (a key set) | **Approximate**: a presence-only key subset, same contract. |
//! | `ProjectPath` in any other terminal mode, `IndexedAccess`, `ProjectMember`, `MappedType`, `Instantiate`, `TypeOf`, every other key | **Unavailable**: the value is one type, and an unfinished one has no documented usable interpretation. |
//!
//! A complete read is **Complete**, including an exact empty or open domain
//! and a deferred carrier. A build with no value (an error or an in-flight
//! recursive hold) is **Unavailable**. A read that observed cancellation or
//! a superseded/torn view is no fact at all ([`ExecutionAbort`]).
//!
//! Membership does not establish member values or modifiers: those are
//! separate facts and carry their own guarantees.

use super::walk::QueryBuildOutput;
use crate::semantic_query::surface_resolution::NonEmptyReasons;
use crate::semantic_query::{
    ExecutionAbort, FactResult, PartialReason, ProjectionMode, QueryResult, ResultCompleteness,
    SemanticQueryKey,
};

/// What an unfinished projection of a given key can still honestly offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionInterpretation {
    /// A lower-bound enumeration: every produced member or key is real.
    PresenceOnly,
    /// One type: unfinished means no usable representation.
    ValueRequired,
}

/// The documented interpretation of an unfinished projection of `key`.
pub(crate) fn interpretation_of(key: &SemanticQueryKey) -> ProjectionInterpretation {
    match key {
        SemanticQueryKey::ProjectPath { context, .. }
            if context.mode == ProjectionMode::Shallow =>
        {
            ProjectionInterpretation::PresenceOnly
        }
        SemanticQueryKey::KeyOf { .. } => ProjectionInterpretation::PresenceOnly,
        _ => ProjectionInterpretation::ValueRequired,
    }
}

/// The fact `value` establishes under the completeness its read observed.
/// `value` is `None` when the build produced none.
pub(crate) fn projection_fact<T>(
    key: &SemanticQueryKey,
    value: Option<T>,
    observed: ResultCompleteness,
) -> Result<FactResult<T>, ExecutionAbort> {
    if let Some(abort) = ExecutionAbort::observed_in(observed) {
        return Err(abort);
    }
    let fault = || NonEmptyReasons::of(PartialReason::SemanticQueryFault);
    let causes = match observed {
        ResultCompleteness::Complete => None,
        ResultCompleteness::Partial(reasons) => {
            Some(NonEmptyReasons::new(reasons).unwrap_or_else(fault))
        }
    };
    Ok(match (value, causes) {
        (Some(value), None) => FactResult::complete(value),
        (Some(value), Some(causes)) => match interpretation_of(key) {
            ProjectionInterpretation::PresenceOnly => FactResult::approximate(value, causes),
            ProjectionInterpretation::ValueRequired => FactResult::unavailable(causes),
        },
        (None, causes) => FactResult::unavailable(causes.unwrap_or_else(fault)),
    })
}

impl<T> QueryBuildOutput<T> {
    /// The fact this build established for `key`. See the module
    /// documentation for each projection's interpretation.
    pub(crate) fn projection_fact(
        &self,
        key: &SemanticQueryKey,
    ) -> Result<FactResult<&T>, ExecutionAbort> {
        let value = match &self.result {
            QueryResult::Value(value) => Some(value),
            QueryResult::Recursive(_) | QueryResult::Error(_) => None,
        };
        projection_fact(key, value, self.completeness())
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> super::ProjectSemanticDispatch<'_, C> {
    /// Read the fact demanded by `key`, including its own availability.
    ///
    /// The read's status travels with its value on cold, warm and joined
    /// paths. An unrelated enclosing observation cannot change this fact.
    /// Dependency evidence still follows the shared read boundary.
    pub fn execute_fact(
        &self,
        key: SemanticQueryKey,
    ) -> Result<FactResult<crate::semantic_query::SemanticQueryValue>, ExecutionAbort> {
        self.record_dispatch_intent_counters(&key);
        let read = self.execute_via_cold_build_helper(key.clone());
        let observed = if read.result_is_partial {
            ResultCompleteness::partial(read.partial_reason_classes())
        } else {
            ResultCompleteness::Complete
        };
        if let Some(abort) = ExecutionAbort::observed_in(observed) {
            return Err(abort);
        }
        match read.value {
            QueryResult::Value(crate::semantic_query::SemanticQueryValue::BroadRuntime(value)) => {
                Ok(value
                    .into_fact()
                    .map(crate::semantic_query::SemanticQueryValue::BroadRuntime))
            }
            QueryResult::Value(value) => projection_fact(&key, Some(value), observed),
            QueryResult::Recursive(_) => {
                let own = NonEmptyReasons::of(PartialReason::SamePathRecursion);
                let causes = NonEmptyReasons::new(observed.reasons())
                    .map_or(own, |observed| own.union(observed));
                Ok(FactResult::unavailable(causes))
            }
            QueryResult::Error(error) => {
                let own = NonEmptyReasons::from_query_error(&error);
                let causes = NonEmptyReasons::new(observed.reasons())
                    .map_or(own, |observed| own.union(observed));
                if let Some(abort) =
                    ExecutionAbort::observed_in(ResultCompleteness::partial(causes.get()))
                {
                    return Err(abort);
                }
                Ok(FactResult::unavailable(causes))
            }
        }
    }
}

#[cfg(test)]
#[path = "projection_fact_tests.rs"]
mod projection_fact_tests;
