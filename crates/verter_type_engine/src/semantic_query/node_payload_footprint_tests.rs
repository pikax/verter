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
    handles: usize,
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

    fn handle(
        &mut self,
        _offer: &mut dyn FnMut(&mut verter_session_query::retention::allocation::AliasSet) -> usize,
    ) {
        self.handles += 1;
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
    let label = verter_type_expr::OwnedStr::from("label");
    let elements: PayloadArray<TupleElement> = (0..3)
        .map(|ordinal| TupleElement {
            label: Some(label.clone()),
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

    let mut visitor = RecordingVisitor::default();
    first.visit_payload_heap(&mut visitor);
    // The array is a shared allocation; each element's label is an owned
    // handle, offered once per element of the one descent and never
    // counted as payload heap.
    assert_eq!(visitor.shared, vec![array_key]);
    assert_eq!(visitor.handles, 3);

    second.visit_payload_heap(&mut visitor);
    // The second payload offers the same array key; the visitor declines to
    // descend, so no label is offered again.
    assert_eq!(visitor.shared, vec![array_key, array_key]);
    assert_eq!(visitor.handles, 3);
    assert_eq!(visitor.owned, 0);
}

/// A template's quasi list is an owned allocation: the payload counts none
/// of it, and the list's own charge counts a repeated quasi once.
#[test]
fn node_payload_footprint_repeated_quasi_counts_once_per_measurement() {
    use verter_session_query::retention::owned::ChargeCtx;
    use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};
    use verter_type_expr::{OwnedSlice, OwnedStr};

    let long = OwnedStr::from("q".repeat(10_000));
    let other = OwnedStr::from("tail");
    let quasis = OwnedSlice::from(vec![long.clone(), other.clone(), long.clone()]);
    let template = SemanticNodeData::TemplateLiteral {
        quasis: quasis.clone(),
        expressions: PayloadArray::from(vec![SemanticNodeId(1), SemanticNodeId(2)]),
    };
    assert_eq!(
        payload_heap_bytes(&template),
        arc_block_bytes(2 * std::mem::size_of::<SemanticNodeId>())
    );

    let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
    quasis.pin(ChargeCtx::new(&account));
    assert_eq!(
        quasis.charge_bytes(),
        Some(
            OwnedSlice::<OwnedStr>::cell_block_bytes()
                + heap_block_bytes(3 * std::mem::size_of::<OwnedStr>())
        )
    );
    assert_eq!(long.charge_bytes(), Some(OwnedStr::charge_for_len(10_000)));
    assert_eq!(other.charge_bytes(), Some(OwnedStr::charge_for_len(4)));
    assert_eq!(
        account.snapshot().pinned_bytes,
        quasis.charge_bytes().unwrap()
            + OwnedStr::charge_for_len(10_000)
            + OwnedStr::charge_for_len(4),
        "the repeated quasi is charged once"
    );
}
