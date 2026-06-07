//! Guards for the `ApparentType` + `TemplateLiteralReduce` key surface.
//!
//! These tests pin the IDENTITY contract of the two new
//! [`SemanticQueryKey`] variants `ApparentType` and `TemplateLiteralReduce`,
//! their env-in-context dimensions (these keys have NO slot, so their R21
//! env dims ride INSIDE the context struct), the HONEST-PENDING behaviour of
//! `ApparentType` (non-producing — there is no lib-member index yet), and the
//! LIVE concatenation producer of `TemplateLiteralReduce` (which routes
//! through the ONE shared deferred evaluator, never a hand-rolled reducer).
//!
//! Identity is probed BEHAVIORALLY through the family memo exactly as the
//! sibling class/namespace/enum guards do: publishing a synthetic candidate under key `a`
//! and then reading `slot_candidate_count_for_tests(b)` is `> 0` iff `a` and
//! `b` project to the SAME `(FamilyKey, ModeSlot)`. A warm entry under one
//! identity is returned for another ONLY when they share a slot.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::{
    ApparentTypeContext, LiteralValue, PrimitiveKind, QueryError, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, TemplateLiteralReduceContext,
};
use verter_session::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn hash16(byte: u8) -> [u8; 16] {
    [byte; 16]
}

fn dummy_node() -> SemanticNodeId {
    SemanticNodeId(1)
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

fn apparent_type_key(base: SemanticNodeId, t: u8, l: u8, j: u32) -> SemanticQueryKey {
    SemanticQueryKey::ApparentType {
        base,
        context: ApparentTypeContext {
            type_env_hash: hash16(t),
            lib_env_hash: hash16(l),
            project_identity: j,
        },
    }
}

fn template_literal_reduce_key(
    pattern: &[&str],
    args: &[SemanticNodeId],
    r: u8,
    t: u8,
    l: u8,
    j: u32,
) -> SemanticQueryKey {
    let quasis: Arc<[Arc<str>]> = pattern.iter().map(|s| Arc::from(*s)).collect();
    let args: Arc<[SemanticNodeId]> = Arc::from(args.to_vec().into_boxed_slice());
    SemanticQueryKey::TemplateLiteralReduce {
        pattern: quasis,
        args,
        context: TemplateLiteralReduceContext {
            resolve_env_hash: hash16(r),
            type_env_hash: hash16(t),
            lib_env_hash: hash16(l),
            project_identity: j,
        },
    }
}

// ---------------------------------------------------------------------------
// (1) ApparentType identity covers L / T / J (carried IN the context, NOT a
//     slot) plus `base`.
// ---------------------------------------------------------------------------

#[test]
fn apparent_type_key_covers_lib_env_demand_and_context() {
    let base = apparent_type_key(dummy_node(), 0, 0, 0);

    // `lib_env_hash` (L) is part of identity — an apparent surface depends
    // on the lib-member index for primitive→wrapper / lib members.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 0, 9, 0));
    // `type_env_hash` (T) is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 9, 0, 0));
    // `project_identity` (J) is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(dummy_node(), 0, 0, 9));
    // `base` is part of identity.
    assert_distinct_identity(&base, &apparent_type_key(SemanticNodeId(2), 0, 0, 0));
}

// ---------------------------------------------------------------------------
// (2) ApparentType do-not-warm-hit across a lib_env boundary.
// ---------------------------------------------------------------------------

#[test]
fn apparent_type_do_not_warm_hit() {
    let env_a = apparent_type_key(dummy_node(), 0, 0, 0);
    let env_b = apparent_type_key(dummy_node(), 0, 1, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "ApparentType must not warm-hit across a lib_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (3) ApparentType execute is a non-producing, honest Miss that admits
//     nothing — the HONEST-PENDING discriminator (FAILS if a fake producer
//     is ever wired).
// ---------------------------------------------------------------------------

#[test]
fn apparent_type_execute_is_non_producing_miss() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    // A concrete base node — a real primitive the apparent-type surface
    // would project members from once the lib-member index exists.
    let base = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key = apparent_type_key(base, 0, 0, 0);

    let result =
        verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key.clone());
    // (a) honest Miss — discriminates a `Value(TypeNode)` producer.
    assert!(
        matches!(result, QueryResult::Error(QueryError::Miss)),
        "non-producing ApparentType execute arm must return Error(Miss), got {result:?}"
    );
    // (b) admitted / cached NOTHING.
    assert_eq!(
        graph.slot_candidate_count_for_tests(&key),
        0,
        "a non-producing ApparentType execute arm must admit NOTHING into the shared memo"
    );
}

// ---------------------------------------------------------------------------
// (4) TemplateLiteralReduce identity covers R / T / L / J + pattern + args,
//     and args ORDER is significant (concatenation order matters).
// ---------------------------------------------------------------------------

#[test]
fn template_literal_reduce_key_covers_context() {
    let a = dummy_node();
    let b = SemanticNodeId(2);
    let base = template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 0);

    // resolve_env_hash (R) is part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 9, 0, 0, 0),
    );
    // type_env_hash (T).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 9, 0, 0),
    );
    // lib_env_hash (L).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 9, 0),
    );
    // project_identity (J).
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 9),
    );
    // pattern (quasis) is part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "_", ""], &[a, b], 0, 0, 0, 0),
    );
    // args are part of identity.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[a], 0, 0, 0, 0),
    );

    // NEGATIVE: arg ORDER matters — `${a}-${b}` and `${b}-${a}` are
    // DISTINCT concatenations and MUST NOT collide. A reorder/sort applied
    // to `args` (as NormalizeUnion does for its order-insensitive members)
    // would make these share a slot — `assert_distinct_identity` then sees
    // count 1 and FAILS. This is the discriminating negative.
    assert_distinct_identity(
        &base,
        &template_literal_reduce_key(&["", "-", ""], &[b, a], 0, 0, 0, 0),
    );
}

// ---------------------------------------------------------------------------
// (5) TemplateLiteralReduce do-not-warm-hit across a resolve_env boundary.
// ---------------------------------------------------------------------------

#[test]
fn template_literal_reduce_do_not_warm_hit() {
    let a = dummy_node();
    let b = SemanticNodeId(2);
    let env_a = template_literal_reduce_key(&["", "-", ""], &[a, b], 0, 0, 0, 0);
    let env_b = template_literal_reduce_key(&["", "-", ""], &[a, b], 1, 0, 0, 0);
    assert_eq!(
        count_for_b_after_publishing_a(&env_a, &env_b),
        0,
        "TemplateLiteralReduce must not warm-hit across a resolve_env boundary"
    );
}

// ---------------------------------------------------------------------------
// (6) PRODUCE discriminator — the LIVE concatenation producer folds an
//     all-literal template via the ONE shared deferred evaluator, and
//     carrier-stops (returns the shell) when an expression is non-literal.
// ---------------------------------------------------------------------------

#[test]
fn template_literal_reduce_reduces_concrete_concatenation() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();

    // `${"a"}-${"b"}` ⇒ pattern ["", "-", ""], args [Literal("a"), Literal("b")].
    let lit_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "a".to_string(),
    )));
    let lit_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "b".to_string(),
    )));
    let key = template_literal_reduce_key(&["", "-", ""], &[lit_a, lit_b], 0, 0, 0, 0);

    let result = verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key);
    let node = match result {
        QueryResult::Value(out) => out.value,
        other => panic!("TemplateLiteralReduce(all-literal) must produce a Value, got {other:?}"),
    };
    // The fold MUST be the concrete concatenation "a-b" — FAILS against a
    // non-producing (Miss) impl AND against a hand-rolled reducer that
    // concatenates wrongly.
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
            assert_eq!(
                s.as_str(),
                "a-b",
                "all-literal template must fold to the concatenated string literal"
            );
        }
        other => panic!("expected Literal(String(\"a-b\")), got {other:?}"),
    }

    // Carrier-stop NEGATIVE: with one NON-literal arg (a `Primitive(String)`
    // node), the template cannot fold — the result is the deferred
    // `TemplateLiteral` shell, NOT a fabricated literal.
    let prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let key2 = template_literal_reduce_key(&["", "-", ""], &[lit_a, prim], 0, 0, 0, 0);
    let result2 = verter_session::for_tests::dispatch_execute_type_node_for_tests(&host, key2);
    let node2 = match result2 {
        QueryResult::Value(out) => out.value,
        QueryResult::Recursive(n) => n,
        other => panic!("TemplateLiteralReduce(non-literal arg) must return a node, got {other:?}"),
    };
    match graph.node_data(node2).as_deref() {
        Some(SemanticNodeData::TemplateLiteral { .. }) => {}
        other => panic!(
            "non-literal arg must carrier-stop to the TemplateLiteral shell, \
             NOT a fabricated literal; got {other:?}"
        ),
    }
}
