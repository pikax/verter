//! Discriminating unit test for the bundle-local context-keyed structural-body
//! memo. The cache is dead-code-until-wired, so this is a DIRECT unit test on
//! the new types (not a `VerterHost` resolve-through-dispatch test): it mints
//! slot ids via the registry API, round-trips cells through the memo, and proves
//! the context key distinguishes EXACTLY where a context-neutral key would
//! collide (the load-bearing discrimination).

use std::sync::Arc;

use super::{
    HotStructuralBodyCell, PreparedStructuralBodySlotId, StructuralBodyDescriptor,
    StructuralBodyKind, StructuralBodyMemo, StructuralBodyMemoKey, StructuralBodyRegistry,
    StructuralBodySpace,
};
use crate::semantic_query::{
    HotTypeRef, MemberMergeRole, SemanticNodeId, SurfaceProvenanceContext,
};

/// A descriptor for a type-space semantic body named `name`.
fn type_semantic_descriptor(name: &str) -> StructuralBodyDescriptor {
    StructuralBodyDescriptor {
        symbol_name: Arc::from(name),
        space: StructuralBodySpace::Type,
        body_kind: StructuralBodyKind::Semantic,
        local_scope: None,
    }
}

/// A distinct cell whose body handle wraps node `n`.
fn cell(n: u64) -> Arc<HotStructuralBodyCell> {
    Arc::new(HotStructuralBodyCell::new(HotTypeRef::new(SemanticNodeId(
        n,
    ))))
}

/// The three distinct lowering contexts the SAME body is exercised under: the
/// macro own-body surface, the heritage surface, and the macro-type-arg own-body
/// provenance — each yields different surface members, so each must memoize a
/// DISTINCT cell.
const CONTEXTS: [(MemberMergeRole, SurfaceProvenanceContext); 3] = [
    (
        MemberMergeRole::OwnBody,
        SurfaceProvenanceContext::Structural,
    ),
    (
        MemberMergeRole::Heritage,
        SurfaceProvenanceContext::Structural,
    ),
    (
        MemberMergeRole::OwnBody,
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
    ),
];

#[test]
fn context_key_distinguishes_exactly_where_a_neutral_key_collides() {
    // Register ONE body descriptor → one slot id.
    let mut registry = StructuralBodyRegistry::new();
    let s = registry.register(type_semantic_descriptor("Props"));

    // Build the three context keys for the SAME slot `s`, and insert a DISTINCT
    // cell under each.
    let mut memo = StructuralBodyMemo::new();
    let mut keys = Vec::new();
    let mut expected_nodes = Vec::new();
    for (i, (merge_role, provenance)) in CONTEXTS.iter().enumerate() {
        let key = StructuralBodyMemoKey::new(s, *provenance, *merge_role);
        let node = 100 + i as u64;
        memo.insert(key, cell(node));
        keys.push(key);
        expected_nodes.push(node);
    }

    // -- Assertion 1: distinct-context distinctness. ------------------------
    // The memo holds 3 distinct entries, and each context's `get` returns ITS
    // OWN cell (compared by the cell's body-handle node) — never another
    // context's.
    assert_eq!(
        memo.len(),
        3,
        "three distinct contexts of the same slot must memoize three distinct entries"
    );
    for (i, key) in keys.iter().enumerate() {
        let got = memo
            .get(key)
            .expect("each inserted context must round-trip back from the memo");
        assert_eq!(
            got.body.node(),
            SemanticNodeId(expected_nodes[i]),
            "context #{i} must return ITS OWN cell, never another context's lowered body"
        );
        // And it must NOT equal any OTHER context's node — explicit cross-context
        // non-aliasing (catches a memo that collapsed two contexts onto one cell).
        for (j, other_node) in expected_nodes.iter().enumerate() {
            if i != j {
                assert_ne!(
                    got.body.node(),
                    SemanticNodeId(*other_node),
                    "context #{i} must NOT return context #{j}'s cell — the contexts are partitioned"
                );
            }
        }
    }

    // -- Assertion 2: wrong-context miss. -----------------------------------
    // A `get` for a context that was NEVER inserted returns `None`. `(Authored,
    // Structural)` is a real, valid context but was not one of the three
    // inserted, so it must miss.
    let never_inserted = StructuralBodyMemoKey::new(
        s,
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::Authored,
    );
    assert!(
        memo.get(&never_inserted).is_none(),
        "a context that was never inserted must miss (return None), not alias an inserted cell"
    );

    // -- Assertion 3: key inequality. ---------------------------------------
    // The three real keys are pairwise `!=` …
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            assert_ne!(
                keys[i], keys[j],
                "the real context keys #{i} and #{j} must be unequal — they differ by \
                 (provenance, merge_role)"
            );
        }
    }
    // … and a key for a DIFFERENT slot id is `!=` every first-slot key.
    let s2 = registry.register(type_semantic_descriptor("Other"));
    assert_ne!(s, s2, "a second registration must mint a DIFFERENT slot id");
    let other_slot_key = StructuralBodyMemoKey::new(
        s2,
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
        MemberMergeRole::OwnBody,
    );
    for (i, key) in keys.iter().enumerate() {
        assert_ne!(
            *key, other_slot_key,
            "a key for a DIFFERENT slot id must be unequal to first-slot key #{i}"
        );
    }

    // -- Assertion 4 (ILLUSTRATIVE CONTRAST, not the discriminator): a -------
    //    context-NEUTRAL key would be WRONG.
    // NOTE ON DISCRIMINATION STRUCTURE: this block is TRUE BY CONSTRUCTION — it
    // builds literal `(body_slot,)` tuples and a slot-only map, so it would pass
    // regardless of the REAL key's behavior. It proves nothing ON ITS OWN; it is
    // the pedagogical CONTRAST showing what a context-neutral key WOULD do. The
    // LOAD-BEARING discrimination of the context axes lives in assertions 1 + 3:
    // assertion 1 (`memo.len() == 3`) FAILS RED if either `merge_role` or
    // `provenance` is dropped from the real key (two of the three inserts would
    // collide), and assertion 3 asserts the three REAL keys are pairwise `!=`.
    // Do NOT read this block as the discriminator.
    //
    // Build a "neutral" tuple `(body_slot,)` for all three contexts (dropping
    // provenance + merge_role). They are ALL EQUAL — a neutral-keyed map would
    // collide all three onto ONE slot and serve the wrong cell. Contrast with
    // the REAL keys, which are pairwise UNEQUAL (assertion 3): the real key MUST
    // distinguish exactly where a neutral key MUST collide.
    let neutral: Vec<(PreparedStructuralBodySlotId,)> = CONTEXTS.iter().map(|_| (s,)).collect();
    for i in 0..neutral.len() {
        for j in (i + 1)..neutral.len() {
            assert_eq!(
                neutral[i], neutral[j],
                "the neutral (body_slot-only) keys #{i} and #{j} MUST collide — demonstrating a \
                 neutral-keyed map would serve the wrong cell across distinct contexts"
            );
        }
    }
    // Belt-and-braces: a neutral map keyed only on the slot keeps just ONE entry
    // for the three distinct contexts (the concrete collision the real memo
    // avoids).
    let mut neutral_map: std::collections::HashMap<PreparedStructuralBodySlotId, u64> =
        std::collections::HashMap::new();
    for (i, _) in CONTEXTS.iter().enumerate() {
        neutral_map.insert(s, expected_nodes[i]);
    }
    assert_eq!(
        neutral_map.len(),
        1,
        "a neutral (slot-only) map collapses three contexts to ONE entry — the WRONG behavior the \
         context key fixes; the real memo kept three (assertion 1)"
    );

    // -- Assertion 5: registry round-trip + out-of-range miss. --------------
    // `descriptor(s)` returns the registered descriptor; the second distinct
    // `register` minted a DIFFERENT id (asserted above); an OUT-OF-RANGE id has
    // no descriptor.
    let desc = registry
        .descriptor(s)
        .expect("the registered slot must round-trip its descriptor");
    assert_eq!(
        &*desc.symbol_name, "Props",
        "the descriptor must round-trip the registered symbol name"
    );
    assert_eq!(
        desc.space,
        StructuralBodySpace::Type,
        "the descriptor must round-trip the registered space"
    );
    assert_eq!(
        desc.body_kind,
        StructuralBodyKind::Semantic,
        "the descriptor must round-trip the registered body kind"
    );
    assert_eq!(registry.len(), 2, "exactly two bodies were registered");

    // Out-of-range negative on the POPULATED registry: an index AT and PAST the
    // dense bound (`registry.len()` and beyond) has no descriptor, because
    // `descriptor` checks only the dense `Vec` bound. This DISCRIMINATES — it
    // fails RED if `descriptor` ever over-returned for an out-of-range id (e.g.
    // saturated to the last descriptor instead of bounds-checking). Slot ids are
    // bundle-local (each `PreparedDeclBundle` owns exactly one registry and an id
    // is consumed within the registry that minted it), so a cross-registry id is
    // a non-scenario; the real, testable guarantee is this dense out-of-range
    // bound, exercised via the `#[cfg(test)]` raw constructor (no public
    // `from_raw` on the production surface).
    let at_bound = PreparedStructuralBodySlotId::from_raw_for_test(registry.len() as u32);
    let well_past = PreparedStructuralBodySlotId::from_raw_for_test(registry.len() as u32 + 100);
    assert!(
        registry.descriptor(at_bound).is_none(),
        "an id exactly AT the dense bound (len) is out of range — no descriptor"
    );
    assert!(
        registry.descriptor(well_past).is_none(),
        "an id well PAST the dense bound is out of range — no descriptor"
    );
    // Sanity floor: the last in-range id (len - 1) DOES resolve, so the
    // out-of-range misses above are a real bound check, not a registry that
    // resolves nothing.
    let last_in_range = PreparedStructuralBodySlotId::from_raw_for_test(registry.len() as u32 - 1);
    assert!(
        registry.descriptor(last_in_range).is_some(),
        "the last in-range id (len - 1) must resolve — proving the out-of-range misses are a \
         genuine bound check, not a registry that resolves nothing"
    );
}
