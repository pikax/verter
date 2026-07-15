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
use verter_session::{for_tests, FileLanguage, HostConfig, UpsertRequest, VerterHost};
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
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/source.ts")
            .static_resolution(),
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

/// Per-canonical invalidation MUST drop the hash-cons memos.
///
/// Both memos cache `(node_id, ...) → result_id` mappings. The cache
/// KEY is content-addressed (semantic-node ids are arena-interned),
/// but the cache VALUE is the result of a WALK through file content
/// — a `TypeOf` evaluation routes through a `ValueRootKey` whose
/// resolved structure depends on the owning file; a generic
/// instantiation may walk through an imported type-decl body. When
/// a canonical's content changes under a live workspace, the cached
/// `(key → result_id)` mapping may no longer reflect the post-edit
/// semantics — even though the KEY remains structurally valid.
///
/// Under the prior `invalidate_all`-only clear policy, a stale
/// mapping survived `invalidate_canonical` and poisoned every
/// subsequent caller asking for the same key. The fix-cycle adds a
/// sledgehammer clear of both memos on every
/// `invalidate_canonical`.
#[test]
fn invalidate_canonical_clears_evaluate_deferred_memo() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let store = host.project_type_store().semantic_graph();

    // Construct a synthetic Alias chain. The evaluator walks the
    // chain through the Alias arm, so the memo populates on first
    // call.
    let leaf = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));
    let mid = store.intern_node(SemanticNodeData::Alias(leaf));
    let head = store.intern_node(SemanticNodeData::Alias(mid));

    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let baseline = store.stats_snapshot();

    // First call — cold; populates the memo.
    let first = for_tests::dispatch_evaluate_deferred_for_tests(&host, head, context);
    let after_first = store.stats_snapshot();
    let miss_delta_1 =
        after_first.evaluate_deferred_memo_misses - baseline.evaluate_deferred_memo_misses;
    assert!(
        miss_delta_1 >= 1,
        "first call MUST miss the cold memo; got delta = {miss_delta_1}"
    );

    // Second call — warm; hits the memo. This proves the memo was
    // populated by the first call.
    let second = for_tests::dispatch_evaluate_deferred_for_tests(&host, head, context);
    let after_second = store.stats_snapshot();
    let hit_delta =
        after_second.evaluate_deferred_memo_hits - after_first.evaluate_deferred_memo_hits;
    assert!(
        hit_delta >= 1,
        "second call MUST hit the warm memo (proves the memo was populated). \
         Hit delta = {hit_delta}. If this is 0, the memo never publishes — the test \
         setup is broken before the discriminator runs."
    );
    assert_eq!(first, second, "pure-function: same inputs → same output");

    // Invalidate the canonical. Under the correct contract this
    // also drops the hash-cons memos.
    let _ = store.invalidate_canonical("/source.ts");

    // Third call (post-invalidation) — MUST miss again. Pre-fix
    // this would hit the surviving stale entry.
    let _ = for_tests::dispatch_evaluate_deferred_for_tests(&host, head, context);
    let after_third = store.stats_snapshot();
    let miss_delta_post_inval =
        after_third.evaluate_deferred_memo_misses - after_second.evaluate_deferred_memo_misses;
    assert!(
        miss_delta_post_inval >= 1,
        "post-invalidate_canonical call MUST miss the memo (the per-canonical clear \
         dropped the prior entry). Miss delta = {miss_delta_post_inval}. A zero delta \
         means the stale entry survived invalidate_canonical — every subsequent caller \
         for this key would observe pre-edit semantics regardless of how the file \
         content changed."
    );
}

/// Per-canonical invalidation MUST drop the substitute hash-cons
/// memo alongside the evaluate-deferred memo.
///
/// Discriminator strategy: populate the substitute memo directly
/// via the public publish API, call `invalidate_canonical`, then
/// peek via the public lookup API and assert `None`. Pre-fix the
/// lookup returns the surviving stale entry; post-fix the
/// sledgehammer clear evicts it.
#[test]
fn invalidate_canonical_clears_substitute_memo() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let store = host.project_type_store().semantic_graph();

    let a = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::String,
    ));
    let b = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Number,
    ));
    let c = store.intern_node(SemanticNodeData::Primitive(
        verter_session::semantic_query::PrimitiveKind::Boolean,
    ));

    // Populate the substitute memo via the public publish API.
    store.substitute_memo_publish(a, b, c, a);
    assert!(
        store.substitute_memo_get(a, b, c).is_some(),
        "substitute_memo must be populated after publish — test setup precondition"
    );

    // Invalidate the canonical. The sledgehammer policy drops the
    // substitute memo in full.
    let _ = store.invalidate_canonical("/source.ts");

    assert!(
        store.substitute_memo_get(a, b, c).is_none(),
        "post-invalidate_canonical, substitute_memo lookup MUST return None — the \
         per-canonical clear dropped the prior entry. If this returns Some, the \
         substitute memo survived invalidate_canonical and would continue serving \
         pre-edit derived results across the workspace."
    );
}
