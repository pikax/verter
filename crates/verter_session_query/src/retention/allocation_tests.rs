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

fn decision_fact(importer: &str, specifier: &str) -> FactVersionRef {
    use crate::facts::resolution::{
        ResolutionFactKey, ResolutionFactRef, ResolutionFactVersion, ResolutionQueryKey,
        ResolveContextId,
    };
    use crate::resolution::dto::{ResolutionContext, ResolvePhase, ResolveRequestKind};
    let query = ResolutionQueryKey::importer(
        importer,
        specifier,
        ResolutionContext {
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        },
        ResolveContextId::unowned(),
        crate::resolution::resolution_world_identity::ResolutionPopulation::Base,
    );
    FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(ResolutionFactRef::new(
        ResolutionFactKey::decision(query),
        ResolutionFactVersion::fresh(1),
    )))
}

#[test]
fn a_cloned_fact_charges_its_shared_canonical_once_per_alias_set() {
    let canonical = "c".repeat(3_000);
    let fact = whole_hash(canonical.clone());
    let clone = fact.clone();
    let mut aliases = AliasSet::new();
    assert_eq!(
        fact_heap_bytes(&fact, &mut aliases),
        arc_block_bytes(canonical.len()),
        "the canonical is charged as its one shared block"
    );
    assert_eq!(
        fact_heap_bytes(&clone, &mut aliases),
        0,
        "a clone shares the canonical this charge already counted"
    );
    assert_eq!(
        fact_heap_bytes(&fact, &mut aliases),
        0,
        "a repeat costs nothing more"
    );
    drop(clone);
    // A canonical one fact alone owns is counted without a set entry.
    let mut alone = AliasSet::new();
    assert_eq!(
        fact_heap_bytes(&fact, &mut alone),
        arc_block_bytes(canonical.len())
    );
    assert!(alone.is_empty());
    // A collecting measurement charges the canonical inline: it is never
    // registered as a shared leaf of its own.
    let mut collecting = AliasSet::collecting();
    assert_eq!(
        fact_heap_bytes(&fact, &mut collecting),
        arc_block_bytes(canonical.len())
    );
    assert!(collecting.take_leaves().is_empty());
}

#[test]
fn a_cloned_decision_charges_its_shared_query_once_per_alias_set() {
    let importer = "i".repeat(2_000);
    let fact = decision_fact(&importer, "./dep");
    let clone = fact.clone();
    let mut aliases = AliasSet::new();
    let once = fact_heap_bytes(&fact, &mut aliases);
    assert_eq!(
        once,
        arc_block_bytes(std::mem::size_of::<
            crate::facts::resolution::ResolutionQueryKey,
        >()) + arc_block_bytes(importer.len())
            + arc_block_bytes("./dep".len()),
        "the query identity and its texts are each charged once"
    );
    assert_eq!(
        fact_heap_bytes(&clone, &mut aliases),
        0,
        "a clone shares the query this charge already counted"
    );
    assert_eq!(
        fact_heap_bytes(&fact, &mut aliases),
        0,
        "a repeat costs nothing more"
    );
}

#[test]
fn an_owned_fact_signature_charges_a_canonical_its_facts_share_once() {
    let shared = whole_hash("s".repeat(10_000));
    let facts: Vec<FactVersionRef> = (0..8).map(|_| shared.clone()).collect();
    let signature = crate::facts::fact_cache::OwnedFactSignature::from(facts);
    let charged = signature.offer(&mut AliasSet::estimating());
    assert!(charged >= 10_000, "the shared canonical is charged");
    assert!(
        charged < 20_000,
        "eight facts naming one canonical charge its text once, not eight times: {charged}"
    );
}

#[test]
fn an_empty_canonical_charges_nothing() {
    let empty = FactVersionRef::FileWholeHash {
        canonical_id: Arc::default(),
        hash: FactHash16::default(),
    };
    assert_eq!(fact_heap_bytes(&empty, &mut AliasSet::new()), 0);
    assert_eq!(fact_heap_bytes(&empty, &mut AliasSet::collecting()), 0);
    // An empty text built as its own allocation still costs its counters.
    let allocated = FactVersionRef::FileWholeHash {
        canonical_id: Arc::from(String::new()),
        hash: FactHash16::default(),
    };
    assert_eq!(
        fact_heap_bytes(&allocated, &mut AliasSet::new()),
        arc_block_bytes(0)
    );
}

#[test]
fn in_population_copies_a_shared_decision_query() {
    use crate::facts::resolution::ResolutionFactKey;
    use crate::resolution::resolution_world_identity::{ResolutionPopulation, SessionFingerprint};
    let FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(fact)) =
        decision_fact("/src/main.ts", "./dep")
    else {
        unreachable!("a decision fact");
    };
    let base = fact.key.clone();
    let session = ResolutionPopulation::Session(SessionFingerprint::from_raw(1));
    let moved = base.in_population(session);
    assert_eq!(base.population(), ResolutionPopulation::Base);
    assert_eq!(moved.population(), session);
    let (
        ResolutionFactKey::Decision { query: before },
        ResolutionFactKey::Decision { query: after },
    ) = (&fact.key, &moved)
    else {
        unreachable!("both are decisions");
    };
    assert!(
        !Arc::ptr_eq(before, after),
        "the rewritten key owns its own copy of the query"
    );
    assert_eq!(fact.key.population(), ResolutionPopulation::Base);
}
