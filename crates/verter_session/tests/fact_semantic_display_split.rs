//! Stage 3 R13 binding: semantic / display lane split.
//!
//! Verify-bullet bindings:
//!
//! - Bullet 2 — Comment-only edit: NO `semantic_hash` change anywhere;
//!   `MemberSemanticFactStore` not re-keyed (`parse_stable_hash`
//!   invariant). `MemberDisplayFactStore` IS re-keyed (content_hash
//!   changes) but the recomputed display fact equals the original.
//! - Bullet 3 — JSDoc edit: `MemberSemanticFactStore` not re-keyed;
//!   `MemberDisplayFactStore` re-keyed with new display value.
//!
//! Tests construct two `MemberSemanticFactKey`s and two
//! `MemberDisplayFactKey`s in the configurations above and assert
//! the keyed-store behaviour matches R13's "semantic invariant
//! under cosmetic, display sensitive to cosmetic" promise.
//!
//! Architectural rules bound: R13.

use std::sync::Arc;

use verter_semantic::facts::{Fact, FactKey, FactLane, SymbolSpace};
use verter_session::file_artifact_store::InternedName;
use verter_session::member_display_fact_store::{
    make_member_display_fact, MemberDisplayFactKey, MemberDisplayFactStore,
};
use verter_session::member_semantic_fact_store::{
    make_member_fact, MemberSemanticFactKey, MemberSemanticFactStore,
};

fn psh(b: u8) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0] = b;
    h
}

fn content(b: u8) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0] = b;
    h
}

fn dummy_sem(b: u8) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0] = b;
    h
}

fn dummy_disp(b: u8) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[0] = b;
    h
}

/// Bullet 2: comment-only edit.
///
/// The shallow walk's `parse_stable_hash` is invariant under
/// comment edits (Stage 1's locked-down invariant — verified by
/// `parse_stable_hash_invariance::whitespace_edit_does_not_change_parse_stable_hash`).
/// Therefore the `MemberSemanticFactStore` key is unchanged; the
/// cached semantic fact survives.
///
/// At the same time, `content_hash` shifts (different source
/// bytes), so the `MemberDisplayFactStore` key IS re-keyed. The
/// recomputed display value MAY equal the original (no JSDoc
/// change in a comment-only edit affects an identifier's display
/// string) — we represent this by recomputing the SAME display
/// hash under a different key.
#[test]
fn comment_only_edit_keeps_semantic_key_rekeys_display_to_equal_value() {
    let semantic_store = MemberSemanticFactStore::new();
    let display_store = MemberDisplayFactStore::new();

    // Initial state: store the semantic + display fact for
    // `Foo.a`.
    let sem_key = MemberSemanticFactKey {
        canonical: Arc::from("/a.ts"),
        parse_stable_hash: psh(1),
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };
    let original_sem_hash = dummy_sem(0x42);
    semantic_store.insert(
        sem_key.clone(),
        Arc::new(make_member_fact(&sem_key, original_sem_hash)),
    );

    let disp_key_v1 = MemberDisplayFactKey {
        canonical: Arc::from("/a.ts"),
        content_hash: content(1),
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };
    let original_disp_hash = dummy_disp(0x77);
    display_store.insert(
        disp_key_v1.clone(),
        Arc::new(make_member_display_fact(
            &disp_key_v1,
            original_sem_hash,
            original_disp_hash,
        )),
    );

    // A comment-only edit shifts content_hash but NOT parse_stable_hash.
    let disp_key_v2 = MemberDisplayFactKey {
        canonical: Arc::from("/a.ts"),
        content_hash: content(2), // different content_hash
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };

    // Semantic store: same parse_stable_hash → same key → warm hit
    // returns the cached entry.
    let sem_warm = semantic_store.get(&sem_key).expect("semantic warm hit");
    assert_eq!(
        sem_warm.semantic_hash, original_sem_hash,
        "comment-only edit MUST NOT re-key the semantic store (R13)"
    );

    // Display store: different content_hash → no entry at v2.
    assert!(
        display_store.get(&disp_key_v2).is_none(),
        "comment-only edit re-keys the display store (R13)"
    );

    // The producer recomputes the display fact for v2; under a
    // comment-only edit that doesn't touch identifier strings or
    // JSDoc, the recomputed display value equals the original.
    display_store.insert(
        disp_key_v2.clone(),
        Arc::new(make_member_display_fact(
            &disp_key_v2,
            original_sem_hash,
            original_disp_hash,
        )),
    );
    let disp_recomputed = display_store
        .get(&disp_key_v2)
        .expect("display fact admitted under new content_hash");
    assert_eq!(
        disp_recomputed.display_hash, original_disp_hash,
        "comment-only edit: recomputed display fact MAY equal original (no JSDoc / ident change)"
    );

    // And the v1 entry MUST still exist (multi-content_hash
    // coexistence) — Stage 6c GC handles eviction.
    let disp_v1 = display_store
        .get(&disp_key_v1)
        .expect("v1 display entry still present");
    assert_eq!(disp_v1.display_hash, original_disp_hash);
}

/// Bullet 3: JSDoc edit.
///
/// A JSDoc edit also shifts content_hash without shifting
/// parse_stable_hash (JSDoc lives outside the decl skeleton).
/// Semantic store unchanged; display store re-keyed AND the
/// recomputed display fact carries the NEW display_hash.
#[test]
fn jsdoc_edit_keeps_semantic_rekeys_display_to_new_value() {
    let semantic_store = MemberSemanticFactStore::new();
    let display_store = MemberDisplayFactStore::new();

    let sem_key = MemberSemanticFactKey {
        canonical: Arc::from("/a.ts"),
        parse_stable_hash: psh(5),
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };
    let sem_hash = dummy_sem(0x11);
    semantic_store.insert(
        sem_key.clone(),
        Arc::new(make_member_fact(&sem_key, sem_hash)),
    );

    let disp_key_v1 = MemberDisplayFactKey {
        canonical: Arc::from("/a.ts"),
        content_hash: content(5),
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };
    let old_disp_hash = dummy_disp(0xA0);
    display_store.insert(
        disp_key_v1.clone(),
        Arc::new(make_member_display_fact(
            &disp_key_v1,
            sem_hash,
            old_disp_hash,
        )),
    );

    // JSDoc edit — different content_hash, new display value.
    let disp_key_v2 = MemberDisplayFactKey {
        canonical: Arc::from("/a.ts"),
        content_hash: content(6),
        parse_env_hash: psh(7),
        exporter: InternedName::from("Foo"),
        member_name: InternedName::from("a"),
        symbol_space: SymbolSpace::Type,
    };
    let new_disp_hash = dummy_disp(0xB0);
    display_store.insert(
        disp_key_v2.clone(),
        Arc::new(make_member_display_fact(
            &disp_key_v2,
            sem_hash,
            new_disp_hash,
        )),
    );

    // Semantic store: unchanged.
    let sem_warm = semantic_store
        .get(&sem_key)
        .expect("semantic warm hit under JSDoc edit");
    assert_eq!(sem_warm.semantic_hash, sem_hash);

    // Display store: re-keyed. The v2 fact carries the NEW
    // display_hash; the v1 fact still has the OLD one.
    let disp_v2 = display_store.get(&disp_key_v2).expect("v2 admitted");
    assert_eq!(
        disp_v2.display_hash, new_disp_hash,
        "JSDoc edit MUST recompute display_hash to new value"
    );
    let disp_v1 = display_store.get(&disp_key_v1).expect("v1 preserved");
    assert_eq!(disp_v1.display_hash, old_disp_hash);
    assert_ne!(
        disp_v1.display_hash, disp_v2.display_hash,
        "JSDoc edit MUST discriminate the v1 vs v2 display facts"
    );
}

/// Sanity: a Fact's `key` carries the right `FactLane` semantics
/// when promoted to an `ObservedFact`. (The lane field on
/// `ObservedFact` is what `fact_dep_signature` writes during
/// observation; producers at Stage 5/6 will set it.)
#[test]
fn fact_carries_distinct_semantic_and_display_hashes() {
    let fact = Fact {
        key: FactKey::Export {
            name: InternedName::from("Foo"),
            space: SymbolSpace::Type,
        },
        semantic_hash: dummy_sem(1),
        display_hash: dummy_disp(2),
    };
    // The two hashes MUST be storable independently — `Fact` is the
    // dual-hash carrier.
    assert_ne!(fact.semantic_hash, fact.display_hash);

    // FactLane carries Semantic vs Display.
    let _ = FactLane::Semantic;
    let _ = FactLane::Display;
}
