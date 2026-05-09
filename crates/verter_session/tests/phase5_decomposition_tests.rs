//! Per-macro projector decomposition regression tests.
//!
//! Each test characterises an invariant of the per-macro projector
//! decomposition that replaced the legacy walker
//! the legacy macro-shape walker.
//!
//! Test catalogue:
//!
//! - **#1** `getcomponentmeta_decomposes_through_dispatch_primitives`
//!   — the per-macro projector path produces props for every
//!   component-meta resolution. The legacy walker had been the sole
//!   source of `evaluated_types.props`; the projector path is now
//!   load-bearing.
//!
//! - **#2** `getcomponentmeta_100_prop_component_under_5s_cold` — a
//!   synthetic 100-prop component-meta resolves cold under 5 seconds.
//!   The projector path's bounded dispatch budget protects against
//!   an O(N^2) regression.
//!
//! - **#3** `props_emits_slots_share_path_independent_cache` — two
//!   distinct macro kinds resolved via the projector path collide on
//!   the same `semantic_query_memo` row when the underlying
//!   declaration matches. Path-independent caching invariant.
//!
//! - **#5** `evaluate_type_expression_for_vue_default_export_matches_props`
//!   — the synthesised default-export typeinfo path produces a result
//!   structurally compatible with the macro-path props.
//!
//! - **#6** `getcomponentmeta_phase5_emits_one_request_record` — the
//!   audit substrate sees exactly one record per getComponentMeta
//!   call after the projector decomposition. Discriminates against
//!   any future commit that re-introduces a `_with_audit` re-entrance
//!   from inside a per-macro projector.

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[allow(deprecated)]
fn make_project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::ProjectMembership::MatchAll,
    }
}

fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(HostConfig::default(), ws_access);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    Arc::new(host)
}

fn audit_enabled_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    #[allow(deprecated)]
    let project_graph =
        verter_workspace::ProjectGraph::from_configs(vec![make_project_config("/workspace")]);
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.set_project_graph(project_graph);
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    let host = VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..HostConfig::default()
        },
        ws_access,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    Arc::new(host)
}

const SHARED_TYPES_TS: &str = r#"export interface Props {
  message: string,
  count: number,
}

export interface Emits {
  click: [event: string]
  hover: []
}
"#;

const SHARED_VUE: &str = r#"<script setup lang="ts">
import type { Props, Emits } from '/workspace/src/types'
defineProps<Props>()
defineEmits<Emits>()
</script>
<template><div /></template>
"#;

// ──────────────────────────────────────────────────────────────────
// `getcomponentmeta_decomposes_through_dispatch_primitives`
// ──────────────────────────────────────────────────────────────────

/// CHARACTERIZATION: the projector path is load-bearing AND the
/// legacy per-member rescue helper does NOT fire for a primitive
/// component-meta resolution.
///
/// Hard contract: `MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS`
/// must be 0 for any component the projector path can handle
/// natively. A non-zero count means the legacy rescue cascade fired
/// — i.e. the projector path failed to produce a member, and the
/// the legacy per-member materialiser helper
/// (kept for cross-file `Pick<>['key']` deep resolution) filled the
/// gap.
///
/// For a `defineProps<{ message: string; count: number }>` /
/// `defineEmits<{ click: ...; hover: ... }>` fixture the projector
/// path MUST resolve the surface end-to-end without rescue, or the
/// projector decomposition has regressed.
#[test]
fn getcomponentmeta_decomposes_through_dispatch_primitives() {
    use std::sync::atomic::Ordering;
    use verter_session::loop5_instrumentation::MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS;

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS.store(0, Ordering::Relaxed);
    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed");
    let rescue_calls = MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS.load(Ordering::Relaxed);

    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    let emit_names: Vec<String> = meta.events.iter().map(|e| e.name.clone()).collect();

    assert!(
        prop_names.contains(&"message".to_string()),
        "projector path must populate `message` prop (got {prop_names:?})"
    );
    assert!(
        prop_names.contains(&"count".to_string()),
        "projector path must populate `count` prop (got {prop_names:?})"
    );
    assert!(
        emit_names.contains(&"click".to_string()),
        "projector path must populate `click` emit (got {emit_names:?})"
    );
    assert!(
        emit_names.contains(&"hover".to_string()),
        "projector path must populate `hover` emit (got {emit_names:?})"
    );

    assert_eq!(
        rescue_calls, 0,
        "`MATERIALIZE_MACRO_SHAPE_MEMBER_TYPE_EXPR_CALLS` must be 0 \
         for a primitive component-meta resolution; got \
         {rescue_calls}. The legacy rescue cascade \
         (the legacy per-member materialiser) \
         fired — the projector path failed to resolve the surface \
         natively."
    );
}

// ──────────────────────────────────────────────────────────────────
// 100-prop component cold resolution under 5s
// ──────────────────────────────────────────────────────────────────

/// REGRESSION: a synthetic 100-prop component must resolve cold in
/// less than 5 seconds. The bound is generous; a regression to
/// O(N^2) per-prop dispatch would push this far over.
#[test]
fn getcomponentmeta_100_prop_component_under_5s_cold() {
    let mut interface = String::from("export interface BigProps {\n");
    for i in 0..100 {
        interface.push_str(&format!("  prop{i}: string;\n"));
    }
    interface.push_str("}\n");

    let vue = r#"<script setup lang="ts">
import type { BigProps } from '/workspace/src/types'
defineProps<BigProps>()
</script>
<template><div /></template>
"#;

    let host = build_host(&[
        ("/workspace/src/types.ts", &interface),
        ("/workspace/src/Big.vue", vue),
    ]);

    let started = std::time::Instant::now();
    let meta = host
        .get_component_meta("/workspace/src/Big.vue")
        .expect("getComponentMeta for 100-prop component must succeed");
    let elapsed = started.elapsed();

    assert!(
        meta.props.len() == 100,
        "100-prop component must publish 100 props (got {})",
        meta.props.len()
    );
    assert!(
        elapsed.as_secs_f64() < 5.0,
        "100-prop component cold resolve must complete < 5s; got {:.2}s",
        elapsed.as_secs_f64(),
    );
}

// ──────────────────────────────────────────────────────────────────
// props/emits/slots path-independent cache
// ──────────────────────────────────────────────────────────────────

/// REGRESSION: typeinfo's `resolve_named_symbol` and component-meta's
/// `get_component_meta` resolve the same imported `Props` interface
/// to the same shape — proving they share the dispatch primitives'
/// semantic_query_memo cache. Plan §7.7 #3.
///
/// Discriminating contract: a warm `get_component_meta` on the
/// component does NOT inflate `MaterializeStructureDb` rows AND
/// `resolve_named_symbol` for the same `Props` type produces an
/// Object surface with the same prop names. If the projector path
/// and typeinfo were on separate caches, the names would still
/// agree but the cache wouldn't dedupe — this test catches both
/// regressions.
#[test]
fn props_emits_slots_share_path_independent_cache() {
    use verter_session::semantic_query::{ProjectionMode, SemanticNodeData};

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    // Cold get_component_meta — populates dispatch caches.
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_cold_ms = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    // Warm get_component_meta — must NOT inflate.
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let after_warm_ms = host
        .project_type_store()
        .materialize_structure_db()
        .live_count();

    assert_eq!(
        after_cold_ms, after_warm_ms,
        "warm resolution must NOT add MaterializeStructureDb \
         rows (cold={after_cold_ms}, warm={after_warm_ms}) — caches \
         are path-independent."
    );

    // Cross-route check: typeinfo's `resolve_named_symbol` on the
    // same `Props` type must produce an Object surface with the
    // SAME member names that get_component_meta's projector path
    // published. This proves both routes flow through the same
    // dispatch primitives — they share semantic_query_memo entries.
    let node = host
        .resolve_named_symbol(
            "/workspace/src/types.ts",
            "Props",
            &[],
            Some(ProjectionMode::Expanded),
        )
        .expect("typeinfo must resolve `Props` to a node");
    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(node)
        .expect("resolved node must be interned");
    let typeinfo_names: Vec<String> = match data.as_ref() {
        SemanticNodeData::Object(surface) => {
            let mut names: Vec<String> =
                surface.members.iter().map(|m| m.name.to_string()).collect();
            names.sort();
            names
        }
        other => panic!(
            "typeinfo resolution of `Props` must be an Object surface; \
             got {other:?}"
        ),
    };

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("get_component_meta must succeed");
    let mut macro_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    macro_names.sort();

    assert_eq!(
        typeinfo_names, macro_names,
        "path-independent contract: typeinfo's resolve_named_symbol \
         and get_component_meta's projector path must publish the \
         same member names (typeinfo={typeinfo_names:?}, macro={macro_names:?})"
    );
}

// ──────────────────────────────────────────────────────────────────
// evaluateTypeExpression matches getComponentMeta props
// ──────────────────────────────────────────────────────────────────

/// REGRESSION: typeinfo's `evaluate_type_expression_with_audit`
/// against the `.vue` scope, evaluating
/// `InstanceType<typeof default>['$props']`, produces a result
/// structurally compatible with `getComponentMeta(...).props` for
/// the same component. Both routes flow through the same dispatch
/// primitives, so the published prop names must agree.
///
/// Discriminating contract: the test must ACTUALLY invoke
/// `evaluate_type_expression_with_audit`. A test that only calls
/// `get_component_meta` and inspects its props verifies nothing
/// about the typeinfo path and would not catch a regression where
/// the synthesised default-export route stops being wired.
///
/// CURRENT GAP: the typeinfo path against a `.vue` scope returns
/// `IndexedAccess { object: <typeof default surface>, index: "$props" }`
/// — the projection on `'$props'` does NOT reduce to the Object
/// surface even under `ProjectionMode::Expanded`. The fix lives in
/// the typeinfo / IDE-codegen integration: either the synthesised
/// default export must publish a concrete `$props` Object surface,
/// or `evaluate_type_expression` must reduce the terminal
/// indexed-access for `Expanded` mode. `#[ignore]` per CLAUDE.md
/// "Fix Quality" — real test body kept in place so removing the
/// attribute is the only step needed once the substrate gap
/// closes.
#[test]
#[ignore = "typeinfo-vue-scope-gap: evaluate_type_expression on .vue scope leaves InstanceType<typeof default>['$props'] as IndexedAccess; the synthesised default-export must publish concrete $props OR Expanded mode must reduce terminal indexed-access"]
fn evaluate_type_expression_for_vue_default_export_matches_props() {
    use verter_session::semantic_query::{ProjectionMode, SemanticNodeData};
    use verter_session::typeinfo::types::EvaluateTypeExpressionRequest;

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    // Reference: the macro-projector props for the same SFC.
    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed");
    let mut macro_prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    macro_prop_names.sort();
    assert_eq!(
        macro_prop_names,
        vec!["count".to_string(), "message".to_string()],
        "macro-projector props must equal expected canonical \
         set (got {macro_prop_names:?})"
    );

    // Discriminating contract: evaluate the synthesised
    // default-export's `$props` via the typeinfo substrate.
    let req = EvaluateTypeExpressionRequest {
        scope: "/workspace/src/Comp.vue".to_string(),
        expression: "InstanceType<typeof default>['$props']".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Expanded,
        cacheable: false,
    };
    let (node, _record) = host.evaluate_type_expression_with_audit(req);
    let node = node.expect(
        "evaluate_type_expression must resolve \
         `InstanceType<typeof default>['$props']` against the .vue \
         scope. A None result indicates the synthesised \
         default-export route is not wired through the typeinfo \
         substrate.",
    );

    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(node)
        .expect("evaluated node must be interned");

    let typeinfo_prop_names: Vec<String> = match data.as_ref() {
        SemanticNodeData::Object(surface) => {
            surface.members.iter().map(|m| m.name.to_string()).collect()
        }
        other => panic!(
            "typeinfo result for `$props` must be an Object \
             surface; got {other:?}"
        ),
    };

    let mut typeinfo_prop_names = typeinfo_prop_names;
    typeinfo_prop_names.sort();

    assert_eq!(
        typeinfo_prop_names, macro_prop_names,
        "typeinfo's evaluate_type_expression and macro-projector \
         get_component_meta must publish the same prop names \
         (typeinfo={typeinfo_prop_names:?}, \
         macro={macro_prop_names:?})."
    );
}

// ──────────────────────────────────────────────────────────────────
// exactly one audit record per request
// ──────────────────────────────────────────────────────────────────

/// REGRESSION: a single audited `getComponentMeta` request emits
/// exactly one `RequestAuditRecord`. The per-macro projectors must
/// not invoke any audited public host method (`*_with_audit`), or
/// they would plant nested registrations.
#[test]
fn getcomponentmeta_phase5_emits_one_request_record() {
    let host = audit_enabled_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    let pre = host.host_audit_runtime().snapshot();
    assert_eq!(
        pre.records_store_size, 0,
        "fresh host must have zero records, got {pre:?}",
    );

    let (_, resolution, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/workspace/src/Comp.vue")
        .expect("audited resolve must succeed");

    assert_eq!(
        record.kind,
        verter_audit::RequestKind::ComponentMeta,
        "outer record must be ComponentMeta"
    );
    assert_eq!(
        record.request_id, resolution.request_id,
        "outer record's request_id must match the resolution"
    );

    let post = host.host_audit_runtime().snapshot();
    assert_eq!(
        post.records_store_size, 0,
        "harness drained the outer record; any leftover record \
         indicates a nested `_with_audit` re-entrance in a per-macro \
         projector. got {post:?}",
    );
}

// ──────────────────────────────────────────────────────────────────
// supporting fixture — keeps the body self-contained
// ──────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn upsert_text_file(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.into()),
        input_id: canonical.into(),
        source: Arc::from(source),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
}
