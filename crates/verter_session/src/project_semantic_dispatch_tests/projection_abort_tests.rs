//! An aborted projection build is no fact: a build whose read observed
//! cancellation or a superseded/torn view hands its readers the abort's
//! typed error, never the value it produced, and the read stays partial and
//! cold. A clean build of the same key afterwards is the exact value.

use std::sync::Arc;

use super::{resolve_decl_key, ProjectSemanticDispatch};
use crate::{FileLanguage, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{
    PartialReasonSet, PathSegment, ProjectionMode, ProjectionReductionContext, PropertyKey,
    QueryError, QueryResult, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};

const CANONICAL: &str = "/w/projection_fact.ts";

fn host_with_box() -> VerterHost {
    let host = VerterHost::new_standalone(Default::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: CANONICAL.to_owned(),
            source: Arc::from("export type Box = { obj: { b: 1; c: string } };\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("fixture must index");
    host
}

fn box_node(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
) -> SemanticNodeId {
    match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
        CANONICAL,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "Box",
    ))) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("Box resolves, got {other:?}"),
    }
}

fn obj_path(base: SemanticNodeId, mode: ProjectionMode) -> SemanticQueryKey {
    SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(vec![PathSegment::Member(PropertyKey::identifier("obj"))]),
        context: ProjectionReductionContext::published(mode),
    }
}

/// Runs `read` with every cold build of this host observing `reasons`, and
/// always clears the injection (even when an assertion inside fails).
fn with_observed_reasons<T>(
    host: &VerterHost,
    reasons: PartialReasonSet,
    read: impl FnOnce() -> T,
) -> T {
    struct Clear<'h>(&'h VerterHost);
    impl Drop for Clear<'_> {
        fn drop(&mut self) {
            *self
                .0
                .test_force
                .engine
                .force_result_partial_reasons_for_tests
                .lock() = PartialReasonSet::empty();
        }
    }
    *host
        .test_force
        .engine
        .force_result_partial_reasons_for_tests
        .lock() = reasons;
    let _clear = Clear(host);
    read()
}

#[test]
fn an_aborted_projection_hands_its_reader_the_abort_never_the_value() {
    let cases = [
        (PartialReasonSet::CANCELLED, QueryError::Cancelled),
        (
            PartialReasonSet::SUPERSEDED_GENERATION,
            QueryError::StaleSemanticOperand,
        ),
        (
            PartialReasonSet::UNSTABLE_STATE,
            QueryError::UnstableState { attempts: 1 },
        ),
    ];
    for (reasons, expected) in cases {
        let host = host_with_box();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let base = box_node(&dispatch);
        for mode in [ProjectionMode::Shallow, ProjectionMode::Expanded] {
            let key = obj_path(base, mode);
            let read = with_observed_reasons(&host, reasons, || dispatch.execute_read(key.clone()));
            match &read.value {
                QueryResult::Error(error) => assert_eq!(
                    error, &expected,
                    "{reasons:?} {mode:?}: the abort's typed error"
                ),
                other => panic!(
                    "{reasons:?} {mode:?}: an aborted build publishes no value, got {other:?}"
                ),
            }
            assert!(
                read.result_is_partial,
                "{reasons:?} {mode:?}: stays partial"
            );
            assert!(read.cache_suppress, "{reasons:?} {mode:?}: never warms");
            assert!(
                read.partial_reasons.contains(reasons),
                "{reasons:?} {mode:?}: the observed class survives"
            );
            assert_eq!(
                host.project_type_store()
                    .semantic_graph()
                    .slot_candidate_count_for_tests(&key),
                0,
                "{reasons:?} {mode:?}: nothing is admitted"
            );

            let clean = dispatch.execute_read(key.clone());
            assert!(
                matches!(clean.value, QueryResult::Value(_)) && !clean.result_is_partial,
                "{reasons:?} {mode:?}: a clean build is the exact value, got {:?}",
                clean.value
            );
        }
    }
}
