//! The projection fact mapping: presence-only projections keep a usable
//! subset, value projections do not, and aborts publish nothing.

use super::projection_fact;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FactStatus, PartialReasonSet, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, ResultCompleteness, SemanticNodeData, SemanticQueryKey,
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
    for _key in [&shallow, &key_of, &expanded] {
        let fact = projection_fact(Some(1), ResultCompleteness::Complete, None);
        assert_eq!(fact, FactResult::Complete(1));
    }
}

#[test]
fn a_partial_member_or_key_projection_is_a_presence_only_approximation() {
    let (shallow, key_of, _) = keys();
    for _key in [&shallow, &key_of] {
        let fact = projection_fact(
            Some(1),
            partial(),
            Some(super::ProjectionInterpretation::PresenceOnly),
        );
        assert!(matches!(fact.status(), FactStatus::Approximate(_, _)));
        assert_eq!(fact.exact(), None);
    }
}

#[test]
fn a_partial_value_projection_has_no_usable_representation() {
    let (shallow, key_of, expanded) = keys();
    for _key in [&shallow, &key_of, &expanded] {
        let fact = projection_fact(Some(1), partial(), None);
        assert!(matches!(fact.status(), FactStatus::Unavailable(_)));
    }
}

#[test]
fn a_build_without_a_value_is_unavailable_even_when_the_read_was_clean() {
    let (_shallow, ..) = keys();
    let fact = projection_fact::<u8>(None, ResultCompleteness::Complete, None);
    assert!(matches!(fact.status(), FactStatus::Unavailable(_)));
}

#[test]
fn cancellation_and_torn_views_abort_instead_of_publishing() {
    use crate::semantic_query::{CacheRead, QueryError, QueryResult};
    use std::sync::Arc;
    for abort in [ExecutionAbort::Cancelled, ExecutionAbort::Superseded] {
        let read = CacheRead::<QueryResult<u8>>::from_value_and_signature(
            QueryResult::Error(QueryError::ExecutionAborted(abort)),
            Arc::from([]),
        );
        assert_eq!(read.into_fact(), Err(abort));
    }
}

#[test]
fn positional_guarantees_keep_a_value_with_the_same_causes_as_an_unavailable_one() {
    let (_, _, _expanded) = keys();
    let fact = projection_fact(
        Some(1),
        partial(),
        Some(super::ProjectionInterpretation::Positional),
    );
    assert!(matches!(fact, FactResult::Approximate { value: 1, .. }));
    assert!(matches!(
        projection_fact(Some(1), partial(), None),
        FactResult::Unavailable { .. }
    ));
}

#[test]
fn typed_root_failures_cannot_be_certified_by_a_positional_forwarder() {
    let graph = crate::semantic_query_memo::SemanticGraphStore::new();
    let (_, _, key) = keys();
    for error in [
        crate::semantic_query::QueryError::Miss,
        crate::semantic_query::QueryError::UnmodeledPosition,
    ] {
        let node = graph.intern_node(crate::semantic_query::SemanticNodeData::Opaque(error));
        let (result, status) = super::adopt_projection_result(
            &graph,
            &key,
            crate::semantic_query::QueryResult::Value(
                crate::semantic_query::SemanticQueryValue::TypeNode(node),
            ),
            ResultCompleteness::Complete,
            Some(super::ProjectionInterpretation::Positional),
        );
        assert!(matches!(status, FactStatus::Unavailable(_)));
        assert!(matches!(
            result,
            crate::semantic_query::QueryResult::Error(_)
        ));
    }
}
