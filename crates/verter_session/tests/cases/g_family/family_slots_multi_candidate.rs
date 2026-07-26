//! Behavioral overlay/base tests for the multi-candidate `FamilySlots`
//! substrate and its per-family bounded-retention policy
//! (`U3.ADAPTIVE_FAMILY_RETENTION`).
//!
//! Proves that distinct views (e.g. a base file content and an overlay
//! edit) of the SAME content-free `SemanticQueryKey` coexist as distinct
//! candidates inside one `(family, slot)`, neither overwriting the
//! other — and that retention is bounded PER FAMILY: each family's slot
//! keeps at most `FamilyKey::candidate_cap()` candidates, evicting a
//! candidate invalid against the publishing caller's stable store view
//! FIRST and the least-recently validated-hit candidate otherwise, with
//! a new cacheable candidate ALWAYS admitted after local eviction. This
//! replaces the legacy uniform `FAMILY_SLOT_CANDIDATE_CAP = 4` +
//! front-removal (FIFO) retention.
//!
//! The structural R6 guard at
//! `tests/cases/g_block/r6_query_identity_keys_content_free.rs` independently pins
//! the key shape (the env-bearing content-free `ResolvedDeclSlotIdentity`
//! slot). These tests pin the coexistence + retention behaviour the key
//! shape alone does not guarantee.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::resolver_core::FactVersionRef;
use verter_session::semantic_query::{
    DepSignature, PrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: verter_session::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

fn instantiate_identity_key(host: &VerterHost, canonical: &str, symbol: &str) -> SemanticQueryKey {
    // Content-free key (R6), built through the production-shaped helper —
    // the sealed `Instantiate` payload is not externally constructible. The
    // key is built once and reused for every publish and lookup, so its
    // source-kind (real-file `FileBacked`) is self-consistent. Identity
    // mode: `slot_domain_siblings(Identity) == []`, so each publish
    // populates EXACTLY one slot and the assertions stay single-slot.
    verter_session::for_tests::instantiate_key_for_tests(
        host,
        verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(symbol),
        ),
        Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Identity),
    )
}

/// Two distinct candidates (distinct
/// `(validated_at_generation, facts)` discriminants) for the SAME
/// content-free `SemanticQueryKey::Instantiate` key coexist in the
/// same slot under the multi-candidate substrate.
#[test]
fn instantiate_slot_holds_two_concurrent_candidates_for_distinct_views() {
    let host = host();
    let canonical = "/multi_candidate/owner.ts";
    upsert(&host, canonical, "export type Foo = { value: number };\n");

    let graph = host.project_type_store().semantic_graph();
    let key = instantiate_identity_key(&host, canonical, "Foo");

    // Publish two candidates with the SAME key but DIFFERENT
    // `validated_at_generation` discriminants. Under the
    // pre-multi-candidate single-`MemoEntry` substrate the second
    // publish would OVERWRITE the first.
    let value_v1 = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let value_v2 = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_v1),
        ReadSetSignature::empty(),
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
        ReadSetSignature::empty(),
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
    let value_v1_again = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        key.clone(),
        QueryResult::Value(value_v1_again),
        ReadSetSignature::empty(),
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

/// **Per-family bounded retention — `candidate_cap()` is per-family, not
/// a uniform constant.**
///
/// The inference/substitution-heavy `Instantiate` family caps its slot
/// at 8 candidates; content-light families such as `NormalizeUnion`
/// keep the floor of 4. The cap actually GOVERNS retention: five
/// distinct-discriminant publishes leave all five candidates in the
/// `Instantiate` slot but bound the `NormalizeUnion` slot at 4, and a
/// new cacheable candidate is ALWAYS admitted after local eviction
/// (bounded occupancy + always-admit). With every candidate still
/// valid, the victim is the front of the LRU order (the
/// least-recently validated-hit candidate).
///
/// RED against the legacy uniform `FAMILY_SLOT_CANDIDATE_CAP = 4`: the
/// `Instantiate` slot caps at 4, so the five-candidate coexistence
/// assertion fails. RED under a uniformized-`candidate_cap` mutation at
/// ANY single value: one of the two per-family probe / occupancy pairs
/// fails.
#[test]
fn cache_candidate_cap_is_per_family_not_uniform() {
    let host = host();
    let canonical = "/multi_candidate/per_family_cap/owner.ts";
    upsert(&host, canonical, "export type Bar = { x: number };\n");

    let graph = host.project_type_store().semantic_graph();

    let instantiate_key = instantiate_identity_key(&host, canonical, "Bar");
    let member = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let normalize_key = SemanticQueryKey::NormalizeUnion {
        members: Arc::from(vec![member].into_boxed_slice()),
    };

    // Policy probes — the exhaustive, wildcard-free per-family
    // `candidate_cap()` table.
    let instantiate_cap = graph.family_candidate_cap_for_tests(&instantiate_key);
    assert_eq!(
        instantiate_cap, 8,
        "POLICY: the inference/substitution-heavy `Instantiate` family \
         must declare a HIGHER cap than the floor (8). Got {instantiate_cap}."
    );
    let normalize_cap = graph.family_candidate_cap_for_tests(&normalize_key);
    assert_eq!(
        normalize_cap, 4,
        "POLICY: the content-light `NormalizeUnion` family must keep the \
         floor cap of 4. Got {normalize_cap}."
    );

    // Behavioral: five distinct-discriminant publishes into each slot.
    // The cap must GOVERN retention — a probe-only guard could be
    // satisfied by a dead `candidate_cap()` the publish path never
    // consults.
    let value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let self_roots_empty: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    let empty_dispatch: DepSignature = Arc::from(Vec::new());
    for generation in 100..105u64 {
        graph.publish_with_view_for_tests(
            &host,
            instantiate_key.clone(),
            QueryResult::Value(value),
            ReadSetSignature::empty(),
            Arc::clone(&self_roots_empty),
            Arc::clone(&empty_dispatch),
            generation,
        );
        graph.publish_with_view_for_tests(
            &host,
            normalize_key.clone(),
            QueryResult::Value(value),
            ReadSetSignature::empty(),
            Arc::clone(&self_roots_empty),
            Arc::clone(&empty_dispatch),
            generation,
        );
    }

    // `Instantiate` (cap 8): all five candidates coexist — the legacy
    // uniform cap-4 would have evicted generation 100.
    let instantiate_gens = graph.slot_candidate_generations_for_tests(&instantiate_key);
    assert_eq!(
        instantiate_gens,
        vec![100, 101, 102, 103, 104],
        "PER-FAMILY CAP BEHAVIOR: five distinct discriminants must ALL \
         coexist in the cap-8 `Instantiate` slot. The legacy uniform \
         cap-4 (or a uniformized `candidate_cap` mutation) leaves only \
         4. Got {instantiate_gens:?}."
    );
    // `NormalizeUnion` (floor 4): bounded occupancy at exactly 4; the
    // all-valid victim is the LRU front (generation 100); the newest
    // candidate (104) is ALWAYS admitted after local eviction.
    let normalize_gens = graph.slot_candidate_generations_for_tests(&normalize_key);
    assert_eq!(
        normalize_gens,
        vec![101, 102, 103, 104],
        "FLOOR-CAP BEHAVIOR: five distinct discriminants must leave \
         exactly 4 candidates in the floor-cap `NormalizeUnion` slot — \
         the all-valid eviction drops the LRU front (100), never the \
         just-admitted candidate (bounded occupancy + always-admit). \
         Got {normalize_gens:?}."
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
    let host = host();
    let graph = host.project_type_store().semantic_graph();

    let canonical_keyed = "/multi_candidate/reverse_index/owner.ts";
    let canonical_a = "/multi_candidate/reverse_index/a.ts";
    let canonical_b = "/multi_candidate/reverse_index/b.ts";
    let canonical_c = "/multi_candidate/reverse_index/c.ts";

    // Identity mode — no backfills — so each publish populates
    // EXACTLY one slot. Lets the test count registrations cleanly.
    let key = instantiate_identity_key(&host, canonical_keyed, "Owner");

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

    let value_a_c = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let value_b_c = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

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

/// **Bounded-retention eviction order: invalid-first, then
/// LRU-by-valid-hit.**
///
/// At the family slot cap, eviction picks (1) a candidate whose fact
/// rail is INVALID against the publishing caller's stable store view —
/// even when that candidate is the MOST RECENTLY admitted (back of the
/// LRU order) — ahead of (2) the front of the LRU order (the
/// least-recently validated-hit candidate) when every candidate still
/// validates. A validated warm hit PROMOTES its candidate to the back,
/// making it the freshest. A same-discriminant re-publish replaces in
/// place and becomes freshest. Every publish is ALWAYS admitted after
/// local eviction, the slot stays bounded at the family cap, and every
/// displaced candidate's per-`admission_seq` reverse-index
/// registrations are drained (orphan-free), whatever its position.
///
/// RED against the legacy uniform front-removal (FIFO): the at-cap
/// publish evicts the front (generation 100), not the invalid back
/// candidate (107). RED with valid-hit promotion disabled: the
/// promoted candidate (100) stays at the front and is evicted by the
/// next at-cap publish.
#[test]
fn family_eviction_prefers_invalid_then_lru_valid_hit() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();

    let canonical_keyed = "/multi_candidate/retention/owner.ts";
    let dep = "/multi_candidate/retention/dep.ts";
    upsert(
        &host,
        canonical_keyed,
        "export type Owner = { x: number };\n",
    );
    upsert(&host, dep, "export const dep: number = 1;\n");

    let key = instantiate_identity_key(&host, canonical_keyed, "Owner");

    let value = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let self_roots_empty: Arc<[Arc<str>]> = Arc::from(Vec::<Arc<str>>::new());
    let empty_dispatch: DepSignature = Arc::from(Vec::new());
    // The INVALID candidate's fact rail: a `FileWholeHash` for the
    // TRACKED dep file whose hash does not match the live view — the
    // candidate can never validate against the publishing caller's
    // stable store view, but stays resident (nothing called
    // `invalidate_canonical`). Its reverse-index registration under
    // `dep` lets the guard assert orphan-free cleanup of a NON-front
    // victim.
    let stale_facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: dep.to_string(),
        hash: [0xEEu8; 16],
    }]);

    let publish = |generation: u64, facts: &Arc<[FactVersionRef]>| {
        let signature = if facts.is_empty() {
            ReadSetSignature::empty()
        } else {
            ReadSetSignature::new(Arc::clone(facts))
        };
        graph.publish_with_view_for_tests(
            &host,
            key.clone(),
            QueryResult::Value(value),
            signature,
            Arc::clone(&self_roots_empty),
            Arc::clone(&empty_dispatch),
            generation,
        );
    };
    let no_facts: Arc<[FactVersionRef]> = Arc::from(Vec::new());

    // Fill the cap-8 `Instantiate` slot: seven always-valid candidates
    // (empty fact rails) generations 100..=106, then the INVALID
    // candidate 107 LAST — the most-recently admitted, at the BACK of
    // the LRU order.
    for generation in 100..=106u64 {
        publish(generation, &no_facts);
    }
    publish(107, &stale_facts);
    let filled = graph.slot_candidate_generations_for_tests(&key);
    assert_eq!(
        filled,
        vec![100, 101, 102, 103, 104, 105, 106, 107],
        "fixture invariant: the cap-8 slot holds all eight candidates in \
         admission order. Got {filled:?}."
    );
    let registrations_before = graph.canonical_to_entries_count(dep);
    assert_eq!(
        registrations_before, 1,
        "fixture invariant: the invalid candidate registers exactly once \
         under its dep canonical. Got {registrations_before}."
    );

    // At-cap publish of a NEW valid discriminant (108): the victim must
    // be the INVALID candidate 107 — NOT the valid LRU front 100,
    // although 107 is the most-recently admitted. 108 is ALWAYS
    // admitted; the slot stays bounded at 8; 107's reverse-index
    // registration is drained (orphan-free non-front victim).
    publish(108, &no_facts);
    let after_invalid_eviction = graph.slot_candidate_generations_for_tests(&key);
    assert_eq!(
        after_invalid_eviction,
        vec![100, 101, 102, 103, 104, 105, 106, 108],
        "INVALID-FIRST EVICTION: the at-cap publish must evict the \
         INVALID candidate 107 even though it is the MOST RECENTLY \
         admitted (back), keeping the valid LRU front 100 and always \
         admitting 108. Legacy unconditional front removal (FIFO) \
         evicts 100 instead. Got {after_invalid_eviction:?}."
    );
    let registrations_after = graph.canonical_to_entries_count(dep);
    assert_eq!(
        registrations_after, 0,
        "ORPHAN-FREE CLEANUP: the evicted invalid candidate's \
         reverse-index registration under its dep canonical must be \
         drained by its own `admission_seq`, whatever the victim's \
         position in the slot. Got {registrations_after}."
    );

    // Valid-hit promotion: a validated warm read hits the first
    // satisfying + valid candidate — the LRU front 100 — and PROMOTES
    // it to the back (freshest).
    assert!(
        graph.get_validated_with_host_for_tests(&key, &host),
        "fixture invariant: the warm read must hit (all eight \
         candidates are valid and self-satisfying)."
    );
    let after_hit = graph.slot_candidate_generations_for_tests(&key);
    assert_eq!(
        after_hit,
        vec![101, 102, 103, 104, 105, 106, 108, 100],
        "VALID-HIT PROMOTION: the validated hit on 100 must move it to \
         the back of the LRU order. Got {after_hit:?}."
    );

    // All-valid at-cap publish (109): every remaining candidate
    // validates, so the victim is the LRU FRONT — 101 (100 was just
    // promoted). 109 is always admitted.
    publish(109, &no_facts);
    let after_lru_eviction = graph.slot_candidate_generations_for_tests(&key);
    assert_eq!(
        after_lru_eviction,
        vec![102, 103, 104, 105, 106, 108, 100, 109],
        "LRU-BY-VALID-HIT EVICTION: with every candidate valid the \
         at-cap publish must evict the least-recently validated-hit \
         candidate 101 — the promoted 100 SURVIVES. With promotion \
         disabled (mutation), 100 is still at the front and is evicted \
         instead. Got {after_lru_eviction:?}."
    );

    // Same-discriminant re-publish (102, empty facts) replaces in
    // place and becomes the freshest — occupancy stays bounded at 8.
    publish(102, &no_facts);
    let after_replace = graph.slot_candidate_generations_for_tests(&key);
    assert_eq!(
        after_replace,
        vec![103, 104, 105, 106, 108, 100, 109, 102],
        "SAME-DISCRIMINANT REPLACEMENT: re-publishing 102's discriminant \
         must replace it in place and make it freshest — no ninth \
         candidate, no eviction. Got {after_replace:?}."
    );
}
