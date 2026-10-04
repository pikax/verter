//! Carrier-arg descent for slot-binding dependency tracing and the
//! free-type-param classifier.
//!
//! `accumulate_lowered_node_carrier_deps` records cross-file declaration
//! deps from a lowered macro-arg node so a content edit to a contributing
//! file invalidates the cached binding; `node_contains_free_type_param`
//! classifies a Conditional CHECK as open when it still carries a free
//! `TypeParam`. A `BareRef` / `TypeOf` / `ImportType` carrier applies its
//! `type_args` at the reference site; those args can carry a cross-file
//! `DeclRef` / `InstantiationRef` (a dep edge) or a free `TypeParam` (an
//! open variable). Both walkers MUST descend
//! `SemanticNodeData::carrier_type_args` — args-only, no head resolution.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, NodeScopeId, ScopeId, SemanticNodeData, SemanticNodeId, ValueRootKey,
};
use crate::types::HostConfig;
use crate::VerterHost;

fn carrier_wrapping(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    arg: SemanticNodeId,
    kind: u8,
) -> SemanticNodeId {
    let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
    match kind {
        0 => graph.intern_node(SemanticNodeData::new_bare_ref(
            Arc::from("Foo"),
            NodeScopeId::Global,
            args,
        )),
        1 => graph.intern_node(SemanticNodeData::new_typeof(
            ValueRootKey {
                scope: ScopeId {
                    canonical_id: Arc::from("/v.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    local_scope: None,
                    binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                        verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    ),
                },
                name: Arc::from("factory"),
            },
            Arc::from(Vec::new().into_boxed_slice()),
            args,
        )),
        _ => graph.intern_node(SemanticNodeData::new_import_type(
            Arc::from("./m"),
            Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
            args,
            false,
        )),
    }
}

// ── accumulate_lowered_node_carrier_deps descends carrier args ──────────
//
// A cross-file `DeclRef` inside a carrier's `type_args` is a dep edge to its
// declaring file. `accumulate_lowered_node_carrier_deps` must descend the
// carrier and record that file's whole-hash fact, so a content edit to the
// dep invalidates the cached binding. NEGATIVE: with the unchanged `_ => {}`
// arm the carrier is a leaf and the cross-file canonical is never recorded.
#[test]
fn accumulate_carrier_deps_descends_carrier_args() {
    for kind in 0u8..3 {
        let host = VerterHost::new_standalone(HostConfig::default());
        let ctx: &dyn crate::resolver_core::ResolverContext = &host;
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        // A cross-file DeclRef (declared in /dep.ts) wrapped in a carrier
        // whose owner is /owner.vue.
        let dep_id = DeclIdentity::from_scope(
            &NodeScopeId::File {
                canonical_id: Arc::from("/dep.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [9u8; 16],
                local_scope: None,
            },
            Arc::from("Inner"),
        );
        let decl_ref = graph.intern_node(SemanticNodeData::DeclRef { identity: dep_id });
        let carrier = carrier_wrapping(&graph, decl_ref, kind);

        let ((), finalise) = crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::unbound(&host),
            || {
                let fixture_dispatch_0 =
                    crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
                super::accumulate_lowered_node_carrier_deps(
                    &fixture_dispatch_0,
                    carrier,
                    "/owner.vue",
                );
            },
        );
        let facts = match finalise {
            crate::resolver_core::FactReadSetFinalise::Ok(facts) => facts,
            other => panic!("carrier dependency tracing must be cacheable: {other:?}"),
        };

        let saw_dep = facts.iter().any(|f| {
            matches!(
                f,
                crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                    if canonical_id == "/dep.ts"
            )
        });
        assert!(
            saw_dep,
            "a cross-file DeclRef inside a carrier's type_args (kind {kind}) must be \
             accumulated as a /dep.ts whole-hash fact; got {facts:?}"
        );
    }
}

// ── node_contains_free_type_param descends carrier args ─────────────────
//
// A free `TypeParam` inside a carrier's `type_args` means the node DOES
// contain a free param (the conditional check is open). NEGATIVE: with the
// unchanged `_ => false` arm a carrier is treated as NOT-free.
#[test]
fn node_contains_free_type_param_descends_carrier_args() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let free_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("X"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("X"),
    });

    for kind in 0u8..3 {
        let carrier = carrier_wrapping(&graph, free_param, kind);
        assert!(
            super::node_contains_free_type_param(&dispatch, carrier),
            "a free TypeParam inside a carrier's type_args (kind {kind}) must make the node \
             contain a free param; carrier {:?}",
            graph.node_data(carrier).as_deref()
        );
    }

    // NEGATIVE control: a carrier whose only arg is a concrete primitive is
    // NOT free (proving the descent reads the actual arg, not a blanket
    // true for any carrier).
    let concrete = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    for kind in 0u8..3 {
        let carrier = carrier_wrapping(&graph, concrete, kind);
        assert!(
            !super::node_contains_free_type_param(&dispatch, carrier),
            "a carrier whose only arg is a concrete primitive (kind {kind}) must NOT be free"
        );
    }
}

// ── the scans read any nesting ───────────────────────────────────────────

/// A nesting past any native-stack or depth bound.
const DEPTH: usize = 10_000;

/// `leaf` under `DEPTH` array types, read on a 1 MiB thread by `probe`.
fn under_nested_arrays<R: Send + 'static>(
    leaf: impl FnOnce(&crate::semantic_query_memo::SemanticGraphStore) -> SemanticNodeId
        + Send
        + 'static,
    probe: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R + Send + 'static,
) -> R {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let host = VerterHost::new_standalone(HostConfig::default());
            let dispatch = ProjectSemanticDispatch::new(&host);
            let graph = Arc::clone(host.project_type_store().semantic_graph());
            let mut node = leaf(&graph);
            for _ in 0..DEPTH {
                node = graph.intern_node(SemanticNodeData::Array {
                    element: node,
                    readonly: false,
                });
            }
            probe(&dispatch, node)
        })
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

fn free_param(graph: &crate::semantic_query_memo::SemanticGraphStore) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("X"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("X"),
    })
}

/// A free type parameter under 10,000 array types keeps the check open.
#[test]
fn a_free_param_is_found_at_any_depth() {
    assert!(under_nested_arrays(free_param, |dispatch, node| {
        super::node_contains_free_type_param(dispatch, node)
    }));
}

/// A reference into another file under 10,000 array types is a non-owner
/// reference.
#[test]
fn a_non_owner_reference_is_found_at_any_depth() {
    let foreign = |graph: &crate::semantic_query_memo::SemanticGraphStore| {
        graph.intern_node(SemanticNodeData::DeclRef {
            identity: DeclIdentity {
                canonical_id: Arc::from("/elsewhere.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: Arc::from("Elsewhere"),
            },
        })
    };
    assert!(under_nested_arrays(foreign, |dispatch, node| {
        super::node_reaches_non_owner_ref(dispatch, "/owner.ts", node)
    }));
}

/// A slot parameter root naming a free type parameter through 10,000
/// aliases is symbolic.
#[test]
fn a_symbolic_slot_root_is_found_through_any_alias_chain() {
    let symbolic = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let host = VerterHost::new_standalone(HostConfig::default());
            let dispatch = ProjectSemanticDispatch::new(&host);
            let graph = Arc::clone(host.project_type_store().semantic_graph());
            let mut node = free_param(&graph);
            for _ in 0..DEPTH {
                node = graph.intern_node(SemanticNodeData::Alias(node));
            }
            super::slot_param_root_is_symbolic_only(&dispatch, node)
        })
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers");
    assert!(symbolic);
}
