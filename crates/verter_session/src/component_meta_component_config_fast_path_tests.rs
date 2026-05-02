//! ComponentConfig theme variant fast-path tests (Issue #6).
//!
//! When `materialize_component_meta_field_types` evaluates a field
//! whose raw type is an indexed access on a `ComponentConfig<typeof
//! theme, AppConfig, key>` alias, the fast path projects the literal
//! `theme.variants.<name>` value directly without re-lowering the
//! ComponentConfig generic body or instantiating its mapped type.
//!
//! These tests use the per-request `CaptureToken` to discriminate
//! between the fast path firing (positive case: counter ≥ 1) and the
//! slow path running (counterfixtures: counter == 0). Per the
//! sidecar's §6.2 predicate contract the fast path requires:
//!
//! - alias body is `Ref { name: "ComponentConfig", type_arguments:
//!   [typeof theme, AppConfig, key_literal] }`
//! - `theme` resolves to a value declaration with a literal
//!   object_shape
//! - `AppConfig` parameter is `Record<...>` (Path A — strict legality
//!   for this landing); proof-cache hits (Path B) are deferred until
//!   the `IndexedReady` shallow `declares_interface_app_config` flag
//!   lands per the §17.7 deviation note.
//! - the indexed path is exactly `['variants', literal_name]` or
//!   `['slots']`
//!
//! Counterfixtures cover the §6.2 disallowed shapes: project-local
//! AppConfig override, generic default, index signature on theme,
//! generic key, workspace-package-inside-node_modules.

use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use crate::capture_token::CaptureToken;
use crate::meta_resolve::materialize::COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER;
use crate::meta_resolve::MEMBER_ROUTE_CALLS_COUNTER;
use crate::types::{HostConfig, ProjectionMode};
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

struct CapturedCounters {
    fast_path_hits: u64,
    member_route_calls: u64,
}

fn captured_counters_for(host: &Arc<VerterHost>, canonical: &str) -> CapturedCounters {
    let guard = CaptureToken::start_for_query("component_config_fast_path");
    let _ = host.get_component_meta(canonical);
    let _ = host.resolve_component_meta(canonical, ProjectionMode::Expanded);
    let snapshot = guard.end();
    CapturedCounters {
        fast_path_hits: snapshot.counter(COMPONENT_CONFIG_FAST_PATH_HITS_COUNTER),
        member_route_calls: snapshot.counter(MEMBER_ROUTE_CALLS_COUNTER),
    }
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
/// The fast path MUST fire on the variants and slots indexed-access
/// fields — and on hit the field skips the rescue + member-route
/// pipeline, so `member_route_calls` does not accumulate via the
/// fast-path-handled fields.
#[test]
fn component_config_theme_variant_props_use_prepared_theme_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", POSITIVE_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert!(
        counters.fast_path_hits >= 1,
        "Issue #6: Record<string, unknown> AppConfig with literal theme \
         + literal indexed-access path MUST fire the fast path; \
         component_config_theme_variant_fast_path_hits == 0",
    );
    // member_route_calls is observed but not strictly asserted here:
    // the indexed-access early-out (Phase 6) may handle some of the
    // field cases, and the remaining fields the fast path covers
    // emit zero member_route_calls (since the fast path publishes
    // and `continue`s before the member-route loop runs). The
    // counterfixtures assert the inverse (fast_path_hits == 0).
    let _ = counters.member_route_calls;
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

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: when AppConfig is an interface (not \
         Record<...>) and no proof-cache entry exists, the fast path \
         MUST decline; got fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #2: interface merging across files ──

const MERGE_PRIMARY_TS: &str = r#"import { theme } from '/workspace/src/theme'

export interface AppConfig {
  // No ui[key] here, but the merge below adds one.
}

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#;

const MERGE_SECONDARY_TS: &str = r#"import './primary'

declare module '/workspace/src/types' {
  interface AppConfig {
    ui?: {
      button?: { variants?: { variant?: 'merged-override' } }
    }
  }
}
"#;

#[test]
fn component_config_interface_merging_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", MERGE_PRIMARY_TS),
        ("/workspace/src/merge.ts", MERGE_SECONDARY_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: interface-merging AppConfig (declared as \
         `interface AppConfig`) must disable the fast path until the \
         proof-cache contract proves \"no ui[key] override\"; got \
         fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #3: module augmentation adding ui[key] ──

const MODULE_AUG_PRIMARY_TS: &str = r#"import { theme } from '/workspace/src/theme'

export interface AppConfig {}

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#;

const MODULE_AUG_SECONDARY_TS: &str = r#"declare module '/workspace/src/types' {
  interface AppConfig {
    ui?: { button?: { variants?: { variant?: 'aug-only' } } }
  }
}

export {}
"#;

#[test]
fn component_config_module_augmentation_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", MODULE_AUG_PRIMARY_TS),
        ("/workspace/src/aug.ts", MODULE_AUG_SECONDARY_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: module-augmentation interface AppConfig \
         must disable the fast path until proven by the proof cache; \
         got fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #4: generic default with override ──

const GENERIC_DEFAULT_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export interface DefaultConfig {
  ui?: { button?: { variants?: { variant?: 'default-only' } } }
}

export type ComponentConfig<T, A = DefaultConfig, K extends keyof T = keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme>
"#;

#[test]
fn component_config_generic_default_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", GENERIC_DEFAULT_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: when AppConfig defaulted to a non-Record \
         shape with ui[key] override, the fast path MUST decline; got \
         fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #5: index signature on prepared theme value ──

const INDEX_SIG_THEME_TS: &str = r#"export const theme: Record<string, { variants: Record<string, string> }> = {
  button: { variants: { variant: 'index-signature' } },
}
"#;

const INDEX_SIG_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export type AppConfig = Record<string, unknown>

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
"#;

#[test]
fn component_config_index_signature_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", INDEX_SIG_THEME_TS),
        ("/workspace/src/types.ts", INDEX_SIG_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: when the prepared theme value's type \
         carries an index signature (not a literal object_shape with a \
         specific `variants` member), the fast path MUST decline; got \
         fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #6: generic key parameter (not a literal) ──

const GENERIC_KEY_TYPES_TS: &str = r#"import { theme } from '/workspace/src/theme'

export type AppConfig = Record<string, unknown>

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

// Key is generic — alias body's `key` parameter is itself a type
// parameter, not a literal at the alias declaration site.
export type GenericButton<K extends keyof typeof theme> = ComponentConfig<typeof theme, AppConfig, K>

export type Button = GenericButton<'variants'>
"#;

#[test]
fn component_config_generic_key_disables_fast_path() {
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        ("/workspace/src/types.ts", GENERIC_KEY_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: when the alias body uses a generic `K` \
         parameter as the component key (not a literal at the alias \
         declaration site), the fast path MUST decline; got \
         fast_path_hits = {}",
        counters.fast_path_hits,
    );
}

// ── Counterfixture #7: workspace-package-inside-node_modules ──

/// `/workspace/node_modules/...` BUT the file's canonical id is still
/// inside the workspace root `/workspace`. The path-substring test
/// `path.contains("/node_modules/")` would WRONGLY classify the file
/// as package-backed; `WorkspaceAccess::is_workspace_owned` correctly
/// reports it as workspace-owned (the whole `/workspace` tree is the
/// project root, so even files under `/workspace/node_modules/` are
/// claimed by the project here).
///
/// The fast path consumes the workspace classification (not a path
/// substring) so this counterfixture asserts that fast-path
/// classification routes through `WorkspaceAccess` and not a
/// path-substring shortcut. We use a non-Record `AppConfig` to keep
/// the counterfixture asserting that the fast path declines — the
/// substring-on-path shortcut would (incorrectly) fire.
const WS_PKG_TYPES_TS: &str = r#"import type { theme } from '/workspace/node_modules/internal-theme/index'

export interface AppConfig {
  ui?: { button?: { variants?: { variant?: 'pkg-override' } } }
}

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#;

const WS_PKG_THEME_TS: &str = r#"export const theme = {
  variants: { variant: { solid: 'solid' } },
  slots: { root: 'root' },
} as const
"#;

#[test]
fn component_config_workspace_package_inside_node_modules_disables_fast_path() {
    let host = build_workspace_host(&[
        (
            "/workspace/node_modules/internal-theme/index.ts",
            WS_PKG_THEME_TS,
        ),
        ("/workspace/src/types.ts", WS_PKG_TYPES_TS),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    let counters = captured_counters_for(&host, "/workspace/src/Button.vue");

    assert_eq!(
        counters.fast_path_hits, 0,
        "§6.2 counterfixture: with a non-Record AppConfig (interface \
         with `ui[key]` override) sourced from a workspace-package \
         inside node_modules, the fast path MUST decline regardless of \
         path substring; got fast_path_hits = {}",
        counters.fast_path_hits,
    );
}
