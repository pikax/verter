//! Guards for the `FlowNarrowingAt` + `ContextualTypeAt` key surface.
//!
//! These tests pin the IDENTITY contract of the two program-analysis
//! [`SemanticQueryKey`] variants `FlowNarrowingAt` and `ContextualTypeAt`,
//! their env-in-context dimensions (these keys have NO slot, so their FULL
//! R21 `P R T L J` env dims ride INSIDE the [`ProgramAnalysisContext`]), the
//! HONEST-PENDING behaviour of BOTH (non-producing — the flow engine and the
//! contextual-typing engine are not yet wired, so there is no execute-side
//! reducer), and the value-domain mapping (both resolve to the
//! `ProgramAnalysis` value domain, NEVER `TypeNode`).
//!
//! Identity is probed BEHAVIORALLY through the family memo exactly as the
//! sibling key-surface guards do: publishing a synthetic candidate under
//! key `a` and then reading `slot_candidate_count_for_tests(b)` is `> 0` iff
//! `a` and `b` project to the SAME `(FamilyKey, ModeSlot)`. A warm entry
//! under one identity is returned for another ONLY when they share a slot.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::query_key_spec::semantic_query_key_specs;
use verter_session::semantic_query::{
    ContextualTypingKey, FlowNarrowingKey, PrimitiveKind, ProgramAnalysisContext, ProgramPointId,
    QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryKeyTag, SemanticQueryValueTag, SubstitutionCanonicalHash,
};
use verter_session::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// Publish a synthetic candidate under `a`, then return the candidate
/// count `b` projects to. `> 0` ⟺ `a` and `b` share a `(FamilyKey, slot)`.
///
/// A FRESH host per call keeps every pair independent. Both new keys carry
/// no projection mode (`ModeSlot::Single`), so backfill — which fans out
/// only along the mode hierarchy — never muddies the probe.
fn count_for_b_after_publishing_a(a: &SemanticQueryKey, b: &SemanticQueryKey) -> usize {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    graph.publish_with_carrier_dispatch_and_generation_for_tests(
        a.clone(),
        QueryResult::Value(node),
        ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
        Arc::from(Vec::new().into_boxed_slice()),
        100,
    );
    graph.slot_candidate_count_for_tests(b)
}

/// `a` and `b` are NON-equal keys AND project to DISTINCT `(FamilyKey,
/// slot)`. Also asserts the positive sanity (`a` reaches its own slot) so
/// the probe is not vacuously passing on a broken publish path.
fn assert_distinct_identity(a: &SemanticQueryKey, b: &SemanticQueryKey) {
    assert_ne!(a, b, "keys must be non-equal");
    assert_eq!(
        count_for_b_after_publishing_a(a, a),
        1,
        "sanity: publishing `a` must reach `a`'s own slot (count 1)"
    );
    assert_eq!(
        count_for_b_after_publishing_a(a, b),
        0,
        "a warm candidate published under `a` must NOT be reachable from \
         `b` — they must project to DISTINCT (FamilyKey, slot)"
    );
}

// ---------------------------------------------------------------------------
// Key constructors.
// ---------------------------------------------------------------------------

fn point(canonical: &str, offset: u32) -> ProgramPointId {
    ProgramPointId {
        canonical_id: Arc::from(canonical),
        offset,
    }
}

fn analysis_context(p: u8, r: u8, t: u8, l: u8, j: u32) -> ProgramAnalysisContext {
    analysis_context_with_subst(p, r, t, l, j, SubstitutionCanonicalHash::empty())
}

#[allow(clippy::too_many_arguments)]
fn analysis_context_with_subst(
    p: u8,
    r: u8,
    t: u8,
    l: u8,
    j: u32,
    substitution: SubstitutionCanonicalHash,
) -> ProgramAnalysisContext {
    ProgramAnalysisContext {
        parse_env_hash: hash16(p),
        resolve_env_hash: hash16(r),
        type_env_hash: hash16(t),
        lib_env_hash: hash16(l),
        project_identity: j,
        substitution,
    }
}

fn flow_narrowing_key(
    canonical: &str,
    offset: u32,
    p: u8,
    r: u8,
    t: u8,
    l: u8,
    j: u32,
) -> SemanticQueryKey {
    SemanticQueryKey::FlowNarrowingAt {
        point: point(canonical, offset),
        flow: FlowNarrowingKey::empty(),
        context: analysis_context(p, r, t, l, j),
    }
}

fn contextual_type_key(
    canonical: &str,
    offset: u32,
    p: u8,
    r: u8,
    t: u8,
    l: u8,
    j: u32,
) -> SemanticQueryKey {
    SemanticQueryKey::ContextualTypeAt {
        point: point(canonical, offset),
        contextual: ContextualTypingKey::empty(),
        context: analysis_context(p, r, t, l, j),
    }
}

/// A `FlowNarrowingAt` key with an explicit `flow` demand axis (all env at 0).
fn flow_narrowing_key_with_flow(
    canonical: &str,
    offset: u32,
    flow: FlowNarrowingKey,
) -> SemanticQueryKey {
    SemanticQueryKey::FlowNarrowingAt {
        point: point(canonical, offset),
        flow,
        context: analysis_context(0, 0, 0, 0, 0),
    }
}

/// A `FlowNarrowingAt` key with an explicit `substitution` axis (all env / flow
/// at default).
fn flow_narrowing_key_with_subst(
    canonical: &str,
    offset: u32,
    substitution: SubstitutionCanonicalHash,
) -> SemanticQueryKey {
    SemanticQueryKey::FlowNarrowingAt {
        point: point(canonical, offset),
        flow: FlowNarrowingKey::empty(),
        context: analysis_context_with_subst(0, 0, 0, 0, 0, substitution),
    }
}

/// A `ContextualTypeAt` key with an explicit `contextual` demand axis.
fn contextual_type_key_with_contextual(
    canonical: &str,
    offset: u32,
    contextual: ContextualTypingKey,
) -> SemanticQueryKey {
    SemanticQueryKey::ContextualTypeAt {
        point: point(canonical, offset),
        contextual,
        context: analysis_context(0, 0, 0, 0, 0),
    }
}

/// A `ContextualTypeAt` key with an explicit `substitution` axis.
fn contextual_type_key_with_subst(
    canonical: &str,
    offset: u32,
    substitution: SubstitutionCanonicalHash,
) -> SemanticQueryKey {
    SemanticQueryKey::ContextualTypeAt {
        point: point(canonical, offset),
        contextual: ContextualTypingKey::empty(),
        context: analysis_context_with_subst(0, 0, 0, 0, 0, substitution),
    }
}

/// A non-empty interned node set, for varying the `flow` / `contextual` axes.
fn node_set(id: u64) -> Arc<[SemanticNodeId]> {
    Arc::from(vec![SemanticNodeId(id)].into_boxed_slice())
}

// ---------------------------------------------------------------------------
// (1) FlowNarrowingAt identity covers the FULL P/R/T/L/J env (carried IN the
//     context, NOT a slot) plus the ProgramPointId (canonical_id + offset).
// ---------------------------------------------------------------------------

#[test]
fn flow_narrowing_at_key_covers_full_env_and_point() {
    let base = flow_narrowing_key("a.ts", 0, 0, 0, 0, 0, 0);

    // Every one of the FULL P R T L J env dims is part of identity — program
    // analysis is the widest-env operation in the key surface.
    // (arg order is P, R, T, L, J).
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 9, 0, 0, 0, 0));
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 0, 9, 0, 0, 0));
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 0, 0, 9, 0, 0));
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 0, 0, 0, 9, 0));
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 0, 0, 0, 0, 9));
    // The ProgramPointId is part of identity — both the canonical file and
    // the offset within it.
    assert_distinct_identity(&base, &flow_narrowing_key("b.ts", 0, 0, 0, 0, 0, 0));
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 7, 0, 0, 0, 0, 0));
}

// ---------------------------------------------------------------------------
// (2) ContextualTypeAt identity covers the FULL env + point, AND is DISTINCT
//     from a FlowNarrowingAt at the same point/env (the two analyses never
//     share a slot).
// ---------------------------------------------------------------------------

#[test]
fn contextual_type_at_key_covers_full_env_and_point() {
    let base = contextual_type_key("a.ts", 0, 0, 0, 0, 0, 0);

    assert_distinct_identity(&base, &contextual_type_key("a.ts", 0, 9, 0, 0, 0, 0)); // P
    assert_distinct_identity(&base, &contextual_type_key("a.ts", 0, 0, 9, 0, 0, 0)); // R
    assert_distinct_identity(&base, &contextual_type_key("a.ts", 0, 0, 0, 9, 0, 0)); // T
    assert_distinct_identity(&base, &contextual_type_key("a.ts", 0, 0, 0, 0, 9, 0)); // L
    assert_distinct_identity(&base, &contextual_type_key("a.ts", 0, 0, 0, 0, 0, 9)); // J
    assert_distinct_identity(&base, &contextual_type_key("b.ts", 0, 0, 0, 0, 0, 0));
    assert_distinct_identity(&base, &contextual_type_key("a.ts", 7, 0, 0, 0, 0, 0));

    // CROSS-KEY NEGATIVE: a FlowNarrowingAt and a ContextualTypeAt at the
    // SAME program point + SAME env are DISTINCT queries (one asks for the
    // narrowed type, the other for the expected type) and MUST NOT collide.
    assert_distinct_identity(&base, &flow_narrowing_key("a.ts", 0, 0, 0, 0, 0, 0));
}

// ---------------------------------------------------------------------------
// (3) Do-not-warm-hit across an env boundary, for each key.
// ---------------------------------------------------------------------------

#[test]
fn flow_narrowing_at_do_not_warm_hit() {
    // Vary parse_env (P) — the dimension a structural reducer would NOT
    // carry; program analysis does, so this must miss.
    let env_a = flow_narrowing_key("a.ts", 0, 0, 0, 0, 0, 0);
    let env_b = flow_narrowing_key("a.ts", 0, 1, 0, 0, 0, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "FlowNarrowingAt must not warm-hit across a parse_env boundary"
    );
}

#[test]
fn contextual_type_at_do_not_warm_hit() {
    let env_a = contextual_type_key("a.ts", 0, 0, 0, 0, 0, 0);
    let env_b = contextual_type_key("a.ts", 0, 1, 0, 0, 0, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ContextualTypeAt must not warm-hit across a parse_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (3c) Per-variant demand-axis identity — the FULL-planned-identity axes the
//      shipped reduced context dropped. Each axis INDEPENDENTLY changes
//      family_and_slot's output: two keys differing ONLY in `flow` /
//      `contextual` / `substitution` (env held identical) MUST occupy DISTINCT
//      (FamilyKey, slot). A context / FamilyKey that omits the axis collapses
//      them → count 1 → FAIL. These are the discriminating negatives that pin
//      the forward-declared flow / contextual / substitution axes — the
//      committed guard only varied parse_env and could not catch a dropped
//      flow / contextual / subst axis.
// ---------------------------------------------------------------------------

#[test]
fn flow_narrowing_at_identity_covers_flow_axis() {
    // Differ ONLY in the per-variant `flow` demand axis (env all 0).
    let base = flow_narrowing_key_with_flow("a.ts", 0, FlowNarrowingKey::empty());
    let other = flow_narrowing_key_with_flow("a.ts", 0, FlowNarrowingKey::new(node_set(7)));
    assert_distinct_identity(&base, &other);
}

#[test]
fn flow_narrowing_at_identity_covers_substitution_axis() {
    // Differ ONLY in the shared `substitution` axis (env all 0, flow empty).
    let base = flow_narrowing_key_with_subst("a.ts", 0, SubstitutionCanonicalHash::empty());
    let other = flow_narrowing_key_with_subst("a.ts", 0, SubstitutionCanonicalHash(hash16(9)));
    assert_distinct_identity(&base, &other);
}

#[test]
fn contextual_type_at_identity_covers_contextual_axis() {
    // Differ ONLY in the per-variant `contextual` demand axis (env all 0).
    let base = contextual_type_key_with_contextual("a.ts", 0, ContextualTypingKey::empty());
    let other =
        contextual_type_key_with_contextual("a.ts", 0, ContextualTypingKey::new(node_set(7)));
    assert_distinct_identity(&base, &other);
}

#[test]
fn contextual_type_at_identity_covers_substitution_axis() {
    let base = contextual_type_key_with_subst("a.ts", 0, SubstitutionCanonicalHash::empty());
    let other = contextual_type_key_with_subst("a.ts", 0, SubstitutionCanonicalHash(hash16(9)));
    assert_distinct_identity(&base, &other);
}

// ---------------------------------------------------------------------------
// (3d) Node-set demand axes are ORDER-INSENSITIVE SET identities. The
//      `FlowNarrowingKey` / `ContextualTypingKey` constructors canonicalize
//      (sort + dedup), so `[a, b]` and `[b, a]` are the SAME key — they MUST
//      land in the SAME (FamilyKey, slot) — while a genuinely different set
//      (`[a, c]`) stays DISTINCT, and duplicates collapse. These FAIL against
//      the pre-fix order-sensitive `Arc` derive (which would treat `[a, b]`
//      and `[b, a]` as two distinct keys → two slots).
// ---------------------------------------------------------------------------

/// A two-element interned node set in the given order.
fn node_set2(a: u64, b: u64) -> Arc<[SemanticNodeId]> {
    Arc::from(vec![SemanticNodeId(a), SemanticNodeId(b)].into_boxed_slice())
}

#[test]
fn flow_narrowing_key_is_order_insensitive_set() {
    let ab = FlowNarrowingKey::new(node_set2(3, 8));
    let ba = FlowNarrowingKey::new(node_set2(8, 3));
    assert_eq!(ab, ba, "FlowNarrowingKey must be an order-insensitive set");
    assert_eq!(
        ab.ids(),
        ba.ids(),
        "canonical ids must agree across orderings"
    );

    // The canonicalization must reach the full SemanticQueryKey identity too.
    let key_ab = flow_narrowing_key_with_flow("a.ts", 0, ab);
    let key_ba = flow_narrowing_key_with_flow("a.ts", 0, ba);
    assert_eq!(
        key_ab, key_ba,
        "two FlowNarrowingAt keys differing only in flow-set ORDER must be equal"
    );
    assert_eq!(
        count_for_b_after_publishing_a(&key_ab, &key_ba),
        1,
        "a candidate published under the `[a, b]` ordering must be reachable \
         from the `[b, a]` ordering — they are the SAME memo slot"
    );

    // A genuinely DIFFERENT set must stay distinct.
    let key_ac = flow_narrowing_key_with_flow("a.ts", 0, FlowNarrowingKey::new(node_set2(3, 9)));
    assert_distinct_identity(&key_ab, &key_ac);

    // Duplicates collapse — the set carries each id once.
    let dup = FlowNarrowingKey::new(Arc::from(
        vec![SemanticNodeId(5), SemanticNodeId(5), SemanticNodeId(2)].into_boxed_slice(),
    ));
    assert_eq!(dup.ids(), &[SemanticNodeId(2), SemanticNodeId(5)]);
}

#[test]
fn contextual_type_key_is_order_insensitive_set() {
    let ab = ContextualTypingKey::new(node_set2(3, 8));
    let ba = ContextualTypingKey::new(node_set2(8, 3));
    assert_eq!(
        ab, ba,
        "ContextualTypingKey must be an order-insensitive set"
    );
    assert_eq!(
        ab.ids(),
        ba.ids(),
        "canonical ids must agree across orderings"
    );

    let key_ab = contextual_type_key_with_contextual("a.ts", 0, ab);
    let key_ba = contextual_type_key_with_contextual("a.ts", 0, ba);
    assert_eq!(
        key_ab, key_ba,
        "two ContextualTypeAt keys differing only in contextual-set ORDER must be equal"
    );
    assert_eq!(
        count_for_b_after_publishing_a(&key_ab, &key_ba),
        1,
        "a candidate published under the `[a, b]` ordering must be reachable \
         from the `[b, a]` ordering — they are the SAME memo slot"
    );

    let key_ac =
        contextual_type_key_with_contextual("a.ts", 0, ContextualTypingKey::new(node_set2(3, 9)));
    assert_distinct_identity(&key_ab, &key_ac);

    let dup = ContextualTypingKey::new(Arc::from(
        vec![SemanticNodeId(5), SemanticNodeId(5), SemanticNodeId(2)].into_boxed_slice(),
    ));
    assert_eq!(dup.ids(), &[SemanticNodeId(2), SemanticNodeId(5)]);
}

// ---------------------------------------------------------------------------
// (4) HONEST-PENDING discriminator — both execute arms are non-producing:
//     they return Error(Miss) and admit NOTHING. FAILS if a fake producer is
//     ever wired (a Value result, or an admitted candidate).
// ---------------------------------------------------------------------------

#[test]
fn flow_narrowing_at_execute_is_non_producing_miss() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = flow_narrowing_key("a.ts", 0, 0, 0, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "non-producing FlowNarrowingAt execute arm must return Error(Miss), got {result:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-producing FlowNarrowingAt execute arm must admit NOTHING into the shared memo"
    );
}

#[test]
fn contextual_type_at_execute_is_non_producing_miss() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = contextual_type_key("a.ts", 0, 0, 0, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "non-producing ContextualTypeAt execute arm must return Error(Miss), got {result:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-producing ContextualTypeAt execute arm must admit NOTHING into the shared memo"
    );
}

// ---------------------------------------------------------------------------
// (5) VALUE-DOMAIN mapping — both keys map to `ProgramAnalysis`, NOT
//     `TypeNode`. Reads the authoritative spec table directly. It FAILS if
//     either row drifts to `TypeNode` (the easy default) or any other domain.
// ---------------------------------------------------------------------------

#[test]
fn flow_contextual_keys_return_program_analysis_value() {
    let specs = semantic_query_key_specs();
    for variant in [
        SemanticQueryKeyTag::FlowNarrowingAt,
        SemanticQueryKeyTag::ContextualTypeAt,
    ] {
        let row = specs
            .iter()
            .find(|s| s.variant == variant)
            .unwrap_or_else(|| panic!("missing spec row for {variant:?}"));
        assert_eq!(
            row.value_domain,
            SemanticQueryValueTag::ProgramAnalysis,
            "{variant:?} must map to the ProgramAnalysis value domain, not {:?}",
            row.value_domain
        );
        // NEGATIVE: explicitly NOT the easy `TypeNode` default.
        assert_ne!(
            row.value_domain,
            SemanticQueryValueTag::TypeNode,
            "{variant:?} must NOT carry the TypeNode value domain"
        );
    }
}

// ---------------------------------------------------------------------------
// (6) Every key maps to EXACTLY ONE value domain — the spec table is a
//     total function from variant → value domain, with each new program-
//     analysis key landing on `ProgramAnalysis`. Discriminating: FAILS if a
//     variant has zero or duplicate rows, or if the two new keys' domain is
//     wrong.
// ---------------------------------------------------------------------------

#[test]
fn every_semantic_query_key_maps_to_exactly_one_value_domain() {
    let specs = semantic_query_key_specs();
    for tag in SemanticQueryKeyTag::ALL {
        let rows: Vec<_> = specs.iter().filter(|s| s.variant == *tag).collect();
        assert_eq!(
            rows.len(),
            1,
            "variant {tag:?} must have EXACTLY ONE spec row (→ exactly one value domain), found {}",
            rows.len()
        );
    }
    // And the two program-analysis keys specifically land on ProgramAnalysis.
    let domain = |tag: SemanticQueryKeyTag| {
        specs
            .iter()
            .find(|s| s.variant == tag)
            .map(|s| s.value_domain)
            .unwrap()
    };
    assert_eq!(
        domain(SemanticQueryKeyTag::FlowNarrowingAt),
        SemanticQueryValueTag::ProgramAnalysis
    );
    assert_eq!(
        domain(SemanticQueryKeyTag::ContextualTypeAt),
        SemanticQueryValueTag::ProgramAnalysis
    );
}
