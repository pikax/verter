use std::sync::Arc;

use super::carrier_head_resolution_tests::{host, upsert_ts};
use super::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{
    DeclIdentity, HashValue, IndexKey, IndexedAccessOrigin, PayloadArray, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

const SOURCE: &str = "export interface Face { value: string }\n\
                     export interface Box<T> { value: T }\n\
                     export type A = 1 | 2;\n\
                     export type B = 2 | 3;\n\
                     export type Scalar = number;\n";

fn identity(name: &str, whole_hash: HashValue) -> DeclIdentity {
    DeclIdentity {
        canonical_id: "/facts.ts".into(),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash,
        decl_name: name.into(),
    }
}

fn subject<C: verter_type_engine::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    case: &str,
) -> SemanticNodeId {
    let graph = dispatch.graph();
    let whole_hash = dispatch
        .ctx
        .shallow_file_state("/facts.ts")
        .expect("the fixture is indexed")
        .whole_hash;
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let named = |name| {
        graph.intern_node(SemanticNodeData::DeclRef {
            identity: identity(name, whole_hash),
        })
    };
    match case {
        "declaration" => named("Face"),
        "application" => graph.intern_node(SemanticNodeData::InstantiationRef {
            base: identity("Box", whole_hash),
            args: PayloadArray::from(Arc::<[_]>::from([string])),
        }),
        "deferred" => graph.intern_node(SemanticNodeData::IndexedAccess {
            object: named("Face"),
            index: IndexKey::String("value".into()),
            origin: IndexedAccessOrigin::Composed,
        }),
        "composite" => graph.intern_node(SemanticNodeData::Union(
            verter_type_engine::semantic_query::composite::CompositeList::authored_shell_for_tests(
                Arc::from([named("A"), named("B")]),
            ),
        )),
        "intersection" => graph.intern_node(SemanticNodeData::Intersection(
            verter_type_engine::semantic_query::composite::CompositeList::authored_shell_for_tests(
                Arc::from([named("Scalar"), string]),
            ),
        )),
        "missing" => named("Missing"),
        "terminal" => string,
        _ => unreachable!(),
    }
}

#[test]
fn structural_fact_reads_preserve_outcomes_order_and_observations() {
    let context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    for (case, expected, composite_only) in [
        ("declaration", "ResolveDecl", false),
        ("application", "Instantiate", false),
        ("deferred", "IndexedAccess", false),
        ("composite", "ResolveDecl", true),
        ("intersection", "ReduceIntersection", true),
        ("missing", "ResolveDecl", false),
        ("terminal", "none", false),
        ("terminal", "none", true),
    ] {
        let host = host();
        upsert_ts(&host, "/facts.ts", SOURCE);
        let fixture = ProjectSemanticDispatch::new(&host);
        let _construction = fixture.enter_construction_scope();
        let node = subject(&fixture, case);
        let run = |stepped| {
            let dispatch = ProjectSemanticDispatch::new(&host);
            let (trace, reads) = host.with_fact_tracer(
                verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
                || {
                    dispatch.trace_structural_fact_read_for_tests(
                        node,
                        context,
                        stepped,
                        composite_only,
                    )
                },
            );
            let result = trace
                .outcome
                .into_complete_node()
                .expect("the fixture must settle completely");
            if case == "terminal" || case == "deferred" {
                assert!(
                    matches!(
                        dispatch.graph().node_data(result).as_deref(),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                    ),
                    "{case}"
                );
            }
            if case == "intersection" {
                assert!(matches!(
                    dispatch.graph().node_data(result).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Never))
                ));
            }
            if case == "missing" {
                assert_eq!(result, node, "a declined lookup keeps its authored carrier");
            }
            use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
            let observations = match reads.finalise() {
                FactReadSetFinalise::Ok(facts) => (true, facts),
                FactReadSetFinalise::NonCacheable(facts) => (false, facts),
                FactReadSetFinalise::MutationUnstable => {
                    panic!("the fixed fixture cannot have a torn observation")
                }
            };
            (trace, observations)
        };
        let (stepped, observations) = run(true);
        // Keep the same pinned operands and graph identities while giving
        // the in-place run an independently cold memo and request state.
        fixture.graph().invalidate_all();
        let inline = run(false);
        assert_eq!((&stepped, observations), (&inline.0, inline.1), "{case}");
        let keys: Vec<_> = stepped
            .requested
            .iter()
            .map(|key| match key {
                SemanticQueryKey::ResolveDecl(_) => "ResolveDecl",
                SemanticQueryKey::Instantiate(_) => "Instantiate",
                SemanticQueryKey::IndexedAccess { .. } => "IndexedAccess",
                SemanticQueryKey::ReduceIntersection { .. } => "ReduceIntersection",
                other => panic!("unexpected structural read: {other:?}"),
            })
            .collect();
        if expected == "none" {
            assert!(keys.is_empty());
        } else {
            assert!(keys.contains(&expected), "{case}: {keys:?}");
        }
        assert!(stepped.observations_complete, "{case}");
        assert!(!stepped.cache_suppress, "{case}");
    }
}

#[test]
fn structural_fact_read_abandon_releases_its_recordings_and_observations() {
    for (case, at_reduction) in [
        ("declaration", false),
        ("application", false),
        ("deferred", false),
        ("composite", false),
        ("intersection", true),
        ("missing", false),
    ] {
        let host = host();
        upsert_ts(&host, "/facts.ts", SOURCE);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let _construction = dispatch.enter_construction_scope();
        let node = subject(&dispatch, case);
        dispatch.assert_structural_fact_read_abandon_for_tests(
            node,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            at_reduction,
        );
    }
}

#[test]
fn structural_fact_read_consumes_a_declined_delivery_inside_its_probe() {
    let host = host();
    upsert_ts(&host, "/facts.ts", SOURCE);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    let node = subject(&dispatch, "missing");
    dispatch.assert_structural_fact_probe_delivery_for_tests(node);
}
