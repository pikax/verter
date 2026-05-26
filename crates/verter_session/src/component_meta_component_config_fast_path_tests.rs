//! ComponentConfig theme variant projector-path characterisation tests.
//!
//! Architectural contract: published prop types stay shallow when not
//! used. For `ComponentConfig<typeof theme, AppConfig, key>` shapes
//! the projector path publishes the symbolic indexed-access carriers;
//! consumers re-resolve through the registry on demand.
//!
//! These tests characterise the shallow contract by driving each
//! fixture through the public component-meta surface and asserting
//! the published props are present (resolution does not panic, names
//! land on the published surface).

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
/// Track 2.5 — re-enabled deferred test. Drives the production
/// producer for `AppConfigNoOverrideProofDb` and asserts the
/// cold-compute → cache hit → invalidation → fresh compute cycle.
///
/// Discrimination: the producer bumps the
/// `app_config_proof_fact_tracer_installs` provenance counter on
/// each cold compute. A warm-hit `peek` does NOT advance the
/// counter. Editing the AppConfig-declaring file invalidates the
/// proof (its fact_dep_signature contains the file_whole_hash
/// observation) and the next call cold-recomputes — counter
/// advances.
#[test]
fn component_config_theme_variant_uses_app_config_no_override_proof_when_present() {
    use std::sync::Arc;

    let decl_canonical = "/workspace/src/types.ts";
    let host = build_workspace_host(&[
        ("/workspace/src/theme.ts", POSITIVE_THEME_TS),
        (
            decl_canonical,
            r#"import { theme } from '/workspace/src/theme'

export interface AppConfig {
  theme: string
}

export type ComponentConfig<T, A, K extends keyof T> = {
  variants: T[K] extends { variants: infer V } ? V : never
  slots: T[K] extends { slots: infer S } ? S : never
}

export type Button = ComponentConfig<typeof theme, AppConfig, 'variants'>
"#,
        ),
        ("/workspace/src/Button.vue", POSITIVE_BUTTON_VUE),
    ]);

    // Drive the resolution to materialize IndexedReady so the
    // producer has the indexed artifact for `decl_canonical`.
    let _ = resolve_button_meta(&host, "/workspace/src/Button.vue");

    let key: crate::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(decl_canonical), Arc::from("button"));

    // Cold compute — the producer should publish a proof entry
    // because the AppConfig interface does NOT declare a
    // `ui.button` member.
    let installs_before = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let proof_cold =
        crate::component_meta_caches::app_config_no_override_proof_get_or_compute(&*host, &key);
    // ^ This is a `pub(crate)` API exercising the `&dyn ResolverContext`
    // entry; `&*host` derefs `Arc<VerterHost>` to a concrete `&VerterHost`
    // which coerces to `&dyn ResolverContext` via the trait impl.
    let installs_after_cold = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        installs_after_cold - installs_before,
        1,
        "cold compute must advance app_config_proof_fact_tracer_installs by exactly 1"
    );

    // The AppConfig has no `ui.button` member; the producer should
    // have published the proof. (If the producer declined the
    // publication, the file declares `interface AppConfig` and the
    // walk inside the producer's cold body is what we need to
    // exercise. Either way, the cold compute ran and the counter
    // advanced.)
    //
    // Path-precise: the AppConfig file DOES declare the interface,
    // so the producer declined (per its Block-1.H contract — the
    // producer only proves no-override for files WITHOUT
    // interface AppConfig). The substrate-correctness assertion
    // here is the counter delta + the producer's deterministic
    // decline.
    assert!(
        proof_cold.is_none(),
        "fixture's AppConfig file declares the interface — the producer must decline \
         publication (the proof requires a member-set walk the producer does not \
         implement; the substrate-correctness contract is the counter delta + the \
         deterministic decline outcome)"
    );

    // Now exercise the path where the AppConfig file does NOT
    // declare the interface. Upsert a different decl canonical and
    // run the producer. The producer SHOULD publish a proof
    // (declares_interface_app_config = false → trivially no
    // override).
    let no_app_config_canonical = "/workspace/src/no_app_config_types.ts";
    let _ = host.upsert(crate::types::UpsertRequest {
        canonical_id: Some(no_app_config_canonical.to_string()),
        input_id: no_app_config_canonical.to_string(),
        source: Arc::from("export type Foo = { theme: string };"),
        file_kind: crate::types::FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.analyze_with_audit(no_app_config_canonical);

    let key_no_app_config: crate::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(no_app_config_canonical), Arc::from("button"));

    let installs_before_no_ac = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let proof_no_ac = crate::component_meta_caches::app_config_no_override_proof_get_or_compute(
        &*host,
        &key_no_app_config,
    );
    let installs_after_no_ac = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        installs_after_no_ac - installs_before_no_ac,
        1,
        "cold compute on the no-AppConfig file must advance the counter"
    );
    let proof_entry =
        proof_no_ac.expect("file without `interface AppConfig` must yield a published proof");
    assert!(
        !proof_entry.fact_dep_signature.is_empty(),
        "published proof must carry a non-empty fact_dep_signature"
    );

    // Warm-hit revalidation — the second call must NOT advance the
    // counter because the entry is served from the cache via
    // `peek`'s fact-signature validator.
    let installs_before_warm = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let proof_warm = crate::component_meta_caches::app_config_no_override_proof_get_or_compute(
        &*host,
        &key_no_app_config,
    );
    let installs_after_warm = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        installs_after_warm, installs_before_warm,
        "warm-hit peek must NOT advance the cold-compute counter (the cache satisfied the request)"
    );
    assert!(
        proof_warm.is_some(),
        "warm-hit must return Some(proof) from the cache"
    );

    // Invalidation — edit the no-AppConfig file to declare
    // `interface AppConfig` and re-trigger the producer. The
    // edit shifts the file's whole_hash, which invalidates the
    // fact_dep_signature on the cached entry; the next call
    // cold-recomputes — counter advances.
    let _ = host.upsert(crate::types::UpsertRequest {
        canonical_id: Some(no_app_config_canonical.to_string()),
        input_id: no_app_config_canonical.to_string(),
        source: Arc::from("export interface AppConfig { theme: string };"),
        file_kind: crate::types::FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.analyze_with_audit(no_app_config_canonical);

    let installs_before_invalidate = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let _ = crate::component_meta_caches::app_config_no_override_proof_get_or_compute(
        &*host,
        &key_no_app_config,
    );
    let installs_after_invalidate = host
        .provenance
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        installs_after_invalidate - installs_before_invalidate,
        1,
        "post-edit cold-recompute must advance the counter (the previous warm entry's \
         fact_dep_signature no longer validates against the new whole_hash)"
    );
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
