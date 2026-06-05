//! Behavioral overlay/base test for the cap-4 multi-candidate
//! `FamilySlots` substrate.
//!
//! Proves that two distinct views (e.g. a base file content and an
//! overlay edit) of the SAME content-free `SemanticQueryKey` coexist
//! as distinct candidates inside one `(family, slot)`, neither
//! overwriting the other.
//!
//! Discriminating contract: this test publishes two `MemoEntry`
//! candidates with distinct admission discriminants
//! (`(validated_at_generation, facts)`) into the same slot. Under the
//! pre-multi-candidate single-`MemoEntry`-per-slot substrate, the
//! second publish OVERWROTE the first —
//! `slot_candidate_count_for_tests` returned `1` and this test
//! FAILS. Under the cap-4 multi-candidate substrate both candidates
//! coexist (`count == 2`).
//!
//! The structural R6 guard at
//! `tests/r6_query_identity_keys_content_free.rs` independently pins
//! the key shape (content-free `DeclKey`). This test pins the
//! coexistence behaviour the key shape alone does not guarantee.

use std::sync::Arc;

use verter_session::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

/// Two distinct candidates (distinct
/// `(validated_at_generation, facts)` discriminants) for the SAME
/// content-free `SemanticQueryKey::Instantiate` key coexist in the
/// same slot under the cap-4 substrate.
#[test]
fn instantiate_slot_holds_two_concurrent_candidates_for_distinct_views() {
    let host = host();
    let canonical = "/multi_candidate/owner.ts";
    upsert(&host, canonical, "export type Foo = { value: number };\n");

    let graph = host.project_type_store().semantic_graph();

    // Content-free key (R6).
    let key = SemanticQueryKey::Instantiate {
        base: verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from(canonical), Arc::from("Foo")),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: verter_session::semantic_query::InstantiateContext::new(ProjectionReductionContext::published(ProjectionMode::Expanded), Default::default()),
    };

    // Publish two candidates with the SAME key but DIFFERENT
    // `validated_at_generation` discriminants. Under the
    // pre-multi-candidate single-`MemoEntry` substrate the second
    // publish would OVERWRITE the first.
    let value_v1 = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Number,
    ));
    let value_v2 = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));

    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_v1),
        verter_session::for_tests::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );

    let count_after_first = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        count_after_first, 1,
        "fixture invariant: a single publish leaves exactly 1 candidate; got {count_after_first}"
    );

    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_v2),
        verter_session::for_tests::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        200,
    );

    let count_after_second = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        count_after_second, 2,
        "MULTI-CANDIDATE BEHAVIOR: a second publish under a distinct \
         `(validated_at_generation, facts)` discriminant must COEXIST \
         with the first candidate (R20 overlay isolation). The \
         pre-multi-candidate single-`MemoEntry`-per-slot substrate \
         would have overwritten the first publish, leaving \
         `count == 1`. Got \
         {count_after_second}."
    );

    // Same-discriminant re-publish (same generation) REPLACES the
    // matching candidate in place — the count does not grow.
    let value_v1_again = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Boolean,
    ));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_v1_again),
        verter_session::for_tests::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );
    let count_after_replace = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        count_after_replace, 2,
        "REPLACE-IN-PLACE BEHAVIOR: a same-discriminant re-publish \
         (generation=100) must REPLACE the existing candidate rather \
         than appending. Got {count_after_replace}."
    );
}

/// `FamilySlots` candidate list is bounded by
/// `FAMILY_SLOT_CANDIDATE_CAP = 4`. Publishing five distinct
/// discriminants into the same `(family, slot)` MUST cap the slot at
/// 4 entries — older candidates FIFO-evict.
#[test]
fn family_slot_caps_concurrent_candidates_at_four_with_fifo_eviction() {
    let host = host();
    let canonical = "/multi_candidate/cap_check.ts";
    upsert(&host, canonical, "export type Bar = { x: number };\n");

    let graph = host.project_type_store().semantic_graph();

    let key = SemanticQueryKey::Instantiate {
        base: verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from(canonical), Arc::from("Bar")),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: verter_session::semantic_query::InstantiateContext::new(ProjectionReductionContext::published(ProjectionMode::Expanded), Default::default()),
    };

    // Publish 5 distinct candidates for the SAME key (5 distinct
    // generations). The cap-4 substrate must evict the oldest after
    // the 5th publish.
    for generation in 100..105u64 {
        let value = graph.intern_node(SemanticNodeData::Primitive(
            verter_session::semantic_query::PrimitiveKind::Boolean,
        ));
        graph.publish_with_carrier_dispatch_and_generation_for_tests(
            key.clone(),
            QueryResult::Value(value),
            verter_session::for_tests::ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
            Arc::from(Vec::new().into_boxed_slice()),
            generation,
        );
    }

    let count = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        count, 4,
        "CAP-4 BEHAVIOR: 5 distinct discriminants must leave exactly 4 \
         candidates in the slot (FIFO-evict the oldest). Got {count}."
    );
}

/// **Reverse-index per-candidate identity discriminator.**
///
/// A `(family, slot)` with two candidates A+C and B+C — both referencing
/// canonical C in their fact rails, but each referencing a distinct
/// second canonical A or B — must keep its `canonical_to_entries`
/// registrations PER-CANDIDATE.
///
/// Pre-fix behaviour (registration keyed `(family, slot)`):
///   invalidate_canonical("/a"):
///     - drains A+C correctly,
///     - cross-canonical cleanup walks (family, slot) and removes
///       `(family, slot)` from canonical /c — even though B+C still
///       lives in the slot and depends on /c.
///   invalidate_canonical("/c"):
///     - finds nothing in canonical_to_entries["/c"] (it was just
///       pruned), returns 0. B+C survives with stale facts.
///
/// Post-fix behaviour (registration keyed `(family, slot,
/// admission_seq)`):
///   invalidate_canonical("/a"):
///     - drains A+C correctly,
///     - cross-canonical cleanup walks (family, slot, A+C_seq) and
///       removes ONLY that seq's registration from canonical /c.
///       B+C's (family, slot, B+C_seq) registration on /c survives.
///   invalidate_canonical("/c"):
///     - reads canonical_to_entries["/c"] (still holds B+C's seq),
///       drains B+C correctly. Slot count goes to 0.
#[test]
fn multi_candidate_reverse_index_survives_sibling_invalidation() {
    use std::sync::Arc;
    use verter_session::for_tests::ReadSetSignature;
    use verter_session::resolver_core::FactVersionRef;
    use verter_session::semantic_query::{
        DepSignature, ProjectionMode, ProjectionReductionContext, QueryResult,
        SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    };
    use verter_session::{HostConfig, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());
    let graph = host.project_type_store().semantic_graph();

    let canonical_keyed = "/multi_candidate/reverse_index/owner.ts";
    let canonical_a = "/multi_candidate/reverse_index/a.ts";
    let canonical_b = "/multi_candidate/reverse_index/b.ts";
    let canonical_c = "/multi_candidate/reverse_index/c.ts";

    // Identity mode — no backfills — so each publish populates
    // EXACTLY one slot. Lets the test count registrations cleanly.
    let key = SemanticQueryKey::Instantiate {
        base: verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from(canonical_keyed), Arc::from("Owner")),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: verter_session::semantic_query::InstantiateContext::new(ProjectionReductionContext::published(ProjectionMode::Identity), Default::default()),
    };

    // Fact rail for candidate A+C: depends on /a.ts and /c.ts.
    let facts_a_c: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: canonical_a.to_string(),
            hash: [1u8; 16],
        },
        FactVersionRef::FileWholeHash {
            canonical_id: canonical_c.to_string(),
            hash: [2u8; 16],
        },
    ]);
    // Fact rail for candidate B+C: depends on /b.ts and /c.ts.
    let facts_b_c: Arc<[FactVersionRef]> = Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: canonical_b.to_string(),
            hash: [3u8; 16],
        },
        FactVersionRef::FileWholeHash {
            canonical_id: canonical_c.to_string(),
            hash: [4u8; 16],
        },
    ]);

    let value_a_c = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Number,
    ));
    let value_b_c = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));

    let self_roots_empty: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    let empty_dispatch: DepSignature = Arc::from(Vec::new());

    // Publish candidate A+C (generation 100).
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_a_c),
        ReadSetSignature::new(Arc::clone(&facts_a_c)),
        Arc::clone(&self_roots_empty),
        Arc::clone(&empty_dispatch),
        100,
    );
    // Publish candidate B+C (generation 200) — distinct discriminant
    // (different facts + different generation), so both coexist.
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_b_c),
        ReadSetSignature::new(Arc::clone(&facts_b_c)),
        Arc::clone(&self_roots_empty),
        Arc::clone(&empty_dispatch),
        200,
    );

    let initial_count = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        initial_count, 2,
        "fixture invariant: both candidates must coexist in the \
         Identity slot before any invalidation. Got {initial_count}."
    );
    // Both candidates' fact rails reference /c.ts. With per-candidate
    // registration each candidate registers its OWN
    // `(family, Identity, candidate_seq)` entry under
    // canonical_to_entries["/c"] — count = 2 (one per candidate).
    let count_on_c_initial = graph.canonical_to_entries_count(canonical_c);
    assert_eq!(
        count_on_c_initial, 2,
        "POST-FIX: both candidates register their OWN per-candidate \
         seq under canonical /c — count == 2. With PRE-FIX \
         (family, slot)-only registration the second candidate's \
         registration would HAVE OVERWRITTEN the first via \
         FxHashMap::insert key collision (same `(family, slot)`), \
         leaving count == 1. Got {count_on_c_initial}."
    );

    // Invalidate canonical A. This drains A+C (which references /a.ts)
    // and runs the cross-canonical cleanup for canonical /c (where A+C
    // had its own seq registration). B+C's separate seq registration
    // on /c must SURVIVE.
    let removed = graph.invalidate_canonical(canonical_a);
    assert_eq!(
        removed, 1,
        "invalidating /a.ts must drain exactly A+C. Got {removed}."
    );
    let after_a = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        after_a, 1,
        "after invalidating /a.ts, only B+C remains in the Identity \
         slot. Got {after_a}."
    );
    // The key assertion: B+C's registration on canonical /c is INTACT.
    let count_on_c_after_a = graph.canonical_to_entries_count(canonical_c);
    assert_eq!(
        count_on_c_after_a, 1,
        "POST-FIX: invalidating /a.ts must remove ONLY A+C's seq \
         registration from /c. B+C's separate seq registration on /c \
         must SURVIVE. Got {count_on_c_after_a}. PRE-FIX behaviour: \
         the cross-canonical cleanup removed `(family, slot)` from \
         /c (only one key, same as A+C's), leaving count == 0 and \
         B+C undrainable on a later /c edit."
    );

    // Invalidate canonical C — under the post-fix per-candidate
    // registration, this drains B+C. Under the pre-fix (family, slot)
    // registration, this would find canonical_to_entries["/c"] empty
    // (just stripped by /a's invalidation), and return 0, leaving B+C
    // as a stale stranded candidate.
    let removed_c = graph.invalidate_canonical(canonical_c);
    assert_eq!(
        removed_c, 1,
        "POST-FIX: invalidating /c.ts must drain B+C (registration \
         preserved by per-candidate identity). Got {removed_c}. \
         PRE-FIX: would have returned 0 because the registration was \
         stripped by the /a.ts invalidation's cleanup."
    );
    let final_count = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        final_count, 0,
        "POST-FIX: after both invalidations the Identity slot is \
         empty. Got {final_count}."
    );
}

/// **Orphan-stamp eviction discriminator.**
///
/// `FamilySlots::publish_one` FIFO-evicts the oldest candidate when the
/// slot is at cap-4. The eviction MUST drain the evicted candidate's
/// reverse-index registrations by the candidate's own
/// `admission_seq`, leaving behind no orphan stamp in
/// `canonical_to_entries`.
///
/// Pre-fix behaviour: `publish_one`'s cap-4 eviction silently removed
/// the candidate from the slot without pruning its
/// `canonical_to_entries` registrations. The orphan stamps remained
/// resident under canonicals the evicted candidate referenced; a later
/// `invalidate_canonical` of one of those canonicals would either
/// no-op (the inner shard remained but the entries map no longer held
/// the family) or worse, attempt to drain a non-existent entry and
/// produce phantom invalidations.
///
/// Post-fix behaviour: every displaced candidate (replacement or
/// FIFO victim) has its `(family, slot, admission_seq)` registrations
/// drained from `canonical_to_entries` immediately, so the reverse
/// index never holds an orphan stamp.
#[test]
fn family_slot_cap_eviction_prunes_orphan_reverse_index_stamp() {
    use std::sync::Arc;
    use verter_session::for_tests::ReadSetSignature;
    use verter_session::resolver_core::FactVersionRef;
    use verter_session::semantic_query::{
        DepSignature, ProjectionMode, ProjectionReductionContext, QueryResult,
        SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    };
    use verter_session::{HostConfig, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());
    let graph = host.project_type_store().semantic_graph();

    let canonical_keyed = "/multi_candidate/orphan_check/owner.ts";
    // Use Identity mode — backfill_targets(Identity) = [], so this
    // publish populates EXACTLY one slot. That lets the test isolate
    // FIFO-eviction's reverse-index drain from the orthogonal
    // backfill-slot behaviour.
    let key = SemanticQueryKey::Instantiate {
        base: verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(Arc::from(canonical_keyed), Arc::from("Foo")),
        args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        context: verter_session::semantic_query::InstantiateContext::new(ProjectionReductionContext::published(ProjectionMode::Identity), Default::default()),
    };

    // The OLDEST candidate (which the cap-4 FIFO will evict) references
    // canonical /oldest.ts uniquely. Five later candidates reference
    // DIFFERENT canonicals so the OLDEST is the only one registered
    // under /oldest.ts.
    let oldest_canonical = "/multi_candidate/orphan_check/oldest.ts";
    let oldest_facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: oldest_canonical.to_string(),
        hash: [99u8; 16],
    }]);
    let value = graph.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Boolean,
    ));
    let self_roots_empty: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    let empty_dispatch: DepSignature = Arc::from(Vec::new());

    // Publish the oldest candidate first (generation 0). With
    // Identity mode this populates EXACTLY the Identity slot — no
    // backfills.
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value),
        ReadSetSignature::new(Arc::clone(&oldest_facts)),
        Arc::clone(&self_roots_empty),
        Arc::clone(&empty_dispatch),
        0,
    );
    assert_eq!(
        graph.canonical_to_entries_count(oldest_canonical),
        1,
        "fixture invariant: with Identity mode the oldest candidate \
         registers EXACTLY ONCE under /oldest.ts (no backfills)."
    );

    // Publish 4 more candidates with DIFFERENT canonicals. The 5th
    // distinct admission past cap-4 evicts the oldest. The
    // post-eviction reverse-index registration count under /oldest.ts
    // MUST be 0 — the orphan stamps (one per populated slot) must
    // ALL be drained.
    for i in 1..=4u8 {
        let other_canonical = format!("/multi_candidate/orphan_check/other_{i}.ts");
        let other_facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
            canonical_id: other_canonical.clone(),
            hash: [i; 16],
        }]);
        let other_value = graph.intern_node(SemanticNodeData::Primitive(
            verter_session::semantic_query::PrimitiveKind::Number,
        ));
        graph.publish_with_carrier_dispatch_and_generation_for_tests(
            key.clone(),
            QueryResult::Value(other_value),
            ReadSetSignature::new(other_facts),
            Arc::clone(&self_roots_empty),
            Arc::clone(&empty_dispatch),
            (i as u64) * 100 + 1,
        );
    }

    // After 5 publishes the slot is capped at 4. The OLDEST (generation
    // 0) candidate was evicted.
    let slot_count = graph.slot_candidate_count_for_tests(&key);
    assert_eq!(
        slot_count, 4,
        "fixture invariant: cap-4 FIFO leaves exactly 4 candidates in \
         the primary slot. Got {slot_count}."
    );
    // The crucial post-fix assertion: the evicted candidate's
    // Identity-slot registration on /oldest.ts is GONE.
    assert_eq!(
        graph.canonical_to_entries_count(oldest_canonical),
        0,
        "POST-FIX: the FIFO-evicted oldest candidate's reverse-index \
         registration under /oldest.ts must be drained on eviction. \
         PRE-FIX (no per-candidate-seq drain on FamilySlots::publish): \
         this assertion would FAIL with count == 1 — an orphan stamp \
         under canonical_to_entries[\"/oldest.ts\"] with no \
         corresponding live candidate in the slot."
    );
}
