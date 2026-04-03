use super::*;
use crate::meta::MetaProject;
use crate::resolver_core::ComponentMetaRequestHost;
use crate::types::{HostConfig, ResolverMode};
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

fn clear_legacy_cached_resolved_state(project: &MetaProject, canonical: &str, mode: ResolverMode) {
    #[cfg(feature = "scheduler")]
    {
        if let Some(mut entry) = project.host().compile_cache.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }

    #[cfg(not(feature = "scheduler"))]
    {
        let mut files = crate::shared::write_lock(&project.host().files);
        if let Some(entry) = files.get_mut(canonical) {
            entry.cached_resolved_meta.remove(&mode);
        }
    }
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
        ResolverMode::Expanded,
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
  escapeKeyDown: [event: KeyboardEvent]
  pointerDownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent]
  interactOutside: [event: Event]
  openAutoFocus: [event: Event]
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
  escapeKeyDown: [event: KeyboardEvent]
  pointerDownOutside: [event: PointerEvent]
  focusOutside: [event: FocusEvent]
  interactOutside: [event: Event]
  openAutoFocus: [event: Event]
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
        .resolve_component_meta("/App.vue", ResolverMode::Type)
        .expect("Type mode should return a result for an existing file");

    assert_eq!(state.mode, ResolverMode::Type);

    // Type mode: resolved_macros should carry identity info but NOT expanded props
    assert!(
        !state.resolved_macros.is_empty(),
        "Type mode should still identify macro type deps"
    );
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.is_empty(),
        "Type mode must NOT materialize expanded prop shapes, got: {:?}",
        prop_names
    );

    // Type mode: no evaluated types
    assert!(
        state.evaluated_types.is_none(),
        "Type mode must NOT compute evaluated types"
    );

    // Type mode: no type registry
    assert!(
        state.resolved_type_registry.is_empty(),
        "Type mode must NOT populate type-registry entries"
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return a result for an existing file");

    assert_eq!(state.mode, ResolverMode::Expanded);

    // Expanded mode: materialized props
    let prop_names = prop_names_from_resolved(&state);
    assert!(
        prop_names.contains(&"a".to_string()),
        "Expanded mode should materialize prop 'a', got: {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"b".to_string()),
        "Expanded mode should materialize prop 'b', got: {:?}",
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
        .resolve_component_meta("/App.vue", ResolverMode::Type);
    let _expanded_result = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded);
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

    // Call Type mode first, then Expanded
    let type_state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Type)
        .expect("Type mode should return result");
    let expanded_state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

    // Type entry must NOT satisfy Expanded
    assert!(
        prop_names_from_resolved(&type_state).is_empty(),
        "Type mode result must have no expanded props"
    );
    assert!(
        !prop_names_from_resolved(&expanded_state).is_empty(),
        "Expanded mode result must have expanded props"
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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

    clear_legacy_cached_resolved_state(&project, "/App.vue", ResolverMode::Expanded);

    let _second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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

    // Type mode should NOT perform the expensive external type traversal
    let type_state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Type)
        .expect("Type mode should return result");

    let p1 = provenance(&project);
    assert_eq!(
        p1.resolved_external_type_cache_misses, 0,
        "Type mode should NOT call resolve_external_type_from_loaded_files"
    );
    assert_eq!(
        p1.resolved_external_type_cache_hits, 0,
        "Type mode should NOT touch the host traversal cache"
    );

    // Expanded mode performs the traversal
    let expanded_state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

    let p2 = provenance(&project);
    assert!(
        prop_names_from_resolved(&type_state).is_empty(),
        "Type mode result must not include expanded props"
    );
    assert!(
        prop_names_from_resolved(&expanded_state).contains(&"a".to_string()),
        "Expanded mode should materialize imported props"
    );
    assert!(
        p2.resolver_node_cache_misses > p1.resolver_node_cache_misses,
        "Expanded mode should perform additional resolver-owned cache work"
    );

    // Second Expanded call should hit the resolved-meta cache (no recompute)
    let recomputes_before = p2.component_meta_resolved_state_recomputes;
    let _third = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

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

    // Expanded mode should still return a result (best-effort)
    let state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded);
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded);
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("first resolve should succeed");
    let second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
    assert!(
        after_second.node_cache_hits > after_first.node_cache_hits,
        "second resolve should reuse the runtime symbol cache after an owner-only change, before={:?} after={:?}",
        after_first,
        after_second
    );
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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

    let seeded = host
        .materialize_imported_dependency_state_in_view("/src/types.ts", None)
        .expect("types dependency should seed imported state");
    let decl = seeded
        .prepared_type_decls
        .get("Props")
        .cloned()
        .expect("seeded dependency should expose Props through the prepared declaration cache");

    {
        let mut cache = host.imported_dependency_cache.lock();
        let entry = cache
            .get_mut("/src/types.ts")
            .expect("types dependency should stay cached");
        Arc::make_mut(entry).snapshot = None;
    }

    let materialized =
        solve_component_meta_registry_decl_in_view(&host, "/src/types.ts", "Props", None)
            .expect("solver-backed registry decl materialization should use cached prepared state");

    assert_eq!(
        materialized, decl.body,
        "registry decl solving should stay shallow when the imported cache does not own a snapshot yet",
    );
    assert!(
        host.clone_current_imported_dependency_entry("/src/types.ts", None)
            .and_then(|entry| entry.snapshot.clone())
            .is_none(),
        "registry decl solving must not bounce into raw snapshot building for imported files",
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
    let store_view = project.host().resolver_store_view();

    let resolved = resolve_jsdoc_tag_type(
        project.host(),
        "/types.ts",
        "DocType",
        &mut tracked_deps,
        Some(&store_view),
    )
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
  change: [value: string]
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/workspace/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/project/App.vue", ResolverMode::Expanded);

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
        .resolve_component_meta("/project/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/project/App.vue", ResolverMode::Expanded)
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

    // After the refactor, get_component_meta should always use Expanded mode
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        crate::meta_resolve::ResolvedDeclarationKind::Interface
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded);

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
// Edge case: Type mode cache invalidation on dependency change
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

    // First Type mode call — populates cache
    let state1 = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Type)
        .expect("first Type mode should return result");
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

    // Second Type mode call — cache should be invalidated by dep change
    project.host().provenance().reset();
    let state2 = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Type)
        .expect("second Type mode should return result");

    let p = provenance(&project);
    // Assert+: resolved state was recomputed (not served from stale cache)
    assert_eq!(
        p.component_meta_resolved_state_recomputes, 1,
        "Type mode cache should invalidate when dependency changes, got recomputes={}",
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
        .resolve_component_meta("/missing.vue", ResolverMode::Expanded);
    assert!(
        result.is_none(),
        "resolve_component_meta should return None for a missing file"
    );

    let result_type = project
        .host()
        .resolve_component_meta("/missing.vue", ResolverMode::Type);
    assert!(
        result_type.is_none(),
        "resolve_component_meta(Type) should also return None for a missing file"
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
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
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
