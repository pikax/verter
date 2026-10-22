//! What rooting a graph node costs the allocator. A root set is its arena
//! and the ids it counts, kept inline in whatever holds it, with each count
//! in the arena's slot table: acquiring, cloning and releasing a root
//! allocates nothing, so the bookkeeping per rooted node is zero
//! allocations however many nodes a request roots.

use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData, SemanticNodeId};
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

use super::{alloc_count, reset_alloc_counter};

/// Root, clone and release one root per node of a graph holding `nodes`
/// nodes, and report the allocations the measured window made.
fn rooting_allocations(nodes: usize) -> u64 {
    let graph = SemanticGraphStore::new();
    let scope = graph.enter_node_root_scope();
    let ids: Vec<SemanticNodeId> = (0..nodes)
        .map(|index| {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
                index as f64 + 0.5,
            )))
        })
        .collect();
    let held = graph.root_nodes(ids.iter().copied()).expect("live nodes");
    drop(scope);
    let mut kept = Vec::with_capacity(nodes);
    reset_alloc_counter();
    for &id in &ids {
        let root = held
            .node_ref(id)
            .expect("an operand of the held set")
            .to_root()
            .expect("a held node roots");
        let copy = root.clone();
        drop(root);
        kept.push(copy);
    }
    let allocations = alloc_count();
    for (root, id) in kept.iter().zip(&ids) {
        assert_eq!(root.id(), *id);
        assert!(root.node_ref().expect("rooted").read().is_ok());
    }
    allocations
}

/// Acquiring a root on a node, cloning it and dropping the original
/// allocate nothing, for every node, at every graph size.
#[test]
fn rooting_a_node_allocates_nothing_at_any_graph_size() {
    for nodes in [64usize, 512, 4_096] {
        let allocations = rooting_allocations(nodes);
        assert_eq!(
            allocations, 0,
            "rooting {nodes} nodes allocated {allocations} times"
        );
    }
}
