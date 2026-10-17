use std::sync::Arc;

use verter_session_query::retention::allocation::{arc_block_bytes, heap_block_bytes};

use super::{payload_heap_bytes, PayloadHeap, PayloadHeapVisitor};
use crate::semantic_query::{
    LiteralValue, PayloadArray, SemanticNodeData, SemanticNodeId, TupleElement,
};

/// Records every shared allocation offered and descends only the first
/// time a key is seen.
#[derive(Default)]
struct RecordingVisitor {
    owned: usize,
    shared: Vec<usize>,
    seen: Vec<usize>,
}

impl PayloadHeapVisitor for RecordingVisitor {
    fn owned(&mut self, bytes: usize) {
        self.owned += bytes;
    }

    fn shared(&mut self, key: usize, _bytes: usize) -> bool {
        self.shared.push(key);
        if self.seen.contains(&key) {
            return false;
        }
        self.seen.push(key);
        true
    }
}

fn arc_key<T: ?Sized>(arc: &Arc<T>) -> usize {
    Arc::as_ptr(arc).cast::<()>() as usize
}

#[test]
fn node_payload_footprint_string_literal_charges_its_capacity() {
    let mut long_text = String::with_capacity(50_000);
    long_text.push('x');
    let long = SemanticNodeData::Literal(LiteralValue::String(long_text));
    let short = SemanticNodeData::Literal(LiteralValue::String("x".to_owned()));
    let capacity = |data: &SemanticNodeData| match data {
        SemanticNodeData::Literal(LiteralValue::String(text)) => text.capacity(),
        _ => unreachable!("fixture is a string literal"),
    };
    let (long_capacity, short_capacity) = (capacity(&long), capacity(&short));
    assert!(long_capacity >= 50_000);

    // Capacity, not length: both literals hold one byte of text.
    assert_eq!(payload_heap_bytes(&long), heap_block_bytes(long_capacity));
    assert_eq!(payload_heap_bytes(&short), heap_block_bytes(short_capacity));
    assert!(
        payload_heap_bytes(&long) - payload_heap_bytes(&short)
            >= long_capacity - heap_block_bytes(short_capacity)
    );
}

#[test]
fn node_payload_footprint_payload_array_charges_its_whole_block() {
    let ids: Vec<SemanticNodeId> = (0..10_000).map(SemanticNodeId).collect();
    let expected = arc_block_bytes(10_000 * std::mem::size_of::<SemanticNodeId>());

    let merged = SemanticNodeData::MergedDecl {
        contributors: PayloadArray::from(ids.clone()),
    };
    assert_eq!(payload_heap_bytes(&merged), expected);

    let elements: Vec<TupleElement> = ids
        .iter()
        .map(|&value| TupleElement {
            label: None,
            value,
            optional: false,
            rest: false,
        })
        .collect();
    let tuple = SemanticNodeData::Tuple {
        elements: PayloadArray::from(elements),
        readonly: false,
    };
    assert_eq!(
        payload_heap_bytes(&tuple),
        arc_block_bytes(10_000 * std::mem::size_of::<TupleElement>())
    );
}

#[test]
fn node_payload_footprint_shared_array_offers_one_key_and_descends_once() {
    let label: Arc<str> = Arc::from("label");
    let elements: PayloadArray<TupleElement> = (0..3)
        .map(|ordinal| TupleElement {
            label: Some(Arc::clone(&label)),
            value: SemanticNodeId(ordinal),
            optional: false,
            rest: false,
        })
        .collect();
    let first = SemanticNodeData::Tuple {
        elements: elements.share_for_tests(),
        readonly: false,
    };
    let second = SemanticNodeData::Tuple {
        elements: elements.share_for_tests(),
        readonly: true,
    };
    let array_key = arc_key(elements.as_arc());
    let label_key = arc_key(&label);

    let mut visitor = RecordingVisitor::default();
    first.visit_payload_heap(&mut visitor);
    // The array, then each element's label: the label is offered once per
    // element of the one descent.
    assert_eq!(
        visitor.shared,
        vec![array_key, label_key, label_key, label_key]
    );

    second.visit_payload_heap(&mut visitor);
    // The second payload offers the same array key; the visitor declines to
    // descend, so no label is offered again.
    assert_eq!(
        visitor.shared,
        vec![array_key, label_key, label_key, label_key, array_key]
    );
    assert_eq!(visitor.owned, 0);
}

#[test]
fn node_payload_footprint_repeated_quasi_counts_once_per_measurement() {
    let long: Arc<str> = Arc::from("q".repeat(10_000));
    let other: Arc<str> = Arc::from("tail");
    let quasis: Arc<[Arc<str>]> = Arc::from(vec![
        Arc::clone(&long),
        Arc::clone(&other),
        Arc::clone(&long),
    ]);
    let template = SemanticNodeData::TemplateLiteral {
        quasis: Arc::clone(&quasis),
        expressions: PayloadArray::from(vec![SemanticNodeId(1), SemanticNodeId(2)]),
    };

    let expected = arc_block_bytes(3 * std::mem::size_of::<Arc<str>>())
        + arc_block_bytes(10_000)
        + arc_block_bytes(4)
        + arc_block_bytes(2 * std::mem::size_of::<SemanticNodeId>());
    assert_eq!(payload_heap_bytes(&template), expected);
}
