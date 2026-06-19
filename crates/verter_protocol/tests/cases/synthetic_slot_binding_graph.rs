//! S1 discrimination tests for the typed-IR
//! `TypeExpr::SyntheticSlotBinding` variant traversing the graph
//! builder + proto encoding pipeline.

use std::sync::Arc;

use verter_protocol::graph::{GraphBuilder, GraphNode};
use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr};

fn make_carrier() -> TypeExpr {
    TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/abs/Foo.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("controls"),
        value_node: 42,
    })
}

/// Feed a `TypeExpr::SyntheticSlotBinding` through `GraphBuilder::node_id`
/// and assert the published `GraphNode` matches the proto-shape contract:
/// `value_node` survives verbatim (no precision loss), `slot_name_id` is
/// non-zero when slot_name is Some(_), and the surface_kind matches the
/// TS `SYNTHETIC_CARRIER_SURFACE_SLOT_BINDING` (= 0) constant.
#[test]
fn graph_builder_synthetic_carrier_roundtrip() {
    let carrier = make_carrier();

    let mut builder = GraphBuilder::new();
    let id = builder.node_id(&carrier);

    // node_id is 1-based; nodes()[0] must be our carrier.
    assert_eq!(id, 1, "first node_id minted must be 1");

    let node = builder
        .nodes()
        .first()
        .expect("at least one node must be present");

    match node {
        GraphNode::SyntheticSlotBinding {
            value_node,
            scope_canonical_id_id,
            surface_kind,
            slot_name_id,
            binding_name_id,
        } => {
            assert_eq!(*value_node, 42, "value_node must round-trip verbatim");
            assert_eq!(
                *surface_kind, 0,
                "SyntheticCarrierSurfaceKind::SlotBinding must encode as 0"
            );

            // String-table entries are 1-based; 0 means absent. All three
            // string fields are present in this fixture, so all three ids
            // must be > 0.
            assert!(
                *scope_canonical_id_id > 0,
                "scope_canonical_id_id must be a valid string-table entry"
            );
            assert!(
                *slot_name_id > 0,
                "slot_name_id must be a valid string-table entry when slot_name is Some(_)"
            );
            assert!(
                *binding_name_id > 0,
                "binding_name_id must be a valid string-table entry"
            );

            // The string table itself must hold the original values.
            let strings = builder.strings();
            assert_eq!(
                strings
                    .get(usize::try_from(*scope_canonical_id_id - 1).unwrap())
                    .map(|s| s.as_str()),
                Some("/abs/Foo.vue")
            );
            assert_eq!(
                strings
                    .get(usize::try_from(*slot_name_id - 1).unwrap())
                    .map(|s| s.as_str()),
                Some("default")
            );
            assert_eq!(
                strings
                    .get(usize::try_from(*binding_name_id - 1).unwrap())
                    .map(|s| s.as_str()),
                Some("controls")
            );
        }
        other => panic!("expected GraphNode::SyntheticSlotBinding, got {other:?}"),
    }
}

/// `slot_name_id == 0` is the wire encoding for "slot_name absent". Build a
/// carrier whose slot_name is None and assert the encoded node uses 0 for
/// that slot.
#[test]
fn graph_builder_synthetic_carrier_absent_slot_name_encodes_as_zero() {
    let carrier = TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/abs/Foo.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::Binding,
        slot_name: None,
        binding_name: Arc::from("controls"),
        value_node: 7,
    });

    let mut builder = GraphBuilder::new();
    let _id = builder.node_id(&carrier);

    match builder.nodes().first().expect("node present") {
        GraphNode::SyntheticSlotBinding {
            surface_kind,
            slot_name_id,
            ..
        } => {
            assert_eq!(
                *surface_kind, 1,
                "SyntheticCarrierSurfaceKind::Binding must encode as 1"
            );
            assert_eq!(
                *slot_name_id, 0,
                "absent slot_name MUST encode as slot_name_id = 0"
            );
        }
        other => panic!("expected SyntheticSlotBinding, got {other:?}"),
    }
}

/// Two structurally identical carriers (even when held in distinct
/// `Arc<SyntheticCarrierKey>` values) MUST dedupe to the same graph
/// node id via the `ExprMemoKey` slow path.
#[test]
fn graph_builder_synthetic_carrier_deduplicates_structurally() {
    let a = make_carrier();
    let b = make_carrier(); // physically distinct Arc, structurally identical

    let mut builder = GraphBuilder::new();
    let id_a = builder.node_id(&a);
    let id_b = builder.node_id(&b);
    assert_eq!(
        id_a, id_b,
        "structurally identical carriers MUST share a node id"
    );
    assert_eq!(
        builder.nodes().len(),
        1,
        "structurally identical carriers MUST publish only one node"
    );
}
