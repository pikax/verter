//! Adoption of a projection producer's fact at the memo settlement boundary.
//!
//! A complete outcome is exact, including empty/open domains and deferred
//! conditionals. An unfinished value needs an explicit producer guarantee:
//! presence subsets enumerate proven members/keys without proving absence;
//! positional lowering completes the authored structure and anchors every
//! unresolved position without weakening independently evaluated siblings;
//! checker recovery carries the pinned checker's diagnostic and recovery.
//! A traversal stopped before completing that structure has no guarantee.
//! Proof consumers (conditional decisions and classifications) require exact
//! facts even when a value consumer can use a positional approximation.
//!
//! Execution validity comes from the prerequisite's typed disposition and
//! ExecutionValidityScope, never from a compatibility reason classification.

use super::walk::QueryBuildOutput;
use crate::semantic_query::surface_resolution::NonEmptyReasons;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FactStatus, PartialReason, QueryError, QueryResult,
    ResultCompleteness, SemanticQueryKey, SemanticQueryValue,
};

/// The producer's bounded interpretation of its unfinished value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionInterpretation {
    /// Every carried member/key is present; omissions prove nothing.
    PresenceOnly,
    /// Structural lowering completed, retaining typed gaps at their positions.
    Positional,
    /// The typed checker-recovery outcome supplies its recovery contract.
    CheckerRecovery,
}

pub(crate) fn projection_fact<T>(
    _key: &SemanticQueryKey,
    value: Option<T>,
    observed: ResultCompleteness,
    guarantee: Option<ProjectionInterpretation>,
) -> FactResult<T> {
    let causes = NonEmptyReasons::new(observed.reasons())
        .unwrap_or_else(|| NonEmptyReasons::of(PartialReason::Propagated));
    match (value, observed.is_partial()) {
        (Some(value), false) => FactResult::complete(value),
        (Some(value), true) if guarantee.is_some() => FactResult::approximate(value, causes),
        _ => FactResult::unavailable(causes),
    }
}

/// Adopt once, before either the owner or a follower receives the result.
/// The compatibility summary is derived from this disposition by the read.
pub(crate) fn adopt_projection_result(
    key: &SemanticQueryKey,
    result: QueryResult<SemanticQueryValue>,
    observed: ResultCompleteness,
    guarantee: Option<ProjectionInterpretation>,
) -> (QueryResult<SemanticQueryValue>, FactStatus) {
    match result {
        QueryResult::Value(value) => {
            let fact = match value {
                SemanticQueryValue::FlowReturn(value) => {
                    let status = value.fact().status();
                    let status = if observed.is_partial() {
                        status.limited_by(
                            NonEmptyReasons::new(observed.reasons())
                                .unwrap_or_else(|| NonEmptyReasons::of(PartialReason::Propagated)),
                        )
                    } else {
                        status
                    };
                    status.with_value(SemanticQueryValue::FlowReturn(value))
                }
                SemanticQueryValue::BroadRuntime(value) if !observed.is_partial() => {
                    value.into_fact().map(SemanticQueryValue::BroadRuntime)
                }
                value => projection_fact(key, Some(value), observed, guarantee),
            };
            let status = fact.status();
            let result = match fact {
                FactResult::Complete(value) | FactResult::Approximate { value, .. } => {
                    QueryResult::Value(value)
                }
                FactResult::Unavailable { causes } => {
                    QueryResult::Error(QueryError::IncompleteSemanticOperand {
                        reasons: causes.get(),
                    })
                }
            };
            (result, status)
        }
        QueryResult::Error(error) => {
            let causes = NonEmptyReasons::from_query_error(&error);
            let causes =
                NonEmptyReasons::new(observed.reasons()).map_or(causes, |more| causes.union(more));
            // A miss after incomplete work is not a stable negative lookup.
            // Consumers may retain their authored carrier after a genuine
            // miss, but cannot discard an unfinished producer's observations.
            let error = if observed.is_partial()
                && matches!(error, QueryError::Miss | QueryError::RaiseMiss)
                && matches!(
                    key,
                    SemanticQueryKey::ResolveDecl(_) | SemanticQueryKey::Instantiate(_)
                ) {
                QueryError::IncompleteSemanticOperand {
                    reasons: causes.get(),
                }
            } else {
                error
            };
            (QueryResult::Error(error), FactStatus::Unavailable(causes))
        }
        QueryResult::Recursive(node) => {
            let causes = NonEmptyReasons::of(PartialReason::SamePathRecursion);
            let causes =
                NonEmptyReasons::new(observed.reasons()).map_or(causes, |more| causes.union(more));
            (
                QueryResult::Recursive(node),
                FactStatus::Unavailable(causes),
            )
        }
    }
}

impl<T> QueryBuildOutput<T> {
    pub(crate) fn with_positional_guarantee(mut self) -> Self {
        self.approximation = Some(ProjectionInterpretation::Positional);
        self
    }
}

impl<T> crate::semantic_query::CacheRead<QueryResult<T>> {
    /// The adopted fact, without reinterpreting compatibility summaries.
    pub fn into_fact(self) -> Result<FactResult<T>, ExecutionAbort> {
        let status = self.fact_status;
        let reasons = self.partial_reason_classes();
        match self.value {
            QueryResult::Value(value) => Ok(status.with_value(value)),
            QueryResult::Error(error) => {
                if let Some(abort) = ExecutionAbort::from_query_error(&error) {
                    Err(abort)
                } else {
                    Ok(FactResult::unavailable(
                        NonEmptyReasons::from_query_error(&error).union(
                            NonEmptyReasons::new(reasons)
                                .unwrap_or_else(|| NonEmptyReasons::from_query_error(&error)),
                        ),
                    ))
                }
            }
            QueryResult::Recursive(_) => Ok(FactResult::unavailable(
                NonEmptyReasons::of(PartialReason::SamePathRecursion).with(reasons),
            )),
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> super::ProjectSemanticDispatch<'_, C> {
    pub fn execute_fact(
        &self,
        key: SemanticQueryKey,
    ) -> Result<FactResult<SemanticQueryValue>, ExecutionAbort> {
        self.record_dispatch_intent_counters(&key);
        self.execute_via_cold_build_helper(key).into_fact()
    }
}

#[cfg(test)]
#[path = "projection_fact_tests.rs"]
mod projection_fact_tests;
