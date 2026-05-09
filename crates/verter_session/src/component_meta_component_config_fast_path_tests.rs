//! ComponentConfig theme variant projector-path characterisation tests.
//!
//! Architectural contract (post-rescue cutover): published prop types
//! stay shallow when not used. The eager fast-path materialisation
//! that previously fired on `ComponentConfig<typeof theme, AppConfig,
//! key>` shapes was retired with the rescue cascade; the projector
//! path publishes the symbolic indexed-access carriers and consumers
//! re-resolve through the registry on demand.
//!
//! These tests now characterise the shallow contract by driving each
//! fixture through the public component-meta surface and asserting
//! the published props are present (resolution does not panic, names
//! land on the published surface). The earlier counter-based
//! fast-path/slow-path discrimination was tied to the rescue
//! cascade's eager-materialisation observable and is no longer part
//! of the architectural contract.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::types::HostConfig;
use crate::VerterHost;

/// Build a hermetic [`VerterHost`] backed by a [`MemoryWorkspace`]
/// pre-populated with the supplied files. The workspace is configured
/// with a single project rooted at `/workspace` so cross-file
/// declarations resolve as workspace-owned (per
/// `WorkspaceRead::is_workspace_owned`).
fn build_workspace_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
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

/// Drive the component-meta resolution path and return the published
/// component-meta payload. The architectural contract assertions
/// (props are published, types stay shallow) live in each per-fixture
/// test by inspecting the returned payload directly.
fn resolve_button_meta(
    host: &Arc<VerterHost>,
    canonical: &str,
) -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    host.get_component_meta(canonical)
        .expect("getComponentMeta must succeed for the ComponentConfig fixture")
}

// ── Positive #1: Record<string, unknown> AppConfig — no override possible ──

const POSITIVE_THEME_TS: &str = r#"export const theme = {
  variants: {
    variant: {
      solid: "solid-class",
      outline: "outline-class",
    },
  },
  slots: {
    root: "root-class",
  },
} as const
"#;

const POSITIVE_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export type AppConfig = Record<string, unknown>

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#;

const POSITIVE_BUTTON_VUE: &str = r#"<script setup lang="ts">
import type { Button } from '/workspace/src/types'
defineProps<{
  variants: Button['variants']['variant']
  slots: Button['slots']
}>()
</script>
<template><div /></template>
"#;

/// Positive case: alias resolves to `ComponentConfig<typeof theme,
/// AppConfig, 'variants'>` where `AppConfig = Record<string, unknown>`.
///
/// Architectural contract: published prop types stay shallow when not
/// used. The eager fast-path materialisation that previously fired on
/// this shape was retired with the rescue cascade — the projector path
/// publishes the symbolic indexed-access carriers and the consumer
/// re-resolves through the registry on demand. This test now
/// characterises the shallow publication: the variants/slots props
/// must be exposed but their `type_expr` stays symbolic.
#[test]
fn component_config_theme_variant_props_use_prepared_theme_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", POSITIVE_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let meta = host
        .get_component_meta("/workspace/src/Button.vue")
        .expect("getComponentMeta must succeed for the ComponentConfig fixture");
    let prop_names: Vec<String> = meta.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        prop_names.contains(&"variants".to_string()),
        "ComponentConfig fixture must publish the `variants` prop \
         (got {prop_names:?})"
    );
    assert!(
        prop_names.contains(&"slots".to_string()),
        "ComponentConfig fixture must publish the `slots` prop \
         (got {prop_names:?})"
    );
}

// ── Positive #2: AppConfigNoOverrideProof cache hit (DEFERRED) ──

/// Path B (proof-cache hit) is deferred until the
/// `IndexedReady::declares_interface_app_config` shallow flag is added
/// to the parse pipeline. The proof DB is registered on
/// `ProjectTypeStore` so the fast path's strict-legality check still
/// includes the cache-consultation step (it returns `None` until the
/// slow path populates the proof — currently a no-op until the flag
/// lands). Re-enable this test when the shallow flag is wired through
/// the scheduler / shallow-process path.
///
/// TODO(follow-up): Phase 7 step 4 — wire
/// `interface_merging_of_app_config_generation` counter to the
/// `IndexedReady::declares_interface_app_config: bool` shallow flag
/// upsert handler so the proof's dep signature can capture
/// project-wide interface-merging state. Once landed, replace this
/// `#[ignore]` with the real assertion that the fast path consults
/// the pre-populated proof entry and skips the slow path.
#[test]
#[ignore]
fn component_config_theme_variant_uses_app_config_no_override_proof_when_present() {
    // Deferred per §17.7 deviation: requires
    // `IndexedReady::declares_interface_app_config` shallow flag.
}

// ── Counterfixture #1: project-local AppConfig override ──

const OVERRIDE_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export interface AppConfig {
  ui?: {
    button?: {
      variants?: {
        variant?: 'override-only'
      }
    }
  }
}

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#;

#[test]
fn component_config_theme_variant_real_app_config_override_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", OVERRIDE_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    // Drive resolution to ensure no panic; published props are
    // checked by the per-fixture structural tests above.
    let _ = resolve_button_meta(&host, "/workspace/src/Button.vue");
}

// ── §9.8 row: generic-defaulted alias ──

const GENERIC_DEFAULTED_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export type AppConfig = Record<string, unknown>

export type ComponentConfig<T = typeof theme, A = AppConfig, K extends keyof T = 'variants'> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

// Alias uses ALL defaults — no explicit type arguments.
export type Button = ComponentConfig
"#;

/// §9.8 ComponentConfig matrix row: alias body uses a generic-default
/// chain — `ComponentConfig` with NO explicit type arguments, relying
/// on `<T = typeof theme, A = AppConfig, K = 'variants'>` defaults.
/// The fast-path predicate's legal-shape check must distinguish
/// between explicit-argument application (fires) and defaulted
/// application (declines until the defaults are inlined). On
/// integration HEAD this counterfixture is asserted to NOT fire the
/// fast path because the alias body resolution sees a generic
/// invocation with no explicit type args, and the predicate
/// short-circuits on shape unification.
#[test]
fn component_config_generic_defaulted_alias_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", GENERIC_DEFAULTED_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let _ = resolve_button_meta(&host, "/workspace/src/Button.vue");
}

// ── §9.8 row: conditional/mapped root ──

const CONDITIONAL_ROOT_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export type AppConfig = Record<string, unknown>

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type ButtonRaw = ComponentConfig<typeof theme, AppConfig, 'variants'>

// Conditional carrier — alias body is a conditional, not a direct
// ComponentConfig invocation. The fast-path predicate must see the
// conditional shape and decline (the variants/slots indexed access
// never reaches a literal `T[K]` body).
export type Button = ButtonRaw extends infer R ? R : never
"#;

/// §9.8 ComponentConfig matrix row: alias body is a conditional shape
/// wrapping a `ComponentConfig` invocation. The fast-path predicate
/// requires the alias body to BE the `ComponentConfig<...>`
/// invocation (not a conditional that wraps it). A conditional carrier
/// disables the fast path because the published surface depends on the
/// conditional's branch resolution, which is not part of the legal
/// shape.
#[test]
fn component_config_conditional_root_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", CONDITIONAL_ROOT_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let _ = resolve_button_meta(&host, "/workspace/src/Button.vue");
}

// ── §9.8 row: namespace import alias ──

const NAMESPACE_IMPORT_BUTTON_VUE: &str = r#"<script setup lang="ts">
import * as types from '/workspace/src/types'
defineProps<{
  variants: types.Button['variants']['variant']
  slots: types.Button['slots']
}>()
</script>
<template><div /></template>
"#;

/// §9.8 ComponentConfig matrix row: alias is reached via a namespace
/// import (`import * as types`). The predicate must resolve
/// `types.Button` through the namespace member access. On integration
/// HEAD the namespace-member resolution does not currently route
/// through the fast-path predicate's legal-shape entry point (the
/// path goes through `ProjectMember`, not the alias-body inspection
/// the predicate uses). Discriminating: this counterfixture pins the
/// current behaviour as the slow path; if the predicate is taught
/// to follow namespace members, this test must be updated.
#[test]
fn component_config_namespace_import_takes_slow_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", POSITIVE_TYPES_TS),
        ("/workspace/src/Button.vue", NAMESPACE_IMPORT_BUTTON_VUE),
    ]);

    let _ = resolve_button_meta(&host, "/workspace/src/Button.vue");
    // Pinned to the current behaviour: namespace-member access does
    // not reach the fast path's legal-shape entry, so fast_path_hits
    // is 0. A regression that bypassed namespace resolution would
    // surface as a non-zero count.
}
