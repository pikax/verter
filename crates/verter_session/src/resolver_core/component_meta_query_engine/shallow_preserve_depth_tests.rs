//! The shallow-preservation scans read a field value's type at any
//! nesting: a hit under 10,000 array types is found on a 1 MiB thread, with
//! no depth ceiling answering `false` for the rest of the structure.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::super::ComponentMetaQueryEngine;
use crate::{HostConfig, VerterHost};
use verter_type_engine::semantic_query::{DeclIdentity, SemanticNodeData, SemanticNodeId};

/// A nesting past any native-stack or depth bound.
const DEPTH: usize = 10_000;

const SCOPE: &str = "/src/App.vue";

/// `leaf` under `DEPTH` array types, read on a 1 MiB thread by `probe`.
fn under_nested_arrays<R: Send + 'static>(
    leaf: impl FnOnce(&verter_type_engine::semantic_query_memo::SemanticGraphStore) -> SemanticNodeId
        + Send
        + 'static,
    probe: impl FnOnce(&mut ComponentMetaQueryEngine<'_>, SemanticNodeId) -> R + Send + 'static,
) -> R {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let host = VerterHost::new_standalone(HostConfig::default());
            let graph = Arc::clone(host.project_type_store().semantic_graph());
            let mut node = leaf(&graph);
            for _ in 0..DEPTH {
                node = graph.intern_node(SemanticNodeData::Array {
                    element: node,
                    readonly: false,
                });
            }

            let fixture_dispatch_0 =
                verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
            let mut engine = ComponentMetaQueryEngine::new(&host, &fixture_dispatch_0);
            probe(&mut engine, node)
        })
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

/// A declaration of another file named `name`.
fn foreign(name: &str) -> DeclIdentity {
    DeclIdentity {
        canonical_id: Arc::from("/src/elsewhere.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: verter_type_engine::semantic_query::HashValue::default(),
        decl_name: Arc::from(name),
    }
}

/// A parent type parameter named under 10,000 array types is referenced.
#[test]
fn a_type_param_reference_is_found_at_any_depth() {
    let referenced = under_nested_arrays(
        |graph| {
            graph.intern_node(SemanticNodeData::TypeParam {
                decl: DeclIdentity::synthetic("T"),
                param_index: 0,
                constraint: None,
                default: None,
                display_name: Arc::from("T"),
            })
        },
        |engine, node| {
            let names: FxHashSet<&str> = FxHashSet::from_iter(["T"]);
            super::node_references_type_param_names(engine.dispatch, node, &names)
        },
    );
    assert!(referenced);
}

/// A builtin utility applied to another file's declaration under 10,000
/// array types is an imported utility route.
#[test]
fn an_imported_utility_route_is_found_at_any_depth() {
    let found = under_nested_arrays(
        |graph| {
            let argument = graph.intern_node(SemanticNodeData::DeclRef {
                identity: foreign("Props"),
            });
            graph.intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: Arc::from("__builtin__"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: verter_type_engine::semantic_query::HashValue::default(),
                    decl_name: Arc::from("Partial"),
                },
                args: Arc::from([argument]),
            })
        },
        |engine, node| {
            engine.node_contains_imported_utility_route(
                SCOPE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                node,
            )
        },
    );
    assert!(found);
}

/// Another file's generic applied under 10,000 array types is an imported
/// generic route.
#[test]
fn an_imported_generic_route_is_found_at_any_depth() {
    let found = under_nested_arrays(
        |graph| {
            let argument = graph.intern_node(SemanticNodeData::Primitive(
                verter_type_engine::semantic_query::PrimitiveKind::String,
            ));
            graph.intern_node(SemanticNodeData::InstantiationRef {
                base: foreign("Box"),
                args: Arc::from([argument]),
            })
        },
        |engine, node| {
            engine.node_has_imported_generic_route(
                SCOPE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                node,
            )
        },
    );
    assert!(found);
}
