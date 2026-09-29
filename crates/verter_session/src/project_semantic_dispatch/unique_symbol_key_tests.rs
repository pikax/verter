//! `unique symbol` property keys as key types: `keyof` over a surface with
//! a `unique symbol` key holds the symbol's nominal type beside the
//! property names, and a mapped type binds its key to that type.

use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host, PROBE_FILE};
use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

/// The key type `keyof` gives a `unique symbol` property key is a node of
/// the declaring file's (the nominal `typeof` carrier of the symbol's
/// declaration, one per declaration): the key union retains it as a child
/// (`for_each_retained_child`), and closing the declaring document
/// releases both with the rest of the file's nodes.
///
/// Oracle: over `declare const sym: unique symbol; interface HasSym {
/// [sym]: number; x: 1 }`, tsc 7.0.2 holds `keyof HasSym` mutually
/// assignable to `"x" | typeof sym` in all four settings; the probe reads
/// the keys through `Keys<T> = T extends string | symbol ? T : never`,
/// which distributes over them.
#[test]
fn a_unique_symbol_key_type_is_retained_by_its_key_union_and_released_on_close() {
    let host = default_probe_host();
    let source = "declare const sym: unique symbol;\ninterface HasSym { [sym]: number; x: 1 }\n\
                  type Keys<T> = T extends string | symbol ? T : never;\n";
    let (keys, key_type) = with_probe_on_host(
        &host,
        Default::default(),
        source,
        "Keys<keyof HasSym>",
        |dispatch, keys| {
            let graph = dispatch.graph();
            let members: Vec<SemanticNodeId> = match graph.node_data(keys).as_deref() {
                Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
                other => panic!("two keys make a union, got {other:?}"),
            };
            let key_type = members
                .iter()
                .copied()
                .find(|member| {
                    matches!(
                        graph.node_data(*member).as_deref(),
                        Some(SemanticNodeData::TypeOfNominal(_))
                    )
                })
                .expect("the symbol key is its nominal type");
            let mut retained = Vec::new();
            graph
                .node_data(keys)
                .expect("the key union")
                .for_each_retained_child(|child| retained.push(child));
            assert!(
                retained.contains(&key_type),
                "the key union retains the symbol's key type: {retained:?}"
            );
            (keys, key_type)
        },
    );
    let graph = std::sync::Arc::clone(host.project_type_store().semantic_graph());
    assert!(graph.node_is_live(keys) && graph.node_is_live(key_type));
    host.evict(PROBE_FILE);
    let store = host.project_type_store();
    store.try_apply_deferred_releases();
    assert_eq!(
        store.deferred_release_count(),
        0,
        "the close release applied"
    );
    assert!(
        !graph.node_is_live(key_type) && !graph.node_is_live(keys),
        "closing {PROBE_FILE} releases the symbol's key type and the union holding it"
    );
}
