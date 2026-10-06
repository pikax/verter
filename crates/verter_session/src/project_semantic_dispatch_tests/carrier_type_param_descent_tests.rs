//! Carrier-arg descent for the type-parameter binder collector.
//!
//! `collect_type_param_nodes_by_name` walks a signature's structural
//! children to find every `TypeParam` binder node named `name` that the
//! substitute engine would rewrite. A `BareRef` / `TypeOf` / `ImportType`
//! carrier applies its `type_args` at the reference site; a binder occurrence
//! inside those args is a position the substitute engine reaches, so the
//! collector must descend `SemanticNodeData::carrier_type_args` (args-only,
//! no head resolution) — otherwise a `<T>`-bearing carrier arg leaves `T`
//! unspecialised at instantiation time.

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::DeclIdentity;
use crate::semantic_query::NodeScopeId;
use crate::semantic_query::ScopeId;
use crate::semantic_query::SemanticNodeData;
use crate::semantic_query::SemanticNodeId;
use crate::semantic_query::ValueRootKey;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

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

// ── D4 — collect_type_param_nodes_by_name descends carrier args ─────────
//
// A `TypeParam` named `X` inside a carrier's `type_args` is a binder
// occurrence the substitute engine reaches; the collector must return it.
// NEGATIVE: with the unchanged `_ => {}` arm the carrier is a leaf and the
// binder node is missed (the returned set would not contain it).
#[test]
fn collect_type_param_nodes_descends_carrier_args() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = Arc::clone(host.project_type_store().semantic_graph());

    let binder = graph.intern_node(SemanticNodeData::TypeParam {
        decl: DeclIdentity::synthetic("X"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("X"),
    });

    for kind in 0u8..3 {
        let carrier = carrier_wrapping(&graph, binder, kind);
        let found = dispatch.collect_type_param_nodes_by_name(
            carrier,
            "X",
            false,
            crate::semantic_query::ClauseSpelling::Bound,
        );
        assert!(
            found.contains(&binder),
            "a `TypeParam` named X inside a carrier's type_args (kind {kind}) must be \
             collected; got {found:?} for carrier {:?}",
            graph.node_data(carrier).as_deref()
        );
    }

    // NEGATIVE control: searching for a DIFFERENT name does NOT collect the
    // X binder (proving the descent honours the name, not a blanket collect).
    for kind in 0u8..3 {
        let carrier = carrier_wrapping(&graph, binder, kind);
        let found = dispatch.collect_type_param_nodes_by_name(
            carrier,
            "Y",
            false,
            crate::semantic_query::ClauseSpelling::Bound,
        );
        assert!(
            !found.contains(&binder),
            "searching name Y must NOT collect the X binder reached through a carrier arg \
             (kind {kind})"
        );
    }
}
