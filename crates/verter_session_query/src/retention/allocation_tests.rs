use std::sync::Arc;

use super::*;
use crate::facts::registry::{InternedName, SymbolSpace};
use crate::facts::version::{FactHash16, ParseFactRef};

fn whole_hash(canonical: String) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.into(),
        hash: FactHash16::default(),
    }
}

#[test]
fn heap_blocks_follow_the_declared_allocator_convention() {
    assert_eq!(heap_block_bytes(0), 0, "an empty payload allocates nothing");
    assert_eq!(heap_block_bytes(1), 32);
    assert_eq!(heap_block_bytes(16), 32);
    assert_eq!(heap_block_bytes(17), 48);
    assert_eq!(
        arc_block_bytes(0),
        32,
        "an empty Arc slice still allocates its counters"
    );
}

#[test]
fn strings_and_vectors_cost_their_capacity_not_their_length() {
    let mut text = String::with_capacity(10_000);
    text.push('a');
    assert!(string_bytes(&text) >= 10_000);
    let mut items: Vec<u64> = Vec::with_capacity(1_000);
    items.push(1);
    assert!(vec_bytes(&items) >= 8_000);
}

#[test]
fn a_fact_signature_is_charged_for_its_variable_canonical_text() {
    let short: Arc<[FactVersionRef]> = Arc::from(vec![whole_hash("/a.ts".into())]);
    let long: Arc<[FactVersionRef]> = Arc::from(vec![whole_hash("x".repeat(10_000))]);
    let short_bytes = fact_slice_bytes(&short, &mut AliasSet::new());
    let long_bytes = fact_slice_bytes(&long, &mut AliasSet::new());
    // The former per-fact proxy charged both alike (64 bytes a fact); a
    // complete charge covers the ten-kilobyte canonical the long one owns.
    assert!(long_bytes >= short_bytes + 9_900);
    assert!(long_bytes >= 10_000 + std::mem::size_of::<FactVersionRef>());
}

#[test]
fn one_charge_counts_a_shared_name_and_a_shared_slice_once() {
    let name = InternedName(Arc::from("n".repeat(4_000)));
    let parse = |canonical: &str| {
        FactVersionRef::Parse(ParseFactRef {
            canonical_id: canonical.into(),
            key: FactKey::Export {
                name: name.clone(),
                space: SymbolSpace::Type,
            },
            lane: crate::facts::registry::FactLane::Semantic,
            expected_hash: FactHash16::default(),
        })
    };
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![parse("/a.ts"), parse("/b.ts")]);
    let mut aliases = AliasSet::new();
    let once = fact_slice_bytes(&facts, &mut aliases);
    assert!(once >= 4_000, "the shared name is charged");
    assert!(
        once < 8_000,
        "two facts naming one interned name charge its text once"
    );
    assert_eq!(
        fact_slice_bytes(&facts, &mut aliases),
        0,
        "an alias of an allocation this charge already counted costs nothing more"
    );
}

#[test]
fn an_evidence_receipt_costs_only_its_slot() {
    let page = crate::facts::receipt::ResultReceipt::page_on(
        vec![whole_hash("y".repeat(5_000))],
        &crate::retention::SemanticRetentionAccount::new(
            crate::retention::RetentionLimits::defaults(),
        ),
    );
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::Receipt(page)]);
    assert_eq!(
        fact_slice_bytes(&facts, &mut AliasSet::new()),
        arc_block_bytes(std::mem::size_of::<FactVersionRef>()),
        "a page carries its own charge, so its holder is charged only the slot"
    );
}

#[test]
fn an_alias_set_counts_each_allocation_once_past_its_inline_capacity() {
    let names: Vec<Arc<str>> = (0..20).map(|index| Arc::from(format!("{index}"))).collect();
    let mut aliases = AliasSet::new();
    for name in &names {
        assert!(aliases.first(name));
    }
    for name in &names {
        assert!(
            !aliases.first(name),
            "an alias is counted once, inline or spilled"
        );
    }
    assert_eq!(aliases.len(), names.len());
}
