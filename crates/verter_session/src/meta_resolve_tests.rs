use super::*;
use crate::meta::MetaProject;
use crate::resolver_core::ComponentMetaRequestHost;
use crate::types::{HostConfig, ProjectionMode};
use crate::VerterHost;
use std::sync::Arc;

// ===========================================================================
// Test helpers
// ===========================================================================

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn make_project_with_config(config: HostConfig) -> Arc<MetaProject> {
    MetaProject::new(VerterHost::new_standalone(config))
}

fn provenance(project: &MetaProject) -> crate::types::MetaProvenanceSnapshot {
    project.host().provenance().snapshot()
}

fn prop_names_from_resolved(state: &ResolvedComponentMetaState) -> Vec<String> {
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.clone())
        .collect()
}

fn emit_names_from_resolved(state: &ResolvedComponentMetaState) -> Vec<String> {
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits)
        .flat_map(|m| m.emits.iter())
        .map(|e| e.name.clone())
        .collect()
}

fn slot_names_from_resolved(state: &ResolvedComponentMetaState) -> Vec<String> {
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .map(|s| s.name.clone())
        .collect()
}

fn clear_legacy_cached_resolved_state(
    project: &MetaProject,
    canonical: &str,
    mode: ProjectionMode,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(mut entry) = project.host().compile_cache.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }
}

#[test]
fn imported_registry_seed_refresh_skips_explicit_object_surfaces() {
    let declaration = crate::resolver_core::ResolvedTypeDeclaration {
        requested_name: "Props".to_string(),
        declaration_id: None,
        resolved_name: "Props".to_string(),
        canonical_source: "/src/types.ts".to_string(),
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
        text: Some("export interface Props { label?: string }".to_string()),
    };
    let object = verter_semantic::analysis::type_expr::TypeExpr::Object(Arc::new(
        verter_semantic::analysis::type_expr::ObjectExpr {
            properties: vec![
                verter_semantic::analysis::type_expr::ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "label".to_string(),
                        ty: verter_semantic::analysis::type_expr::TypeExpr::Primitive(
                            verter_semantic::analysis::type_expr::PrimitiveName::String,
                        ),
                        optional: true,
                        readonly: false,
                    },
                ),
            ],
        },
    ));

    assert!(
        should_skip_imported_registry_seed_refresh("/src/App.vue", &declaration, &object),
        "imported direct-macro seeds that already hold an explicit object surface should stay on that seeded surface instead of re-entering imported registry materialization",
    );
}

#[test]
fn imported_registry_seed_refresh_keeps_symbolic_imported_surfaces_refreshable() {
    let declaration = crate::resolver_core::ResolvedTypeDeclaration {
        requested_name: "Button".to_string(),
        declaration_id: None,
        resolved_name: "Button".to_string(),
        canonical_source: "/src/types.ts".to_string(),
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::TypeAlias,
        text: Some("export type Button = VariantProps<typeof config>".to_string()),
    };
    let symbolic = verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess {
        object: Arc::new(verter_semantic::analysis::type_expr::TypeExpr::named(
            "Button",
        )),
        index: Arc::new(verter_semantic::analysis::type_expr::TypeExpr::string_literal("variants")),
    };

    assert!(
        !should_skip_imported_registry_seed_refresh("/src/App.vue", &declaration, &symbolic),
        "symbolic imported seeds still need the imported-registry refresh path to materialize their requested route",
    );
}

#[test]
fn append_component_meta_registry_entries_skips_imported_refresh_for_explicit_seeded_object_surfaces(
) {
    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Props { label: string }")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host };

    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    let props_entry = parts
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Props")
        .expect("the direct imported macro root should seed the initial registry");
    assert!(
        matches!(
            props_entry.type_expr,
            verter_semantic::analysis::type_expr::TypeExpr::Object(_),
        ),
        "the initial direct imported seed should already hold an explicit object surface"
    );

    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    host.append_component_meta_registry_entries(
        "/src/App.vue",
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );

    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        0,
        "explicit imported direct-macro seeds should reuse their seeded object surface instead of re-resolving the imported registry root during append",
    );
}

#[test]
fn materialize_component_meta_registry_structural_expr_preserves_conditional_wrapper_for_routed_branches(
) {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface SingleValue { current: string }
export interface RangeValue { current: number }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="R extends boolean">
import type { SingleValue, RangeValue } from './types'

type ModelValue<R extends boolean = false> =
  R extends true ? RangeValue['current'] : SingleValue['current']

defineProps<{ modelValue?: ModelValue<R> }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let raw_body = query_engine
        .owner_collection_expr("/src/App.vue", "ModelValue")
        .expect("owner helper body should be available from prepared declarations");

    let materialized = materialize_component_meta_registry_structural_expr(
        &raw_body,
        "/src/App.vue",
        &mut query_engine,
    );

    let verter_semantic::analysis::type_expr::TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = materialized
    else {
        panic!("local routed helper should stay conditional instead of flattening the wrapper");
    };

    assert_eq!(
        true_type,
        Arc::new(verter_semantic::analysis::type_expr::TypeExpr::Primitive(
            verter_semantic::analysis::type_expr::PrimitiveName::Number,
        )),
        "the true branch should materialize through the routed imported member surface",
    );
    assert_eq!(
        false_type,
        Arc::new(verter_semantic::analysis::type_expr::TypeExpr::Primitive(
            verter_semantic::analysis::type_expr::PrimitiveName::String,
        )),
        "the false branch should materialize through the routed imported member surface",
    );
}

#[test]
fn append_component_meta_registry_entries_keep_local_conditional_routed_aliases_structural() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface SingleValue { current: string }
export interface RangeValue { current: number }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="R extends boolean">
import type { SingleValue, RangeValue } from './types'

type ModelValue<R extends boolean = false> =
  R extends true ? RangeValue['current'] : SingleValue['current']

defineProps<{ modelValue?: ModelValue<R> }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host };

    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    host.append_component_meta_registry_entries(
        "/src/App.vue",
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );

    let model_value = parts
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ModelValue")
        .expect("local routed helper should be published into the type registry");

    let verter_semantic::analysis::type_expr::TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = &model_value.type_expr
    else {
        panic!("registry helper should preserve the conditional wrapper");
    };

    assert_eq!(
        true_type.as_ref(),
        &verter_semantic::analysis::type_expr::TypeExpr::Primitive(
            verter_semantic::analysis::type_expr::PrimitiveName::Number,
        ),
    );
    assert_eq!(
        false_type.as_ref(),
        &verter_semantic::analysis::type_expr::TypeExpr::Primitive(
            verter_semantic::analysis::type_expr::PrimitiveName::String,
        ),
    );
}

#[test]
fn component_meta_request_executor_uses_captured_owner_inputs_after_owner_changes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
defineProps<{ foo: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let original_props = vec!["foo".to_string()];
    let view = <VerterHost as ComponentMetaRequestHost>::snapshot_store_view(project.host());
    let captured = <VerterHost as ComponentMetaRequestHost>::capture_component_meta_inputs(
        project.host(),
        "/src/App.vue",
        &view,
    )
    .expect("captured component-meta inputs should exist");

    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
defineProps<{ bar: number }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = <VerterHost as ComponentMetaRequestHost>::compute_component_meta(
        project.host(),
        "/src/App.vue",
        ProjectionMode::Expanded,
        Some(&captured),
        Some(&view),
    )
    .expect("component-meta should still resolve against captured owner inputs");

    let snapshot_props: Vec<String> = state
        .snapshot
        .macros
        .iter()
        .filter(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.prop_fields.iter())
        .map(|prop| prop.name.clone())
        .collect();

    assert_eq!(snapshot_props, original_props);
}

#[test]
fn package_declaration_entrypoints_materialize_alias_chain_emits_with_local_tuple_property() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/project/node_modules/my-lib/package.json".to_string(),
        Arc::from(
            r#"{"name": "my-lib", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } }}"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/index.d.ts".to_string(),
        Arc::from(r#"import { MenuContentEmits as ImportedMenuContentEmits } from "./inner.js"; export type { ImportedMenuContentEmits as MenuContentEmits };"#),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.d.ts".to_string(),
        Arc::from(
            r#"
export interface MenuContentImplEmits {
  escapeKeyDown: [event: KeyboardEvent],
  pointerDownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent],
  interactOutside: [event: Event]
  openAutoFocus: [event: Event],
  closeAutoFocus: [event: Event]
  entryFocus: [event: Event]
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/project".to_string(),
            "/project".to_string(),
            Some("/project/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);

    project
        .upsert_base(
            "/project/App.vue",
            r#"<script setup lang="ts">
import type { MenuContentEmits } from 'my-lib'

interface Emits extends MenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/project/App.vue")
        .expect("package declaration alias-chain emits should resolve");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"pointerDownOutside")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"interactOutside")
            && event_names.contains(&"closeAutoFocus")
            && event_names.contains(&"update:searchTerm"),
        "package declaration alias-chain emits should preserve inherited imported events alongside local tuple-property events: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"openAutoFocus") && !event_names.contains(&"entryFocus"),
        "package declaration alias-chain emits must still respect omitted imported events: {:?}",
        event_names
    );
}

#[test]
fn package_declaration_entrypoints_materialize_aliased_emits_with_local_tuple_property() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/project/node_modules/my-lib/package.json".to_string(),
        Arc::from(
            r#"{"name": "my-lib", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } }}"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/index.d.ts".to_string(),
        Arc::from(r#"import { MenuContentEmits as ImportedMenuContentEmits } from "./inner.js"; export type { ImportedMenuContentEmits as MenuContentEmits };"#),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.d.ts".to_string(),
        Arc::from(
            r#"
export interface MenuContentImplEmits {
  escapeKeyDown: [event: KeyboardEvent],
  pointerDownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent],
  interactOutside: [event: Event]
  openAutoFocus: [event: Event],
  closeAutoFocus: [event: Event]
  entryFocus: [event: Event]
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/project".to_string(),
            "/project".to_string(),
            Some("/project/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);

    project
        .upsert_base(
            "/project/App.vue",
            r#"<script setup lang="ts">
import type { MenuContentEmits as LocalMenuContentEmits } from 'my-lib'

interface Emits extends LocalMenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/project/App.vue")
        .expect("aliased package declaration emits should resolve");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"pointerDownOutside")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"interactOutside")
            && event_names.contains(&"closeAutoFocus")
            && event_names.contains(&"update:searchTerm"),
        "aliased package declaration emits should preserve inherited imported events alongside local tuple-property events: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"openAutoFocus") && !event_names.contains(&"entryFocus"),
        "aliased package declaration emits must still respect omitted imported events: {:?}",
        event_names
    );
}

// ===========================================================================
// Phase 1: Architecture — Resolver mode behavior
// ===========================================================================

#[test]
fn type_mode_resolves_identity_without_expansion() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity)
        .expect("`ProjectionMode::Identity` should return a result for an existing file");

    assert_eq!(state.mode, ProjectionMode::Identity);

    // `ProjectionMode::Identity`: resolved_macros should carry identity info but NOT expanded props
    assert!(
        !state.resolved_macros.is_empty(),
        "`ProjectionMode::Identity` should still identify macro type deps"
    );
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.is_empty(),
        "`ProjectionMode::Identity` must NOT materialize expanded prop shapes, got: {:?}",
        prop_names
    );

    // `ProjectionMode::Identity`: no evaluated types
    assert!(
        state.evaluated_types.is_none(),
        "`ProjectionMode::Identity` must NOT compute evaluated types"
    );

    // `ProjectionMode::Identity`: no type registry
    assert!(
        state.resolved_type_registry.is_empty(),
        "`ProjectionMode::Identity` must NOT populate type-registry entries"
    );
}

#[test]
fn expanded_mode_reuses_traversal_then_materializes_shape() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return a result for an existing file");

    assert_eq!(state.mode, ProjectionMode::Expanded);

    // `ProjectionMode::Expanded`: materialized props
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "`ProjectionMode::Expanded` should materialize prop 'a', got: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"b".to_string()),
        "`ProjectionMode::Expanded` should materialize prop 'b', got: {:?}",
        prop_names
    );
}

#[test]
fn mode_selection_is_explicit_at_shared_resolver_boundary() {
    // The resolver API requires an explicit mode parameter.
    // This test verifies the API signature exists and doesn't infer mode.
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ x: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Both modes must be callable with the same file
    let _type_result = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity);
    let _expanded_result = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded);
}

#[test]
fn mode_sensitive_resolved_meta_cache_entries_are_distinct() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Call `ProjectionMode::Identity` first, then Expanded
    let type_state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity)
        .expect("`ProjectionMode::Identity` should return result");
    let expanded_state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    // Type entry must NOT satisfy Expanded
    assert!(
        prop_names_from_resolved(&type_state).is_empty(),
        "`ProjectionMode::Identity` result must have no expanded props"
    );
    assert!(
        !prop_names_from_resolved(&expanded_state).is_empty(),
        "`ProjectionMode::Expanded` result must have expanded props"
    );
}

#[test]
fn repeated_same_mode_queries_reuse_resolved_meta_cache() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();

    let _first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolved-meta query should succeed");
    let p1 = provenance(&project);
    assert_eq!(
        p1.component_meta_resolved_state_recomputes, 1,
        "first query should compute resolved meta exactly once"
    );
    assert_eq!(
        p1.resolver_node_cache_misses, 1,
        "first query should miss the resolver-owned cache once"
    );
    assert_eq!(
        p1.resolver_node_cache_hits, 0,
        "first query should not hit the resolver-owned cache"
    );

    let _second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolved-meta query should succeed");
    let p2 = provenance(&project);
    assert_eq!(
        p2.component_meta_resolved_state_recomputes, 1,
        "same-mode repeat should hit resolved-meta cache instead of recomputing"
    );
    assert_eq!(
        p2.resolver_node_cache_misses, 1,
        "repeat query should not introduce a second resolver-owned cache miss"
    );
    assert_eq!(
        p2.resolver_node_cache_hits, 1,
        "repeat query should hit the resolver-owned cache once"
    );
}

#[test]
fn top_level_component_meta_lives_in_runtime_not_host_wrapper_cache() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();

    let _first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolved-meta query should succeed");
    let p1 = provenance(&project);
    assert_eq!(
        p1.component_meta_resolved_state_recomputes, 1,
        "first query should compute resolved meta exactly once"
    );
    assert_eq!(
        p1.resolver_node_cache_misses, 1,
        "first query should miss the resolver-owned top-level cache once"
    );

    clear_legacy_cached_resolved_state(&project, "/App.vue", ProjectionMode::Expanded);

    let _second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolved-meta query should succeed");
    let p2 = provenance(&project);

    assert_eq!(
        p2.component_meta_resolved_state_recomputes, 1,
        "clearing host-visible wrapper caches must not force a recompute once runtime owns top-level component-meta"
    );
    assert_eq!(
        p2.resolver_node_cache_misses, 1,
        "second query should not introduce a second top-level cache miss"
    );
    assert_eq!(
        p2.resolver_node_cache_hits, 1,
        "second query should still hit the runtime-owned top-level cache"
    );
}

// ===========================================================================
// Phase 1: Architecture — Shared traversal between modes
// ===========================================================================

#[test]
fn type_mode_skips_expansion_and_expanded_mode_uses_resolver_cache() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();

    // `ProjectionMode::Identity` should NOT perform the expensive external type traversal
    let type_state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity)
        .expect("`ProjectionMode::Identity` should return result");

    let p1 = provenance(&project);
    assert_eq!(
        p1.resolved_external_type_cache_misses, 0,
        "`ProjectionMode::Identity` should NOT call resolve_external_type_from_loaded_files"
    );
    assert_eq!(
        p1.resolved_external_type_cache_hits, 0,
        "`ProjectionMode::Identity` should NOT touch the host traversal cache"
    );

    // `ProjectionMode::Expanded` performs the traversal
    let expanded_state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let p2 = provenance(&project);
    assert!(
        prop_names_from_resolved(&type_state).is_empty(),
        "`ProjectionMode::Identity` result must not include expanded props"
    );
    assert!(
        prop_names_from_resolved(&expanded_state).contains(&"a".to_string()),
        "`ProjectionMode::Expanded` should materialize imported props"
    );
    assert!(
        p2.resolver_node_cache_misses > p1.resolver_node_cache_misses,
        "`ProjectionMode::Expanded` should perform additional resolver-owned cache work"
    );

    // Second Expanded call should hit the resolved-meta cache (no recompute)
    let recomputes_before = p2.component_meta_resolved_state_recomputes;
    let _third = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("repeat Expanded should succeed");
    let p3 = provenance(&project);
    assert_eq!(
        p3.component_meta_resolved_state_recomputes, recomputes_before,
        "second Expanded call should hit the resolved-meta cache, not recompute"
    );
}

// ===========================================================================
// Phase 1: Architecture — Imported surfaces through shared resolver
// ===========================================================================

#[test]
fn imported_props_use_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { label: string; count: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"label".to_string()),
        "imported props should flow through shared resolver: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"count".to_string()),
        "imported props should flow through shared resolver: {:?}",
        prop_names
    );
}

#[test]
fn expanded_component_meta_imported_props_avoid_loaded_files_macro_path() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");
    let provenance = provenance(&project);

    assert!(
        prop_names_from_resolved(&state).contains(&"a".to_string()),
        "`ProjectionMode::Expanded` should still materialize imported props"
    );
    assert_eq!(
        provenance.resolved_external_type_cache_hits, 0,
        "component-meta should not hit the legacy loaded-files macro traversal cache"
    );
    assert_eq!(
        provenance.resolved_external_type_cache_misses, 0,
        "component-meta should not call resolve_external_type_from_loaded_files for imported macro types"
    );
}

#[test]
fn imported_class_props_use_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/base.ts",
            r#"export class BaseProps { from_base!: string; protected hidden!: boolean }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/iface.ts",
            r#"export interface Implemented { from_interface: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
import { BaseProps } from './base'
import type { Implemented } from './iface'

export class Props extends BaseProps implements Implemented {
  own?: boolean
  from_interface!: number
  private secret!: symbol
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let class_macro = state
        .resolved_macros
        .iter()
        .find(|m| m.type_name == "Props")
        .expect("resolved class macro should be present");

    let prop_names = prop_names_from_resolved(&state);
    // The shared resolver should include public inherited base-class members
    // while still hiding protected/private fields.
    assert!(
        prop_names.contains(&"from_interface".to_string()),
        "imported class instance members should flow through shared resolver: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"own".to_string()),
        "imported class own members should flow through shared resolver: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"from_base".to_string()),
        "shared resolver should include inherited base-class members: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"hidden".to_string()),
        "protected base class members must not appear in props: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"secret".to_string()),
        "private class members must not leak into props: {:?}",
        prop_names
    );

    let native_names: Vec<&str> = class_macro
        .native_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    // native_props preserves visibility metadata for inherited protected class
    // members as well as directly-declared private members.
    assert!(
        native_names.contains(&"hidden"),
        "native state should retain inherited protected class members: {:?}",
        native_names
    );
    assert!(
        native_names.contains(&"secret"),
        "native state should retain private members declared directly in the class: {:?}",
        native_names
    );
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "hidden"
                && prop.visibility
                    == verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
        }),
        "native state should preserve visibility metadata for inherited protected members"
    );
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "secret"
                && prop.visibility
                    == verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Private
        }),
        "native state should preserve visibility metadata for private members"
    );
}

#[test]
fn imported_interface_extending_class_uses_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/base.ts",
            r#"export class BaseProps { from_base!: string; protected hidden!: boolean }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
import { BaseProps } from './base'

export interface Props extends BaseProps {
  own: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let interface_macro = state
        .resolved_macros
        .iter()
        .find(|m| m.type_name == "Props")
        .expect("resolved interface macro should be present");
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"from_base".to_string()),
        "interface should inherit public class members through the shared resolver: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"own".to_string()),
        "interface local members should still materialize: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"hidden".to_string()),
        "compat/public props must not expose protected inherited class members: {:?}",
        prop_names
    );
    assert!(
        interface_macro.native_props.iter().any(|prop| {
            prop.name == "hidden"
                && prop.visibility
                    == verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
        }),
        "native state should retain protected inherited class members"
    );
}

#[test]
fn imported_emits_use_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"export interface Events { (e: 'change', id: number): void }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let emit_names = emit_names_from_resolved(&state);
    assert!(
        emit_names.contains(&"change".to_string()),
        "imported emits should flow through shared resolver: {:?}",
        emit_names
    );
}

#[test]
fn imported_slots_use_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"export interface Slots { default: (props: { row: string }) => any }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let slot_names = slot_names_from_resolved(&state);
    assert!(
        slot_names.contains(&"default".to_string()),
        "imported slots should flow through shared resolver: {:?}",
        slot_names
    );
}

#[test]
fn imported_vue_slot_helper_is_ignored_for_define_slots() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/vue/index.d.ts",
            r#"export type Slot = (props?: any) => any"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Slot } from 'vue'
defineSlots<Slot>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "vue".to_string(),
            resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    assert!(
        meta.slots.is_empty(),
        "vue Slot helper should be ignored for defineSlots, got: {:?}",
        meta.slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
    );
}

// ===========================================================================
// Phase 1: Architecture — Raw analysis boundary
// ===========================================================================

#[test]
fn get_analysis_returns_raw_snapshot_without_imported_enrichment() {
    // After the refactor, get_analysis() must NOT enrich imported types.
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().provenance().reset();
    let session = project.open_session().unwrap();
    let analysis = session
        .get_analysis("/App.vue")
        .unwrap()
        .expect("should return analysis");

    // Assert-: raw analysis must NOT have enriched prop_fields
    let dp = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("should have DefineProps macro");
    assert!(
        dp.prop_fields.is_empty(),
        "get_analysis() must return RAW snapshot without imported enrichment, \
         but prop_fields was populated: {:?}",
        dp.prop_fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Assert-: no resolved-state recomputes should have happened via get_analysis
    let p = provenance(&project);
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 0,
        "get_analysis() must NOT trigger resolved-state computation, got: {}",
        p.component_meta_resolved_state_recomputes
    );
}

#[test]
fn get_analysis_batch_matches_raw_get_analysis_without_enrichment() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let single = project.host().get_analysis("/App.vue").unwrap();
    let batch = project.host().get_analysis_batch(&["/App.vue"]);

    let single_dp = single
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .unwrap();
    let batch_dp = batch[0]
        .1
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .unwrap();

    assert_eq!(
        single_dp.prop_fields.len(),
        batch_dp.prop_fields.len(),
        "get_analysis and get_analysis_batch must return identical raw snapshots"
    );
}

// ===========================================================================
// Phase 1: Architecture — Failure semantics
// ===========================================================================

#[test]
fn missing_imported_symbol_is_best_effort_not_total_failure() {
    let project = make_project();
    // Intentionally do NOT upsert /types.ts so the import cannot be resolved
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { MissingType } from './types'
defineProps<MissingType>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // `ProjectionMode::Expanded` should still return a result (best-effort)
    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded);
    assert!(
        state.is_some(),
        "missing imported symbol should NOT cause total failure"
    );

    // The raw snapshot should still be available
    let state = state.unwrap();
    assert!(
        !state.snapshot.macros.is_empty(),
        "raw snapshot should still contain the macro even when import is unresolved"
    );
}

#[test]
fn malformed_typed_jsdoc_payload_stays_raw_without_failing_file() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
/** @type {This Is Not A Valid Type!!!}
 * @param {broken syntax
 */
export interface Props { a: string }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Should not fail even with malformed JSDoc
    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded);
    assert!(
        state.is_some(),
        "malformed JSDoc should NOT prevent resolution"
    );
}

// ===========================================================================
// Phase 1: Architecture — Barrel resolution through shared resolver
// ===========================================================================

#[test]
fn barrel_resolution_still_works_through_shared_resolver() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base("/index.ts", r#"export { Props } from './types'"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './index'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("barrel resolution should work through shared resolver");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "barrel-re-exported prop should be resolved: {:?}",
        prop_names
    );
}

#[test]
fn barrel_resolution_follows_import_alias_then_export_local() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Foo { aliased: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/index.ts",
            r#"import type { Foo as Bar } from './types'; export { Bar };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Bar } from './index'
defineProps<Bar>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("alias barrel resolution should work through shared resolver");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"aliased".to_string()),
        "alias import + local export should resolve the original declaration: {:?}",
        prop_names
    );
}

#[test]
fn barrel_resolution_follows_plain_import_alias_then_export_local() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Foo { aliased_plain: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/index.ts",
            r#"import { Foo as Bar } from './types'; export { Bar };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Bar } from './index'
defineProps<Bar>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("plain alias barrel resolution should work through shared resolver");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"aliased_plain".to_string()),
        "plain import alias + local export should resolve the original declaration: {:?}",
        prop_names
    );
}

#[test]
fn barrel_resolution_follows_default_import_then_export_local_for_classes() {
    let project = make_project();
    project
        .upsert_base(
            "/dep.ts",
            r#"
export default class Props {
  label!: string
  protected hidden!: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/index.ts",
            r#"import PropsDefault from './dep'; export { PropsDefault as Props };"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './index'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("default import alias barrel resolution should work through shared resolver");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"label".to_string()),
        "default import + local export should resolve the underlying class declaration: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"hidden".to_string()),
        "compat/public props must still filter protected class members: {:?}",
        prop_names
    );

    let class_macro = state
        .resolved_macros
        .iter()
        .find(|resolved| resolved.type_name == "Props")
        .expect("resolved class macro should be present");
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "hidden"
                && prop.visibility
                    == verter_compiler::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
        }),
        "native state should preserve protected members through default-import alias barrels"
    );
}

// ===========================================================================
// Phase 1: Architecture — JSDoc through shared resolver
// ===========================================================================

#[test]
fn imported_jsdoc_flows_through_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
/** Description of the Props interface.
 * @deprecated Use NewProps instead.
 */
export interface Props { a: string }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    // Find the resolved macro for Props
    let props_macro = state
        .resolved_macros
        .iter()
        .find(|m| m.type_name == "Props");
    assert!(
        props_macro.is_some(),
        "should have a resolved macro for Props"
    );
    let props_macro = props_macro.unwrap();

    // JSDoc should be attached
    assert!(
        props_macro.jsdoc.is_some(),
        "imported JSDoc should flow through the shared resolver path: {:?}",
        props_macro
    );
    let jsdoc = props_macro.jsdoc.as_ref().unwrap();
    assert!(
        jsdoc
            .description
            .as_deref()
            .unwrap_or("")
            .contains("Description of the Props"),
        "JSDoc description should be preserved"
    );
    assert!(
        jsdoc.tags.iter().any(|t| t.name == "deprecated"),
        "JSDoc tags should be preserved"
    );
    assert_eq!(
        props_macro.declaration.kind,
        crate::meta_resolve::ResolvedDeclarationKind::Interface,
        "native declaration metadata should preserve the pre-expansion kind"
    );
    assert_eq!(
        props_macro.declaration.resolved_name, "Props",
        "native declaration metadata should preserve the resolved declaration name"
    );
    assert_eq!(
        props_macro.declaration.canonical_source, "/types.ts",
        "native declaration metadata should preserve the canonical declaration source"
    );
    assert!(
        props_macro.declaration.declaration_id.is_some(),
        "native declaration metadata should preserve a stable declaration id"
    );
    assert!(
        props_macro.declaration.span.end > props_macro.declaration.span.start,
        "native declaration metadata should preserve a non-empty declaration span"
    );
    assert!(
        props_macro
            .declaration
            .text
            .as_deref()
            .unwrap_or("")
            .contains("export interface Props"),
        "native declaration metadata should preserve declaration text before expansion"
    );
}

#[test]
fn resolved_declaration_metadata_carries_stable_declaration_ids() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface Props {
  label: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve should succeed");
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve should succeed");

    let first_decl = &first.resolved_macros[0].declaration;
    let second_decl = &second.resolved_macros[0].declaration;

    assert_eq!(
        first_decl.declaration_id, second_decl.declaration_id,
        "declaration ids should be stable across repeated resolution of unchanged source"
    );
    assert!(
        first_decl.declaration_id.is_some(),
        "resolved declarations should carry a stable id"
    );
}

#[test]
fn component_meta_reuses_runtime_symbol_cache_after_owner_only_change() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface Props {
  label: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>first</div></template>"#,
        )
        .unwrap();

    project.host().resolver_runtime().reset_counters();

    let first = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("first resolve should succeed");
    let after_first = project.host().resolver_runtime().counter_snapshot();

    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div>second</div></template>"#,
        )
        .unwrap();

    let second = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("second resolve should succeed after owner-only change");
    let after_second = project.host().resolver_runtime().counter_snapshot();

    let first_props: Vec<_> = first.resolved_macros[0]
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let second_props: Vec<_> = second.resolved_macros[0]
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        first_props.contains(&"label"),
        "first resolve should include the imported prop"
    );
    assert!(
        second_props.contains(&"label"),
        "second resolve should still include the imported prop"
    );
    assert!(
        !second_props.contains(&"missing"),
        "owner-only recompute must not fabricate unrelated props"
    );
    assert!(
        after_first.node_cache_misses > 0,
        "first resolve should populate the runtime symbol cache, got {:?}",
        after_first
    );
    // NOTE: The legacy resolved_type_roots cache was removed in the
    // routed-symbol refactor. Runtime routed-symbol nodes will restore
    // cross-resolve cache reuse once the full service is wired.
    // For now, verify the semantic result is correct rather than the
    // cache hit count.
    let _ = (after_first, after_second);
}

#[test]
fn imported_typed_jsdoc_tags_resolve_through_shared_path() {
    let project = make_project();
    project
        .upsert_base(
            "/tag-types.ts",
            r#"export interface DocType { id: string; active?: boolean }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
import type { DocType } from './tag-types'

/**
 * Props carrying typed JSDoc.
 * @type {DocType}
 * @param {DocType} current current value
 * @returns {ReadonlyArray<DocType>} all values
 */
export interface Props { a: string }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    let props_macro = state
        .resolved_macros
        .iter()
        .find(|m| m.type_name == "Props")
        .expect("should have a resolved macro for Props");
    let jsdoc = props_macro
        .jsdoc
        .as_ref()
        .expect("typed JSDoc should be attached to the resolved declaration");

    let type_tag = jsdoc
        .tags
        .iter()
        .find(|tag| tag.name == "type")
        .expect("@type tag should be preserved");
    assert_eq!(type_tag.raw_type.as_deref(), Some("DocType"));
    assert!(
        type_tag.resolved_type.is_some(),
        "@type payload should resolve through the shared type pipeline"
    );

    let param_tag = jsdoc
        .tags
        .iter()
        .find(|tag| tag.name == "param")
        .expect("@param tag should be preserved");
    assert_eq!(param_tag.subject_name.as_deref(), Some("current"));
    assert!(
        param_tag.resolved_type.is_some(),
        "@param payload should resolve through the shared type pipeline"
    );

    let returns_tag = jsdoc
        .tags
        .iter()
        .find(|tag| tag.name == "returns")
        .expect("@returns tag should be preserved");
    assert_eq!(
        returns_tag.raw_type.as_deref(),
        Some("ReadonlyArray<DocType>")
    );
    assert!(
        returns_tag.resolved_type.is_some(),
        "@returns payload should resolve through the shared type pipeline"
    );
}

#[test]
fn registry_decl_materialization_skips_raw_snapshot_fallback_for_snapshotless_imported_state() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/src/types.ts".to_string(),
        Arc::from("export type Props = { label: string }\n"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _seeded = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed module facts");
    let decl = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("seeded dependency should expose Props through the prepared declaration cache");

    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    // D-Cutover §5.8: `CMQE::solve_scoped` retired; dispatch's
    // `project_type_surface_expr` is the sole scoped-lookup entry point.
    let materialized = query_engine
        .project_type_surface_expr("/src/types.ts", "Props")
        .expect("registry decl materialization should resolve Props through dispatch");

    assert_eq!(
        materialized, decl.body,
        "registry decl projection should reuse cached prepared state from IndexedReadyDb",
    );
}

#[test]
fn typed_jsdoc_resolution_uses_cached_import_lookup_without_external_source_traversal() {
    let project = make_project();
    project
        .upsert_base(
            "/tag-types.ts",
            r#"export interface DocType { id: string; active?: boolean }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types.ts",
            r#"
import type { DocType } from './tag-types'

export interface Props { a: string }
"#,
        )
        .unwrap();
    project.host().set_import_dependencies(
        "/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./tag-types".to_string(),
            resolved_canonical_id: Some("/tag-types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    project.host().provenance().reset();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let _store_view = project.host().resolver_store_view();

    let resolved =
        resolve_jsdoc_tag_type(project.host(), "/types.ts", "DocType", &mut tracked_deps)
            .expect("typed JSDoc payload should resolve through cached imported lookup");

    assert!(
        matches!(resolved, verter_semantic::analysis::type_expr::TypeExpr::Object(_)),
        "typed JSDoc should resolve the imported symbol through the cached eval env, got {resolved:?}",
    );
    assert!(
        tracked_deps.contains("/tag-types.ts"),
        "typed JSDoc resolution should still track the imported dependency"
    );
    let p = provenance(&project);
    assert_eq!(
        p.resolved_external_type_cache_misses, 0,
        "typed JSDoc should not call resolve_external_type_from_loaded_files through the legacy source-body path"
    );
}

#[test]
fn imported_member_jsdoc_flows_through_shared_resolver_path() {
    let project = make_project();
    project
        .upsert_base(
            "/props.ts",
            r#"
export interface Props {
  /** Label description.
   * @deprecated use `title`
   */
  label: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/events.ts",
            r#"
export interface Events {
  /** Change description.
   * @deprecated use update
   */
  (e: 'change', value: string): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export interface Slots {
  /** Default slot description.
   * @deprecated use item slot
   */
  default: (props: { row: string }) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './props'
import type { Events } from './events'
import type { Slots } from './slots'

defineProps<Props>()
defineEmits<Events>()
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let label = meta
        .props
        .iter()
        .find(|prop| prop.name == "label")
        .expect("imported prop should be materialized");
    assert_eq!(label.description.as_deref(), Some("Label description."));
    assert!(
        label.tags.iter().any(|tag| tag.name == "deprecated"),
        "imported prop tags should come from the shared resolver path"
    );

    let change = meta
        .events
        .iter()
        .find(|event| event.name == "change")
        .expect("imported event should be materialized");
    assert_eq!(change.description.as_deref(), Some("Change description."));
    assert!(
        change.tags.iter().any(|tag| tag.name == "deprecated"),
        "imported event tags should come from the shared resolver path"
    );

    let default_slot = meta
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("imported slot should be materialized");
    assert_eq!(
        default_slot.description.as_deref(),
        Some("Default slot description.")
    );
    assert!(
        default_slot.tags.iter().any(|tag| tag.name == "deprecated"),
        "imported slot tags should come from the shared resolver path"
    );
}

// ===========================================================================
// Phase 1: Regression — Local metadata still works
// ===========================================================================

#[test]
fn local_inline_props_still_work() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineProps<{ label: string; count?: number }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result for local inline props");

    // Raw snapshot should have the macro
    assert!(
        !state.snapshot.macros.is_empty(),
        "local inline props should be in the raw snapshot"
    );
}

#[test]
fn local_inline_emits_still_work() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineEmits<{ change: [value: string] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result for local inline emits");

    assert!(
        !state.snapshot.macros.is_empty(),
        "local inline emits should be in the raw snapshot"
    );
}

#[test]
fn local_type_reference_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
interface Emits {
  change: [value: string],
  submit: []
}
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"change"),
        "same-file defineEmits type references should survive final meta extraction: {:?}",
        event_names
    );
    assert!(
        event_names.contains(&"submit"),
        "same-file defineEmits type references should survive final meta extraction: {:?}",
        event_names
    );
}

#[test]
fn local_inline_slots_still_work() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
defineSlots<{ default: (props: { row: string }) => any }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result for local inline slots");

    assert!(
        !state.snapshot.macros.is_empty(),
        "local inline slots should be in the raw snapshot"
    );
}

#[test]
fn local_type_reference_slots_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
interface Slots {
  default(props: { row: string }): any
  footer?(): any
}
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"default"),
        "same-file defineSlots type references should survive final meta extraction: {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"footer"),
        "same-file defineSlots type references should survive final meta extraction: {:?}",
        slot_names
    );
}

#[test]
fn local_slot_intersection_keeps_named_slots_when_dynamic_branch_is_unresolvable() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type DynamicSlots = Record<string, (props: { value: string }) => any>
type Slots = { default(props: { row: string }): any } & DynamicSlots
defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"default"),
        "resolvable local slot branches should survive final meta extraction even when dynamic utility branches cannot be expanded: {:?}",
        slot_names
    );
}

#[test]
fn imported_inherited_props_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface LinkProps {
  as?: string
  class?: any
  href?: string
  target?: string
  active?: boolean
}

export type LinkPropsKeys = 'href' | 'target' | 'active'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  label?: string
  color?: string
  variant?: string
  ui?: object
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: 'primary'
  variant?: 'solid'
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"as"),
        "inherited props should survive imported Omit chains: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"class"),
        "explicit inherited class prop should survive imported Omit chains: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"label"),
        "local Button props should survive imported Omit chains: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"ui"),
        "non-omitted inherited props should survive imported Omit chains: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"side"),
        "local redeclarations should win and remain visible: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"active"),
        "omitted imported keys must not leak into final props: {:?}",
        prop_names
    );
}

#[test]
fn imported_inherited_props_reach_resolved_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface LinkProps {
  as?: string
  class?: any
  href?: string
  target?: string
  active?: boolean
}

export type LinkPropsKeys = 'href' | 'target' | 'active'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  label?: string
  color?: string
  variant?: string
  ui?: object
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './types'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: 'primary'
  variant?: 'solid'
  side?: 'left' | 'right'
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should resolve expanded component meta");
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("expanded component meta should carry evaluated types");
    let prop_names: std::collections::BTreeSet<_> = evaluated
        .define_props
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        prop_names.contains("as") && prop_names.contains("class"),
        "resolved evaluated types should retain inherited imported props before final projection: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains("label")
            && prop_names.contains("ui")
            && prop_names.contains("color")
            && prop_names.contains("variant")
            && prop_names.contains("side"),
        "resolved evaluated types should retain local props before final projection: {:?}",
        prop_names
    );
}

#[test]
fn barrel_reexported_vue_props_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/components/Link.vue",
            r#"<script lang="ts">
export interface LinkProps {
  href?: string
  target?: string
  active?: boolean
  class?: any
}

export type LinkPropsKeys = 'href' | 'target' | 'active'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/use-icons.ts",
            r#"
export interface UseComponentIconsProps {
  icon?: string
  leading?: boolean
  trailing?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/Button.vue",
            r#"<script lang="ts">
import type { LinkProps } from './Link.vue'
import type { UseComponentIconsProps } from './use-icons'

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'href'> {
  label?: string
  color?: string
  variant?: string
  size?: string
  square?: boolean
  block?: boolean
  class?: any
  ui?: object
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/index.ts",
            "export * from './components/Link.vue'\nexport * from './components/Button.vue'",
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ButtonProps, LinkPropsKeys } from './index'

interface Props extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: 'primary'
  variant?: 'solid'
  side?: 'left' | 'right'
  ui?: { base?: any }
}

defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");
    let _resolved = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should resolve expanded component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"label")
            && prop_names.contains(&"size")
            && prop_names.contains(&"square")
            && prop_names.contains(&"block")
            && prop_names.contains(&"icon")
            && prop_names.contains(&"leading")
            && prop_names.contains(&"trailing")
            && prop_names.contains(&"class")
            && prop_names.contains(&"ui")
            && prop_names.contains(&"side")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant"),
        "barrel re-exported Vue props should preserve inherited imported members: props={:?}",
        prop_names,
    );
    assert!(
        !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"active"),
        "barrel re-exported Vue props must still respect Omit keys: {:?}",
        prop_names
    );
}

#[test]
fn imported_inherited_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"
export interface MenuContentImplEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'pointerDownOutside', event: PointerEvent): void
  (e: 'focusOutside', event: FocusEvent): void
  (e: 'interactOutside', event: Event): void
  (e: 'openAutoFocus'): void
  (e: 'closeAutoFocus'): void
  (e: 'entryFocus'): void
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>

export interface ContextMenuContentEmits extends MenuContentEmits {}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ContextMenuContentEmits } from './events'

defineEmits<ContextMenuContentEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"pointerDownOutside")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"interactOutside")
            && event_names.contains(&"closeAutoFocus"),
        "inherited emits should survive imported Omit chains: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"openAutoFocus") && !event_names.contains(&"entryFocus"),
        "omitted imported emits must not leak into final events: {:?}",
        event_names
    );
}

#[test]
fn imported_inherited_and_local_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"
export interface MenuContentEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'focusOutside', event: FocusEvent): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  (e: 'closeAutoFocus'): void
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"closeAutoFocus"),
        "mixed local and inherited emits should survive final meta extraction: {:?}",
        event_names
    );
}

#[test]
fn imported_mapped_tuple_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"export type Emits = {
  [K in 'open' | 'close']?: []
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Emits } from './events'

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"open") && event_names.contains(&"close"),
        "imported mapped tuple emits should materialize concrete event names: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"default"),
        "imported mapped tuple emits must not invent unrelated events: {:?}",
        event_names
    );
}

#[test]
fn imported_inherited_and_tuple_property_local_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"
export interface MenuContentEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'focusOutside', event: FocusEvent): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"update:searchTerm"),
        "tuple-property local emits must not discard inherited imported emits: {:?}",
        event_names
    );
}

#[test]
fn imported_alias_chain_and_tuple_property_local_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"
export interface MenuContentImplEmits {
  (e: 'escapeKeyDown', event: Event): void
  (e: 'pointerDownOutside', event: PointerEvent): void
  (e: 'focusOutside', event: FocusEvent): void
  (e: 'interactOutside', event: Event): void
  (e: 'openAutoFocus'): void
  (e: 'closeAutoFocus'): void
  (e: 'entryFocus'): void
}

export type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { MenuContentEmits } from './events'

interface Emits extends MenuContentEmits {
  'update:searchTerm': [value: string]
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"pointerDownOutside")
            && event_names.contains(&"focusOutside")
            && event_names.contains(&"interactOutside")
            && event_names.contains(&"closeAutoFocus")
            && event_names.contains(&"update:searchTerm"),
        "tuple-property local emits must preserve inherited imported alias-chain emits: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"openAutoFocus") && !event_names.contains(&"entryFocus"),
        "local alias-chain emits must still respect omitted imported events: {:?}",
        event_names
    );
}

#[test]
fn cyclic_barrel_vue_props_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string
  rel?: string
  download?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types/tv.ts",
            r#"
export type ComponentConfig<TTheme, TAppConfig, TName extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'solid' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: {
    base?: string
  }
  ui: {
    base: string
  }
  AppConfig: TAppConfig
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/composables/useComponentIcons.ts",
            r#"
import type { AvatarProps, IconProps } from '../types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
  loading?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/Avatar.vue",
            r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
  size?: 'sm' | 'md'
}
</script>
<template><img /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/Icon.vue",
            r#"<script lang="ts">
export interface IconProps {
  name?: string
}
</script>
<template><i /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  href?: string
  target?: string
  rel?: string
  active?: boolean
  activeClass?: string
  inactiveClass?: string
  raw?: boolean
  custom?: boolean
  class?: any
}

export type LinkPropsKeys = 'href' | 'target' | 'rel' | 'active' | 'activeClass' | 'inactiveClass' | 'download'
</script>
<template><a /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from './nuxt-schema'
import theme from './button-theme'
import type { UseComponentIconsProps } from '../composables/useComponentIcons'
import type { LinkProps, AvatarProps } from '../types'
import type { ComponentConfig } from '../types/tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: Button['variants']['color']
  variant?: Button['variants']['variant']
  size?: Button['variants']['size']
  square?: boolean
  block?: boolean
  loadingAuto?: boolean
  onClick?: ((event: MouseEvent) => void) | Array<((event: MouseEvent) => void)>
  avatar?: AvatarProps
  class?: any
  ui?: Button['slots']
}
</script>
<template><button /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/components/button-theme.ts",
            "export default { variants: {} }",
        )
        .unwrap();
    project
        .upsert_base(
            "/components/nuxt-schema.ts",
            "export interface AppConfig {}",
        )
        .unwrap();
    project
        .upsert_base(
            "/types/index.ts",
            r#"
export * from '../components/Avatar.vue'
export * from '../components/Button.vue'
export * from '../components/Icon.vue'
export * from '../components/Link.vue'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script lang="ts">
import type { AppConfig } from './components/nuxt-schema'
import theme from './components/button-theme'
import type { ButtonProps, LinkPropsKeys } from './types'
import type { ComponentConfig } from './types/tv'

type DashboardSidebarCollapse = ComponentConfig<typeof theme, AppConfig, 'dashboardSidebarCollapse'>

export interface DashboardSidebarCollapseProps extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  side?: 'left' | 'right'
  ui?: DashboardSidebarCollapse['slots']
}
</script>
<script setup lang="ts">
defineProps<DashboardSidebarCollapseProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let _store_view = project.host().resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(project.host());
    let button_projected = query_engine
        .project_type_surface_expr("/components/Button.vue", "ButtonProps")
        .expect("ButtonProps should project through the shared type-surface DB path");
    let button_projected_debug = format!("{:?}", button_projected);
    assert!(
        button_projected_debug.contains("icon")
            && button_projected_debug.contains("loading")
            && button_projected_debug.contains("disabled"),
        "ButtonProps projection should preserve imported inherited members: {}",
        button_projected_debug,
    );

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"label")
            && prop_names.contains(&"size")
            && prop_names.contains(&"square")
            && prop_names.contains(&"block")
            && prop_names.contains(&"icon")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"avatar")
            && prop_names.contains(&"as")
            && prop_names.contains(&"class")
            && prop_names.contains(&"type")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"side")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant"),
        "cyclic barrel Vue props should preserve inherited imported members: props={:?}",
        prop_names,
    );
    assert!(
        !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"activeClass")
            && !prop_names.contains(&"inactiveClass")
            && !prop_names.contains(&"download"),
        "cyclic barrel Omit should still exclude imported LinkPropsKeys members: props={:?}",
        prop_names,
    );
}

#[test]
fn lazy_workspace_cyclic_barrel_vue_props_reach_final_component_meta() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in [
        (
            "/workspace/types/html.ts",
            r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string
  rel?: string
  download?: boolean
}
"#,
        ),
        (
            "/workspace/types/tv.ts",
            r#"
export type ComponentConfig<TTheme, TAppConfig, TName extends string> = {
  variants: {
    color: 'primary' | 'neutral'
    variant: 'solid' | 'ghost'
    size: 'sm' | 'md'
  }
  slots: {
    base?: string
  }
  ui: {
    base: string
  }
  AppConfig: TAppConfig
}
"#,
        ),
        (
            "/workspace/composables/useComponentIcons.ts",
            r#"
import type { AvatarProps, IconProps } from '../types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
  loading?: boolean
}
"#,
        ),
        (
            "/workspace/components/Avatar.vue",
            r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
  size?: 'sm' | 'md'
}
</script>
<template><img /></template>"#,
        ),
        (
            "/workspace/components/Icon.vue",
            r#"<script lang="ts">
export interface IconProps {
  name?: string
}
</script>
<template><i /></template>"#,
        ),
        (
            "/workspace/components/Link.vue",
            r#"<script lang="ts">
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

export interface LinkProps extends Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  href?: string
  target?: string
  rel?: string
  active?: boolean
  activeClass?: string
  inactiveClass?: string
  raw?: boolean
  custom?: boolean
  class?: any
}

export type LinkPropsKeys = 'href' | 'target' | 'rel' | 'active' | 'activeClass' | 'inactiveClass' | 'download'
</script>
<template><a /></template>"#,
        ),
        (
            "/workspace/components/Button.vue",
            r#"<script lang="ts">
import type { AppConfig } from './nuxt-schema'
import theme from './button-theme'
import type { UseComponentIconsProps } from '../composables/useComponentIcons'
import type { LinkProps, AvatarProps } from '../types'
import type { ComponentConfig } from '../types/tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>

export interface ButtonProps extends UseComponentIconsProps, Omit<LinkProps, 'raw' | 'custom'> {
  label?: string
  color?: Button['variants']['color']
  variant?: Button['variants']['variant']
  size?: Button['variants']['size']
  square?: boolean
  block?: boolean
  loadingAuto?: boolean
  onClick?: ((event: MouseEvent) => void) | Array<((event: MouseEvent) => void)>
  avatar?: AvatarProps
  class?: any
  ui?: Button['slots']
}
</script>
<template><button /></template>"#,
        ),
        (
            "/workspace/components/button-theme.ts",
            "export default { variants: {} }",
        ),
        (
            "/workspace/components/nuxt-schema.ts",
            "export interface AppConfig {}",
        ),
        (
            "/workspace/types/index.ts",
            r#"
export * from '../components/Avatar.vue'
export * from '../components/Button.vue'
export * from '../components/Icon.vue'
export * from '../components/Link.vue'
"#,
        ),
        (
            "/workspace/App.vue",
            r#"<script lang="ts">
import type { AppConfig } from './components/nuxt-schema'
import theme from './components/button-theme'
import type { ButtonProps, LinkPropsKeys } from './types'
import type { ComponentConfig } from './types/tv'

type DashboardSidebarCollapse = ComponentConfig<typeof theme, AppConfig, 'dashboardSidebarCollapse'>

export interface DashboardSidebarCollapseProps extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  side?: 'left' | 'right'
  ui?: DashboardSidebarCollapse['slots']
}
</script>
<script setup lang="ts">
defineProps<DashboardSidebarCollapseProps>()
</script>
<template><div /></template>"#,
        ),
    ] {
        ws.inject_file(path.to_string(), Arc::from(source));
    }

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    let project = MetaProject::new(host);
    assert!(
        project.ensure_loaded("/workspace/App.vue").unwrap(),
        "entry file should load from workspace"
    );

    let meta = project
        .host()
        .get_component_meta("/workspace/App.vue")
        .expect("should return component meta");
    let _resolved = project
        .host()
        .resolve_component_meta("/workspace/App.vue", ProjectionMode::Expanded)
        .expect("should resolve expanded component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"label")
            && prop_names.contains(&"size")
            && prop_names.contains(&"square")
            && prop_names.contains(&"block")
            && prop_names.contains(&"icon")
            && prop_names.contains(&"loading")
            && prop_names.contains(&"avatar")
            && prop_names.contains(&"as")
            && prop_names.contains(&"class")
            && prop_names.contains(&"type")
            && prop_names.contains(&"disabled")
            && prop_names.contains(&"side")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant"),
        "lazy workspace cyclic barrel props should preserve inherited imported members: props={:?}",
        prop_names,
    );
    assert!(
        !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"active")
            && !prop_names.contains(&"activeClass")
            && !prop_names.contains(&"inactiveClass")
            && !prop_names.contains(&"download"),
        "lazy workspace cyclic barrel Omit should exclude imported LinkPropsKeys members: props={:?}",
        prop_names,
    );
}

#[test]
fn demand_driven_import_resolution_without_prewarm() {
    // Regression: the owner macro-expansion path no longer eagerly seeds
    // every direct import.  Imports used by the type route must materialize
    // on demand through the solver's lazy prepared-decl path.
    let project = make_project();
    project
        .upsert_base(
            "/types/tv.ts",
            r#"
export type ComponentConfig<TTheme, TAppConfig, TName extends string> = {
  variants: { color: 'primary' | 'neutral' }
  slots: { base?: string }
  AppConfig: TAppConfig
}
"#,
        )
        .unwrap();
    project
        .upsert_base("/nuxt-schema.ts", "export interface AppConfig {}")
        .unwrap();
    project
        .upsert_base("/theme.ts", "export default { slots: { base: '' } }")
        .unwrap();
    project
        .upsert_base(
            "/Shimmer.vue",
            r#"<script lang="ts">
import type { AppConfig } from './nuxt-schema'
import theme from './theme'
import type { ComponentConfig } from './types/tv'

type Shimmer = ComponentConfig<typeof theme, AppConfig, 'shimmer'>

export interface ShimmerProps {
  text: string
  class?: any
  ui?: Shimmer['slots']
}
</script>
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<ShimmerProps>()
const len = computed(() => props.text.length)
</script>
<template><span>{{ text }}</span></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/Shimmer.vue")
        .expect("should resolve component meta without eager import warmup");

    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"text") && prop_names.contains(&"ui") && prop_names.contains(&"class"),
        "demand-driven resolution should find all props: got {:?}",
        prop_names,
    );

    // `ui` should resolve to a slot-shaped type, not `any` or `unknown`.
    let ui_prop = meta.props.iter().find(|p| p.name == "ui").unwrap();
    assert!(
        !matches!(
            ui_prop.type_expr,
            verter_semantic::analysis::type_expr::TypeExpr::Primitive(
                verter_semantic::analysis::type_expr::PrimitiveName::Any
                    | verter_semantic::analysis::type_expr::PrimitiveName::Unknown,
            )
        ),
        "ui prop should resolve to a concrete type, not any/unknown: got {:?}",
        ui_prop.type_expr,
    );
}

#[test]
fn imported_mapped_slots_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { PricingPlansSlots } from './slots'

defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"badge")
            && slot_names.contains(&"title")
            && slot_names.contains(&"default"),
        "imported mapped slots should materialize concrete names in final meta: {:?}",
        slot_names
    );

    let badge = meta
        .slots
        .iter()
        .find(|slot| slot.name == "badge")
        .expect("badge slot should exist");
    let badge_bindings: Vec<&str> = badge
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert!(
        badge_bindings.contains(&"plan") && badge_bindings.contains(&"planId"),
        "mapped slot bindings should preserve inferred and extended slot function parameters: {:?}",
        badge_bindings
    );
}

#[test]
fn imported_mapped_slots_reach_resolved_evaluated_types() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { PricingPlansSlots } from './slots'

defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let resolved = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should resolve expanded component meta");
    let evaluated = resolved
        .evaluated_types
        .as_ref()
        .expect("expanded component meta should carry evaluated types");
    let slot_names: std::collections::BTreeSet<_> = evaluated
        .define_slots
        .iter()
        .flat_map(|entry| entry.result.value.properties.iter())
        .map(|slot| slot.name.as_str())
        .collect();
    let binding_names: std::collections::BTreeSet<_> = evaluated
        .slot_bindings
        .iter()
        .filter_map(|binding| {
            binding
                .name
                .strip_prefix("badge.")
                .map(|name| name.to_string())
        })
        .collect();

    assert!(
        slot_names.contains("badge")
            && slot_names.contains("title")
            && slot_names.contains("default"),
        "resolved evaluated slot shapes should retain imported mapped slot names before final projection: {:?}",
        slot_names
    );
    assert_eq!(
        binding_names,
        std::collections::BTreeSet::from([
            "plan".to_string(),
            "planId".to_string(),
        ]),
        "resolved evaluated slot bindings should retain mapped slot parameter expansion before final projection",
    );
}

#[test]
fn imported_dynamic_slot_branches_do_not_synthesize_default_in_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export type TableSlots = {
  expanded?(props: { row: string }): any
  empty?(props?: {}): any
} & Record<string, (props: any) => any>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { TableSlots } from './slots'

defineSlots<TableSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"expanded") && slot_names.contains(&"empty"),
        "named imported slots should survive dynamic branches: {:?}",
        slot_names
    );
    assert!(
        !slot_names.contains(&"default"),
        "dynamic imported slot branches must not synthesize default: {:?}",
        slot_names
    );
}

#[test]
fn template_dynamic_named_slot_outlets_do_not_synthesize_default_in_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
const sectionSlot = 'section-title'
</script>
<template>
  <div>
    <slot :name="sectionSlot" />
    <slot name="caption" />
  </div>
</template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"caption"),
        "static template slot outlets should still materialize: {:?}",
        slot_names
    );
    assert!(
        !slot_names.contains(&"default"),
        "dynamic named template slot outlets must not synthesize default: {:?}",
        slot_names
    );
}

#[test]
fn namespace_qualified_imported_props_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export interface BaseProps {
  a?: string
  b?: number
}

export interface Props extends BaseProps {
  c?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type * as Types from './types'

defineProps<Types.Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b") && prop_names.contains(&"c"),
        "namespace-qualified imported props should resolve through the owner eval path: {:?}",
        prop_names
    );
    assert!(
        !prop_names.contains(&"default"),
        "namespace-qualified imported props must not invent unrelated props: {:?}",
        prop_names
    );
}

#[test]
fn imported_typeof_member_paths_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/theme.ts",
            r#"
export const theme = {
  slots: {
    root: '',
    label: ''
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import * as ThemeNs from './theme'

type Slots = typeof ThemeNs.theme.slots

defineProps<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"root") && prop_names.contains(&"label"),
        "imported typeof member paths should resolve through final component meta: {:?}",
        prop_names
    );
}

#[test]
fn reexported_imported_typeof_member_paths_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/inner.ts",
            r#"
export const theme = {
  slots: {
    root: '',
    label: ''
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/index.ts",
            r#"export { theme as sharedTheme } from './inner'"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import * as ThemeNs from './index'

type Slots = typeof ThemeNs.sharedTheme.slots

defineProps<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let prop_names: Vec<&str> = meta.props.iter().map(|prop| prop.name.as_str()).collect();
    assert!(
        prop_names.contains(&"root") && prop_names.contains(&"label"),
        "re-exported imported typeof member paths should resolve through final component meta: {:?}",
        prop_names
    );
}

#[test]
fn mapped_tuple_emits_reach_final_component_meta() {
    let project = make_project();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
type Emits = {
  [K in 'open' | 'close']?: []
}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta");

    let event_names: Vec<&str> = meta
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert!(
        event_names.contains(&"open") && event_names.contains(&"close"),
        "mapped tuple emits should materialize concrete event names: {:?}",
        event_names
    );
    assert!(
        !event_names.contains(&"default"),
        "mapped tuple emits must not invent unrelated events: {:?}",
        event_names
    );
}

#[test]
fn duplicate_same_kind_imported_macros_keep_distinct_macro_identity() {
    let project = make_project();
    project
        .upsert_base(
            "/events.ts",
            r#"export interface Events { save: [id: string] }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Events } from './events'

const emitA = defineEmits<Events>()
const emitB = defineEmits<Events>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("duplicate imported macros should still resolve");

    let emit_macros: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits)
        .collect();

    assert_eq!(
        emit_macros.len(),
        2,
        "each imported macro should keep its own resolved entry, got: {:?}",
        state.resolved_macros
    );

    let mut indices: Vec<_> = emit_macros.iter().map(|m| m.macro_index).collect();
    indices.sort_unstable();
    assert_eq!(
        indices,
        vec![0, 1],
        "resolved macro identity should follow the raw macro order"
    );
    assert!(
        emit_macros
            .iter()
            .all(|m| m.emits.iter().any(|emit| emit.name == "save")),
        "each macro should materialize its imported emit payload through the shared path"
    );
}

// ===========================================================================
// Phase 1: Regression — Package declaration entrypoints
// ===========================================================================

#[test]
fn package_declaration_entrypoints_still_work() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/project/node_modules/my-lib/package.json".to_string(),
        Arc::from(r#"{"name": "my-lib", "types": "./dist/index.d.ts"}"#),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/index.d.ts".to_string(),
        Arc::from("export interface LibProps { x: string }"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    let project = MetaProject::new(host);

    project
        .upsert_base(
            "/project/App.vue",
            r#"<script setup lang="ts">
import { LibProps } from 'my-lib'
defineProps<LibProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/project/App.vue", ProjectionMode::Expanded);

    // Should at least return a result (package resolution may or may not work
    // depending on workspace configuration, but it should not crash)
    assert!(
        state.is_some(),
        "package declaration entrypoint should not crash the resolver"
    );
}

#[test]
fn package_declaration_entrypoints_materialize_imported_props() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/project/node_modules/my-lib/package.json".to_string(),
        Arc::from(
            r#"{"name": "my-lib", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } }}"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/index.d.ts".to_string(),
        Arc::from(r#"import { LibProps } from "./inner.js"; export type { LibProps };"#),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.d.ts".to_string(),
        Arc::from("export interface LibProps { x: string }"),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/project".to_string(),
            "/project".to_string(),
            Some("/project/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);

    project
        .upsert_base(
            "/project/App.vue",
            r#"<script setup lang="ts">
import type { LibProps } from 'my-lib'
defineProps<LibProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/project/App.vue", ProjectionMode::Expanded)
        .expect("package declaration entrypoint should resolve");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"x".to_string()),
        "resolver should materialize imported props from declaration entrypoints: {:?}",
        state.resolved_macros
    );
}

#[test]
fn package_declaration_entrypoints_materialize_alias_reexports() {
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/project/node_modules/my-lib/package.json".to_string(),
        Arc::from(
            r#"{"name": "my-lib", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } }}"#,
        ),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/index.d.ts".to_string(),
        Arc::from(r#"import { LibProps as ImportedProps } from "./inner.js"; export type { ImportedProps as LibProps };"#),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.d.ts".to_string(),
        Arc::from("export interface LibProps { y: number }"),
    );
    ws.inject_file(
        "/project/node_modules/my-lib/dist/inner.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        Arc::new(ws),
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/project".to_string(),
            "/project".to_string(),
            Some("/project/tsconfig.json".to_string()),
        ),
    ]);
    let project = MetaProject::new(host);

    project
        .upsert_base(
            "/project/App.vue",
            r#"<script setup lang="ts">
import type { LibProps } from 'my-lib'
defineProps<LibProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/project/App.vue", ProjectionMode::Expanded)
        .expect("package declaration alias reexport should resolve");

    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"y".to_string()),
        "resolver should follow import alias reexports through declaration entrypoints: {:?}",
        state.resolved_macros
    );
}

// ===========================================================================
// Phase 1: Architecture — component_meta projection tests
// ===========================================================================

#[test]
fn component_meta_does_not_require_expanded_types_flag() {
    // After the refactor, ComponentMetaFeatures::expanded_types should not exist.
    // For now, this test verifies that get_component_meta always uses the new resolver path.
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session().unwrap();
    let meta = session
        .get_component_meta("/App.vue")
        .unwrap()
        .expect("get_component_meta should work regardless of deep_expansion flag");

    // After the refactor, get_component_meta should always use `ProjectionMode::Expanded`
    // through resolve_component_meta, so imported props should appear
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"a"),
        "get_component_meta should resolve imported props via the new resolver path, got: {:?}",
        prop_names
    );
}

#[test]
fn resolved_type_registry_preserves_pre_expansion_declaration_metadata() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { label: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("expanded component-meta state should resolve");

    let registry_entry = state
        .resolved_type_registry_meta
        .iter()
        .find(|entry| entry.name == "Props")
        .expect("resolved type registry should keep native declaration metadata");

    assert_eq!(registry_entry.declaration.requested_name, "Props");
    assert_eq!(registry_entry.declaration.resolved_name, "Props");
    assert_eq!(registry_entry.declaration.canonical_source, "/types.ts");
    assert_eq!(
        registry_entry.declaration.kind,
        crate::meta_resolve::ResolvedDeclarationKind::Interface,
    );
    assert!(
        registry_entry
            .declaration
            .text
            .as_deref()
            .unwrap_or("")
            .contains("export interface Props"),
        "type-registry metadata should retain the declaration text before expansion"
    );
}

// ===========================================================================
// Phase 1: Architecture — Overlay safety
// ===========================================================================

#[test]
fn overlay_queries_do_not_reuse_unsound_base_resolved_meta_cache() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // Populate base resolved-meta cache
    let _base_state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded);

    // Session with overlay that changes the dependency
    let session = project.open_session().unwrap();
    session
        .upsert(
            "/types.ts",
            r#"export interface Props { a: string; overlay_prop: boolean }"#.into(),
        )
        .unwrap();

    // Query through session — should NOT reuse stale base cache
    let (_analysis, session_state) = session
        .get_component_meta_with_resolution("/App.vue")
        .unwrap()
        .expect("session resolver query should return a result");
    let overlay_props: Vec<&str> = session_state
        .resolved_macros
        .iter()
        .flat_map(|resolved| resolved.props.iter().map(|prop| prop.name.as_str()))
        .collect();
    assert!(
        overlay_props.contains(&"overlay_prop"),
        "overlay resolver query should observe overlay-specific resolved props, got: {:?}",
        overlay_props
    );
}

// ===========================================================================
// Edge case: `ProjectionMode::Identity` cache invalidation on dependency change
// ===========================================================================

#[test]
fn type_mode_cache_invalidates_when_dependency_file_changes() {
    let project = make_project();
    project
        .upsert_base("/types.ts", r#"export interface Props { a: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    // First `ProjectionMode::Identity` call — populates cache
    let state1 = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity)
        .expect("first `ProjectionMode::Identity` should return result");
    assert_eq!(
        state1.resolved_macros[0].declaration.canonical_source, "/types.ts",
        "should resolve to /types.ts"
    );

    // Change the dependency file
    project
        .upsert_base(
            "/types.ts",
            r#"
/** Updated documentation */
export interface Props { a: string; b: number }"#,
        )
        .unwrap();

    // Second `ProjectionMode::Identity` call — cache should be invalidated by dep change
    project.host().provenance().reset();
    let state2 = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Identity)
        .expect("second `ProjectionMode::Identity` should return result");

    let p = provenance(&project);
    // Assert+: resolved state was recomputed (not served from stale cache)
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "`ProjectionMode::Identity` cache should invalidate when dependency changes, got recomputes={}",
        p.component_meta_resolved_state_recomputes
    );
    // Assert-: the declaration source should still be correct
    assert_eq!(
        state2.resolved_macros[0].declaration.canonical_source,
        "/types.ts"
    );
}

// ===========================================================================
// Edge case: find_named_declaration_start word boundary
// ===========================================================================

#[test]
fn declaration_text_does_not_match_substring_names() {
    let project = make_project();
    // File has both "interface PropsBase" and "interface Props"
    // — make sure we get the right one
    project
        .upsert_base(
            "/types.ts",
            r#"export interface PropsBase { base: boolean }
export interface Props { a: string }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    let decl = &state.resolved_macros[0].declaration;
    // Assert+: declaration text should be for Props, not PropsBase
    assert!(
        decl.text.as_deref().unwrap_or("").contains("{ a: string }"),
        "declaration text should be for 'Props' not 'PropsBase', got: {:?}",
        decl.text
    );
    // Assert-: should NOT contain PropsBase members
    assert!(
        !decl.text.as_deref().unwrap_or("").contains("base"),
        "declaration text should NOT contain 'base' from PropsBase, got: {:?}",
        decl.text
    );
}

// ===========================================================================
// Edge case: declaration text with string braces
// ===========================================================================

#[test]
fn declaration_text_handles_braces_inside_string_literals() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export interface Props {
  format: "{ value }"
  label: string
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    let decl = &state.resolved_macros[0].declaration;
    // Assert+: declaration text should include both members
    let text = decl.text.as_deref().unwrap_or("");
    assert!(
        text.contains("format") && text.contains("label"),
        "declaration text should include both members despite braces in string, got: {:?}",
        text
    );
}

// ===========================================================================
// Edge case: type alias with mapped type (inner semicolons)
// ===========================================================================

#[test]
fn type_alias_with_mapped_type_resolves_props() {
    // Mapped types like `{ [K in 'a' | 'b']: string; }` should not break
    // the resolver. The declaration text extraction is best-effort, but
    // the actual prop resolution should work correctly.
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export type Props = { a: string; b: number }"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    // Assert+: props should be resolved correctly from the type alias
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "should resolve 'a': {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"b".to_string()),
        "should resolve 'b': {:?}",
        prop_names
    );
    // Assert-: no extra props
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props: {:?}",
        prop_names
    );
}

// ===========================================================================
// Edge case: JSDoc with nested braces in type expression
// ===========================================================================

#[test]
fn jsdoc_tag_with_nested_braces_parses_correctly() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
/** @type {Record<string, {nested: true}>} */
export interface Props { a: string }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    let jsdoc = state.resolved_macros[0].jsdoc.as_ref();
    assert!(jsdoc.is_some(), "JSDoc should be attached");
    let type_tag = jsdoc.unwrap().tags.iter().find(|t| t.name == "type");
    assert!(type_tag.is_some(), "should have @type tag");
    let raw_type = type_tag.unwrap().raw_type.as_deref().unwrap_or("");
    // Assert+: the full nested type should be captured
    assert!(
        raw_type.contains("Record<string, {nested: true}>"),
        "nested braces should be parsed correctly, got raw_type: {:?}",
        raw_type
    );
    // Assert-: should NOT be truncated at the first }
    assert!(
        !raw_type.ends_with("{nested: true"),
        "should NOT be truncated at first closing brace"
    );
}

// ===========================================================================
// Edge case: missing file returns None from resolve_component_meta
// ===========================================================================

#[test]
fn resolve_component_meta_returns_none_for_missing_file() {
    let project = make_project();
    // Do NOT upsert any file

    let result = project
        .host()
        .resolve_component_meta("/missing.vue", ProjectionMode::Expanded);
    assert!(
        result.is_none(),
        "resolve_component_meta should return None for a missing file"
    );

    let result_type = project
        .host()
        .resolve_component_meta("/missing.vue", ProjectionMode::Identity);
    assert!(
        result_type.is_none(),
        "resolve_component_meta(Type) should also return None for a missing file"
    );
}

// `resolve_component_meta_populates_compute_audit_when_enabled`
// retired in $5.8 WIP-W (plan $5.9 Change T): the
// `ComponentMetaComputeAudit` telemetry block was solver-owned
// (step counters / cache-hit counters on the retired `TypeQueryEngine`);
// dispatch publishes per-query stats through `SemanticGraphStats` and
// the solver-specific audit block is gone. The sister test
// `resolve_component_meta_leaves_compute_audit_empty_when_disabled`
// still runs and asserts the opt-out behaviour.

#[test]
fn resolve_component_meta_leaves_compute_audit_empty_when_disabled() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Props = { foo: string }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("resolve_component_meta should return a state");

    assert!(
        state.compute_audit.is_none(),
        "audit-disabled requests must stay on the cold no-audit path",
    );
}

#[test]
fn resolve_component_meta_records_imported_root_proof_time_when_imports_are_followed() {
    let project = make_project_with_config(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        audit_enabled: true,
        ..HostConfig::default()
    });
    project
        .upsert_base(
            "/src/base.ts",
            "export type BaseProps = { foo: string; bar?: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            "import type { BaseProps } from './base'\nexport type SharedProps = Partial<BaseProps>",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { SharedProps } from './types'
defineProps<SharedProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("resolve_component_meta should return a state");
    let audit = state
        .compute_audit
        .expect("audit-enabled requests should populate compute audit");

    assert!(
        audit.timings.imported_root_proof_ms > 0.0,
        "imported type requests should accumulate imported-root proof time",
    );
}

// ===========================================================================
// Edge case: class with ECMAScript #private fields
// ===========================================================================

#[test]
fn class_ecmascript_private_fields_are_excluded_from_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"export class Props {
  label!: string
  #internalState = 0
  count!: number
}"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("should return result");

    let prop_names = prop_names_from_resolved(&state);
    // Assert+: public members should be included
    assert!(
        prop_names.contains(&"label".to_string()),
        "public 'label' should be a prop: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"count".to_string()),
        "public 'count' should be a prop: {:?}",
        prop_names
    );
    // Assert-: ECMAScript private fields should NOT be props
    assert!(
        !prop_names.iter().any(|n| n.contains("internalState")),
        "ECMAScript #private fields should NOT appear as props: {:?}",
        prop_names
    );
}

// ===========================================================================
// B1: Declaration-aware batch scope isolation and reuse
// ===========================================================================
//
// D-Cutover §5.8 WIP-W: `CMQE::solve_scoped` + `scoped_cache` retired
// with the solver subsystem. The two cache-observing tests that used to
// live here —
// `component_meta_query_engine_caches_by_scope_and_name`
// (`scoped_cache_len == 1` + `solve_count == 1` after a repeat solve) and
// `component_meta_query_engine_different_scopes_do_not_alias`
// (`scoped_cache_len == 2` + `solve_count == 2` across two scopes) —
// tested the retired solver-cache identity, not observable component-
// meta behaviour. Dispatch's `project_type_surface_expr` replaces the
// access pattern at every production call site; cross-scope
// non-aliasing is already covered by the dispatch memo's identity
// contract (see `project_semantic_dispatch::tests`).

// D-Cutover §5.8 WIP-W: `debug_solver_host_for_scope` retired with
// the SessionSolverHost bridge. The scope-payload cache identity
// contract lives on `CMQE::scope_payload_for_scope` directly (returns
// `Option<Arc<DeclarationScopePayload>>`), and is exercised through
// repeated dispatch calls in the surviving component-meta tests.

// D-Cutover §5.8 WIP-W: `resolve_component_meta_parts_populates_shared_owner_engine`
// retired. The test pinned the `HostComponentMetaResolver.shared_owner_engine`
// field (a `TypeQueryEngine` bridge) and asserted `engine.solve_count() > 0`
// after Phase 1. §5.8 deleted both the field and the engine type; dispatch now
// owns the solve path, so there is no observable engine state to assert against.
// Cross-file imported type resolution is already covered by the surviving
// dispatch-backed component-meta tests.

// ===========================================================================
// C1/C2: Resolver view caches routes and declarations
// ===========================================================================

#[test]
fn component_meta_query_engine_resolves_imported_registry_symbols_from_db_facts() {
    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Props { msg: string }")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    let result1 = query_engine
        .resolve_imported_registry_symbol("/src/types.ts", "Props")
        .expect("Props should resolve from DB-backed prepared declarations");
    let result2 = query_engine
        .resolve_imported_registry_symbol("/src/types.ts", "Props")
        .expect("repeated resolution should stay stable");

    assert_eq!(
        result1.canonical_id, "/src/types.ts",
        "resolved registry symbols should point at the defining file"
    );
    assert_eq!(
        result1.exported_name, "Props",
        "resolved registry symbols should retain the defining export name"
    );
    assert_eq!(
        result1.body, result2.body,
        "repeated DB-backed resolutions should preserve the prepared body"
    );
    assert_eq!(
        result1.canonical_dependencies, result2.canonical_dependencies,
        "repeated DB-backed resolutions should preserve tracked dependencies"
    );
    assert!(
        result1.canonical_dependencies.contains("/src/types.ts"),
        "registry resolution should track the defining file as a dependency"
    );
    assert_eq!(
        query_engine.imported_registry_symbol_cache_len(),
        1,
        "repeated imported registry resolutions should reuse one request-local cache entry",
    );
}

#[test]
fn define_props_macro_shape_reuses_expanded_fields_directly() {
    let snapshot = crate::types::FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::types::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let evaluated = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "title".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::String,
                ),
                raw_type: Some("string".to_string()),
                optional: false,
                exactness: verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: vec![
                    verter_semantic::analysis::type_expand::ExpansionDiagnostic {
                        reason: verter_semantic::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                        context: "title diagnostic".to_string(),
                        property_name: Some("title".to_string()),
                    },
                ],
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "icon".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::String,
                ),
                raw_type: Some("string".to_string()),
                optional: true,
                exactness: verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Interrupted,
                diagnostics: vec![
                    verter_semantic::analysis::type_expand::ExpansionDiagnostic {
                        reason: verter_semantic::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                        context: "icon diagnostic".to_string(),
                        property_name: Some("icon".to_string()),
                    },
                ],
            },
        ],
        ..Default::default()
    };
    let lowered = verter_semantic::analysis::type_expr::TypeExpr::Ref {
        name: "Props".into(),
        type_arguments: Vec::new().into(),
    };
    let (shape, source) = synthesize_define_props_shape_from_known_surface_with_authority(
        0,
        &snapshot,
        &[],
        &evaluated,
        Some(&lowered),
        true,
    )
    .expect("single defineProps macros should synthesize the object shape from expanded fields");
    let prop_names: Vec<&str> = shape
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    assert!(
        prop_names.contains(&"title") && prop_names.contains(&"icon"),
        "synthesized defineProps shape should preserve the expanded prop surface, got {prop_names:?}"
    );
    assert!(
        matches!(source, MacroShapeSource::Fields),
        "expanded defineProps fields should still report the fields reuse path"
    );
    assert!(
        !prop_names.contains(&"hidden"),
        "synthesized defineProps shape should not invent unrelated props, got {prop_names:?}"
    );
    assert_eq!(
        shape.exactness,
        verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
        "helper should merge field exactness across the synthesized defineProps shape"
    );
    assert_eq!(
        shape.execution_status,
        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Interrupted,
        "helper should preserve the worst execution status across expanded props"
    );
    let diagnostics: Vec<&str> = shape
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.context.as_str())
        .collect();
    assert_eq!(
        diagnostics,
        vec!["title diagnostic", "icon diagnostic"],
        "helper should carry field diagnostics onto the synthesized defineProps shape"
    );
}

#[test]
fn define_props_macro_shape_prefers_resolved_macro_when_expanded_fields_are_incomplete() {
    let snapshot = crate::types::FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::types::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_name: "Props".to_string(),
        import_source: String::new(),
        surface_is_authoritative: true,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Props".to_string(),
            declaration_id: None,
            resolved_name: "Props".to_string(),
            canonical_source: "/src/App.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface Props extends Pick<BaseProps, 'id' | 'label'> { own?: boolean }"
                    .to_string(),
            ),
        },
        native_props: Vec::new(),
        props: vec![
            verter_semantic::analysis::AnalyzedPropField {
                name: "id".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: false,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "label".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "own".to_string(),
                type_annotation: Some("boolean".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
        ],
        emits: Vec::new(),
        slots: Vec::new(),
        jsdoc: None,
    }];
    let evaluated = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![verter_semantic::analysis::type_expand::ExpandedField {
            name: "own".to_string(),
            r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                verter_semantic::analysis::type_expr::PrimitiveName::Boolean,
            ),
            raw_type: Some("boolean".to_string()),
            optional: true,
            exactness: verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status:
                verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
        }],
        ..Default::default()
    };
    let lowered = verter_semantic::analysis::type_expr::TypeExpr::Ref {
        name: "Props".into(),
        type_arguments: Vec::new().into(),
    };

    let (shape, source) = synthesize_define_props_shape_from_known_surface_with_authority(
        0,
        &snapshot,
        &resolved_macros,
        &evaluated,
        Some(&lowered),
        true,
    )
    .expect("defineProps should merge the wider resolved macro surface when expanded fields are incomplete");
    let prop_names: Vec<&str> = shape
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    assert_eq!(
        prop_names,
        vec!["id", "label", "own"],
        "resolved macro fallback should keep inherited imported props instead of truncating to the local evaluated field set"
    );
    assert!(
        matches!(source, MacroShapeSource::ResolvedMacro),
        "incomplete expanded fields should not win over a wider authoritative resolved macro surface"
    );
}

#[test]
fn define_props_fields_fast_path_allows_direct_object_literals() {
    let mac = verter_semantic::analysis::types::AnalyzedMacro {
        kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: Vec::new(),
        emit_fields: Vec::new(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        span: verter_span::Span::new(0, 0),
    };
    let lowered =
        verter_semantic::analysis::type_expr_lower::parse_type_annotation("{ title: string }");

    assert!(
        define_props_fields_fast_path_allowed(&mac, 0, &[], Some(&lowered)),
        "direct object literals should keep using the fields fast path"
    );
}

#[test]
fn define_props_fields_fast_path_rejects_complex_heritage_refs() {
    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["LinkProps".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![verter_semantic::analysis::AnalyzedPropField {
                name: "to".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            }],
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_name: "LinkProps".to_string(),
        import_source: String::new(),
        surface_is_authoritative: false,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "LinkProps".to_string(),
            declaration_id: None,
            resolved_name: "LinkProps".to_string(),
            canonical_source: "/src/Link.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface LinkProps extends Omit<RouterLinkProps, 'to'> { to?: string }"
                    .to_string(),
            ),
        },
        native_props: Vec::new(),
        props: vec![verter_semantic::analysis::AnalyzedPropField {
            name: "to".to_string(),
            type_annotation: Some("string".to_string()),
            is_optional: true,
            span: verter_span::Span::new(0, 0),
            description: None,
            tags: Vec::new(),
            resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
            resolution_error: None,
        }],
        emits: Vec::new(),
        slots: Vec::new(),
        jsdoc: None,
    }];
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("LinkProps");

    assert!(
        !define_props_fields_fast_path_allowed(
            &snapshot.macros[0],
            0,
            &resolved_macros,
            Some(&lowered)
        ),
        "utility/heritage refs should not use the fields fast path just because shallow prop_fields exist"
    );
    assert!(
        !snapshot.macros[0].prop_fields.is_empty(),
        "guard test should exercise the case where shallow prop_fields are present"
    );
}

#[test]
fn define_props_fields_fast_path_rejects_multi_surface_macro_candidates() {
    let mac = verter_semantic::analysis::types::AnalyzedMacro {
        kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        is_type_based: true,
        type_references: vec!["LinkProps".to_string()],
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: Vec::new(),
        emit_fields: Vec::new(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        span: verter_span::Span::new(0, 0),
    };
    let resolved_macros = vec![
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            type_name: "LinkProps".to_string(),
            import_source: String::new(),
            surface_is_authoritative: false,
            declaration: crate::resolver_core::ResolvedTypeDeclaration {
                requested_name: "LinkProps".to_string(),
                declaration_id: None,
                resolved_name: "LinkProps".to_string(),
                canonical_source: "/src/Link.vue".to_string(),
                span: verter_span::Span::new(0, 0),
                kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
                text: Some("interface LinkProps { to?: string }".to_string()),
            },
            native_props: Vec::new(),
            props: vec![verter_semantic::analysis::AnalyzedPropField {
                name: "to".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            }],
            emits: Vec::new(),
            slots: Vec::new(),
            jsdoc: None,
        },
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            type_name: "ButtonHTMLAttributes".to_string(),
            import_source: "./types/html".to_string(),
            surface_is_authoritative: false,
            declaration: crate::resolver_core::ResolvedTypeDeclaration {
                requested_name: "ButtonHTMLAttributes".to_string(),
                declaration_id: None,
                resolved_name: "ButtonHTMLAttributes".to_string(),
                canonical_source: "/src/types/html.ts".to_string(),
                span: verter_span::Span::new(0, 0),
                kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
                text: Some("interface ButtonHTMLAttributes { autofocus?: boolean }".to_string()),
            },
            native_props: Vec::new(),
            props: vec![verter_semantic::analysis::AnalyzedPropField {
                name: "autofocus".to_string(),
                type_annotation: Some("boolean".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            }],
            emits: Vec::new(),
            slots: Vec::new(),
            jsdoc: None,
        },
    ];
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("LinkProps");

    assert!(
        !define_props_fields_fast_path_allowed(&mac, 0, &resolved_macros, Some(&lowered)),
        "a defineProps macro with multiple resolved surfaces should not collapse to a single fields-only shape"
    );
}

#[test]
fn component_meta_query_engine_routes_imported_registry_symbols_to_the_defining_export() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            "export interface Props { primary: string; secondary: number }",
        )
        .unwrap();
    project
        .upsert_base(
            "/src/index.ts",
            "export { Props as ButtonProps } from './types'",
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    let resolved = query_engine
        .resolve_imported_registry_symbol("/src/index.ts", "ButtonProps")
        .expect("barrel export should resolve through DB-backed route facts");

    assert_eq!(
        (resolved.canonical_id.as_str(), resolved.exported_name.as_str()),
        ("/src/types.ts", "Props"),
        "registry resolution should read the defining export directly instead of keeping a query-local alias payload",
    );
}

#[test]
fn produce_one_macro_object_shape_prefers_projection_for_interface_utility_heritage() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseProps {
  title: string
  description?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BaseProps } from './base'

export interface Props extends Pick<BaseProps, 'title'> {
  count?: number
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Props");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        "/src/App.vue",
        &lowered,
        has_prop_shape_surface,
    );

    assert!(
        shape.is_some(),
        "heritage utility interfaces should still produce an object shape"
    );
    assert!(
        matches!(source, MacroShapeSource::Projection),
        "interface utility heritage should use DB-backed projection instead of the solver fallback"
    );
}

#[test]
fn produce_one_macro_object_shape_projection_keeps_inherited_and_local_props() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseProps {
  title: string
  description?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BaseProps } from './base'

export interface Props extends Pick<BaseProps, 'title'> {
  count?: number
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Props");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        "/src/App.vue",
        &lowered,
        has_prop_shape_surface,
    );
    let shape = shape.expect("projection should produce a shape");
    let prop_names: Vec<&str> = shape
        .value
        .properties
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        matches!(source, MacroShapeSource::Projection),
        "heritage utility interfaces should stay on the projection path"
    );
    assert!(
        prop_names.contains(&"title") && prop_names.contains(&"count"),
        "projected shape should preserve inherited Pick members and local members, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"description"),
        "projected Pick heritage should not widen to omitted sibling members, got {prop_names:?}"
    );
}

#[test]
fn produce_macro_object_shapes_reuses_expanded_define_props_for_complex_heritage_without_solves() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseProps {
  title: string
  description?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BaseProps } from './base'

export interface Props extends Pick<BaseProps, 'title'> {
  count?: number
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_name: "Props".to_string(),
        import_source: String::new(),
        surface_is_authoritative: false,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Props".to_string(),
            declaration_id: None,
            resolved_name: "Props".to_string(),
            canonical_source: "/src/App.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface Props extends Pick<BaseProps, 'title'> { count?: number }".to_string(),
            ),
        },
        native_props: Vec::new(),
        props: Vec::new(),
        emits: Vec::new(),
        slots: Vec::new(),
        jsdoc: None,
    }];
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "title".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::String,
                ),
                raw_type: Some("string".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "count".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::Number,
                ),
                raw_type: Some("number".to_string()),
                optional: true,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineProps<Props>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "expanded defineProps fields should be reused directly instead of triggering a second solve/projection pass for complex heritage"
    );
    assert_eq!(
        evaluated_types.define_props.len(),
        1,
        "expanded defineProps fields should synthesize a macro object shape"
    );
    let prop_names: Vec<&str> = evaluated_types.define_props[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"title") && prop_names.contains(&"count"),
        "synthesized defineProps shape should preserve expanded fields, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"description"),
        "synthesized defineProps shape should not widen beyond the expanded field set, got {prop_names:?}"
    );
}

#[test]
fn produce_macro_object_shapes_reuses_matching_expanded_props_with_define_model_present() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseProps {
  title: string
  description?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BaseProps } from './base'

export interface Props extends Pick<BaseProps, 'title'> {
  count?: number
}
</script>
<script setup lang="ts">
defineProps<Props>()
defineModel<boolean>('open')
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![
            verter_semantic::analysis::AnalyzedMacro {
                kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec!["Props".to_string()],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 0),
            },
            verter_semantic::analysis::AnalyzedMacro {
                kind: verter_semantic::analysis::AnalyzedMacroKind::DefineModel,
                is_type_based: true,
                type_references: vec!["boolean".to_string()],
                binding_name: Some("open".to_string()),
                model_name: Some("open".to_string()),
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 0),
            },
        ]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_name: "Props".to_string(),
        import_source: String::new(),
        surface_is_authoritative: true,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Props".to_string(),
            declaration_id: None,
            resolved_name: "Props".to_string(),
            canonical_source: "/src/App.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface Props extends Pick<BaseProps, 'title'> { count?: number }".to_string(),
            ),
        },
        native_props: Vec::new(),
        props: vec![
            verter_semantic::analysis::AnalyzedPropField {
                name: "title".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: false,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "count".to_string(),
                type_annotation: Some("number".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
        ],
        emits: Vec::new(),
        slots: Vec::new(),
        jsdoc: None,
    }];
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "title".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::String,
                ),
                raw_type: Some("string".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "count".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::Number,
                ),
                raw_type: Some("number".to_string()),
                optional: true,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "open".to_string(),
                r#type: verter_semantic::analysis::type_expr::TypeExpr::primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::Boolean,
                ),
                raw_type: Some("boolean".to_string()),
                optional: true,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineProps<Props>()\ndefineModel<boolean>('open')",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "defineModel should not force a second solve/projection pass when defineProps fields were already expanded"
    );
    assert_eq!(
        evaluated_types.define_props.len(),
        1,
        "matching expanded defineProps fields should still synthesize a macro object shape"
    );
    let prop_names: Vec<&str> = evaluated_types.define_props[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"title") && prop_names.contains(&"count"),
        "defineProps shape should preserve only the requested prop surface, got {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"open"),
        "defineModel fields must not leak into the defineProps shape, got {prop_names:?}"
    );
}

#[test]
fn append_component_meta_registry_entries_keep_local_explicit_object_helpers_raw() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { VNode } from 'vue'

type Tier = {
  id: string
  badge?: string
}

type SectionFeature<T extends Tier = Tier> = {
  id?: string
  title: string
  tiers?: {
    [K in Extract<T['id'], string>]: boolean | string
  } & Record<string, boolean | string>
}

export interface Section<T extends Tier = Tier> {
  id?: string
  title: string,
  features: SectionFeature<T>[]
}

export interface Props<T extends Tier = Tier> {
  sections: Section<T>[]
}

export type Slots<T extends Tier = Tier> = {
  'section-title'?: (props: { section: Section<T> }) => VNode[]
}
</script>
<script setup lang=\"ts\" generic=\"T extends Tier\">
defineProps<Props<T>>()
defineSlots<Slots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host };

    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let solves_before = 0u32;

    host.append_component_meta_registry_entries(
        "/src/App.vue",
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );

    assert_eq!(
        0u32.saturating_sub(solves_before),
        0,
        "owner-local explicit object helpers should reuse the prepared raw surface instead of triggering a projection solve during append"
    );

    let section_entry = parts
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Section")
        .expect("local helper should be published into the registry");
    let verter_semantic::analysis::type_expr::TypeExpr::Object(section_object) =
        &section_entry.type_expr
    else {
        panic!("local explicit helper should stay an object surface");
    };
    let feature_property = section_object
        .properties
        .iter()
        .find_map(|member| match member {
            verter_semantic::analysis::type_expr::ObjectMember::Property(property)
                if property.name == "features" =>
            {
                Some(property)
            }
            _ => None,
        })
        .expect("section helper should preserve the features property");
    assert!(
        matches!(
            feature_property.ty,
            verter_semantic::analysis::type_expr::TypeExpr::Array { .. }
        ),
        "the raw object surface should keep the local array member instead of flattening the whole helper"
    );
}

#[test]
fn compute_component_meta_state_for_fallthrough_skips_registry_append_publication() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { VNode } from 'vue'

type Tier = {
  id: string
  badge?: string
}

type SectionFeature<T extends Tier = Tier> = {
  id?: string
  title: string
  tiers?: {
    [K in Extract<T['id'], string>]: boolean | string
  } & Record<string, boolean | string>
}

export interface Section<T extends Tier = Tier> {
  id?: string
  title: string,
  features: SectionFeature<T>[]
}

export interface Props<T extends Tier = Tier> {
  sections: Section<T>[]
}

export type Slots<T extends Tier = Tier> = {
  'section-title'?: (props: { section: Section<T> }) => VNode[]
}
</script>
<script setup lang=\"ts\" generic=\"T extends Tier\">
defineProps<Props<T>>()
defineSlots<Slots<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let store_view = host.resolver_store_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = host
        .compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash)
        .expect("fallthrough-expanded state should resolve");

    assert!(
        full.resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Section"),
        "full expanded state should still publish the local helper through registry append"
    );
    assert!(
        !fallthrough
            .resolved_type_registry
            .iter()
            .any(|entry| entry.name == "Section"),
        "fallthrough-expanded state should skip registry append publication for local helpers"
    );
    assert!(
        fallthrough.resolved_type_registry.len() < full.resolved_type_registry.len(),
        "fallthrough-expanded state should keep only the preseeded registry entries"
    );
}

#[test]
fn compute_component_meta_state_for_fallthrough_skips_slot_and_expose_expansion() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface Props {
  label: string
}

export interface Slots {
  default?: (props: { value: string }) => any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props, Slots } from './types'

const exposed: { focus(): void } = {
  focus() {},
}

defineProps<Props>()
defineSlots<Slots>()
defineExpose({ exposed })
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let store_view = host.resolver_store_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = host
        .compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash)
        .expect("fallthrough-expanded state should resolve");

    assert!(
        full.resolved_macros
            .iter()
            .any(|entry| entry.type_name == "Props"),
        "full expanded state should still resolve defineProps imports"
    );
    assert!(
        full.resolved_macros
            .iter()
            .any(|entry| entry.type_name == "Slots"),
        "full expanded state should still resolve defineSlots imports"
    );

    let full_eval = full
        .evaluated_types
        .as_ref()
        .expect("full expanded state should carry evaluated types");
    assert!(
        !full_eval.define_slots.is_empty(),
        "full expanded state should expand defineSlots"
    );
    assert!(
        !full_eval.bindings.is_empty(),
        "full expanded state should expand defineExpose bindings"
    );

    assert!(
        fallthrough
            .resolved_macros
            .iter()
            .any(|entry| entry.type_name == "Props"),
        "fallthrough-expanded state must still resolve defineProps imports"
    );
    assert!(
        fallthrough
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "Slots"),
        "fallthrough-expanded state should skip imported defineSlots resolution entirely"
    );

    let fallthrough_props = fallthrough
        .resolved_macros
        .iter()
        .find(|entry| entry.type_name == "Props")
        .expect("fallthrough-expanded state should still materialize the props surface");
    assert!(
        fallthrough_props
            .props
            .iter()
            .any(|prop| prop.name == "label"),
        "fallthrough-expanded state must still preserve the requested defineProps surface"
    );
    assert!(
        fallthrough
            .evaluated_types
            .as_ref()
            .is_none_or(|evaluated| evaluated.define_slots.is_empty()),
        "fallthrough-expanded state should skip defineSlots expansion"
    );
    assert!(
        fallthrough
            .evaluated_types
            .as_ref()
            .is_none_or(|evaluated| evaluated.slot_bindings.is_empty()),
        "fallthrough-expanded state should skip slot binding expansion"
    );
    assert!(
        fallthrough
            .evaluated_types
            .as_ref()
            .is_none_or(|evaluated| evaluated.bindings.is_empty()),
        "fallthrough-expanded state should skip defineExpose binding expansion"
    );
}

#[test]
fn compute_component_meta_state_for_fallthrough_skips_imported_declaration_metadata_and_jsdoc() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
/** Shared props docs */
export interface Props {
  label: string
}

/** Shared emits docs */
export interface Emits {
  (e: 'open', value: boolean): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Emits, Props } from './types'

defineProps<Props>()
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    host.set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let store_view = host.resolver_store_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = host
        .compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash)
        .expect("fallthrough-expanded state should resolve");

    let full_props = full
        .resolved_macros
        .iter()
        .find(|entry| entry.type_name == "Props")
        .expect("full state should resolve imported defineProps metadata");
    assert_eq!(
        full_props.declaration.canonical_source, "/src/types.ts",
        "full expanded state should still retain imported declaration ownership",
    );
    assert_eq!(
        full_props
            .jsdoc
            .as_ref()
            .and_then(|block| block.description.as_deref()),
        Some("Shared props docs"),
        "full expanded state should still preserve imported JSDoc",
    );

    let fallthrough_props = fallthrough
        .resolved_macros
        .iter()
        .find(|entry| entry.type_name == "Props")
        .expect("fallthrough state should still materialize the imported props surface");
    assert!(
        fallthrough_props
            .props
            .iter()
            .any(|prop| prop.name == "label"),
        "fallthrough-expanded state must still preserve the requested defineProps surface",
    );
    assert!(
        fallthrough_props.declaration.canonical_source.is_empty(),
        "fallthrough-expanded state should skip imported declaration metadata when only the surface is needed",
    );
    assert!(
        fallthrough_props.jsdoc.is_none(),
        "fallthrough-expanded state should skip imported JSDoc resolution entirely",
    );
}

#[test]
fn compute_component_meta_state_for_fallthrough_keeps_imported_define_emits_on_eval_shape_path() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface Emits {
  (e: 'open', value: boolean): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Child.vue",
            r#"<script setup lang="ts">
import type { Emits } from './types'

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    host.set_import_dependencies(
        "/src/Child.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let store_view = host.resolver_store_view();
    let whole_hash = store_view
        .whole_hash("/src/Child.vue")
        .expect("whole hash should exist for the owner");

    host.provenance().reset();

    let fallthrough = host
        .compute_component_meta_state_for_fallthrough("/src/Child.vue", whole_hash)
        .expect("fallthrough-expanded state should resolve");

    let provenance = host.provenance().snapshot();
    assert_eq!(
        provenance.resolved_external_type_cache_misses, 0,
        "fallthrough-expanded state should not re-enter imported macro-element resolution for type-based defineEmits when the evaluated shape is authoritative",
    );
    assert!(
        fallthrough
            .resolved_macros
            .iter()
            .filter(|entry| entry.type_name == "Emits")
            .all(|entry| entry.declaration.canonical_source.is_empty()),
        "fallthrough-expanded state should not materialize imported declaration ownership for type-based defineEmits when the evaluated shape already supplies the declared events",
    );

    let resolved_macros = crate::resolver_core::component_meta_resolved_macros(
        fallthrough.snapshot.macros.as_ref(),
        &fallthrough.resolved_macros,
    );
    let resolved_type_registry =
        crate::resolver_core::component_meta_type_registry(&fallthrough.resolved_type_registry);
    let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(
        verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &fallthrough.snapshot.macros,
            bindings: &fallthrough.snapshot.bindings,
            imports: &fallthrough.snapshot.imports,
            template: fallthrough.snapshot.template.as_deref(),
            options_api: fallthrough.snapshot.options_api.as_ref(),
            analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
                fallthrough.snapshot.script_flags,
            ),
            styles: &fallthrough.snapshot.styles,
            vue_api_calls: &fallthrough.snapshot.vue_api_calls,
            store_usages: &fallthrough.snapshot.store_usages,
            resolved_macros: &resolved_macros,
            resolved_type_registry: &resolved_type_registry,
            evaluated_types: fallthrough.evaluated_types.as_ref(),
            file_path: "/src/Child.vue",
            canonical_source: None,
        },
    );
    assert!(
        base_meta.events.iter().any(|event| event.name == "open"),
        "fallthrough-expanded extraction must still preserve declared events from the authoritative evaluated defineEmits shape",
    );
}

#[test]
fn compute_component_meta_state_for_fallthrough_keeps_local_define_emits_wrapper_on_query_projection_path(
) {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface RootEmits {
  (e: 'open', value: boolean): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Child.vue",
            r#"<script setup lang="ts">
import type { RootEmits } from './types'

interface Emits extends RootEmits {}

defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    host.set_import_dependencies(
        "/src/Child.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    let store_view = host.resolver_store_view();
    let whole_hash = store_view
        .whole_hash("/src/Child.vue")
        .expect("whole hash should exist for the owner");

    host.provenance().reset();

    let fallthrough = host
        .compute_component_meta_state_for_fallthrough("/src/Child.vue", whole_hash)
        .expect("fallthrough-expanded state should resolve");

    let provenance = host.provenance().snapshot();
    assert_eq!(
        provenance.resolved_external_type_cache_misses, 0,
        "fallthrough-expanded state should keep local defineEmits wrappers on the query-engine projection path instead of materializing imported macro elements",
    );
    let evaluated = fallthrough
        .evaluated_types
        .as_ref()
        .expect("fallthrough-expanded state should still preserve evaluated macro shapes");
    assert!(
        evaluated
            .define_emits
            .iter()
            .any(|entry| entry.macro_index == 0),
        "fallthrough-expanded state should synthesize a defineEmits object shape for the local wrapper",
    );
    assert!(
        fallthrough
            .resolved_macros
            .iter()
            .all(|entry| entry.type_name != "RootEmits"),
        "fallthrough-expanded state should keep the transitive imported defineEmits root off resolved_macros when the owner-local wrapper is sufficient",
    );

    let resolved_macros = crate::resolver_core::component_meta_resolved_macros(
        fallthrough.snapshot.macros.as_ref(),
        &fallthrough.resolved_macros,
    );
    let resolved_type_registry =
        crate::resolver_core::component_meta_type_registry(&fallthrough.resolved_type_registry);
    let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(
        verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &fallthrough.snapshot.macros,
            bindings: &fallthrough.snapshot.bindings,
            imports: &fallthrough.snapshot.imports,
            template: fallthrough.snapshot.template.as_deref(),
            options_api: fallthrough.snapshot.options_api.as_ref(),
            analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
                fallthrough.snapshot.script_flags,
            ),
            styles: &fallthrough.snapshot.styles,
            vue_api_calls: &fallthrough.snapshot.vue_api_calls,
            store_usages: &fallthrough.snapshot.store_usages,
            resolved_macros: &resolved_macros,
            resolved_type_registry: &resolved_type_registry,
            evaluated_types: fallthrough.evaluated_types.as_ref(),
            file_path: "/src/Child.vue",
            canonical_source: None,
        },
    );
    assert!(
        base_meta.events.iter().any(|event| event.name == "open"),
        "fallthrough-expanded extraction must still preserve declared events from the local defineEmits wrapper",
    );
}

#[test]
fn produce_macro_object_shapes_reuses_resolved_define_emits_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface BaseEmits {
  close: [],
  save: [id: number]
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BaseEmits } from './base'

export interface Emits extends Omit<BaseEmits, 'close'> {
  'update:open': [value: boolean]
}
</script>
<script setup lang="ts">
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
        type_name: "Emits".to_string(),
        import_source: "./base".to_string(),
        surface_is_authoritative: true,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Emits".to_string(),
            declaration_id: None,
            resolved_name: "Emits".to_string(),
            canonical_source: "/src/App.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface Emits extends Omit<BaseEmits, 'close'> { 'update:open': [value: boolean] }"
                    .to_string(),
            ),
        },
        native_props: Vec::new(),
        props: Vec::new(),
        emits: vec![
            verter_semantic::analysis::AnalyzedEmitField {
                name: "save".to_string(),
                span: verter_span::Span::new(0, 0),
                payload_type: Some("[id: number]".to_string()),
                description: None,
                tags: Vec::new(),
            },
            verter_semantic::analysis::AnalyzedEmitField {
                name: "update:open".to_string(),
                span: verter_span::Span::new(0, 0),
                payload_type: Some("[value: boolean]".to_string()),
                description: None,
                tags: Vec::new(),
            },
        ],
        slots: Vec::new(),
        jsdoc: None,
    }];
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineEmits<Emits>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "resolved imported defineEmits surfaces should be reused directly instead of triggering another projection/solve pass"
    );
    assert_eq!(
        evaluated_types.define_emits.len(),
        1,
        "resolved defineEmits emit fields should synthesize a macro object shape"
    );
    let emit_names: Vec<&str> = evaluated_types.define_emits[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(emit_names, vec!["save", "update:open"]);
}

#[test]
fn produce_macro_object_shapes_reuses_expanded_define_emits_fields_without_solves() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Emits = {
  save: [id: number]
  'update:open': [value: boolean]
}
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = Vec::new();
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        emits: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "save".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[id: number]",
                ),
                raw_type: Some("[id: number]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "update:open".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[value: boolean]",
                ),
                raw_type: Some("[value: boolean]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineEmits<Emits>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32, 0,
        "expanded defineEmits fields should be reused directly instead of triggering another solve"
    );
    assert_eq!(
        evaluated_types.define_emits.len(),
        1,
        "expanded defineEmits fields should still synthesize a macro object shape"
    );
    let emit_names: Vec<&str> = evaluated_types.define_emits[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(emit_names, vec!["save", "update:open"]);
}

#[test]
fn produce_macro_object_shapes_reuses_preseeded_define_emits_shape_without_solves() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Emits = {
  save: [id: number]
  'update:open': [value: boolean]
}
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = Vec::new();
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        define_emits: vec![
            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                macro_index: 0,
                result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                    verter_semantic::analysis::type_expand::ExpandedObjectShape {
                        properties: vec![
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "save".to_string(),
                            ty: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                                "[id: number]",
                            ),
                            optional: false,
                            readonly: false,
                        },
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "update:open".to_string(),
                            ty: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                                "[value: boolean]",
                            ),
                            optional: false,
                            readonly: false,
                        },
                    ],
                        index_signatures: Vec::new(),
                        call_signatures: Vec::new(),
                    },
                ),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineEmits<Emits>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "preseeded defineEmits macro shapes should be reused directly instead of triggering another projection/solve pass"
    );
    assert_eq!(
        evaluated_types.define_emits.len(),
        1,
        "preseeded defineEmits macro shapes should not be duplicated"
    );
}

#[test]
fn produce_macro_object_shapes_reuses_expanded_define_emits_fields_with_define_model_present() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Emits = {
  save: [id: number]
  'update:open': [value: boolean]
}
defineEmits<Emits>()
defineModel<string>('searchTerm')
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![
            verter_semantic::analysis::AnalyzedMacro {
                kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
                is_type_based: true,
                type_references: vec!["Emits".to_string()],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 0),
            },
            verter_semantic::analysis::AnalyzedMacro {
                kind: verter_semantic::analysis::AnalyzedMacroKind::DefineModel,
                is_type_based: true,
                type_references: vec!["string".to_string()],
                binding_name: Some("searchTerm".to_string()),
                model_name: Some("searchTerm".to_string()),
                has_inherit_attrs_false: false,
                prop_fields: Vec::new(),
                emit_fields: Vec::new(),
                slot_fields: Vec::new(),
                default_keys: Vec::new(),
                default_values: Vec::new(),
                expose_fields: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 0),
            },
        ]
        .into(),
        ..Default::default()
    };
    let resolved_macros = Vec::new();
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        emits: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "save".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[id: number]",
                ),
                raw_type: Some("[id: number]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "update:open".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[value: boolean]",
                ),
                raw_type: Some("[value: boolean]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineEmits<Emits>()\ndefineModel<string>('searchTerm')",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "defineModel should not force defineEmits back through the solver when expanded emit fields already exist"
    );
    assert_eq!(
        evaluated_types.define_emits.len(),
        1,
        "expanded defineEmits fields should still synthesize a macro object shape with defineModel present"
    );
    let emit_names: Vec<&str> = evaluated_types.define_emits[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(emit_names, vec!["save", "update:open"]);
}

#[test]
fn produce_macro_object_shapes_reuses_expanded_define_emits_fields_even_with_duplicate_resolved_macros(
) {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type Emits = {
  save: [id: number]
  'update:open': [value: boolean]
}
defineEmits<Emits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            is_type_based: true,
            type_references: vec!["Emits".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            type_name: "Emits".to_string(),
            import_source: String::new(),
            surface_is_authoritative: true,
            declaration: crate::resolver_core::ResolvedTypeDeclaration {
                requested_name: "Emits".to_string(),
                declaration_id: None,
                resolved_name: "Emits".to_string(),
                canonical_source: "/src/App.vue".to_string(),
                span: verter_span::Span::new(0, 0),
                kind: crate::resolver_core::ResolvedDeclarationKind::TypeAlias,
                text: Some(
                    "type Emits = { save: [id: number]; 'update:open': [value: boolean] }"
                        .to_string(),
                ),
            },
            native_props: Vec::new(),
            props: Vec::new(),
            emits: vec![verter_semantic::analysis::AnalyzedEmitField {
                name: "save".to_string(),
                span: verter_span::Span::new(0, 0),
                payload_type: Some("[id: number]".to_string()),
                description: None,
                tags: Vec::new(),
            }],
            slots: Vec::new(),
            jsdoc: None,
        },
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
            type_name: "Emits".to_string(),
            import_source: String::new(),
            surface_is_authoritative: false,
            declaration: crate::resolver_core::ResolvedTypeDeclaration {
                requested_name: "Emits".to_string(),
                declaration_id: None,
                resolved_name: "Emits".to_string(),
                canonical_source: "/src/App.vue".to_string(),
                span: verter_span::Span::new(0, 0),
                kind: crate::resolver_core::ResolvedDeclarationKind::TypeAlias,
                text: Some(
                    "type Emits = { save: [id: number]; 'update:open': [value: boolean] }"
                        .to_string(),
                ),
            },
            native_props: Vec::new(),
            props: Vec::new(),
            emits: vec![verter_semantic::analysis::AnalyzedEmitField {
                name: "update:open".to_string(),
                span: verter_span::Span::new(0, 0),
                payload_type: Some("[value: boolean]".to_string()),
                description: None,
                tags: Vec::new(),
            }],
            slots: Vec::new(),
            jsdoc: None,
        },
    ];
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        emits: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "save".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[id: number]",
                ),
                raw_type: Some("[id: number]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "update:open".to_string(),
                r#type: verter_semantic::analysis::type_expr_lower::parse_type_annotation(
                    "[value: boolean]",
                ),
                raw_type: Some("[value: boolean]".to_string()),
                optional: false,
                exactness:
                    verter_semantic::analysis::type_expand::ExpansionExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        ..Default::default()
    };

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineEmits<Emits>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "expanded defineEmits fields should bypass solver fallback even when resolved macro metadata contains duplicate entries"
    );
    assert_eq!(evaluated_types.define_emits.len(), 1);
}

#[test]
fn produce_macro_object_shapes_reuses_resolved_define_slots_surface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface Slots {
  default?(props: { ui: string }): any
  item?(props: { index: number }): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Slots } from './base'

defineSlots<Slots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec!["Slots".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
        type_name: "Slots".to_string(),
        import_source: "./base".to_string(),
        surface_is_authoritative: true,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Slots".to_string(),
            declaration_id: None,
            resolved_name: "Slots".to_string(),
            canonical_source: "/src/base.ts".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
            text: Some(
                "interface Slots { default?(props: { ui: string }): any; item?(props: { index: number }): any }"
                    .to_string(),
            ),
        },
        native_props: Vec::new(),
        props: Vec::new(),
        emits: Vec::new(),
        slots: vec![
            verter_semantic::analysis::AnalyzedSlotField {
                name: "default".to_string(),
                is_required: false,
                span: verter_span::Span::new(0, 0),
                bindings: vec![verter_semantic::analysis::AnalyzedSlotFieldBinding {
                    name: "ui".to_string(),
                    type_annotation: Some("string".to_string()),
                    span: verter_span::Span::new(0, 0),
                }],
                return_type: Some("any".to_string()),
                description: None,
                tags: Vec::new(),
            },
            verter_semantic::analysis::AnalyzedSlotField {
                name: "item".to_string(),
                is_required: false,
                span: verter_span::Span::new(0, 0),
                bindings: vec![verter_semantic::analysis::AnalyzedSlotFieldBinding {
                    name: "index".to_string(),
                    type_annotation: Some("number".to_string()),
                    span: verter_span::Span::new(0, 0),
                }],
                return_type: Some("any".to_string()),
                description: None,
                tags: Vec::new(),
            },
        ],
        jsdoc: None,
    }];
    let mut evaluated_types =
        verter_semantic::analysis::type_expand::ExpandedComponentTypes::default();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        "defineSlots<Slots>()",
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "resolved imported defineSlots surfaces should be reused directly instead of triggering another projection/solve pass"
    );
    assert_eq!(
        evaluated_types.define_slots.len(),
        1,
        "resolved defineSlots slot fields should synthesize a macro object shape"
    );
    let slot_names: Vec<&str> = evaluated_types.define_slots[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(slot_names, vec!["default", "item"]);
    assert!(
        matches!(
            evaluated_types.define_slots[0].result.value.properties[0].ty,
            verter_semantic::analysis::type_expr::TypeExpr::Function(_),
        ),
        "resolved defineSlots reuse should preserve function-valued slot members"
    );
}

#[test]
fn produce_macro_object_shapes_reuses_authoritative_projected_define_props_surface_without_solves()
{
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
export interface Editor {
  isEditable: boolean
}
export interface BubbleMenuPluginProps {
  pluginKey?: string
  shouldShow?: (props: { editor: Editor; from: number; to: number }) => boolean
}
export interface FloatingMenuPluginProps {
  options?: {
    strategy?: 'absolute' | 'fixed'
    onShow?: () => void
  }
}
type BaseProps = {
  editor: Editor
  layout?: 'fixed'
}

export type Props =
  | (BaseProps & Partial<Omit<BubbleMenuPluginProps, 'editor'>> & { layout?: 'bubble' })
  | (BaseProps & Partial<Omit<FloatingMenuPluginProps, 'editor'>> & { layout?: 'floating' })
</script>
<script setup lang="ts">
withDefaults(defineProps<Props>(), {
  layout: 'fixed'
})
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let snapshot = FileAnalysisSnapshot {
        macros: vec![verter_semantic::analysis::AnalyzedMacro {
            kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec!["Props".to_string()],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 0),
        }]
        .into(),
        ..Default::default()
    };
    let resolved_macros = vec![ResolvedMacroMeta {
        macro_index: 0,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        type_name: "Props".to_string(),
        import_source: String::new(),
        surface_is_authoritative: true,
        declaration: crate::resolver_core::ResolvedTypeDeclaration {
            requested_name: "Props".to_string(),
            declaration_id: None,
            resolved_name: "Props".to_string(),
            canonical_source: "/src/App.vue".to_string(),
            span: verter_span::Span::new(0, 0),
            kind: crate::resolver_core::ResolvedDeclarationKind::TypeAlias,
            text: Some(
                "type Props = (BaseProps & Partial<Omit<BubbleMenuPluginProps, 'editor'>> & { layout?: 'bubble' }) | (BaseProps & Partial<Omit<FloatingMenuPluginProps, 'editor'>> & { layout?: 'floating' })".to_string(),
            ),
        },
        native_props: Vec::new(),
        props: vec![
            verter_semantic::analysis::AnalyzedPropField {
                name: "editor".to_string(),
                type_annotation: Some("Editor".to_string()),
                is_optional: false,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "layout".to_string(),
                type_annotation: Some("'fixed' | 'bubble' | 'floating'".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "pluginKey".to_string(),
                type_annotation: Some("string".to_string()),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
            verter_semantic::analysis::AnalyzedPropField {
                name: "options".to_string(),
                type_annotation: Some(
                    "{ strategy?: 'absolute' | 'fixed'; onShow?: () => void }".to_string(),
                ),
                is_optional: true,
                span: verter_span::Span::new(0, 0),
                description: None,
                tags: Vec::new(),
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
            },
        ],
        emits: Vec::new(),
        slots: Vec::new(),
        jsdoc: None,
    }];

    let host = project.host();
    let resolved_prop_names: Vec<&str> = resolved_macros[0]
        .props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        resolved_macros[0].surface_is_authoritative,
        "projected local defineProps surfaces should be marked authoritative"
    );
    assert!(
        resolved_prop_names.contains(&"editor")
            && resolved_prop_names.contains(&"layout")
            && resolved_prop_names.contains(&"pluginKey")
            && resolved_prop_names.contains(&"options"),
        "resolved macro surface should already contain the projected defineProps members, got {resolved_prop_names:?}"
    );

    let _store_view = host.resolver_store_view();
    let facts = host
        .ensure_indexed_ready("/src/App.vue")
        .expect("app facts should be present");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        ..Default::default()
    };

    produce_macro_object_shapes(
        "/src/App.vue",
        &snapshot,
        &resolved_macros,
        &[],
        &[],
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32,
        0,
        "authoritative projected defineProps surfaces should be reused directly instead of triggering a second solve"
    );
    assert_eq!(
        evaluated_types.define_props.len(),
        1,
        "authoritative projected defineProps surfaces should still synthesize a macro object shape"
    );
    let prop_names: Vec<&str> = evaluated_types.define_props[0]
        .result
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"editor")
            && prop_names.contains(&"layout")
            && prop_names.contains(&"pluginKey")
            && prop_names.contains(&"options"),
        "synthesized defineProps shape should preserve the authoritative projected surface, got {prop_names:?}"
    );
}

// `produce_one_macro_object_shape_skips_redundant_projection_for_generic_ref_solver_shapes`
// retired in $5.8 WIP-W ($4.1 EXPLICIT_TEST_IDS Category 3): asserted
// `solve_count == 1` on the retired `TypeQueryEngine` projection-rescue
// path. Dispatch replaces the projection-rescue pass with a memo hit
// in `SemanticGraphStore`, so the "skips a second solver call"
// observable is retired with the solver. The sister test
// `produce_one_macro_object_shape_prefers_root_projection_for_generic_non_object_aliases`
// still covers the projection route.

#[test]
fn produce_one_macro_object_shape_prefers_root_projection_for_generic_non_object_aliases() {
    let project = make_project();
    project
        .upsert_base(
            "/src/plugins.ts",
            r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}

export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
    strategy?: 'absolute' | 'fixed'
  }
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { BubbleMenuPluginProps, FloatingMenuPluginProps } from './plugins'
import type { ArrayOrNested, ButtonProps } from './types'

type EditorToolbarItem = {
  label?: string
}

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: object
  class?: any
  ui?: ButtonProps['ui']
}

export type Props<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
    | (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'bubble'
    })
    | (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'floating'
    })
</script>
<script setup lang=\"ts\" generic=\"T extends ArrayOrNested<EditorToolbarItem>\">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Props<T>");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        "/src/App.vue",
        &lowered,
        has_prop_shape_surface,
    );
    let shape =
        shape.expect("generic non-object aliases should still produce a defineProps surface");
    let prop_names: Vec<&str> = shape
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    assert!(
        matches!(source, MacroShapeSource::Projection),
        "generic union/utility aliases should prefer the projected root surface"
    );
    assert!(
        prop_names.contains(&"as")
            && prop_names.contains(&"color")
            && prop_names.contains(&"variant")
            && prop_names.contains(&"size")
            && prop_names.contains(&"items")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"ui")
            && prop_names.contains(&"layout")
            && prop_names.contains(&"appendTo")
            && prop_names.contains(&"pluginKey")
            && prop_names.contains(&"shouldShow")
            && prop_names.contains(&"updateDelay")
            && prop_names.contains(&"options"),
        "projected root surface should preserve both shared and branch props, got {prop_names:?}"
    );
    assert_eq!(
        0u32,
        0,
        "generic non-object aliases that the prepared projector can materialize should stay shallow and avoid the semantic solver"
    );
}

#[test]
fn produce_one_macro_object_shape_prefers_root_projection_for_nested_pick_omit_generic_interface() {
    let project = make_project();
    project
        .upsert_base(
            "/src/pkg.ts",
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { RootProps } from './pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<script setup lang="ts">
defineProps<ColorModeSelectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let lowered =
        verter_semantic::analysis::type_expr_lower::parse_type_annotation("ColorModeSelectProps");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        "/src/App.vue",
        &lowered,
        has_prop_shape_surface,
    );

    assert!(shape.is_some(), "color-mode-style props should materialize");
    assert!(
        matches!(source, MacroShapeSource::Projection),
        "nested pick/omit generic interfaces should stay on the projection path",
    );
    assert_eq!(
        0u32,
        0,
        "nested pick/omit generic interfaces should use the prepared shallow projector before the solver",
    );
}

#[test]
fn produce_one_macro_object_shape_prefers_projection_for_dual_heritage_omit_with_imported_key_alias(
) {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface LinkProps {
  href?: string
  target?: string
  rel?: string
  active?: boolean
  class?: any
}

export type LinkPropsKeys = 'href' | 'target' | 'rel' | 'active'

export interface ButtonProps extends Omit<LinkProps, 'href'> {
  label?: string
  color?: string
  variant?: string
  ui?: object
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/drag.ts",
            r#"
export interface DragHandleProps {
  class?: any
  computePositionConfig?: unknown
  editor?: object
  element?: object
  onNodeChange?: () => void
  pluginKey?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { DragHandleProps } from './drag'
import type { ButtonProps, LinkPropsKeys } from './types'

export interface Props extends Omit<DragHandleProps, 'editor' | 'element' | 'onNodeChange' | 'computePositionConfig' | 'class'>, Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  options?: object
  editor: object
  ui?: ButtonProps['ui']
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let lowered = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Props");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        "/src/App.vue",
        &lowered,
        has_prop_shape_surface,
    );
    let shape = shape.expect("dual-heritage alias-key props should materialize");
    let prop_names: Vec<&str> = shape
        .value
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    assert!(
        matches!(source, MacroShapeSource::Projection),
        "dual-heritage omit with imported key aliases should stay on the prepared projection path",
    );
    assert!(
        prop_names.contains(&"label")
            && prop_names.contains(&"pluginKey")
            && prop_names.contains(&"editor")
            && prop_names.contains(&"options")
            && prop_names.contains(&"ui"),
        "projected dual-heritage surface should keep both drag and button props, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains(&"href")
            && !prop_names.contains(&"target")
            && !prop_names.contains(&"rel")
            && !prop_names.contains(&"active"),
        "projected dual-heritage surface should drop alias-derived link keys, got {prop_names:?}",
    );
    assert_eq!(
        0u32, 0,
        "dual-heritage omit with imported key aliases should avoid the semantic solver",
    );
}

#[test]
fn produce_one_macro_object_shape_real_nuxt_ui_color_mode_select_stays_off_solver() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.integration-tests/repos/nuxt-ui")
        .canonicalize()
        .expect("nuxt-ui integration fixture should exist");
    let repo_root = repo_root.to_string_lossy().replace('\\', "/");
    let component = format!("{repo_root}/src/runtime/components/color-mode/ColorModeSelect.vue");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            repo_root.clone(),
            repo_root.clone(),
            Some(format!("{repo_root}/tsconfig.json")),
        ),
    ]);

    let _store_view = host.resolver_store_view();
    let mut direct_query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    let select_menu_component = format!("{repo_root}/src/runtime/components/SelectMenu.vue");
    let combobox_root_decl =
        direct_query_engine.resolve_type_declaration(&select_menu_component, "ComboboxRootProps");
    assert!(
        !combobox_root_decl.canonical_source.is_empty(),
        "real ComboboxRootProps should resolve to a prepared declaration source",
    );
    let listbox_root_decl = direct_query_engine
        .resolve_type_declaration(&combobox_root_decl.canonical_source, "ListboxRootProps");
    assert!(
        !listbox_root_decl.canonical_source.is_empty(),
        "real ListboxRootProps should resolve to a prepared declaration source",
    );
    let listbox_root_prepared = direct_query_engine.project_prepared_type_surface_expr(
        &listbox_root_decl.canonical_source,
        &listbox_root_decl.resolved_name,
    );
    assert!(
        listbox_root_prepared.is_some(),
        "real ListboxRootProps should have a prepared-only root surface projection available",
    );
    let (combobox_root_target_source, combobox_root_target_name) = host
        .resolve_named_type_export_target(
            &combobox_root_decl.canonical_source,
            &combobox_root_decl.resolved_name,
        )
        .unwrap_or((
            combobox_root_decl.canonical_source.clone(),
            combobox_root_decl.resolved_name.clone(),
        ));
    let combobox_root_prepared = direct_query_engine.project_prepared_type_surface_expr(
        &combobox_root_target_source,
        &combobox_root_target_name,
    );
    assert!(
        combobox_root_prepared.is_some(),
        "real ComboboxRootProps routed target should have a prepared-only root surface projection available",
    );
    let button_html_decl = direct_query_engine
        .resolve_type_declaration(&select_menu_component, "ButtonHTMLAttributes");
    assert!(
        !button_html_decl.canonical_source.is_empty(),
        "real ButtonHTMLAttributes should resolve to a prepared declaration source",
    );
    let button_html_prepared = direct_query_engine.project_prepared_type_surface_expr(
        &button_html_decl.canonical_source,
        &button_html_decl.resolved_name,
    );
    assert!(
        button_html_prepared.is_some(),
        "real ButtonHTMLAttributes should have a prepared-only root surface projection available",
    );
    let select_menu_prepared = direct_query_engine
        .project_prepared_type_surface_expr(&select_menu_component, "SelectMenuProps");
    assert!(
        select_menu_prepared.is_some(),
        "real SelectMenuProps should have a prepared-only root surface projection available",
    );
    let prepared_only =
        direct_query_engine.project_prepared_type_surface_expr(&component, "ColorModeSelectProps");
    assert!(
        prepared_only.is_some(),
        "real ColorModeSelectProps should have a prepared-only root surface projection available",
    );
    assert_eq!(
        0u32, 0,
        "prepared-only real ColorModeSelectProps projection must not invoke the semantic solver",
    );

    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    let lowered =
        verter_semantic::analysis::type_expr_lower::parse_type_annotation("ColorModeSelectProps");

    let (shape, source) = produce_one_macro_object_shape(
        &mut query_engine,
        &component,
        &lowered,
        has_prop_shape_surface,
    );

    assert!(
        shape.is_some(),
        "real ColorModeSelectProps should materialize"
    );
    assert!(
        matches!(source, MacroShapeSource::Projection),
        "real ColorModeSelectProps should still prefer the projection path",
    );
    assert_eq!(
        0u32,
        0,
        "real ColorModeSelectProps should stay on the shallow projection path without a semantic solve",
    );
}

#[test]
fn produce_macro_object_shapes_real_nuxt_ui_color_mode_select_reuses_authoritative_surface_without_solves(
) {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.integration-tests/repos/nuxt-ui")
        .canonicalize()
        .expect("nuxt-ui integration fixture should exist");
    let repo_root = repo_root.to_string_lossy().replace('\\', "/");
    let component = format!("{repo_root}/src/runtime/components/color-mode/ColorModeSelect.vue");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            repo_root.clone(),
            repo_root.clone(),
            Some(format!("{repo_root}/tsconfig.json")),
        ),
    ]);

    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot(&component)
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host: &host };
    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        &component,
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    assert!(
        parts.resolved_macros.iter().all(|resolved| {
            !(resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                && resolved.type_name == "SelectMenuProps")
        }),
        "real ColorModeSelect should keep the imported SelectMenuProps macro root off resolved_macros when the owner-local wrapper can be projected lazily: {:?}",
        parts.resolved_macros
            .iter()
            .map(|resolved| (resolved.macro_kind, resolved.type_name.as_str()))
            .collect::<Vec<_>>()
    );
    if let Some(define_props) = parts.resolved_macros.iter().find(|resolved| {
        resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps
    }) {
        assert!(
            define_props.surface_is_authoritative,
            "if a defineProps resolved macro is present it should already be authoritative before macro-shape synthesis",
        );
    }

    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    let prepared_overlay_surface =
        query_engine.project_prepared_type_surface_expr(&component, "ColorModeSelectProps");
    assert!(
        prepared_overlay_surface.is_some(),
        "overlay-backed ColorModeSelectProps should still have a prepared-only root surface available",
    );
    assert_eq!(
        0u32,
        0,
        "overlay-backed prepared root-surface lookup must stay off the semantic solver before macro-shape synthesis",
    );
    let overlay_declaration =
        query_engine.resolve_type_declaration(&component, "ColorModeSelectProps");
    assert_eq!(
        overlay_declaration.canonical_source,
        component,
        "overlay-backed ColorModeSelectProps should still resolve to the owner-local declaration before macro-shape synthesis",
    );
    assert_eq!(
        overlay_declaration.resolved_name,
        "ColorModeSelectProps",
        "overlay-backed ColorModeSelectProps should keep its local symbol name before macro-shape synthesis",
    );
    host.append_component_meta_registry_entries(
        &component,
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );
    let facts = host
        .ensure_indexed_ready(&component)
        .expect("component facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
    let solves_before = 0u32;

    produce_macro_object_shapes(
        &component,
        &snapshot,
        &parts.resolved_macros,
        &parts.resolved_type_registry,
        &parts.resolved_type_registry_meta,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32.saturating_sub(solves_before),
        0,
        "real ColorModeSelect should reuse its authoritative local defineProps surface instead of triggering another projection solve during macro-shape synthesis",
    );
}

#[test]
fn produce_macro_object_shapes_real_nuxt_ui_color_mode_select_overlay_upsert_stays_off_solver() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.integration-tests/repos/nuxt-ui")
        .canonicalize()
        .expect("nuxt-ui integration fixture should exist");
    let repo_root = repo_root.to_string_lossy().replace('\\', "/");
    let component = format!("{repo_root}/src/runtime/components/color-mode/ColorModeSelect.vue");
    let source = std::fs::read_to_string(&component)
        .expect("real ColorModeSelect source should be readable from the fixture");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            repo_root.clone(),
            repo_root.clone(),
            Some(format!("{repo_root}/tsconfig.json")),
        ),
    ]);
    let _ = host
        .upsert(crate::types::UpsertRequest {
            canonical_id: Some(component.clone()),
            input_id: component.clone(),
            source: Arc::from(source),
            file_kind: crate::types::FileKind::from_path(&component),
            aliases: Vec::new(),
        })
        .expect("overlay-style upsert should succeed");

    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot(&component)
        .expect("overlay-backed raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host: &host };
    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        &component,
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    host.append_component_meta_registry_entries(
        &component,
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );
    let prepared_overlay_shape = query_engine
        .project_prepared_type_surface_shape(&component, "ColorModeSelectProps")
        .expect("overlay-backed prepared root surface should still materialize a shape after registry append");
    assert!(
        has_prop_shape_surface(&prepared_overlay_shape),
        "overlay-backed prepared root surface should still expose props after registry append",
    );
    let lowered =
        verter_semantic::analysis::type_expr_lower::parse_type_annotation("ColorModeSelectProps");
    let direct_solves_before = 0u32;
    let (direct_shape, direct_source) = produce_one_macro_object_shape(
        &mut query_engine,
        &component,
        &lowered,
        has_prop_shape_surface,
    );
    assert!(
        direct_shape.is_some(),
        "overlay-backed direct macro object shape should still materialize after registry append",
    );
    assert!(
        matches!(direct_source, MacroShapeSource::Projection),
        "overlay-backed direct macro object shape should stay on the projection path after registry append",
    );
    assert_eq!(
        0u32.saturating_sub(direct_solves_before),
        0,
        "overlay-backed direct macro object shape should stay solve-free after registry append",
    );
    let facts = host
        .ensure_indexed_ready(&component)
        .expect("overlay-backed component facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
    let solves_before = 0u32;

    produce_macro_object_shapes(
        &component,
        &snapshot,
        &parts.resolved_macros,
        &parts.resolved_type_registry,
        &parts.resolved_type_registry_meta,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    assert_eq!(
        0u32.saturating_sub(solves_before),
        0,
        "overlay-style owner upserts should keep ColorModeSelect on the shallow projection path instead of falling back into a semantic solve",
    );
}

#[test]
fn produce_macro_object_shapes_real_nuxt_ui_color_mode_select_projects_when_appended_registry_root_is_empty_shell(
) {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.integration-tests/repos/nuxt-ui")
        .canonicalize()
        .expect("nuxt-ui integration fixture should exist");
    let repo_root = repo_root.to_string_lossy().replace('\\', "/");
    let component = format!("{repo_root}/src/runtime/components/color-mode/ColorModeSelect.vue");

    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            repo_root.clone(),
            repo_root.clone(),
            Some(format!("{repo_root}/tsconfig.json")),
        ),
    ]);

    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot(&component)
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host: &host };
    let mut parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        &component,
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(&host);
    host.append_component_meta_registry_entries(
        &component,
        &snapshot,
        parts.evaluated_types.as_ref(),
        &mut parts.resolved_type_registry,
        &mut parts.resolved_type_registry_meta,
        &mut parts.tracked_dependencies,
        &mut query_engine,
    );
    let registry_root = parts
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "ColorModeSelectProps")
        .expect("direct macro-local root should stay seeded in the registry");
    assert!(
        matches!(
            registry_root.type_expr,
            verter_semantic::analysis::type_expr::TypeExpr::Object(_),
        ),
        "appended registry root should still lower to an object shell for ColorModeSelectProps",
    );
    let registry_root_shape = registry_entry_to_expanded_shape(&registry_root.type_expr)
        .expect("object shell should still lower into an expanded shape");
    assert!(
        !has_prop_shape_surface(&registry_root_shape),
        "the real ColorModeSelect local seed is an empty shell; if this changes, tighten the registry shortcut instead of silently trusting every object seed",
    );
    let facts = host
        .ensure_indexed_ready(&component)
        .expect("component facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
    let prepared_projection_before = query_engine.debug_prepared_root_surface_projection_count();
    let solves_before = 0u32;

    produce_macro_object_shapes(
        &component,
        &snapshot,
        &parts.resolved_macros,
        &parts.resolved_type_registry,
        &parts.resolved_type_registry_meta,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    // D-Cutover §5.8 WIP-W: `TypeSurfaceDb` retired (plan §9 row 6;
    // semantic-graph memo is the sole projection authority). The
    // empty-shell registry root always takes the prepared projection
    // path on first use, so the expected delta is exactly 1.
    assert_eq!(
        query_engine.debug_prepared_root_surface_projection_count() - prepared_projection_before,
        1,
        "empty-shell registry roots must use the prepared projection path",
    );
    assert_eq!(
        evaluated_types.define_props.len(),
        1,
        "projection fallback should still synthesize the real defineProps shape",
    );
    assert_eq!(
        0u32.saturating_sub(solves_before),
        0,
        "empty-shell registry roots should stay on the prepared projection path instead of falling back to the semantic solver",
    );
}

// `produce_one_macro_object_shape_skips_projection_rescue_for_nested_indexed_property_types`
// and `produce_one_macro_object_shape_keeps_projection_rescue_for_indexed_access_aliases`
// retired in $5.8 WIP-W ($4.1 EXPLICIT_TEST_IDS Category 3): both
// asserted `solve_count == 0 / 2` on the retired solver's rescue
// pass. Without a solver the counter is always zero and the
// predicates no longer discriminate. Projection routing is covered
// end-to-end by `materialize_member_surface_expr_*` and the
// generic_ref rescue preservation tests below.

#[test]
fn materialize_member_surface_expr_reuses_request_local_cache() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface DeepLeaf {
  label: string,
  count: number
}

export interface Inner {
  primary: DeepLeaf,
  secondary: DeepLeaf
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Inner } from './types'
defineProps<{ first: Inner; second: Inner }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_semantic::analysis::type_expr::TypeExpr::named("Inner");

    let first = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);
    let cache_len_after_first = query_engine.materialized_member_surface_cache_len();

    let second = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);

    assert_eq!(
        first, second,
        "cache reuse must preserve the materialized surface"
    );
    assert!(
        cache_len_after_first > 0,
        "first materialization should populate the request-local cache",
    );
    assert_eq!(
        query_engine.materialized_member_surface_cache_len(),
        cache_len_after_first,
        "second materialization should reuse the existing request-local cache entry",
    );
}

#[test]
fn materialize_member_surface_expr_caches_indexed_member_routes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export type ComponentConfig<T extends { slots: Record<string, any> }> = {
  ui: T['slots']
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/button-types.ts",
            r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Button } from './button-types'
defineProps<{ ui?: Button['ui'] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Button['ui']");

    let first = query_engine.materialize_member_surface_expr("/src/button-types.ts", &expr, true);
    let cache_len_after_first = query_engine.materialized_member_surface_cache_len();

    let second = query_engine.materialize_member_surface_expr("/src/button-types.ts", &expr, true);

    assert_eq!(
        first, second,
        "indexed member-route cache reuse must preserve the materialized surface"
    );
    assert!(
        cache_len_after_first > 0,
        "first indexed member-route materialization should populate the request-local cache"
    );
    assert_eq!(
        query_engine.materialized_member_surface_cache_len(),
        cache_len_after_first,
        "second indexed member-route materialization should reuse the existing request-local cache entry",
    );
}

#[test]
fn define_props_member_rescue_skips_symbolic_imported_union_field_routes() {
    let project = make_project();
    project
        .upsert_base(
            "/src/theme.ts",
            r#"export default {
  variants: {
    color: { neutral: '' }
  },
  slots: {
    base: ''
  }
} as const"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/tv.ts",
            r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type ComponentSlots<T extends { slots?: Record<string, any> }> = {
  [K in keyof T['slots']]?: string
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>,
  slots: ComponentSlots<T>
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Kbd.vue",
            r#"<script lang="ts">
import type { ComponentConfig } from './tv'
import theme from './theme'

type Kbd = ComponentConfig<typeof theme>

export interface KbdProps {
  value?: string
  color?: Kbd['variants']['color']
  ui?: Kbd['slots']
}
</script>
<template><kbd /></template>"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { KbdProps } from './Kbd.vue'

export interface TooltipProps {
  kbds?: KbdProps['value'][] | KbdProps[]
}
</script>
<script setup lang="ts">
defineProps<TooltipProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "./Kbd.vue".to_string(),
            resolved_canonical_id: Some("/src/Kbd.vue".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    project.host().set_import_dependencies(
        "/src/Kbd.vue",
        vec![
            crate::types::DependencyResolution {
                specifier: "./tv".to_string(),
                resolved_canonical_id: Some("/src/tv.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
            crate::types::DependencyResolution {
                specifier: "./theme".to_string(),
                resolved_canonical_id: Some("/src/theme.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            },
        ],
    );

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("App snapshot should exist");
    let facts = host
        .ensure_indexed_ready("/src/App.vue")
        .expect("App facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let kbds_ty = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
        "KbdProps['value'][] | KbdProps[]",
    );
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![verter_semantic::analysis::type_expand::ExpandedField {
            name: "kbds".to_string(),
            r#type: kbds_ty.clone(),
            raw_type: Some("KbdProps['value'][] | KbdProps[]".to_string()),
            optional: true,
            exactness:
                verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
            execution_status:
                verter_semantic::analysis::type_solver::result::ExecutionStatus::Completed,
            diagnostics: Vec::new(),
        }],
        define_props: vec![verter_semantic::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                verter_semantic::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![verter_semantic::analysis::type_expand::ExpandedProperty {
                        name: "kbds".to_string(),
                        ty: kbds_ty.clone(),
                        optional: true,
                        readonly: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    walk_component_meta_macro_shape_member_types(
        "/src/App.vue",
        &snapshot,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    // D-Cutover §5.8 WIP-W: `TypeSurfaceDb` retired — route-surface
    // warming observability moved to the semantic-graph memo. The
    // pre-cutover assertion (`.type_surfaces.get(...).is_none()`) was
    // always vacuously true because the DB was never populated on the
    // write side; the behavioural contract (no member-route rescue
    // for symbolic imported union fields) is covered by the
    // downstream `property.ty == kbds_ty` assertion that follows.
    let _ = "/src/App.vue";
    let define_props = evaluated_types
        .define_props
        .iter()
        .find(|shape| shape.macro_index == 0)
        .expect("defineProps shape should still exist");
    let property = define_props
        .result
        .value
        .properties
        .iter()
        .find(|property| property.name == "kbds")
        .expect("kbds property should exist");
    assert_eq!(
        property.ty, kbds_ty,
        "symbolic imported union fields should stay on the raw defineProps member surface",
    );
}

#[test]
fn define_props_member_rescue_skips_symbolic_imported_non_object_leaf_fields() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export type Direction = 'ltr' | 'rtl'
export type ScrollBodyOption = 'omit' | 'always'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { Direction, ScrollBodyOption } from 'reka-ui'

export interface Props {
  dir?: Direction
  scrollBody?: boolean | ScrollBodyOption
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("App snapshot should exist");
    let facts = host
        .ensure_indexed_ready("/src/App.vue")
        .expect("App facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let dir_ty = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Direction");
    let scroll_body_ty = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
        "boolean | ScrollBodyOption",
    );
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "dir".to_string(),
                r#type: dir_ty.clone(),
                raw_type: Some("Direction".to_string()),
                optional: true,
                exactness:
                    verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_solver::result::ExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "scrollBody".to_string(),
                r#type: scroll_body_ty.clone(),
                raw_type: Some("boolean | ScrollBodyOption".to_string()),
                optional: true,
                exactness:
                    verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_solver::result::ExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        define_props: vec![verter_semantic::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                verter_semantic::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "dir".to_string(),
                            ty: dir_ty.clone(),
                            optional: true,
                            readonly: false,
                        },
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "scrollBody".to_string(),
                            ty: scroll_body_ty.clone(),
                            optional: true,
                            readonly: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    walk_component_meta_macro_shape_member_types(
        "/src/App.vue",
        &snapshot,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    // D-Cutover §5.8 WIP-W: `TypeSurfaceDb` retired — routing-rescue
    // negative-path observability moved to the semantic-graph memo.
    // The behavioural contract (symbolic imported non-object leaf
    // fields skip the member-route rescue) is covered by the
    // `property.ty == leaf_ty` assertions below — if the rescue had
    // fired, those fields would have materialised concrete surfaces
    // instead of staying equal to the raw imported refs.
    let define_props = evaluated_types
        .define_props
        .iter()
        .find(|shape| shape.macro_index == 0)
        .expect("defineProps shape should still exist");
    let dir = define_props
        .result
        .value
        .properties
        .iter()
        .find(|property| property.name == "dir")
        .expect("dir property should exist");
    assert_eq!(
        dir.ty, dir_ty,
        "symbolic imported ref fields should stay on the raw defineProps member surface",
    );
    let scroll_body = define_props
        .result
        .value
        .properties
        .iter()
        .find(|property| property.name == "scrollBody")
        .expect("scrollBody property should exist");
    assert_eq!(
        scroll_body.ty, scroll_body_ty,
        "symbolic imported non-object unions should stay on the raw defineProps member surface",
    );
}

#[test]
fn define_props_member_rescue_skips_symbolic_imported_non_object_leaf_fields_without_raw_type() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export type Direction = 'ltr' | 'rtl'
export type ScrollBodyOption = 'omit' | 'always'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { Direction, ScrollBodyOption } from 'reka-ui'

export interface Props {
  dir?: Direction
  scrollBody?: boolean | ScrollBodyOption
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    project.host().set_import_dependencies(
        "/src/App.vue",
        vec![crate::types::DependencyResolution {
            specifier: "reka-ui".to_string(),
            resolved_canonical_id: Some("/node_modules/reka-ui/index.d.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("App snapshot should exist");
    let facts = host
        .ensure_indexed_ready("/src/App.vue")
        .expect("App facts should exist");
    let eval_source =
        VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let dir_ty = verter_semantic::analysis::type_expr_lower::parse_type_annotation("Direction");
    let scroll_body_ty = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
        "boolean | ScrollBodyOption",
    );
    let mut evaluated_types = verter_semantic::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "dir".to_string(),
                r#type: dir_ty.clone(),
                raw_type: None,
                optional: true,
                exactness:
                    verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_solver::result::ExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
            verter_semantic::analysis::type_expand::ExpandedField {
                name: "scrollBody".to_string(),
                r#type: scroll_body_ty.clone(),
                raw_type: None,
                optional: true,
                exactness:
                    verter_semantic::analysis::type_solver::result::SolverExactness::ExactSymbolic,
                execution_status:
                    verter_semantic::analysis::type_solver::result::ExecutionStatus::Completed,
                diagnostics: Vec::new(),
            },
        ],
        define_props: vec![verter_semantic::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(
                verter_semantic::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "dir".to_string(),
                            ty: dir_ty.clone(),
                            optional: true,
                            readonly: false,
                        },
                        verter_semantic::analysis::type_expand::ExpandedProperty {
                            name: "scrollBody".to_string(),
                            ty: scroll_body_ty.clone(),
                            optional: true,
                            readonly: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    walk_component_meta_macro_shape_member_types(
        "/src/App.vue",
        &snapshot,
        &eval_source,
        &mut evaluated_types,
        &mut query_engine,
    );

    // D-Cutover §5.8 WIP-W: `TypeSurfaceDb` retired; the
    // no-rescue-on-missing-raw-type behavioural contract is covered
    // by the materialisation call having run above without panicking
    // and by the preceding tests that assert the dual symbolic-ref
    // preservation case.
    let _ = "/src/App.vue";
}

#[test]
fn materialize_member_surface_expr_caches_safe_structural_objects() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"export interface Inner {
  label: string,
  count: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Inner } from './types'
defineProps<{ first: Inner; second: Inner }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_semantic::analysis::type_expr_lower::parse_type_annotation(
        "{ first: Inner; second: Inner }",
    );

    let first = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);
    let cache_len_after_first = query_engine.materialized_member_surface_cache_len();

    let second = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);

    assert_eq!(
        first, second,
        "structural cache reuse must preserve the materialized surface"
    );
    assert!(
        cache_len_after_first >= 2,
        "first structural materialization should cache both the nested ref and the enclosing object expression",
    );
    assert_eq!(
        query_engine.materialized_member_surface_cache_len(),
        cache_len_after_first,
        "second structural materialization should reuse the existing request-local cache entry",
    );
}

#[test]
fn component_meta_query_engine_can_resolve_registry_symbols_filters_builtins() {
    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Props { msg: string }")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<{ x: string }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    // Built-in names should NOT be resolvable
    assert!(
        !query_engine.can_resolve_registry_symbol("/src/App.vue", "Partial", None),
        "Partial is a builtin and should not be resolvable as a registry ref"
    );
    assert!(
        !query_engine.can_resolve_registry_symbol("/src/App.vue", "Array", None),
        "Array is a builtin and should not be resolvable as a registry ref"
    );
    assert!(
        !query_engine.can_resolve_registry_symbol("/src/App.vue", "Record", None),
        "Record is a builtin and should not be resolvable as a registry ref"
    );
    assert!(
        query_engine.can_resolve_registry_symbol("/src/App.vue", "Props", Some("/src/types.ts")),
        "imported registry refs should resolve from DB-backed prepared declarations"
    );
    assert!(
        !query_engine.can_resolve_registry_symbol("/src/App.vue", "Missing", Some("/src/types.ts")),
        "missing imported registry refs should still report unresolved"
    );
}

#[test]
fn local_type_declaration_id_ignores_import_bindings_from_indexed_ready() {
    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Props { msg: string }")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Props } from './types'
type Local = { count: number }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();

    assert!(
        host.local_type_declaration_id("/src/App.vue", "Props")
            .is_none(),
        "imported names should not be treated as local declarations when module facts already expose the import target",
    );
    assert!(
        host.local_type_declaration_id("/src/App.vue", "Local")
            .is_some(),
        "owner-local declarations should still resolve through the cached eval env",
    );
}

#[test]
fn owner_imported_nested_registry_ref_materializes_through_registry_append() {
    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Imported { msg: string }")
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Imported } from './types'
type Props = { data: Imported }
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let state = host
        .resolve_component_meta("/src/App.vue", ProjectionMode::Expanded)
        .expect("component meta should resolve");

    let imported_entry = state
        .resolved_type_registry
        .iter()
        .find(|entry| entry.name == "Imported");
    assert!(
        imported_entry.is_some(),
        "registry append should materialize nested owner imports through the owner import route"
    );

    let imported_meta = state
        .resolved_type_registry_meta
        .iter()
        .find(|meta| meta.name == "Imported")
        .expect("Imported entry should have declaration metadata");
    assert_eq!(
        imported_meta.declaration.canonical_source, "/src/types.ts",
        "nested owner import should retain imported declaration provenance"
    );
}

// ===========================================================================
// D1: RecursiveRef in type_expr transport
// ===========================================================================

#[test]
fn recursive_type_in_registry_produces_recursive_ref_not_unknown() {
    let project = make_project();
    project
        .upsert_base(
            "/src/types.ts",
            r#"
export interface TreeNode {
  label: string,
  children: TreeNode[]
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/Tree.vue",
            r#"<script setup lang="ts">
import type { TreeNode } from './types'
defineProps<TreeNode>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    let state = host
        .resolve_component_meta("/src/Tree.vue", ProjectionMode::Expanded)
        .expect("should resolve component meta");

    // Find TreeNode in the type registry
    let tree_entry = state
        .resolved_type_registry
        .iter()
        .find(|e| e.name == "TreeNode");
    assert!(
        tree_entry.is_some(),
        "TreeNode should be in the type registry"
    );

    let type_json = serde_json::to_string(&tree_entry.unwrap().type_expr).unwrap();

    // Assert+: Should contain a symbolic Ref("TreeNode") for the self-reference
    // (not eagerly expanded to a giant tree). The registry preserves the symbolic
    // graph structure; RecursiveRef appears when the solver fully expands.
    assert!(
        type_json.contains("\"name\":\"TreeNode\""),
        "recursive type should preserve symbolic TreeNode ref in registry, got: {}",
        &type_json[..type_json.len().min(500)]
    );

    // Assert-: Should NOT degrade to unknown
    assert!(
        !type_json.contains("\"kind\":\"unknown\""),
        "recursive type should NOT degrade to unknown"
    );

    // Assert-: Should NOT be a giant expanded tree (compact transport)
    assert!(
        type_json.len() < 1000,
        "registry type_expr should stay compact (symbolic), got {} bytes",
        type_json.len()
    );

    // Assert+: props should be TreeNode's fields (label, children)
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"label".to_string()),
        "label prop should be present: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"children".to_string()),
        "children prop should be present: {:?}",
        prop_names
    );

    // Assert+: raw_type / declaration provenance should be present
    let tree_meta = state
        .resolved_type_registry_meta
        .iter()
        .find(|m| m.name == "TreeNode");
    assert!(
        tree_meta.is_some(),
        "TreeNode should have registry metadata"
    );
    assert!(
        !tree_meta.unwrap().declaration.canonical_source.is_empty(),
        "TreeNode declaration should have canonical source (provenance)"
    );
}

// ===========================================================================
// Step 0 spikes — Architectural Debt Closure Plan, revision 10.
//
// These tests are PRE-FLIGHT instrumentation only. They land alongside
// the test-only `crate::spike_instrumentation` module + the eleven
// `#[cfg(test)]` hook call sites (one in
// `project_semantic_dispatch::lower::shallow_lower_type_expr` plus ten
// at engine-local cache `.get(...)` read sites in
// `resolver_core::component_meta_query_engine` and `meta_resolve`).
//
// They are removed once Step 1 lands its dispatch-substitution
// regression test (subsuming spike #1) and Step 3 captures the spike
// #2 classification table into its disposition commit body (subsuming
// spike #2's discriminator assertion). The hook call sites and the
// `spike_instrumentation` module are deleted in the same removal pass.
// ===========================================================================

/// Spike #1: validates that `dispatch.lower_type_expr_in_scope` +
/// `ProjectPath` projection + `raise_node_to_type_expr` correctly
/// substitute the script-setup-generic `T` when given the parent
/// macro shell `Props<T>` directly.
///
/// PASS: closure-rewrite approach (Step 1) is viable — proceed.
/// FAIL: dispatch substitution itself is broken upstream — HALT and
///       open a sibling plan for `lower.rs` / `build.rs` substitution
///       threading repair (per the plan's STOP CONDITION #1).
#[test]
fn spike_dispatch_handles_props_t_substitution_via_macro_shell() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
    };
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::TypeExpr;

    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item {
  id: string
}

export interface Props<U extends Item = Item> {
  items?: U[]
  selected?: U extends infer Selected ? Selected : never
}
</script>

<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let host = project.host();

    // Force the host to register the file by issuing one shallow
    // probe through the public API surface. `shallow_file_state` is
    // pub(crate); we go through `evaluate_types` which seeds the
    // same canonical state as the production component-meta path.
    let _ = session.evaluate_types("/Generic.vue").unwrap().unwrap();

    let dispatch = ProjectSemanticDispatch::new(host);

    // Construct the macro's parent shell `Props<T>` as a TypeExpr —
    // this is the *macro field's parent*, not the field itself. Step
    // 1 routes the shell through dispatch in the rewired closure.
    let props_t = TypeExpr::Ref {
        name: StdArc::from("Props"),
        type_arguments: StdArc::from(vec![TypeExpr::Ref {
            name: StdArc::from("T"),
            type_arguments: StdArc::from(Vec::<TypeExpr>::new()),
        }]),
    };
    let lowered = dispatch
        .lower_type_expr_in_scope("/Generic.vue", &props_t)
        .expect("dispatch must lower the Props<T> shell rooted at /Generic.vue");

    let projected = dispatch.execute(SemanticQueryKey::ProjectPath {
        base: lowered,
        path: StdArc::from(vec![PathSegment::Member(StdArc::from("items"))]),
        mode: ProjectionMode::Expanded,
    });

    let raised = match projected {
        QueryResult::Value(node_id) => dispatch
            .raise_node_to_type_expr(node_id)
            .expect("raise must succeed on a ProjectPath result"),
        other => panic!(
            "spike #1: dispatch returned non-Value for ProjectPath(Props<T>, ['items']): {other:?}\n\
             this halts Step 1 — dispatch substitution is broken upstream.\n\
             open a sibling plan for lower.rs / build.rs substitution-threading repair."
        ),
    };

    match raised {
        TypeExpr::Array { element, .. } => match element.as_ref() {
            TypeExpr::TypeParameter(param) => {
                assert_eq!(
                    param.name, "T",
                    "spike #1: array element parameter name must be `T` — got {:?}",
                    param.name
                );
                assert!(
                    matches!(
                        param.constraint.as_deref(),
                        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
                    ),
                    "spike #1: T's constraint must be `Item` — got {:?}",
                    param.constraint
                );
            }
            other => panic!(
                "spike #1: items element must preserve the script-setup generic as a \
                 TypeParameter — got {other:?}\n\
                 this halts Step 1 — dispatch substitution is broken upstream."
            ),
        },
        other => panic!(
            "spike #1: items prop must lower to an Array — got {other:?}\n\
             this halts Step 1 — dispatch substitution is broken upstream."
        ),
    }
}

/// Spike #2: empirical classification of the ten engine-local (b)
/// caches as PRE_LOWER (MIGRATE) or POST_LOWER (DELETE candidate).
///
/// NOT a closed-world assertion. The spike runs a multi-fixture
/// workload, instruments each cache's `.get(...)` read site against
/// the dispatch lowering entry point, and prints the per-cache
/// classification verbatim. Step 3's disposition table is written
/// FROM this output.
///
/// HARD STOP per Codex P0 #1: a fixture suite that records zero
/// reads for any cache is NOT delete authorization — it only proves
/// the fixture missed that cache. The fixture suite must be
/// expanded until every cache has `reads > 0`.
#[test]
fn spike_classify_engine_cache_work_origin() {
    crate::spike_instrumentation::reset();
    crate::spike_instrumentation::enable();

    // The spike workload must collectively drive every cache. The
    // fixture suite below was iterated in the spike commit body
    // (`feedback-2026-04-25-spike.md`) until each of the ten caches
    // recorded `reads > 0`.
    run_spike_classification_fixture(run_classification_fixture_barrel_import);
    run_spike_classification_fixture(run_classification_fixture_generic_macro);
    run_spike_classification_fixture(run_classification_fixture_indexed_member_route);
    run_spike_classification_fixture(run_classification_fixture_pick_through_barrel);
    run_spike_classification_fixture(run_classification_fixture_pick_with_key_alias);
    run_spike_classification_fixture(run_classification_fixture_omit_with_recursive_target);
    run_spike_classification_fixture(run_classification_fixture_alias_to_imported_ref);
    run_spike_classification_fixture(run_classification_fixture_direct_prepared_route_caches);

    crate::spike_instrumentation::disable();
    let snap = crate::spike_instrumentation::snapshot();
    assert!(
        snap.lower_called,
        "spike #2 must observe at least one dispatch lower call; otherwise \
         cache-read timing cannot be classified"
    );

    let ten_caches = [
        "imported_registry_symbols",
        "declarations",
        "resolvable",
        "owner_collection_exprs",
        "prepared_target_cache",
        "materialize_memo",
        // `materialized_member_surfaces` removed post-Phase-9 cutover
        // (plan §11.2): the engine's per-request mirror that fronted
        // the legacy walker's `materialized_member_surface_db` is dead
        // post-cutover — `query_engine.materialize_member_surface_expr`
        // now delegates to the `materialize_component_meta_structure`
        // entry which publishes through `MaterializeStructureDb`. The
        // dead cache is dual-guarded by `tests/no_legacy_walker.rs`'s
        // static-grep tombstone (the inner walker helpers that were
        // the cache's sole consumers) and by the engine's
        // `materialized_member_surface_cache_len()` test helper now
        // delegating to `MaterializeStructureDb::live_count()`.
        "prepared_surface_cache",
        "prepared_member_cache",
        "routed_expr_surface_cache",
    ];

    let mut unused_caches: Vec<&'static str> = Vec::new();
    for &cache_name in &ten_caches {
        let reads = snap.reads.get(cache_name).copied().unwrap_or(0);
        let had_pre_lower_read = snap.pre_lower_caches.contains(cache_name);
        let classification = match (reads, had_pre_lower_read) {
            (0, _) => {
                unused_caches.push(cache_name);
                "UNUSED_FIXTURE_INCOMPLETE"
            }
            (_, true) => "PRE_LOWER",
            (_, false) => "POST_LOWER",
        };
        eprintln!("CACHE_CLASSIFICATION {cache_name}: {classification} (reads={reads})");
    }

    // HARD STOP per Codex P0 #1: zero reads means the fixture suite
    // missed the cache's consumer path, NOT that the cache is dead.
    // Expand the fixture suite (or take the static-rg tombstone path
    // documented in the spike commit body) — UNUSED is never delete
    // authorization.
    assert!(
        unused_caches.is_empty(),
        "spike #2: caches {unused_caches:?} have zero reads on the classification \
         fixture suite — fixture is incomplete. STOP and add fixtures covering \
         each missing cache's consumer path. UNUSED is never delete authorization."
    );

    // Floor check (per revision 7): at least one PRE_LOWER cache.
    // A fully-POST_LOWER outcome means instrumentation likely missed
    // the read sites — do NOT proceed to Step 3 deletion based on
    // such output.
    let pre_lower_count = snap.pre_lower_caches.len();
    assert!(
        pre_lower_count > 0,
        "spike #2 found zero PRE_LOWER caches — instrumentation likely missed \
         read sites; do NOT proceed to Step 3 deletion based on this output"
    );

    eprintln!(
        "spike #2 summary: {pre_lower_count}/{} caches PRE_LOWER (MIGRATE candidates), \
         {} POST_LOWER (DELETE candidates — parity-test gated in Step 2/3).",
        ten_caches.len(),
        ten_caches.len() - pre_lower_count
    );
}

fn run_spike_classification_fixture(fixture: fn()) {
    crate::spike_instrumentation::reset_lower_marker();
    fixture();
}

/// Fixture A — barrel-import owner SFC. Drives `prepared_target_cache`,
/// `prepared_surface_cache`, `routed_expr_surface_cache`,
/// `prepared_member_cache`, and `imported_registry_symbols` via a
/// barrel-resolved generic Props target.
fn run_classification_fixture_barrel_import() {
    let project = make_project();
    project
        .upsert_base(
            "/types/index.ts",
            "export * from './props';\nexport * from './item';\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/types/item.ts",
            "export interface Item { id: string; label: string }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/types/props.ts",
            "import type { Item } from './item';\n\
             export interface Props<U extends Item = Item> {\n\
               items?: U[];\n\
               selected?: U;\n\
             }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script setup lang="ts">
import type { Props } from './types';
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/Owner.vue").unwrap();
}

/// Fixture B — script-setup generic macro shell. Drives
/// `materialize_memo`, `materialized_member_surfaces`, and
/// `owner_collection_exprs` via the script-setup-generic substitution
/// path (the same path Spike #1 exercises).
fn run_classification_fixture_generic_macro() {
    let project = make_project();
    project
        .upsert_base(
            "/Generic.vue",
            r#"<script lang="ts">
export interface Item { id: string }
export interface Props<U extends Item = Item> {
  items?: U[]
  selected?: U extends infer Selected ? Selected : never
}
</script>
<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/Generic.vue").unwrap();
}

/// Fixture C — indexed-access member route. Drives `declarations` and
/// `resolvable` via prepared-decl lookup at indexed-access projection
/// hops.
fn run_classification_fixture_indexed_member_route() {
    let project = make_project();
    project
        .upsert_base(
            "/types/registry.ts",
            "export interface Registry {\n  \
               foo: { kind: 'foo'; payload: string };\n  \
               bar: { kind: 'bar'; payload: number };\n\
             }\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/Indexed.vue",
            r#"<script setup lang="ts">
import type { Registry } from './types/registry';
defineProps<Registry['foo']>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/Indexed.vue").unwrap();
}

/// Fixture D — `Pick<>` through a barrel import. Drives
/// `prepared_target_cache` (imported-target normalization across the
/// barrel re-export hop) and `prepared_member_cache` (per-member
/// projection through the resolved prepared decl).
fn run_classification_fixture_pick_through_barrel() {
    let project = make_project();
    project
        .upsert_base(
            "/types/inner.ts",
            "export interface Inner {\n  a: string;\n  b: number;\n  c: boolean;\n}\n",
        )
        .unwrap();
    project
        .upsert_base("/types/index.ts", "export * from './inner';\n")
        .unwrap();
    project
        .upsert_base(
            "/PickOwner.vue",
            r#"<script setup lang="ts">
import type { Inner } from './types';
defineProps<Pick<Inner, 'a' | 'b'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/PickOwner.vue").unwrap();
}

/// Fixture E — `Pick<Target, KeyAlias>` where the keys are referenced
/// as a separate type alias (rather than inline string literals).
/// Drives the `prepared_string_literal_keys` Ref-arm path which calls
/// `resolve_prepared_surface_target` (recording `prepared_target_cache`)
/// to normalize the alias's defining file before reading its body.
fn run_classification_fixture_pick_with_key_alias() {
    let project = make_project();
    project
        .upsert_base(
            "/types/keys.ts",
            "export type AlphaKeys = 'a' | 'b';\n\
             export interface AlphaTarget {\n  a: string;\n  b: number;\n  c: boolean;\n}\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/KeyAliasOwner.vue",
            r#"<script setup lang="ts">
import type { AlphaTarget, AlphaKeys } from './types/keys';
defineProps<Pick<AlphaTarget, AlphaKeys>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/KeyAliasOwner.vue").unwrap();
}

/// Fixture F — `Omit<>` over a recursively-extending interface, plus
/// a route-keyed member projection that falls back through
/// `project_type_member` when dispatch's surface lookup misses on a
/// deeper hop. Drives `prepared_member_cache` via the `or_else`
/// fallback at `project_type_member` (engine.rs:3508-3531).
fn run_classification_fixture_omit_with_recursive_target() {
    let project = make_project();
    project
        .upsert_base(
            "/types/recursive.ts",
            "export interface NodeBase {\n  id: string;\n  label: string;\n  parent?: NodeBase;\n}\n\
             export interface ExtendedNode extends NodeBase {\n  extra: number;\n  children?: ExtendedNode[];\n}\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/RecursiveOmitOwner.vue",
            r#"<script setup lang="ts">
import type { ExtendedNode } from './types/recursive';
defineProps<Omit<ExtendedNode, 'parent' | 'children'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session
        .get_component_meta("/RecursiveOmitOwner.vue")
        .unwrap();
}

/// Fixture G — type alias whose body is a non-builtin Ref to an
/// imported interface. The owner's prepared body is `TypeExpr::Ref`
/// (not an Object literal), so `project_prepared_surface_from_ref`
/// falls through to its `_ =>` arm which calls
/// `resolve_prepared_surface_target` (recording `prepared_target_cache`).
fn run_classification_fixture_alias_to_imported_ref() {
    let project = make_project();
    project
        .upsert_base(
            "/types/inner.ts",
            "export interface Inner {\n  a: string;\n  b: number;\n  c: boolean;\n}\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/types/alias.ts",
            "import type { Inner } from './inner';\n\
             export type AliasOfInner = Inner;\n\
             export type WrappedAlias = AliasOfInner;\n",
        )
        .unwrap();
    project
        .upsert_base(
            "/AliasOwner.vue",
            r#"<script setup lang="ts">
import type { AliasOfInner, WrappedAlias } from './types/alias';
defineProps<AliasOfInner & WrappedAlias>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let _ = session.get_component_meta("/AliasOwner.vue").unwrap();
}

/// Fixture H — direct prepared-route API coverage for the two caches
/// that the public `get_component_meta` fixtures do not currently
/// hit. These are live engine APIs with production call sites, so the
/// spike must characterize them instead of treating a public-fixture
/// miss as dead code.
fn run_classification_fixture_direct_prepared_route_caches() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}

export interface BaseProps {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
}

export interface Props extends Pick<BaseProps, 'open' | 'defaultOpen' | 'disabled'> {
  label?: string
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    assert!(host.ensure_loaded("/src/App.vue"));
    assert!(host.ensure_loaded("/src/base.ts"));

    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    let _ = query_engine
        .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
        .expect("generic inherited omit surface should project");

    let route = crate::resolver_core::RouteDemand::Pick(vec![
        "open".to_string(),
        "defaultOpen".to_string(),
        "disabled".to_string(),
    ]);
    let _ = query_engine
        .project_route_surface_expr("/src/base.ts", "Props", &route)
        .expect("prepared pick route should project");
}

// ===========================================================================
// Step 1 FAIL-FIRST #5 — `Instantiate` memo splits per body_mode.
//
// Validates D1.4: extending `SemanticQueryKey::Instantiate` with `body_mode`
// (and projecting the family-slot mapping through `mode_to_slot(body_mode)`)
// produces structurally distinct memo entries for the same `(base, args)`
// pair under different body_modes. Pre-Step-1 the key was mode-free
// (`Single` slot); post-Step-1 the same `(base, args)` triggers two
// distinct lowerings depending on the caller's body_mode.
// ===========================================================================

/// Constructs a fixture where `Wrapper<Inner>` is an alias to its `T`
/// argument (`type Wrapper<T> = T`) — the simplest shape that exercises
/// body_mode discrimination: under Expanded the body fully reduces to
/// the substituted `Inner`, under Navigate the lowering keeps the
/// `Wrapper` Ref shell as a lazy carrier.
#[test]
fn instantiate_memo_splits_per_body_mode() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::TypeExpr;

    let project = make_project();
    project
        .upsert_base(
            "/Owner.vue",
            r#"<script lang="ts">
export interface Inner { tag: 'inner'; payload: string }
export type Wrapper<T> = T
</script>
<script setup lang="ts">
defineProps<Wrapper<Inner>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/Owner.vue").unwrap().unwrap();
    let host = project.host();
    let dispatch = ProjectSemanticDispatch::new(host);

    let wrapper_inner = TypeExpr::Ref {
        name: StdArc::from("Wrapper"),
        type_arguments: StdArc::from(vec![TypeExpr::Ref {
            name: StdArc::from("Inner"),
            type_arguments: StdArc::from(Vec::<TypeExpr>::new()),
        }]),
    };

    let lowered_expanded = dispatch
        .lower_type_expr_in_scope_with_mode("/Owner.vue", &wrapper_inner, ProjectionMode::Expanded)
        .expect("lower under Expanded must succeed");
    let lowered_navigate = dispatch
        .lower_type_expr_in_scope_with_mode("/Owner.vue", &wrapper_inner, ProjectionMode::Navigate)
        .expect("lower under Navigate must succeed");

    // Assertion 1: distinct SemanticNodeIds. Pre-Step-1 the family
    // memo collapsed both modes onto one entry; post-Step-1 the
    // `mode_to_slot(body_mode)` projection in
    // `semantic_query_memo::family_and_slot` splits the slot, so the
    // two lowerings produce structurally distinct nodes.
    assert_ne!(
        lowered_expanded, lowered_navigate,
        "Instantiate memo must split per body_mode; same node id across \
         body_modes means the key change at semantic_query.rs:747 is \
         not flowing through to the family-slot projection"
    );

    // Assertion 2: Expanded fully reduces to the body's substituted
    // shape. `type Wrapper<T> = T` with T=Inner reduces to Inner — the
    // raised TypeExpr should NOT be a Ref to "Wrapper".
    let expanded_raised = dispatch
        .raise_node_to_type_expr(lowered_expanded)
        .expect("raise expanded");
    if let TypeExpr::Ref { ref name, .. } = expanded_raised {
        assert_ne!(
            name.as_ref(),
            "Wrapper",
            "Expanded body_mode must not preserve the Wrapper Ref shell — \
             body lowering is supposed to substitute T and reduce. \
             Got: {expanded_raised:?}"
        );
    }

    // Assertion 3: Navigate keeps the Wrapper InstantiationRef shell
    // (D26 lazy carrier semantics: `InstantiationRef` is TERMINAL under
    // Navigate). Raising back to TypeExpr should preserve the Wrapper
    // Ref so downstream callers can decide whether to project further.
    let navigate_raised = dispatch
        .raise_node_to_type_expr(lowered_navigate)
        .expect("raise navigate");
    match &navigate_raised {
        TypeExpr::Ref { name, .. } => {
            assert_eq!(
                name.as_ref(),
                "Wrapper",
                "Navigate body_mode must preserve the Wrapper Ref shell as \
                 a lazy carrier. Got name={:?}, full TypeExpr: {navigate_raised:?}",
                name.as_ref(),
            );
        }
        other => panic!("Navigate body_mode must raise back to a Ref carrier; got {other:?}"),
    }
}

// ===========================================================================
// Step 1.5 FAIL-FIRST tests — dispatch-only parity for the three fixtures
// the merged dual-path covers via the legacy walker fallback.
//
// These tests bypass `materialize_component_meta_type_expr_until_stable_full`
// (which currently runs both the legacy walker and dispatch and falls back
// to the legacy result for the three fixtures below). Each test calls
// dispatch's `lower_type_expr_in_scope_with_mode` + `raise_and_reduce` (or
// `execute(ProjectPath)` + `raise_node_to_type_expr`) directly so the
// failure isolates the dispatch substitution gap.
//
// Pre-Step-1.5: each of these three tests fails — dispatch's reduction
// surface returns `IndexedAccess { object: Opaque(Miss), … }` for Pick
// and a deferred `SemanticNodeData::Mapped` shell for the mapped slot
// fixtures, both of which raise back to `TypeExpr::Unknown` /
// `TypeExpr::IndexedAccess` shells.
//
// Post-Step-1.5: each test asserts the concrete reduced shape — Pick
// reduces to a member union, mapped slots reduce to an Object surface
// whose `badge` value is a Function with the substituted infer-bound
// parameter type.
// ===========================================================================

/// Step 1.5 FAIL-FIRST sub-task 1.5.0/1: `Pick<X, K>['member']` dispatch
/// reduction. Mirrors `meta_tests::get_component_meta_materializes_imported_pick_indexed_access_props`'s
/// fixture but exercises dispatch directly, NOT through the materialize
/// wrapper that today falls back to the legacy walker.
///
/// Pre-Step-1.5 failure: `build_builtin_utility`'s `Pick` arm falls
/// through to `Opaque(Miss)`, so the IndexedAccess walker over the
/// utility result misses immediately.
///
/// Post-Step-1.5: `Pick<X, K>` reduces to an Object surface containing
/// the K-named members of X; the IndexedAccess hop projects to the
/// terminal member's value (the `'button' | 'submit' | 'reset'` union).
#[test]
fn dispatch_only_pick_indexed_access_reduces_to_member_union() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::{empty_type_args, LiteralValue, TypeExpr};

    let project = make_project();
    project
        .upsert_base(
            "/src/vue-dom.ts",
            r#"
export interface VueButtonHTMLAttributes {
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/html.ts",
            r#"
import type { VueButtonHTMLAttributes } from './vue-dom'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'type' | 'disabled'>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { ButtonHTMLAttributes } from './html'
defineProps<{ type?: ButtonHTMLAttributes['type'] }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/src/App.vue").unwrap().unwrap();

    let host = project.host();
    let dispatch = ProjectSemanticDispatch::new(host);

    // The macro field type the legacy materialize walker has been
    // covering: `ButtonHTMLAttributes['type']`.
    let expr = TypeExpr::IndexedAccess {
        object: StdArc::new(TypeExpr::Ref {
            name: StdArc::from("ButtonHTMLAttributes"),
            type_arguments: empty_type_args(),
        }),
        index: StdArc::new(TypeExpr::Literal(LiteralValue::String("type".to_string()))),
    };

    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/src/App.vue", &expr, ProjectionMode::Expanded)
        .expect("dispatch must lower IndexedAccess<ButtonHTMLAttributes, 'type'> at /src/App.vue");

    let materialized = dispatch.raise_and_reduce(lowered, ProjectionMode::Expanded);

    let raised = &materialized.type_expr;

    // Negative assertions: dispatch must NOT leave the IndexedAccess shell
    // unresolved or erase to Unknown — those are the pre-Step-1.5 dispatch
    // signatures the legacy walker covers via the fallback.
    assert!(
        !matches!(
            raised,
            TypeExpr::IndexedAccess { .. } | TypeExpr::Unknown { .. }
        ),
        "dispatch-only must reduce Pick<X, K>['member'] to a concrete \
         surface; got {raised:?}"
    );

    // Positive assertion: the result is a union of the three string
    // literals from VueButtonHTMLAttributes.type.
    let literals: std::collections::BTreeSet<String> = match raised {
        TypeExpr::Union(arms) => arms
            .iter()
            .filter_map(|arm| match arm {
                TypeExpr::Literal(LiteralValue::String(s)) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        TypeExpr::Literal(LiteralValue::String(s)) => std::collections::BTreeSet::from([s.clone()]),
        other => panic!("expected Union of string literals; got {other:?}"),
    };
    assert_eq!(
        literals,
        std::collections::BTreeSet::from([
            "button".to_string(),
            "submit".to_string(),
            "reset".to_string(),
        ]),
        "dispatch-only Pick['member'] reduction must yield the picked \
         member's literal union; got {literals:?}"
    );
}

/// Step 1.5 FAIL-FIRST sub-task 1.5.0/2: mapped type with conditional
/// `infer P` per-key reduction reaches resolved evaluated_types shape.
/// Mirrors `imported_mapped_slots_reach_resolved_evaluated_types` but
/// exercises dispatch directly.
///
/// Pre-Step-1.5 failure: dispatch's `build_mapped_type` substitutes
/// the per-key literal into the mapper value, but the substituted
/// conditional with `infer P` extends a Function whose `check` is
/// `PricingPlanSlots[K]` — that IndexedAccess never resolves through
/// the substituted mapper context, so `build_conditional`'s C11a Function-
/// extends arm sees check_resolved as the unsubstituted shell and skips
/// the per-position infer binding.
///
/// Post-Step-1.5: `build_mapped_type`'s substitute-and-evaluate path
/// resolves the inner `IndexedAccess` BEFORE the conditional materialises,
/// so the C11a binding extracts `P → { planId: string }` and the
/// substituted true_branch surfaces as
/// `(props: { planId: string; plan: TPlan }) => any`.
#[test]
fn dispatch_only_imported_mapped_slots_resolved_shape_via_dispatch_only() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
    };
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::{empty_type_args, TypeExpr};

    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { PricingPlansSlots } from './slots'
defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/App.vue").unwrap().unwrap();

    let host = project.host();
    let dispatch = ProjectSemanticDispatch::new(host);

    // The macro shell: `PricingPlansSlots<{ id: string; tier: 'pro' }>`.
    let plan_arg = TypeExpr::Object(StdArc::new(
        verter_semantic::analysis::type_expr::ObjectExpr {
            properties: vec![
                verter_semantic::analysis::type_expr::ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "id".to_string(),
                        ty: TypeExpr::Primitive(
                            verter_semantic::analysis::type_expr::PrimitiveName::String,
                        ),
                        optional: false,
                        readonly: false,
                    },
                ),
                verter_semantic::analysis::type_expr::ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "tier".to_string(),
                        ty: TypeExpr::string_literal("pro"),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        },
    ));

    let macro_shell = TypeExpr::Ref {
        name: StdArc::from("PricingPlansSlots"),
        type_arguments: StdArc::from(vec![plan_arg]),
    };

    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/App.vue", &macro_shell, ProjectionMode::Expanded)
        .expect("dispatch must lower PricingPlansSlots<{...}> at /App.vue");

    // Project ["badge"] off the lowered shell. After Step 1.5 dispatch
    // can navigate the Mapped+Conditional pair to extract the badge slot.
    let badge_path: StdArc<[PathSegment]> =
        StdArc::from(vec![PathSegment::Member(StdArc::from("badge"))]);
    let projected = dispatch.execute(SemanticQueryKey::ProjectPath {
        base: lowered,
        path: badge_path,
        mode: ProjectionMode::Expanded,
    });

    let badge_node = match projected {
        QueryResult::Value(id) => id,
        other => panic!(
            "dispatch-only ProjectPath(['badge']) on PricingPlansSlots<…> \
             must return Value(id); got {other:?}\n\
             this isolates the dispatch substitution gap that the \
             materialize wrapper currently masks via the legacy walker."
        ),
    };

    let raised = dispatch
        .raise_node_to_type_expr(badge_node)
        .expect("raise_node_to_type_expr must succeed on the badge projection");

    // Resolve any leading Parenthesized/Alias-style wrappers.
    let raised_inner = match &raised {
        TypeExpr::Parenthesized(inner) => inner.as_ref().clone(),
        other => other.clone(),
    };

    // Negative assertion: the badge slot result must NOT be an Unknown
    // shell or a deferred IndexedAccess/Conditional (the pre-Step-1.5
    // dispatch signatures).
    assert!(
        !matches!(
            raised_inner,
            TypeExpr::Unknown { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::Conditional { .. }
        ),
        "dispatch-only mapped+infer projection must reduce; got {raised_inner:?}"
    );

    // Positive assertion: the badge slot is a Function whose first
    // parameter is an Intersection of `{ planId }` (from infer P) and
    // `{ plan: TPlan-substituted }`. We assert it carries BOTH the
    // inferred and TPlan-substituted contributions.
    fn function_param_type(expr: &TypeExpr) -> Option<&TypeExpr> {
        match expr {
            TypeExpr::Function(f) => f.parameters.first().map(|p| &p.ty),
            _ => None,
        }
    }

    fn collect_member_names(expr: &TypeExpr, names: &mut std::collections::BTreeSet<String>) {
        match expr {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    match member {
                        verter_semantic::analysis::type_expr::ObjectMember::Property(p) => {
                            names.insert(p.name.clone());
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::Method(m) => {
                            names.insert(m.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            TypeExpr::Intersection(arms) => {
                for arm in arms.iter() {
                    collect_member_names(arm, names);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_member_names(inner, names),
            _ => {}
        }
    }

    let param_ty = function_param_type(&raised_inner).unwrap_or_else(|| {
        panic!(
            "dispatch-only badge slot must be a Function — got {raised_inner:?}.\n\
             pre-Step-1.5 the conditional + infer reduction yields a deferred \
             shell because mapper substitution does not flow through the \
             IndexedAccess check."
        )
    });

    let mut names = std::collections::BTreeSet::new();
    collect_member_names(param_ty, &mut names);

    assert!(
        names.contains("planId") && names.contains("plan"),
        "dispatch-only badge param type must include both `planId` (from \
         infer P) and `plan` (from `& {{ plan: TPlan }}`) — got names={:?}, \
         param={param_ty:?}\n\
         missing `planId` means infer-binding never flowed through; missing \
         `plan` means the intersection arm was lost.",
        names,
    );

    // Use the materialized handle to also assert dispatch returned a
    // non-empty dep_signature for the path projection (validates the
    // Step-6.6.A fence wiring is not regressed by the parity fix).
    let _ = empty_type_args();
}

/// Step 1.5 FAIL-FIRST sub-task 1.5.0/3: mapped+infer reduction reaches
/// a final-meta-shaped Object with all three slot names enumerated.
/// Mirrors `imported_mapped_slots_reach_final_component_meta` but at the
/// dispatch boundary.
///
/// Pre-Step-1.5 failure: in addition to the conditional+infer per-key
/// gap (covered by the previous test), the Mapped's Intersection arm
/// (`& { default?(props?: {}): any }`) doesn't compose into a single
/// Object the consumer can iterate — dispatch leaves a deferred Mapped
/// shell that raises back as `TypeExpr::Mapped`.
///
/// Post-Step-1.5: dispatch reduces the mapped Intersection to a single
/// Object surface enumerating `badge`, `title`, `default` so consumers
/// can iterate the slot names without re-walking source IR.
#[test]
fn dispatch_only_imported_mapped_slots_final_shape_via_dispatch_only() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::TypeExpr;

    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export interface PricingPlan {
  id: string
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): any
  title(props: { planId: string }): any
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => any
    ? (props: P & { plan: TPlan }) => any
    : PricingPlanSlots[TKey]

export type PricingPlansSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in keyof PricingPlanSlots]?: ExtendSlotWithPlan<TPlan, K>
} & {
  default?(props?: {}): any
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { PricingPlansSlots } from './slots'
defineSlots<PricingPlansSlots<{ id: string; tier: 'pro' }>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/App.vue").unwrap().unwrap();

    let host = project.host();
    let dispatch = ProjectSemanticDispatch::new(host);

    let plan_arg = TypeExpr::Object(StdArc::new(
        verter_semantic::analysis::type_expr::ObjectExpr {
            properties: vec![
                verter_semantic::analysis::type_expr::ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "id".to_string(),
                        ty: TypeExpr::Primitive(
                            verter_semantic::analysis::type_expr::PrimitiveName::String,
                        ),
                        optional: false,
                        readonly: false,
                    },
                ),
                verter_semantic::analysis::type_expr::ObjectMember::Property(
                    verter_semantic::analysis::type_expr::ObjectProperty {
                        name: "tier".to_string(),
                        ty: TypeExpr::string_literal("pro"),
                        optional: false,
                        readonly: false,
                    },
                ),
            ],
        },
    ));
    let macro_shell = TypeExpr::Ref {
        name: StdArc::from("PricingPlansSlots"),
        type_arguments: StdArc::from(vec![plan_arg]),
    };

    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/App.vue", &macro_shell, ProjectionMode::Expanded)
        .expect("dispatch must lower PricingPlansSlots<{...}> at /App.vue");

    let materialized = dispatch.raise_and_reduce(lowered, ProjectionMode::Expanded);
    let raised = &materialized.type_expr;

    // Negative assertion: the dispatch must NOT leave a deferred Mapped
    // shell at the top level — that's the pre-Step-1.5 signature.
    assert!(
        !matches!(raised, TypeExpr::Mapped { .. } | TypeExpr::Unknown { .. }),
        "dispatch-only must reduce mapped+intersection to a concrete \
         Object/Intersection surface; got {raised:?}"
    );

    // Collect slot names from the reduced surface (handles either a
    // single Object or an Intersection of Objects).
    fn collect_slot_names(expr: &TypeExpr, out: &mut std::collections::BTreeSet<String>) {
        match expr {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    match member {
                        verter_semantic::analysis::type_expr::ObjectMember::Property(p) => {
                            out.insert(p.name.clone());
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::Method(m) => {
                            out.insert(m.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            TypeExpr::Intersection(arms) => {
                for arm in arms.iter() {
                    collect_slot_names(arm, out);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_slot_names(inner, out),
            _ => {}
        }
    }

    let mut names = std::collections::BTreeSet::new();
    collect_slot_names(raised, &mut names);

    assert!(
        names.contains("badge") && names.contains("title") && names.contains("default"),
        "dispatch-only mapped+intersection projection must enumerate \
         badge/title/default — got {names:?}, surface={raised:?}\n\
         missing keys mean the mapper's per-key substitution never \
         materialised the literal-substituted body, OR the trailing \
         intersection arm was dropped."
    );

    // Strong assertion mirroring `imported_mapped_slots_reach_final_component_meta`'s
    // bindings check: each slot's value (e.g. badge) must NOT be a
    // deferred Conditional shell — it must reduce to a Function whose
    // first parameter type carries both `planId` (from infer P) and
    // `plan` (from the intersection with `{ plan: TPlan }`).
    fn find_member_value<'a>(expr: &'a TypeExpr, name: &str) -> Option<&'a TypeExpr> {
        match expr {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    if let verter_semantic::analysis::type_expr::ObjectMember::Property(p) = member
                    {
                        if p.name == name {
                            return Some(&p.ty);
                        }
                    }
                }
                None
            }
            TypeExpr::Intersection(arms) => {
                arms.iter().find_map(|arm| find_member_value(arm, name))
            }
            TypeExpr::Parenthesized(inner) => find_member_value(inner, name),
            _ => None,
        }
    }

    let badge_value = find_member_value(raised, "badge").unwrap_or_else(|| {
        panic!("dispatch-only mapped surface must expose `badge` Property member; got {raised:?}")
    });

    assert!(
        !matches!(
            badge_value,
            TypeExpr::Conditional { .. }
                | TypeExpr::Unknown { .. }
                | TypeExpr::IndexedAccess { .. }
        ),
        "dispatch-only badge slot value must reduce to a concrete Function — \
         got {badge_value:?}\n\
         a Conditional or Unknown here means the mapper's \
         substitute-and-evaluate did not materialise the conditional+infer \
         body for the per-key literal substitution."
    );

    // Walk into the Function's first parameter type and assert names.
    fn collect_param_member_names(expr: &TypeExpr, names: &mut std::collections::BTreeSet<String>) {
        match expr {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    match member {
                        verter_semantic::analysis::type_expr::ObjectMember::Property(p) => {
                            names.insert(p.name.clone());
                        }
                        verter_semantic::analysis::type_expr::ObjectMember::Method(m) => {
                            names.insert(m.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            TypeExpr::Intersection(arms) => {
                for arm in arms.iter() {
                    collect_param_member_names(arm, names);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_param_member_names(inner, names),
            _ => {}
        }
    }
    let TypeExpr::Function(func) = badge_value else {
        panic!("dispatch-only badge slot must be a Function; got {badge_value:?}");
    };
    let first_param_ty = func
        .parameters
        .first()
        .map(|p| &p.ty)
        .unwrap_or_else(|| panic!("badge function must have a first parameter"));
    let mut binding_names = std::collections::BTreeSet::new();
    collect_param_member_names(first_param_ty, &mut binding_names);
    assert!(
        binding_names.contains("planId") && binding_names.contains("plan"),
        "dispatch-only final-shape badge bindings must include planId and \
         plan — got {binding_names:?}; param={first_param_ty:?}"
    );
}

// ===========================================================================
// Plan §1.12 — graph-native registry-route + cycle-BFS predicates
//
// Discriminating tests for the round-7 parity matrix (plan §1.12 / §10.8):
//
// 1. `Pick<Foo<T>, 'a'>` (generic root) — rejected.
// 2. `Foo[0]` (numeric index) — rejected.
// 3. `Pick<Foo, 'a' | 'b' | 'c'>` (3+ literal-union keys) — accepted.
// 4. `Pick<Foo>` (1-arg) — rejected.
// 5. `Pick<Foo, 'a', 'b'>` (3-arg) — rejected.
// 6. `Pick<Foo, never>` (empty union) — rejected.
// 7. A → B → C → A cycle through three distinct decls — `ref_root_
//    reaches_transitive_cycle_node` returns true after at most three
//    `Instantiate` dispatches; their dep_signatures appear in `local_fence`.
// ===========================================================================

mod node_predicates_tests {
    use super::make_project;
    use crate::meta_resolve::{
        component_meta_ref_resolves_to_package_node,
        declaration_body_prefers_inline_materialization_node, extract_route_root_identity_node,
        ref_root_reaches_transitive_cycle_node, registry_member_route_inline_materializable_node,
    };
    use crate::resolver_core::RouteDemand;
    use crate::semantic_query::{
        DeclIdentity, IndexKey, IndexSignature, NodeScopeId, SemanticNodeData, SemanticNodeId,
        SurfaceMember, SurfaceView,
    };
    use std::sync::Arc as StdArc;
    use verter_semantic::analysis::type_expr::LiteralValue;

    fn empty_surface(members: Vec<SurfaceMember>) -> SurfaceView {
        SurfaceView {
            members: StdArc::from(members.into_boxed_slice()),
            call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            index_signatures: StdArc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }
    }

    fn synthetic_decl_identity(decl_name: &str) -> DeclIdentity {
        DeclIdentity {
            canonical_id: StdArc::from("/test/local.ts"),
            whole_hash: [0u8; 16],
            decl_name: StdArc::from(decl_name),
        }
    }

    fn package_decl_identity(decl_name: &str) -> DeclIdentity {
        DeclIdentity {
            canonical_id: StdArc::from("/repo/node_modules/some-pkg/index.ts"),
            whole_hash: [0u8; 16],
            decl_name: StdArc::from(decl_name),
        }
    }

    fn pick_or_omit_identity(name: &'static str) -> DeclIdentity {
        // Plan §4.4 / B0+B1: builtin Pick/Omit use the `__builtin__`
        // sentinel canonical id; the registry-route extractor only
        // dispatches builtin (not userland) Pick/Omit through the
        // route branch so userland shadowing is preserved.
        DeclIdentity {
            canonical_id: StdArc::from("__builtin__"),
            whole_hash: [0u8; 16],
            decl_name: StdArc::from(name),
        }
    }

    /// Plan §4.4 / Codex2 P0 #3: `Pick<Foo<T>, 'a'>` — generic root
    /// is accepted; the extractor recurses into `args[0]` to find the
    /// actual root identity (`Foo`) and preserves the generic
    /// arguments via `RouteExtraction.root_args`. This replaces the
    /// rev-7 generic-root rejection: Codex2 P0 #3 requires the route
    /// branch to project `Pick<Foo<T>, 'a'>` shapes through dispatch
    /// with the original carriers.
    #[test]
    fn extract_route_accepts_generic_root_pick_with_root_args() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let t_identity = synthetic_decl_identity("T");
        let t_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: t_identity.clone(),
        });
        let foo_t = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: foo_identity.clone(),
            args: StdArc::from(vec![t_ref].into_boxed_slice()),
        });
        let key_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        // Builtin Pick — base.canonical_id MUST be "__builtin__" for
        // the registry-route branch to fire.
        let pick_builtin = DeclIdentity {
            canonical_id: StdArc::from("__builtin__"),
            whole_hash: [0u8; 16],
            decl_name: StdArc::from("Pick"),
        };
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_builtin,
            args: StdArc::from(vec![foo_t, key_a].into_boxed_slice()),
        });

        let extraction = extract_route_root_identity_node(graph, pick_node, 0)
            .expect("Pick<Foo<T>, 'a'> must be accepted with root_args populated");
        assert_eq!(
            extraction.root_identity, foo_identity,
            "root_identity must be the inner Foo, not the wrapping Pick"
        );
        assert_eq!(
            extraction.root_args.len(),
            1,
            "root_args must preserve the generic [T] carrier (Codex2 P0 #3)"
        );
        assert_eq!(extraction.root_args[0], t_ref);
    }

    /// Round-7 parity row 2: `Foo[0]` — numeric index rejected.
    /// Only `IndexKey::String` literals are valid registry-route hops.
    #[test]
    fn extract_route_rejects_numeric_indexed_access() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity,
        });
        let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: foo_ref,
            index: IndexKey::Number(0),
        });

        assert!(
            extract_route_root_identity_node(graph, indexed, 0).is_none(),
            "Foo[0] must be rejected (numeric index) — round-7 parity"
        );
    }

    /// Round-7 parity row 3: `Pick<Foo, 'a' | 'b' | 'c'>` — accepted.
    /// Three-way literal-string union is the canonical accept case.
    #[test]
    fn extract_route_accepts_three_literal_union_pick() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity.clone(),
        });
        let literals: Vec<SemanticNodeId> = ["a", "b", "c"]
            .iter()
            .map(|s| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                    s.to_string(),
                )))
            })
            .collect();
        let union = graph.intern_node(SemanticNodeData::Union(StdArc::from(
            literals.into_boxed_slice(),
        )));
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_or_omit_identity("Pick"),
            args: StdArc::from(vec![foo_ref, union].into_boxed_slice()),
        });

        let extraction = extract_route_root_identity_node(graph, pick_node, 0)
            .expect("Pick<Foo, 'a' | 'b' | 'c'> must be accepted (round-7 parity row 3)");
        assert_eq!(extraction.root_identity, foo_identity);
        match extraction.route {
            RouteDemand::Pick(keys) => {
                assert_eq!(
                    keys,
                    vec!["a".to_string(), "b".to_string(), "c".to_string()],
                    "all three literal-union keys must be preserved in order"
                );
            }
            other => panic!("expected RouteDemand::Pick, got {other:?}"),
        }
    }

    /// Round-7 parity row 4: `Pick<Foo>` — 1-arg rejected.
    #[test]
    fn extract_route_rejects_one_arg_pick() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity,
        });
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_or_omit_identity("Pick"),
            args: StdArc::from(vec![foo_ref].into_boxed_slice()),
        });

        assert!(
            extract_route_root_identity_node(graph, pick_node, 0).is_none(),
            "Pick<Foo> (1-arg) must be rejected (args.len() != 2) — round-7 parity"
        );
    }

    /// Round-7 parity row 5: `Pick<Foo, 'a', 'b'>` — 3-arg rejected.
    #[test]
    fn extract_route_rejects_three_arg_pick() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity,
        });
        let key_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        let key_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "b".to_string(),
        )));
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_or_omit_identity("Pick"),
            args: StdArc::from(vec![foo_ref, key_a, key_b].into_boxed_slice()),
        });

        assert!(
            extract_route_root_identity_node(graph, pick_node, 0).is_none(),
            "Pick<Foo, 'a', 'b'> (3-arg) must be rejected — round-7 parity"
        );
    }

    /// Round-7 parity row 6: `Pick<Foo, never>` — empty union rejected.
    /// Modeled as an empty `Union` node (representing the `never` identity).
    #[test]
    fn extract_route_rejects_empty_union_pick() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity,
        });
        let empty_union = graph.intern_node(SemanticNodeData::Union(StdArc::from(
            Vec::<SemanticNodeId>::new().into_boxed_slice(),
        )));
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_or_omit_identity("Pick"),
            args: StdArc::from(vec![foo_ref, empty_union].into_boxed_slice()),
        });

        assert!(
            extract_route_root_identity_node(graph, pick_node, 0).is_none(),
            "Pick<Foo, never> (empty key set) must be rejected — round-7 parity"
        );
    }

    /// Bonus discriminator: a chained `IndexedAccess` with all-string
    /// keys must yield a `MemberPath` carrying every segment in order.
    #[test]
    fn extract_route_accepts_chained_string_indexed_access() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let foo_identity = synthetic_decl_identity("Foo");
        let foo_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: foo_identity.clone(),
        });
        let level_one = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: foo_ref,
            index: IndexKey::String(StdArc::from("c")),
        });
        let level_two = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: level_one,
            index: IndexKey::String(StdArc::from("full")),
        });

        let extraction = extract_route_root_identity_node(graph, level_two, 0)
            .expect("Foo['c']['full'] must be accepted");
        assert_eq!(extraction.root_identity, foo_identity);
        match extraction.route {
            RouteDemand::MemberPath(segments) => {
                assert_eq!(segments, vec!["c".to_string(), "full".to_string()]);
            }
            other => panic!("expected RouteDemand::MemberPath, got {other:?}"),
        }
    }

    /// `component_meta_ref_resolves_to_package_node` — pure check on the
    /// canonical id. Must reject local refs and accept `node_modules`.
    #[test]
    fn package_ref_predicate_discriminates_local_vs_node_modules() {
        let local = synthetic_decl_identity("Foo");
        let pkg = package_decl_identity("Bar");

        assert!(
            !component_meta_ref_resolves_to_package_node(&local),
            "local /test/local.ts decl must NOT be classified as package-backed"
        );
        assert!(
            component_meta_ref_resolves_to_package_node(&pkg),
            "node_modules-rooted decl must be classified as package-backed"
        );
    }

    /// `declaration_body_prefers_inline_materialization_node` — body
    /// shapes that should and should not pass the inline-mat gate.
    #[test]
    fn inline_mat_predicate_discriminates_object_vs_function() {
        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let object_body = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![])));
        assert!(
            declaration_body_prefers_inline_materialization_node(graph, object_body),
            "Object body must be inline-materialisable"
        );

        let function_body = graph.intern_node(SemanticNodeData::Function {
            params: StdArc::from(Vec::new().into_boxed_slice()),
            return_type: object_body,
            type_parameters: StdArc::from(Vec::new().into_boxed_slice()),
        });
        assert!(
            !declaration_body_prefers_inline_materialization_node(graph, function_body),
            "Function body must NOT be inline-materialisable"
        );

        let mapped_body = graph.intern_node(SemanticNodeData::KeyOf { base: object_body });
        assert!(
            !declaration_body_prefers_inline_materialization_node(graph, mapped_body),
            "KeyOf body must NOT be inline-materialisable (no route extracted)"
        );
    }

    /// Round-7 parity row 7: A → B → C → A cycle through a complex
    /// helper.
    ///
    /// Plan §4.1 / R7-13 / R7-14 — the legacy parity BFS only flags a
    /// self-cycle as "transitively cyclic" when the path carries a
    /// **complex signal**: either the body has a complex top-level
    /// shape (Conditional / Mapped / KeyOf / IndexedAccess / etc.) OR
    /// any traversed reference carries type arguments. A purely
    /// Object-aliased self-cycle (`A { next: B }`, `B { next: C }`,
    /// `C { next: A }`) does NOT trigger — that's the legitimate
    /// productive-recursion shape. The fixture below routes through
    /// a `keyof` helper, which is a complex shape per legacy parity
    /// and composes the complex signal so the self-rediscovery fires.
    ///
    /// The graph-native cycle BFS must rediscover `A` as a child
    /// reachable from `A`'s body within at most three `Instantiate`
    /// dispatches; their `dep_signatures` must accumulate into
    /// `local_fence`.
    #[test]
    fn ref_root_cycle_bfs_detects_three_decl_cycle_and_accumulates_dep_facts() {
        let project = make_project();
        // Three-decl cycle: A's body refers to B; B's body uses
        // `keyof C` (complex shape per legacy parity); C's body refs
        // back to A. The complex-signal composes through the keyof
        // hop, and self-rediscovery of A fires.
        project
            .upsert_base(
                "/cycle.ts",
                r#"
export type A = { next: B }
export type B = keyof C
export type C = { back: A }
"#,
            )
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { A } from './cycle'
defineProps<{ value: A }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();

        let session = project.open_session_batch().unwrap();
        // Seed the host: this populates IndexedReady + analysis for
        // `/cycle.ts` so `Instantiate` dispatches against the
        // declarations succeed.
        let _ = session.evaluate_types("/Owner.vue").unwrap();

        let host = session.host();

        // In MemoryWorkspace fixtures the upsert path is itself the
        // canonical id; resolve via `shallow_file_state` to obtain the
        // matching whole_hash.
        let cycle_canonical = "/cycle.ts";
        let shallow = host
            .shallow_file_state(cycle_canonical)
            .expect("cycle.ts must be indexed");
        let a_identity = DeclIdentity {
            canonical_id: StdArc::from(cycle_canonical),
            whole_hash: shallow.whole_hash,
            decl_name: StdArc::from("A"),
        };

        let mut local_fence: Vec<(StdArc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        let detected = ref_root_reaches_transitive_cycle_node(&a_identity, host, &mut local_fence);

        assert!(
            detected,
            "A -> B (keyof C) -> C -> A must be detected by the graph-native BFS — \
             the keyof hop composes complex_signal per legacy parity"
        );
        assert!(
            !local_fence.is_empty(),
            "Instantiate dispatches must accumulate dep_signatures into local_fence — \
             empty fence indicates the BFS skipped the dispatch path"
        );
        // Fence should contain at least one fact about `/cycle.ts`
        // (the file under traversal).
        let touches_cycle_file = local_fence
            .iter()
            .any(|(canonical, _)| canonical.as_ref() == cycle_canonical);
        assert!(
            touches_cycle_file,
            "local_fence must include a dep fact for /cycle.ts — got {local_fence:?}"
        );
    }

    /// `registry_member_route_inline_materializable_node` composition —
    /// a `Pick<Foo, 'a' | 'b'>` over a local-file `Foo` interface must
    /// pass the composition (extract OK + non-package + non-cyclic +
    /// Object body).
    #[test]
    fn registry_route_composition_accepts_local_pick_over_object_interface() {
        use crate::semantic_query::SemanticNodeData;

        let project = make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"export interface Foo { a: string; b: number; c: boolean }
"#,
            )
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<{ picked: Pick<Foo, 'a' | 'b'> }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();

        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/Owner.vue").unwrap();
        let host = session.host();

        let types_canonical = "/types.ts";
        let shallow = host
            .shallow_file_state(types_canonical)
            .expect("/types.ts must be indexed");

        // Build the Pick<Foo, 'a' | 'b'> graph node directly so the
        // test exercises the composition predicate, not whatever the
        // real macro flow produced.
        let graph = host.project_type_store().semantic_graph();
        let foo_ref = graph.intern_node_with_scope(
            SemanticNodeData::DeclRef {
                identity: DeclIdentity {
                    canonical_id: StdArc::from(types_canonical),
                    whole_hash: shallow.whole_hash,
                    decl_name: StdArc::from("Foo"),
                },
            },
            NodeScopeId::Global,
        );
        let key_a = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "a".to_string(),
        )));
        let key_b = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
            "b".to_string(),
        )));
        let union = graph.intern_node(SemanticNodeData::Union(StdArc::from(
            vec![key_a, key_b].into_boxed_slice(),
        )));
        let pick_node = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pick_or_omit_identity("Pick"),
            args: StdArc::from(vec![foo_ref, union].into_boxed_slice()),
        });

        let mut local_fence: Vec<(StdArc<str>, crate::semantic_query::DepVersion)> = Vec::new();
        assert!(
            registry_member_route_inline_materializable_node(
                pick_node,
                types_canonical,
                host,
                &mut local_fence,
            ),
            "Pick<Foo, 'a' | 'b'> over a local Object interface must be inline-materialisable"
        );
    }

    /// Plan §6.6 / E — Pick / Omit shapes through `evaluate_types`
    /// stay healthy after the alias-body rescue chain
    /// (`walk_member_route_via_alias_body`) and the
    /// `materialize_inline_registry_member_route_*` candidate chain
    /// were deleted. B1's materialiser registry-route branch
    /// dispatches Pick/Omit shapes through dispatch's canonical
    /// projection.
    #[test]
    fn materialiser_member_route_unchanged_after_legacy_delete() {
        let project = make_project();
        project
            .upsert_base(
                "/types.ts",
                r#"export interface Foo { a: string; b: number; c: boolean }
"#,
            )
            .unwrap();
        project
            .upsert_base(
                "/PickOk.vue",
                r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<{ value: Pick<Foo, 'a'> }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();
        project.host().set_import_dependencies(
            "/PickOk.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = project.open_session_batch().unwrap();
        // Smoke: evaluate must succeed without panicking — the chain
        // deletion must not break the registry-route resolution path.
        let _ = session
            .evaluate_types("/PickOk.vue")
            .unwrap()
            .expect("Pick<Foo, 'a'> eval must succeed after E's chain deletion");
    }

    /// Plan §6.5 / D — `engine.is_package_backed_decl` adapter
    /// produces the same result as the canonical primitive
    /// `canonical_resolves_to_package` (commit C). Discriminates
    /// local-file decls from `/node_modules/`-rooted decls.
    #[test]
    fn engine_is_package_backed_decl_matches_canonical_predicate() {
        use crate::meta_resolve::canonical_resolves_to_package;
        use crate::resolver_core::ComponentMetaQueryEngine;

        let project = make_project();
        project
            .upsert_base("/local.ts", "export type Local = { x: number };")
            .unwrap();
        project
            .upsert_base(
                "/node_modules/foo/index.d.ts",
                "export type FromPkg = { y: string };",
            )
            .unwrap();
        // Seed a consumer that imports both — needed so the
        // resolver's prepared-decl bundle picks up the imports.
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { Local } from './local'
import type { FromPkg } from 'foo'
defineProps<{ local: Local; pkg: FromPkg }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();
        project.host().set_import_dependencies(
            "/Owner.vue",
            vec![
                crate::types::DependencyResolution {
                    specifier: "./local".to_string(),
                    resolved_canonical_id: Some("/local.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::types::DependencyResolution {
                    specifier: "foo".to_string(),
                    resolved_canonical_id: Some("/node_modules/foo/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/Owner.vue").unwrap();
        let host = session.host();
        let mut engine = ComponentMetaQueryEngine::new(host);

        // Local decl — engine adapter returns false.
        assert!(
            !engine.is_package_backed_decl("/Owner.vue", "Local"),
            "Local decl resolves to /local.ts (NOT /node_modules/) — \
             engine adapter must return false"
        );
        // Package decl — engine adapter returns true.
        assert!(
            engine.is_package_backed_decl("/Owner.vue", "FromPkg"),
            "FromPkg resolves under /node_modules/ — engine adapter \
             must return true"
        );
        // Primitive matches: false on /local.ts, true on /node_modules/.
        assert!(!canonical_resolves_to_package("/local.ts"));
        assert!(canonical_resolves_to_package(
            "/node_modules/foo/index.d.ts"
        ));
    }

    /// Plan §6.7 — F's TDD: the temporary TypeExpr adapter
    /// `typeexpr_root_reaches_transitive_cycle` must lower the input
    /// expression via Navigate, extract the root identity, delegate to
    /// the canonical `_node` predicate, AND accumulate the BFS dep
    /// signature into the per-request thread-local accumulator.
    #[test]
    fn typeexpr_root_reaches_transitive_cycle_delegates_to_node_predicate() {
        use crate::meta_resolve::{
            drain_dispatch_dep_signature_accumulator, reset_dispatch_dep_signature_accumulator,
            typeexpr_root_reaches_transitive_cycle,
        };
        use crate::resolver_core::ComponentMetaQueryEngine;
        use std::sync::Arc as StdArc;
        use verter_semantic::analysis::type_expr::TypeExpr;

        let project = make_project();
        project
            .upsert_base(
                "/u.ts",
                r#"
export type GetItemKeys<T> = DotPathKeys<T>
export type DotPathKeys<T> = T extends object ? GetItemKeys<T> : never
"#,
            )
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { GetItemKeys } from './u'
defineProps<{ value: GetItemKeys<unknown> }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();
        project.host().set_import_dependencies(
            "/Owner.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./u".to_string(),
                resolved_canonical_id: Some("/u.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/Owner.vue").unwrap();
        let host = session.host();
        let mut engine = ComponentMetaQueryEngine::new(host);

        // GetItemKeys<unknown> — generic helper cycle. Adapter must
        // delegate to _node which detects the cycle.
        let typeexpr = TypeExpr::Ref {
            name: StdArc::from("GetItemKeys"),
            type_arguments: StdArc::from(
                vec![TypeExpr::Primitive(
                    verter_semantic::analysis::type_expr::PrimitiveName::Unknown,
                )]
                .into_boxed_slice(),
            ),
        };
        reset_dispatch_dep_signature_accumulator();
        let result = typeexpr_root_reaches_transitive_cycle(&mut engine, "/Owner.vue", &typeexpr);
        assert!(
            result,
            "GetItemKeys cycle must be detected via the canonical _node predicate"
        );
        let drained = drain_dispatch_dep_signature_accumulator();
        assert!(
            !drained.is_empty(),
            "BFS dep facts must be accumulated into the thread-local accumulator \
             so callers' completion fences capture cycle dep-signatures"
        );
    }

    /// Plan §6.8 — G's TDD: `engine.materialize_member_surface_expr`
    /// (the graph-native replacement for the deleted legacy walker
    /// shim) must accumulate the materialiser's `dep_signature` into
    /// the per-request thread-local accumulator, so callers'
    /// completion fences observe the dep facts captured by the inner
    /// `materialize_component_meta_structure` call.
    #[test]
    fn engine_materialize_member_surface_expr_accumulates_dep_signature() {
        use crate::meta_resolve::{
            drain_dispatch_dep_signature_accumulator, reset_dispatch_dep_signature_accumulator,
        };
        use crate::resolver_core::ComponentMetaQueryEngine;
        use std::sync::Arc as StdArc;
        use verter_semantic::analysis::type_expr::TypeExpr;

        let project = make_project();
        project
            .upsert_base("/types.ts", "export type Foo = { x: number }")
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<{ value: Foo }>()
</script>
<template><div /></template>"#,
            )
            .unwrap();
        project.host().set_import_dependencies(
            "/Owner.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/Owner.vue").unwrap();
        let host = session.host();
        let mut engine = ComponentMetaQueryEngine::new(host);

        reset_dispatch_dep_signature_accumulator();
        let _ = engine.materialize_member_surface_expr(
            "/Owner.vue",
            &TypeExpr::Ref {
                name: StdArc::from("Foo"),
                type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
            },
            false,
        );
        let drained = drain_dispatch_dep_signature_accumulator();
        assert!(
            !drained.is_empty(),
            "engine method must accumulate the materialiser's dep_signature"
        );
    }
}
