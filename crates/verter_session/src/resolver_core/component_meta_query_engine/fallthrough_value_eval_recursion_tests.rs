//! Discriminating tests for the fallthrough node-DAG recursion bound + the
//! wholesale-uncacheable override identity.
//!
//! These exercise: (1) the persistent-memo + shared op-budget that bounds the
//! `known_spread` / `dynamic-root` node walkers over a content-interned diamond;
//! (3) the wholesale-uncacheable override identity on the consumed-bindings key
//! (an override-bearing key is `Uncacheable`, so it is never stored or
//! warm-reused, while the no-override key stays cacheable); (4) a recursive-alias
//! characterization of the PRE-EXISTING cycle-sentinel termination invariant.

use std::sync::Arc;

use super::{collect_dynamic_root_candidates_from_node, known_spread_keys_from_node};
use crate::meta::MetaProject;
use crate::request_context::{RequestContext, RequestContextGuard};
use crate::resolver_core::{
    FallthroughOverrideIdentity, FallthroughPropOverride, FallthroughPropOverrideSet,
};
use crate::semantic_query::{SemanticNodeData, SemanticNodeId, SurfaceView};
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

/// #3 — Override-bearing consumed-bindings is wholesale uncacheable: a
/// consumed-bindings cache key whose override identity is derived (via
/// `for_overrides`) from ANY non-empty override set is `Uncacheable`, so it is
/// never stored, looked up, or warm-reused; the no-override key stays cacheable
/// and differs from it.
///
/// Discriminates Part 1's `for_overrides`: were a non-empty override set to map
/// to a cacheable identity (the pre-change exact-key behavior), the
/// override-bearing key would report `is_cacheable()` and the first assertion
/// would FAIL.
#[test]
fn consumed_bindings_key_is_uncacheable_for_override_bearing() {
    use crate::resolver_core::fallthrough_resolver::consumed_bindings_key;

    let overrides = FallthroughPropOverrideSet {
        entries: vec![FallthroughPropOverride {
            name: "bag".to_string(),
            node: SemanticNodeId(3),
        }],
    };
    let override_key = consumed_bindings_key(
        "/App.vue",
        "0",
        FallthroughOverrideIdentity::for_overrides(Some(&overrides)),
    );
    assert!(
        !override_key.is_cacheable(),
        "an override-bearing consumed-bindings key is wholesale uncacheable — never stored or warm-reused"
    );

    let no_override = consumed_bindings_key(
        "/App.vue",
        "0",
        FallthroughOverrideIdentity::for_overrides(None),
    );
    assert!(
        no_override.is_cacheable(),
        "the no-override consumed-bindings key stays cacheable"
    );
    assert_ne!(
        override_key, no_override,
        "the override-bearing key differs from the no-override key"
    );
}

/// #4 — Recursive-alias characterization of the PRE-EXISTING cycle-sentinel
/// termination invariant (the `structural_materialize` `active` Navigate-node
/// sentinel), NOT this change's walker memo: a recursive alias prop materializes
/// a bounded recursive/opaque leaf and the resolution returns WITHOUT stack
/// growth. Reaching the assertions (rather than overflowing/hanging) is the
/// no-stack-growth discriminator; the bounded published surface is the
/// opaque-leaf discriminator. It would pass on the parent tree too — it guards
/// the durable termination invariant, not this commit's mechanism.
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
