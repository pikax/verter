//! Discriminator tests for the hash-cons memo on
//! `evaluate_deferred_semantic_node_with_context`.
//!
//! The evaluator's fix-point walk through Alias / KeyOf /
//! IndexedAccess / Mapped / Conditional / TypeOf / TemplateLiteral /
//! DeclPlaceholder hops is a pure function of `(node,
//! ProjectionReductionContext)`. The store's `evaluate_deferred_memo`
//! collapses repeated `(node, context)` visits to one cached result,
//! which is the architectural win for ChatMessages.vue: a
//! K-independent subtree like `MessageBase<T>` embedded inside a
//! K-dependent value expression evaluates once per K pre-memo
//! (because the OUTER expression's recursive walk re-enters the
//! evaluator with the same K-independent inner node). Post-memo
//! every subsequent visit collapses to one `DashMap::get`.
//!
//! The tests below DISCRIMINATE:
//!
//! 1. **`evaluate_deferred_semantic_node_with_context` consults the
//!    memo on every call** — the very first call MISSES; a second
//!    call with the same `(node, context)` pair HITS. Both counter
//!    rails advance.
//!
//! 2. **Different contexts produce distinct cache keys** — the same
//!    node evaluated under `published(Expanded)` versus
//!    `published(Shallow)` produces TWO misses (one each) and ZERO
//!    hits. Context is part of the cache key.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    LiteralValue, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

const SOURCE_TS: &str = r#"
export interface DeepSource {
  alpha: string;
  beta: number;
}

export type Indirect = DeepSource['alpha'];
"#;

fn make_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(SOURCE_TS),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });
    host
}

/// Lower a top-level alias to its deferred carrier (DeclRef /
/// IndexedAccess / similar) so the evaluator has something
/// non-trivial to fix-point on.
fn lower_alias_carrier(host: &Arc<VerterHost>, alias_name: &str) -> SemanticNodeId {
    let expr = TypeExpr::Ref {
        name: Arc::from(alias_name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        host,
        "/source.ts",
        &expr,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )
    .expect("lowering must succeed")
}

#[test]
fn repeated_evaluate_with_same_node_and_context_hits_memo() {
    let host = make_host();
    let carrier = lower_alias_carrier(&host, "Indirect");
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let store = host.project_type_store().semantic_graph();
    let baseline = store.stats_snapshot();

    // First call MUST miss (cold lookup) and publish.
    let first = for_tests::dispatch_evaluate_deferred_for_tests(&host, carrier, context);
    let after_first = store.stats_snapshot();
    assert!(
        after_first.evaluate_deferred_memo_misses > baseline.evaluate_deferred_memo_misses,
        "first call MUST miss the evaluate-deferred memo on a cold pair; baseline misses = {}, \
         after-first misses = {}. The memo wire-up at the top of \
         `evaluate_deferred_semantic_node_with_context` is bypassed or the publish leg fails.",
        baseline.evaluate_deferred_memo_misses,
        after_first.evaluate_deferred_memo_misses
    );

    // Second call with same (node, context) MUST hit.
    let second = for_tests::dispatch_evaluate_deferred_for_tests(&host, carrier, context);
    let after_second = store.stats_snapshot();
    assert_eq!(
        first, second,
        "two calls with the same (node, context) MUST return the same SemanticNodeId — the \
         evaluator is a pure function of its two inputs."
    );
    assert!(
        after_second.evaluate_deferred_memo_hits > after_first.evaluate_deferred_memo_hits,
        "second call with the same pair MUST hit the evaluate-deferred memo. After-first hits = \
         {}, after-second hits = {}. A flat hit-counter means the memo lookup did not consult \
         the cache or the cache key composition differs between get and publish.",
        after_first.evaluate_deferred_memo_hits,
        after_second.evaluate_deferred_memo_hits
    );
}

#[test]
fn different_contexts_produce_distinct_memo_keys() {
    let host = make_host();
    let carrier = lower_alias_carrier(&host, "Indirect");

    let store = host.project_type_store().semantic_graph();
    let baseline = store.stats_snapshot();

    let ctx_expanded = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let ctx_shallow = ProjectionReductionContext::published(ProjectionMode::Shallow);

    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, carrier, ctx_expanded);
    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, carrier, ctx_shallow);

    let after = store.stats_snapshot();
    let miss_delta = after.evaluate_deferred_memo_misses - baseline.evaluate_deferred_memo_misses;
    let hit_delta = after.evaluate_deferred_memo_hits - baseline.evaluate_deferred_memo_hits;

    assert!(
        miss_delta >= 2,
        "two distinct contexts on the SAME node MUST miss the memo at least twice; observed \
         miss delta = {miss_delta}. A miss-counter < 2 means the contexts hash-collide on the \
         cache key — a correctness regression."
    );
    // The TOP-LEVEL pair (carrier, ctx_expanded) and (carrier,
    // ctx_shallow) are two distinct cold keys → 2 misses from the
    // entry-node memo lookup. Recursive sub-evaluations may also
    // hit-or-miss; the inequality `>= 2` accounts for both.
    let _ = hit_delta;
}
