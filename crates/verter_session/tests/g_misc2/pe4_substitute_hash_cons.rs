//! Discriminator tests for the hash-cons memo on
//! `substitute_semantic_type_param`.
//!
//! Substitution is a pure function of its three inputs
//! (`value_expr`, `parameter_node`, `arg`). The semantic-graph store
//! is arena-scoped (one per project_identity + env hashes), and
//! `SemanticNodeId`s inside the arena are content-addressed integers,
//! so a triple of ids is a complete identity for the substitution
//! result. A hash-cons memo on the store collapses identical triples
//! that surface from different call paths to one cached result.
//!
//! The tests below DISCRIMINATE:
//!
//! 1. **`substitute_semantic_type_param` consults the memo on every
//!    call** — the very first call MISSES; the very second call with
//!    the same `(value_expr, parameter_node, arg)` triple HITS. Both
//!    counter rails advance accordingly. A non-wired memo would
//!    leave both counters at zero.
//!
//! 2. **Distinct triples miss the memo** — calling with different
//!    `arg` values produces distinct cache keys, so every call
//!    misses. The hits counter stays flat and the misses counter
//!    advances by the number of distinct triples.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    LiteralValue, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryOutput,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

const SOURCE_TS: &str = r#"
export interface KSource {
  a: string;
  b: number;
  c: boolean;
}

export type Mapped = { [K in 'a' | 'b' | 'c']: KSource[K] };
"#;

/// Set the host up with the fixture loaded.
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

/// Lower `Mapped` so its `value_expr` and binder become reachable as
/// semantic-node ids. The lowered top-level decl is a
/// `DeclPlaceholder`; one `SemanticQueryKey::Instantiate` dispatch
/// unwraps it into the concrete `Mapped` carrier (the per-K
/// materialiser would consult).
fn lower_mapped(host: &Arc<VerterHost>) -> SemanticNodeId {
    let expr = TypeExpr::Ref {
        name: Arc::from("Mapped"),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        host,
        "/source.ts",
        &expr,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )
    .expect("lowering `Mapped` must succeed");

    // The carrier is a `DeclPlaceholder` carrier; force one
    // Instantiate dispatch to resolve the body and reach the real
    // `Mapped` node. This mirrors the path the per-K materialiser
    // takes when it consults the value_expr.
    let graph = host.project_type_store().semantic_graph();
    match graph.node_data(carrier).as_deref() {
        Some(SemanticNodeData::Opaque(
            verter_session::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            },
        )) => {
            let _ = whole_hash;
            let base = verter_session::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                Arc::clone(canonical_id),
                Arc::clone(name),
            );
            let key = SemanticQueryKey::Instantiate {
                base,
                args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                context: verter_session::semantic_query::InstantiateContext::new(
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                    Default::default(),
                ),
            };
            match for_tests::dispatch_execute_type_node_for_tests(host, key) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                other => panic!("Instantiate dispatch must succeed; got {other:?}"),
            }
        }
        _ => carrier,
    }
}

/// Walk the lowered carrier and extract `(value_expr,
/// parameter_node)` from the underlying mapped type's `MapperKey`.
/// The lowered top-level can be a `DeclRef` (transit-shallow lowering
/// stops at the declaration boundary); we follow it through the
/// `Instantiate` query to unwrap one body layer at a time until we
/// reach the Mapped node.
fn extract_mapper_inputs(
    host: &Arc<VerterHost>,
    carrier: SemanticNodeId,
) -> (SemanticNodeId, SemanticNodeId) {
    let graph = host.project_type_store().semantic_graph();
    let mut current = carrier;
    for _ in 0..8 {
        let data = graph
            .node_data(current)
            .expect("lowered carrier must have semantic data");
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => {
                current = *inner;
            }
            SemanticNodeData::Mapped { mapper, .. } => {
                return (mapper.value_expr, mapper.parameter_node);
            }
            SemanticNodeData::DeclRef { identity } => {
                let key = SemanticQueryKey::Instantiate {
                    base: identity.to_type_slot_unscoped(),
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    context: verter_session::semantic_query::InstantiateContext::new(
                        ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                        Default::default(),
                    ),
                };
                current = match for_tests::dispatch_execute_type_node_for_tests(host, key) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    other => {
                        panic!("Instantiate of DeclRef must yield a value node; got {other:?}")
                    }
                };
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                let key = SemanticQueryKey::Instantiate {
                    base: base.to_type_slot_unscoped(),
                    args: Arc::clone(args),
                    context: verter_session::semantic_query::InstantiateContext::new(
                        ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                        Default::default(),
                    ),
                };
                current = match for_tests::dispatch_execute_type_node_for_tests(host, key) {
                    QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
                    other => panic!(
                        "Instantiate of InstantiationRef must yield a value node; got {other:?}"
                    ),
                };
            }
            other => panic!(
                "expected to reach a `SemanticNodeData::Mapped` within 8 hops; \
                 stopped at: {other:?}"
            ),
        }
    }
    panic!("did not reach a Mapped node within the carrier-unwrap budget")
}

fn intern_literal(host: &Arc<VerterHost>, name: &str) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        name.to_string(),
    )))
}

#[test]
fn repeated_substitutions_with_identical_triple_hit_hash_cons_memo() {
    let host = make_host();
    let mapped_carrier = lower_mapped(&host);
    let (value_expr, parameter_node) = extract_mapper_inputs(&host, mapped_carrier);
    let arg = intern_literal(&host, "a");

    let store = host.project_type_store().semantic_graph();
    let baseline = store.stats_snapshot();

    // First call MUST miss (cold lookup) and publish.
    let first = for_tests::dispatch_substitute_for_tests(&host, value_expr, parameter_node, arg);
    let after_first = store.stats_snapshot();
    assert!(
        after_first.substitute_memo_misses > baseline.substitute_memo_misses,
        "first call MUST miss the hash-cons memo on a cold triple; baseline misses = {}, \
         after-first misses = {}. The memo wire-up at the top of \
         `substitute_semantic_type_param` is bypassed or the publish leg fails.",
        baseline.substitute_memo_misses,
        after_first.substitute_memo_misses
    );

    // Second call with the SAME triple MUST hit and return the
    // exact same SemanticNodeId.
    let second = for_tests::dispatch_substitute_for_tests(&host, value_expr, parameter_node, arg);
    let after_second = store.stats_snapshot();
    assert_eq!(
        first, second,
        "two calls with the same (value_expr, parameter_node, arg) triple MUST return the same \
         SemanticNodeId — substitution is a pure function of its three inputs."
    );
    assert!(
        after_second.substitute_memo_hits > after_first.substitute_memo_hits,
        "second call with the same triple MUST hit the hash-cons memo. After-first hits = {}, \
         after-second hits = {}. A flat hit-counter means the memo lookup did not consult the \
         cache or the cache key composition differs between get and publish.",
        after_first.substitute_memo_hits,
        after_second.substitute_memo_hits
    );
}

#[test]
fn distinct_triples_miss_hash_cons_memo() {
    let host = make_host();
    let mapped_carrier = lower_mapped(&host);
    let (value_expr, parameter_node) = extract_mapper_inputs(&host, mapped_carrier);

    let store = host.project_type_store().semantic_graph();
    let baseline = store.stats_snapshot();

    // Three distinct args produce three distinct triples; all MUST
    // miss.
    let arg_a = intern_literal(&host, "a");
    let arg_b = intern_literal(&host, "b");
    let arg_c = intern_literal(&host, "c");

    let _ = for_tests::dispatch_substitute_for_tests(&host, value_expr, parameter_node, arg_a);
    let _ = for_tests::dispatch_substitute_for_tests(&host, value_expr, parameter_node, arg_b);
    let _ = for_tests::dispatch_substitute_for_tests(&host, value_expr, parameter_node, arg_c);

    let after = store.stats_snapshot();
    let miss_delta = after.substitute_memo_misses - baseline.substitute_memo_misses;
    let hit_delta = after.substitute_memo_hits - baseline.substitute_memo_hits;

    // Both the top-level entry AND the recursive
    // `substitute_with_change_tracking` helper probe the SAME
    // store-owned `substitute_memo`. The unified memo is keyed by
    // `(node, parameter_node, arg)`, which is identical key
    // composition at both layers, so a top-level call now produces
    // exactly one probe per distinct intermediate subtree it
    // recurses into (in addition to the top-level entry probe).
    //
    // Discriminating assertions retained:
    //  - Each distinct top-level triple MUST produce at LEAST one
    //    miss (the cold path ran).
    //  - Distinct top-level triples MUST NOT collapse to the same
    //    memo entry — `hit_delta == 0` proves the key composition
    //    keeps `arg` in the identity, so wrongly collapsing
    //    `(value_expr, parameter_node, arg_a)` and
    //    `(value_expr, parameter_node, arg_b)` would surface here.
    assert!(
        miss_delta >= 3,
        "three distinct triples MUST each miss the hash-cons memo at the top-level entry; \
         observed miss delta = {miss_delta}. The unified memo also counts recursive-helper \
         probes so the total may exceed 3, but it must never fall short of one miss per cold \
         top-level triple."
    );
    assert_eq!(
        hit_delta, 0,
        "three distinct triples MUST produce zero memo hits; observed hit delta = {hit_delta}. \
         A non-zero hit count means the cache key composition wrongly collapsed distinct \
         triples (e.g. `arg` dropped from the identity)."
    );
}
