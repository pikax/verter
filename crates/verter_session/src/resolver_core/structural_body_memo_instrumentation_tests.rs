//! Discriminating unit test for the Option-A structural-body-memo instrumentation
//! counters. The LOAD-BEARING discriminator is
//! [`bucket_index_distinct_for_each_context`]: it asserts
//! [`context_bucket_index`] maps each of the 6 `(provenance, merge_role)` pairs to
//! a DISTINCT index — a colliding or constant index fn FAILS it. The remaining
//! assertions drive the REAL memo `get`/`insert` and assert the dump reflects the
//! exact values, proving the counters are real atomics (not stubs). Every counter
//! exercised here has a live `get`/`insert` bump site.
//!
//! The counters are process-global statics; under the in-process test surface
//! multiple tests share a process. These tests serialize on
//! [`lock_counter_test_gate`] and [`reset_structural_body_memo_instrumentation`]
//! at the top so the counter-VALUE assertions are not raced by a concurrent test
//! mutating the same statics. (The pure-arithmetic
//! [`bucket_index_distinct_for_each_context`] reads no shared mutable state, but
//! takes the lock too for uniformity.)

use std::sync::Arc;

use super::{
    context_bucket_index, dump_structural_body_memo_instrumentation, lock_counter_test_gate,
    reset_structural_body_memo_instrumentation, CONTEXT_BUCKET_COUNT,
    STRUCTURAL_BODY_MEMO_CELLS_CREATED, STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS,
    STRUCTURAL_BODY_MEMO_HITS, STRUCTURAL_BODY_MEMO_LOOKUPS, STRUCTURAL_BODY_MEMO_MISSES,
};
use crate::resolver_core::structural_body_memo::{
    HotStructuralBodyCell, StructuralBodyDescriptor, StructuralBodyKind, StructuralBodyMemo,
    StructuralBodyMemoKey, StructuralBodyRegistry, StructuralBodySpace,
};
use crate::semantic_query::{
    HotTypeRef, MemberMergeRole, SemanticNodeId, SurfaceProvenanceContext,
};

/// All 6 `(provenance, merge_role)` contexts — the full 2 × 3 cross-product.
const ALL_CONTEXTS: [(SurfaceProvenanceContext, MemberMergeRole); CONTEXT_BUCKET_COUNT] = [
    (
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::Authored,
    ),
    (
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::OwnBody,
    ),
    (
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::Heritage,
    ),
    (
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
        MemberMergeRole::Authored,
    ),
    (
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
        MemberMergeRole::OwnBody,
    ),
    (
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
        MemberMergeRole::Heritage,
    ),
];

fn type_semantic_descriptor(name: &str) -> StructuralBodyDescriptor {
    StructuralBodyDescriptor {
        symbol_name: Arc::from(name),
        space: StructuralBodySpace::Type,
        body_kind: StructuralBodyKind::Semantic,
        local_scope: None,
    }
}

fn cell(n: u64) -> Arc<HotStructuralBodyCell> {
    Arc::new(HotStructuralBodyCell::new(HotTypeRef::new(SemanticNodeId(
        n,
    ))))
}

// -- Assertion 1 (THE LOAD-BEARING DISCRIMINATOR). --------------------------
// `context_bucket_index` maps each of the 6 `(provenance, merge_role)` pairs to
// a DISTINCT index in `0..6`. A colliding index fn (e.g.
// `provenance_ord + merge_role_ord`, which maps `(MacroTypeArgOwnBody, Authored)`
// = 1 + 0 and `(Structural, OwnBody)` = 0 + 1 BOTH to 1) FAILS this — the set
// would have < 6 elements. A constant fn FAILS even harder (set size 1). Modelled
// on `kind_index_for_key_distinct_for_each_variant`.
#[test]
fn bucket_index_distinct_for_each_context() {
    let _gate = lock_counter_test_gate();

    let mut seen = std::collections::HashSet::new();
    for (provenance, merge_role) in ALL_CONTEXTS {
        let idx = context_bucket_index(provenance, merge_role);
        assert!(
            idx < CONTEXT_BUCKET_COUNT,
            "context_bucket_index({provenance:?}, {merge_role:?}) = {idx} must be < \
             CONTEXT_BUCKET_COUNT ({CONTEXT_BUCKET_COUNT})"
        );
        assert!(
            seen.insert(idx),
            "context_bucket_index collided: {provenance:?} × {merge_role:?} mapped to bucket \
             {idx}, already produced by an earlier context — the index fn is not injective"
        );
    }
    assert_eq!(
        seen.len(),
        CONTEXT_BUCKET_COUNT,
        "all {CONTEXT_BUCKET_COUNT} contexts must map to {CONTEXT_BUCKET_COUNT} DISTINCT buckets; \
         got {} distinct (a colliding/constant index fn fails here)",
        seen.len()
    );
}

// -- Assertion 2: counters reflect the real get/insert bumps. ---------------
// A `get` miss bumps LOOKUPS + MISSES (not HITS); an `insert` bumps
// CELLS_CREATED + the right context bucket; a `get` HIT (after the insert) bumps
// LOOKUPS + HITS (not MISSES). Drives the REAL memo methods so the wiring is
// exercised end to end.
#[test]
fn counters_reflect_real_get_and_insert_bumps() {
    let _gate = lock_counter_test_gate();
    reset_structural_body_memo_instrumentation();

    let mut registry = StructuralBodyRegistry::new();
    let slot = registry.register(type_semantic_descriptor("Props"));
    let mut memo = StructuralBodyMemo::new();

    let provenance = SurfaceProvenanceContext::MacroTypeArgOwnBody;
    let merge_role = MemberMergeRole::OwnBody;
    let key = StructuralBodyMemoKey::new(slot, provenance, merge_role);

    // (a) A `get` MISS before any insert: LOOKUPS=1, MISSES=1, HITS=0.
    assert!(
        memo.get(&key).is_none(),
        "the cold get must miss (nothing inserted yet)"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_LOOKUPS.load(RELAXED),
        1,
        "miss bumps LOOKUPS"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_MISSES.load(RELAXED),
        1,
        "a miss bumps MISSES"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_HITS.load(RELAXED),
        0,
        "a miss must NOT bump HITS"
    );

    // (b) An `insert`: CELLS_CREATED=1, the right context bucket=1.
    memo.insert(key, cell(100));
    assert_eq!(
        STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(RELAXED),
        1,
        "an insert bumps CELLS_CREATED"
    );
    let expected_bucket = context_bucket_index(provenance, merge_role);
    assert_eq!(
        STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS[expected_bucket].load(RELAXED),
        1,
        "an insert bumps the context bucket for its (provenance, merge_role)"
    );

    // (c) A `get` HIT (same key): LOOKUPS=2, HITS=1, MISSES still 1.
    assert!(
        memo.get(&key).is_some(),
        "the warm get must hit (the cell was inserted)"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_LOOKUPS.load(RELAXED),
        2,
        "the second get bumps LOOKUPS to 2"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_HITS.load(RELAXED),
        1,
        "the warm get bumps HITS to 1"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_MISSES.load(RELAXED),
        1,
        "the warm get must NOT bump MISSES (still 1 from the earlier miss)"
    );
}

// -- Assertion 3: per-bucket attribution + distinct-only counting. ----------
// Insert cells under 2 DIFFERENT contexts of the SAME slot; the 2 corresponding
// buckets each read 1 and the other 4 read 0 — the buckets attribute distinct
// contexts to distinct slots. THEN re-insert the SAME context key (a duplicate):
// CELLS_CREATED and the bucket must NOT re-bump (distinct-only counting). This
// DISCRIMINATES the pre-fix bug where the bump happened BEFORE the insert result
// was known and a duplicate over-counted.
#[test]
fn context_buckets_attribute_distinct_contexts() {
    let _gate = lock_counter_test_gate();
    reset_structural_body_memo_instrumentation();

    let mut registry = StructuralBodyRegistry::new();
    let slot = registry.register(type_semantic_descriptor("Props"));
    let mut memo = StructuralBodyMemo::new();

    // Two distinct contexts of the SAME slot.
    let ctx_a = (
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::Heritage,
    );
    let ctx_b = (
        SurfaceProvenanceContext::MacroTypeArgOwnBody,
        MemberMergeRole::OwnBody,
    );
    let idx_a = context_bucket_index(ctx_a.0, ctx_a.1);
    let idx_b = context_bucket_index(ctx_b.0, ctx_b.1);
    assert_ne!(
        idx_a, idx_b,
        "the two contexts must map to different buckets"
    );

    memo.insert(StructuralBodyMemoKey::new(slot, ctx_a.0, ctx_a.1), cell(1));
    memo.insert(StructuralBodyMemoKey::new(slot, ctx_b.0, ctx_b.1), cell(2));

    for (idx, bucket) in STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS.iter().enumerate() {
        let expected = if idx == idx_a || idx == idx_b { 1 } else { 0 };
        assert_eq!(
            bucket.load(RELAXED),
            expected,
            "bucket {idx} must read {expected} (the two inserted contexts hit buckets {idx_a} \
             and {idx_b}; every other bucket stays 0)"
        );
    }
    assert_eq!(
        STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(RELAXED),
        2,
        "two distinct inserts → CELLS_CREATED == 2"
    );

    // Distinct-only: re-inserting an EXISTING context key (same slot + same
    // provenance + same merge_role) replaces the cell but must NOT re-bump
    // CELLS_CREATED or the bucket — the memo counts DISTINCT context cells, not
    // raw insert calls. (Pre-fix, the bump ran before the insert result was
    // known and this duplicate over-counted to 3.)
    let dup = memo.insert(StructuralBodyMemoKey::new(slot, ctx_a.0, ctx_a.1), cell(99));
    assert!(
        dup.is_some(),
        "re-inserting the same context key must report the replaced cell"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(RELAXED),
        2,
        "a re-insert of an existing context must NOT re-bump CELLS_CREATED (distinct-only)"
    );
    assert_eq!(
        STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS[idx_a].load(RELAXED),
        1,
        "a re-insert of an existing context must NOT re-bump its bucket (distinct-only)"
    );
}

// -- Assertion 4: dump + reset round-trip. ----------------------------------
// `dump_*` contains every counter's value (and the right ones reflect the bumps);
// `reset_*` zeroes them all (a post-reset dump reads all-zero). Every dumped
// counter has a live get/insert bump site — no constant-zero counter.
#[test]
fn dump_contains_every_counter_and_reset_zeroes_all() {
    let _gate = lock_counter_test_gate();
    reset_structural_body_memo_instrumentation();

    // Drive a non-trivial state across every counter family.
    let mut registry = StructuralBodyRegistry::new();
    let slot = registry.register(type_semantic_descriptor("Props"));
    let mut memo = StructuralBodyMemo::new();
    let key = StructuralBodyMemoKey::new(
        slot,
        SurfaceProvenanceContext::Structural,
        MemberMergeRole::Authored,
    );
    let _ = memo.get(&key); // miss → LOOKUPS=1, MISSES=1
    memo.insert(key, cell(7)); // CELLS_CREATED=1, bucket 0 = 1
    let _ = memo.get(&key); // hit → LOOKUPS=2, HITS=1

    let dump = dump_structural_body_memo_instrumentation();
    for key_name in [
        "STRUCTURAL_BODY_MEMO_LOOKUPS",
        "STRUCTURAL_BODY_MEMO_HITS",
        "STRUCTURAL_BODY_MEMO_MISSES",
        "STRUCTURAL_BODY_MEMO_CELLS_CREATED",
        "STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS",
    ] {
        assert!(
            dump.contains(key_name),
            "dump must contain {key_name}; got: {dump}"
        );
    }
    // The bumped values must appear (discriminates a dump that always prints 0).
    assert!(
        dump.contains("\"STRUCTURAL_BODY_MEMO_LOOKUPS\": 2"),
        "dump must reflect LOOKUPS=2; got: {dump}"
    );
    assert!(
        dump.contains("\"STRUCTURAL_BODY_MEMO_HITS\": 1"),
        "dump must reflect HITS=1; got: {dump}"
    );
    assert!(
        dump.contains("\"STRUCTURAL_BODY_MEMO_MISSES\": 1"),
        "dump must reflect MISSES=1; got: {dump}"
    );
    assert!(
        dump.contains("\"STRUCTURAL_BODY_MEMO_CELLS_CREATED\": 1"),
        "dump must reflect CELLS_CREATED=1; got: {dump}"
    );

    // -- reset zeroes EVERY counter. ----------------------------------------
    reset_structural_body_memo_instrumentation();
    assert_eq!(STRUCTURAL_BODY_MEMO_LOOKUPS.load(RELAXED), 0);
    assert_eq!(STRUCTURAL_BODY_MEMO_HITS.load(RELAXED), 0);
    assert_eq!(STRUCTURAL_BODY_MEMO_MISSES.load(RELAXED), 0);
    assert_eq!(STRUCTURAL_BODY_MEMO_CELLS_CREATED.load(RELAXED), 0);
    for (idx, bucket) in STRUCTURAL_BODY_MEMO_CONTEXT_BUCKETS.iter().enumerate() {
        assert_eq!(
            bucket.load(RELAXED),
            0,
            "reset must zero context bucket {idx}"
        );
    }
    // A post-reset dump reads all-zero (no residual counter prints non-zero).
    let post = dump_structural_body_memo_instrumentation();
    assert!(
        post.contains("\"STRUCTURAL_BODY_MEMO_LOOKUPS\": 0")
            && post.contains("\"STRUCTURAL_BODY_MEMO_HITS\": 0")
            && post.contains("\"STRUCTURAL_BODY_MEMO_MISSES\": 0")
            && post.contains("\"STRUCTURAL_BODY_MEMO_CELLS_CREATED\": 0"),
        "a post-reset dump must read all-zero; got: {post}"
    );
}

use std::sync::atomic::Ordering::Relaxed as RELAXED;
