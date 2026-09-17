//! Guards for the two AWAITED-RELATION key surfaces.
//!
//! These pin the IDENTITY contract of [`SemanticQueryKey::AwaitedNormalize`]
//! and [`SemanticQueryKey::AsyncReturnPayload`]: their env-in-context
//! dimensions (neither key has a slot, so the R21 env dims ride INSIDE the
//! shared `StructuralReduceContext`), and — the load-bearing one — that the
//! two relations are SEPARATE families.
//!
//! Why the separation is a correctness property, not naming: measured on
//! tsc 7.0.2 by declaration emit, the two relations disagree on a naked type
//! parameter. `await v` where `v: T` is `Awaited<T>`, while
//! `async f<T>(v: T) { return v }` publishes `Promise<T>`. If one operand
//! under one env projected to one `(FamilyKey, slot)`, an async function's
//! published return could be served from an `await` expression's cached
//! answer, or the reverse — silently, and only for generics.
//!
//! Identity is probed BEHAVIOURALLY through the family memo, exactly as the
//! sibling key-surface guards do: publish a synthetic candidate under key
//! `a`, then `slot_candidate_count_for_tests(b)` is `> 0` iff `a` and `b`
//! project to the SAME `(FamilyKey, ModeSlot)`.

use std::sync::Arc;

use verter_session::for_tests::ReadSetSignature;
use verter_session::semantic_query::{
    PrimitiveKind, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    StructuralReduceContext,
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

/// Publish a synthetic candidate under `a`, then return the candidate count
/// `b` projects to. `> 0` ⟺ `a` and `b` share a `(FamilyKey, slot)`.
///
/// A FRESH host per call keeps every pair independent. Both keys carry no
/// projection mode (`ModeSlot::Single`), so backfill — which fans out only
/// along the mode hierarchy — never muddies the probe.
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

fn context(r: u8, t: u8, l: u8, j: u32) -> StructuralReduceContext {
    StructuralReduceContext {
        resolve_env_hash: hash16(r),
        type_env_hash: hash16(t),
        lib_env_hash: hash16(l),
        project_identity: j,
    }
}

fn normalize_key(operand: SemanticNodeId, r: u8, t: u8, l: u8, j: u32) -> SemanticQueryKey {
    SemanticQueryKey::AwaitedNormalize {
        operand,
        context: context(r, t, l, j),
    }
}

fn payload_key(operand: SemanticNodeId, r: u8, t: u8, l: u8, j: u32) -> SemanticQueryKey {
    SemanticQueryKey::AsyncReturnPayload {
        operand,
        context: context(r, t, l, j),
    }
}

/// Every env axis independently separates the family. Discriminating: if any
/// axis were dropped from the family key, the pair sharing every OTHER axis
/// would collide and the count would be 1.
fn assert_every_env_axis_separates(
    build: impl Fn(SemanticNodeId, u8, u8, u8, u32) -> SemanticQueryKey,
    label: &str,
) {
    let a = dummy_node();
    let base = build(a, 0, 0, 0, 0);
    for (other, axis) in [
        (build(a, 1, 0, 0, 0), "resolve_env"),
        (build(a, 0, 1, 0, 0), "type_env"),
        (build(a, 0, 0, 1, 0), "lib_env"),
        (build(a, 0, 0, 0, 1), "project_identity"),
    ] {
        assert_eq!(
            count_for_b_after_publishing_a(&base, &other),
            0,
            "{label} must not warm-hit across a {axis} boundary",
        );
    }
    // Positive sanity: the probe is not vacuously passing on a broken
    // publish path — the key MUST reach its own slot.
    assert_eq!(
        count_for_b_after_publishing_a(&base, &base),
        1,
        "{label} must reach its own slot (count 1)",
    );
}

#[test]
fn awaited_normalize_do_not_warm_hit() {
    assert_every_env_axis_separates(normalize_key, "AwaitedNormalize");
}

#[test]
fn async_return_payload_do_not_warm_hit() {
    assert_every_env_axis_separates(payload_key, "AsyncReturnPayload");
}

/// THE ARCHITECTURAL PIN: one operand, one env, two relations, two families.
///
/// This is the guard that fails if the two families are ever collapsed back
/// into one reducer behind a disposition flag. Measured on tsc 7.0.2, the
/// two relations answer a naked type parameter differently (`Awaited<T>` for
/// the await/iteration relation, `T` for the async publication relation), so
/// a shared slot would serve one relation's answer for the other.
#[test]
fn the_two_awaited_relations_never_share_a_family() {
    let a = dummy_node();
    let normalize = normalize_key(a, 0, 0, 0, 0);
    let payload = payload_key(a, 0, 0, 0, 0);

    assert_ne!(normalize, payload, "the two keys must not be equal");
    assert_eq!(
        count_for_b_after_publishing_a(&normalize, &normalize),
        1,
        "sanity: AwaitedNormalize must reach its own slot",
    );
    assert_eq!(
        count_for_b_after_publishing_a(&normalize, &payload),
        0,
        "an AwaitedNormalize answer must NOT be reachable from \
         AsyncReturnPayload — the relations disagree on a naked type \
         parameter, so sharing a slot would publish one relation's answer \
         for the other",
    );
    // …and the reverse direction, so neither family can absorb the other.
    assert_eq!(
        count_for_b_after_publishing_a(&payload, &normalize),
        0,
        "an AsyncReturnPayload answer must NOT be reachable from \
         AwaitedNormalize",
    );
}
