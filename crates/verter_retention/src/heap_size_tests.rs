use std::sync::Arc;

use super::{footprint, hash_table_bytes, owned_tree, HeapSize};
use crate::allocation::{arc_block_bytes, heap_block_bytes};
use crate::owned::{ChargeCtx, OwnedStr};
use crate::{ChargeClass, RetentionLimits, SemanticRetentionAccount};

#[derive(HeapSize)]
struct Leaf {
    text: String,
    shared: Arc<str>,
    #[heap_size(inline)]
    ordinal: u32,
}

#[derive(HeapSize)]
enum Node {
    Leaf(Leaf),
    Branch {
        children: Vec<Arc<Node>>,
        name: OwnedStr,
    },
    Empty,
}

fn account() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits::defaults())
}

#[test]
fn a_tree_counts_capacity_each_raw_allocation_once_and_no_owned_handle() {
    let shared: Arc<str> = Arc::from("s".repeat(5_000));
    let mut text = String::with_capacity(10_000);
    text.push('t');
    let leaf = Arc::new(Node::Leaf(Leaf {
        text,
        shared: Arc::clone(&shared),
        ordinal: 1,
    }));
    let name = OwnedStr::from("n".repeat(7_000));
    let tree = Node::Branch {
        // The same leaf twice: one allocation, counted once.
        children: vec![Arc::clone(&leaf), Arc::clone(&leaf), Arc::new(Node::Empty)],
        name: name.clone(),
    };
    let measured = footprint(&tree);
    let expected = heap_block_bytes(3 * std::mem::size_of::<Arc<Node>>())
        + 2 * arc_block_bytes(std::mem::size_of::<Node>())
        + heap_block_bytes(10_000)
        + arc_block_bytes(5_000);
    assert_eq!(measured.bytes, expected);
    assert_eq!(
        measured.handles.len(),
        1,
        "the owned name is offered, not counted"
    );
    measured.handles.pin(ChargeCtx::new(&account()));
}

#[test]
fn a_hash_map_costs_its_table_by_capacity() {
    let mut map: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    assert_eq!(footprint(&map).bytes, 0);
    map.insert(1, "x".repeat(100));
    let table = hash_table_bytes(map.capacity(), std::mem::size_of::<(u32, String)>());
    assert!(table > 0);
    assert_eq!(footprint(&map).bytes, table + heap_block_bytes(100));
}

/// A tree held in an owned cell is charged once, whole, by the cell: every
/// clone of the handle shares that charge, it moves to the callers when
/// the cache lets go, and the last holder's drop releases it.
#[test]
fn an_owned_tree_charges_its_whole_tree_once_until_its_last_holder_drops() {
    let account = account();
    let ctx = ChargeCtx::new(&account);
    let shared: Arc<str> = Arc::from("s".repeat(5_000));
    let tree = owned_tree(Node::Leaf(Leaf {
        text: "t".repeat(9_000),
        shared,
        ordinal: 0,
    }));
    let hold = tree.hold(ctx).expect("admitted");
    let charged = tree.charge_bytes().expect("charged by the hold");
    assert!(charged >= 14_000, "the whole tree is charged: {charged}");
    assert_eq!(account.snapshot().retained_bytes, charged);
    let caller = tree.clone();
    drop(tree);
    drop(hold);
    assert_eq!(caller.charge_class(), Some(ChargeClass::Pinned));
    assert_eq!(account.snapshot().pinned_bytes, charged);
    drop(caller);
    assert_eq!(account.snapshot().total_bytes(), 0);
}
