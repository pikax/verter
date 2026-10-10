use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    LiteralValue, NodeScopeId, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId,
};

fn literal(n: f64) -> SemanticNodeData {
    SemanticNodeData::Literal(LiteralValue::Number(n))
}

fn array(element: SemanticNodeId) -> SemanticNodeData {
    SemanticNodeData::Array {
        element,
        readonly: false,
    }
}

fn is_placeholder(store: &SemanticGraphStore, id: SemanticNodeId) -> bool {
    matches!(
        store.node_data(id).as_deref(),
        Some(SemanticNodeData::Opaque(QueryError::Miss))
    )
}

/// A computation's intermediates live exactly as long as its scope: the
/// scope's last guard releases them, the node and the child only its edge
/// kept are destroyed, and a later intern of the same payload mints a
/// fresh id instead of handing the dead one out again.
#[test]
fn a_scope_roots_its_interns_until_its_last_guard_drops() {
    let store = SemanticGraphStore::default();
    let baseline = store.node_count();
    let (child, parent) = {
        let scope = store.enter_root_scope();
        let child = store.intern_node(literal(7.0));
        let parent = store.intern_node(array(child));
        assert_eq!(scope.rooted_len(), 2);
        // The child is counted by its scope root and by the parent's edge.
        assert_eq!(store.node_refs_for_tests(child), Some(2));
        assert_eq!(store.node_refs_for_tests(parent), Some(1));
        (child, parent)
    };
    assert_eq!(store.node_count(), baseline, "both nodes were destroyed");
    assert_eq!(store.pending_destruction_for_tests(), 0);
    assert!(is_placeholder(&store, child) && is_placeholder(&store, parent));
    assert_eq!(store.lease(parent).unwrap_err(), LeaseError::Stale);

    let _scope = store.enter_root_scope();
    let again = store.intern_node(literal(7.0));
    assert_ne!(again, child, "a destroyed node is never handed out again");
    assert!(!is_placeholder(&store, again));
}

/// The twelve primitive vocabulary nodes are permanent: no root release
/// destroys them. A file-scoped primitive is an ordinary node.
#[test]
fn primitive_vocabulary_nodes_are_permanent() {
    let store = SemanticGraphStore::default();
    let (global, scoped) = {
        let _scope = store.enter_root_scope();
        let global = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let scoped = store.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::String),
            NodeScopeId::File {
                canonical_id: Arc::from("/w/a.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [7u8; 16],
                local_scope: None,
            },
        );
        (global, scoped)
    };
    assert_eq!(store.node_refs_for_tests(global), Some(u32::MAX - 1));
    assert!(store.node_is_live(global));
    assert!(!store.node_is_live(scoped));
}

/// A lease keeps its node and everything the node retains readable after
/// every other root is gone, and its release returns the store to its
/// baseline — in either order of two leases over a parent and its child.
#[test]
fn leases_release_to_baseline_in_both_orders() {
    for parent_first in [true, false] {
        let store = SemanticGraphStore::default();
        let baseline = store.node_count();
        let (child_lease, parent_lease) = {
            let _scope = store.enter_root_scope();
            let child = store.intern_node(literal(1.0));
            let parent = store.intern_node(array(child));
            (store.lease(child).unwrap(), store.lease(parent).unwrap())
        };
        assert_eq!(store.node_count(), baseline + 2);
        let parent = parent_lease.id();
        let child = child_lease.id();
        assert!(matches!(
            store.leased_node_data(&parent_lease).as_deref(),
            Ok(SemanticNodeData::Array { element, .. }) if *element == child
        ));
        if parent_first {
            drop(parent_lease);
            assert!(!store.node_is_live(parent));
            assert!(store.node_is_live(child), "the child lease still holds it");
            drop(child_lease);
        } else {
            drop(child_lease);
            assert!(
                store.node_is_live(child),
                "the parent's edge still holds the child"
            );
            drop(parent_lease);
        }
        assert_eq!(store.node_count(), baseline, "parent_first={parent_first}");
        assert!(!store.node_is_live(child) && !store.node_is_live(parent));
    }
}

/// A lease is bound to the store that minted it: reading it against
/// another store whose arena holds an overlapping id is refused.
#[test]
fn a_foreign_lease_is_refused() {
    let minting = SemanticGraphStore::default();
    let other = SemanticGraphStore::default();
    let _a = minting.enter_root_scope();
    let _b = other.enter_root_scope();
    let leased = minting.intern_node(literal(3.0));
    let overlapping = other.intern_node(literal(4.0));
    assert_eq!(
        leased, overlapping,
        "both arenas hand out the same first id"
    );
    let lease = minting.lease(leased).unwrap();
    assert_eq!(
        other.leased_node_data(&lease).unwrap_err(),
        LeaseError::Foreign
    );
    assert!(minting.leased_node_data(&lease).is_ok());
}

/// A payload naming a destroyed child is refused as a typed stale operand
/// instead of resurrecting the child or dangling on it; one naming an id
/// never handed out is a foreign operand.
#[test]
fn a_payload_over_a_dead_or_unallocated_child_is_refused() {
    let store = SemanticGraphStore::default();
    let dead = {
        let _scope = store.enter_root_scope();
        store.intern_node(literal(9.0))
    };
    assert!(!store.node_is_live(dead));
    let _scope = store.enter_root_scope();
    let over_dead = store.intern_node(array(dead));
    assert!(matches!(
        store.node_data(over_dead).as_deref(),
        Some(SemanticNodeData::Opaque(QueryError::StaleSemanticOperand))
    ));
    let over_unallocated = store.intern_node(array(SemanticNodeId(1 << 40)));
    assert!(matches!(
        store.node_data(over_unallocated).as_deref(),
        Some(SemanticNodeData::Opaque(QueryError::ForeignSemanticOperand))
    ));
}

/// Destruction is iterative: releasing the only root of a very deep chain
/// destroys the whole chain on a small stack.
#[test]
fn a_deep_chain_is_destroyed_without_recursion() {
    const DEPTH: usize = 50_000;
    let store = Arc::new(SemanticGraphStore::default());
    let worker = Arc::clone(&store);
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let baseline = worker.node_count();
            let lease = {
                let _scope = worker.enter_root_scope();
                let mut top = worker.intern_node(literal(0.0));
                for _ in 0..DEPTH {
                    top = worker.intern_node(array(top));
                }
                worker.lease(top).unwrap()
            };
            assert_eq!(worker.node_count(), baseline + DEPTH + 1);
            drop(lease);
            assert_eq!(worker.node_count(), baseline);
            assert_eq!(worker.pending_destruction_for_tests(), 0);
        })
        .unwrap()
        .join()
        .unwrap();
}

/// Work fanned out to another thread under the scope's handle is rooted by
/// the same scope: its interns survive the worker and are released only
/// with the scope's last guard.
#[test]
fn a_scope_handle_roots_work_on_another_thread() {
    let store = Arc::new(SemanticGraphStore::default());
    let baseline = store.node_count();
    let scope = store.enter_root_scope();
    let handle = scope.handle();
    let worker_store = Arc::clone(&store);
    let interned = std::thread::spawn(move || {
        let _entered = handle.enter();
        worker_store.intern_node(literal(5.0))
    })
    .join()
    .unwrap();
    assert!(
        store.node_is_live(interned),
        "the worker's guard was not the last"
    );
    // A nested entry joins the same scope instead of rooting separately.
    {
        let nested = store.enter_root_scope();
        assert_eq!(nested.rooted_len(), 1);
        assert_eq!(store.intern_node(literal(5.0)), interned);
    }
    assert!(store.node_is_live(interned));
    drop(scope);
    assert!(!store.node_is_live(interned));
    assert_eq!(store.node_count(), baseline);
}

/// The dedup index never hands out a dying node. One thread repeatedly
/// interns a payload under its own short scope while another holds the same
/// payload only through its own scope; every id either thread receives is
/// live while that thread's scope holds it, and both drain to baseline.
#[test]
fn concurrent_intern_and_final_release_never_serve_a_dying_node() {
    const ROUNDS: usize = 2_000;
    let store = Arc::new(SemanticGraphStore::default());
    let baseline = store.node_count();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                for round in 0..ROUNDS {
                    let _scope = store.enter_root_scope();
                    let child = store.intern_node(literal(42.0));
                    let parent = store.intern_node(array(child));
                    assert!(
                        store.node_is_live(child) && store.node_is_live(parent),
                        "round {round} was handed a dying node"
                    );
                    assert!(matches!(
                        store.node_data(parent).as_deref(),
                        Some(SemanticNodeData::Array { element, .. }) if *element == child
                    ));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(store.node_count(), baseline);
    assert_eq!(store.pending_destruction_for_tests(), 0);
}

/// A root set counts every occurrence it names, so a holder naming one
/// node twice keeps it until the holder drops, and a clone is a second
/// owner.
#[test]
fn a_root_set_counts_its_multiset() {
    let store = SemanticGraphStore::default();
    let node = {
        let _scope = store.enter_root_scope();
        store.intern_node(literal(11.0))
    };
    assert!(!store.node_is_live(node));
    let _scope = store.enter_root_scope();
    let node = store.intern_node(literal(11.0));
    let set = store.root_set([node, node]).unwrap();
    assert_eq!(store.node_refs_for_tests(node), Some(3));
    let copy = set.clone();
    drop(set);
    assert_eq!(store.node_refs_for_tests(node), Some(3));
    drop(copy);
    assert_eq!(store.node_refs_for_tests(node), Some(1));
    assert_eq!(
        store.root_set([node, SemanticNodeId(1 << 40)]).unwrap_err(),
        LeaseError::Unallocated
    );
    assert_eq!(
        store.node_refs_for_tests(node),
        Some(1),
        "a refused set releases what it counted"
    );
}
