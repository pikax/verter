//! U2B.7 guards — the `FlowNarrowingAt` + `ContextualTypeAt` key surface.
//!
//! These tests pin the IDENTITY contract of the two new program-analysis
//! [`SemanticQueryKey`] variants `FlowNarrowingAt` and `ContextualTypeAt`,
//! their env-in-context dimensions (these keys have NO slot, so their FULL
//! R21 `P R T L J` env dims ride INSIDE the [`ProgramAnalysisContext`]), the
//! HONEST-PENDING behaviour of BOTH (non-producing — the flow engine and the
//! contextual-typing engine land in U6, so there is no execute-side reducer),
//! and the value-domain mapping (both resolve to the `ProgramAnalysis` value
//! domain, NEVER `TypeNode`).
//!
//! Identity is probed BEHAVIORALLY through the family memo exactly as the
//! sibling U2B.5 / U2B.6 guards do: publishing a synthetic candidate under
//! key `a` and then reading `slot_candidate_count_for_tests(b)` is `> 0` iff
//! `a` and `b` project to the SAME `(FamilyKey, ModeSlot)`. A warm entry
//! under one identity is returned for another ONLY when they share a slot.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::query_key_spec::semantic_query_key_specs;
use verter_session::semantic_query::{
    PrimitiveKind, ProgramAnalysisContext, ProgramPointId, QueryError, QueryResult,
    SemanticNodeData, SemanticQueryKey, SemanticQueryKeyTag, SemanticQueryValueTag,
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

#[allow(clippy::too_many_arguments)]
fn analysis_context(p: u8, r: u8, t: u8, l: u8, j: u32) -> ProgramAnalysisContext {
    ProgramAnalysisContext {
        parse_env_hash: hash16(p),
        resolve_env_hash: hash16(r),
        type_env_hash: hash16(t),
        lib_env_hash: hash16(l),
        project_identity: j,
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
        context: analysis_context(p, r, t, l, j),
    }
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
//     `TypeNode`. Reads the authoritative spec table directly. This is the
//     guard tightened for U2B.7: it FAILS if either row drifts to `TypeNode`
//     (the easy default) or any other domain.
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
    // And the two U2B.7 keys specifically land on ProgramAnalysis.
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
