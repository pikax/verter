//! T5-3 (protocol half) — the `Unknown` graph-snapshot wire shape is
//! UNCHANGED by the opaque `UnknownValue` payload: the builder interns the
//! raw text into the string table verbatim and emits the same
//! `GraphNode::Unknown { raw }` shape (UnknownNode raw_id semantics intact).

use verter_protocol::graph::{GraphBuilder, GraphNode};
use verter_type_expr::{TypeExpr, UnknownValue};

#[test]
fn graph_builder_unknown_roundtrips_raw_verbatim() {
    for (raw, value) in [
        (
            "Custom & Raw",
            UnknownValue::unsupported_syntax("Custom & Raw"),
        ),
        (
            "semanticMiss",
            UnknownValue::compatibility_projection("semanticMiss"),
        ),
    ] {
        let mut builder = GraphBuilder::new();
        let id = builder.node_id(&TypeExpr::Unknown(value));
        assert_eq!(id, 1, "first node_id minted must be 1");
        match builder.nodes().first().expect("one node") {
            GraphNode::Unknown { raw: raw_id } => {
                let resolved = &builder.strings()[(*raw_id - 1) as usize];
                assert_eq!(resolved, raw, "the string table carries the raw verbatim");
            }
            other => panic!("expected GraphNode::Unknown, got {other:?}"),
        }
    }
}
