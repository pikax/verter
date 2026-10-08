//! The projection fact mapping: presence-only projections keep a usable
//! subset, value projections do not, and aborts publish nothing.

use super::projection_fact;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FactStatus, PartialReason, PartialReasonSet, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, ResultCompleteness, SemanticNodeData,
    SemanticQueryKey,
};
use crate::semantic_query_memo::SemanticGraphStore;

fn keys() -> (SemanticQueryKey, SemanticQueryKey, SemanticQueryKey) {
    let graph = SemanticGraphStore::new();
    let base = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let context = |mode| ProjectionReductionContext::published(mode);
    (
        SemanticQueryKey::ProjectPath {
            base,
            path: Vec::new().into(),
            context: context(ProjectionMode::Shallow),
        },
        SemanticQueryKey::KeyOf {
            base,
            context: context(ProjectionMode::Shallow),
        },
        SemanticQueryKey::ProjectPath {
            base,
            path: Vec::new().into(),
            context: context(ProjectionMode::Expanded),
        },
    )
}

fn partial() -> ResultCompleteness {
    ResultCompleteness::partial(PartialReasonSet::MISSING_DEPENDENCY)
}

#[test]
fn a_complete_read_is_exact_for_every_projection() {
    let (shallow, key_of, expanded) = keys();
    for key in [&shallow, &key_of, &expanded] {
        let fact = projection_fact(key, Some(1), ResultCompleteness::Complete).unwrap();
        assert_eq!(fact, FactResult::Complete(1));
    }
}

#[test]
fn a_partial_member_or_key_projection_is_a_presence_only_approximation() {
    let (shallow, key_of, _) = keys();
    for key in [&shallow, &key_of] {
        let fact = projection_fact(key, Some(1), partial()).unwrap();
        assert!(matches!(fact.status(), FactStatus::Approximate(_)));
        assert_eq!(fact.exact(), None);
    }
}

#[test]
fn a_partial_value_projection_has_no_usable_representation() {
    let (_, _, expanded) = keys();
    let fact = projection_fact(&expanded, Some(1), partial()).unwrap();
    assert!(matches!(fact.status(), FactStatus::Unavailable(_)));
}

#[test]
fn a_build_without_a_value_is_unavailable_even_when_the_read_was_clean() {
    let (shallow, ..) = keys();
    let fact = projection_fact::<u8>(&shallow, None, ResultCompleteness::Complete).unwrap();
    assert!(matches!(fact.status(), FactStatus::Unavailable(_)));
}

#[test]
fn cancellation_and_torn_views_abort_instead_of_publishing() {
    let (shallow, ..) = keys();
    let cancelled = ResultCompleteness::partial(PartialReasonSet::CANCELLED);
    assert_eq!(
        projection_fact(&shallow, Some(1), cancelled).unwrap_err(),
        ExecutionAbort::Cancelled
    );
    let torn = ResultCompleteness::partial(PartialReason::SupersededGeneration.bit());
    assert_eq!(
        projection_fact(&shallow, Some(1), torn).unwrap_err(),
        ExecutionAbort::Superseded
    );
}
