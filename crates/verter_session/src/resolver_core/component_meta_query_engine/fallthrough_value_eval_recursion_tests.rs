//! Discriminating tests for the fallthrough node-DAG recursion fix + the exact
//! override-identity cache key.
//!
//! These exercise: (1) the persistent-memo + shared op-budget that bounds the
//! `known_spread` / `dynamic-root` node walkers over a content-interned diamond;
//! (2/3) the exact `FallthroughOverrideValueKey` projection (no field drop, no
//! depth truncation, over-budget → `Uncacheable`, order-independence,
//! structure-not-digest equality) and the consumed-bindings key now carrying the
//! override identity; (4) a recursive-alias characterization that the structural
//! materializer returns a bounded leaf without stack growth.

use std::sync::Arc;

use super::{collect_dynamic_root_candidates_from_node, known_spread_keys_from_node};
use crate::meta::MetaProject;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::resolver_core::fallthrough_override_key::{
    FallthroughOverrideSetKey, FallthroughOverrideValueKey,
};
use crate::resolver_core::{
    ComponentMetaQueryEngine, FallthroughOverrideIdentity, FallthroughPropOverride,
};
use crate::semantic_query::{
    IndexKey, PrimitiveKind, SemanticNodeData, SemanticNodeId, SurfaceView,
};
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use verter_type_expr::LiteralValue;

fn open_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn empty_surface() -> SurfaceView {
    SurfaceView {
        members: Arc::from(Vec::new()),
        call_signatures: Arc::from(Vec::new()),
        construct_signatures: Arc::from(Vec::new()),
        index_signatures: Arc::from(Vec::new()),
        keyspace: None,
        has_index_signature: false,
    }
}

/// Install a per-request projection budget with cap `cap` for the duration of
/// the returned guard. Returns the context (for reading the executed counter)
/// and the guard (drop = uninstall).
fn install_budget(cap: usize) -> (Arc<RequestContext>, RequestContextGuard) {
    let rctx = RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from("/App.vue"),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        cap,
    );
    let guard = RequestContextGuard::install(Arc::clone(&rctx));
    (rctx, guard)
}

/// Build a depth-`n` content-interned shared diamond whose union arms BOTH point
/// at one shared alias of the level below (`type Dn = An | Bn`, `An = Bn =
/// D(n-1)`). Distinct nodes are O(n); a path-scoped (non-memoized) walk
/// re-traverses O(2^n).
fn build_diamond(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    leaf: SemanticNodeId,
    n: u32,
) -> SemanticNodeId {
    let mut cur = leaf;
    for _ in 0..n {
        let alias = graph.intern_node(SemanticNodeData::Alias(cur));
        cur = graph.intern_node(SemanticNodeData::Union(Arc::from(vec![alias, alias])));
    }
    cur
}

/// #1 — Diamond-DAG over-budget for BOTH node walkers. The persistent memo
/// makes each distinct node computed once (so the shared op-budget is charged
/// O(distinct nodes), NOT O(2^depth)); the walk still produces a real result.
///
/// RED on the pre-fix tree: the walkers neither memoize nor charge the budget,
/// so `projection_ops_executed_count()` stays `0` (fails `delta >= n`) while the
/// walk does O(2^depth) re-traversals. GREEN here: each walker charges a bounded
/// O(n) and returns a result. A hypothetical "budget but no memo" regression
/// would charge ~2^depth and fail the upper bound — so the assertion
/// discriminates BOTH halves of the fix without ever hanging.
#[test]
fn diamond_dag_walkers_are_memo_bounded_and_charge_shared_budget() {
    let project = open_project();
    let host = project.host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let ctx: &dyn crate::resolver_core::ResolverContext = host;

    let n: u32 = 14;

    // known_spread diamond: leaf is an (empty) object surface.
    let ks_leaf = graph.intern_node(SemanticNodeData::Object(empty_surface()));
    let ks_top = build_diamond(&graph, ks_leaf, n);

    // dynamic-root diamond: leaf is a string literal (a native-tag candidate).
    let dr_leaf = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "div".to_string(),
    )));
    let dr_top = build_diamond(&graph, dr_leaf, n);

    let (rctx, _guard) = install_budget(100_000);

    let before_ks = rctx.projection_budget.projection_ops_executed_count();
    let spread = known_spread_keys_from_node(ctx, ks_top);
    let after_ks = rctx.projection_budget.projection_ops_executed_count();
    let ks_delta = after_ks - before_ks;

    assert!(
        spread.is_some(),
        "the diamond still resolves to a known spread surface"
    );
    assert!(
        ks_delta >= n as usize,
        "known_spread must charge the shared budget per distinct node (delta {ks_delta}, n {n}) — \
         pre-fix the walker does not touch the budget at all (delta 0)"
    );
    assert!(
        ks_delta <= 8 * n as usize,
        "the persistent memo must bound known_spread work to O(distinct nodes); \
         a non-memoized walk would charge ~2^{} (delta {ks_delta})",
        n + 2
    );

    let before_dr = rctx.projection_budget.projection_ops_executed_count();
    let candidates = collect_dynamic_root_candidates_from_node(ctx, dr_top, &[]);
    let after_dr = rctx.projection_budget.projection_ops_executed_count();
    let dr_delta = after_dr - before_dr;

    assert!(
        !candidates.is_empty(),
        "the diamond still resolves to native-tag dynamic-root candidates"
    );
    assert!(
        dr_delta >= n as usize,
        "dynamic-root must charge the shared budget per distinct node (delta {dr_delta}, n {n})"
    );
    assert!(
        dr_delta <= 8 * n as usize,
        "the persistent memo must bound dynamic-root work to O(distinct nodes) (delta {dr_delta})"
    );
}

fn value_key_of(
    engine: &ComponentMetaQueryEngine<'_>,
    node: SemanticNodeId,
) -> FallthroughOverrideValueKey {
    match engine.fallthrough_override_identity(&[FallthroughPropOverride {
        name: "p".to_string(),
        node,
    }]) {
        FallthroughOverrideIdentity::Exact(set) => set.entries[0].1.clone(),
        other => panic!("expected Exact identity, got {other:?}"),
    }
}

/// #2 — The exact override-identity projection: distinct override values are
/// distinct keys (no aliasing), structurally-equal values are equal keys
/// (equality compares STRUCTURE, never a digest), deep structure does not
/// truncate, an over-budget projection yields `Uncacheable`, and prop order
/// does not matter after uniqueness.
///
/// RED on the pre-fix tree: the override identity was a `u64`
/// `node_structural_content_hash` that OMITTED `IndexedAccess.index` and
/// TRUNCATED at depth 64 — so `T["a"]` vs `T["b"]` (and a >64-deep difference)
/// collided. GREEN here: the typed `FallthroughOverrideValueKey` keeps every
/// field, so they are distinct.
#[test]
fn override_value_key_is_exact_and_distinguishes_every_field() {
    let project = open_project();
    let host = project.host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let engine = ComponentMetaQueryEngine::new(ctx);

    // (a) IndexedAccess differing ONLY in `index` — the field the old u64
    // fingerprint dropped.
    let object = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let indexed_a = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: IndexKey::String(Arc::from("a")),
    });
    let indexed_b = graph.intern_node(SemanticNodeData::IndexedAccess {
        object,
        index: IndexKey::String(Arc::from("b")),
    });
    let key_a = value_key_of(&engine, indexed_a);
    let key_b = value_key_of(&engine, indexed_b);
    assert_ne!(
        key_a, key_b,
        "IndexedAccess values differing only in `index` must project to DISTINCT keys"
    );

    // (b) structure-not-digest: re-projecting the SAME node yields an EQUAL key.
    assert_eq!(
        key_a,
        value_key_of(&engine, indexed_a),
        "re-projecting the same value yields a structurally-equal key"
    );

    // (c) deep structure does not truncate: a chain deeper than the old depth-64
    // cap, differing only at the bottom, stays distinct.
    let deep_string = build_alias_chain(
        &graph,
        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
        70,
    );
    let deep_number = build_alias_chain(
        &graph,
        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
        70,
    );
    assert_ne!(
        value_key_of(&engine, deep_string),
        value_key_of(&engine, deep_number),
        "a difference BELOW the old depth-64 cap must NOT be truncated away"
    );

    // (d) over-budget → Uncacheable.
    let deep = build_alias_chain(
        &graph,
        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean)),
        20,
    );
    let (_rctx, _guard) = install_budget(5);
    let identity = engine.fallthrough_override_identity(&[FallthroughPropOverride {
        name: "p".to_string(),
        node: deep,
    }]);
    assert_eq!(
        identity,
        FallthroughOverrideIdentity::Uncacheable,
        "an over-budget override projection must yield Uncacheable, not a partial key"
    );
}

/// #2b — prop order does not matter after uniqueness; the empty set is
/// `NoOverrides`.
#[test]
fn override_identity_is_order_independent_and_canonical() {
    let project = open_project();
    let host = project.host();
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let engine = ComponentMetaQueryEngine::new(ctx);

    let a = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let b = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let forward = engine.fallthrough_override_identity(&[
        FallthroughPropOverride {
            name: "alpha".to_string(),
            node: a,
        },
        FallthroughPropOverride {
            name: "beta".to_string(),
            node: b,
        },
    ]);
    let reversed = engine.fallthrough_override_identity(&[
        FallthroughPropOverride {
            name: "beta".to_string(),
            node: b,
        },
        FallthroughPropOverride {
            name: "alpha".to_string(),
            node: a,
        },
    ]);
    assert_eq!(
        forward, reversed,
        "the same effective overrides in any source order must canonicalize to ONE identity"
    );

    assert_eq!(
        engine.fallthrough_override_identity(&[]),
        FallthroughOverrideIdentity::NoOverrides,
        "the empty override set canonicalizes to NoOverrides"
    );
}

fn build_alias_chain(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    leaf: SemanticNodeId,
    depth: u32,
) -> SemanticNodeId {
    let mut cur = leaf;
    for _ in 0..depth {
        cur = graph.intern_node(SemanticNodeData::Alias(cur));
    }
    cur
}

/// #3 — The consumed-bindings cache key now carries the override identity, so
/// the SAME child branch reached under TWO different override sets keys
/// DISTINCTLY (no wrong reuse). RED on the pre-fix tree: `consumed_bindings_key`
/// hardcoded `override_fingerprint: 0`, so two different override sets produced
/// the SAME key and reused the wrong consumed-bindings.
#[test]
fn consumed_bindings_key_separates_distinct_override_sets() {
    use crate::resolver_core::fallthrough_resolver::consumed_bindings_key;

    let identity_a = FallthroughOverrideIdentity::Exact(Arc::new(FallthroughOverrideSetKey {
        entries: vec![(
            Arc::from("bag"),
            FallthroughOverrideValueKey::Primitive(PrimitiveKind::String),
        )],
    }));
    let identity_b = FallthroughOverrideIdentity::Exact(Arc::new(FallthroughOverrideSetKey {
        entries: vec![(
            Arc::from("bag"),
            FallthroughOverrideValueKey::Primitive(PrimitiveKind::Number),
        )],
    }));

    let key_a = consumed_bindings_key("/App.vue", "0", identity_a.clone());
    let key_b = consumed_bindings_key("/App.vue", "0", identity_b);
    assert_ne!(
        key_a, key_b,
        "the SAME branch under DIFFERENT override sets must key distinctly (no wrong reuse)"
    );

    let key_a_again = consumed_bindings_key("/App.vue", "0", identity_a);
    assert_eq!(
        key_a, key_a_again,
        "the same branch under the SAME override identity keys identically"
    );
}

/// #4 — Recursive-alias characterization: the structural materializer emits a
/// bounded recursive/opaque leaf and the resolution returns WITHOUT stack
/// growth. Reaching the assertions (rather than overflowing/hanging) is the
/// no-stack-growth discriminator; the bounded published surface is the
/// opaque-leaf discriminator.
#[test]
fn recursive_alias_prop_materializes_bounded_leaf_without_stack_growth() {
    let project = open_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export type Tree = { value: string; children: Tree[] }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Tree } from './types'
defineProps<{ root: Tree }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let started = std::time::Instant::now();
    let resolved = project
        .host()
        .resolve_component_meta("/src/App.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved component meta should exist");
    let meta = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        crate::host_manage::extract_component_meta_from_resolved(
            project.host(),
            "/src/App.vue",
            &resolved,
            false,
            ctx,
        )
    });
    let elapsed = started.elapsed();

    // No stack growth / no hang: the recursive alias terminated.
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "recursive alias materialization must not hang (elapsed {:.2}s)",
        elapsed.as_secs_f64()
    );

    // Opaque-leaf: the recursive prop is published as a bounded reference, NOT
    // an infinitely-expanded structure.
    let root = meta
        .props
        .iter()
        .find(|prop| prop.name == "root")
        .expect("root prop should be present");
    let rendered = root.raw_type.clone().unwrap_or_default();
    assert!(
        rendered.len() < 100_000,
        "the recursive alias must publish a bounded leaf, not an unbounded expansion \
         (rendered {} bytes)",
        rendered.len()
    );
}
