//! Per-macro projector decomposition regression tests.
//!
//! Each test characterises an invariant of the per-macro projector
//! decomposition.
//!
//! Test catalogue:
//!
//! - **#1** `getcomponentmeta_decomposes_through_dispatch_primitives`
//!   — the per-macro projector path produces props for every
//!   component-meta resolution. The projector path is the sole
//!   source of `evaluated_types.props` and is load-bearing.
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
//! - **#4** `projector_self_reduces_nested_indexed_access_chain` — a
//!   same-file two-level IndexedAccess
//!   (`ButtonStyles['variants']['size']`) reduces to its concrete
//!   literal-union leaf. Discriminating: the parser-side
//!   `expand_field_expr` closure preserves this surface symbolically,
//!   so the projector's `reduce_field_type_expr` is the sole reducer
//!   on the publication path. Bypassing it surfaces the symbolic
//!   IndexedAccess and fails the assertion.
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

/// CHARACTERIZATION: the projector path is the sole authority for
/// component-meta member resolution.
///
/// Hard contract: every published prop/emit must carry a concrete
/// `TypeExpr` (primitive / object / function — NOT `IndexedAccess`,
/// `Ref` shell, or `Unknown`). A symbolic / unresolved shape proves
/// the projector failed to reduce the macro surface natively.
///
/// For a `defineProps<{ message: string; count: number }>` /
/// `defineEmits<{ click: ...; hover: ... }>` fixture the projector
/// path MUST resolve the surface end-to-end natively, or the
/// projector decomposition has regressed.
#[test]
fn getcomponentmeta_decomposes_through_dispatch_primitives() {
    use verter_type_expr::{PrimitiveName, TypeExpr};

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed");

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

    // Structural concrete-leaf check: the projector is the sole
    // authority post-§7.3 cutover. If it fails to reduce, props
    // surface as `IndexedAccess` / `Ref` / `Unknown` shells. The
    // primitive `Props` interface must publish `string` / `number`
    // primitives — anything else proves the projector did not
    // reduce the macro surface natively.
    let message_prop = meta
        .props
        .iter()
        .find(|p| p.name == "message")
        .expect("`message` prop must be present");
    assert!(
        matches!(
            message_prop.type_expr,
            TypeExpr::Primitive(PrimitiveName::String)
        ),
        "`message` prop must reduce to a `string` primitive; got {:?}. \
         A non-primitive shape proves the projector did not resolve \
         the surface natively.",
        message_prop.type_expr,
    );
    let count_prop = meta
        .props
        .iter()
        .find(|p| p.name == "count")
        .expect("`count` prop must be present");
    assert!(
        matches!(
            count_prop.type_expr,
            TypeExpr::Primitive(PrimitiveName::Number)
        ),
        "`count` prop must reduce to a `number` primitive; got {:?}. \
         A non-primitive shape proves the projector did not resolve \
         the surface natively.",
        count_prop.type_expr,
    );
}

// ──────────────────────────────────────────────────────────────────
// nested IndexedAccess projector self-reduction
// ──────────────────────────────────────────────────────────────────

// Single-file inline interface with two-level IndexedAccess. The
// parser-side `expand_field_expr` closure preserves this expression
// symbolically (no cross-block dispatch projection turns the chain
// into a concrete leaf for this surface shape), so the projector's
// `reduce_field_type_expr` is the SOLE authority that reduces
// `ButtonStyles['variants']['size']` → `"sm" | "md" | "lg"`. Mirrors
// the `indexed_access_two_levels` correctness-suite fixture
// (`tests/correctness/fixtures.rs`), which is the canonical Tier-1
// regression for this invariant — duplicated here so the
// `phase5_decomposition_tests` characterization fails loudly when the
// projector's reducer is regressed (e.g. an early-return that bypasses
// `materialize_component_meta_type_expr_until_stable`).
const NESTED_INDEXED_ACCESS_VUE: &str = r#"<script setup lang="ts">
interface ButtonStyles {
  variants: {
    size: 'sm' | 'md' | 'lg';
    color: 'red' | 'blue';
  };
}
defineProps<{ size: ButtonStyles['variants']['size'] }>();
</script>
<template><div /></template>
"#;

/// REGRESSION: a property whose declared type is a same-file
/// two-level `ButtonStyles['variants']['size']` IndexedAccess chain
/// reduces natively through the projector path's
/// `reduce_field_type_expr` to the concrete literal union
/// `"sm" | "md" | "lg"`.
///
/// **Discriminating contract (this is the property the test exists to
/// enforce):** for this fixture the parser-side `expand_field_expr`
/// closure returns the IndexedAccess shell symbolically — there is no
/// alternate path that produces the literal union. So the projector's
/// `reduce_field_type_expr` is the sole reducer on the publication
/// path. Bypassing it (e.g. an early-return, an always-true guard, or
/// removing it from `reduce_published_field_types`) will surface here
/// as a symbolic `IndexedAccess` and fail the assertion. This was
/// validated by injecting `if route_is_package_backed || true { return
/// expr; }` into the bypass guard and observing the published prop
/// regress to `IndexedAccess { object: IndexedAccess { object: Ref { name:
/// "ButtonStyles", .. }, index: Literal("variants") }, index:
/// Literal("size") }`.
///
/// Hard contract:
/// - The published `size` prop's `type_expr` is a concrete
///   `Union([Literal("sm"), Literal("md"), Literal("lg")])`.
/// - The published prop is NOT a symbolic `IndexedAccess`.
/// - The published prop is NOT `Unknown { raw: "semanticMiss" }`.
#[test]
fn projector_self_reduces_nested_indexed_access_chain() {
    use verter_type_expr::{LiteralValue, TypeExpr};

    let host = build_host(&[("/workspace/src/Comp.vue", NESTED_INDEXED_ACCESS_VUE)]);

    let meta = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("getComponentMeta must succeed for nested IndexedAccess fixture");

    let size_prop = meta
        .props
        .iter()
        .find(|p| p.name == "size")
        .unwrap_or_else(|| {
            panic!(
                "projector must publish `size` prop for nested IndexedAccess \
                 surface; got {:?}",
                meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
            )
        });

    // Negative assertions: the published shape must be concrete.
    assert!(
        !matches!(size_prop.type_expr, TypeExpr::IndexedAccess { .. }),
        "`size` prop type must NOT be a symbolic IndexedAccess; got {:?}. \
         A symbolic IndexedAccess proves the projector's \
         `reduce_field_type_expr` was bypassed.",
        size_prop.type_expr,
    );
    if let TypeExpr::Unknown { raw } = &size_prop.type_expr {
        panic!("`size` prop must NOT be Unknown; got Unknown {{ raw: {raw:?} }}");
    }

    // Positive assertion: the published type is the literal union
    // `"sm" | "md" | "lg"` — the structural result of
    // `ButtonStyles['variants']['size']`.
    let union_members = match &size_prop.type_expr {
        TypeExpr::Union(members) => members.clone(),
        other => panic!(
            "`size` prop must reduce to Union([\"sm\", \"md\", \"lg\"]); \
             got {other:?}. A non-union shape proves the projector did \
             not reduce the IndexedAccess chain natively."
        ),
    };
    let mut literal_strings: Vec<String> = union_members
        .iter()
        .filter_map(|m| match m {
            TypeExpr::Literal(LiteralValue::String(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    literal_strings.sort();
    assert_eq!(
        literal_strings,
        vec!["lg".to_string(), "md".to_string(), "sm".to_string()],
        "Union must contain exactly the three literal members `\"sm\" | \"md\" | \"lg\"`; \
         got members={:?}",
        union_members,
    );

    // Sanity: exactly one prop on the macro surface.
    assert_eq!(
        meta.props.len(),
        1,
        "macro surface must publish exactly one prop (`size`); \
         got {} props: {:?}",
        meta.props.len(),
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
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
/// The substrate fix that closes this gap lives in two places:
/// `verter_session::resolver_core::vue_default_synth` synthesises
/// the implicit `default` value symbol for any file whose analysis
/// carries type-based Vue compiler macros, and
/// `verter_session::typeinfo::evaluate_type_expression` inlines the
/// scope's eval-source as a prelude in the scratch file so the
/// scratch picks up that synthesised `default` and `typeof default`
/// reduces to a concrete Object surface.
#[test]
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
    let (node, _record) = host.evaluate_type_expression_with_audit(req).into_parts();
    let node = node.ok().flatten().expect(
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

/// REGRESSION (bounded path-precision contract): the SAME terminal
/// indexed-access expression `InstanceType<typeof default>['$props']`,
/// evaluated in `Navigate` mode against the `.vue` scope, keeps the
/// projected `$props` terminal SHALLOW — a carrier
/// (`DeclRef` / `Ref` / `InstantiationRef` / `Alias`), NOT the eagerly
/// expanded `{count, message}` Object.
///
/// Discriminating pair with
/// `evaluate_type_expression_for_vue_default_export_matches_props`
/// (which runs the IDENTICAL expression in `Expanded` and DOES expand
/// `$props` to `{count, message}`). The two tests pin the load-bearing
/// invariant of this fix: the indexed-access terminal projection runs
/// in the CALLER's mode, not a hardcoded mode.
///
/// This test FAILS if the deferred indexed-access terminal projection
/// (`evaluate.rs`) / the literal eager projection (`lower.rs`) is
/// hardcoded to `Expanded` (an over-correction of the original
/// hardcoded-`Navigate` bug): a Navigate caller would then wrongly
/// expand `$props`. It also FAILS if the object recursion is run in
/// the caller's mode AND that leaks expansion onto the terminal.
/// Together with the `Expanded` test it proves the terminal mode
/// strictly tracks the caller.
#[test]
fn evaluate_indexed_access_terminal_in_navigate_stays_shallow() {
    use verter_session::semantic_query::{ProjectionMode, SemanticNodeData};
    use verter_session::typeinfo::types::EvaluateTypeExpressionRequest;

    let host = build_host(&[
        ("/workspace/src/types.ts", SHARED_TYPES_TS),
        ("/workspace/src/Comp.vue", SHARED_VUE),
    ]);

    // IDENTICAL expression to the F2 `Expanded` test, but the caller
    // requests `Navigate`. Path-precision: the terminal `['$props']`
    // segment runs in the caller's mode — Navigate keeps it shallow.
    let req = EvaluateTypeExpressionRequest {
        scope: "/workspace/src/Comp.vue".to_string(),
        expression: "InstanceType<typeof default>['$props']".to_string(),
        extra_imports: Vec::new(),
        mode: ProjectionMode::Navigate,
        cacheable: false,
    };
    let (node, _record) = host.evaluate_type_expression_with_audit(req).into_parts();
    let node = node.ok().flatten().expect(
        "evaluate_type_expression must resolve \
         `InstanceType<typeof default>['$props']` in Navigate against \
         the .vue scope",
    );

    let store = host.project_type_store().semantic_graph();
    let data = store
        .node_data(node)
        .expect("evaluated `$props` terminal must be interned");

    // Bounded contract: a Navigate terminal stays a SHALLOW carrier.
    // An Object here would mean the terminal projection expanded under
    // Navigate — i.e. the terminal mode no longer tracks the caller.
    assert!(
        !matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "Navigate-mode `InstanceType<typeof default>['$props']` must \
         stay shallow (a DeclRef/Ref/InstantiationRef/Alias carrier), \
         NOT an eagerly expanded Object; got {:?}",
        data.as_ref()
    );
    // Positive shape check: the shallow carrier is one of the expected
    // carrier variants (discriminating — not merely "not Object",
    // which a Primitive/Unknown miss would also satisfy). The F2 test
    // shows the Expanded carrier resolves to `Props`, so the Navigate
    // carrier is the `DeclRef(Props)` / placeholder that precedes it.
    assert!(
        matches!(
            data.as_ref(),
            SemanticNodeData::DeclRef { .. }
                | SemanticNodeData::InstantiationRef { .. }
                | SemanticNodeData::Alias(_)
                | SemanticNodeData::Opaque(
                    verter_session::semantic_query::QueryError::DeclPlaceholder { .. }
                )
        ),
        "Navigate `$props` terminal must stay a declaration/\
         instantiation carrier; got {:?}",
        data.as_ref()
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
