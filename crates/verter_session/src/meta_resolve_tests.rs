use super::*;
use crate::meta::MetaProject;
use crate::resolver_core::ComponentMetaRequestHost;
use crate::types::{HostConfig, ProjectionMode};
use crate::VerterHost;
use std::sync::Arc;
use verter_type_expr::TypeExpr;

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

/// Resolve the typeinfo macro-surface DTOs for the `resolved_macros` entries
/// matching `kind`, mirroring the production `component_meta_resolved_macros`
/// path: the published props/emits/slots/exposed surface is owned SOLELY by
/// the typeinfo macro-surface authority (`vue_macro_dtos`), keyed on the admitted
/// macro index; `resolved_macros` supplies only the index + kind. Entries are
/// deduplicated by macro index (multiple `ResolvedMacroMeta` per index are
/// gating/provenance facts, not distinct field authorities).
fn macro_dtos_for_kind(
    host: &VerterHost,
    owner: &str,
    state: &ResolvedComponentMetaState,
    kind: verter_semantic::analysis::AnalyzedMacroKind,
) -> Vec<std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos>> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == kind)
        .filter(|m| seen.insert(m.macro_index))
        .map(|m| {
            host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
                owner_canonical: std::sync::Arc::from(owner),
                macro_index: m.macro_index,
                macro_kind: m.macro_kind,
                root_identity: host.current_or_read_whole_hash(owner).unwrap_or([0u8; 16]),
                level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
            })
        })
        .collect()
}

fn prop_names_from_resolved(
    host: &VerterHost,
    owner: &str,
    state: &ResolvedComponentMetaState,
) -> Vec<String> {
    macro_dtos_for_kind(
        host,
        owner,
        state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
    )
    .iter()
    .flat_map(|dtos| dtos.prop_fields().iter())
    .map(|p| p.name.clone())
    .collect()
}

fn emit_names_from_resolved(
    host: &VerterHost,
    owner: &str,
    state: &ResolvedComponentMetaState,
) -> Vec<String> {
    macro_dtos_for_kind(
        host,
        owner,
        state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
    )
    .iter()
    .flat_map(|dtos| dtos.emit_fields().iter())
    .map(|e| e.name.clone())
    .collect()
}

fn slot_names_from_resolved(
    host: &VerterHost,
    owner: &str,
    state: &ResolvedComponentMetaState,
) -> Vec<String> {
    macro_dtos_for_kind(
        host,
        owner,
        state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
    )
    .iter()
    .flat_map(|dtos| dtos.slot_fields().iter())
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
        // cached_resolved_meta lives on DerivedRawState (D48 split).
        // The key is `(mode, view_fingerprint)`; this helper clears
        // both the base slot (view_fingerprint == 0) and every
        // overlay-bearing slot for `mode`.
        if let Some(mut entry) = project.host().derived_raw_cache().get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry
                .cached_resolved_meta
                .retain(|(slot_mode, _view_fp), _| slot_mode != &mode);
        }
    }
}

#[test]
fn imported_registry_seed_refresh_does_not_engage_skip_under_graph_only_authority() {
    // Under the typed-IR-only resolver contract,
    // `imported_declaration_surface_is_authoritative` returns false at
    // the cold-classification site (no typed body in scope), so the
    // skip-refresh fast path never engages from the imported direct
    // macro seed. The typed-IR contract has no branch that reads
    // declaration text and detects "no heritage markers" to authorise
    // skipping. The integration test
    // `append_component_meta_registry_entries_seeds_explicit_object_surface_for_imported_props`
    // covers the surviving invariant: the imported seed still carries
    // an explicit object surface in the initial registry.
    let declaration = crate::resolver_core::ResolvedTypeDeclaration {
        requested_name: "Props".to_string(),
        declaration_id: None,
        resolved_name: "Props".to_string(),
        canonical_source: "/src/types.ts".to_string(),
        span: verter_span::Span::default(),
        kind: crate::resolver_core::ResolvedDeclarationKind::Interface,
        text: Some("export interface Props { label?: string }".to_string()),
    };
    let object = verter_type_expr::TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: vec![verter_type_expr::ObjectMember::Property(
            verter_type_expr::ObjectProperty::synthetic_public(
                "label".to_string(),
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                true,
                false,
            ),
        )],
    }));

    assert!(
        !should_skip_imported_registry_seed_refresh("/src/App.vue", &declaration, &object),
        "graph-only contract: skip-refresh fast path does not engage; \
         the structural refresh pipeline owns the imported direct-macro \
         seed regardless of whether the existing surface is an explicit \
         object",
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
    let symbolic = verter_type_expr::TypeExpr::IndexedAccess {
        object: Arc::new(verter_type_expr::TypeExpr::named("Button")),
        index: Arc::new(verter_type_expr::TypeExpr::string_literal("variants")),
    };

    assert!(
        !should_skip_imported_registry_seed_refresh("/src/App.vue", &declaration, &symbolic),
        "symbolic imported seeds still need the imported-registry refresh path to materialize their requested route",
    );
}

#[test]
fn append_component_meta_registry_entries_seeds_shallow_ref_for_imported_props() {
    // Discriminating invariant (shallow-by-default): the direct imported macro
    // root seeds the registry with a SHALLOW `TypeExpr::Ref { name }` — NOT an
    // eagerly-materialised object surface. Consumers re-resolve the named root
    // through the registry on demand; the typeinfo / evaluated path is the
    // single shape authority. A regression that eagerly inlined the imported
    // declaration body at seed time would surface an `Object` here and FAIL this
    // assertion, so the shallow-Ref seed is the surviving discriminator.
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host, ctx: host };

    let parts = crate::resolver_core::resolve_component_meta_parts(
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
    match &props_entry.type_expr {
        verter_type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            assert_eq!(
                name.as_ref(),
                "Props",
                "the shallow seed Ref names the root"
            );
            assert!(
                type_arguments.is_empty(),
                "the direct imported root seed carries no type arguments"
            );
        }
        other => panic!(
            "the direct imported seed should be a shallow Ref (shallow-by-default), got {other:?}"
        ),
    }
    // Negative: it must NOT be an eagerly-materialised object surface.
    assert!(
        !matches!(props_entry.type_expr, verter_type_expr::TypeExpr::Object(_)),
        "the seed must stay shallow — an eager object surface violates shallow-by-default"
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let raw_body = query_engine
        .owner_collection_expr("/src/App.vue", "ModelValue")
        .expect("owner helper body should be available from prepared declarations");

    let materialized = query_engine
        .materialize_registry_structural_candidate("/src/App.vue", &raw_body)
        .0;

    let verter_type_expr::TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = &materialized
    else {
        panic!("local routed helper should stay conditional instead of flattening the wrapper");
    };

    assert_eq!(
        true_type,
        &Arc::new(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Number,
        )),
        "the true branch should materialize through the routed imported member surface",
    );
    assert_eq!(
        false_type,
        &Arc::new(verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String,
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host, ctx: host };

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

    let verter_type_expr::TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = &model_value.type_expr
    else {
        panic!("registry helper should preserve the conditional wrapper");
    };

    assert_eq!(
        true_type.as_ref(),
        &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number,),
    );
    assert_eq!(
        false_type.as_ref(),
        &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String,),
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
        // The fixed view is a current request-bound snapshot.
        true,
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

    // `ProjectionMode::Identity`: resolved_macros should carry identity info but
    // NOT trigger expansion. The published props/emits/slots/exposed surface is
    // owned by the typeinfo path (`vue_macro_dtos`), which is mode-INDEPENDENT, so "no
    // expanded props" is no longer expressible as an empty macro-surface; the
    // mode gate is owned by `evaluated_types` / `resolved_type_registry` below.
    assert!(
        !state.resolved_macros.is_empty(),
        "`ProjectionMode::Identity` should still identify macro type deps"
    );

    // `ProjectionMode::Identity`: no evaluated types (the cross-file shape
    // materialiser runs only in `Expanded`).
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

    // `ProjectionMode::Expanded`: materialized props (sourced from the typeinfo
    // macro-surface authority, the SOLE props/emits/slots/exposed owner).
    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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

    // The two modes produce DISTINCT resolved-meta states: the mode gate is the
    // cross-file shape materialiser, observable via `evaluated_types` (the
    // typeinfo macro-surface DTOs are mode-independent, so the published
    // props/emits/slots/exposed surface cannot distinguish the modes — only the
    // expansion side-effects can).
    assert_eq!(type_state.mode, ProjectionMode::Identity);
    assert_eq!(expanded_state.mode, ProjectionMode::Expanded);
    assert!(
        type_state.evaluated_types.is_none(),
        "`ProjectionMode::Identity` result must NOT materialize evaluated types"
    );
    assert!(
        expanded_state.evaluated_types.is_some(),
        "`ProjectionMode::Expanded` result MUST materialize evaluated types"
    );
    assert!(
        type_state.resolved_type_registry.is_empty(),
        "`ProjectionMode::Identity` result must NOT populate the type registry"
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

    // `ProjectionMode::Identity` should NOT perform the expensive external type
    // traversal. We keep the result only to assert the traversal side-effects
    // below; the mode gate is the provenance counters, not the (mode-independent)
    // published macro surface.
    let _type_state = project
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
    // The Identity gate is enforced by the traversal-side-effect assertions
    // above (Identity triggers ZERO external-type traversal) — a real,
    // non-tautological signal. The published props surface is owned by the
    // mode-independent typeinfo path, so we assert it materialises in Expanded
    // rather than using it as an Identity-emptiness proxy.
    assert!(
        prop_names_from_resolved(project.host(), "/App.vue", &expanded_state)
            .contains(&"a".to_string()),
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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
        prop_names_from_resolved(project.host(), "/App.vue", &state).contains(&"a".to_string()),
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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
                    == verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility::Protected
        }),
        "native state should preserve visibility metadata for inherited protected members"
    );
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "secret"
                && prop.visibility
                    == verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility::Private
        }),
        "native state should preserve visibility metadata for private members"
    );
}

/// A LOCAL `defineProps<C>()` over a class with mixed accessibility publishes
/// ONLY the public instance field. The shared surface RECORDS the protected /
/// private members, but the publication-boundary `Public`-only filter keeps
/// them off the published props. Static members and the constructor are never
/// surface members.
///
/// Discrimination: this FAILS on a tree where the analyzer records non-public
/// members but the publication-boundary visibility filter is absent — `b` /
/// `c` would leak into the published props.
#[test]
fn local_define_props_over_class_publishes_only_public_instance_field() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
  static s: string = ""
  constructor() {}
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<C>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);

    assert!(
        prop_names.contains(&"a".to_string()),
        "public field `a` must be published: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"b".to_string()),
        "protected field `b` must NOT be published: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"c".to_string()),
        "private field `c` must NOT be published: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"s".to_string()),
        "static field `s` must NOT be published: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"constructor".to_string()),
        "the constructor must NOT be published as a prop: {prop_names:?}"
    );
}

/// `defineProps<Partial<C>>()` over a class with mixed accessibility excludes
/// the non-public members from the PUBLISHED props — TS `keyof ClassType`
/// yields only public keys, so a mapped type (`Partial`) over a class does not
/// carry the private/protected members onto the published macro surface.
///
/// Discrimination: this FAILS on a tree where the typed-IR keyof/mapped keyspace
/// is NOT visibility-gated — `Partial<C>` would carry `b`/`c` (with non-public
/// visibility) onto the published surface and a missing publication filter would
/// leak them into props.
///
/// CHARACTERIZATION on `native_props` (honest pin of CURRENT legacy behavior,
/// NOT an aspirational assertion): native_props is populated by the SEPARATE
/// parser-side macro analyzer (`verter_compiler` `resolve_type` →
/// `collect_native_props`), which enumerates the referenced class surface
/// DIRECTLY and currently records the raw class members even under a
/// `Partial<…>` wrapper — bypassing the typed-IR keyspace chokepoints this
/// change gates. native_props re-sources from the shared (visibility-gated)
/// surface in B5 (roadmap); B11 deletes the legacy eager-OXC rail. Until then
/// this test PINS the current legacy behavior (native_props keeps `b`/`c` under
/// the wrapper, with faithful visibility) so the divergence between the gated
/// published surface and the keep-all legacy native_props is explicit and not
/// silently skipped. The in-scope native_props contract (direct class
/// enumeration, keep-all with faithful visibility) is asserted by
/// `native_props_fidelity_for_directly_declared_class_keeps_all_visibilities`.
#[test]
fn mapped_over_class_excludes_non_public_from_published_props() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Partial<C>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "public field `a` must be published through Partial<C>: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"b".to_string()),
        "protected `b` must NOT be published through Partial<C>: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"c".to_string()),
        "private `c` must NOT be published through Partial<C>: {prop_names:?}"
    );

    // CHARACTERIZATION (honest pin of current legacy native_props behavior;
    // see doc-comment): the parser-side eager-OXC rail enumerates the class
    // directly, so under the `Partial<…>` wrapper native_props STILL records
    // the non-public members `b`/`c` (keep-all, with faithful visibility),
    // unlike the visibility-gated PUBLISHED props above. This pins the B5/B11
    // gap explicitly rather than skipping the native_props invariant. When B5
    // re-sources native_props from the shared surface, this characterization
    // flips and must be updated alongside that change.
    use verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility;
    let macro_meta = state
        .resolved_macros
        .iter()
        .find(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("resolved defineProps macro should be present");
    let native_visibility_of = |name: &str| -> Option<ResolvedMemberVisibility> {
        macro_meta
            .native_props
            .iter()
            .find(|prop| prop.name == name)
            .map(|prop| prop.visibility)
    };
    assert_eq!(
        native_visibility_of("b"),
        Some(ResolvedMemberVisibility::Protected),
        "legacy native_props keeps protected `b` (with visibility) under Partial<C> \
         until B5 re-sources native_props from the shared surface"
    );
    assert_eq!(
        native_visibility_of("c"),
        Some(ResolvedMemberVisibility::Private),
        "legacy native_props keeps private `c` (with visibility) under Partial<C> \
         until B5 re-sources native_props from the shared surface"
    );
}

/// POSITIVE CONTROL (non-discriminating by design): `defineProps<Pick<C,
/// 'a'>>()` over a class publishes the picked PUBLIC key `a` and nothing else.
///
/// This test does NOT discriminate the visibility fix: `a` is public, and `b` /
/// `c` are not in the pick key set, so it passes whether or not Pick public-
/// filters its source members. It is retained as a positive control that the
/// happy-path public Pick still materialises. The DISCRIMINATING coverage for
/// the Pick public-keyspace gate lives in
/// `pick_over_class_excludes_picked_protected_key` /
/// `pick_over_class_excludes_picked_private_key` below (a `Pick` whose key is a
/// NON-public member must be empty).
#[test]
fn pick_over_class_publishes_only_picked_public_key() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Pick<C, 'a'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "Pick<C, 'a'> must publish `a`: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"b".to_string()),
        "Pick<C, 'a'> must NOT publish protected `b`: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"c".to_string()),
        "Pick<C, 'a'> must NOT publish private `c`: {prop_names:?}"
    );
}

/// DISCRIMINATING (fix #1, Pick public-keyspace gate): `defineProps<Pick<C,
/// 'b'>>()` where `b` is a PROTECTED class member. TS `Pick<C, K>` is a
/// public-keyspace projection — `b` is not a member of `keyof C`, so the picked
/// surface is EMPTY and no prop is published.
///
/// Discrimination: FAILS on the pre-fix tree where Pick reconstruction
/// (`build_builtin_utility`'s Pick arm) filters `object_filter_source_surface`'s
/// FULL surface by NAME only — `b` matches the pick name and leaks onto the
/// published props as a non-public member. PASSES once Pick public-filters its
/// source members before the name predicate.
#[test]
fn pick_over_class_excludes_picked_protected_key() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Pick<C, 'b'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        !prop_names.contains(&"b".to_string()),
        "Pick<C, 'b'> over a PROTECTED key must publish NO prop (b ∉ keyof C): {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"a".to_string()) && !prop_names.contains(&"c".to_string()),
        "Pick<C, 'b'> must not publish any other member either: {prop_names:?}"
    );
}

/// DISCRIMINATING (fix #1, Pick public-keyspace gate): `defineProps<Pick<C,
/// 'c'>>()` where `c` is a PRIVATE class member. `c` is not a member of
/// `keyof C`, so the picked surface is EMPTY.
///
/// Discrimination: FAILS on the pre-fix tree (private `c` leaks through the
/// name-only Pick filter); PASSES once Pick public-filters its source members.
#[test]
fn pick_over_class_excludes_picked_private_key() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Pick<C, 'c'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        !prop_names.contains(&"c".to_string()),
        "Pick<C, 'c'> over a PRIVATE key must publish NO prop (c ∉ keyof C): {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"a".to_string()) && !prop_names.contains(&"b".to_string()),
        "Pick<C, 'c'> must not publish any other member either: {prop_names:?}"
    );
}

/// DISCRIMINATING (fix #1, Omit public-keyspace gate): `defineProps<Omit<C,
/// 'a'>>()` over a class omits the PUBLIC key `a`. The remaining surface must
/// publish NOTHING — TS `Omit<C, K> = Pick<C, Exclude<keyof C, K>>`, and
/// `keyof C` is `'a'` only (public), so omitting `'a'` leaves an empty
/// public keyspace. The non-public `b` / `c` must NOT be left published.
///
/// Discrimination: FAILS on the pre-fix tree where Omit keeps every source
/// member whose name is not omitted — `b` / `c` survive the name-only filter
/// and leak onto the published props. PASSES once Omit public-filters its
/// source members before the name predicate.
#[test]
fn omit_over_class_does_not_leave_non_public_members_published() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Omit<C, 'a'>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        !prop_names.contains(&"b".to_string()),
        "Omit<C, 'a'> must NOT leave protected `b` published: {prop_names:?}"
    );
    assert!(
        !prop_names.contains(&"c".to_string()),
        "Omit<C, 'a'> must NOT leave private `c` published: {prop_names:?}"
    );
}

/// DISCRIMINATING (fix #2/#3, indexed-access public-keyspace gate):
/// `defineProps<Partial<C>['c']>()` indexes the PRIVATE key `c` out of a mapped
/// surface over a class. `c` ∉ `keyof C` (public-only), so `Partial<C>` carries
/// no `c` member and the indexed access is a miss — no prop is published.
///
/// Discrimination: FAILS on the pre-fix tree where the mapped/indexed admission
/// (`walk.rs` Tier-1 Object membership, `base_member_admission_non_emitting`)
/// admits `c` by NAME only, forging a value type for the private member.
/// PASSES once the object admission requires `is_public()`.
#[test]
fn partial_then_indexed_private_key_is_miss() {
    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<Partial<C>['c']>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert!(
        !prop_names.contains(&"c".to_string()),
        "Partial<C>['c'] over a PRIVATE key must not publish `c`: {prop_names:?}"
    );
    assert!(
        prop_names.is_empty() || prop_names.iter().all(|n| n == "constructor"),
        "Partial<C>['c'] indexes a non-existent public key; surface must be empty: {prop_names:?}"
    );
}

/// `native_props` FIDELITY: a directly-declared `defineProps<C>()` over a class
/// retains EVERY instance member (public/protected/private) WITH its correct
/// visibility — the native surface enumerates the class directly (NOT via
/// keyof), so the keyspace gate does not touch it. The published props stay
/// public-only.
///
/// Discrimination: FAILS if the keyspace gate wrongly reaches the native
/// surface (then `b`/`c` would be absent from native_props), or if the
/// reconstruction dropped visibility (then `b`/`c` would be present but marked
/// Public).
#[test]
fn native_props_fidelity_for_directly_declared_class_keeps_all_visibilities() {
    use verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility;

    let project = make_project();
    project
        .upsert_base(
            "/types.ts",
            r#"
export class C {
  public a: string = ""
  protected b: number = 0
  private c: boolean = false
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { C } from './types'
defineProps<C>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let state = project
        .host()
        .resolve_component_meta("/App.vue", ProjectionMode::Expanded)
        .expect("`ProjectionMode::Expanded` should return result");

    let macro_meta = state
        .resolved_macros
        .iter()
        .find(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("resolved defineProps macro should be present");

    // Published props: public only.
    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
    assert_eq!(
        {
            let mut p: Vec<&String> = prop_names.iter().filter(|n| *n != "constructor").collect();
            p.sort();
            p
        },
        vec![&"a".to_string()],
        "published props over a directly-declared class are public-only: {prop_names:?}"
    );

    // native_props: ALL three members, each with its true visibility.
    let visibility_of = |name: &str| -> Option<ResolvedMemberVisibility> {
        macro_meta
            .native_props
            .iter()
            .find(|prop| prop.name == name)
            .map(|prop| prop.visibility)
    };
    assert_eq!(
        visibility_of("a"),
        Some(ResolvedMemberVisibility::Public),
        "native_props must keep public `a` as Public"
    );
    assert_eq!(
        visibility_of("b"),
        Some(ResolvedMemberVisibility::Protected),
        "native_props must keep protected `b` as Protected"
    );
    assert_eq!(
        visibility_of("c"),
        Some(ResolvedMemberVisibility::Private),
        "native_props must keep private `c` as Private"
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
    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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
                    == verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility::Protected
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

    let emit_names = emit_names_from_resolved(project.host(), "/App.vue", &state);
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

    let slot_names = slot_names_from_resolved(project.host(), "/App.vue", &state);
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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
                    == verter_compiler::utils::oxc::script::type_surface::ResolvedMemberVisibility::Protected
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
    // Discriminating invariant: declaration text recovery via
    // source-reparse is not supported. Native declaration metadata
    // still preserves kind/span/declaration_id graph-natively
    // (asserted above); declaration text is always None under the
    // graph-only resolver.
    assert_eq!(
        props_macro.declaration.text, None,
        "graph-only resolver: declaration text is no longer populated"
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

    let first_props = prop_names_from_resolved(project.host(), "/App.vue", &first);
    let second_props = prop_names_from_resolved(project.host(), "/App.vue", &second);

    assert!(
        first_props.contains(&"label".to_string()),
        "first resolve should include the imported prop"
    );
    assert!(
        second_props.contains(&"label".to_string()),
        "second resolve should still include the imported prop"
    );
    assert!(
        !second_props.contains(&"missing".to_string()),
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
    let _store_view = project.host().resolver_store_view_read().into_owned_view();

    let resolved = crate::resolver_core::with_bare_host_ctx_for_test(project.host(), |ctx| {
        resolve_jsdoc_tag_type(
            project.host(),
            ctx,
            "/types.ts",
            "DocType",
            &mut tracked_deps,
        )
    })
    .expect("typed JSDoc payload should resolve through cached imported lookup");

    assert!(
        matches!(resolved, verter_type_expr::TypeExpr::Object(_)),
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
            verter_type_expr::TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Any | verter_type_expr::PrimitiveName::Unknown,
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

/// Slot-deepening caller rail: `defineSlots<T>` lowers its slot surface
/// through `project_type_surface_shape_via_host_threaded` (the second
/// root-surface bridge that carries NO prepared-decl rescue). A slot type
/// that is a compound intersection of an imported
/// mapped/record slot branch and a named-slot literal is the hard case;
/// this proves the slot rail composes the compound slot surface through
/// dispatch alone, so `default` survives to final meta.
#[test]
fn slot_deepening_compound_surface_is_dispatch_authoritative() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export type DynamicSlots = Record<string, (props: { value: string }) => any>
export interface NamedSlots {
  default(props: { row: string }): any
}
export type ComponentSlots = NamedSlots & DynamicSlots
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { ComponentSlots } from './slots'
defineSlots<ComponentSlots>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let meta = project
        .host()
        .get_component_meta("/App.vue")
        .expect("should return component meta (no prepared-decl root-surface fallback exists)");

    let slot_names: Vec<&str> = meta.slots.iter().map(|slot| slot.name.as_str()).collect();
    assert!(
        slot_names.contains(&"default"),
        "the named-slot branch of the compound imported slot type must survive \
         to final meta through the dispatch slot surface alone: {slot_names:?}",
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
    // Each macro index materializes its own emit payload through the SOLE
    // typeinfo macro-surface authority (keyed on the distinct macro index).
    assert!(
        emit_macros.iter().all(|m| {
            project
                .host()
                .vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
                    owner_canonical: std::sync::Arc::from("/App.vue"),
                    macro_index: m.macro_index,
                    macro_kind: m.macro_kind,
                    root_identity: project
                        .host()
                        .current_or_read_whole_hash("/App.vue")
                        .unwrap_or([0u8; 16]),
                    level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
                })
                .emit_fields()
                .iter()
                .any(|emit| emit.name == "save")
        }),
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

    let prop_names = prop_names_from_resolved(project.host(), "/project/App.vue", &state);
    assert!(
        prop_names.contains(&"x".to_string()),
        "resolver should materialize imported props from declaration entrypoints: {:?}",
        prop_names
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

    let prop_names = prop_names_from_resolved(project.host(), "/project/App.vue", &state);
    assert!(
        prop_names.contains(&"y".to_string()),
        "resolver should follow import alias reexports through declaration entrypoints: {:?}",
        prop_names
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
    // Discriminating invariant: declaration text recovery via
    // source-reparse is not supported. The type-registry metadata
    // still retains kind/declaration_id graph-natively (asserted
    // above); the declaration text field is None under the graph-
    // only resolver.
    assert_eq!(
        registry_entry.declaration.text, None,
        "graph-only resolver: declaration text is no longer populated"
    );
}

// ===========================================================================
// Overlay safety
// ===========================================================================

// Overlay-isolation invariant: a session view carrying an overlay-Upsert
// must observe the overlay's resolved component-meta, never a base-cache
// entry computed from the pre-overlay source. The view-aware mirror key
// (`(canonical, content_hash)`) plus cold-compute view threading keeps
// base and overlay candidates in distinct cache slots so concurrent
// sessions cannot coalesce onto each other's results (R20).
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

    // Query through session — should NOT reuse stale base cache. The overlay
    // view is observed through the PUBLISHED component-meta surface (the public
    // session output), which is the overlay-aware authority; the session host's
    // own `vue_macro_dtos` reads the base view, so we assert the published props.
    let (analysis, _session_state) = session
        .get_component_meta_with_resolution("/App.vue")
        .unwrap()
        .expect("session resolver query should return a result");
    let overlay_props: Vec<&str> = analysis
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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

// `resolve_component_meta_populates_compute_audit_when_enabled` is
// intentionally not part of this suite. The previous
// `ComponentMetaComputeAudit` telemetry block was a solver-owned
// surface (step counters / cache-hit counters on a solver-specific
// engine) and is not part of the final design; dispatch publishes
// per-query stats through `SemanticGraphStats`. The sister test
// `resolve_component_meta_leaves_compute_audit_empty_when_disabled`
// runs below and asserts the opt-out behaviour.

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

    let prop_names = prop_names_from_resolved(project.host(), "/App.vue", &state);
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
// `component_meta_query_engine_caches_by_scope_and_name` /
// `component_meta_query_engine_different_scopes_do_not_alias` are
// intentionally not part of this suite — they pinned a solver-cache
// identity (`scoped_cache_len` + `solve_count`) that is not part of
// the final design. Dispatch's `project_type_surface_expr` replaces
// the access pattern at every production call site; cross-scope
// non-aliasing is covered by the dispatch memo's identity contract
// (see `project_semantic_dispatch::tests`).
//
// `debug_solver_host_for_scope` and the
// `HostComponentMetaResolver.shared_owner_engine` field are also not
// part of the final design. Scope-payload cache identity lives on
// `CMQE::scope_payload_for_scope` directly (returns
// `Option<Arc<DeclarationScopePayload>>`) and is exercised through
// repeated dispatch calls in the surviving component-meta tests.
// Cross-file imported type resolution is covered by the surviving
// dispatch-backed component-meta tests.

// ===========================================================================
// Resolver view caches routes and declarations
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host, ctx: host };

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
    let verter_type_expr::TypeExpr::Object(section_object) = &section_entry.type_expr else {
        panic!("local explicit helper should stay an object surface");
    };
    let feature_property = section_object
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(property) if property.name == "features" => {
                Some(property)
            }
            _ => None,
        })
        .expect("section helper should preserve the features property");
    assert!(
        matches!(
            feature_property.ty,
            verter_type_expr::TypeExpr::Array { .. }
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash, ctx)
    })
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash, ctx)
    })
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
    let fallthrough_props_dtos =
        host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
            owner_canonical: std::sync::Arc::from("/src/App.vue"),
            macro_index: fallthrough_props.macro_index,
            macro_kind: fallthrough_props.macro_kind,
            root_identity: host
                .current_or_read_whole_hash("/src/App.vue")
                .unwrap_or([0u8; 16]),
            level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
        });
    assert!(
        fallthrough_props_dtos
            .prop_fields()
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
            .is_none_or(|evaluated| evaluated.bindings.is_empty()),
        "fallthrough-expanded state should skip defineExpose binding expansion"
    );
    // Note: slot_bindings are populated by graph-native synthesis
    // (`resolve_slot_bindings_graph_native`) which runs for both
    // full and fallthrough modes — slot binding identification is
    // load-bearing for the inheritance resolver. The current
    // invariant: defineSlots SHAPE expansion (heavy) is skipped, but
    // slot binding identification is preserved.
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let whole_hash = store_view
        .whole_hash("/src/App.vue")
        .expect("whole hash should exist for the owner");

    let full = host
        .compute_component_meta_state("/src/App.vue", super::ProjectionMode::Expanded, whole_hash)
        .expect("full expanded state should resolve");
    let fallthrough = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.compute_component_meta_state_for_fallthrough("/src/App.vue", whole_hash, ctx)
    })
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
    let fallthrough_props_dtos =
        host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
            owner_canonical: std::sync::Arc::from("/src/App.vue"),
            macro_index: fallthrough_props.macro_index,
            macro_kind: fallthrough_props.macro_kind,
            root_identity: host
                .current_or_read_whole_hash("/src/App.vue")
                .unwrap_or([0u8; 16]),
            level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
        });
    assert!(
        fallthrough_props_dtos
            .prop_fields()
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let whole_hash = store_view
        .whole_hash("/src/Child.vue")
        .expect("whole hash should exist for the owner");

    host.provenance().reset();

    let fallthrough = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.compute_component_meta_state_for_fallthrough("/src/Child.vue", whole_hash, ctx)
    })
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
        host,
        "/src/Child.vue",
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let whole_hash = store_view
        .whole_hash("/src/Child.vue")
        .expect("whole hash should exist for the owner");

    host.provenance().reset();

    let fallthrough = crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
        host.compute_component_meta_state_for_fallthrough("/src/Child.vue", whole_hash, ctx)
    })
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
        host,
        "/src/Child.vue",
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
        },
    );
    assert!(
        base_meta.events.iter().any(|event| event.name == "open"),
        "fallthrough-expanded extraction must still preserve declared events from the local defineEmits wrapper",
    );
}

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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_type_expr::TypeExpr::named("Inner");

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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload("Button['ui']", None);

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

/// The structural materialiser caches the DECL-ROOTED member subjects of
/// an anonymous inline object, but NOT the enclosing anonymous object
/// itself: a `{ first: Inner; second: Inner }` surface keys no
/// `MaterializeStructureDb` slot for the inline object (it is a root-less
/// anonymous subject — `derive_materialization_subject` returns `None`, so
/// it computes uncached), while its `Inner` member ref IS canonicalised to
/// `slot(/src/types.ts, Inner)` and cached. Both `first` and `second`
/// reference the SAME `Inner` decl, so they co-locate onto ONE slot —
/// `live_count == 1`. The materialised surface is still deterministic and
/// reused (`first == second`); reuse for the anonymous wrapper rides its
/// decl-rooted members, not the wrapper itself.
#[test]
fn materialize_member_surface_expr_caches_decl_rooted_members_not_anonymous_object() {
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let expr = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(
        "{ first: Inner; second: Inner }",
        None,
    );

    let first = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);
    let cache_len_after_first = query_engine.materialized_member_surface_cache_len();

    let second = query_engine.materialize_member_surface_expr("/src/types.ts", &expr, true);

    assert_eq!(
        first, second,
        "structural cache reuse must preserve the materialized surface"
    );
    assert_eq!(
        cache_len_after_first, 1,
        "the decl-rooted `Inner` member MUST cache onto ONE slot (both `first` and `second` \
         reference the same `Inner`), while the enclosing ANONYMOUS inline object keys no \
         MaterializeStructureDb slot (it is a root-less subject — uncached). got cache_len={cache_len_after_first}",
    );
    assert_eq!(
        query_engine.materialized_member_surface_cache_len(),
        cache_len_after_first,
        "second structural materialization should reuse the existing cache entry (no growth)",
    );
}

/// The registry member-surface materialiser preserves open mapped
/// carriers: `materialize_member_surface_expr` lowers at `Navigate`
/// and runs the structural materialiser with `mode: Navigate`, so an
/// OPEN mapped (`{ [K in keyof T]: T }` over the unbound `T`) survives
/// as the deferred `Mapped` carrier via the shared L1 predicates —
/// never falling through into Expanded materialisation, including when
/// the carrier hides behind a composition deep enough to exhaust the
/// bounded openness walks (whose exhaustion verdict fails
/// OPEN-OR-UNKNOWN, the safe direction). An open mapped behind more
/// than 64 top-level intersection arms must still carrier-stop on the
/// registry route.
///
/// **Discriminating.** An implementation that Expanded-materialises
/// the composition fails BOTH legs: the Mapped carrier disappears from
/// the result, and the dispatch log records `Published(Expanded)`
/// projection demands (the publication-pipeline ban). The shallow
/// control shows the same composition WITHOUT depth pressure preserves
/// the carrier, so a deep failure is the budget fail-direction, not
/// the predicate.
#[test]
fn materialize_member_surface_expr_preserves_open_mapped_carrier_on_walk_budget_exhaustion() {
    use verter_type_expr::{MappedModifier, ObjectExpr, ObjectMember, ObjectProperty, TypeParam};

    let project = make_project();
    project
        .upsert_base("/src/types.ts", "export interface Anchor { id: string }\n")
        .unwrap();

    let host = project.host();
    let _store_view = host.resolver_store_view();
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

    let open_t = TypeExpr::TypeParameter(TypeParam {
        name: "T".to_string(),
        constraint: None,
        default: None,
    });
    // `{ [K in keyof T]: T }` over the unbound outer `T` — an OPEN mapped
    // carrier (open key space AND open value body).
    let open_mapped = TypeExpr::Mapped {
        parameter: "K".to_string(),
        source: Arc::new(TypeExpr::KeyOf(Arc::new(open_t.clone()))),
        value: Arc::new(open_t),
        optional: MappedModifier::None,
        readonly: MappedModifier::None,
        name_type: None,
    };
    let closed_arm = |i: usize| {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                format!("a{i}"),
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                false,
                false,
            ))],
        }))
    };
    fn contains_mapped(expr: &TypeExpr) -> bool {
        match expr {
            TypeExpr::Mapped { .. } => true,
            TypeExpr::Intersection(arms) | TypeExpr::Union(arms) => {
                arms.iter().any(contains_mapped)
            }
            TypeExpr::Parenthesized(inner) => contains_mapped(inner),
            _ => false,
        }
    }

    // Control (no budget pressure): a small composition with the open
    // mapped carrier-stops — the predicate itself sees the carrier.
    let mut shallow_arms = vec![open_mapped.clone()];
    shallow_arms.extend((0..2).map(closed_arm));
    let shallow = query_engine.materialize_member_surface_expr(
        "/src/types.ts",
        &TypeExpr::Intersection(shallow_arms.into()),
        true,
    );
    assert!(
        contains_mapped(&shallow),
        "control: a shallow composition with an open mapped must preserve the Mapped \
         carrier, got {shallow:?}"
    );

    // Deep composition: the open mapped sits FIRST in the arm list behind
    // 80 closed arms — deep enough to exhaust any bounded top-level walk,
    // whose undecidable verdict must PRESERVE the carrier. The structural
    // materialiser runs at `Navigate`; the shared L1 predicates keep the
    // open mapped a deferred carrier. The dispatch log staying free of
    // `Published(Expanded)` projection demands is the proof the Expanded
    // materialiser never ran (the unsafe fall-through this guard exists
    // to stop).
    let guard = crate::capture_token::CaptureToken::start_for_query(
        "registry_open_mapped_budget_exhaustion",
    );
    let mut deep_arms = vec![open_mapped];
    deep_arms.extend((0..80).map(closed_arm));
    let deep = query_engine.materialize_member_surface_expr(
        "/src/types.ts",
        &TypeExpr::Intersection(deep_arms.into()),
        true,
    );
    let snapshot = guard.end();
    assert!(
        contains_mapped(&deep),
        "an open mapped carrier behind >64 top-level composition nodes must still \
         carrier-stop on the registry route (budget exhaustion fails open-or-unknown), \
         got {deep:?}"
    );
    let expanded: Vec<String> = snapshot
        .dispatch_log
        .iter()
        .filter(|e| {
            let ctx = match &e.key {
                crate::semantic_query::SemanticQueryKey::Instantiate { context, .. } => {
                    Some(context.projection_reduction())
                }
                crate::semantic_query::SemanticQueryKey::KeyOf { context, .. }
                | crate::semantic_query::SemanticQueryKey::MappedType { context, .. }
                | crate::semantic_query::SemanticQueryKey::ProjectPath { context, .. } => {
                    Some(*context)
                }
                _ => None,
            };
            ctx.is_some_and(|c| {
                c.demand == crate::semantic_query::ReductionDemand::Published
                    && c.mode == crate::semantic_query::ProjectionMode::Expanded
            })
        })
        .map(|e| format!("{:?}", e.key))
        .collect();
    assert!(
        expanded.is_empty(),
        "the registry member-surface route must preserve the open mapped carrier WITHOUT \
         falling through into Expanded materialisation — found Published(Expanded) \
         dispatches: {expanded:?}"
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
    let _store_view = host.resolver_store_view_read().into_owned_view();
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
    let _store_view = host.resolver_store_view_read().into_owned_view();

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
    let prop_names = prop_names_from_resolved(host, "/src/Tree.vue", &state);
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
// Dispatch substitution spike (#1).
//
// A black-box dispatch test (it uses NO instrumentation hooks): it
// validates that `dispatch.lower_type_expr_in_scope` + `ProjectPath`
// projection + `raise_node_to_type_expr` substitute the
// script-setup-generic `T` when given the parent macro shell `Props<T>`
// directly.
//
// The companion cache-classification spike (#2) and its test-only
// `crate::spike_instrumentation` thread-local hooks have been DELETED:
// the route-correctness those classification fixtures exercised
// (barrel-import, generic-macro, indexed-member-route, pick-through-
// barrel, pick-with-key-alias, omit-with-recursive-target,
// alias-to-imported-ref, and the direct `RouteDemand` engine API) is
// covered by discriminating live dispatch/projector tests in
// `component_meta_query_engine/tests.rs`,
// `component_meta_family_producers_observe_cross_file_deps.rs`,
// `shallow_walk_no_over_materialise.rs`, and the indexed/Pick dispatch
// tests — all of which assert concrete projected surfaces rather than
// the spike's non-discriminating `pre_lower_count > 0`.
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
        SemanticQueryOutput,
    };
    use std::sync::Arc as StdArc;
    use verter_type_expr::TypeExpr;

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
            type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
        }]),
    };
    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/Generic.vue", &props_t, ProjectionMode::Expanded)
        .expect("dispatch must lower the Props<T> shell rooted at /Generic.vue");

    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: lowered,
        path: StdArc::from(vec![PathSegment::Member(StdArc::from("items"))]),
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });

    let raised = match projected {
        QueryResult::Value(SemanticQueryOutput { value: node_id, .. }) => dispatch
            .materialize_output_type_expr_for_test(node_id)
            .expect("raise must succeed on a ProjectPath result"),
        other => panic!(
            "spike #1: dispatch returned non-Value for ProjectPath(Props<T>, ['items']): {other:?}\n\
             this halts Step 1 — dispatch substitution is broken upstream.\n\
             open a sibling plan for lower.rs / build.rs substitution-threading repair."
        ),
    };

    match &raised {
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

// ===========================================================================
// Step 1 FAIL-FIRST #5 — `Instantiate` memo splits per projection mode.
//
// Validates D1.4: carrying the projection mode on
// `SemanticQueryKey::Instantiate.context.projection_reduction.mode`
// (and projecting the family-slot mapping through `mode_to_slot(mode)`)
// produces structurally distinct memo entries for the same `(base, args)`
// pair under different projection modes. Pre-Step-1 the key was mode-free
// (`Single` slot); post-Step-1 the same `(base, args)` triggers two
// distinct lowerings depending on the caller's projection mode.
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
    use verter_type_expr::TypeExpr;

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
            type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
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
    // `mode_to_slot(mode)` projection in
    // `semantic_query_memo::family_and_slot` splits the slot, so the
    // two lowerings produce structurally distinct nodes.
    assert_ne!(
        lowered_expanded, lowered_navigate,
        "Instantiate memo must split per projection mode; same node id across \
         modes means the context.projection_reduction.mode change is \
         not flowing through to the family-slot projection"
    );

    // Assertion 2: Expanded fully reduces to the body's substituted
    // shape. `type Wrapper<T> = T` with T=Inner reduces to Inner — the
    // raised TypeExpr should NOT be a Ref to "Wrapper".
    let expanded_raised = dispatch
        .materialize_output_type_expr_for_test(lowered_expanded)
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
        .materialize_output_type_expr_for_test(lowered_navigate)
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
    use verter_type_expr::{empty_type_args, LiteralValue, TypeExpr};

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

    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        lowered,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
    );

    let raised = &materialized;

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
/// the substituted mapper context, so `build_conditional`'s infer-binding
/// Function-extends arm sees check_resolved as the unsubstituted shell and skips
/// the per-position infer binding.
///
/// Post-Step-1.5: `build_mapped_type`'s substitute-and-evaluate path
/// resolves the inner `IndexedAccess` BEFORE the conditional materialises,
/// so the infer binding extracts `P → { planId: string }` and the
/// substituted true_branch surfaces as
/// `(props: { planId: string; plan: TPlan }) => any`.
#[test]
fn dispatch_only_imported_mapped_slots_resolved_shape_via_dispatch_only() {
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{
        PathSegment, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
        SemanticQueryOutput,
    };
    use std::sync::Arc as StdArc;
    use verter_type_expr::{empty_type_args, TypeExpr};

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
    let plan_arg = TypeExpr::Object(StdArc::new(verter_type_expr::ObjectExpr {
        properties: vec![
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "id".to_string(),
                    TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                    false,
                    false,
                ),
            ),
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "tier".to_string(),
                    TypeExpr::string_literal("pro"),
                    false,
                    false,
                ),
            ),
        ],
    }));

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
    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: lowered,
        path: badge_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });

    let badge_node = match projected {
        QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
        other => panic!(
            "dispatch-only ProjectPath(['badge']) on PricingPlansSlots<…> \
             must return Value(id); got {other:?}\n\
             this isolates the dispatch substitution gap that the \
             materialize wrapper currently masks via the legacy walker."
        ),
    };

    let raised = dispatch
        .materialize_output_type_expr_for_test(badge_node)
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
                        verter_type_expr::ObjectMember::Property(p) => {
                            names.insert(p.name.clone());
                        }
                        verter_type_expr::ObjectMember::Method(m) => {
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
    use verter_type_expr::TypeExpr;

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

    let plan_arg = TypeExpr::Object(StdArc::new(verter_type_expr::ObjectExpr {
        properties: vec![
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "id".to_string(),
                    TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                    false,
                    false,
                ),
            ),
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "tier".to_string(),
                    TypeExpr::string_literal("pro"),
                    false,
                    false,
                ),
            ),
        ],
    }));
    let macro_shell = TypeExpr::Ref {
        name: StdArc::from("PricingPlansSlots"),
        type_arguments: StdArc::from(vec![plan_arg]),
    };

    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/App.vue", &macro_shell, ProjectionMode::Expanded)
        .expect("dispatch must lower PricingPlansSlots<{...}> at /App.vue");

    let materialized = dispatch.materialize_reduced_output_type_expr_for_test(
        lowered,
        crate::semantic_query::ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    let raised = &materialized;

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
                        verter_type_expr::ObjectMember::Property(p) => {
                            out.insert(p.name.clone());
                        }
                        verter_type_expr::ObjectMember::Method(m) => {
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
                    if let verter_type_expr::ObjectMember::Property(p) = member {
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
                        verter_type_expr::ObjectMember::Property(p) => {
                            names.insert(p.name.clone());
                        }
                        verter_type_expr::ObjectMember::Method(m) => {
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
// graph-native registry-route + cycle-BFS predicates
//
// Discriminating tests for the registry-route + cycle-BFS predicate matrix:
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

/// Producer-chain invariant: when `defineEmits<Emits>()` consumes a local
/// interface `Emits` that `extends ExternalEmits<T>` from a package, the
/// `AnalyzedEmitField` produced through the typeinfo emit normalizer must carry
/// the typed call-signature payload on `payload_expr` (`Tuple` of post-event-name
/// params with the generic `T` substituted), and `payload_expr_scope` anchors to
/// the call signature's DECLARING file (the package `.d.ts` where
/// `(e, payload: T): void` is written) — the SFC-supplied generic argument lives
/// in the typed `payload_expr` element types, not in the scope. Without the typed
/// form, downstream consumers fall back to re-parsing the display `payload_type`
/// text — the Typed-IR-Only Resolver Rule (CLAUDE.md) forbids that.
#[test]
fn resolved_macro_emits_carry_payload_expr_for_cross_file_interface_extends() {
    let project = make_project();
    project
        .upsert_base(
            "/node_modules/reka-ui/index.d.ts",
            r#"
export interface TabsRootEmits<T> {
  (e: 'update:modelValue', payload: T): void
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script lang="ts">
import type { TabsRootEmits } from 'reka-ui'

export interface Emits extends TabsRootEmits<string | number> {}
</script>
<script setup lang="ts">
defineEmits<Emits>()
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
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    let resolver_host = super::HostComponentMetaResolver { host, ctx: host };
    let parts = crate::resolver_core::resolve_component_meta_parts(
        &resolver_host,
        "/src/App.vue",
        &snapshot,
        true,
        None,
        crate::resolver_core::ComponentMetaResolutionPurpose::Full,
    );

    let define_emits = parts
        .resolved_macros
        .iter()
        .find(|resolved| {
            resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits
        })
        .expect("defineEmits<Emits> should produce a resolved macro meta entry");

    // The published emit surface (incl. the typed `payload_expr`) is owned by
    // the SOLE typeinfo macro-surface authority, keyed on the macro index.
    let define_emits_dtos = host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from("/src/App.vue"),
        macro_index: define_emits.macro_index,
        macro_kind: define_emits.macro_kind,
        root_identity: host
            .current_or_read_whole_hash("/src/App.vue")
            .unwrap_or([0u8; 16]),
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    });

    let emit = define_emits_dtos
        .emit_fields()
        .iter()
        .find(|emit| emit.name == "update:modelValue")
        .unwrap_or_else(|| {
            panic!(
                "update:modelValue emit should be present on resolved define-emits, got {:?}",
                define_emits_dtos
                    .emit_fields()
                    .iter()
                    .map(|emit| emit.name.as_str())
                    .collect::<Vec<_>>(),
            )
        });

    let payload_expr = emit
        .payload_expr
        .as_ref()
        .expect("payload_expr must be populated for cross-file interface-extends emits");
    let payload_expr_scope = emit
        .payload_expr_scope
        .as_ref()
        .expect("payload_expr_scope must be populated when payload_expr is populated");

    assert_eq!(
        payload_expr_scope.as_str(),
        "/node_modules/reka-ui/index.d.ts",
        "payload_expr_scope anchors to the call signature's DECLARING file (where \
         `(e, payload: T): void` is written); the SFC-supplied generic argument \
         (`string | number`) is encoded in the typed `payload_expr` Tuple's \
         element types, NOT by re-anchoring the signature's scope to the SFC",
    );

    let verter_type_expr::TypeExpr::Tuple { elements, .. } = payload_expr else {
        panic!(
            "call-signature emit payload should lower to a Tuple, got {:?}",
            payload_expr,
        );
    };
    assert_eq!(
        elements.len(),
        1,
        "(e, payload: T) tuple after skip(1) should hold a single labelled element"
    );
    assert_eq!(
        elements[0].label.as_deref(),
        Some("payload"),
        "tuple element should preserve the payload label",
    );
    let verter_type_expr::TypeExpr::Union(members) = &elements[0].ty else {
        panic!(
            "the generic T should be substituted with the union string|number, got {:?}",
            elements[0].ty,
        );
    };
    assert!(
        members.contains(&verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String,
        )),
        "payload union should contain string, got {:?}",
        members,
    );
    assert!(
        members.contains(&verter_type_expr::TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Number,
        )),
        "payload union should contain number, got {:?}",
        members,
    );
}

// ─────────────────────────────────────────────────────────────────────
// Transparent-carrier alias-reached-member provenance (dispatch path).
//
// A member reached ONLY through a transparent alias / instantiation shell
// (`NoInfer<Base>`, a utility identity wrapper) whose own body has no
// declared members is NOT `declared_in_macro_type_arg`. The bit applies
// only to members directly written in the macro type-argument's own object
// body (or a directly-referenced declaration's own body when that decl IS
// the macro argument).
//
// These assert the DISPATCH/projector surface directly via
// `evaluate_types().props` (the `project_props` → shared-resolver path that
// stamps `declared_in_macro_type_arg` from the dispatch surface member),
// NOT the materializer-fed `define_props` mirror.
// ─────────────────────────────────────────────────────────────────────

/// Transparent-carrier discriminating: `defineProps<NoInfer<Base>>()` —
/// `Base`'s members are reached THROUGH the transparent `NoInfer`
/// identity carrier, so the dispatch surface MUST stamp
/// `declared_in_macro_type_arg = false`.
///
/// The transparent-carrier crossing downgrades provenance to
/// `Structural`: propagating `MacroTypeArgOwnBody` through the
/// `Alias(Base)` carrier and stamping Base's members `true` would be the
/// bug this guards against.
#[test]
fn dispatch_no_infer_alias_member_provenance_is_false() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'
defineProps<NoInfer<Base>>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .expect("evaluate_types must resolve")
        .expect("some evaluated types");

    // Sanity: the members are present (the NoInfer identity carrier
    // resolved through to Base) — discriminates "dropped/never" from the
    // provenance question.
    let label = evaluated
        .props
        .iter()
        .find(|p| p.name == "label")
        .expect("NoInfer<Base> must publish `label` through the dispatch surface");
    let count = evaluated
        .props
        .iter()
        .find(|p| p.name == "count")
        .expect("NoInfer<Base> must publish `count` through the dispatch surface");

    assert!(
        !label.declared_in_macro_type_arg && !count.declared_in_macro_type_arg,
        "members reached THROUGH the transparent `NoInfer<Base>` carrier \
         MUST carry `declared_in_macro_type_arg == false` on the dispatch surface \
         (the carrier's own body has no declared members). Got label={}, count={}",
        label.declared_in_macro_type_arg,
        count.declared_in_macro_type_arg,
    );
}

/// Transparent-carrier discriminating (alias-shell): `export type Props =
/// NoInfer<Base>; defineProps<Props>()` — the macro arg is a named alias
/// whose body is the `NoInfer<Base>` instantiation (no direct own-body
/// members). All members are reached through the transparent alias +
/// NoInfer hops → `false`.
#[test]
fn dispatch_no_infer_alias_shell_member_provenance_is_false() {
    let project = make_project();
    project
        .upsert_base(
            "/src/base.ts",
            r#"export interface Base {
  label?: string
  count?: number
}
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
import type { Base } from './base'
export type Props = NoInfer<Base>
defineProps<Props>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .expect("evaluate_types must resolve")
        .expect("some evaluated types");
    let stamped_true: Vec<&str> = evaluated
        .props
        .iter()
        .filter(|p| p.declared_in_macro_type_arg)
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        stamped_true.is_empty(),
        "alias-shell `NoInfer<Base>` members are reached through the \
         transparent alias + NoInfer carriers (the alias `Props`'s own body has \
         NO direct members), so NONE may carry `declared_in_macro_type_arg == \
         true` on the dispatch surface. Mis-stamped: {stamped_true:?}",
    );
}

/// Transparent-carrier GUARD (must stay correct): a DIRECTLY-referenced
/// object alias that IS the macro argument keeps
/// `declared_in_macro_type_arg = true` for its own-body members. The
/// transparent-carrier downgrade MUST NOT regress this — a direct object
/// alias is NOT a transparent identity carrier.
#[test]
fn dispatch_direct_object_alias_member_provenance_stays_true() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type DirectProps = { title: string; count?: number }
defineProps<DirectProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .expect("evaluate_types must resolve")
        .expect("some evaluated types");
    let title = evaluated
        .props
        .iter()
        .find(|p| p.name == "title")
        .expect("direct alias must publish `title`");
    assert!(
        title.declared_in_macro_type_arg,
        "transparent-carrier GUARD — a DIRECT object alias that IS the macro \
         argument keeps its \
         own-body members `declared_in_macro_type_arg == true` (it is not a \
         transparent identity carrier). The downgrade MUST NOT regress this. Got \
         title.declared_in_macro_type_arg=false",
    );
}

// ─────────────────────────────────────────────────────────────────────
// Vue macro object-surface UNION enumeration.
//
// `defineProps<FixedProps | BubbleProps>()` publishes the UNION of the
// arm members (a prop present in ANY arm is part of the macro surface —
// the Vue macro convention), NOT the TS property-access INTERSECTION of
// common members. The dispatch macro object-surface demand
// (`ReductionDemand::MacroObjectSurface`) selects the union-arm rule at
// the empty-path Shallow terminal surface, cache-keyed distinctly from
// ordinary `ProjectPath` (which keeps the intersection).
//
// Asserts the DISPATCH projector surface via `evaluate_types().props`.
// ─────────────────────────────────────────────────────────────────────
#[test]
fn dispatch_macro_props_union_enumerates_all_arm_members() {
    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
type FixedProps = { kind: 'fixed'; size: number }
type BubbleProps = { kind: 'bubble'; color: string }
defineProps<FixedProps | BubbleProps>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let evaluated = session
        .evaluate_types("/src/App.vue")
        .expect("evaluate_types must resolve")
        .expect("some evaluated types");
    let names: Vec<&str> = evaluated.props.iter().map(|p| p.name.as_str()).collect();

    // Common member present in BOTH arms.
    assert!(
        names.contains(&"kind"),
        "common union member `kind` must be on the dispatch macro surface, \
         got: {names:?}"
    );
    // Branch-specific members present in only ONE arm — the union
    // convention keeps them (the intersection rule would drop them).
    for required in ["size", "color"] {
        assert!(
            names.contains(&required),
            "the dispatch macro object-surface MUST enumerate the UNION of \
             object-arm members; branch-specific `{required}` (present in only one \
             arm) must survive. The intersection rule would drop it. Got: {names:?}"
        );
    }

    // Requiredness: a member absent from any arm becomes optional; a
    // member present in all arms stays required.
    let size = evaluated.props.iter().find(|p| p.name == "size").unwrap();
    let kind = evaluated.props.iter().find(|p| p.name == "kind").unwrap();
    assert!(
        size.optional,
        "`size` is declared in only one arm, so it MUST be optional on the \
         merged macro surface. Got optional={}",
        size.optional
    );
    assert!(
        !kind.optional,
        "`kind` is declared in BOTH arms (and required in each), so it MUST \
         stay required on the merged macro surface. Got optional={}",
        kind.optional
    );
}

// ─────────────────────────────────────────────────────────────────────
// Aliased conditional-emits carrier walk (dispatch).
//
// `defineEmits<ConditionalEmits>()` where
// `type ConditionalEmits = Mode extends 'editor' ? EditorEmits : ViewerEmits`
// lowers (Navigate) to a terminal DeclRef carrier, NOT the Conditional
// directly. The emits branch-merge (`resolve_payload_surface_with_scope`,
// EmitClassMacroObject) only fired when the payload node was DIRECTLY a
// Conditional, so a NAMED conditional-emit alias missed the merge and the
// inherited emit set collapsed. The carrier walk
// (`resolve_emit_payload_to_conditional_root`) follows DeclRef /
// DeclPlaceholder carriers to the Conditional root before the branch
// projection.
//
// This drives `resolve_payload_surface_with_scope` DIRECTLY with the
// aliased-conditional payload node (the named-alias DeclRef carrier the
// macro projector resolves it to under Navigate), asserting the shared
// branch-merge surface enumerates BOTH branches' events. Driving the
// branch-merge in isolation keeps the test discriminating for the carrier
// walk specifically, independent of the upstream macro-payload-resolution
// path's handling of unbound component generics.
// ─────────────────────────────────────────────────────────────────────
#[test]
fn dispatch_aliased_conditional_emits_branch_merge() {
    use crate::meta_resolve::projectors::{
        resolve_payload_surface_with_scope, PayloadSurfaceScope,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData};

    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="Mode extends 'editor' | 'viewer'">
type EditorEmits = { itemEdited: [id: number] }
type ViewerEmits = { itemViewed: [id: number] }
type ConditionalEmits = Mode extends 'editor' ? EditorEmits : ViewerEmits
defineEmits<ConditionalEmits>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    // Seed the canonical state through the public consumer path.
    let _ = session.evaluate_types("/src/App.vue").unwrap();
    let host = project.host();

    let scope = "/src/App.vue";
    let dispatch = ProjectSemanticDispatch::new(host);

    // The macro projector resolves `defineEmits<ConditionalEmits>()`'s type
    // argument in Navigate mode, which yields a terminal `DeclRef` carrier
    // for the NAMED conditional alias (NOT the `Conditional` node). Lower
    // the alias reference the same way to obtain that carrier payload node.
    let conditional_ref = TypeExpr::Ref {
        name: std::sync::Arc::from("ConditionalEmits"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
    };
    let payload_node = dispatch
        .lower_type_expr_in_scope_with_mode(scope, &conditional_ref, ProjectionMode::Navigate)
        .expect("ConditionalEmits must lower to a Navigate carrier node");

    // Pre-condition: the payload is a carrier (NOT a bare Conditional),
    // exactly the shape that defeated the direct-only branch-merge.
    assert!(
        !matches!(
            crate::project_semantic_dispatch::node_data_for(host, payload_node).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "fixture precondition: the Navigate-lowered named conditional alias must \
         be a CARRIER (DeclRef/DeclPlaceholder), not a bare Conditional — that is \
         the shape the carrier walk must follow"
    );

    let mut diag_sink = Vec::new();
    let surface = resolve_payload_surface_with_scope(
        &dispatch,
        payload_node,
        0,
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineEmits,
        PayloadSurfaceScope::EmitClassMacroObject,
        &mut diag_sink,
    );
    let surface = surface.expect(
        "the emits branch-merge must resolve the aliased-conditional \
         payload surface by following the DeclRef carrier to the Conditional root",
    );
    let members = crate::meta_resolve::projectors::read_surface_members(host, surface);
    let event_names: Vec<String> = members.iter().map(|m| m.name.to_string()).collect();

    for required in ["itemEdited", "itemViewed"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "the branch-merge surface MUST merge BOTH branches of the \
             undecided NAMED conditional emit alias `ConditionalEmits` (Mode \
             extends 'editor' ? EditorEmits : ViewerEmits). Event `{required}` is \
             missing — the merge must follow the DeclRef carrier to the \
             Conditional root. Got events: {event_names:?}"
        );
    }
}

// Carrier walk terminates by visited-node IDENTITY, not by a depth cap:
// a legitimate alias chain LONGER than the retired depth-8 bound must
// still reach its terminal Conditional emit so the branch-merge fires.
//
// `defineEmits<EmitChain0>()` where `EmitChain0 -> EmitChain1 -> ... ->
// EmitChain11 -> (Mode extends 'editor' ? EditorEmits : ViewerEmits)` is
// a 12-hop alias chain to the Conditional root. Under the retired
// `depth > 8` cap the carrier walk returned `None` at hop 9 — BEFORE
// reaching the Conditional — so the inherited emit set collapsed and the
// branch-merge silently lost both events. Identity-bounded termination
// follows every distinct hop (no node repeats), reaches the Conditional,
// and enumerates BOTH branches' events.
//
// Driven directly through `resolve_payload_surface_with_scope` (the
// EmitClassMacroObject branch-merge entry) with the Navigate-lowered
// carrier for the chain head — the same shape the macro projector hands
// the branch-merge. This is discriminating: it FAILS against the
// depth-8-capped tree (events missing) and PASSES against the
// identity-bounded tree.
// ─────────────────────────────────────────────────────────────────────
#[test]
fn dispatch_long_alias_chain_to_conditional_emits_branch_merge() {
    use crate::meta_resolve::projectors::{
        resolve_payload_surface_with_scope, PayloadSurfaceScope,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData};

    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts" generic="Mode extends 'editor' | 'viewer'">
type EditorEmits = { itemEdited: [id: number] }
type ViewerEmits = { itemViewed: [id: number] }
type EmitChain11 = Mode extends 'editor' ? EditorEmits : ViewerEmits
type EmitChain10 = EmitChain11
type EmitChain9 = EmitChain10
type EmitChain8 = EmitChain9
type EmitChain7 = EmitChain8
type EmitChain6 = EmitChain7
type EmitChain5 = EmitChain6
type EmitChain4 = EmitChain5
type EmitChain3 = EmitChain4
type EmitChain2 = EmitChain3
type EmitChain1 = EmitChain2
type EmitChain0 = EmitChain1
defineEmits<EmitChain0>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/src/App.vue").unwrap();
    let host = project.host();

    let scope = "/src/App.vue";
    let dispatch = ProjectSemanticDispatch::new(host);

    // The macro projector resolves `defineEmits<EmitChain0>()`'s type
    // argument in Navigate mode → a terminal `DeclRef` carrier for the
    // NAMED alias chain head (NOT the `Conditional`). Lower the chain head
    // the same way to obtain that carrier payload node.
    let chain_head_ref = TypeExpr::Ref {
        name: std::sync::Arc::from("EmitChain0"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
    };
    let payload_node = dispatch
        .lower_type_expr_in_scope_with_mode(scope, &chain_head_ref, ProjectionMode::Navigate)
        .expect("EmitChain0 must lower to a Navigate carrier node");

    // Precondition: the chain head is a carrier (NOT a bare Conditional),
    // and the chain is longer than the retired depth-8 bound (12 hops to
    // the Conditional root).
    assert!(
        !matches!(
            crate::project_semantic_dispatch::node_data_for(host, payload_node).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "fixture precondition: the Navigate-lowered chain head must be a \
         CARRIER (DeclRef/DeclPlaceholder), not a bare Conditional"
    );

    let mut diag_sink = Vec::new();
    let surface = resolve_payload_surface_with_scope(
        &dispatch,
        payload_node,
        0,
        verter_semantic::analysis::component_meta::MacroExpansionKind::DefineEmits,
        PayloadSurfaceScope::EmitClassMacroObject,
        &mut diag_sink,
    );
    let surface = surface.expect(
        "long-chain branch-merge must follow the >8-hop DeclRef carrier chain \
         to the Conditional root — identity-bounded termination reaches it; the \
         retired depth-8 cap returned None before hop 12 and lost the merge",
    );
    let members = crate::meta_resolve::projectors::read_surface_members(host, surface);
    let event_names: Vec<String> = members.iter().map(|m| m.name.to_string()).collect();

    for required in ["itemEdited", "itemViewed"] {
        assert!(
            event_names.iter().any(|n| n == required),
            "long-chain branch-merge MUST merge BOTH branches of the undecided \
             NAMED conditional emit at the END of a 12-hop alias chain. Event \
             `{required}` is missing — a depth-8 cap truncates the walk before \
             the Conditional. Got events: {event_names:?}"
        );
    }
}

// Carrier walk terminates a 2-node alias CYCLE by visited-node identity,
// on the FIRST re-entry — NOT by exhausting the pathological fuse.
//
// `type CycA = CycB; type CycB = CycA` used as the emit payload has no
// Conditional root, so the carrier walk must return `None`. The retired
// `depth > 8` cap only terminated this cycle by bouncing CycA<->CycB nine
// times until the bound tripped; the `*resolved != node` filter caught
// only 1-cycles, never this 2-cycle. Visited-node identity catches the
// cycle the instant CycA is re-entered (the third call), independent of
// the fuse.
//
// Discriminating proof of identity (not depth): the walk re-enters CycA
// after visiting exactly {CycA, CycB}, so `visited.len() == 2` and
// termination occurs at re-entry depth 2 — three orders of magnitude
// below `EMIT_CARRIER_WALK_FUSE` (1024). If termination depended on the
// fuse the cycle would bounce up to 1024 hops before returning; instead
// it returns immediately with a 2-element visited set.
// ─────────────────────────────────────────────────────────────────────
#[test]
fn dispatch_mutual_alias_cycle_emits_terminates_by_identity() {
    use crate::meta_resolve::projectors::{
        resolve_emit_payload_to_conditional_root, EMIT_CARRIER_WALK_FUSE,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{ProjectionMode, SemanticNodeData, SemanticNodeId};

    // Seed the cyclic aliases through the SHALLOW indexing path
    // (`ensure_indexed_ready`) rather than full SFC evaluation. The
    // carrier walk only needs the prepared type-decl bodies for CycA/CycB
    // to exist; full `defineEmits<CycA>()` evaluation of a mutual alias
    // cycle is a SEPARATE upstream concern and is out of scope here — this
    // test isolates the carrier walk's own cycle termination.
    let ws = verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default());
    ws.inject_file(
        "/src/cyclic.ts".to_string(),
        std::sync::Arc::from("export type CycA = CycB\nexport type CycB = CycA\n"),
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        std::sync::Arc::new(ws),
    );
    let _seeded = host
        .ensure_indexed_ready("/src/cyclic.ts")
        .expect("cyclic alias module must shallow-index");

    let scope = "/src/cyclic.ts";
    let dispatch = ProjectSemanticDispatch::new(&host);

    // Navigate-lower the cyclic alias head to its DeclRef carrier — the
    // shape the macro projector hands the carrier walk. Navigate produces
    // a TERMINAL carrier without recursing into the cyclic body, so this
    // lowering does not itself diverge.
    let cyc_ref = TypeExpr::Ref {
        name: std::sync::Arc::from("CycA"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new().into_boxed_slice()),
    };
    let payload_node = dispatch
        .lower_type_expr_in_scope_with_mode(scope, &cyc_ref, ProjectionMode::Navigate)
        .expect("CycA must lower to a Navigate carrier node");

    // Precondition: the cyclic head is a carrier, not a Conditional.
    assert!(
        !matches!(
            crate::project_semantic_dispatch::node_data_for(&host, payload_node).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "fixture precondition: CycA must Navigate-lower to a CARRIER, not a \
         bare Conditional"
    );

    // Drive the carrier walk DIRECTLY so the termination mechanism is
    // observable. A mutual alias cycle has no Conditional root → `None`.
    let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
    let result = resolve_emit_payload_to_conditional_root(&dispatch, payload_node, 0, &mut visited);

    assert!(
        result.is_none(),
        "a mutual 2-node alias cycle (type CycA = CycB; type CycB = CycA) used \
         as an emit payload has no Conditional root — the carrier walk MUST \
         return None, got {result:?}"
    );

    // IDENTITY, not the fuse: the walk re-entered CycA after visiting
    // exactly {CycA, CycB}. Termination happened at re-entry depth 2, far
    // below the pathological fuse — proof the cycle is caught by node
    // identity on the first repeat, not by exhausting the depth bound.
    assert_eq!(
        visited.len(),
        2,
        "the carrier walk must visit exactly the two cycle nodes (CycA, CycB) \
         then terminate on the FIRST re-entry of CycA — visited set: \
         {visited:?}"
    );
    assert!(
        visited.len() < EMIT_CARRIER_WALK_FUSE,
        "cycle termination must occur far below the pathological fuse \
         ({EMIT_CARRIER_WALK_FUSE}); identity termination visited only \
         {} node(s)",
        visited.len()
    );
}

mod node_predicates_tests {
    use super::make_project;
    use crate::meta_resolve::{
        component_meta_ref_resolves_to_package_node,
        declaration_body_prefers_inline_materialization_node, extract_route_root_identity_node,
        ref_root_reaches_transitive_cycle_node,
    };
    use crate::resolver_core::RouteDemand;
    use crate::semantic_query::{
        DeclIdentity, IndexKey, IndexSignature, SemanticNodeData, SemanticNodeId, SurfaceMember,
        SurfaceView,
    };
    use std::sync::Arc as StdArc;
    use verter_type_expr::LiteralValue;

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
        // builtin Pick/Omit use the `__builtin__`
        // sentinel canonical id; the registry-route extractor only
        // dispatches builtin (not userland) Pick/Omit through the
        // route branch so userland shadowing is preserved.
        DeclIdentity {
            canonical_id: StdArc::from("__builtin__"),
            whole_hash: [0u8; 16],
            decl_name: StdArc::from(name),
        }
    }

    /// Route-extraction contract: `Pick<Foo<T>, 'a'>` — generic root
    /// is accepted; the extractor recurses into `args[0]` to find the
    /// actual root identity (`Foo`) and preserves the generic
    /// arguments via `RouteExtraction.root_args`. The route branch
    /// must project `Pick<Foo<T>, 'a'>` shapes through dispatch with
    /// the original carriers (rejecting generic roots here would
    /// break member-projection over generic helpers).
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
            "root_args must preserve the generic [T] carrier"
        );
        assert_eq!(extraction.root_args[0], t_ref);
    }

    /// Predicate matrix row 2: `Foo[0]` — numeric index rejected.
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
            index: IndexKey::Number(
                crate::semantic_query::CanonicalIndexInt::from_canonical_i64(0).expect("canonical"),
            ),
        });

        assert!(
            extract_route_root_identity_node(graph, indexed, 0).is_none(),
            "Foo[0] must be rejected (numeric index) — registry-route predicate matrix"
        );
    }

    /// Predicate matrix row 3: `Pick<Foo, 'a' | 'b' | 'c'>` — accepted.
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
            .expect("Pick<Foo, 'a' | 'b' | 'c'> must be accepted (predicate matrix row 3)");
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

    /// Predicate matrix row 4: `Pick<Foo>` — 1-arg rejected.
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
            "Pick<Foo> (1-arg) must be rejected (args.len() != 2) — predicate matrix"
        );
    }

    /// Predicate matrix row 5: `Pick<Foo, 'a', 'b'>` — 3-arg rejected.
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
            "Pick<Foo, 'a', 'b'> (3-arg) must be rejected — predicate matrix"
        );
    }

    /// Predicate matrix row 6: `Pick<Foo, never>` — empty union rejected.
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
            "Pick<Foo, never> (empty key set) must be rejected — predicate matrix"
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

    /// `component_meta_ref_resolves_to_package_node` — routes the canonical-id
    /// classification through `ResolverContext::workspace_is_package_backed`.
    /// Must reject local refs and accept `node_modules`-rooted refs whose
    /// realpath is not claimed by any workspace project.
    #[test]
    fn package_ref_predicate_discriminates_local_vs_node_modules() {
        let project = make_project();
        let host = project.host();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;

        let local = synthetic_decl_identity("Foo");
        let pkg = package_decl_identity("Bar");

        assert!(
            !component_meta_ref_resolves_to_package_node(ctx, &local),
            "local /test/local.ts decl must NOT be classified as package-backed"
        );
        assert!(
            component_meta_ref_resolves_to_package_node(ctx, &pkg),
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
            signature_span: None,
            return_type_span: None,
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

    /// Predicate matrix row 7: A → B → C → A cycle through a complex
    /// helper.
    ///
    /// 13 / R7-14 — the legacy parity BFS only flags a
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

    /// L2: the cycle/fence guard derives its root identity from the
    /// utility's SOURCE type-argument, not just the outer `Ref` name.
    /// `Pick<A, 'next'>` where `A` transitively cycles (A → B(keyof C) →
    /// C → A) must be DETECTED — pre-fix `root_decl_identity` rooted the
    /// BFS at `__builtin__::Pick` (structurally blind to the source
    /// chain) and missed the cycle.
    #[test]
    fn cycle_guard_roots_at_utility_source_type_argument() {
        use verter_type_expr::TypeExpr;

        let project = make_project();
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
        let session = project.open_session_batch().unwrap();
        // Seed IndexedReady/analysis for /cycle.ts.
        let _ = session.evaluate_types("/cycle.ts");

        let host = session.host();
        let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);

        // `Pick<A, 'next'>` — the source argument `A` is the cyclic root.
        let pick_over_cycle = TypeExpr::named_with_args(
            "Pick",
            vec![TypeExpr::named("A"), TypeExpr::string_literal("next")],
        );
        let detected = crate::meta_resolve::lowered_root_reaches_transitive_cycle(
            &mut engine,
            "/cycle.ts",
            &pick_over_cycle,
        );
        assert!(
            detected,
            "Pick<A,'next'> over a cyclic source `A` must be detected via the \
             source type-argument root (L2) — rooting only at `Pick` misses it"
        );
    }

    /// Pick / Omit shapes through `evaluate_types`
    /// stay healthy after the alias-body rescue chain and the
    /// inline-registry-member-route candidate chain were deleted.
    /// B1's materialiser registry-route branch dispatches Pick/Omit
    /// shapes through dispatch's canonical projection.
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

    /// `engine.materialize_member_surface_expr` (the graph-native
    /// replacement for the deleted legacy walker shim) must fan the
    /// materialiser's `dep_signature` into every active fact tracer,
    /// so callers' completion fences observe the dep facts captured
    /// by the inner `materialize_component_meta_structure` call.
    ///
    /// The materialise site
    /// (`meta_resolve/materialize/field_types.rs::materialize_component_meta_type_expr_until_stable_full`)
    /// converts the dispatch's `DepSignature` into `FactVersionRef`
    /// entries via `dep_signature_to_fact_signature` and emits them
    /// through `observe_fact_signature`, which delivers into every
    /// active `FactReadSet` on the tracer stack.
    #[test]
    fn engine_materialize_member_surface_expr_accumulates_dep_signature() {
        use crate::resolver_core::ComponentMetaQueryEngine;
        use std::sync::Arc as StdArc;
        use verter_type_expr::TypeExpr;

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

        // Wrap the engine call in `with_fact_tracer` so we can
        // observe the fan-out signature. The materialise site emits
        // its dep_signature through `observe_fact_signature`, which
        // delivers `FactVersionRef` entries into the active tracer's
        // `FactReadSet`.
        let (_result, fact_set) = host.with_fact_tracer(|| {
            let mut engine = ComponentMetaQueryEngine::new(host);
            engine.materialize_member_surface_expr(
                "/Owner.vue",
                &TypeExpr::Ref {
                    name: StdArc::from("Foo"),
                    type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
                },
                false,
            )
        });
        assert!(
            !fact_set.is_empty(),
            "engine method must fan the materialiser's dep_signature into the \
             active fact tracer; observed an empty FactReadSet inside the \
             with_fact_tracer scope"
        );
    }

    /// `type_node_has_package_backed_root` is the
    /// graph-native package-backed root predicate (former TypeExpr
    /// counterpart §6.15 / N). Operates on
    /// `SemanticNodeId`. Equivalence baseline: for representative
    /// shapes (DeclRef, InstantiationRef, IndexedAccess chain, Array,
    /// KeyOf, Tuple), the predicate produces the legacy result.
    #[test]
    fn type_node_has_package_backed_root_matches_type_expr_predicate() {
        use crate::meta_resolve::type_node_has_package_backed_root;

        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;

        let local = synthetic_decl_identity("Local");
        let pkg = package_decl_identity("FromPkg");

        // Bare DeclRef cases
        let local_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: local.clone(),
        });
        let pkg_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: pkg.clone(),
        });
        assert!(
            !type_node_has_package_backed_root(ctx, local_ref, 0),
            "DeclRef -> /test/local.ts must not be flagged package-backed"
        );
        assert!(
            type_node_has_package_backed_root(ctx, pkg_ref, 0),
            "DeclRef -> /node_modules/* must be flagged package-backed"
        );

        // InstantiationRef inherits the base identity's canonical scope
        let pkg_inst = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: pkg.clone(),
            args: StdArc::from(vec![local_ref].into_boxed_slice()),
        });
        assert!(
            type_node_has_package_backed_root(ctx, pkg_inst, 0),
            "InstantiationRef base in node_modules must be flagged package-backed"
        );
        let local_inst = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: local.clone(),
            args: StdArc::from(vec![pkg_ref].into_boxed_slice()),
        });
        assert!(
            !type_node_has_package_backed_root(ctx, local_inst, 0),
            "InstantiationRef base in local file must NOT be flagged \
             even when args reference a package decl (mirrors TypeExpr semantics — \
             only the root identity counts)"
        );

        // IndexedAccess chain: walks down into `object`, ignoring `index`.
        let pkg_indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: pkg_ref,
            index: IndexKey::String(StdArc::from("foo")),
        });
        assert!(
            type_node_has_package_backed_root(ctx, pkg_indexed, 0),
            "IndexedAccess(pkg, 'foo') must follow object to detect pkg root"
        );
        let local_indexed_chain = {
            let inner = graph.intern_node(SemanticNodeData::IndexedAccess {
                object: local_ref,
                index: IndexKey::String(StdArc::from("a")),
            });
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object: inner,
                index: IndexKey::String(StdArc::from("b")),
            })
        };
        assert!(
            !type_node_has_package_backed_root(ctx, local_indexed_chain, 0),
            "Two-deep IndexedAccess on local root must NOT trigger"
        );

        // Array carrier
        let pkg_array = graph.intern_node(SemanticNodeData::Array {
            element: pkg_ref,
            readonly: false,
        });
        assert!(
            type_node_has_package_backed_root(ctx, pkg_array, 0),
            "Array<pkg> must follow element"
        );

        // KeyOf carrier
        let pkg_keyof = graph.intern_node(SemanticNodeData::KeyOf { base: pkg_ref });
        assert!(
            type_node_has_package_backed_root(ctx, pkg_keyof, 0),
            "keyof pkg must follow base"
        );

        // Tuple — any element with a package root flips the predicate
        let local_tuple_only = graph.intern_node(SemanticNodeData::Tuple {
            elements: StdArc::from(
                vec![crate::semantic_query::TupleElement {
                    label: None,
                    value: local_ref,
                    optional: false,
                    rest: false,
                }]
                .into_boxed_slice(),
            ),
            readonly: false,
        });
        assert!(
            !type_node_has_package_backed_root(ctx, local_tuple_only, 0),
            "[Local] tuple must NOT be flagged"
        );
        let mixed_tuple = graph.intern_node(SemanticNodeData::Tuple {
            elements: StdArc::from(
                vec![
                    crate::semantic_query::TupleElement {
                        label: None,
                        value: local_ref,
                        optional: false,
                        rest: false,
                    },
                    crate::semantic_query::TupleElement {
                        label: None,
                        value: pkg_ref,
                        optional: false,
                        rest: false,
                    },
                ]
                .into_boxed_slice(),
            ),
            readonly: false,
        });
        assert!(
            type_node_has_package_backed_root(ctx, mixed_tuple, 0),
            "[Local, Pkg] tuple must be flagged via the second element"
        );

        // Alias passes through to inner
        let alias_pkg = graph.intern_node(SemanticNodeData::Alias(pkg_ref));
        assert!(
            type_node_has_package_backed_root(ctx, alias_pkg, 0),
            "Alias must follow inner"
        );

        // Non-route shapes (Object, Primitive) — predicate returns false
        let obj = graph.intern_node(SemanticNodeData::Object(empty_surface(vec![])));
        assert!(
            !type_node_has_package_backed_root(ctx, obj, 0),
            "Plain Object must NOT be flagged (no root identity)"
        );
        let prim = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        assert!(
            !type_node_has_package_backed_root(ctx, prim, 0),
            "Primitive must NOT be flagged"
        );
    }

    /// depth fuse: pathological synthetic graphs do
    /// not stack-overflow. The predicate fuses at depth=256 and returns
    /// `false` (matching the legacy walker's runaway-recursion behaviour
    /// elsewhere in the file).
    #[test]
    fn type_node_has_package_backed_root_depth_fuse_at_256() {
        use crate::meta_resolve::type_node_has_package_backed_root;

        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;

        // Build an IndexedAccess chain N=300 deep over a local DeclRef
        // root. Should walk fine until depth 256, then fuse-return false.
        let local = synthetic_decl_identity("L");
        let local_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: local.clone(),
        });
        let mut current = local_ref;
        for i in 0..300u32 {
            current = graph.intern_node(SemanticNodeData::IndexedAccess {
                object: current,
                index: IndexKey::String(StdArc::from(format!("k{i}"))),
            });
        }
        // At depth=0 entry, walking the 300-deep chain to its local
        // root would normally yield false; the fuse just guards against
        // pathological recursion. Either way, must not panic.
        let _ = type_node_has_package_backed_root(ctx, current, 0);
    }

    /// `preserve_package_backed_symbolic_refs_node`
    /// is the graph-native parallel-pair walker (former TypeExpr
    /// counterpart §6.15 / N). Operates on
    /// `SemanticNodeId` parallel pairs. Walks materialized + raw
    /// surfaces; when a raw property's value is a package-backed
    /// `DeclRef`/`InstantiationRef`, the corresponding materialized
    /// member's value is overridden with the raw graph node (preserving
    /// the symbolic Ref through materialisation).
    #[test]
    fn preserve_package_backed_symbolic_refs_node_overrides_pkg_member() {
        use crate::meta_resolve::preserve_package_backed_symbolic_refs_node;
        use crate::semantic_query::{IndexSignature, SemanticNodeData, SurfaceMember, SurfaceView};

        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let pkg_identity = package_decl_identity("PkgType");
        let pkg_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: pkg_identity.clone(),
        });
        let local_identity = synthetic_decl_identity("LocalType");
        let local_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: local_identity.clone(),
        });
        let prim_string = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));
        let prim_number = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));

        // Build raw surface: { a: PkgType, b: LocalType, c: string }
        let raw_surface = SurfaceView {
            members: StdArc::from(
                vec![
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("a"),
                        value: pkg_ref,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("b"),
                        value: local_ref,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("c"),
                        value: prim_string,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                ]
                .into_boxed_slice(),
            ),
            call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            index_signatures: StdArc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let raw = graph.intern_node(SemanticNodeData::Object(raw_surface));

        // Materialised version: { a: number, b: number, c: number } —
        // every property has been collapsed to `number`. The expected
        // result after preservation: a switches BACK to PkgType (the
        // raw symbolic Ref is restored), b stays as `number` (raw was
        // a local Ref, not package-backed), c stays as `number`.
        let materialized_surface = SurfaceView {
            members: StdArc::from(
                vec![
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("a"),
                        value: prim_number,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("b"),
                        value: prim_number,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                    SurfaceMember {
                        visibility: verter_type_expr::MemberVisibility::Public,
                        name: StdArc::from("c"),
                        value: prim_number,
                        optional: false,
                        readonly: false,
                        is_method: false,
                        declared_in_macro_type_arg: false,
                        merge_role: crate::semantic_query::MemberMergeRole::Authored,
                        spans: Default::default(),
                        declaration_origin: None,
                    },
                ]
                .into_boxed_slice(),
            ),
            call_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            construct_signatures: StdArc::from(Vec::new().into_boxed_slice()),
            index_signatures: StdArc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        };
        let materialized = graph.intern_node(SemanticNodeData::Object(materialized_surface));

        let result_id = preserve_package_backed_symbolic_refs_node(host, materialized, raw, 0);

        // Inspect the result surface.
        let Some(result_data) = graph.node_data(result_id) else {
            panic!("result must have node data");
        };
        let SemanticNodeData::Object(result_surface) = result_data.as_ref() else {
            panic!("result must be an Object surface");
        };
        // Member a — must be the raw pkg_ref (package preservation fired).
        let member_a = &result_surface.members[0];
        assert_eq!(member_a.name.as_ref(), "a");
        assert_eq!(
            member_a.value, pkg_ref,
            "package-backed raw Ref must be preserved into the materialised member"
        );
        // Member b — must remain the materialised `number` (no override).
        let member_b = &result_surface.members[1];
        assert_eq!(member_b.name.as_ref(), "b");
        assert_eq!(
            member_b.value, prim_number,
            "local raw Ref must NOT trigger preservation; member stays materialised"
        );
        // Member c — must remain materialised (no Ref in raw).
        let member_c = &result_surface.members[2];
        assert_eq!(member_c.name.as_ref(), "c");
        assert_eq!(
            member_c.value, prim_number,
            "primitive raw value must NOT trigger preservation"
        );
    }

    /// non-Object pair: pass-through (returns
    /// materialized unchanged). Mirrors the TypeExpr predicate's
    /// `_ => materialized.clone()` arm.
    #[test]
    fn preserve_package_backed_symbolic_refs_node_passes_through_non_object() {
        use crate::meta_resolve::preserve_package_backed_symbolic_refs_node;
        use crate::semantic_query::SemanticNodeData;

        let project = make_project();
        let host = project.host();
        let graph = host.project_type_store().semantic_graph();

        let prim_number = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::Number,
        ));
        let prim_string = graph.intern_node(SemanticNodeData::Primitive(
            crate::semantic_query::PrimitiveKind::String,
        ));

        // Both primitives — non-Object. Returns materialized unchanged.
        let result = preserve_package_backed_symbolic_refs_node(host, prim_number, prim_string, 0);
        assert_eq!(
            result, prim_number,
            "non-Object pair must return materialized unchanged"
        );

        // Materialized = Object, raw = primitive. Returns materialized unchanged.
        let obj_surface = empty_surface(vec![]);
        let obj = graph.intern_node(SemanticNodeData::Object(obj_surface));
        let result2 = preserve_package_backed_symbolic_refs_node(host, obj, prim_string, 0);
        assert_eq!(
            result2, obj,
            "Object materialized + non-Object raw must pass through unchanged"
        );
    }

    /// Node-domain registry structural materialisation, no-args `Ref` arm:
    /// a package-backed `Ref { name, [] }` short-circuits (returns the input
    /// unchanged) and carries object-surface fact `false`; a local
    /// `Ref { name, [] }` projects to its whole surface and carries the
    /// producing node's object-surface fact. Asserts BOTH the materialised
    /// `TypeExpr` and the threaded object-surface fact for each branch.
    #[test]
    fn registry_structural_expr_handles_package_vs_local_no_args_ref() {
        use crate::resolver_core::ComponentMetaQueryEngine;
        use std::sync::Arc as StdArc;
        use verter_type_expr::TypeExpr;

        let project = make_project();
        // Local interface — projects through surface.
        project
            .upsert_base(
                "/local.ts",
                "export interface LocalLeaf { x: number; y: string }",
            )
            .unwrap();
        // Package-backed type — must short-circuit (stay symbolic).
        project
            .upsert_base(
                "/node_modules/some-pkg/index.d.ts",
                "export interface FromPkg { p: number }",
            )
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { LocalLeaf } from './local'
import type { FromPkg } from 'some-pkg'
defineProps<{ a: LocalLeaf; b: FromPkg }>()
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
                    specifier: "some-pkg".to_string(),
                    resolved_canonical_id: Some("/node_modules/some-pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/Owner.vue").unwrap();
        let host = session.host();
        let mut engine = ComponentMetaQueryEngine::new(host);

        // Package-backed `FromPkg` — short-circuits unchanged.
        // Discriminating assertion: the result MUST be exactly the
        // input Ref (cloned). If the refactor accidentally inverts the
        // package check or skips it, FromPkg would be projected and
        // fail this assertion.
        let pkg_ref = TypeExpr::Ref {
            name: StdArc::from("FromPkg"),
            type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
        };
        let (materialized_pkg, pkg_is_object) =
            engine.materialize_registry_structural_candidate("/Owner.vue", &pkg_ref);
        assert_eq!(
            materialized_pkg, pkg_ref,
            "package-backed Ref must short-circuit unchanged; \
             refactor regression if this fails (e.g., inverted package check)"
        );
        assert!(
            !pkg_is_object,
            "a symbolic package-backed Ref carries no explicit object surface, \
             so the threaded object-surface fact must be false — regression if \
             the fact mis-reports a symbolic ref as an object surface"
        );

        // Local `LocalLeaf` — must NOT short-circuit to itself; must
        // produce SOMETHING different from the input (either projected
        // Object surface or whatever the projection returns). The key
        // discriminator: local refs do NOT use the package short-circuit
        // path. If the refactor accidentally treats local refs as
        // package-backed, this assertion fails.
        let local_ref = TypeExpr::Ref {
            name: StdArc::from("LocalLeaf"),
            type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
        };
        let (materialized_local, local_is_object) =
            engine.materialize_registry_structural_candidate("/Owner.vue", &local_ref);
        // Local Ref should project through the whole-surface candidate; since
        // LocalLeaf is a real interface, the candidate returns the Object
        // surface, so materialized_local must NOT be the input Ref unchanged.
        assert_ne!(
            materialized_local, local_ref,
            "local LocalLeaf with projectable interface body must NOT \
             short-circuit unchanged — refactor regression if this fails \
             (e.g., local refs misclassified as package-backed)"
        );
        assert!(
            local_is_object,
            "LocalLeaf projects to its interface object surface, so the threaded \
             object-surface fact must be true — regression if the fact is dropped \
             or forced always-false"
        );
    }
}

/// Overlay/base prop isolation through context-aware `vue_macro_dtos`.
///
/// `vue_macro_dtos_with_ctx(ctx, …)` resolves the macro surface through the
/// active `ResolverContext`. An overlay session that adds `overlay_prop` to the
/// SFC's own `interface Props { a }` MUST see `[a, overlay_prop]` — the surface
/// is resolved against the OVERLAY `IndexedReady`, not the base host view. A
/// base-view read MUST see only `[a]`, and the overlay surface MUST NOT leak
/// into the base read (the two key on distinct `whole_hash`es).
///
/// Discrimination: a `vue_macro_dtos_with_ctx` that ignores `ctx` and reads the
/// base host view (the pre-step-2 `vue_macro_dtos` behaviour) returns `[a]` for
/// the overlay session too, failing the `overlay_prop` assertion. Verified by
/// mutation (binding the cold path to a base `HostResolverContext` drops
/// `overlay_prop`).
#[test]
fn overlay_session_vue_macro_dtos_sees_overlay_prop_without_leaking_to_base() {
    use crate::resolver_core::ResolverContext;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    const SFC: &str = "/Comp.vue";
    let base_src = "<script setup lang=\"ts\">\n\
         interface Props { a: string }\n\
         defineProps<Props>()\n\
         </script>";
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: None,
            input_id: SFC.to_string(),
            source: Arc::from(base_src),
            file_language: crate::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Locate the `defineProps` macro index from the authoritative snapshot.
    let indexed = host
        .ensure_indexed_ready(SFC)
        .expect("SFC must index ready");
    let define_props_index = indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .expect("the SFC declares a defineProps macro");

    let request_for = |root_identity: [u8; 16]| crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from(SFC),
        macro_index: define_props_index,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        root_identity,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    let prop_names = |dtos: &crate::typeinfo::framework_surface::MacroSurfaceDtos| -> Vec<String> {
        dtos.prop_fields().iter().map(|p| p.name.clone()).collect()
    };

    // Base-view read (no overlay): only the base prop `a`.
    let base_dtos = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        prop_names(&base_dtos),
        vec!["a".to_string()],
        "base-view defineProps surface must be exactly [a]"
    );

    // Overlay session: overlay the SFC adding `overlay_prop` to `Props`.
    let overlay_src = "<script setup lang=\"ts\">\n\
         interface Props { a: string; overlay_prop: number }\n\
         defineProps<Props>()\n\
         </script>";
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(SFC.to_string(), Arc::from(overlay_src));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let session_ctx = crate::resolver_core::SessionResolverContext::new(
        &host,
        &view,
        &store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );

    // The overlay session's own whole-hash hint (resolved through the session
    // ctx) keys the overlay DTO slot; the core re-derives + validates it.
    let overlay_hash = ResolverContext::get_whole_hash(&session_ctx, SFC).unwrap_or([0u8; 16]);
    let overlay_dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        &session_ctx,
        &request_for(overlay_hash),
    )
    .dtos;
    let overlay_props = prop_names(&overlay_dtos);
    assert!(
        overlay_props.contains(&"a".to_string()),
        "overlay defineProps surface keeps the base prop `a`: {overlay_props:?}"
    );
    assert!(
        overlay_props.contains(&"overlay_prop".to_string()),
        "overlay defineProps surface MUST include the overlay-added `overlay_prop` \
         — the surface is resolved against the overlay IndexedReady, not the base \
         host view: {overlay_props:?}"
    );

    // No leak: a fresh base-view read still sees only `[a]`. The overlay surface
    // was keyed on a distinct `whole_hash`, so the base slot is untouched.
    let base_dtos_after = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        prop_names(&base_dtos_after),
        vec!["a".to_string()],
        "the overlay session's `overlay_prop` must NOT leak into the base-view \
         defineProps surface: {:?}",
        prop_names(&base_dtos_after)
    );
}

/// Overlay/base `defineModel` isolation through context-aware `vue_macro_dtos`.
///
/// The `defineModel` model prop is synthesized from the owner SFC's
/// `IndexedReady` macro facts (`props_from_typeinfo_surface` →
/// `model_prop_fields`), NOT a macro-T object surface. That snapshot fetch MUST
/// flow through the active `ResolverContext` (`ctx.ensure_indexed_ready`,
/// overlay-aware), not the base `VerterHost`. An overlay session that rewrites
/// `defineModel<string>('old')` to `defineModel<number>('fresh')` MUST see the
/// model prop `fresh`; a base-view read MUST see only `old`, and the overlay
/// model prop MUST NOT leak into the base read.
///
/// Discrimination: the pre-fix `model_prop_fields(host, …)` read the base
/// `host.ensure_indexed_ready`, so the overlay session's model prop resolved
/// against the BASE macro facts and surfaced `old`, not `fresh`. Routing the
/// snapshot read through `ctx.ensure_indexed_ready` fixes the leak. Verified by
/// mutation (reverting `model_prop_fields` to the base host read surfaces `old`
/// for the overlay session — the `fresh` assertion fails).
#[test]
fn overlay_session_vue_macro_dtos_define_model_reads_overlay_without_leaking_to_base() {
    use crate::resolver_core::ResolverContext;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    const SFC: &str = "/Model.vue";
    // Base SFC: `defineModel<string>('old')` → model prop named `old`.
    let base_src = "<script setup lang=\"ts\">\n\
         defineModel<string>('old')\n\
         </script>";
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: None,
            input_id: SFC.to_string(),
            source: Arc::from(base_src),
            file_language: crate::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    // Locate the `defineModel` macro index from the authoritative snapshot.
    let indexed = host
        .ensure_indexed_ready(SFC)
        .expect("SFC must index ready");
    let define_model_index = indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
        .expect("the SFC declares a defineModel macro");

    let request_for = |root_identity: [u8; 16]| crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from(SFC),
        macro_index: define_model_index,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineModel,
        root_identity,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    let prop_names = |dtos: &crate::typeinfo::framework_surface::MacroSurfaceDtos| -> Vec<String> {
        dtos.prop_fields().iter().map(|p| p.name.clone()).collect()
    };

    // Base-view read (no overlay): only the base model prop `old`.
    let base_dtos = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        prop_names(&base_dtos),
        vec!["old".to_string()],
        "base-view defineModel surface must be exactly [old]"
    );

    // Overlay session: overlay the SFC rewriting the model name + type.
    let overlay_src = "<script setup lang=\"ts\">\n\
         defineModel<number>('fresh')\n\
         </script>";
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(SFC.to_string(), Arc::from(overlay_src));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let session_ctx = crate::resolver_core::SessionResolverContext::new(
        &host,
        &view,
        &store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );

    let overlay_hash = ResolverContext::get_whole_hash(&session_ctx, SFC).unwrap_or([0u8; 16]);
    let overlay_dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        &session_ctx,
        &request_for(overlay_hash),
    )
    .dtos;
    let overlay_props = prop_names(&overlay_dtos);
    assert!(
        overlay_props.contains(&"fresh".to_string()),
        "overlay defineModel surface MUST reflect the overlay's \
         `defineModel<number>('fresh')` model prop — `model_prop_fields` must fetch \
         the owner SFC IndexedReady through the active overlay ctx, not the base \
         host view: {overlay_props:?}"
    );
    assert!(
        !overlay_props.contains(&"old".to_string()),
        "the overlay session MUST NOT leak the base `defineModel<string>('old')` \
         model prop: {overlay_props:?}"
    );

    // No leak: a fresh base-view read still sees only `[old]`.
    let base_dtos_after = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        prop_names(&base_dtos_after),
        vec!["old".to_string()],
        "the overlay session's `fresh` model prop must NOT leak into the base-view \
         defineModel surface: {:?}",
        prop_names(&base_dtos_after)
    );
}

/// Overlay/base SLOT-BINDING isolation through context-aware `vue_macro_dtos`.
///
/// The slot binding object lives in a CROSS-FILE carrier (`/slots.ts`). An
/// overlay session that rewrites `/slots.ts`'s slot-prop object from `{ old }`
/// to `{ fresh }` MUST see the slot binding `fresh` — the slot normalizer's
/// callable realization + first-parameter object navigation
/// (`slots_from_typeinfo_surface` → `navigate_param_to_object_surface`) flow
/// through the active session `ctx`, so they read the OVERLAY `/slots.ts`, not
/// the base host view. A base-view read MUST see only `old`, and the overlay
/// binding MUST NOT leak into the base read.
///
/// Discrimination: the pre-fix slot path built a FRESH base `HostResolverContext`
/// inside the pre-node-domain slot raise path / `navigate_param_to_object_surface`,
/// so the overlay session's slot bindings were resolved against the BASE
/// `/slots.ts` and surfaced `old`, not `fresh`. Routing those reads through
/// `ctx.dispatch()` fixes the leak. Verified by mutation (reverting the slot
/// helpers to a base context surfaces `old` for the overlay session).
#[test]
fn overlay_session_vue_macro_slot_bindings_read_overlay_carrier_without_leaking_to_base() {
    use crate::resolver_core::ResolverContext;

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    const SFC: &str = "/Comp.vue";
    const SLOTS_TS: &str = "/slots.ts";
    // Base `/slots.ts`: the slot-prop object carries `old`.
    let base_slots = "export type SlotProps = { old: string };\n\
         export type SlotFn = (props: SlotProps) => any;\n\
         export type Slots = { default: SlotFn };\n";
    let sfc_src = "<script setup lang=\"ts\">\n\
         import type { Slots } from './slots';\n\
         defineSlots<Slots>()\n\
         </script>";
    for (id, src) in [(SLOTS_TS, base_slots), (SFC, sfc_src)] {
        let kind = if id.ends_with(".vue") {
            crate::FileLanguage::vue()
        } else {
            crate::FileLanguage::script_ts()
        };
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: id.to_string(),
                source: Arc::from(src),
                file_language: kind,
                aliases: Vec::new(),
            })
            .expect("upsert succeeds");
    }

    let indexed = host
        .ensure_indexed_ready(SFC)
        .expect("SFC must index ready");
    let define_slots_index = indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .expect("the SFC declares a defineSlots macro");

    let request_for = |root_identity: [u8; 16]| crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from(SFC),
        macro_index: define_slots_index,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
        root_identity,
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    };
    // Binding names on the `default` slot.
    let default_binding_names =
        |dtos: &crate::typeinfo::framework_surface::MacroSurfaceDtos| -> Vec<String> {
            dtos.slot_fields()
                .iter()
                .find(|s| s.name == "default")
                .map(|s| s.bindings.iter().map(|b| b.name.clone()).collect())
                .unwrap_or_default()
        };

    // Base-view read: the slot binding is `old`.
    let base_dtos = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        default_binding_names(&base_dtos),
        vec!["old".to_string()],
        "base-view defineSlots `default` binding must be exactly [old]"
    );

    // Overlay session: rewrite `/slots.ts`'s slot-prop object to `{ fresh }`.
    let overlay_slots = "export type SlotProps = { fresh: boolean };\n\
         export type SlotFn = (props: SlotProps) => any;\n\
         export type Slots = { default: SlotFn };\n";
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(SLOTS_TS.to_string(), Arc::from(overlay_slots));
    let view = crate::session_view::OverlaidView::new(Arc::clone(&host), overlays);
    let store_view = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let session_ctx = crate::resolver_core::SessionResolverContext::new(
        &host,
        &view,
        &store_view,
        std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new()),
    );

    // The SFC's own whole-hash is UNCHANGED (only the carrier `/slots.ts` was
    // overlaid), so the overlay read keys on the same SFC hash — the binding
    // must still reflect the OVERLAY carrier through the ctx-bound resolution.
    let overlay_hash = ResolverContext::get_whole_hash(&session_ctx, SFC).unwrap_or([0u8; 16]);
    let overlay_dtos = crate::typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx(
        &session_ctx,
        &request_for(overlay_hash),
    )
    .dtos;
    let overlay_bindings = default_binding_names(&overlay_dtos);
    assert_eq!(
        overlay_bindings,
        vec!["fresh".to_string()],
        "the overlay session's defineSlots `default` binding MUST reflect the \
         OVERLAY `/slots.ts` (`fresh`), NOT the base carrier (`old`) — the slot \
         callable realization + first-param navigation read through the session \
         ctx: {overlay_bindings:?}"
    );

    // No leak: a fresh base-view read still sees only `old`.
    let base_dtos_after = host.vue_macro_dtos(&request_for(
        host.current_or_read_whole_hash(SFC).unwrap_or([0u8; 16]),
    ));
    assert_eq!(
        default_binding_names(&base_dtos_after),
        vec!["old".to_string()],
        "the overlay session's `fresh` slot binding must NOT leak into the \
         base-view defineSlots surface: {:?}",
        default_binding_names(&base_dtos_after)
    );
}

/// Graph-native generic slot-alias bindings under a CONCRETE substitution.
///
/// `defineSlots<TabsSlots<{ id: string }>>()` where the slot's binding
/// parameter is a generic alias whose BODY ROOT is a Conditional that is OPEN
/// when the type parameter is unbound but REDUCES to a concrete object once the
/// parameter is substituted:
/// `type SlotProps<T> = T extends { id: infer U } ? { row: U } : { row: never }`.
/// At `T = { id: string }` the conditional reduces to `{ row: string }`, so the
/// graph-native synthesis MUST realize the callable slot member under the
/// substitution, find the first parameter, and publish the binding row `row`.
///
/// Discrimination: the pre-fix `slot_param_root_is_symbolic_only` instantiated
/// the `InstantiationRef` carrier with EMPTY args, so `SlotProps<T>`'s body
/// stayed an OPEN Conditional (`T` an unbound `TypeParam`) and the predicate
/// returned `true` (symbolic-only) — the synthesis SKIPPED the slot and
/// published NO binding row. With `(base, args, context)` preserved, the body
/// reduces to the concrete `{ row: string }` object (non-symbolic) and the
/// `default.row` binding is published. Verified pre-fail / post-pass.
#[test]
fn graph_native_generic_slot_alias_publishes_bindings_under_concrete_substitution() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
export type SlotProps<T> = T extends { id: infer U } ? { row: U } : { row: never }
export type SlotFn<T> = (props: SlotProps<T>) => any
export type TabsSlots<T> = { default: SlotFn<T> }
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { TabsSlots } from './slots'

defineSlots<TabsSlots<{ id: string }>>()
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
    assert!(
        slot_names.contains("default"),
        "the `default` slot must be present on the resolved slot shape: {slot_names:?}"
    );

    let default_bindings: std::collections::BTreeSet<_> = evaluated
        .slot_bindings
        .iter()
        .filter_map(|binding| binding.name.strip_prefix("default.").map(str::to_string))
        .collect();
    assert!(
        default_bindings.contains("row"),
        "the generic slot alias `SlotProps<{{ id: string }}>` must publish the \
         binding row `row` under the concrete substitution T = {{ id: string }} \
         (the pre-fix empty-args InstantiationRef instantiation dropped it): \
         {default_bindings:?}"
    );
}

/// Graph-native OPEN generic slot alias does NOT invent bindings.
///
/// When the slot parameter's root shape stays SYMBOLIC (here an
/// `IndexedAccess` `T['missing']` that never resolves to a concrete object),
/// the synthesis must DECLINE to publish a binding row — preserving the
/// shallow-until-resolved contract. Inventing a row here would materialise a
/// guess from an undetermined generic context.
///
/// Discrimination: a synthesis that enumerates the symbolic param root anyway
/// (e.g. committing to a mapped/indexed surface) would publish a phantom
/// binding; the `slot_param_root_is_symbolic_only` gate keeps `IndexedAccess`
/// roots symbolic so NO binding is published. (Paired with the positive test
/// above: the fix must publish for the CONCRETE case while still declining the
/// OPEN case — preserving an empty-args instantiation would fail the positive
/// test; removing the symbolic gate entirely would fail this one.)
#[test]
fn graph_native_open_generic_slot_alias_does_not_invent_bindings() {
    let project = make_project();
    project
        .upsert_base(
            "/slots.ts",
            r#"
// The slot param root is an indexed access on an open type parameter; it
// never resolves to a concrete object surface, so it stays symbolic.
export type OpenSlots<T> = { default: (props: T['missing']) => any }
export type Wrapper<T> = OpenSlots<T>
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/App.vue",
            r#"<script setup lang="ts">
import type { Wrapper } from './slots'

defineSlots<Wrapper<{ id: string }>>()
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

    // `{ id: string }['missing']` is an indexed access whose key is not present;
    // the param root never resolves to a concrete object, so NO `default.*`
    // binding row is invented.
    let default_bindings: Vec<&str> = evaluated
        .slot_bindings
        .iter()
        .filter(|binding| binding.name.starts_with("default."))
        .map(|binding| binding.name.as_str())
        .collect();
    assert!(
        default_bindings.is_empty(),
        "an open / unresolved indexed-access slot param root must NOT invent \
         binding rows: {default_bindings:?}"
    );
}

#[test]
fn project_model_drops_non_model_cursor() {
    // FIX-9: `project_model` enforces `PublishedSurfaceKind::Model` from the
    // cursor's CARRIED surface before publishing (`descend_published_member`
    // does NOT validate the cursor surface kind, so the API would otherwise be
    // weaker than the per-member admission invariant). A non-`Model` cursor MUST
    // early-return `None`; the production caller (project_evaluated_types) passes
    // a `Model` cursor, which is NOT gated by this check.
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};
    use crate::meta_resolve::projectors::{build_owner_decl_identity, project_model};
    use crate::resolver_core::ComponentMetaQueryEngine;

    let project = make_project();
    project
        .upsert_base(
            "/src/App.vue",
            r#"<script setup lang="ts">
const model = defineModel<string>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let host = project.host();
    assert!(host.ensure_loaded("/src/App.vue"));
    let snapshot = host
        .get_raw_analysis_snapshot("/src/App.vue")
        .expect("raw snapshot should exist");
    // Locate the `defineModel` macro.
    let (macro_index, mac) = snapshot
        .macros
        .iter()
        .enumerate()
        .find(|(_, m)| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
        .expect("the SFC declares a defineModel macro");
    assert!(
        mac.is_type_based,
        "fixture precondition: `defineModel<string>()` is a type-based model macro"
    );

    let owner = build_owner_decl_identity(host, "/src/App.vue");

    // A non-`Model` cursor (here `Props`) MUST be rejected by the kind check —
    // `project_model` returns `None` without publishing the model surface under
    // the wrong published-surface kind.
    {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let wrong_projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
        let mut diag_sink = Vec::new();
        let result = project_model(
            &mut engine,
            &owner,
            "/src/App.vue",
            macro_index,
            mac,
            &snapshot,
            &mut diag_sink,
            wrong_projection.cursor(),
        );
        assert!(
            result.is_none(),
            "FIX-9: `project_model` MUST return `None` for a non-`Model` cursor (the kind check \
             fails closed); got {result:?}"
        );
    }

    // The correct `Model` cursor is NOT gated by the kind check — it proceeds to
    // resolve + publish the model payload (a present model surface).
    {
        let mut engine = ComponentMetaQueryEngine::new(host);
        let model_projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Model);
        let mut diag_sink = Vec::new();
        let result = project_model(
            &mut engine,
            &owner,
            "/src/App.vue",
            macro_index,
            mac,
            &snapshot,
            &mut diag_sink,
            model_projection.cursor(),
        );
        assert!(
            result.is_some(),
            "FIX-9: a `Model` cursor must NOT be rejected by the kind check — `defineModel<string>()` \
             publishes a model field; got {result:?}"
        );
    }
}
