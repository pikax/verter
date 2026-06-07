//! Discriminator tests for the `build_mapped_type` and
//! `synthesise_mapped_surface` key-space-independent value hoist.
//!
//! The hoist factors out the per-K materialisation when the mapper's
//! `value_expr` contains no structural reference to the binder
//! (`mapper.parameter_node`). For such mapped types the per-K
//! substitution would be the identity, so every K's substituted
//! carrier collapses to `value_expr` itself; the downstream evaluate
//! → Instantiate → trailing Conditional reduction chain should run
//! once per mapped type instead of once per enumerated key.
//!
//! The tests below DISCRIMINATE:
//!
//! 1. **K-independent value position must hoist** — `{ [K in K-set]:
//!    ConstantType }` produces N members whose `value` semantic-node
//!    id is the *same* id for every K. Pre-hoist the per-K
//!    materialiser is called N times and may produce N distinct ids
//!    (each individually structurally identical, but the per-K
//!    materialiser allocates fresh `Literal(K)` nodes and may
//!    interleave caches that diverge the resolved-after-evaluation
//!    id). Post-hoist a single evaluation closes all K values to one
//!    id.
//!
//! 2. **K-dependent value position must NOT hoist** — `{ [K in keyof
//!    T]: T[K] }`-style mapped types reference the binder via the
//!    `IndexedAccess` index and must retain the per-K materialiser
//!    semantics (each K reads a distinct member of T). The two K
//!    values must produce *distinct* semantic-node ids reflecting the
//!    different selected member types. If the hoist over-fires here
//!    every K would collapse to the same value — a correctness
//!    regression.
//!
//! 3. **`infer`-bearing K-dependent value position must NOT hoist** —
//!    `{ [K in keyof T]: T[K] extends infer R ? R : never }` references
//!    the binder via `T[K]` AND introduces an `infer R`. The walker's
//!    cross-variant `Infer { name }` fallback must classify this as
//!    K-dependent so the per-K materialiser remains the authority.
//!
//! All three fixtures exercise core mapped-type semantics that are
//! correct independent of the hoist. The hoist's value is asserted
//! via the *shared-id* discriminator on (1), and the *distinct-ids*
//! discriminator on (2) and (3). These three together discriminate
//! "hoisted what I should" vs. "wrongly hoisted what I shouldn't".

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticQueryKey, SemanticQueryOutput,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

/// K-INDEPENDENT VALUE — every key's value is `number`. After
/// publication the mapped surface members must all reference the
/// *same* semantic-node id for the value type.
const K_INDEPENDENT_TS: &str = r#"
export type KIndependent = { [K in 'a' | 'b' | 'c']: number };
"#;

/// K-DEPENDENT VALUE — `T[K]` references the binder via the index.
/// After publication the surface members must reference *distinct*
/// semantic-node ids for the value type (one per selected member).
const K_DEPENDENT_TS: &str = r#"
export interface KSource {
  a: string;
  b: number;
  c: boolean;
}

export type KDependent = { [K in 'a' | 'b' | 'c']: KSource[K] };
"#;

/// K-DEPENDENT VALUE w/ `infer R` — references the binder via `T[K]`
/// AND introduces an `infer R` in the value position. The walker's
/// cross-variant `Infer { name }` fallback must keep this on the
/// per-K materialiser path.
const K_DEPENDENT_INFER_TS: &str = r#"
export interface KSourceInfer {
  a: string;
  b: number;
}

export type KDependentInfer = {
  [K in 'a' | 'b']: KSourceInfer[K] extends infer R ? R : never
};
"#;

fn evaluate_alias(
    host: &Arc<VerterHost>,
    alias_name: &str,
) -> verter_session::semantic_query::SemanticNodeId {
    let expr = TypeExpr::Ref {
        name: Arc::from(alias_name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };
    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        host,
        "/source.ts",
        &expr,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    )
    .unwrap_or_else(|| panic!("lowering `{alias_name}` under Published(Expanded) must succeed"));

    let project_query = SemanticQueryKey::ProjectPath {
        base: carrier,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    };
    match for_tests::dispatch_execute_type_node_for_tests(host, project_query) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        other => panic!(
            "ProjectPath {{ {alias_name}, [], Published(Expanded) }} must yield a value node, \
             got {other:?}"
        ),
    }
}

fn surface_member_value_ids(
    host: &Arc<VerterHost>,
    surface_node: verter_session::semantic_query::SemanticNodeId,
) -> Vec<(String, verter_session::semantic_query::SemanticNodeId)> {
    let graph = host.project_type_store().semantic_graph();
    let data = graph
        .node_data(surface_node)
        .expect("surface node must have semantic data");
    let view = match data.as_ref() {
        SemanticNodeData::Object(view) => view.clone(),
        other => panic!("surface must be an Object, got: {other:?}"),
    };
    view.members
        .iter()
        .map(|m| (m.name.as_ref().to_string(), m.value))
        .collect()
}

#[test]
fn k_independent_value_collapses_to_shared_node_id_under_hoist() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(K_INDEPENDENT_TS),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });

    // Snapshot the per-K materialiser counter BEFORE the alias is
    // evaluated. Post-hoist the counter must NOT advance for a
    // K-independent value position — the per-K materialiser is
    // bypassed entirely and a single shared evaluation runs once.
    // Pre-hoist (regression direction) every key would call the
    // per-K materialiser, advancing the counter by N. The counter
    // is the discriminating signal because the arena's shard dedup
    // would mask a "same shared id" structural check — that check
    // passes pre-hoist too.
    let store = host.project_type_store().semantic_graph();
    let before = store.stats_snapshot().mapped_per_k_materializations;

    let surface = evaluate_alias(&host, "KIndependent");
    let members = surface_member_value_ids(&host, surface);

    let after = store.stats_snapshot().mapped_per_k_materializations;

    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"a") && names.contains(&"b") && names.contains(&"c"),
        "K-independent mapped surface must enumerate 'a', 'b', 'c'; got: {names:?}"
    );

    // Structural invariant: every K's value is the same shared node
    // id. This holds pre-hoist (arena shard dedup) AND post-hoist
    // (single shared evaluation), so it's a sanity rail rather than
    // a discriminator. The counter-delta below is the real
    // discriminator.
    let value_ids: std::collections::HashSet<_> = members.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        value_ids.len(),
        1,
        "K-independent mapped type must have ONE shared value node id across all members; got \
         {} distinct ids: {value_ids:?}",
        value_ids.len()
    );

    // DISCRIMINATOR: post-hoist the per-K materialiser counter must
    // not advance — a K-independent value position is hoisted out of
    // the per-K loop and the shared evaluation runs through
    // `evaluate_deferred_semantic_node_with_context` directly. A
    // non-zero delta proves the hoist failed to fire.
    assert_eq!(
        after - before,
        0,
        "K-independent mapped type must NOT invoke the per-K materialiser; observed {} \
         per-K materialisations (expected 0). The hoist guard mis-classified the value \
         expression as K-dependent.",
        after - before
    );
}

#[test]
fn k_dependent_value_keeps_per_k_materialiser_with_distinct_ids() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(K_DEPENDENT_TS),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });

    let store = host.project_type_store().semantic_graph();
    let before = store.stats_snapshot().mapped_per_k_materializations;

    let surface = evaluate_alias(&host, "KDependent");
    let members = surface_member_value_ids(&host, surface);

    let after = store.stats_snapshot().mapped_per_k_materializations;

    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"a") && names.contains(&"b") && names.contains(&"c"),
        "K-dependent mapped surface must enumerate 'a', 'b', 'c'; got: {names:?}"
    );

    // K-dependent discriminator: every K selects a different member of
    // KSource, so the resolved value types must be distinct. The
    // hoist MUST decline here (`subtree_references_node` returns true
    // because `T[K]`'s `IndexedAccess { index: TypeNode(K) }` carries
    // the binder), so the per-K materialiser runs and produces
    // distinct resolved values per K. If the hoist incorrectly fires
    // the per-K outputs collapse to one — fail.
    let value_ids: std::collections::HashSet<_> = members.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        value_ids.len(),
        3,
        "K-dependent mapped type must have THREE distinct value node ids (one per selected \
         member of KSource); got {} ids: {value_ids:?}. A collapse here indicates the K-independent \
         hoist over-fired on a K-dependent value position.",
        value_ids.len()
    );

    // DISCRIMINATOR: the per-K materialiser must be invoked at least
    // 3 times (once per K). A zero delta proves the hoist over-fired
    // and the per-K materialiser never ran — which would be a
    // correctness regression. The `>= 3` rather than `== 3` lower
    // bound tolerates the Shallow walker's nested per-K dispatch
    // (it routes through the `selected_key` materialiser, which also
    // bumps the counter).
    assert!(
        after - before >= 3,
        "K-dependent mapped type must invoke the per-K materialiser at least 3 times; \
         observed {} per-K materialisations. The hoist over-fired and bypassed the per-K \
         materialiser on a K-dependent value position.",
        after - before
    );
}

#[test]
fn k_dependent_value_with_infer_keeps_per_k_materialiser() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(K_DEPENDENT_INFER_TS),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });

    let store = host.project_type_store().semantic_graph();
    let before = store.stats_snapshot().mapped_per_k_materializations;

    let surface = evaluate_alias(&host, "KDependentInfer");
    let members = surface_member_value_ids(&host, surface);

    let after = store.stats_snapshot().mapped_per_k_materializations;

    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"a") && names.contains(&"b"),
        "K-dependent infer mapped surface must enumerate 'a', 'b'; got: {names:?}"
    );

    // The value expression `KSourceInfer[K] extends infer R ? R : never`
    // references K via the IndexedAccess. The walker MUST classify
    // this as K-dependent (the IndexedAccess index TypeNode carries
    // the binder), so the per-K materialiser runs and produces
    // distinct value ids for the two keys (string vs. number).
    let value_ids: std::collections::HashSet<_> = members.iter().map(|(_, id)| *id).collect();
    assert_eq!(
        value_ids.len(),
        2,
        "K-dependent mapped type with `infer R` must keep distinct per-K resolved values; got \
         {} ids: {value_ids:?}. Collapse means the hoist mis-classified an `infer`-bearing \
         K-dependent value as K-independent.",
        value_ids.len()
    );

    // DISCRIMINATOR: the per-K materialiser must run at least twice
    // (once per K). The `infer R` introduces a new binder local to
    // the value position, so a naive "any TypeParam reference?" walker
    // would miss the K dependency. The walker's IndexedAccess
    // recursion catches it through the `TypeNode(K)` index.
    assert!(
        after - before >= 2,
        "K-dependent infer-bearing mapped type must invoke the per-K materialiser at least \
         twice; observed {} per-K materialisations. The walker mis-classified the IndexedAccess \
         index as K-independent.",
        after - before
    );
}
