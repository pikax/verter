//! Sub-task B accessor coverage — `MemberSemanticFactStore` /
//! `MemberDisplayFactStore` are reachable through
//! `ProjectTypeStore::{member_semantic_fact_store,
//! member_display_fact_store}` and participate in the per-canonical
//! eviction cascade (per `evict_canonical_inventory.json`).
//!
//! Discriminating signals:
//! - Empty-host accessor returns an empty store (control).
//! - Direct `insert` populates the store via the accessor.
//! - `evict_canonical` drops the canonical's entries from BOTH
//!   stores in the same call, while leaving entries for sibling
//!   canonicals intact.
//!
//! Pairs with `cache_baseline_characterisation::evict_canonical_drain_inventory_matches_source_body`
//! — that audit-test enforces the fixture entries match the
//! `evict_canonical` source body; this test enforces the runtime
//! behaviour the fixture documents.

use std::sync::Arc;

use verter_semantic::analysis::Hash16;
use verter_semantic::facts::{Fact, FactKey, SymbolSpace};

use verter_session::file_artifact_store::InternedName;
use verter_session::member_display_fact_store::MemberDisplayFactKey;
use verter_session::member_semantic_fact_store::MemberSemanticFactKey;
use verter_session::project_type_store::ProjectTypeStore;

fn hash(byte: u8) -> Hash16 {
    let mut h = [0u8; 16];
    h[0] = byte;
    h
}

fn semantic_key(canonical: &str, exporter: &str, name: &str) -> MemberSemanticFactKey {
    MemberSemanticFactKey {
        canonical: Arc::from(canonical),
        parse_stable_hash: hash(0x10),
        parse_env_hash: hash(0x20),
        exporter: InternedName::from(exporter),
        member_name: InternedName::from(name),
        symbol_space: SymbolSpace::Type,
    }
}

fn display_key(canonical: &str, exporter: &str, name: &str) -> MemberDisplayFactKey {
    MemberDisplayFactKey {
        canonical: Arc::from(canonical),
        content_hash: hash(0x30),
        parse_env_hash: hash(0x20),
        exporter: InternedName::from(exporter),
        member_name: InternedName::from(name),
        symbol_space: SymbolSpace::Type,
    }
}

fn dummy_fact(byte: u8, exporter: &str, name: &str) -> Arc<Fact> {
    Arc::new(Fact {
        key: FactKey::Member {
            exporter: InternedName::from(exporter),
            name: InternedName::from(name),
            space: SymbolSpace::Type,
        },
        semantic_hash: hash(byte),
        display_hash: hash(byte ^ 0xff),
    })
}

#[test]
fn fresh_store_is_empty_via_accessor() {
    let store = ProjectTypeStore::new();
    assert!(
        store.member_semantic_fact_store().is_empty(),
        "fresh ProjectTypeStore must expose an empty member_semantic_fact_store"
    );
    assert!(
        store.member_display_fact_store().is_empty(),
        "fresh ProjectTypeStore must expose an empty member_display_fact_store"
    );
}

#[test]
fn insert_via_accessor_is_observable_via_get() {
    let store = ProjectTypeStore::new();

    let sem_key = semantic_key("/w/a.ts", "Foo", "x");
    let fact = dummy_fact(0x11, "Foo", "x");
    store
        .member_semantic_fact_store()
        .insert(sem_key.clone(), fact.clone());

    let read_back = store
        .member_semantic_fact_store()
        .get(&sem_key)
        .expect("semantic-lane entry must be reachable through the accessor");
    assert!(Arc::ptr_eq(&read_back, &fact));

    let dis_key = display_key("/w/a.ts", "Foo", "x");
    store
        .member_display_fact_store()
        .insert(dis_key.clone(), fact.clone());
    let read_back = store
        .member_display_fact_store()
        .get(&dis_key)
        .expect("display-lane entry must be reachable through the accessor");
    assert!(Arc::ptr_eq(&read_back, &fact));
}

#[test]
fn evict_canonical_drops_both_lanes_for_the_canonical_only() {
    let store = ProjectTypeStore::new();
    let edited_canonical = "/w/edited.ts";
    let sibling_canonical = "/w/sibling.ts";

    // Two entries on the edited canonical (one per lane).
    store.member_semantic_fact_store().insert(
        semantic_key(edited_canonical, "Edited", "x"),
        dummy_fact(0x01, "Edited", "x"),
    );
    store.member_display_fact_store().insert(
        display_key(edited_canonical, "Edited", "x"),
        dummy_fact(0x02, "Edited", "x"),
    );
    // Sibling canonical's entries must survive.
    store.member_semantic_fact_store().insert(
        semantic_key(sibling_canonical, "Sib", "y"),
        dummy_fact(0x03, "Sib", "y"),
    );
    store.member_display_fact_store().insert(
        display_key(sibling_canonical, "Sib", "y"),
        dummy_fact(0x04, "Sib", "y"),
    );

    assert_eq!(store.member_semantic_fact_store().len(), 2);
    assert_eq!(store.member_display_fact_store().len(), 2);

    store.evict_canonical(edited_canonical);

    // Edited canonical: both lanes drained.
    assert!(
        store
            .member_semantic_fact_store()
            .get(&semantic_key(edited_canonical, "Edited", "x"))
            .is_none(),
        "evict_canonical must drop the semantic-lane entry for the edited canonical"
    );
    assert!(
        store
            .member_display_fact_store()
            .get(&display_key(edited_canonical, "Edited", "x"))
            .is_none(),
        "evict_canonical must drop the display-lane entry for the edited canonical"
    );

    // Sibling canonical: both lanes preserved (discrimination —
    // a "drop everything" stub would fail this assertion).
    assert!(
        store
            .member_semantic_fact_store()
            .get(&semantic_key(sibling_canonical, "Sib", "y"))
            .is_some(),
        "evict_canonical must NOT drop semantic-lane entries for unrelated canonicals"
    );
    assert!(
        store
            .member_display_fact_store()
            .get(&display_key(sibling_canonical, "Sib", "y"))
            .is_some(),
        "evict_canonical must NOT drop display-lane entries for unrelated canonicals"
    );
    assert_eq!(
        store.member_semantic_fact_store().len(),
        1,
        "exactly one semantic-lane entry survives the eviction (sibling)"
    );
    assert_eq!(
        store.member_display_fact_store().len(),
        1,
        "exactly one display-lane entry survives the eviction (sibling)"
    );
}
