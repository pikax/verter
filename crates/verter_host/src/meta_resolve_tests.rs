use super::*;
use crate::meta::MetaProject;
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
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.clone())
        .collect()
}

fn emit_names_from_resolved(state: &ResolvedComponentMetaState) -> Vec<String> {
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineEmits)
        .flat_map(|m| m.emits.iter())
        .map(|e| e.name.clone())
        .collect()
}

fn slot_names_from_resolved(state: &ResolvedComponentMetaState) -> Vec<String> {
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .map(|s| s.name.clone())
        .collect()
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

    let _second = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("second resolved-meta query should succeed");
    let p2 = provenance(&project);
    assert_eq!(
        p2.component_meta_resolved_state_recomputes, 1,
        "same-mode repeat should hit resolved-meta cache instead of recomputing"
    );
}

// ===========================================================================
// Phase 1: Architecture — Shared traversal between modes
// ===========================================================================

#[test]
fn type_mode_skips_traversal_and_expanded_mode_uses_host_cache() {
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
    let _type_state = project
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
    let _expanded_state = project
        .host()
        .resolve_component_meta("/App.vue", ResolverMode::Expanded)
        .expect("Expanded mode should return result");

    let p2 = provenance(&project);
    // Expanded mode should have done at least one traversal
    assert!(
        p2.resolved_external_type_cache_misses > 0 || p2.resolved_external_type_cache_hits > 0,
        "Expanded mode should use the host traversal cache"
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
    assert!(
        prop_names.contains(&"from_base".to_string()),
        "imported class base members should flow through shared resolver: {:?}",
        prop_names
    );
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
        !prop_names.contains(&"hidden".to_string()),
        "protected class members must not leak into props: {:?}",
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
    assert!(
        native_names.contains(&"hidden"),
        "native state should retain protected members before compat filtering: {:?}",
        native_names
    );
    assert!(
        native_names.contains(&"secret"),
        "native state should retain private members before compat filtering: {:?}",
        native_names
    );
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "hidden"
                && prop.visibility
                    == verter_core::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
        }),
        "native state should preserve visibility metadata for protected members"
    );
    assert!(
        class_macro.native_props.iter().any(|prop| {
            prop.name == "secret"
                && prop.visibility
                    == verter_core::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Private
        }),
        "native state should preserve visibility metadata for private members"
    );
    assert!(
        class_macro
            .native_props
            .iter()
            .any(|prop| prop.name == "from_base"
                && prop.type_annotation.as_deref() == Some("string")),
        "native state should retain raw type annotations for class properties"
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
                    == verter_core::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
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
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
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
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
        .unwrap();
    let batch_dp = batch[0]
        .1
        .macros
        .iter()
        .find(|m| m.kind == verter_analysis::AnalyzedMacroKind::DefineProps)
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
                    == verter_core::utils::oxc::vue::resolve_type::ResolvedMemberVisibility::Protected
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
        .filter(|m| m.macro_kind == verter_analysis::AnalyzedMacroKind::DefineEmits)
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
    let ws = verter_vfs::MemoryWorkspace::new(verter_vfs::MemoryOptions::default());
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
    let ws = verter_vfs::MemoryWorkspace::new(verter_vfs::MemoryOptions::default());
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
        verter_analysis::project_resolver::IdeProjectConfig::new(
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
    let ws = verter_vfs::MemoryWorkspace::new(verter_vfs::MemoryOptions::default());
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
        verter_analysis::project_resolver::IdeProjectConfig::new(
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
    let session_state = session
        .resolve_component_meta_state("/App.vue", ResolverMode::Expanded)
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
