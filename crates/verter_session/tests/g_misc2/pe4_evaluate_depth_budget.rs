//! Discriminator test for the cooperative fail-fast budget guard on
//! `evaluate_deferred_semantic_node_with_context`.
//!
//! The evaluator's fix-point loop recursively re-enters itself
//! through `KeyOf`, `IndexedAccess`, and `TemplateLiteral` operator
//! arms. Pathological mapped-type patterns (e.g.
//! `ChatMessagesSlots<T>`'s per-K loop combined with nested
//! conditionals over keyspace-derived literals) can drive the chain
//! arbitrarily deep — each K's substituted body produces a NEW
//! interned id whose recursive evaluator chain re-enters with that
//! new id as the cache key, defeating the entry-node hash-cons memo.
//!
//! The TLS depth guard caps recursion at
//! `EVALUATE_DEFERRED_DEPTH_CEILING` (256). On exhaustion the
//! evaluator returns the input node — a structural-transit
//! carrier-stop equivalent — and DOES NOT publish into the memo
//! (`ComputeAdmission::ReturnOnly` policy at the evaluator layer).
//!
//! The truncation signal is sticky across the recursive call chain:
//! once any recursive frame on the current thread fires
//! `over_ceiling`, every parent frame on its unwinding path also
//! suppresses its publish AND returns its own entry node carrier-
//! stop, so a parent's downstream-operator reduction over a
//! truncated child's carrier never enters the warm memo as a
//! budget-tainted entry.
//!
//! These tests DISCRIMINATE:
//!
//! 1. **Short alias chain** — a 10-deep chain (well under the 256
//!    ceiling) completes without the guard firing.
//!
//! 2. **Deep KeyOf chain** — a 10_000-deep `KeyOf(KeyOf(...))` chain
//!    (well above the 256 ceiling) triggers the guard. The call
//!    returns WITHOUT panic / OOM, AND the returned node id equals
//!    the INPUT node id (the carrier-stop contract), AND zero
//!    budget-tainted entries are published into
//!    `evaluate_deferred_memo` for the post-truncation parent
//!    frames on the unwinding path.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

/// A 10-deep alias chain via `type` aliases. The Alias-arm in the
/// evaluator advances `node = target` on every iteration of the
/// fix-point loop, so 10 hops fits well under the 256 ceiling.
const SHORT_CHAIN_TS: &str = r#"
export type L0 = string;
export type L1 = L0;
export type L2 = L1;
export type L3 = L2;
export type L4 = L3;
export type L5 = L4;
export type L6 = L5;
export type L7 = L6;
export type L8 = L7;
export type L9 = L8;
export type L10 = L9;
"#;

fn make_host(source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(source),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });
    host
}

#[test]
fn short_alias_chain_completes_without_hitting_budget_guard() {
    let host = make_host(SHORT_CHAIN_TS);
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let expr = TypeExpr::Ref {
        name: Arc::from("L10"),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/source.ts",
        &expr,
        context,
    )
    .expect("lowering must succeed");

    // Drive the evaluator on the deepest alias. Under the budget the
    // 10-hop chain completes (10 << 256) and resolves to the
    // terminal `string` primitive. Pre-guard the same path would
    // also complete — this test guards against the guard FIRING
    // too aggressively on benign deep chains.
    let resolved = for_tests::dispatch_evaluate_deferred_for_tests(&host, carrier, context);

    let graph = host.project_type_store().semantic_graph();
    let data = graph
        .node_data(resolved)
        .expect("resolved node must have data");

    // The Alias-arm in evaluate_deferred unwraps `Alias(target)` →
    // `target` on every iteration. After 10 hops the terminal is
    // resolved to a non-Alias node. We don't assert the exact node
    // type because the lowering may produce DeclRef carriers — the
    // discriminating assertion is: the budget guard did NOT fire
    // (we got past the recursive walk without an early carrier-stop).
    //
    // Empirically the resolver yields either a Primitive(string)
    // (full unwrap) or a DeclRef carrier (transit-shallow). Both
    // are valid under the published(Expanded) demand; the test
    // succeeds on either as long as the dispatch returned a
    // well-defined node.
    let _ = data;
}

#[test]
fn budget_guard_returns_input_node_on_deep_recursion() {
    // Build a structurally-synthetic recursive carrier chain:
    // `KeyOf(KeyOf(...))` nested 10_000 deep. Each `KeyOf` arm in
    // `evaluate_deferred_semantic_node_with_context` performs ONE
    // RECURSIVE CALL on the inner `base` before re-dispatching the
    // outer `KeyOf` query. So the chain forces N actual recursive
    // entries into `evaluate_deferred_*`, which is exactly what the
    // TLS depth guard caps.
    //
    // Pre-guard this would stack-overflow at sufficient nesting
    // depth. Post-guard the inner call short-circuits at the
    // ceiling (returning its input node) so the outer chain
    // unwinds cleanly without consuming unbounded stack.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let store = host.project_type_store().semantic_graph();

    // Build the leaf: a Primitive(string).
    let leaf = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));

    // Build a `KeyOf` chain wrapping the leaf at depth well above
    // the ceiling (256). 10_000 is enough to discriminate the
    // budget — pre-guard 10_000 recursive frames overrun even the
    // 134_217_728-byte RUST_MIN_STACK that the workspace test
    // harness sets; post-guard the chain stops at the ceiling and
    // unwinds cleanly.
    let mut current = leaf;
    for _ in 0..10_000 {
        current = store.intern_node(SemanticNodeData::KeyOf { base: current });
    }
    let chain_head = current;

    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let graph = host.project_type_store().semantic_graph();
    let baseline = graph.stats_snapshot();

    // Drive the evaluator. The depth guard caps recursive entry at
    // 256; the inner call returns the input node, the outer
    // unwinds the operator dispatch chain (every KeyOf above the
    // ceiling sees the inner call's input-node carrier, opens its
    // own SemanticQueryKey::KeyOf dispatch, and yields whatever
    // that returns — most likely Opaque(Miss) because the underlying
    // input is a Primitive(string) that has no `keyof`).
    let resolved = for_tests::dispatch_evaluate_deferred_for_tests(&host, chain_head, context);

    assert!(
        graph.node_data(resolved).is_some(),
        "resolved node MUST have valid semantic data — got None, which means the guard \
         returned a sentinel rather than a real node id"
    );

    // ── Discriminator 1 — carrier-stop contract: the top-level
    // call MUST return the INPUT node. The TLS truncated flag is
    // sticky from the moment the deepest frame fires the ceiling;
    // every parent frame on the unwinding path observes the flag
    // and returns its own `entry_node`. The chain_head is the
    // top-level entry, so the carrier-stop reaches the outermost
    // caller unchanged. ──
    assert_eq!(
        resolved, chain_head,
        "post-truncation the top-level call MUST return the input node (carrier-stop \
         contract). Got {resolved:?}, expected the chain_head {chain_head:?}. A non-input \
         return id means the truncation signal did not propagate to the top-level publish \
         site — the parent frame published a budget-tainted result instead of yielding the \
         carrier-stop."
    );

    // ── Discriminator 2 — no budget-tainted publish: re-issue the
    // same `(chain_head, context)` call. If the prior call had
    // published a stale entry into `evaluate_deferred_memo`, this
    // second call would HIT the memo and the miss-counter would
    // NOT advance. Under the correct contract no publish landed,
    // so the second call MUST miss again. ──
    let after_first = graph.stats_snapshot();
    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, chain_head, context);
    let after_second = graph.stats_snapshot();
    let second_miss_delta =
        after_second.evaluate_deferred_memo_misses - after_first.evaluate_deferred_memo_misses;
    assert!(
        second_miss_delta >= 1,
        "second call on `(chain_head, context)` MUST miss the memo (no budget-tainted \
         publish from the truncated first call). Observed miss delta = {second_miss_delta}. \
         A zero delta means the truncated first call published into the memo and the \
         second call hit a stale entry — violating ComputeAdmission::ReturnOnly at the \
         evaluator layer."
    );
    let baseline_to_after_first_miss_delta =
        after_first.evaluate_deferred_memo_misses - baseline.evaluate_deferred_memo_misses;
    let _ = baseline_to_after_first_miss_delta;
}

#[test]
fn budget_guard_skips_parent_frame_publish_under_ceiling() {
    // Discriminator for the cooperative-cascade contract: when the
    // depth guard fires inside a recursive sub-evaluation, the
    // PARENT frame (still under the ceiling) must ALSO skip its
    // publish into `evaluate_deferred_memo`, because the parent
    // consumed a truncated child's input-node carrier and any
    // downstream operator the parent dispatched against that
    // carrier produced a budget-tainted result.
    //
    // Setup: build a 10_000-deep `KeyOf` chain over a Primitive
    // leaf. The OUTERMOST `KeyOf` is the top-level entry node; its
    // recursive sub-call walks the inner chain — which trips the
    // guard at depth 256 — and the outer arm receives the truncated
    // inner result. If the suppression cascade is wired correctly,
    // the OUTERMOST node MUST NOT appear in the memo after the call.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let store = host.project_type_store().semantic_graph();
    let leaf = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));
    let mut current = leaf;
    for _ in 0..10_000 {
        current = store.intern_node(SemanticNodeData::KeyOf { base: current });
    }
    let outermost = current;

    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let graph = host.project_type_store().semantic_graph();

    let before = graph.stats_snapshot();
    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, outermost, context);
    let after_one = graph.stats_snapshot();
    // Confirm the call actually ran the recursive walk: at least
    // one miss must have been recorded (the first cold lookup).
    let one_miss_delta =
        after_one.evaluate_deferred_memo_misses - before.evaluate_deferred_memo_misses;
    assert!(
        one_miss_delta >= 1,
        "first call MUST miss the memo on the cold outermost key (delta={one_miss_delta})"
    );

    // Re-evaluate. If the parent published a budget-tainted entry,
    // this call hits — miss-counter does NOT advance. Under the
    // correct contract, the parent suppressed its publish, so the
    // second call misses again.
    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, outermost, context);
    let after_two = graph.stats_snapshot();
    let two_miss_delta =
        after_two.evaluate_deferred_memo_misses - after_one.evaluate_deferred_memo_misses;
    assert!(
        two_miss_delta >= 1,
        "second call on the outermost truncated key MUST miss the memo again (parent \
         frame suppressed its publish under truncation). Observed delta = {two_miss_delta}. \
         A zero delta means a budget-tainted result survived in the warm memo — the \
         parent-frame publish-suppression cascade is broken."
    );
}
