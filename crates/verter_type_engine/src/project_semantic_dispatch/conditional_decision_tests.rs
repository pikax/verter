//! A conditional query read is a complete value or an unavailable reading
//! naming why; a partial read never offers its value, and a cancelled or
//! torn read aborts.

use super::conditional_read_fact;
use crate::semantic_query::{
    CacheRead, ExecutionAbort, FactResult, PartialReasonSet, PrimitiveKind, QueryError,
    QueryResult, SemanticNodeData,
};
use crate::semantic_query_memo::SemanticGraphStore;
use std::sync::Arc;

fn read(
    value: QueryResult<crate::semantic_query::SemanticNodeId>,
) -> CacheRead<QueryResult<crate::semantic_query::SemanticNodeId>> {
    CacheRead::from_value_and_signature(value, Arc::from([]))
}

fn partial(
    value: QueryResult<crate::semantic_query::SemanticNodeId>,
    reasons: PartialReasonSet,
) -> CacheRead<QueryResult<crate::semantic_query::SemanticNodeId>> {
    let mut read = read(value);
    read.result_is_partial = true;
    read.partial_reasons = reasons;
    read
}

#[test]
fn a_finished_read_is_the_complete_value() {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_eq!(
        conditional_read_fact(read(QueryResult::Value(number))),
        Ok(FactResult::Complete(number))
    );
}

#[test]
fn a_partial_read_discards_its_value_and_names_why() {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let fact = conditional_read_fact(partial(
        QueryResult::Value(number),
        PartialReasonSet::BUDGET_EXCEEDED,
    ));
    let Ok(FactResult::Unavailable { causes }) = fact else {
        panic!("a partial conditional read is no reading: {fact:?}");
    };
    assert_eq!(
        causes.get(),
        PartialReasonSet::UNDECIDED_CONDITIONAL.union(PartialReasonSet::BUDGET_EXCEEDED)
    );
}

#[test]
fn a_failed_or_recursive_read_is_unavailable() {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    for value in [
        QueryResult::Error(QueryError::Miss),
        QueryResult::Recursive(number),
    ] {
        let fact = conditional_read_fact(read(value));
        let Ok(FactResult::Unavailable { causes }) = fact else {
            panic!("no value is no reading: {fact:?}");
        };
        assert!(causes
            .get()
            .contains(PartialReasonSet::UNDECIDED_CONDITIONAL));
    }
}

#[test]
fn a_cancelled_or_torn_read_aborts() {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_eq!(
        conditional_read_fact(partial(
            QueryResult::Value(number),
            PartialReasonSet::CANCELLED
        )),
        Err(ExecutionAbort::Cancelled)
    );
    assert_eq!(
        conditional_read_fact(partial(
            QueryResult::Value(number),
            PartialReasonSet::SUPERSEDED_GENERATION
        )),
        Err(ExecutionAbort::Superseded)
    );
}
