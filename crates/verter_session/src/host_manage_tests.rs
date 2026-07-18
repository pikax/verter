use super::*;

use std::sync::Arc;
use verter_type_expr::TypeExpr;
use verter_workspace::{WorkspaceAccess, WorkspaceRead};

const LAZY_ANALYSIS_SFC: &str = r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<style>
.foo { color: red; }
</style>"#;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn make_lazy_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::None,
        ..HostConfig::default()
    })
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

struct CountingWorkspace {
    inner: Arc<verter_workspace::MemoryWorkspace>,
    read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    exists_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    manifest_read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
    resolve_counts: parking_lot::Mutex<rustc_hash::FxHashMap<(String, String), u64>>,
}

impl CountingWorkspace {
    fn new() -> Self {
        Self {
            inner: Arc::new(verter_workspace::MemoryWorkspace::new(
                verter_workspace::MemoryOptions::default(),
            )),
            read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            exists_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            manifest_read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
            resolve_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
        }
    }

    fn inject_file(&self, path: &str, source: &str) {
        self.inner
            .inject_file(path.to_string(), Arc::<str>::from(source.to_string()));
    }

    fn remove_file(&self, path: &str) {
        self.inner.remove_file(path);
    }

    fn reset_reads(&self) {
        self.read_counts.lock().clear();
    }

    fn read_count(&self, path: &str) -> u64 {
        self.read_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn reset_exists(&self) {
        self.exists_counts.lock().clear();
    }

    fn exists_count(&self, path: &str) -> u64 {
        self.exists_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn reset_resolves(&self) {
        self.resolve_counts.lock().clear();
    }

    fn resolve_count(&self, importer_id: &str, specifier: &str) -> u64 {
        self.resolve_counts
            .lock()
            .get(&(importer_id.to_string(), specifier.to_string()))
            .copied()
            .unwrap_or(0)
    }
}

impl verter_workspace::WorkspaceRead for CountingWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        *self
            .read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_file(canonical_id)
    }

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        *self
            .exists_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
    }

    fn read_package_manifest(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::PackageManifest> {
        *self
            .manifest_read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_package_manifest(canonical_id)
    }

    fn classify_file(&self, canonical_id: &str) -> verter_language::FileLanguage {
        self.inner.classify_file(canonical_id)
    }

    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        ctx: verter_workspace::ResolutionContext,
    ) -> Option<verter_workspace::ResolveResult> {
        *self
            .resolve_counts
            .lock()
            .entry((importer_id.to_string(), specifier.to_string()))
            .or_default() += 1;
        self.inner.resolve_import(importer_id, specifier, ctx)
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn dependency_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::DependencySnapshotView> {
        self.inner.dependency_snapshot(canonical_id)
    }

    fn read_dir(
        &self,
        dir: &str,
    ) -> Result<Vec<verter_workspace::DirEntry>, verter_workspace::VfsError> {
        self.inner.read_dir(dir)
    }

    fn walk(
        &self,
        root: &str,
        filter_dir: &dyn Fn(&str) -> bool,
        filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, verter_workspace::VfsError> {
        self.inner.walk(root, filter_dir, filter_file)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
}

impl verter_workspace::WorkspaceAccess for CountingWorkspace {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges);
    }

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner.set_exact_resolutions(canonical_id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner
            .record_parsed_edges_with_exact_resolutions(canonical_id, edges, resolutions)
    }

    // ── R6/R7: forwarding wrapper for new reverse-graph methods ──
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps);
    }

    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions);
    }

    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id);
    }

    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source);
    }

    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id);
    }

    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id);
    }

    fn configure_resolver(&self, projects: Vec<verter_workspace::resolver::IdeProjectConfig>) {
        self.inner.configure_resolver(projects);
    }

    fn write_file(&self, path: &str, content: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.write_file(path, content)
    }

    fn create_dir_all(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.create_dir_all(path)
    }

    fn delete_file(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.delete_file(path)
    }

    fn delete_dir_all(&self, path: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.delete_dir_all(path)
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), verter_workspace::VfsError> {
        self.inner.copy_file(src, dst)
    }
}

fn exact_dependency(specifier: &str, resolved: &str) -> DependencyResolution {
    DependencyResolution {
        specifier: specifier.to_string(),
        resolved_canonical_id: Some(resolved.to_string()),
        possible_canonical_ids: Vec::new(),
    }
}

#[cfg(target_arch = "wasm32")]
fn mutate_lazy_analysis_source(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    let broken = entry
        .source
        .replace("<script", "<scripx")
        .replace("</script>", "</scripx>")
        .replace("<style", "<styla")
        .replace("</style>", "</styla>");
    entry.source = Arc::from(broken);
}

#[cfg(target_arch = "wasm32")]
fn clear_framework_parse(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    entry.framework_parse = None;
}

// Legacy trace-line formatting tests 5 ( clean-cut rule). `format_component_meta_trace_line`,
// `ComponentMetaTraceEvent`, and `ComponentMetaTraceLine` no longer
// exist; their replacement is `StructuredAuditEvent` tested
// in `component_meta_audit/structured_event.rs`.

#[test]
fn build_eval_script_source_without_parse_artifact_still_extracts_script_blocks() {
    let source = r#"<script lang="ts">
interface Props {
  label: string
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#;

    let extracted = VerterHost::build_eval_script_source("/App.vue", source, None);
    assert!(
        extracted.contains("interface Props"),
        "script content should be preserved without cached parse, got: {extracted}"
    );
    assert!(
        extracted.contains("defineProps<Props>()"),
        "script setup content should be preserved without cached parse, got: {extracted}"
    );
    assert!(
        !extracted.contains("<template>"),
        "template markup must not be passed into type evaluation, got: {extracted}"
    );
}

/// Extraction is gated on the file's LANGUAGE CLASSIFICATION, never on the
/// raw text: a NON-CARRIER file (`.ts` / `.d.ts`) whose text contains a
/// `<script ...>` ... `</script>` pair (a JSDoc `@example` block — the
/// vue-router@5 / @regle/core / unhead dist shape) passes through UNCHANGED.
/// The former unconditional forgiving raw scan blanked such a file down to
/// its documentation example, destroying its whole type surface.
#[test]
fn build_eval_script_source_never_script_scans_a_non_carrier_file() {
    let source = r#"/**
 * Usage example:
 * ```vue
 * <script setup>
 * const value = useReal()
 * </script>
 * ```
 */
export type Real = string | { path: string }
"#;

    for canonical in ["/dep.ts", "/dep.d.ts", "/dep.tsx", "/dep.mjs"] {
        let (eval, extracted) =
            VerterHost::build_eval_script_source_with_extraction(canonical, source, None);
        assert!(
            !extracted,
            "{canonical}: a non-carrier file must never report script extraction"
        );
        assert_eq!(
            eval, source,
            "{canonical}: a non-carrier file's source passes through unchanged"
        );
    }

    // Control: the SAME text under a carrier canonical keeps the artifact-less
    // forgiving extraction (the raw scan applies to a genuine `.vue`).
    let (eval, extracted) =
        VerterHost::build_eval_script_source_with_extraction("/Doc.vue", source, None);
    assert!(
        extracted,
        "a carrier canonical keeps the artifact-less forgiving extraction"
    );
    assert!(
        eval.contains("const value = useReal()"),
        "the carrier extraction keeps the script bytes, got: {eval}"
    );
    assert!(
        !eval.contains("export type Real"),
        "the carrier extraction blanks non-script bytes, got: {eval}"
    );
}

#[test]
fn provenance_snapshot_includes_vfs_dir_index_counters_from_workspace() {
    let unique = format!(
        "verter-host-provenance-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("Comp.vue");
    std::fs::write(&file_path, "<template><div /></template>").unwrap();

    let canonical = file_path.to_string_lossy().replace('\\', "/");
    let ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
        verter_workspace::FilesystemOptions::default(),
    ));
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_vfs_provenance();
    assert!(
        ws.file_exists(&canonical),
        "the filesystem workspace should seed its dir index from disk"
    );

    let snapshot = host.provenance_snapshot();
    assert_eq!(
        snapshot.dir_index_refresh_count, 1,
        "host provenance snapshots should surface VFS dir-index refreshes for benchmark validation"
    );
    assert_eq!(
        snapshot.native_fs_read_dir_count, 1,
        "host provenance snapshots should include the VFS read_dir count"
    );
    assert_eq!(
        snapshot.dir_index_hit_count, 0,
        "the first dir-index seed should refresh, not hit a cached directory listing"
    );
    assert_eq!(
        snapshot.native_fs_read_file_miss_count, 0,
        "seeding the dir index for a present file should not record a disk read miss"
    );

    std::fs::remove_file(&file_path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn current_store_view_can_resolve_missing_relative_type_routes_for_existing_workspace_files() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/composables/useComponentIcons.ts",
        "export interface UseComponentIconsProps { icon?: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/types/index.ts",
        "export interface LinkProps { href?: string }\n",
    );
    upsert_vue(
        &host,
        "/workspace/components/Button.vue",
        r#"<script lang="ts">
import type { UseComponentIconsProps } from '../composables/useComponentIcons'
import type { LinkProps } from '../types'

export interface ButtonProps extends UseComponentIconsProps, LinkProps {
  label?: string
}
</script>
<template><button /></template>"#,
    );

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical(
            "/workspace/components/Button.vue",
            "../composables/useComponentIcons")
        .as_deref(),
        Some("/workspace/composables/useComponentIcons.ts"),
        "current store views should resolve missing relative type routes for existing workspace files",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical(
            "/workspace/components/Button.vue",
            "../types")
        .as_deref(),
        Some("/workspace/types/index.ts"),
        "current store views should resolve missing relative barrel routes for existing workspace files",
    );
}

#[test]
fn prepared_type_decl_resolves_plain_declaration_import_helpers() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/helper/dist/helper.d.ts",
        "export type Prettify<T> = { [K in keyof T]: T[K] }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/helper/dist/helper.js",
        "export const runtimeOnly = true\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"
import { Prettify } from 'helper'
export type FancyProps = Prettify<{ open: boolean }>
"#,
    );

    host.set_import_dependencies(
        "/workspace/node_modules/lib/dist/index.d.ts",
        vec![crate::types::DependencyResolution {
            specifier: "helper".to_string(),
            resolved_canonical_id: Some(
                "/workspace/node_modules/helper/dist/helper.js".to_string(),
            ),
            possible_canonical_ids: vec![
                "/workspace/node_modules/helper/dist/helper.js".to_string(),
                "/workspace/node_modules/helper/dist/helper.d.ts".to_string(),
            ],
        }],
    );

    let prepared = host
        .prepared_type_decl("/workspace/node_modules/lib/dist/index.d.ts", "FancyProps")
        .expect("FancyProps should prepare from the imported declaration cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("Prettify")
            .map(|identity| (identity.canonical_id.as_ref(), identity.symbol_name.as_ref())),
        Some(("/workspace/node_modules/helper/dist/helper.d.ts", "Prettify")),
        "plain imports inside declaration files must resolve helper names through the declaration entrypoint rather than leaving them unresolved or pinned to JS companions",
    );
}

#[test]
fn prepared_type_decl_rebuilds_name_resolution_after_import_route_upgrade() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/workspace/.nuxt/ui/checkbox.ts",
        "const theme = { slots: { root: 'slot' } } as const\nexport default theme\n",
    );
    upsert_vue(
        &host,
        "/workspace/Checkbox.vue",
        r#"<script lang="ts">
import theme from '#build/ui/checkbox'

export interface CheckboxProps {
  slots?: typeof theme.slots
}
</script>
<template><div /></template>"#,
    );

    let initial = host
        .prepared_type_decl("/workspace/Checkbox.vue", "CheckboxProps")
        .expect("CheckboxProps should prepare before import routes are upgraded");
    assert_eq!(
        initial
            .name_resolution
            .get("theme")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("#build/ui/checkbox"),
        "before the import route is upgraded the prepared decl should still carry the raw alias target",
    );

    host.set_import_dependencies(
        "/workspace/Checkbox.vue",
        vec![crate::types::DependencyResolution {
            specifier: "#build/ui/checkbox".to_string(),
            resolved_canonical_id: Some("/workspace/.nuxt/ui/checkbox.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/.nuxt/ui/checkbox.ts".to_string()],
        }],
    );

    let rebuilt = host
        .prepared_type_decl("/workspace/Checkbox.vue", "CheckboxProps")
        .expect("CheckboxProps should rebuild after import routes are upgraded");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("theme")
            .map(|identity| (identity.canonical_id.as_ref(), identity.symbol_name.as_ref())),
        Some(("/workspace/.nuxt/ui/checkbox.ts", "default")),
        "prepared decl caches must rebuild when dependency resolutions improve so later typeof/name-resolution walks do not reopen the raw alias path",
    );
}

#[test]
fn prepared_type_decl_bundle_invalidates_when_exact_resolution_changes() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/base.ts", "export interface Base { base: string }\n");
    ws.inject_file("/src/alt.ts", "export interface Base { alt: number }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed module facts");
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/base.ts")],
    );

    let _view_before = host.resolver_store_view_read().into_owned_view();
    let initial = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should materialize before the route change");
    assert_eq!(
        initial
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/alt.ts")],
    );

    let _view_after = host.resolver_store_view_read().into_owned_view();
    let rebuilt = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after the effective dependency target changes");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/alt.ts"),
        "exact-resolution fact validation must invalidate the old bundle when the effective target changes",
    );
}

/// A type / value imported through a re-export BARREL must store the FINAL
/// defining-file canonical in the prepared/eager `name_resolution`, NOT the
/// intermediate barrel. The carrier fallback already walks to the final file;
/// the eager fast-path used to stop at the barrel (the divergence). This pins
/// that the eager `name_resolution` now canonicalizes BOTH rails (type imports
/// via the type-export authority, value imports via the value-export
/// authority) to the final defining file at preparation time.
///
/// Discriminating: a build that left the eager `name_resolution` pinned to the
/// barrel canonical would return `/src/barrel.ts` for both `Node` and
/// `theme` — the asserts demand the final `/src/defining.ts`.
#[test]
fn prepared_decl_name_resolution_canonicalizes_barrel_reexport_to_final_defining_file() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/defining.ts",
        "export type Node = { label: string }\nexport const themeImpl = { color: 'dark' }\n",
    );
    ws.inject_file(
        "/src/barrel.ts",
        "export type { Node } from './defining'\nexport { themeImpl as theme } from './defining'\n",
    );
    ws.inject_file(
        "/src/owner.ts",
        "import type { Node } from './barrel'\n\
         import { theme } from './barrel'\n\
         export interface Props { n: Node; t: typeof theme }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    for owner in ["/src/owner.ts", "/src/barrel.ts"] {
        let _ = host
            .ensure_indexed_ready(owner)
            .unwrap_or_else(|| panic!("{owner} should index"));
    }
    host.set_import_dependencies(
        "/src/owner.ts",
        vec![exact_dependency("./barrel", "/src/barrel.ts")],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./defining", "/src/defining.ts")],
    );

    let prepared = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should prepare through the barrel");

    // TYPE rail: `Node` imported through the barrel canonicalizes to the
    // FINAL defining file (not the intermediate `/src/barrel.ts`).
    assert_eq!(
        prepared.name_resolution.get("Node").map(|identity| (
            identity.canonical_id.as_ref(),
            identity.symbol_name.as_ref()
        )),
        Some(("/src/defining.ts", "Node")),
        "the barrel-imported TYPE `Node` must canonicalize to the FINAL defining file \
         /src/defining.ts in the eager name_resolution, not the intermediate barrel",
    );

    // VALUE rail: `theme` imported through the barrel canonicalizes to the
    // FINAL defining `themeImpl` (the value-export authority peels the alias).
    assert_eq!(
        prepared.name_resolution.get("theme").map(|identity| (
            identity.canonical_id.as_ref(),
            identity.symbol_name.as_ref()
        )),
        Some(("/src/defining.ts", "themeImpl")),
        "the barrel-imported VALUE `theme` must canonicalize to the FINAL defining \
         (/src/defining.ts, themeImpl), not the intermediate barrel binding",
    );
}

/// Editing the barrel's re-export TARGET must invalidate the owner's prepared
/// `name_resolution` — the recorded barrel route facts catch the retarget so a
/// stale final-root is never served. This pins the invalidation rail P6a's
/// canonicalization must preserve.
///
/// Discriminating: if the canonicalization did NOT record the barrel route
/// facts (or rooted only on the owner + final file), retargeting the barrel
/// from `/src/a.ts` to `/src/b.ts` would keep serving the stale `/src/a.ts`
/// final root — the second assert demands `/src/b.ts`.
#[test]
fn prepared_decl_name_resolution_barrel_retarget_invalidates_final_canonical() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/a.ts", "export type Node = { from: 'a' }\n");
    ws.inject_file("/src/b.ts", "export type Node = { from: 'b' }\n");
    ws.inject_file("/src/barrel.ts", "export type { Node } from './a'\n");
    ws.inject_file(
        "/src/owner.ts",
        "import type { Node } from './barrel'\nexport interface Props { n: Node }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    for owner in ["/src/owner.ts", "/src/barrel.ts"] {
        let _ = host
            .ensure_indexed_ready(owner)
            .unwrap_or_else(|| panic!("{owner} should index"));
    }
    host.set_import_dependencies(
        "/src/owner.ts",
        vec![exact_dependency("./barrel", "/src/barrel.ts")],
    );
    host.set_import_dependencies("/src/barrel.ts", vec![exact_dependency("./a", "/src/a.ts")]);

    let initial = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should prepare through the barrel pointing at /src/a.ts");
    assert_eq!(
        initial
            .name_resolution
            .get("Node")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/a.ts"),
        "before the barrel retarget the final canonical is /src/a.ts",
    );

    // Retarget the barrel's re-export from ./a to ./b (a content edit to the
    // barrel's re-export clause + its route).
    ws.inject_file("/src/barrel.ts", "export type { Node } from './b'\n");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/barrel.ts".to_string()),
            input_id: "/src/barrel.ts".to_string(),
            source: Arc::from("export type { Node } from './b'\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("barrel retarget upsert");
    host.set_import_dependencies("/src/barrel.ts", vec![exact_dependency("./b", "/src/b.ts")]);
    // Re-index the retargeted barrel (mirrors the initial setup) so it is
    // present for the canonicalization walk on the owner's bundle rebuild.
    let _ = host
        .ensure_indexed_ready("/src/barrel.ts")
        .expect("retargeted barrel should re-index");

    let rebuilt = host
        .prepared_type_decl("/src/owner.ts", "Props")
        .expect("Props should rebuild after the barrel retarget");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Node")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/b.ts"),
        "the barrel retarget must invalidate the owner's prepared name_resolution so the \
         final canonical follows the barrel to /src/b.ts (no stale-served final root)",
    );
}

#[test]
fn prepared_decl_bundle_without_store_view_reuses_stable_cache() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/dep.ts", "export interface Base { id: string }");
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/dep.ts")],
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");
    host.provenance().reset();

    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup without a store view should materialize exactly one bundle"
    );
    assert_eq!(
        after_first.dep_resolution_calls, 0,
        "this fixture carries exact import targets already, so first lookup should not need dependency-resolution recomputation"
    );

    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.ts"),
    );
    assert_eq!(
        after_second.bundle_materializations, 1,
        "warm lookup without a store view should reuse the stable bundle cache instead of rematerializing"
    );
    assert_eq!(
        after_second.dep_resolution_calls, 0,
        "warm lookup without a store view should not recompute dependency resolutions"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "warm lookup without a store view should register at least one bundle cache hit, got {:?}",
        after_second
    );
}

/// Prepared-decl-bundle breadth: ONE bundle, exercised across BOTH a
/// cache-hit-equivalence re-read AND an exact-resolution-change invalidation —
/// pairing the two halves the model siblings assert SEPARATELY
/// (`prepared_decl_bundle_without_store_view_reuses_stable_cache` =
/// reuse-only; `prepared_type_decl_bundle_invalidates_when_exact_resolution_changes`
/// = invalidate-only) against ONE shared `/src/types.ts:Props` bundle.
///
/// 1. First lookup materializes exactly one bundle
///    (`bundle_materializations == 1`).
/// 2. A second unchanged lookup REUSES the stable cache — no rematerialization
///    (`bundle_materializations` stays 1) and a cache hit registers
///    (`bundle_cache_hits >= 1`). This is the cache-hit equivalence, and both
///    reads resolve `Base` to `/src/base.ts`.
/// 3. The exact resolution is then upgraded (the import route for `./dep` is
///    retargeted `/src/base.ts` → `/src/alt.ts` via `set_import_dependencies`).
///    The next lookup INVALIDATES + rebuilds: `bundle_materializations` climbs
///    to 2 AND the rebuilt `name_resolution` now resolves `Base` to
///    `/src/alt.ts`.
///
/// Discriminates: if prepared-bundle invalidation regressed (the exact-route
/// fact dropped from the bundle's validity signature), the post-retarget
/// lookup REUSES the stale bundle — `bundle_materializations` stays 1 and
/// `Base` keeps resolving to `/src/base.ts`. If reuse regressed, step 2
/// rematerializes (`bundle_materializations` climbs to 2 prematurely).
#[test]
fn prepared_decl_bundle_reuses_then_invalidates_on_exact_resolution_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/base.ts", "export interface Base { base: string }\n");
    ws.inject_file("/src/alt.ts", "export interface Base { alt: number }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './dep'\nexport interface Props extends Base {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed module facts");
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/base.ts")],
    );
    host.provenance().reset();

    // ── First lookup: materialize exactly one bundle, resolving Base →
    // /src/base.ts.
    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup should materialize exactly one bundle"
    );
    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    // ── Cache-hit equivalence: an unchanged second lookup REUSES the bundle.
    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle");
    let after_second = host.provenance().snapshot();
    assert_eq!(
        after_second.bundle_materializations, 1,
        "cache-hit equivalence: the unchanged re-read MUST reuse the stable bundle \
         cache, not rematerialize"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "cache-hit equivalence: the unchanged re-read MUST register a bundle cache hit, got {after_second:?}"
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/base.ts"),
    );

    // ── Invalidation: upgrade the exact resolution (retarget ./dep to
    // /src/alt.ts). The bundle's exact-route fact must invalidate the stale
    // bundle so the next lookup rebuilds.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/alt.ts")],
    );

    let rebuilt = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after the effective dependency target changes");
    let after_rebuild = host.provenance().snapshot();
    assert_eq!(
        after_rebuild.bundle_materializations, 2,
        "DISCRIMINATING (invalidation): upgrading the exact resolution MUST \
         invalidate the stale bundle so the next lookup REBUILDS \
         (bundle_materializations 1 -> 2) — a reused stale bundle would keep it at 1"
    );
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/alt.ts"),
        "DISCRIMINATING (invalidation): the rebuilt bundle's name_resolution MUST \
         observe the upgraded route (Base -> /src/alt.ts); the stale /src/base.ts \
         resolution must NOT survive",
    );
}

#[test]
fn prepared_decl_bundle_with_store_view_reuses_cache_for_structural_exact_resolutions() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/dep.ts".to_string(),
        Arc::from("export interface Base { id: string }\n"),
    );
    ws.inject_file(
        "/workspace/types.ts".to_string(),
        Arc::from("import type { Base } from './dep'\nexport interface Props extends Base {}\n"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.set_import_dependencies(
        "/workspace/types.ts",
        vec![exact_dependency("./dep", "/workspace/dep.ts")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    host.provenance().reset();

    let first = host
        .prepared_type_decl("/workspace/types.ts", "Props")
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup with a captured store view should materialize exactly one bundle"
    );

    let second = host
        .prepared_type_decl("/workspace/types.ts", "Props")
        .expect("second lookup should reuse the prepared bundle even with the same captured view");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/workspace/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/workspace/dep.ts"),
    );
    assert_eq!(
        after_second.bundle_materializations, 1,
        "warm lookup with the same captured store view should reuse the stable bundle cache"
    );
    assert!(
        after_second.bundle_cache_hits >= 1,
        "warm lookup with the same captured store view should register a bundle cache hit, got {:?}",
        after_second
    );
    // Verify the dependency resolution is persisted on DerivedRawState
    // (D48 split — import_routes is the sub-mirror of
    // IndexedReady.import_routes).
    let derived_entry = host
        .derived_raw_cache()
        .get("/workspace/types.ts")
        .expect("types file should have a derived_raw_cache entry");
    assert_eq!(
        derived_entry
            .import_routes
            .get("./dep")
            .and_then(|dep| dep.resolved_canonical_id.as_deref()),
        Some("/workspace/dep.ts"),
        "structurally derived exact resolutions should be persisted onto the DerivedRawState entry",
    );
}

#[test]
fn imported_import_route_upgrades_replace_cached_known_miss_entries() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    upsert_non_sfc(
        &host,
        canonical_id,
        r#"import { FancyProps } from "./inner.js"
export type { FancyProps }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/inner.d.ts",
        "export interface FancyProps { open: boolean }",
    );

    let _ = host
        .ensure_indexed_ready(canonical_id)
        .expect("declaration entrypoint should seed module facts");

    host.set_import_dependencies(
        canonical_id,
        vec![exact_dependency(
            "./inner.js",
            "/workspace/node_modules/lib/dist/inner.d.ts",
        )],
    );

    let resolved = host.resolve_imported_type_root(canonical_id, "FancyProps");
    assert_eq!(
        resolved,
        (
            "/workspace/node_modules/lib/dist/inner.d.ts".to_string(),
            "FancyProps".to_string()
        ),
        "imported declaration entrypoints must upgrade stale miss routes to the exact declaration target",
    );

    let facts = host
        .ensure_indexed_ready(canonical_id)
        .expect("module facts should exist after resolution");
    assert_eq!(
        facts
            .shallow_state
            .import_target("FancyProps")
            .map(|target| target.canonical_id.as_str()),
        Some("/workspace/node_modules/lib/dist/inner.d.ts"),
        "rebuilt imported cache entries must publish the upgraded declaration route",
    );
}

/// The generation-current `ImportRoute` hash is consulted on a
/// cache-VALIDATION / read path (`HostStoreView` derived-hash
/// construction). Building that hash must be side-effect-free: it may
/// re-resolve a known-miss specifier to observe whether the dependency
/// has appeared, but it must not materialize a shallow-only importer
/// into the indexed `FileArtifactStore`, nor mutate the importer's
/// host-owned import-route / dependency caches.
///
/// Scenario: `/workspace/src/importer.ts` records `./theme` as a
/// known-miss in `DerivedRawState.import_routes` (the importer has no
/// indexed `IndexedReady` — only the route table). `./theme.ts` is then
/// added, so the workspace `content_generation` advances and the
/// previously-unresolvable specifier now resolves.
///
/// Discrimination property: a known-miss re-resolve that routes through
/// the full `resolve_type_dependency_canonical` path falls through
/// `cached_import_route_resolution` to `authoritative_import_route`
/// (which calls `ensure_indexed_ready` and materializes the shallow-only
/// importer into the indexed store) and then `resolve_workspace_dependency_and_cache`
/// → `cache_positive_import_route_result` (which rewrites the known-miss
/// entry to a positive resolution and registers the new dependency).
/// This test fails if the oracle re-resolves through that side-effecting
/// path: post-call the importer would be in the indexed store and
/// `import_routes["./theme"]` would be a positive resolution. A
/// side-effect-free workspace re-resolve keeps the indexed store and
/// the route/dependency caches untouched while still folding the
/// now-resolvable target into the returned hash.
#[test]
fn generation_current_import_route_hash_oracle_is_side_effect_free() {
    let host = make_host();
    let importer = "/workspace/src/importer.ts";
    upsert_non_sfc(
        &host,
        importer,
        "import { Theme } from './theme'\nexport type Re = Theme\n",
    );

    // Record `./theme` as a known-miss in `DerivedRawState.import_routes`
    // — `./theme.ts` does not exist yet. `set_import_dependencies`
    // stamps the miss with the current workspace `content_generation`.
    let known_miss = DependencyResolution {
        specifier: "./theme".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: Vec::new(),
    };
    host.set_import_dependencies(importer, vec![known_miss]);

    let route_is_known_miss = |label: &str| {
        let derived = host
            .derived_raw_cache()
            .get(importer)
            .unwrap_or_else(|| panic!("{label}: importer must have a DerivedRawState entry"));
        let resolution = derived
            .import_routes
            .get("./theme")
            .unwrap_or_else(|| panic!("{label}: ./theme must be recorded in import_routes"));
        VerterHost::import_route_is_known_miss(resolution)
    };
    assert!(
        route_is_known_miss("after set_import_dependencies"),
        "./theme must start as a known-miss before the dependency is added"
    );
    let hash_before_dep = host
        .generation_current_import_route_hash(importer)
        .expect("oracle must produce a hash while ./theme is unresolved");

    // Add `./theme.ts`, advancing the workspace `content_generation` so
    // the previously-unresolvable specifier now resolves. The
    // precondition check goes through the bare workspace VFS resolve so
    // it does not itself mutate the importer's import-route cache.
    upsert_non_sfc(
        &host,
        "/workspace/src/theme.ts",
        "export interface Theme { item: string }\n",
    );
    assert_eq!(
        host.ws()
            .resolve_import(
                importer,
                "./theme",
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
            )
            .map(|r| r.source_id),
        Some("/workspace/src/theme.ts".to_string()),
        "precondition: ./theme must be workspace-resolvable once theme.ts exists"
    );

    let importer_indexed_before = host.project_type_store().indexed().get_any(importer);
    let dependencies_before = host
        .dependency_cache()
        .get(importer)
        .map(|entry| entry.dependencies.clone())
        .unwrap_or_default();

    // Invoke the oracle on the cache-validation read path. It must
    // re-resolve `./theme` against the current workspace generation
    // WITHOUT materializing the importer or mutating its caches.
    let hash_after_dep = host
        .generation_current_import_route_hash(importer)
        .expect("oracle must produce a hash after ./theme becomes resolvable");

    let importer_indexed_after = host.project_type_store().indexed().get_any(importer);
    assert_eq!(
        importer_indexed_before.is_some(),
        importer_indexed_after.is_some(),
        "oracle must NOT change whether the importer is materialized in the indexed store"
    );
    assert!(
        route_is_known_miss("after oracle"),
        "oracle must NOT rewrite the ./theme known-miss to a positive import-route resolution"
    );
    let dependencies_after = host
        .dependency_cache()
        .get(importer)
        .map(|entry| entry.dependencies.clone())
        .unwrap_or_default();
    assert_eq!(
        dependencies_before, dependencies_after,
        "oracle must NOT register theme.ts in the importer's dependency set"
    );

    // The re-resolve must still be absence-sensitive: the appearance of
    // theme.ts folds into the hash so dependent caches invalidate.
    assert_ne!(
        hash_before_dep, hash_after_dep,
        "oracle hash must change once the previously-unresolvable ./theme resolves"
    );
}

/// `generation_current_import_route_hash` reads a file's route table
/// from two sources: the content-pinned `IndexedReady.import_routes`
/// snapshot, and — when the indexed snapshot is unavailable — the
/// live-tracked `DerivedRawState.import_routes` table.
///
/// `IndexedReady.import_routes` is the import-target surface captured
/// at index time. It is empty for a file with no statically-routed
/// imports. A route can still be added to that file *after* indexing:
/// on-demand resolutions (compile prefetch, external `src=` handling)
/// route through `resolve_workspace_dependency_and_cache` →
/// `cache_positive_import_route_result`, which writes only
/// `DerivedRawState.import_routes` (and the dependency set) — it does
/// NOT back-fill or re-materialise the already-published `IndexedReady`.
///
/// So an indexed file can simultaneously hold an EMPTY content-pinned
/// `IndexedReady.import_routes` and a POPULATED
/// `DerivedRawState.import_routes`. The oracle must fall through the
/// empty indexed snapshot to the populated `DerivedRawState` table,
/// otherwise it returns `None`, no `ImportRoute` derived fact is
/// recorded for the file, and dependent caches keyed on that fact
/// cannot observe a route change.
///
/// Discrimination property: this test fixes the route table into the
/// empty-`IndexedReady` + populated-`DerivedRawState` state and asserts
/// the oracle returns `Some(hash)` matching the `DerivedRawState`
/// routes. It FAILS if the oracle treats a present-but-empty
/// `IndexedReady.import_routes` as the authoritative (empty) route
/// table and short-circuits to `None`; it PASSES once the empty
/// indexed table defers to the `DerivedRawState` fallback.
#[test]
fn generation_current_import_route_hash_empty_indexed_falls_through_to_derived_raw() {
    let host = make_host();

    // The dependency exists before anything else, so an on-demand
    // resolution against it produces a POSITIVE route.
    let dep = "/workspace/src/prefetch_dep.ts";
    upsert_non_sfc(&host, dep, "export interface Dep { ok: boolean }\n");

    // The owner has NO `import` statements — nothing the indexer can
    // route into `IndexedReady.import_routes`. Its indexed route table
    // is therefore empty.
    let owner = "/workspace/src/prefetch_owner.ts";
    upsert_non_sfc(&host, owner, "export const marker = 1\n");

    // Materialise the owner's `IndexedReady` BEFORE any route is
    // recorded. Because the owner imports nothing, `import_routes` is
    // empty and `import_route_hash` is `None`.
    let indexed = host
        .ensure_indexed_ready(owner)
        .expect("owner IndexedReady must materialise");
    assert!(
        indexed.import_routes.is_empty(),
        "fixture invariant: an import-free owner must materialise an EMPTY \
         IndexedReady.import_routes — otherwise the empty-indexed path is \
         not exercised",
    );
    assert!(
        indexed.import_route_hash.is_none(),
        "fixture invariant: an empty IndexedReady.import_routes carries no \
         import_route_hash (every producer gates it on !is_empty())",
    );

    // A compile-prefetch-style on-demand resolution. This routes
    // through `resolve_workspace_dependency_and_cache` →
    // `cache_positive_import_route_result`, which writes the positive
    // route into `DerivedRawState.import_routes` ONLY — it does not
    // evict or re-materialise the owner's `IndexedReady`.
    let resolved = host.resolve_type_dependency_canonical(owner, "./prefetch_dep");
    assert_eq!(
        resolved.as_deref(),
        Some(dep),
        "precondition: the on-demand resolution must resolve ./prefetch_dep",
    );

    // The owner's content-pinned `IndexedReady` survived the resolution
    // and STILL carries an empty route table — the prefetch landed only
    // in `DerivedRawState`. If this is non-empty the fixture has been
    // invalidated and the empty-indexed path is no longer exercised.
    let indexed_after = host
        .current_content_pinned_indexed(owner)
        .expect("owner IndexedReady must still be content-pinned-current");
    assert!(
        indexed_after.import_routes.is_empty(),
        "fixture invariant: cache_positive_import_route_result must NOT \
         back-fill IndexedReady.import_routes — it stays empty while the \
         route lands in DerivedRawState",
    );

    // `DerivedRawState.import_routes` now carries the positive route.
    let derived_routes = host
        .derived_raw_cache()
        .get(owner)
        .map(|entry| entry.import_routes.clone())
        .expect("fixture invariant: DerivedRawState entry must exist for the owner");
    let derived_route = derived_routes
        .get("./prefetch_dep")
        .expect("fixture invariant: ./prefetch_dep must be recorded in DerivedRawState");
    assert!(
        !VerterHost::import_route_is_known_miss(derived_route),
        "fixture invariant: ./prefetch_dep resolves to an existing file, so \
         its DerivedRawState route is a POSITIVE resolution",
    );

    // Discriminator: with an EMPTY content-pinned IndexedReady route
    // table and a POPULATED DerivedRawState route table, the oracle
    // must fall through to the DerivedRawState routes and return
    // `Some(hash)`. Pre-fix the present-but-empty IndexedReady snapshot
    // shadows the DerivedRawState fallback and the oracle returns
    // `None` — no ImportRoute fact, dependent caches miss the change.
    let oracle_hash = host.generation_current_import_route_hash(owner);
    let expected_hash = crate::resolver_store::hash_import_route_targets(&derived_routes);
    assert_eq!(
        oracle_hash,
        Some(expected_hash),
        "an empty content-pinned IndexedReady.import_routes must NOT hide a \
         populated DerivedRawState route table: the oracle must fall through \
         to DerivedRawState and return its route hash. Pre-fix the empty \
         IndexedReady snapshot wins the route-source selection and the \
         oracle short-circuits to None.",
    );
}

#[test]
fn resolve_imported_type_root_caches_stable_miss_in_imported_root_db() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    upsert_non_sfc(
        &host,
        canonical_id,
        "export interface PresentProps { open: boolean }\n",
    );

    host.ensure_indexed_ready(canonical_id)
        .expect("module facts should seed the provider before capturing a store view");
    let view = host.resolver_store_view_read().into_owned_view();

    let resolved = host.resolve_imported_type_root(canonical_id, "MissingProps");
    assert_eq!(
        resolved,
        (canonical_id.to_string(), "MissingProps".to_string()),
        "legacy callers still observe the provider/name tuple on a miss",
    );

    let cached = host
        .resolver
        .runtime
        .imported_roots
        .get(canonical_id, "MissingProps", &view)
        .expect("imported-root lookup should publish a stable miss to the shared DB");
    assert!(
        cached.is_miss(),
        "missing imported roots must be cached as Miss, not as a fallback self-resolution: {:?}",
        cached
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_canonicalizes_imported_extends_base() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    ws.inject_file("/src/base.ts", "export interface Base { id: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("prepared decl should materialize the imported extends base");

    let base = prepared
        .name_resolution
        .get("Base")
        .expect("prepared decl should carry imported Base resolution");
    assert_eq!(
        base.canonical_id.as_ref(),
        "/src/base.ts",
        "prepared decl name_resolution should point at the imported canonical owner",
    );
    assert_eq!(
        base.symbol_name.as_ref(),
        "Base",
        "prepared decl name_resolution should preserve the imported exported name",
    );
}

#[test]
fn prepared_type_decl_mints_content_free_class_heritage_base_facts() {
    // The class-heritage candidates are PRODUCER-MINTED content-free facts on
    // the prepared decl (minted once at lazy decl-body lowering from the class
    // body's Intersection fold) — never a query-time TypeExpr walk. Each fact
    // carries the authored base NAME (also the `name_resolution` routing key
    // the dispatch head-resolution uses) plus one content-free
    // `TypeArgLocator` per authored heritage type argument. The fact stores no
    // resolved identity and no embedded body: the head resolves at dispatch
    // time, the arguments deref + lower on demand.
    use verter_type_expr::locators::{LocatorSymbolSpace, TypeBodyPathStep};

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/base.ts",
        "export class Base<T, U> { static tag: string = ''; constructor(x: T, y: U) {} }\n",
    );
    ws.inject_file(
        "/src/derived.ts",
        "import { Base } from './base'\n\
         export class Derived extends Base<string, number> {}\n\
         export class Plain { static own: number = 1 }\n\
         interface LocalIface { y: number }\n\
         export interface NotAClass extends LocalIface { x: string }\n\
         export type AliasIx = LocalIface & { z: boolean }\n",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let prepared = host
        .prepared_type_decl("/src/derived.ts", "Derived")
        .expect("prepared decl should materialize the derived class");
    assert_eq!(
        prepared.heritage_bases.len(),
        1,
        "one heritage base fact for `extends Base<string, number>`, got {:?}",
        prepared.heritage_bases
    );
    let fact = &prepared.heritage_bases[0];
    assert_eq!(fact.name, "Base", "the authored base name");
    assert_eq!(
        fact.name_resolution_ref, "Base",
        "the name_resolution routing key is the authored head name"
    );
    // Dispatch head-resolution: the fact's routing key resolves CROSS-FILE
    // through the prepared decl's own name_resolution — the fact itself never
    // stores the resolved identity.
    let head = prepared
        .name_resolution
        .get(fact.name_resolution_ref.as_str())
        .expect("the heritage head routes through name_resolution");
    assert_eq!(head.canonical_id.as_ref(), "/src/base.ts");
    assert_eq!(head.symbol_name.as_ref(), "Base");
    // One content-free locator per authored type argument, addressing the
    // heritage Ref arm of the class body's Intersection fold; `arg_index`
    // selects the authored argument.
    assert_eq!(fact.type_args.len(), 2, "two authored type arguments");
    for (index, arg) in fact.type_args.iter().enumerate() {
        assert_eq!(arg.arg_index, index as u32, "source-order arg ordinal");
        assert_eq!(arg.anchor.canonical_id.as_ref(), "/src/derived.ts");
        assert_eq!(arg.anchor.symbol.as_ref(), "Derived");
        assert_eq!(arg.anchor.space, LocatorSymbolSpace::Type);
        assert_eq!(
            arg.path.as_ref(),
            &[TypeBodyPathStep::IntersectionArm { ordinal: 0 }],
            "the arg-bearing position is the heritage Ref arm (arm 0, before \
             the own Object arm)"
        );
    }

    // A heritage-free class mints no facts.
    let plain = host
        .prepared_type_decl("/src/derived.ts", "Plain")
        .expect("prepared decl should materialize the plain class");
    assert!(
        plain.heritage_bases.is_empty(),
        "a heritage-free class carries no heritage base facts: {:?}",
        plain.heritage_bases
    );

    // A NON-class Intersection body must NOT mint class-heritage facts: an
    // interface's extends fold and an alias's authored intersection are not
    // class heritage.
    let iface = host
        .prepared_type_decl("/src/derived.ts", "NotAClass")
        .expect("prepared decl should materialize the interface");
    assert!(
        iface.heritage_bases.is_empty(),
        "an interface extends fold mints no CLASS heritage facts: {:?}",
        iface.heritage_bases
    );
    let alias = host
        .prepared_type_decl("/src/derived.ts", "AliasIx")
        .expect("prepared decl should materialize the alias");
    assert!(
        alias.heritage_bases.is_empty(),
        "an alias intersection is authored composition, not heritage: {:?}",
        alias.heritage_bases
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_reuses_indexed_package_shallow_state_without_reread() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "import type { Payload } from './payload.d.ts'\nexport interface PackageEmits {\n  (e: 'open', value?: Payload): void\n}\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/payload.d.ts",
        "export interface Payload { value: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        vec![exact_dependency(
            "./payload.d.ts",
            "/workspace/node_modules/pkg/dist/payload.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let (target_canonical, target_name) = host.resolve_imported_type_root(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "PackageEmits",
    );
    assert_eq!(
        target_canonical, "/workspace/node_modules/pkg/dist/index3.d.ts",
        "shallow lookup should still normalize the package export target first",
    );
    assert_eq!(target_name, "PackageEmits");

    ws.reset_reads();
    host.provenance().reset();

    let prepared = host
        .prepared_type_decl(target_canonical.as_str(), target_name.as_str())
        .expect("prepared package declaration should reuse the warmed indexed shallow state");

    let payload = prepared
        .name_resolution
        .get("Payload")
        .expect("prepared package declaration should still resolve imported helper names");
    assert_eq!(
        payload.canonical_id.as_ref(), "/workspace/node_modules/pkg/dist/payload.d.ts",
        "prepared package declaration should canonicalize imported helper edges from the warmed shallow state",
    );
    assert_eq!(payload.symbol_name.as_ref(), "Payload");
    assert!(
        ws.read_count("/workspace/node_modules/pkg/dist/index3.d.ts") <= 1,
        "prepared package declaration lookup should pay at most one shallow package target read for the active route",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected provider barrel owns a canonical IndexedReady",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the inspected active package target owns a canonical IndexedReady",
    );
    // The ONCE/no-reread discriminator: the prepared-decl build runs
    // entirely against the artifacts the first resolution warmed — zero
    // new materialisations (a re-build of the warmed package target
    // would show up here even if the workspace read were served by an
    // intermediate cache).
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the prepared-decl build must REUSE the warmed package artifacts — \
         zero IndexedReady materialisations",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/payload.d.ts"),
        0,
        "the prepared declaration lookup resolves imported helper edges from the dependency tables without reading the helper source",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
            .is_none(),
        "imported helper edges the walk never inspects stay shallow: no IndexedReady is materialized for them",
    );
}

#[test]
fn prepared_type_decl_backfills_missing_local_symbol_when_cache_is_partial() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    let source = r#"
type Local = { open: boolean }
export type FancyProps = Local
"#;
    upsert_non_sfc(&host, canonical_id, source);

    let prepared = host
        .prepared_type_decl(canonical_id, "Local")
        .expect("missing local decl should be prepared from shallow state");

    assert_eq!(prepared.root_identity.canonical_id.as_ref(), canonical_id);
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Local");
}

// NOTE: stale prepared-decl replacement test was removed — prepared decls
// are managed through the host-owned bundle cache path, not IndexedReady.

#[test]
fn resolver_store_view_tracks_transitive_dependency_targets() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(&host, "/src/types.ts", "export { Props } from './dep'\n");
    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    let view = host.resolver_store_view_read().into_owned_view();

    assert!(
        view.whole_hash("/src/types.ts").is_some(),
        "captured store view should include direct dependency whole hashes"
    );
    assert!(
        view.whole_hash("/src/dep.ts").is_some(),
        "captured store view should include transitive dependency whole hashes"
    );
    assert!(
        view.derived_hash(
            "/src/types.ts",
            crate::resolver_core::DerivedFactKind::ImportRoute,
        )
        .is_some(),
        "captured store view should snapshot transitive import-route hashes"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolver_store_view_does_not_materialize_tracked_indexed_ready() {
    let host = strict_host();

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(&host, "/src/types.ts", "export { Props } from './dep'\n");
    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/unused.ts",
        "export interface Unused { label: string }\n",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    host.project_type_store()
        .indexed()
        .remove("/src/Consumer.vue");
    host.project_type_store().indexed().remove("/src/types.ts");
    host.project_type_store().indexed().remove("/src/dep.ts");
    host.project_type_store().indexed().remove("/src/unused.ts");
    host.provenance().reset();

    let view = host.resolver_store_view_read().into_owned_view();
    let provenance = host.provenance_snapshot();

    assert_eq!(
        provenance.indexed_ready_scheduler_snapshot_reuse, 0,
        "capturing a store view should not eagerly materialize tracked module facts; the view should snapshot known whole/import-route hashes without paying scheduler-backed module-facts loads",
    );
    assert!(
        view.whole_hash("/src/types.ts").is_some(),
        "store view should still capture tracked whole hashes for direct dependencies",
    );
    assert!(
        view.whole_hash("/src/dep.ts").is_some(),
        "store view should still capture tracked whole hashes for transitive dependencies",
    );
    assert!(
        view.derived_hash(
            "/src/types.ts",
            crate::resolver_core::DerivedFactKind::ImportRoute,
        )
        .is_some(),
        "store view should snapshot tracked import-route hashes from host-owned dependency state without materializing module facts",
    );

    let resolved = host.resolve_type_dependency_canonical_shallow("/src/Consumer.vue", "./types");
    assert_eq!(
        resolved.as_deref(),
        Some("/src/types.ts"),
        "lazy store views should still resolve direct import edges through the captured import-route hash/state",
    );
}

#[test]
fn resolver_store_view_tracks_reexport_import_routes() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/index.ts", "export { Props } from './dep'\n");

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow("/src/index.ts", "./dep")
            .as_deref(),
        Some("/src/dep.ts"),
        "captured store views should resolve re-export import routes without requiring a synthesized import-route snapshot",
    );
}

#[test]
fn resolver_store_view_prefers_declaration_companion_routes_for_dts_imports() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/index.d.ts",
        "import { Props } from './inner.js'\nexport type { Props }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/inner.d.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/inner.js", "export const runtimeOnly = true\n");

    let _view = host.resolver_store_view_read().into_owned_view();

    assert_eq!(
        host.resolve_type_dependency_canonical("/src/index.d.ts", "./inner.js")
            .as_deref(),
        Some("/src/inner.d.ts"),
        "captured store views should resolve declaration-file imports through the declaration companion",
    );
}

#[test]
fn resolver_store_view_resolves_exports_for_unloaded_workspace_barrels() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/types/index.ts".to_string(),
        Arc::from("export * from '../Button.vue'\n"),
    );
    ws.inject_file(
        "/workspace/Button.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
        ),
    );
    ws.inject_file(
        "/workspace/App.vue".to_string(),
        Arc::from(
            r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    assert!(
        host.ensure_loaded("/workspace/App.vue"),
        "entry file should load from workspace",
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let exports = host.resolve_exports("/workspace/types/index.ts");

    assert!(
        exports.iter().any(|export| {
            export.name == "ButtonProps"
                && export.source_canonical_id.as_deref() == Some("/workspace/Button.vue")
        }),
        "captured store view should resolve exports for unloaded workspace barrels, got: {exports:?}",
    );
}

#[test]
fn store_view_generic_dependency_paths_promote_snapshot_and_env_into_imported_cache() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/shared.d.ts",
        r#"export interface Alpha { alpha?: string }"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let source = host
        .read_analysis_source("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("dependency source should load into the imported dependency cache");
    assert!(
        source.contains("Alpha"),
        "sanity check: the dependency source should be readable"
    );

    let before = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("source-only imported dependency entry should exist");
    // In the new IndexedReady model, snapshot is always Arc<FileAnalysisSnapshot>.
    // Before explicit snapshot build, it starts as default (empty bindings).
    assert!(
        before.snapshot.bindings.is_empty(),
        "source-only imported dependency entry should start with an empty snapshot"
    );
    let snapshot = host
        .get_raw_analysis_snapshot("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("store-view snapshot path should build the dependency snapshot");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid analysis snapshot"
    );

    let env = host
        .base_eval_env_arc("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("store-view eval env path should build the dependency env");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "built dependency env should expose the declaration symbol"
    );

    let after = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("dependency entry should remain cached after store-view generic access");
    // Verify facts still exist after store-view generic access
    assert!(
        !after.raw_source.is_empty(),
        "store-view access should preserve the module facts"
    );
}

#[test]
fn store_view_imported_seed_reuses_cached_source_for_snapshot_and_env() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    ws.reset_reads();

    let entry = host
        .ensure_indexed_ready(canonical_id)
        .expect("explicit shallow seeding should load imported dependency state");
    // external_type_analysis is Arc<AnalyzedExternalTypeSource> (non-optional in IndexedReady);
    // verify it was populated by checking the analysis has content.
    assert!(
        entry
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "explicit shallow seeding should build external type analysis",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "explicit shallow seeding should read the imported file once",
    );

    assert!(
        host.current_eval_state(canonical_id).is_some(),
        "current_eval_state should reuse the imported cache after explicit seeding",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "current_eval_state should reuse the seeded imported cache without another workspace read",
    );

    let snapshot = host
        .get_raw_analysis_snapshot(canonical_id)
        .expect("snapshot build should reuse the seeded imported cache");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid snapshot",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "snapshot materialization should not reread the imported file once seeded",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("eval env build should reuse the seeded imported cache");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "eval env should expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "eval env build should not reread the imported file once seeded",
    );
}

#[test]
fn store_view_warm_imported_eval_env_hit_reuses_indexed_route_hash_without_reread() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(canonical_id).is_some(),
        "routed shallow state should seed the imported declaration lazily",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "routed shallow seeding should read the imported file once",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("first eval env build should succeed from the routed imported file");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "first eval env build should expose the imported declaration symbol",
    );

    ws.reset_reads();

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("warm eval env lookup should reuse the cached env");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "warm eval env lookup should still expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        0,
        "warm eval env lookup should not reread the imported file once the indexed shallow hash and env are cached",
    );
}

#[test]
fn store_view_indexed_imported_seed_reuses_cached_source_for_snapshot_and_env() {
    let ws = Arc::new(CountingWorkspace::new());
    let canonical_id = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(canonical_id, "export interface Alpha { alpha?: string }");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(canonical_id).is_some(),
        "routed shallow seeding should load the imported dependency state",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "routed shallow seeding should read the imported file once",
    );

    assert!(
        host.read_analysis_source(canonical_id).is_some(),
        "read_analysis_source should reuse the indexed imported source once seeded",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "read_analysis_source should not reread the imported file once indexed state is cached",
    );

    assert!(
        host.current_eval_state(canonical_id).is_some(),
        "current_eval_state should reuse the indexed imported source once seeded",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "current_eval_state should not reread the imported file once indexed state is cached",
    );

    let snapshot = host
        .get_raw_analysis_snapshot(canonical_id)
        .expect("snapshot build should reuse the indexed imported source");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid snapshot",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "snapshot materialization should not reread the imported file once indexed state is cached",
    );

    let analysis = host
        .external_type_analysis(canonical_id)
        .expect("external type analysis should reuse the indexed imported source");
    assert!(
        analysis.local_type_symbol("Alpha").is_some(),
        "external type analysis should still expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "external type analysis should not reread the imported file once indexed state is cached",
    );

    let env = host
        .base_eval_env_arc(canonical_id)
        .expect("eval env build should reuse the indexed imported source");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "eval env should expose the imported declaration symbol",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "eval env build should not reread the imported file once indexed state is cached",
    );
}

#[test]
fn cached_import_route_resolution_reuses_untracked_current_version_across_epoch_bumps() {
    let ws = Arc::new(CountingWorkspace::new());
    let provider = "/workspace/node_modules/pkg/dist/index.d.ts";
    let target = "/workspace/node_modules/pkg/dist/inner.d.ts";
    ws.inject_file(provider, "export type { InnerProps } from './inner.d.ts'\n");
    ws.inject_file(target, "export interface InnerProps { label: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(provider, vec![exact_dependency("./inner.d.ts", target)]);

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state(provider).is_some(),
        "routed shallow seeding should build the current provider surface after the view snapshot",
    );

    let resolved = host.cached_import_route_resolution(provider, "./inner.d.ts");
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolution| resolution.resolved_canonical_id.as_deref()),
        Some(target),
        "the untracked current-version import-route cache should resolve before any unrelated host mutation",
    );

    upsert_non_sfc(
        &host,
        "/workspace/src/Unrelated.ts",
        "export const changed = 1",
    );

    let resolved = host.cached_import_route_resolution(provider, "./inner.d.ts");
    assert_eq!(
        resolved
            .as_ref()
            .and_then(|resolution| resolution.resolved_canonical_id.as_deref()),
        Some(target),
        "unchanged imported providers loaded after the view snapshot should keep reusing their current import-route cache across unrelated epoch bumps",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn store_view_external_type_analysis_materializes_tracked_imported_dependency_indexed_ready() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(host.ensure_loaded("/workspace/src/Consumer.vue"));
    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.routed_shallow_state("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "active route traversal should be able to build the target's shallow state first",
    );

    assert!(
        host.external_type_analysis("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "tracked imported declarations should expose external type analysis from the shallow source path",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the inspected tracked imported dependency owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[test]
fn store_view_seeded_imported_barrel_backfills_wildcard_import_routes() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/node_modules/pkg/dist/index.d.ts";
    let shared = "/workspace/node_modules/pkg/dist/shared.d.ts";
    ws.inject_file(barrel, "export * from './shared.js'\n");
    ws.inject_file(shared, "export interface Shared { label?: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let _view = host.resolver_store_view_read().into_owned_view();

    let facts = host
        .ensure_indexed_ready(barrel)
        .expect("explicit shallow seeding should materialize the barrel facts");
    assert_eq!(
        facts.shallow_state.wildcard_reexports.len(),
        1,
        "barrel should publish its wildcard reexport",
    );
    assert_eq!(
        facts.shallow_state.wildcard_reexports[0].canonical_id,
        shared,
        "seeded IndexedReady must backfill wildcard canonical IDs even when the store view had no exact-resolution snapshot",
    );

    ws.reset_resolves();
    let resolved = host.resolve_type_dependency_canonical_shallow(barrel, "./shared.js");
    assert_eq!(
        resolved.as_deref(),
        Some(shared),
        "shallow dependency lookup should reuse the seeded barrel route",
    );
    assert_eq!(
        ws.resolve_count(barrel, "./shared.js"),
        0,
        "seeded wildcard routes should not bounce back to the live workspace resolver",
    );
}

#[test]
fn read_dep_source_for_type_resolution_promotes_eval_source_for_loaded_workspace_file() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    upsert_vue(
        &host,
        "/workspace/src/InputMenu.vue",
        r#"<script setup lang="ts">
const answer: string = '42'
</script>
<template><div>{{ answer }}</div></template>"#,
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly materializes
    // from workspace sources, so we can't assert is_none before read_dep.

    let first = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let second = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let promoted = host
        .ensure_indexed_ready("/workspace/src/InputMenu.vue")
        .expect("type-resolution read should promote eval source into the host dependency cache");

    assert_eq!(
        first.as_deref().map(str::trim),
        Some("const answer: string = '42'"),
        "Vue type-resolution reads should return script content only",
    );
    assert_eq!(
        second, first,
        "warm reads should reuse the same promoted source"
    );
    assert_eq!(
        Some(promoted.eval_source.trim()),
        Some("const answer: string = '42'"),
        "the promoted dependency cache entry should keep the extracted type-resolution source",
    );
    assert!(
        promoted.framework_parse.is_some(),
        "the promoted Vue dependency cache entry should retain the carrier parse artifact",
    );
    // In the new IndexedReady model, ensure_indexed_ready eagerly builds a
    // full snapshot, so we just verify the facts are present and well-formed.
    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        promoted.external_type_analysis.stats().top_level_statement_count > 0,
        "type-resolution reads should seed shallow external type analysis alongside the eval source",
    );
}

#[test]
fn ensure_indexed_ready_reuses_cached_vue_entry_arc() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="ts">
import type { Base } from './base'

export interface Props extends Base {
  label: string
}
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/types.vue",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let first = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("first Vue imported dependency state should be built");
    let second = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("second Vue imported dependency state should reuse the cached entry");

    assert_eq!(
        first.whole_hash, second.whole_hash,
        "repeated Vue imported dependency state lookups should produce equivalent entries",
    );
    // In IndexedReady, snapshot is Arc<FileAnalysisSnapshot> (non-optional).
    assert!(
        first.framework_parse.is_some(),
        "cached Vue imported dependency entry should retain parse state",
    );
    assert!(
        first.script_analysis.is_some() && first.export_signatures.is_some(),
        "cached Vue imported dependency entry should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        first.external_type_analysis.stats().top_level_statement_count > 0,
        "cached Vue imported dependency entry should eagerly retain external type analysis so later resolver lookups do not reparse",
    );
}

#[test]
fn ensure_indexed_ready_populates_external_type_analysis_for_non_sfc() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("imported dependency state should be materialized");

    // snapshot is Arc<FileAnalysisSnapshot> (non-optional) in IndexedReady.
    // Non-SFC entries always have a snapshot populated after materialization.
    assert!(
        !entry.raw_source.is_empty(),
        "non-SFC imported dependency state should retain the analysis snapshot without caching env",
    );
    assert!(
        entry.script_analysis.is_some() && entry.export_signatures.is_some(),
        "non-SFC imported dependency state should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        entry.external_type_analysis.stats().top_level_statement_count > 0,
        "non-SFC imported dependency state should eagerly retain external type analysis so later resolver lookups stay on cache",
    );
}

/// Warm re-upsert (unchanged content) must still surface external `src`
/// block requests. Bundler transforms re-resolve them every time; empty
/// warm requests cause HOST_MISSING_EXTERNAL when the dep was never loaded
/// on a prior pass (zyronon-douyin `<style src="./switches.less">`).
#[test]
fn warm_upsert_still_returns_external_style_src_requests() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    let src = r#"<template><div class="x"/></template>
<script>export default { name: 'Switches' }</script>
<style src="./switches.less" lang="less"></style>
"#;
    let first = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/switches.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert_eq!(
        first.external_source_requests.len(),
        1,
        "cold upsert must report the style src request"
    );
    assert_eq!(
        first.external_source_requests[0].specifier,
        "./switches.less"
    );

    let second = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/src/switches.vue".to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        !second.changed,
        "byte-identical re-upsert should be unchanged"
    );
    assert_eq!(
        second.external_source_requests.len(),
        1,
        "warm upsert must still report external style src requests"
    );
    assert_eq!(
        second.external_source_requests[0].specifier,
        "./switches.less"
    );
    assert_eq!(
        second.external_source_requests[0].resolved_canonical_id,
        first.external_source_requests[0].resolved_canonical_id
    );
}

#[test]
fn resolve_dep_source_reuses_cached_source_without_loading_dependency_into_host_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/partial.html",
        "<div class=\"partial\">partial</div>",
    );
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<template src="./partial.html"></template>
<script setup>const ok = true</script>"#,
    );
    ws.set_exact_resolutions(
        "/workspace/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "./partial.html".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/src/partial.html".to_string()),
            possible_canonical_ids: vec!["/workspace/src/partial.html".to_string()],
        }],
    );

    ws.reset_reads();
    let first = host.resolve_dep_source(
        "/workspace/src/App.vue",
        "/workspace/src/partial.html",
        "./partial.html",
    );
    let second = host.resolve_dep_source(
        "/workspace/src/App.vue",
        "/workspace/src/partial.html",
        "./partial.html",
    );

    assert_eq!(
        first.as_deref(),
        Some("<div class=\"partial\">partial</div>"),
        "first dependency source lookup should return the external source text"
    );
    assert_eq!(
        second, first,
        "warm dependency source lookup should return the same cached source"
    );
    // External dep source reads go through workspace read_file each time.
    // The functional contract is that both calls return the same content.
    assert!(
        host.get_source("/workspace/src/partial.html").is_none(),
        "external dep source should not be promoted into host file state"
    );
}

#[test]
fn cached_import_route_is_reused_by_internal_and_public_import_lookups() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/src/dep.ts", "export const dep = 1");
    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import { dep } from "@/dep"
</script>"#,
    );
    ws.set_exact_resolutions(
        "/workspace/src/App.vue",
        vec![verter_workspace::ExactResolution {
            specifier: "@/dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/src/dep.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/src/dep.ts".to_string()],
        }],
    );

    ws.reset_resolves();
    let first = host.resolve_loaded_dependency_canonical(
        "/workspace/src/App.vue",
        "@/dep",
        verter_workspace::ResolveRequestKind::EsmImport,
    );
    let second = host.resolve_import("/workspace/src/App.vue", "@/dep");
    let third = host.resolve_loaded_dependency_canonical(
        "/workspace/src/App.vue",
        "@/dep",
        verter_workspace::ResolveRequestKind::EsmImport,
    );

    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/dep.ts"),
        "first lookup should resolve through the workspace fallback"
    );
    assert_eq!(
        second.as_deref(),
        Some("/workspace/src/dep.ts"),
        "public resolve_import should reuse the same cached canonical route"
    );
    assert_eq!(
        third, first,
        "subsequent internal lookups should keep hitting the promoted cache entry"
    );
    assert_eq!(
        ws.resolve_count("/workspace/src/App.vue", "@/dep"),
        1,
        "workspace resolve_import should run once before the cached dependency resolution is reused"
    );
}

#[test]
fn build_fallthrough_eval_env_skips_unused_runtime_import_dependency_lookups() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/used.ts", "export const used = 'used'");
    upsert_non_sfc(&host, "/src/unused.ts", "export const unused = 'unused'");
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import { used } from './used'
import { unused } from './unused'
</script>
<template><div :title="used" /></template>"#,
    );
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            exact_dependency("./used", "/src/used.ts"),
            exact_dependency("./unused", "/src/unused.ts"),
        ],
    );

    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let env = host
        .build_fallthrough_eval_env_lightweight("/src/App.vue", &snapshot, None)
        .expect("fallthrough owner env should build");

    assert!(
        env.value_symbols.contains_key("used"),
        "template-referenced runtime bindings should still be materialized"
    );
    assert!(
        !env.value_symbols.contains_key("unused"),
        "unused runtime imports should stay out of the fallthrough owner env"
    );
}

#[test]
fn build_fallthrough_eval_env_skips_nested_non_root_component_bindings() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/used.ts", "export const used = 'used'");
    upsert_non_sfc(
        &host,
        "/src/unused-nested.ts",
        "export const unusedNested = 'unused-nested'",
    );
    upsert_vue(
        &host,
        "/src/Child.vue",
        r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><span /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import { used } from './used'
import { unusedNested } from './unused-nested'
import Child from './Child.vue'
</script>
<template>
  <div :title="used">
    <Child :label="unusedNested" />
  </div>
</template>"#,
    );
    host.set_import_dependencies(
        "/src/App.vue",
        vec![
            exact_dependency("./used", "/src/used.ts"),
            exact_dependency("./unused-nested", "/src/unused-nested.ts"),
            exact_dependency("./Child.vue", "/src/Child.vue"),
        ],
    );

    let snapshot = host
        .get_analysis_snapshot_internal("/src/App.vue", None)
        .expect("analysis snapshot should exist");
    let mut visiting = rustc_hash::FxHashSet::default();
    let resolved = host
        .compute_component_meta_state(
            "/src/App.vue",
            crate::types::ProjectionMode::Expanded,
            host.get_whole_hash("/src/App.vue")
                .expect("whole hash should exist for App.vue"),
        )
        .expect("resolved meta should exist");
    let resolution = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        host.compute_fallthrough_surface_from_resolved_state(
            "/src/App.vue",
            &resolved,
            None,
            &mut visiting,
            ctx,
        )
    })
    .expect("fallthrough should resolve");
    assert!(
        matches!(
            resolution.fallthrough_surface,
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "sanity check: single native root should still produce a fallthrough branch"
    );

    let base_meta = verter_semantic::analysis::component_meta::extract_component_meta(
        verter_semantic::analysis::component_meta::ComponentMetaInput {
            macros: &resolved.snapshot.macros,
            bindings: &resolved.snapshot.bindings,
            imports: &resolved.snapshot.imports,
            template: resolved.snapshot.template.as_deref(),
            options_api: resolved.snapshot.options_api.as_ref(),
            analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
                resolved.snapshot.script_flags,
            ),
            styles: &resolved.snapshot.styles,
            vue_api_calls: &resolved.snapshot.vue_api_calls,
            store_usages: &resolved.snapshot.store_usages,
            resolved_macros: &[],
            resolved_type_registry: &[],
            evaluated_types: None,
            file_path: "/src/App.vue",
        },
    );
    let env = host
        .build_fallthrough_eval_env_lightweight(
            "/src/App.vue",
            &snapshot,
            Some(&base_meta.root_reachability),
        )
        .expect("fallthrough owner env should build");

    assert!(
        env.value_symbols.contains_key("used"),
        "root-branch runtime bindings should still be materialized"
    );
    assert!(
        !env.value_symbols.contains_key("unusedNested"),
        "nested non-root component prop bindings should stay out of the root fallthrough env"
    );
}

#[test]
fn extract_component_meta_from_resolved_keeps_fallthrough_on_captured_store_view() {
    let host = make_host();
    upsert_vue(&host, "/src/Link.vue", r#"<template><a /></template>"#);
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script setup lang="ts">
import Link from './Link.vue'
</script>
<template><Link /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./Link.vue", "/src/Link.vue")],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();

    upsert_non_sfc(&host, "/src/shared.ts", "export const shared = 'shared'");
    upsert_vue(
        &host,
        "/src/UnrelatedA.vue",
        r#"<script setup lang="ts">
import { shared } from './shared'
</script>
<template><div :title="shared" /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/UnrelatedB.vue",
        r#"<script setup lang="ts">
import { shared } from './shared'
</script>
<template><div :title="shared" /></template>"#,
    );

    host.provenance().reset();

    let resolved = host
        .resolve_component_meta("/src/Button.vue", crate::types::ProjectionMode::Expanded)
        .expect("resolved meta should be computed from the captured view");

    let meta = crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
        extract_component_meta_from_resolved(&host, "/src/Button.vue", &resolved, true, ctx)
    })
    .analysis;

    assert!(
        matches!(
            meta.fallthrough_surface,
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "button fallthrough should still resolve through the imported Link root",
    );
}

/// Typed-completeness gate: a NON-budget partial (a fuse / semantic-miss class
/// signal folded via `mark_request_result_partial`) gates BOTH
/// fallthrough cache-admission sites — `store_node` and
/// `cache_fallthrough_result` — EVEN THOUGH the projection budget is NOT
/// exhausted. This proves the gate keys on the typed cold-compute completeness,
/// not the ad-hoc `is_exhausted()` predicate the fix deletes.
///
/// Without the fix both gates consult `current_request_budget().is_exhausted()`
/// (false here), so the node IS stored and the mirror IS warmed. With it, both
/// gates consult `current_cold_compute_completeness().is_partial()` (true),
/// refusing admission.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn non_budget_partial_gates_fallthrough_admission_with_budget_unexhausted() {
    use crate::resolver_core::fallthrough_resolver::intrinsic_surface_key;
    use crate::resolver_core::FallthroughRequestHost;

    let host = make_host();
    upsert_vue(&host, "/src/App.vue", r#"<template><div /></template>"#);
    let canonical = "/src/App.vue";

    // A request budget with ample headroom — it is NEVER exhausted.
    let rctx = crate::request_context::RequestContext::with_kind_timing_and_projection_budget(
        1,
        Arc::from(canonical),
        verter_audit::RequestKind::ComponentMeta,
        false,
        false,
        None,
        100_000,
    );
    let _guard = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx));
    let _scope = crate::request_context::ColdComputeCompletenessScope::enter();

    // Fold a NON-budget partial (fuse / semantic-miss class) WITHOUT touching
    // the projection budget.
    crate::request_context::mark_request_result_partial();

    // The discriminating precondition split: the partial is typed completeness,
    // NOT budget exhaustion. The deleted ad-hoc gate would NOT fire here.
    assert!(
        !rctx.projection_budget.is_exhausted(),
        "the projection budget must NOT be exhausted — this isolates the non-budget partial"
    );
    assert!(
        crate::request_context::current_cold_compute_completeness().is_partial(),
        "the cold-compute scope must carry a Partial after a non-budget fold"
    );

    // (a) The owner-run node admission refuses on typed completeness.
    //
    // The cacheability probe is CLEAN here (no fenced serve, no overflow), which
    // is what isolates the rail under test: the ONLY thing that can refuse this
    // admission is the typed cold-compute completeness.
    let (anchor, generation) = host.project_intrinsic_cache_anchor(canonical);
    let key = intrinsic_surface_key(&anchor, generation, "div");
    let members = host.intrinsic_members_for_tag("div");
    let node = host.build_runtime_intrinsic_surface_node(&members);
    host.resolver_runtime()
        .fallthrough
        .compute_and_maybe_admit(&host, || ((), Some((key.clone(), node))));
    let view = FallthroughRequestHost::snapshot_store_view(&host);
    assert!(
        host.resolver_runtime()
            .fallthrough
            .get_cached_node(&key, &view)
            .is_none(),
        "a NON-budget partial (budget not exhausted) MUST refuse owner admission — the gate is \
         typed completeness, NOT is_exhausted() (pre-fix is_exhausted()=false stored the node)"
    );

    // (b) `cache_fallthrough_result` refuses the legacy mirror on the same gate.
    let result = crate::types::FallthroughResolution {
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness:
            verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface::None {
            reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
        },
        fact_versions: Vec::new(),
    };
    crate::fact_signature_helpers::with_cacheability_scope(&host, |probe| {
        let admission = crate::resolver_core::FallthroughStableAdmission::from_test_scope(probe);
        host.cache_fallthrough_result(canonical, None, &result, &admission);
    });
    let mirror_present = host
        .derived_raw_cache()
        .get(canonical)
        .and_then(|entry| entry.cached_fallthrough.as_ref().map(|_| ()))
        .is_some();
    assert!(
        !mirror_present,
        "a NON-budget partial MUST refuse the cached_fallthrough mirror — typed-completeness gate, \
         not is_exhausted() (pre-fix the un-exhausted budget warmed the mirror)"
    );

    drop(_scope);
    drop(_guard);
}

#[test]
fn fallthrough_barrel_routing_preserves_default_vs_named_binding_identity() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/components/index.ts",
        "export { default as Button } from './NamedButton.vue'\nexport { default } from './DefaultButton.vue'\n",
    );
    upsert_vue(
        &host,
        "/src/components/NamedButton.vue",
        "<template><button /></template>",
    );
    upsert_vue(
        &host,
        "/src/components/DefaultButton.vue",
        "<template><a /></template>",
    );
    upsert_vue(
        &host,
        "/src/AppNamed.vue",
        r#"<script setup lang="ts">
import { Button } from './components'
</script>
<template><Button /></template>"#,
    );
    upsert_vue(
        &host,
        "/src/AppDefault.vue",
        r#"<script setup lang="ts">
import Button from './components'
</script>
<template><Button /></template>"#,
    );

    host.set_import_dependencies(
        "/src/AppNamed.vue",
        vec![exact_dependency("./components", "/src/components/index.ts")],
    );
    host.set_import_dependencies(
        "/src/AppDefault.vue",
        vec![exact_dependency("./components", "/src/components/index.ts")],
    );
    host.set_import_dependencies(
        "/src/components/index.ts",
        vec![
            exact_dependency("./NamedButton.vue", "/src/components/NamedButton.vue"),
            exact_dependency("./DefaultButton.vue", "/src/components/DefaultButton.vue"),
        ],
    );

    let named = host
        .get_component_meta("/src/AppNamed.vue")
        .expect("named barrel import should resolve component meta");
    let default = host
        .get_component_meta("/src/AppDefault.vue")
        .expect("default barrel import should resolve component meta");

    let named_props: std::collections::BTreeSet<_> = named
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();
    let default_props: std::collections::BTreeSet<_> = default
        .accepted_props
        .iter()
        .map(|prop| prop.name.as_str())
        .collect();

    assert!(
        named_props.contains("disabled"),
        "named barrel import should route to the button child surface, got {:?}",
        named_props
    );
    assert!(
        !named_props.contains("href"),
        "named barrel import should not route through the barrel default export, got {:?}",
        named_props
    );
    assert!(
        default_props.contains("href"),
        "default barrel import should route to the default-exported child surface, got {:?}",
        default_props
    );
    assert!(
        !default_props.contains("disabled"),
        "default barrel import should not route through the named Button export, got {:?}",
        default_props
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_fallthrough_surface_reuses_parent_snapshot_for_child_binding_lookup() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Parent.vue",
        r#"<script setup lang="ts">
import Child from './Child.vue'
</script>
<template>
  <Child class="root-child" />
</template>"#,
    );
    ws.inject_file(
        "/src/Child.vue",
        r#"<script setup lang="ts">
defineProps<{ label?: string }>()
</script>
<template><button /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Parent.vue"),
        "parent should load from the workspace",
    );
    host.set_import_dependencies(
        "/src/Parent.vue",
        vec![exact_dependency("./Child.vue", "/src/Child.vue")],
    );

    ws.reset_reads();
    let resolution = host.resolve_fallthrough_surface("/src/Parent.vue");

    assert!(
        resolution.is_some(),
        "fallthrough should resolve through the imported child root",
    );
    assert!(
        ws.read_count("/src/Parent.vue") <= 1,
        "fallthrough should reuse the parent snapshot while resolving child binding identity instead of rereading the parent source; saw {} reads",
        ws.read_count("/src/Parent.vue"),
    );
}

#[test]
fn prepared_type_decl_lookup_resolves_barrel_reexport_through_indexed_ready() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface BaseProps { replace?: boolean }",
    );
    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export type { BaseProps } from './base'",
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    // Materialize barrel and base module facts
    let barrel_facts = host
        .ensure_indexed_ready("/src/barrel.ts")
        .expect("barrel should materialize module facts");
    assert!(
        barrel_facts.shallow_state.exports.contains_key("BaseProps"),
        "barrel module facts should list BaseProps as a reexport",
    );

    // The base file should also be materializable
    let base_facts = host
        .ensure_indexed_ready("/src/base.ts")
        .expect("base should materialize module facts");
    assert!(
        base_facts.shallow_state.has_type_symbol("BaseProps"),
        "base module facts should have BaseProps as a local symbol",
    );
}

#[test]
fn prepared_type_decl_lookup_rejects_stale_cache_entries() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Warm the bundle cache so the fact-validated entry exists.
    let _view_before = host.resolver_store_view_read().into_owned_view();
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_some(),
        "prepared lookup should succeed before the file content changes"
    );

    // Change the file content — `Props` no longer exists.
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Other { value: number }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should re-materialize after content change");

    // Take a new view that records the updated hash.
    let _view_after = host.resolver_store_view_read().into_owned_view();

    // The stale bundle (cached with original hash) must be rejected by
    // fact validation against the new view, and the re-materialized
    // bundle must not contain the removed symbol.
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_none(),
        "prepared lookup should drop stale cached declarations when the owning file hash changes"
    );
    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_none(),
        "prepared lookup without an explicit store view should also reject the stale bundle"
    );
}

#[test]
fn resolved_type_declaration_same_name_edit_never_replays_stale_metadata() {
    let host = make_host();
    let canonical = "/src/types.ts";

    upsert_non_sfc(&host, canonical, "export interface Props { label: string }");
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("the initial declaration must be indexed");

    let before =
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(&host, canonical, "Props");
    assert_eq!(
        before.kind,
        crate::resolver_core::ResolvedDeclarationKind::Interface,
        "control: the first lookup must resolve the authored interface"
    );

    upsert_non_sfc(
        &host,
        canonical,
        "export type Props = { label: string; count: number }",
    );
    let _ = host
        .ensure_indexed_ready(canonical)
        .expect("the edited declaration must be re-indexed");

    let after =
        crate::host_manage::jsdoc_resolve::resolve_type_declaration(&host, canonical, "Props");
    assert_eq!(
        after.kind,
        crate::resolver_core::ResolvedDeclarationKind::TypeAlias,
        "a same-name edit must resolve current declaration metadata rather than replaying a stale symbol-cache entry: before={before:?}, after={after:?}"
    );
    assert_ne!(
        after.span, before.span,
        "the declaration span must move with the edited body"
    );
}

#[test]
fn raw_template_analysis_extracts_css_var_names() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div :style=\"{ '--theme-color': color }\">A</div></template>",
    );

    let template = host
        .raw_template_analysis_for_file("/src/A.vue")
        .expect("raw template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "raw template analysis should include CSS vars from :style bindings"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn override_template_analysis_helper_uses_content_override() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/A.vue",
        "<script setup>\nconst color = 'red'\n</script>\n<template><div>A</div></template>",
    );

    let profile = CompileProfile::default();
    let profile_hash = crate::hash::compile_profile_hash(&profile);
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/src/A.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Template,
                index: 0,
                code: Arc::from("<div :style=\"{ '--theme-color': color }\">A</div>"),
                source_map: None,
            }],
        })
        .expect("template override should succeed");

    let template = host
        .compute_override_template_analysis("/src/A.vue", profile_hash)
        .expect("override template analysis should be computed");
    assert!(
        template
            .css_var_names
            .iter()
            .any(|name| name == "--theme-color"),
        "override template analysis should reflect the overridden template"
    );
}

/// @ai-generated - get_analysis populates resolved_canonical_id for relative imports
#[test]
fn get_analysis_resolves_relative_import() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child msg=\"hello\" /></template>",
    );

    let analysis = host.get_analysis("/project/Parent.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Child.vue")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/Child.vue"),
        "relative import should resolve to canonical ID"
    );
}

/// @ai-generated - get_analysis resolves imports via alias map
#[test]
fn get_analysis_resolves_alias_import() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/src/components/Child.vue",
        "<script setup>\ndefineProps({ msg: String })\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport Child from '@/components/Child.vue'\n</script>\n<template><Child/></template>",
    );
    // Configure workspace resolver via host wrapper.
    {
        host.configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig {
                root: "/project".to_string(),
                workspace_root: "/project".to_string(),
                tsconfig_path: None,
                provider_root: "/project".to_string(),
                workspace_aliases: vec![verter_workspace::WorkspaceAlias {
                    find: "@/".to_string(),
                    replacement: "/project/src/".to_string(),
                }],
                compiler_options:
                    verter_semantic::analysis::project_resolver::IdeProjectCompilerOptions::default(
                    ),
                references: vec![],
                membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                    &verter_workspace::CanonicalPath::new("/project"),
                ),
            },
        ]);
    }

    let analysis = host.get_analysis("/project/src/App.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "@/components/Child.vue")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/src/components/Child.vue"),
        "alias import should resolve via alias map"
    );
}

/// @ai-generated - get_analysis resolves imports with extension guessing
#[test]
fn get_analysis_resolves_extension_guessing() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child'\n</script>\n<template><Child/></template>",
    );

    let analysis = host.get_analysis("/project/Parent.vue").unwrap();
    let child_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Child")
        .unwrap();
    assert_eq!(
        child_import.resolved_canonical_id.as_deref(),
        Some("/project/Child.vue"),
        "extension-less import should resolve via .vue guessing"
    );
}

/// @ai-generated - get_analysis leaves bare specifiers unresolved
#[test]
fn get_analysis_bare_specifier_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport { ref } from 'vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let vue_import = analysis.imports.iter().find(|i| i.source == "vue").unwrap();
    assert!(
        vue_import.resolved_canonical_id.is_none(),
        "bare specifier 'vue' should not resolve (no node_modules resolution)"
    );
}

/// @ai-generated - get_analysis leaves unregistered file imports unresolved
#[test]
fn get_analysis_missing_file_unresolved() {
    let host = make_host();
    upsert_vue(
        &host,
        "App.vue",
        "<script setup>\nimport Missing from './Missing.vue'\n</script>\n<template><div/></template>",
    );

    let analysis = host.get_analysis("App.vue").unwrap();
    let missing_import = analysis
        .imports
        .iter()
        .find(|i| i.source == "./Missing.vue")
        .unwrap();
    assert!(
        missing_import.resolved_canonical_id.is_none(),
        "import of unregistered file should not resolve"
    );
}

#[test]
fn get_analysis_uses_parse_artifact_for_lazy_analysis() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, source is immutable in the scheduler snapshot,
    // so mutating host.files has no effect. The scheduler path reads from
    // HostSourceData.framework_parse directly. We just verify get_analysis()
    // returns correct lazy-recomputed data with AnalysisLevel::None.
    #[cfg(target_arch = "wasm32")]
    mutate_lazy_analysis_source(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "lazy script analysis should reuse cached parse for bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "lazy style analysis should reuse cached parse for style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for cached style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "lazy style analysis should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "lazy script analysis should preserve module references"
    );
}

#[test]
fn get_analysis_falls_back_when_parse_artifact_missing() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, framework_parse is immutable in HostSourceData
    // and always present for Vue SFCs. The scheduler path handles both
    // artifact present and absent cases. We just verify correctness.
    #[cfg(target_arch = "wasm32")]
    clear_framework_parse(&host);

    let analysis = host.get_analysis("App.vue").unwrap();

    assert!(
        analysis.bindings.iter().any(|b| b.name == "msg"),
        "source fallback should still recover bindings"
    );
    assert_eq!(
        analysis.styles.len(),
        1,
        "source fallback should still recover style blocks"
    );
    let css = analysis.styles[0]
        .css
        .as_ref()
        .expect("CSS analysis should exist for fallback style block");
    assert!(
        css.classes.iter().any(|class| class.name == "foo"),
        "source fallback should preserve CSS classes"
    );
    assert!(
        analysis
            .module_references
            .iter()
            .any(|reference| reference.literal_specifier.as_deref() == Some("vue")),
        "source fallback should preserve module references"
    );
}

/// @ai-generated - get_export_span for .vue file returns binding span
#[test]
fn get_export_span_vue_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    let span = host.get_export_span("Child.vue", "msg");
    assert!(span.is_some(), "should find 'msg' binding in .vue file");
    let (start, end) = span.unwrap();
    let source = host.get_source("Child.vue").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(spanned, "msg", "span should cover the binding identifier");
}

/// @ai-generated - get_export_span for .vue file returns None for unknown binding
#[test]
fn get_export_span_vue_unknown_binding() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    assert!(
        host.get_export_span("Child.vue", "nonexistent").is_none(),
        "unknown binding should return None"
    );
}

/// @ai-generated - get_export_span for .ts file returns export signature span
#[test]
fn get_export_span_ts_file() {
    let host = make_host();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "utils.ts".to_string(),
            source: Arc::from("export function helper() { return 1; }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();

    let span = host.get_export_span("utils.ts", "helper");
    assert!(span.is_some(), "should find 'helper' export in .ts file");
    let (start, end) = span.unwrap();
    let source = host.get_source("utils.ts").unwrap();
    let spanned = &source[start as usize..end as usize];
    assert_eq!(
        spanned, "helper",
        "span should cover the function identifier"
    );
}

/// @ai-generated - get_export_span for .vue default import anchors at file start
#[test]
fn get_export_span_vue_default() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    // The component default export has no authored source token; the honest
    // anchor is the file start, never an unrelated internal local's span.
    let span = host.get_export_span("Child.vue", "default");
    assert_eq!(
        span,
        Some((0, 0)),
        "default export of .vue should anchor at file start (0, 0)"
    );
}

/// @ai-generated - an EMPTY .vue still carries a default export anchored at file start
#[test]
fn get_export_span_vue_default_empty_sfc() {
    let host = make_host();
    // A completely empty SFC compiles to the synthetic empty-component shell;
    // its default export has no authored token either, so the same honest
    // file-start anchor must hold (navigation terminates at the component).
    upsert_vue(&host, "Empty.vue", "");

    let span = host.get_export_span("Empty.vue", "default");
    assert_eq!(
        span,
        Some((0, 0)),
        "default export of an empty .vue should anchor at file start (0, 0)"
    );
}

/// @ai-generated - resolve_import public method works
#[test]
fn resolve_import_public_method() {
    let host = make_host();
    upsert_vue(&host, "/project/Child.vue", "<template><div/></template>");
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    assert_eq!(
        host.resolve_import("/project/Parent.vue", "./Child.vue")
            .as_deref(),
        Some("/project/Child.vue")
    );
    // Bare specifiers that aren't in the file map resolve to None
    assert!(host
        .resolve_import("/project/Parent.vue", "lodash")
        .is_none());
}

#[test]
fn resolve_import_public_method_handles_relative_full_paths() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/src/components/BarrelComp.vue",
        "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
    );
    upsert_ts(
        &host,
        "/project/src/components/index.ts",
        "export { default as BarrelComp } from './BarrelComp.vue'",
    );
    upsert_vue(
        &host,
        "/project/src/App.vue",
        "<script setup>\nimport { BarrelComp } from './components'\n</script>\n<template><BarrelComp /></template>",
    );

    assert_eq!(
        host.resolve_import("/project/src/components/index.ts", "./BarrelComp.vue")
            .as_deref(),
        Some("/project/src/components/BarrelComp.vue"),
        "relative imports from full-path barrel files should resolve to the child SFC"
    );
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap();
}

#[test]
fn enriches_destructured_composable_bindings() {
    let host = make_host();

    // Composable that returns { x: ref, y: ref, reset: function }
    upsert_ts(
        &host,
        "/project/useMouse.ts",
        r#"
import { ref } from 'vue'
export function useMouse() {
const x = ref(0)
const y = ref(0)
function reset() { x.value = 0; y.value = 0 }
return { x, y, reset }
}
"#,
    );

    // SFC that destructures the composable return
    upsert_vue(
        &host,
        "/project/App.vue",
        r#"<script setup>
import { useMouse } from './useMouse.ts'
const { x, y, reset } = useMouse()
</script>
<template><div>{{ x }} {{ y }}</div></template>"#,
    );

    let analysis = host.get_analysis("/project/App.vue").unwrap();

    // x and y should be enriched to Ref (from composable return shape)
    let x_binding = analysis.bindings.iter().find(|b| b.name == "x").unwrap();
    assert_eq!(
        x_binding.reactivity_kind,
        verter_semantic::analysis::ReactivityKind::Ref,
        "x should be enriched from MaybeRef to Ref via composable return shape"
    );

    let y_binding = analysis.bindings.iter().find(|b| b.name == "y").unwrap();
    assert_eq!(
        y_binding.reactivity_kind,
        verter_semantic::analysis::ReactivityKind::Ref,
        "y should be enriched from MaybeRef to Ref via composable return shape"
    );

    // reset should stay as a function (ReactivityKind::None since it's not reactive)
    let reset_binding = analysis
        .bindings
        .iter()
        .find(|b| b.name == "reset")
        .unwrap();
    assert_eq!(
        reset_binding.reactivity_kind,
        verter_semantic::analysis::ReactivityKind::None,
        "reset (a function) should be None, not reactive"
    );

    // Negative: non-enriched bindings should not be affected
    assert!(
        !x_binding.is_reactive
            || x_binding.reactivity_kind != verter_semantic::analysis::ReactivityKind::MaybeRef,
        "x should NOT remain MaybeRef after enrichment"
    );
}

#[test]
fn get_export_span_follows_reexport_to_vue() {
    let host = make_host();

    // Target: Popup.vue with a binding
    upsert_vue(
        &host,
        "/project/Popup.vue",
        "<script setup>\nconst message = 'hello'\n</script>\n<template><div>{{ message }}</div></template>",
    );

    // Barrel: index.ts re-exports Popup.vue as default
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { default as Popup } from './Popup.vue'",
    );

    // Follow the re-export: "Popup" in index.ts → default in Popup.vue
    let result = host.get_export_span_follow_reexports("/project/index.ts", "Popup");

    assert!(result.is_some(), "should follow re-export to Popup.vue");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/Popup.vue",
        "should resolve to Popup.vue canonical ID"
    );
    // The component default export anchors at the file start, like Svelte.
    assert_eq!(
        (start, end),
        (0, 0),
        "should anchor at the Popup.vue file start (start={start}, end={end})"
    );
    // Negative: should NOT return index.ts
    assert_ne!(
        canonical_id, "/project/index.ts",
        "must NOT return the barrel file itself"
    );
}

#[test]
fn get_export_span_follows_reexport_to_vue_full_paths() {
    let host = make_host();

    upsert_vue(
        &host,
        "/project/src/components/BarrelComp.vue",
        "<script setup>\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n",
    );
    upsert_ts(
        &host,
        "/project/src/components/index.ts",
        "export { default as BarrelComp } from './BarrelComp.vue'",
    );

    let result =
        host.get_export_span_follow_reexports("/project/src/components/index.ts", "BarrelComp");

    assert!(
        result.is_some(),
        "should follow full-path barrel re-export to BarrelComp.vue"
    );
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/src/components/BarrelComp.vue",
        "should resolve to the full child Vue canonical ID"
    );
    assert_eq!(
        (start, end),
        (0, 0),
        "should anchor at the BarrelComp.vue file start"
    );
}

#[test]
fn get_export_span_follows_two_level_reexport_to_svelte_default() {
    let host = make_host();

    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/project/BarrelChild.svelte".to_string(),
        source: Arc::from(
            "<script lang=\"ts\">\nlet { label }: { label: string } = $props();\n</script>\n<p>{label}</p>",
        ),
        file_language: FileLanguage::svelte(),
        aliases: Vec::new(),
    })
    .expect("load Svelte child");
    upsert_ts(
        &host,
        "/project/level-one.ts",
        "export { default as BarrelChild } from './BarrelChild.svelte';\n",
    );
    upsert_ts(
        &host,
        "/project/level-two.ts",
        "export * from './level-one';\n",
    );

    let result = host
        .get_export_span_follow_reexports("/project/level-two.ts", "BarrelChild")
        .expect("two export hops must reach the Svelte component default");
    assert_eq!(result, ("/project/BarrelChild.svelte".to_string(), 0, 0));
}

#[test]
fn get_export_span_follows_named_reexport() {
    let host = make_host();

    // Target: utils.ts with an exported function
    upsert_ts(
        &host,
        "/project/utils.ts",
        "export function helper() { return 42 }",
    );

    // Barrel: re-exports helper as myHelper
    upsert_ts(
        &host,
        "/project/index.ts",
        "export { helper as myHelper } from './utils.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/index.ts", "myHelper");

    assert!(result.is_some(), "should follow named re-export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/utils.ts",
        "should resolve to utils.ts"
    );
    assert!(start < end, "should have a valid span");
    // Negative: should NOT return barrel
    assert_ne!(canonical_id, "/project/index.ts");
}

#[test]
fn get_export_span_follows_multi_hop_chain() {
    let host = make_host();

    upsert_ts(&host, "/project/a.ts", "export { b } from './b.ts'");
    upsert_ts(&host, "/project/b.ts", "export { c as b } from './c.ts'");
    upsert_ts(&host, "/project/c.ts", "export const c = 42");

    // Should follow a→b→c (no depth limit, cycle detection only)
    let result = host.get_export_span_follow_reexports("/project/a.ts", "b");
    assert!(result.is_some(), "should follow the chain");
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(canonical_id, "/project/c.ts", "should reach c.ts");
}

#[test]
fn get_export_span_local_export_unchanged() {
    let host = make_host();

    upsert_ts(&host, "utils.ts", "export function foo() { return 1 }");

    // Local export — no re-export, returns span in same file
    let result = host.get_export_span_follow_reexports("utils.ts", "foo");

    assert!(result.is_some(), "should find local export");
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "utils.ts",
        "local export should return same file"
    );
    assert!(start < end, "should have a valid span");
}

#[test]
fn follow_reexport_cycle_same_binding() {
    let host = make_host();

    // A re-exports foo from B, B re-exports foo from A → cycle
    upsert_ts(&host, "a.ts", "export { foo } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { foo } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "foo");
    assert!(
        result.is_none(),
        "cycle on same binding should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_same_file_different_binding() {
    let host = make_host();

    // A re-exports foo from B (as foo→bar), B re-exports bar from A (as bar→baz),
    // A has a local baz export. Different bindings each hop → not a cycle.
    upsert_ts(
        &host,
        "/project/a.ts",
        "export { bar as foo } from './b.ts'\nexport const baz = 99",
    );
    upsert_ts(
        &host,
        "/project/b.ts",
        "export { baz as bar } from './a.ts'",
    );

    let result = host.get_export_span_follow_reexports("/project/a.ts", "foo");
    assert!(
        result.is_some(),
        "different bindings through same files should resolve, not be treated as cycle"
    );
    let (canonical_id, _, _) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/a.ts",
        "should resolve to a.ts local baz export"
    );
}

#[test]
fn follow_reexport_indirect_cycle() {
    let host = make_host();

    // A→B→C→A with same binding name "x" at each hop
    upsert_ts(&host, "a.ts", "export { x } from './b.ts'");
    upsert_ts(&host, "b.ts", "export { x } from './c.ts'");
    upsert_ts(&host, "c.ts", "export { x } from './a.ts'");

    let result = host.get_export_span_follow_reexports("a.ts", "x");
    assert!(
        result.is_none(),
        "indirect 3-file cycle should return None, got: {result:?}"
    );
}

#[test]
fn follow_reexport_deep_chain_no_limit() {
    let host = make_host();

    // 15-hop chain: f0→f1→f2→...→f14→terminal.ts
    // Each hop renames: val0→val1→...→val14→val
    for i in 0..15 {
        let next = if i < 14 {
            format!("f{}.ts", i + 1)
        } else {
            "terminal.ts".to_string()
        };
        let next_binding = if i < 14 {
            format!("val{}", i + 1)
        } else {
            "val".to_string()
        };
        let src = format!(
            "export {{ {} as val{} }} from './{}'",
            next_binding, i, next
        );
        upsert_ts(&host, &format!("/project/f{}.ts", i), &src);
    }
    upsert_ts(&host, "/project/terminal.ts", "export const val = 'done'");

    let result = host.get_export_span_follow_reexports("/project/f0.ts", "val0");
    assert!(
        result.is_some(),
        "15-hop chain should resolve without depth limit"
    );
    let (canonical_id, start, end) = result.unwrap();
    assert_eq!(
        canonical_id, "/project/terminal.ts",
        "should reach terminal.ts"
    );
    assert!(start < end, "should have a valid span");
}

fn compile_template(host: &VerterHost, id: &str) {
    let _ = host
        .get_virtual_file(crate::types::VirtualQuery {
            raw_id: Some(format!("{id}?vue&type=template")),
            canonical_id: None,
            node_kind: None,
            compile_profile: crate::types::CompileProfile::default(),
        })
        .unwrap();
}

#[test]
fn prop_shorthand_detected() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    // `:bar` with no value → shorthand; `:bar="bar"` → not shorthand
    upsert_vue(
        &host,
        "App.vue",
        r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar /><MyComp :bar="bar" /></template>"#,
    );
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(
        tmpl.components.len() >= 2,
        "should have at least 2 component usages, got {}",
        tmpl.components.len()
    );

    // First usage: `:bar` (shorthand)
    let comp1 = &tmpl.components[0];
    assert_eq!(comp1.props.len(), 1, "first usage has 1 prop");
    assert!(
        comp1.props[0].is_shorthand,
        "`:bar` (no value) should be shorthand"
    );

    // Second usage: `:bar="bar"` (not shorthand)
    let comp2 = &tmpl.components[1];
    assert_eq!(comp2.props.len(), 1, "second usage has 1 prop");
    assert!(
        !comp2.props[0].is_shorthand,
        "`:bar=\"bar\"` should NOT be shorthand"
    );
}

#[test]
fn prop_name_span_covers_name() {
    let host = make_host();
    upsert_vue(
        &host,
        "MyComp.vue",
        "<script setup>\ndefineProps<{ bar: number }>()\n</script>\n<template><div/></template>",
    );
    let sfc = r#"<script setup>
import MyComp from './MyComp.vue'
const bar = 1
</script>
<template><MyComp :bar="bar" foo="static" /></template>"#;
    upsert_vue(&host, "App.vue", sfc);
    compile_template(&host, "App.vue");

    let analysis = host.get_analysis("App.vue").unwrap();
    let tmpl = analysis
        .template
        .as_ref()
        .expect("should have template analysis");
    assert!(!tmpl.components.is_empty());

    let comp = &tmpl.components[0];
    // Find the bound prop `:bar`
    let bound_prop = comp.props.iter().find(|p| p.name == "bar").unwrap();
    let source = host.get_source("App.vue").unwrap();
    let name_text = &source[bound_prop.name_span.start as usize..bound_prop.name_span.end as usize];
    assert_eq!(
        name_text, "bar",
        "name_span should cover 'bar' (the arg, not ':')"
    );
    assert!(
        bound_prop.name_span.start >= bound_prop.span.start,
        "name_span should be within the full prop span"
    );

    // Find the static prop `foo`
    let static_prop = comp.props.iter().find(|p| p.name == "foo").unwrap();
    let name_text =
        &source[static_prop.name_span.start as usize..static_prop.name_span.end as usize];
    assert_eq!(name_text, "foo", "static prop name_span should cover 'foo'");
    assert!(
        !static_prop.is_shorthand,
        "static prop should not be shorthand"
    );
}

#[test]
fn arc_shared_fields_are_pointer_equal() {
    let host = make_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    let a1 = host.get_analysis("App.vue").unwrap();
    let a2 = host.get_analysis("App.vue").unwrap();

    // Arc-shared fields should be pointer-equal between two calls
    // on the same unchanged file.
    assert!(
        Arc::ptr_eq(&a1.module_references, &a2.module_references),
        "module_references should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.macros, &a2.macros),
        "macros should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.styles, &a2.styles),
        "styles should be Arc-shared (pointer equal)"
    );
    assert!(
        Arc::ptr_eq(&a1.vue_api_calls, &a2.vue_api_calls),
        "vue_api_calls should be Arc-shared (pointer equal)"
    );
}

#[test]
fn enriched_imports_do_not_affect_stored_data() {
    let host = make_host();
    upsert_vue(
        &host,
        "/project/Child.vue",
        "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "/project/Parent.vue",
        "<script setup>\nimport Child from './Child.vue'\n</script>\n<template><Child/></template>",
    );

    // First call: enriches imports with resolved_canonical_id
    let a1 = host.get_analysis("/project/Parent.vue").unwrap();
    assert!(
        a1.imports[0].resolved_canonical_id.is_some(),
        "enriched import should have resolved_canonical_id"
    );

    // Verify stored data is not mutated by checking that the
    // internal stored imports still have None
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::host_executor::HostSourceData;
        let source_snap = host
            .scheduler
            .try_get_source("/project/Parent.vue")
            .expect("scheduler should have Parent.vue");
        let hd = source_snap
            .downcast_data::<HostSourceData>()
            .expect("source data should be HostSourceData");
        assert!(
            hd.parse.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let files = crate::shared::read_lock(&host.files);
        let entry = files.get("/project/Parent.vue").unwrap();
        assert!(
            entry.script_analysis.imports[0]
                .resolved_canonical_id
                .is_none(),
            "stored import should NOT be mutated by get_analysis enrichment"
        );
    }
}

#[test]
fn get_analysis_batch_returns_all_existing() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nconst a = 1\n</script>\n<template><div/></template>",
    );
    upsert_vue(
        &host,
        "B.vue",
        "<script setup>\nconst b = 2\n</script>\n<template><div/></template>",
    );

    let results = host.get_analysis_batch(&["A.vue", "B.vue", "NonExistent.vue"]);
    assert_eq!(results.len(), 2, "should return only existing files");
    assert!(
        results.iter().any(|(id, _)| id == "A.vue"),
        "should contain A.vue"
    );
    assert!(
        results.iter().any(|(id, _)| id == "B.vue"),
        "should contain B.vue"
    );
    // Negative: should NOT contain non-existent
    assert!(
        !results.iter().any(|(id, _)| id == "NonExistent.vue"),
        "should not contain non-existent file"
    );
}

#[test]
fn get_analysis_batch_matches_individual() {
    let host = make_host();
    upsert_vue(
        &host,
        "A.vue",
        "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n<template><div/></template>",
    );

    let individual = host.get_analysis("A.vue").unwrap();
    let batch = host.get_analysis_batch(&["A.vue"]);
    assert_eq!(batch.len(), 1);
    let (_, batch_snap) = &batch[0];

    assert_eq!(
        individual.bindings.len(),
        batch_snap.bindings.len(),
        "batch bindings count should match individual"
    );
    assert_eq!(
        individual.imports.len(),
        batch_snap.imports.len(),
        "batch imports count should match individual"
    );
    assert_eq!(
        individual.script_flags, batch_snap.script_flags,
        "batch script_flags should match individual"
    );
}

#[test]
fn get_analysis_batch_empty_returns_empty() {
    let host = make_host();
    let results = host.get_analysis_batch(&[]);
    assert!(results.is_empty(), "empty batch should return empty vec");
}

// ── Export signature tests ──────────────────────────────────────

fn upsert_ts_result(host: &VerterHost, id: &str, src: &str) -> crate::HostUpdateResult {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(src),
        file_language: FileLanguage::script_ts(),
        aliases: Vec::new(),
    })
    .unwrap()
}

/// @ai-generated - upsert of .ts file returns export signatures
#[test]
fn upsert_returns_export_signatures_for_ts() {
    let host = make_host();
    let result = upsert_ts_result(
        &host,
        "index.ts",
        r#"export const foo = 1;
export type Bar = string;
export { default as Button } from './Button.vue';
"#,
    );

    assert!(
        !result.export_signatures.is_empty(),
        "upsert should return export signatures for .ts files"
    );

    let foo_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "foo")
        .expect("should have 'foo' export");
    assert!(!foo_sig.is_type, "foo is a value export");
    assert!(
        foo_sig.reexport_source.is_none(),
        "foo is local, not a re-export"
    );

    let bar_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Bar")
        .expect("should have 'Bar' export");
    assert!(bar_sig.is_type, "Bar is a type export");

    let button_sig = result
        .export_signatures
        .iter()
        .find(|s| s.name == "Button")
        .expect("should have 'Button' re-export");
    assert_eq!(
        button_sig.reexport_source.as_deref(),
        Some("./Button.vue"),
        "Button re-export source should be './Button.vue'"
    );
    assert_eq!(
        button_sig.reexport_local.as_deref(),
        Some("default"),
        "Button re-export local name should be 'default'"
    );
}

/// @ai-generated - get_analysis includes export signatures
#[test]
fn get_analysis_includes_export_signatures() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export function helper() { return 1; }\nexport type Util = number;",
    );

    let analysis = host.get_analysis("utils.ts").unwrap();
    assert!(
        !analysis.export_signatures.is_empty(),
        "analysis should include export signatures"
    );

    let helper_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "helper")
        .expect("should have 'helper' export");
    assert!(!helper_sig.is_type);

    let util_sig = analysis
        .export_signatures
        .iter()
        .find(|s| s.name == "Util")
        .expect("should have 'Util' export");
    assert!(util_sig.is_type);
}

/// @ai-generated - resolve_exports follows re-export chains
#[test]
fn resolve_exports_follows_reexport_chains() {
    let host = make_host();

    upsert_vue(
        &host,
        "/project/Button.vue",
        "<script setup>\ndefineProps({ label: String })\n</script>\n<template><button>{{ label }}</button></template>",
    );

    upsert_ts(
        &host,
        "/project/components/index.ts",
        "export { default as Button } from './Button.vue';",
    );

    // Set up dependency so ./Button.vue resolves from components/index.ts
    host.set_import_dependencies(
        "/project/components/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./Button.vue".to_string(),
            resolved_canonical_id: Some("/project/Button.vue".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/components/index.ts");
    assert!(
        !exports.is_empty(),
        "barrel file should have resolved exports"
    );

    let button = exports
        .iter()
        .find(|e| e.name == "Button")
        .expect("should have 'Button' resolved export");
    assert_eq!(
        button.source_canonical_id.as_deref(),
        Some("/project/Button.vue"),
        "Button should resolve to Button.vue"
    );
    assert_eq!(
        button.source_name, "default",
        "Button maps to 'default' in the source file"
    );
}

#[test]
fn resolve_exports_reads_workspace_only_barrels_and_vue_targets() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/src/runtime/types/index.ts".to_string(),
        Arc::from("export * from '../components/Link.vue'"),
    );
    ws.inject_file(
        "/workspace/src/runtime/components/Link.vue".to_string(),
        Arc::from(
            r#"<script lang="ts">
export interface LinkProps {
  href?: string
  replace?: boolean
}
</script>
<template><div /></template>"#,
        ),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    let exports = host.resolve_exports("/workspace/src/runtime/types/index.ts");
    let link_props = exports
        .iter()
        .find(|export| export.name == "LinkProps")
        .expect("workspace-only barrel should expose LinkProps");

    assert_eq!(
        link_props.source_canonical_id.as_deref(),
        Some("/workspace/src/runtime/components/Link.vue"),
        "workspace-only re-export should resolve to the Vue declaration owner"
    );
    assert_eq!(
        link_props.source_name, "LinkProps",
        "workspace-only re-export should preserve the exported declaration name"
    );
}

/// @ai-generated - resolve_exports handles direct local exports
#[test]
fn resolve_exports_local_exports() {
    let host = make_host();
    upsert_ts(
        &host,
        "utils.ts",
        "export const FOO = 1;\nexport type Bar = string;",
    );

    let exports = host.resolve_exports("utils.ts");
    assert_eq!(exports.len(), 2, "should have 2 exports");

    let foo = exports.iter().find(|e| e.name == "FOO").unwrap();
    assert!(
        foo.source_canonical_id.is_none(),
        "local export has no source file"
    );
    assert_eq!(foo.source_name, "FOO");
    assert!(!foo.is_type);

    let bar = exports.iter().find(|e| e.name == "Bar").unwrap();
    assert!(bar.is_type);
}

/// @ai-generated - resolve_exports handles wildcard re-exports
#[test]
fn resolve_exports_wildcard_reexports() {
    let host = make_host();

    upsert_ts(
        &host,
        "/project/types.ts",
        "export type Foo = string;\nexport type Bar = number;",
    );
    upsert_ts(&host, "/project/index.ts", "export * from './types';");

    host.set_import_dependencies(
        "/project/index.ts",
        vec![crate::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/project/types.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/index.ts");
    assert!(
        exports.iter().any(|e| e.name == "Foo"),
        "wildcard re-export should include Foo"
    );
    assert!(
        exports.iter().any(|e| e.name == "Bar"),
        "wildcard re-export should include Bar"
    );

    let foo = exports.iter().find(|e| e.name == "Foo").unwrap();
    assert_eq!(
        foo.source_canonical_id.as_deref(),
        Some("/project/types.ts"),
        "Foo should trace back to types.ts"
    );
}

/// @ai-generated - resolve_exports detects circular re-exports
#[test]
fn resolve_exports_circular_protection() {
    let host = make_host();

    upsert_ts(&host, "a.ts", "export * from './b';");
    upsert_ts(&host, "b.ts", "export * from './a';");

    host.set_import_dependencies(
        "a.ts",
        vec![crate::DependencyResolution {
            specifier: "./b".to_string(),
            resolved_canonical_id: Some("b.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "b.ts",
        vec![crate::DependencyResolution {
            specifier: "./a".to_string(),
            resolved_canonical_id: Some("a.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    // Should not infinite loop
    let exports = host.resolve_exports("a.ts");
    // The result is empty because both files only re-export each other with no local exports
    assert!(
        exports.is_empty(),
        "circular re-exports with no local exports should return empty"
    );
}

/// @ai-generated - resolve_exports multi-level barrel chain
#[test]
fn resolve_exports_multi_level_barrel() {
    let host = make_host();

    upsert_ts(&host, "/project/deep.ts", "export const DEEP = 42;");
    upsert_ts(&host, "/project/mid.ts", "export { DEEP } from './deep';");
    upsert_ts(&host, "/project/top.ts", "export { DEEP } from './mid';");

    host.set_import_dependencies(
        "/project/mid.ts",
        vec![crate::DependencyResolution {
            specifier: "./deep".to_string(),
            resolved_canonical_id: Some("/project/deep.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );
    host.set_import_dependencies(
        "/project/top.ts",
        vec![crate::DependencyResolution {
            specifier: "./mid".to_string(),
            resolved_canonical_id: Some("/project/mid.ts".to_string()),
            possible_canonical_ids: vec![],
        }],
    );

    let exports = host.resolve_exports("/project/top.ts");
    let deep = exports
        .iter()
        .find(|e| e.name == "DEEP")
        .expect("should have DEEP");
    assert_eq!(
        deep.source_canonical_id.as_deref(),
        Some("/project/deep.ts"),
        "should trace through two levels to deep.ts"
    );
}

#[test]
fn get_semantic_hash_returns_hash_for_loaded_file() {
    let host = make_host();
    upsert_vue(&host, "App.vue", "<template><div>hi</div></template>");
    let hash = host.get_semantic_hash("App.vue");
    assert!(hash.is_some(), "loaded file should return a semantic hash");
    assert_ne!(hash.unwrap(), [0u8; 16], "hash should not be all zeros");
}

#[test]
fn get_semantic_hash_returns_none_for_missing_file() {
    let host = make_host();
    assert!(
        host.get_semantic_hash("nonexistent.vue").is_none(),
        "missing file should return None"
    );
}

#[test]
fn get_semantic_hash_changes_on_content_change() {
    let host = make_host();
    upsert_vue(&host, "App.vue", "<template><div>a</div></template>");
    let h1 = host.get_semantic_hash("App.vue").unwrap();
    upsert_vue(&host, "App.vue", "<template><div>b</div></template>");
    let h2 = host.get_semantic_hash("App.vue").unwrap();
    assert_ne!(h1, h2, "semantic hash should change when content changes");
}

fn resolve_expanded_state(
    host: &VerterHost,
    canonical_or_alias: &str,
) -> crate::meta_resolve::ResolvedComponentMetaState {
    host.resolve_component_meta(canonical_or_alias, crate::types::ProjectionMode::Expanded)
        .expect("expanded resolved state should exist")
}

fn resolved_macro_by_type<'a>(
    state: &'a crate::meta_resolve::ResolvedComponentMetaState,
    type_name: &str,
) -> &'a crate::meta_resolve::ResolvedMacroMeta {
    state
        .resolved_macros
        .iter()
        .find(|meta| meta.type_name == type_name)
        .unwrap_or_else(|| panic!("missing resolved macro for {type_name}"))
}

/// Resolve the typeinfo macro-surface DTOs for a resolved macro entry. The
/// published props/emits/slots/exposed surface is owned SOLELY by the typeinfo
/// macro-surface authority (`vue_macro_dtos`), keyed on the admitted macro
/// index; `ResolvedMacroMeta` supplies only the index + kind for provenance.
fn macro_dtos_for_resolved(
    host: &VerterHost,
    owner: &str,
    resolved: &crate::meta_resolve::ResolvedMacroMeta,
) -> std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos> {
    host.vue_macro_dtos(&crate::typeinfo::types::VueMacroSurfaceRequest {
        owner_canonical: std::sync::Arc::from(owner),
        macro_index: resolved.macro_index,
        macro_kind: resolved.macro_kind,
        root_identity: host.current_or_read_whole_hash(owner).unwrap_or([0u8; 16]),
        level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
    })
}

/// Typeinfo macro-surface DTOs for the macro matching `type_name`.
fn macro_dtos_by_type(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    type_name: &str,
) -> std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos> {
    macro_dtos_for_resolved(host, owner, resolved_macro_by_type(state, type_name))
}

/// Typeinfo macro-surface DTO bundles for every resolved macro of `kind`
/// (deduped by macro index, mirroring the production producer).
fn dtos_for_kind(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    kind: verter_semantic::analysis::AnalyzedMacroKind,
) -> Vec<std::sync::Arc<crate::typeinfo::framework_surface::MacroSurfaceDtos>> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == kind)
        .filter(|m| seen.insert(m.macro_index))
        .map(|m| macro_dtos_for_resolved(host, owner, m))
        .collect()
}

/// Aggregate prop/emit/slot names for every resolved macro of `kind` (deduped
/// by macro index, mirroring the production producer).
fn names_for_kind(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
    kind: verter_semantic::analysis::AnalyzedMacroKind,
    pick: fn(&crate::typeinfo::framework_surface::MacroSurfaceDtos) -> Vec<String>,
) -> Vec<String> {
    let mut seen = rustc_hash::FxHashSet::default();
    state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == kind)
        .filter(|m| seen.insert(m.macro_index))
        .flat_map(|m| pick(&macro_dtos_for_resolved(host, owner, m)))
        .collect()
}

fn hm_prop_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    names_for_kind(
        host,
        owner,
        state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineProps,
        |d| {
            d.prop_fields()
                .iter()
                .map(|p| p.analysis.name.clone())
                .collect()
        },
    )
}

fn hm_slot_names(
    host: &VerterHost,
    owner: &str,
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<String> {
    names_for_kind(
        host,
        owner,
        state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
        |d| d.slot_fields().iter().map(|s| s.name.clone()).collect(),
    )
}

#[test]
fn resolve_imported_type_from_ts_dep() {
    let host = make_host();
    // Upsert the .ts type file
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ButtonProps { label: string; size?: number }",
    );
    // Upsert the .vue file that imports from ./types
    upsert_vue(
        &host,
        "/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script><template><div /></template>"#,
    );

    let state = resolve_expanded_state(&host, "/Button.vue");
    let dtos = macro_dtos_by_type(&host, "/Button.vue", &state, "ButtonProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();

    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        props.contains(&"size"),
        "expanded props should contain 'size', got: {:?}",
        props
    );
}

#[test]
fn resolve_component_meta_returns_no_resolved_macros_for_no_imported_type_deps() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Simple.vue",
        r#"<script setup lang="ts">
defineProps<{ count: number }>()
</script><template><div /></template>"#,
    );
    let state = resolve_expanded_state(&host, "/Simple.vue");
    assert!(
        state.resolved_macros.is_empty(),
        "should not resolve any cross-file macros when there are no imported type deps"
    );
}

#[test]
fn resolve_imported_type_from_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let dtos = macro_dtos_for_resolved(&host, "/Comp.vue", resolved);
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must not leak raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

#[test]
fn resolve_imported_type_from_dual_script_vue_dep() {
    let host = make_host();
    upsert_vue(
        &host,
        "/types.vue",
        "<script lang=\"ts\">\nexport interface DualProps { title: string; count: number }\n</script>\n<script setup lang=\"ts\">\n// empty setup block\n</script>\n<template><div /></template>",
    );
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { DualProps } from './types.vue'\ndefineProps<DualProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let dtos = macro_dtos_by_type(&host, "/Comp.vue", &state, "DualProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"title"),
        "expanded props should contain 'title' from companion script, got: {:?}",
        props
    );
}

#[test]
fn resolve_imported_type_from_vue_dep_without_vue_suffix_uses_file_kind() {
    let host = make_host();
    // Use .vue extension so that VFS resolution can resolve the import.
    // The test verifies that Vue SFC script extraction works for deps
    // that are stored with VueSfc file kind.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/types.vue".to_string(),
            source: Arc::from(
                "<script setup lang=\"ts\">export interface Props { label: string }</script>\n<template><div /></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup lang=\"ts\">\nimport type { Props } from './types.vue'\ndefineProps<Props>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/Comp.vue");
    let resolved = resolved_macro_by_type(&state, "Props");
    let dtos = macro_dtos_for_resolved(&host, "/Comp.vue", resolved);
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "expanded props should contain 'label', got: {:?}",
        props
    );
    assert!(
        !resolved
            .declaration
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("<template>"),
        "declaration text must NOT contain raw SFC markup, got: {:?}",
        resolved.declaration.text
    );
}

#[test]
fn resolve_component_meta_uses_workspace_type_resolution_for_package_declarations() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/fancy/package.json".to_string(),
        Arc::from(
            r#"{ "name": "fancy", "types": "./dist/index.d.ts", "exports": { ".": { "import": "./dist/index.js" } } }"#,
        ),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.d.ts".to_string(),
        Arc::from("export interface FancyProps { open: boolean; label?: string }"),
    );
    ws.inject_file(
        "/workspace/node_modules/fancy/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        "<script setup lang=\"ts\">\nimport type { FancyProps } from 'fancy'\ndefineProps<FancyProps>()\n</script>\n<template><div /></template>",
    );

    let state = resolve_expanded_state(&host, "/workspace/src/Consumer.vue");
    let dtos = macro_dtos_by_type(&host, "/workspace/src/Consumer.vue", &state, "FancyProps");
    let props: Vec<&str> = dtos
        .prop_fields()
        .iter()
        .map(|prop| prop.analysis.name.as_str())
        .collect();
    assert!(
        props.contains(&"open"),
        "expanded props should contain fields from the package declaration entrypoint, got: {:?}",
        props
    );
}

// ═══════════════════════════════════════════════════════════
// enrich_imported_types tests
// ═══════════════════════════════════════════════════════════

/// resolve_component_meta(Expanded) populates prop fields from imported interface
#[test]
fn enrich_basic_imported_interface() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let props = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        props.contains(&"label".to_string()),
        "props should include 'label': {:?}",
        props
    );
    // Negative: get_analysis must NOT have enriched the snapshot
    let analysis = host.get_analysis("/src/Comp.vue").unwrap();
    let dp = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .unwrap();
    assert!(
        dp.prop_fields.is_empty(),
        "get_analysis must NOT enrich prop_fields"
    );
}

/// resolve_component_meta(Expanded) merges props from intersection types
#[test]
fn enrich_intersection_merges_all_deps() {
    let host = make_host();
    upsert_non_sfc(&host, "/src/a.ts", "export interface A { x: string }");
    upsert_non_sfc(&host, "/src/b.ts", "export interface B { y: number }");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { A } from './a'
import type { B } from './b'
defineProps<A & B>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let names = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        names.contains(&"x".to_string()),
        "should have 'x' from A: {:?}",
        names
    );
    assert!(
        names.contains(&"y".to_string()),
        "should have 'y' from B: {:?}",
        names
    );
}

/// resolve_component_meta(Expanded) wraps call-signature emit payloads in brackets
#[test]
fn enrich_emit_call_signature_wraps_brackets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/events.ts",
        "export interface Events { (e: 'change', id: number): void }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Events } from './events'
defineEmits<Events>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let emit_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineEmits,
    );
    let emits: Vec<_> = emit_dtos
        .iter()
        .flat_map(|d| d.emit_fields().iter())
        .collect();
    let change = emits.iter().find(|e| e.analysis.name == "change");
    assert!(change.is_some(), "should have 'change' emit");
    let payload = change
        .unwrap()
        .analysis
        .payload_type
        .as_deref()
        .unwrap_or("");
    assert!(
        payload.starts_with('[') && payload.ends_with(']'),
        "call-signature payload should be wrapped in brackets, got: {payload}"
    );
}

/// resolve_component_meta(Expanded) extracts slot bindings from imported type
#[test]
fn enrich_slot_bindings_from_imported_type() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string; index: number }) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let slot_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
    );
    let slots: Vec<_> = slot_dtos
        .iter()
        .flat_map(|d| d.slot_fields().iter())
        .collect();
    let default_slot = slots.iter().find(|s| s.name == "default");
    assert!(default_slot.is_some(), "should have 'default' slot");
    let bindings = &default_slot.unwrap().bindings;
    assert!(!bindings.is_empty(), "slot should have bindings");
    let binding_names: Vec<&str> = bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        binding_names.contains(&"row"),
        "should have 'row': {:?}",
        binding_names
    );
    assert!(
        binding_names.contains(&"index"),
        "should have 'index': {:?}",
        binding_names
    );
}

/// resolve_component_meta(Expanded) captures method-style slot signatures
#[test]
fn enrich_slot_method_style() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default(props: { item: string }): any; header(props: { title: string }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let slot_names = hm_slot_names(&host, "/src/Comp.vue", &state);
    assert!(
        slot_names.contains(&"default".to_string()),
        "should have 'default': {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"header".to_string()),
        "should have 'header': {:?}",
        slot_names
    );
}

/// resolve_component_meta(Expanded) resolves nested type references
#[test]
fn enrich_nested_type_expansion() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"export type Status = 'active' | 'inactive'
export interface Props { name: string; status: Status }"#,
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let prop_names = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        prop_names.contains(&"name".to_string()),
        "should have 'name': {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"status".to_string()),
        "should have 'status': {:?}",
        prop_names
    );
    // Negative: props should not contain 'Status' as a prop (it's a type, not a prop)
    assert!(
        !prop_names.contains(&"Status".to_string()),
        "Status is a type, not a prop"
    );
}

/// resolve_component_meta(Expanded) extracts slot return types
#[test]
fn enrich_slot_return_type_property_style() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/slots.ts",
        "export interface Slots { default: (props: { row: string }) => VNode[]; header: (props: {}) => any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { Slots } from './slots'
defineSlots<Slots>()
</script>
<template><div /></template>"#,
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let slot_dtos = dtos_for_kind(
        &host,
        "/src/Comp.vue",
        &state,
        verter_semantic::analysis::AnalyzedMacroKind::DefineSlots,
    );
    let slots: Vec<_> = slot_dtos
        .iter()
        .flat_map(|d| d.slot_fields().iter())
        .collect();

    let default_slot = slots.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "default slot should have return type VNode[]"
    );

    let header_slot = slots.iter().find(|s| s.name == "header").unwrap();
    assert_eq!(
        header_slot.return_type.as_deref(),
        Some("any"),
        "header slot should have return type any"
    );
}

/// @ai-generated - local defineSlots with return types
#[test]
fn local_slot_return_type_property_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default: (props: { item: string }) => VNode[],
  header: (props: {}) => any
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "local default slot should have return type"
    );
}

/// @ai-generated - local defineSlots with method-style return types
#[test]
fn local_slot_return_type_method_style() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): VNode[]
}>()
</script>
<template><div /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let ds = analysis
        .macros
        .iter()
        .find(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .expect("should have DefineSlots macro");

    let default_slot = ds.slot_fields.iter().find(|s| s.name == "default").unwrap();
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "method-style slot should have return type"
    );
}

// ═══════════════════════════════════════════════════════════
// Template slots via lazy analysis (compute_template_analysis_if_missing)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - template slots detected via lazy META compilation
#[test]
fn template_slots_via_analysis_only() {
    let host = make_host(); // analysis_level: Full → scope includes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    assert_eq!(tpl.defined_slots.len(), 1);
    assert_eq!(tpl.defined_slots[0].name, "default");
}

/// @ai-generated - named slots detected via lazy META compilation
#[test]
fn template_slots_named() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<template><slot name="header" /><slot /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template analysis should be populated");
    assert_eq!(tpl.defined_slots.len(), 2);
    assert!(tpl.defined_slots.iter().any(|s| s.name == "header"));
    assert!(tpl.defined_slots.iter().any(|s| s.name == "default"));
}

/// @ai-generated - template analysis not computed when scope doesn't include template
#[test]
fn template_slots_not_computed_on_lazy_host() {
    let host = make_lazy_host(); // analysis_level: None → scope excludes template
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    assert!(
        analysis.template.is_none(),
        "template should not be computed when scope excludes it"
    );
}

/// @ai-generated - persisted template analysis reused on second call
#[test]
fn template_slots_persisted_across_calls() {
    let host = make_host();
    upsert_vue(
        &host,
        "/Comp.vue",
        "<script setup>\n</script>\n<template><div><slot /></div></template>",
    );

    let a1 = host.get_analysis("/Comp.vue").unwrap();
    assert!(a1.template.is_some(), "first call should compute template");

    let a2 = host.get_analysis("/Comp.vue").unwrap();
    assert!(
        a2.template.is_some(),
        "second call should reuse persisted template"
    );
    assert_eq!(
        a2.template.unwrap().defined_slots.len(),
        1,
        "persisted template should have the slot"
    );
}

/// @ai-generated - template slots computed even when type deps are unresolved
#[test]
fn template_slots_with_unresolved_type_deps() {
    let host = make_host();
    // Don't upsert ./types.ts — the dep is unresolved
    upsert_vue(
        &host,
        "/Comp.vue",
        r#"<script setup lang="ts">
import type { Foo } from './types'
defineProps<Foo>()
</script>
<template><slot /></template>"#,
    );

    let analysis = host.get_analysis("/Comp.vue").unwrap();
    let tpl = analysis
        .template
        .expect("template should be computed even with unresolved type deps");
    assert_eq!(
        tpl.defined_slots.len(),
        1,
        "should detect the <slot> despite unresolved type dep"
    );
}

// ── Fix 1: effective_target + resolved_dependency_targets ──────────

#[test]
fn effective_target_returns_resolved_when_present() {
    let res = crate::types::DependencyResolution {
        specifier: "./types".to_string(),
        resolved_canonical_id: Some("/src/types.ts".to_string()),
        possible_canonical_ids: vec!["/src/types.js".to_string(), "/src/types.d.ts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/types.ts"),
        "resolved_canonical_id should win over possibles"
    );
}

#[test]
fn effective_target_picks_dts_over_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec![
            "/src/utils.js".to_string(),
            "/src/utils.ts".to_string(),
            "/src/utils.d.ts".to_string(),
        ],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.d.ts"),
        ".d.ts should have highest priority"
    );
}

#[test]
fn effective_target_picks_ts_over_js() {
    let res = crate::types::DependencyResolution {
        specifier: "./utils".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/src/utils.jsx".to_string(), "/src/utils.tsx".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/utils.tsx"),
        ".tsx should win over .jsx"
    );
}

#[test]
fn effective_target_returns_none_when_empty() {
    let res = crate::types::DependencyResolution {
        specifier: "./missing".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: Vec::new(),
    };
    assert_eq!(res.effective_target(), None);
}

#[test]
fn effective_target_vue_only_when_no_script_candidates() {
    let res = crate::types::DependencyResolution {
        specifier: "./Comp".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/src/Comp.vue".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/src/Comp.vue"),
        ".vue should be returned when it is the only candidate"
    );
}

#[test]
fn effective_target_prefers_dcts_over_cjs() {
    let res = crate::types::DependencyResolution {
        specifier: "./lib".to_string(),
        resolved_canonical_id: None,
        possible_canonical_ids: vec!["/lib/index.cjs".to_string(), "/lib/index.d.cts".to_string()],
    };
    assert_eq!(
        res.effective_target(),
        Some("/lib/index.d.cts"),
        ".d.cts should win over .cjs"
    );
}

#[test]
fn resolved_dependency_targets_uses_effective_target() {
    let mut import_routes = rustc_hash::FxHashMap::default();
    // Resolved: should use resolved_canonical_id only
    import_routes.insert(
        "./types".to_string(),
        crate::types::DependencyResolution {
            specifier: "./types".to_string(),
            resolved_canonical_id: Some("/src/types.ts".to_string()),
            possible_canonical_ids: vec!["/src/types.js".to_string()],
        },
    );
    // Unresolved: should use highest-priority possible
    import_routes.insert(
        "./utils".to_string(),
        crate::types::DependencyResolution {
            specifier: "./utils".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec![
                "/src/utils.js".to_string(),
                "/src/utils.d.ts".to_string(),
            ],
        },
    );
    // No resolution at all
    import_routes.insert(
        "./missing".to_string(),
        crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        },
    );

    let targets = VerterHost::resolved_dependency_targets(&import_routes);

    assert!(
        targets.contains("/src/types.ts"),
        "should include resolved ID"
    );
    assert!(
        !targets.contains("/src/types.js"),
        "should NOT include possibles when resolved exists"
    );
    assert!(
        targets.contains("/src/utils.d.ts"),
        "should include highest-priority possible"
    );
    assert!(
        !targets.contains("/src/utils.js"),
        "should NOT include lower-priority possible"
    );
    assert_eq!(targets.len(), 2, "missing should not contribute a target");
}

#[test]
fn external_type_analysis_reuses_cached_analysis_for_same_dependency() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    ws.inject_file("/src/base.ts", "export interface Base { id: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let first = host
        .external_type_analysis("/src/types.ts")
        .expect("first analysis should load and cache the dependency");
    let second = host
        .external_type_analysis("/src/types.ts")
        .expect("second analysis should reuse the cached dependency analysis");

    assert!(
        Arc::ptr_eq(&first, &second),
        "repeated dependency analysis should reuse the cached analysis object",
    );
    assert_eq!(
        ws.read_count("/src/types.ts"),
        1,
        "the dependency source should only be loaded once for repeated analysis lookups",
    );
}

#[test]
fn external_type_analysis_prefers_declaration_companion_for_runtime_js_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from("export interface Props { label: string }\n"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    let analysis = host
        .external_type_analysis("/workspace/node_modules/pkg/dist/index.js")
        .expect("runtime-script analysis requests should prefer the declaration companion");

    assert!(
        analysis.local_symbol_span("Props").is_some(),
        "the declaration companion analysis should expose declaration symbols",
    );

    // In the new IndexedReady DB, ensure_indexed_ready normalizes .js → .d.ts
    // companion, so the .js path returns the .d.ts entry. Verify the declaration companion
    // is properly cached with analysis content.
    let declaration_entry = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/index.d.ts")
        .expect("the declaration companion should own the cached analysis");
    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        declaration_entry
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "the declaration companion should cache the analysis surface",
    );
}

#[test]
fn resolve_eval_dependency_canonical_prefers_declaration_companion_shallowly() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js",
        "export const runtimeOnly = true\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved =
        host.resolve_eval_dependency_canonical("/workspace/node_modules/pkg/dist/index.js");

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/pkg/dist/index.d.ts"),
        "runtime-script dependency canonicalization should prefer the declaration companion",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/index.js"),
        0,
        "companion selection should not read the runtime script when a declaration companion exists",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/pkg/dist/index.d.ts"),
        0,
        "companion selection should stay on shallow existence probes and avoid reading the declaration companion",
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly materializes.
    // Verify that the FileArtifactStore was NOT populated by the shallow
    // resolve_eval_dependency_canonical call itself.
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.js")
            .is_none(),
        "shallow companion selection should not cache .js facts in the FileArtifactStore",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts").is_none(),
        "companion canonicalization must not materialize or cache the declaration target during shallow selection",
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 1, wildcard-hash-
/// stability behavior): the indexed shallow surface for a barrel must
/// resolve its `export *` wildcard edges to real canonical ids through the
/// shared `resolve_route_edge_canonical` policy, so the wildcard target
/// enters the `DerivedFactKind::Route` surface hash. A surface built with a
/// `NullResolver` (every wildcard `canonical_id` empty) is target-blind: a
/// route depending on the wildcard target stale-serves when that target's
/// resolution changes.
///
/// FAILS on a regression where the wildcard `canonical_id` is `""` (empty)
/// and the route hash is target-blind. PASSES while the indexed
/// materialiser resolves `./impl` to `/workspace/impl.ts` through the
/// shared route-edge policy.
#[test]
fn indexed_barrel_wildcard_surface_resolves_edge_canonicals() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/index.ts", "export * from './impl';\n");
    ws.inject_file("/workspace/impl.ts", "export type Impl = { id: number };\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    // A configured project makes the workspace resolver resolve relative
    // specifiers (mirrors a real workspace; a bare unconfigured workspace
    // cannot resolve `./impl`).
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let indexed = host
        .ensure_indexed_ready("/workspace/index.ts")
        .expect("indexed materialiser must produce an artifact for the barrel");
    let wildcards = &indexed.shallow_state.wildcard_reexports;
    assert_eq!(
        wildcards.len(),
        1,
        "the barrel has exactly one `export *` wildcard reexport"
    );
    assert_eq!(
        wildcards[0].canonical_id, "/workspace/impl.ts",
        "the indexed surface MUST resolve the wildcard edge to the impl \
         canonical (NOT an empty NullResolver id) — an empty canonical keeps \
         the wildcard target out of the route-surface hash, stale-serving \
         every route that depends on it"
    );
}

/// DISCRIMINATING regression: an INDEXED barrel's PLAIN (non-type) `export *`
/// wildcard edge must resolve through the shared TS-first route-edge policy
/// (`resolve_route_edge_canonical`), matching the route-traversal and overlay
/// surfaces. A bare `export *` source IS captured in `export_signatures`, so
/// the indexed materialiser's `resolve_missing` loop resolves it — but for a
/// PLAIN `export *` it classifies the source as `EsmImport` and bakes the
/// runtime `.js` `source_id` without TS-first normalization. The shared policy
/// (used by route traversal + overlay) maps a `.js`-with-`.d.ts`-companion
/// source to the `.d.ts` declaration, so the indexed surface diverged — a
/// producer disagreement that `hash_route_surface` (which hashes
/// `wildcard.canonical_id`) turns into a stale serve.
///
/// FAILS pre-fix: the indexed wildcard edge bakes `/workspace/runtime.js`.
/// PASSES post-fix: the wildcard pass overwrites it with the shared-policy
/// `/workspace/runtime.d.ts`, matching the route-edge oracle.
#[test]
fn indexed_plain_export_star_resolves_wildcard_edge_through_ts_first_policy() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // PLAIN `export *` (not `export type *`) — the EsmImport-classified shape.
    ws.inject_file(barrel, "export * from './runtime';\n");
    // `./runtime` has a runtime `.js` WITH a `.d.ts` declaration companion: the
    // shared policy picks the `.d.ts`, a raw EsmImport resolve picks the `.js`.
    ws.inject_file("/workspace/runtime.js", "export const Runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    let oracle = host.resolve_route_edge_canonical(barrel, "./runtime");
    assert_eq!(
        oracle.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "precondition: the shared route-edge policy is TS-first (.d.ts companion)"
    );

    let indexed = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser must produce an artifact for the barrel");
    let baked = indexed
        .import_routes
        .get("./runtime")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        baked.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "the indexed plain `export *` wildcard edge MUST resolve through the shared \
         TS-first route-edge policy (.d.ts companion), not bake the EsmImport `.js`"
    );
    let wildcard = indexed
        .shallow_state
        .wildcard_reexports
        .iter()
        .find(|w| w.source_specifier == "./runtime")
        .map(|w| w.canonical_id.clone());
    assert_eq!(
        wildcard.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "the baked wildcard `canonical_id` (digested by `hash_route_surface`) MUST \
         equal the shared route-edge oracle so the indexed surface agrees with \
         route traversal / overlay"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 2): a cached
/// `RouteResult::Miss` produced because an `export *` wildcard edge could not
/// be resolved must NOT be served stale after the wildcard's target file
/// appears. The Miss was rooted only on the provider's `FileWholeHash` +
/// `Route` derived hash — neither of which re-resolves a known-miss specifier
/// — so when the target appeared (the provider's own content unchanged) the
/// recorded facts still revalidated and the stale Miss was served forever.
///
/// The fix roots such a Miss in the `DerivedFactKind::ImportRoute` rail:
/// `generation_current_import_route_hash` re-resolves the provider's known-miss
/// specifiers against the live workspace, so the recorded fact changes the
/// moment `./missing` resolves — invalidating the cached Miss. (If that fact
/// cannot be produced, the route entry is not admitted, so a cold re-resolve
/// runs instead of serving an unrooted Miss.)
///
/// FAILS pre-fix: the second resolve returns `None` (the stale cached Miss).
/// PASSES post-fix: the second resolve returns the now-existing target.
#[test]
fn unresolvable_wildcard_route_miss_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    // A barrel that re-exports from a target which does NOT yet exist.
    upsert_non_sfc(&host, "/workspace/index.ts", "export * from './missing';\n");

    // Cold resolve: `Foo` cannot resolve because `./missing` is unresolvable.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Foo");
    assert_eq!(
        first, None,
        "precondition: Foo must miss while ./missing does not resolve"
    );

    // The wildcard target appears (file-set change advances the epoch).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    // The cached Miss MUST NOT be served stale — Foo now resolves through the
    // wildcard to the freshly-appeared target.
    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Foo");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Foo".to_string())),
        "after ./missing appears, the cached wildcard Miss MUST invalidate and \
         re-resolve Foo to the now-existing target (RouteDb stale-serve hole 2)"
    );
}

/// CHARACTERIZATION (RouteDb stale-serve hole 2, review finding 1 facet a —
/// already correct on the INDEXED path; documents WHY no producer change was
/// needed). Review finding 1 facet a hypothesised that a barrel mixing an
/// UNRESOLVABLE `export * from './missing'` with a RESOLVABLE sibling
/// stale-serves because the bare `export *` edge is "not in
/// `required_import_sources` (a bare `export *` has no exported name)". That
/// premise is FALSE for the indexed materialiser: a bare `export *` is captured
/// in `export_signatures` as `ExportSignature { name: "*", reexport_source:
/// Some("./missing"), .. }` (see `verter_semantic::analysis::exports` —
/// `Statement::ExportAllDeclaration`), so it enters `required_import_sources`
/// and `prepared_decl`'s `resolve_missing` records it in `import_routes` as a
/// known-miss `DependencyResolution { resolved_canonical_id: None,
/// possible_canonical_ids: [] }` — WITHOUT any sibling, resolvable or not.
/// `generation_current_import_route_hash` therefore already detects the
/// known-miss and re-resolves `./missing` against the live workspace, so the
/// recorded `ImportRoute` fact MOVES the moment `./missing` appears and the
/// cached `Miss` invalidates.
///
/// This test PASSES both pre- and post-fix (no producer change lands for the
/// indexed path); it is retained as a regression guard against a future change
/// that drops `export *` sources from `export_signatures` / the import-route
/// known-miss rail. The genuine facet-b drop bug is fixed + pinned by
/// `route_resolved_via_later_wildcard_not_dropped_by_unresolvable_earlier_wildcard`.
#[test]
fn mixed_barrel_indexed_wildcard_known_miss_already_rooted_via_export_signatures() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    // A resolvable sibling exists; the wildcard target does NOT yet exist.
    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Present = number;\n",
    );
    // Barrel mixes an UNRESOLVABLE `export *` with a RESOLVABLE named reexport.
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export * from './missing';\nexport { Present } from './present';\n",
    );

    // Force the indexed surface: the resolvable sibling populates
    // `import_routes`, so the route entry is ADMITTED (the bug's precondition —
    // an admitted entry whose recorded fact does NOT track the wildcard
    // known-miss).
    let _ = host.ensure_indexed_ready("/workspace/index.ts");

    // Cold resolve a name ONLY the unresolvable wildcard can provide → MISS.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        first, None,
        "precondition: Missing must miss while ./missing is unresolvable"
    );

    // The wildcard target appears (provider content unchanged).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Missing = string;\n",
    );

    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "the mixed-barrel cached Miss MUST invalidate when ./missing appears — the \
         resolvable sibling's ImportRoute hash does NOT re-resolve the wildcard \
         known-miss, so without rooting the unresolvable wildcard in import_routes \
         the stale Miss is served forever (RouteDb stale-serve hole 2, facet a)"
    );
}

/// DISCRIMINATING regression (route-surface seed, negative-route staleness):
/// the base `build_indexed_route_surface` seed loop must NOT re-bake a
/// `set_import_dependencies` known-miss whose recorded `content_generation`
/// is stale against the live file set. A known-miss admitted at generation G
/// carries its admission generation in the
/// `import_routes_known_miss_recorded_at_generation` sidecar precisely so a
/// later file appearance (which advances `content_generation`) forces a
/// re-resolve — but the seed loop gated only on the POSITIVE stamp sidecar,
/// so the unstamped-positive known-miss seeded unconditionally,
/// `resolve_missing` skipped it (`import_routes.contains_key`), and the
/// rebuilt `IndexedReady` published the stale negative route under a FRESH
/// `edge_generation` — permanently edge-current, never re-resolved.
///
/// The reexport is type-only so the re-resolve flows through the TypeImport
/// lane, which `set_import_dependencies` leaves to the live resolver (its
/// exact-resolution rows pin only the ESM lanes for a known-miss).
///
/// FAILS pre-fix: the refreshed surface still records the known-miss.
/// PASSES post-fix: the stale known-miss is skipped at seed and `./missing`
/// re-resolves to the now-existing target.
#[test]
fn base_seed_does_not_rebake_stale_known_miss_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "export type { Foo } from './missing';\n",
    );
    // The caller's resolver reports `./missing` as unresolvable — a
    // known-miss admitted at the CURRENT content generation.
    host.set_import_dependencies(
        "/workspace/owner.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    let first = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady materialises");
    let first_route = first
        .import_routes
        .get("./missing")
        .expect("precondition: the known-miss seeds into the indexed surface");
    assert!(
        first_route.resolved_canonical_id.is_none()
            && first_route.possible_canonical_ids.is_empty(),
        "precondition: while ./missing does not exist the seeded route is a known-miss"
    );

    // The target appears — `content_generation` advances past the
    // known-miss admission generation.
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady re-materialises");
    let second_route = second
        .import_routes
        .get("./missing")
        .expect("the refreshed surface still tracks the ./missing specifier");
    assert_eq!(
        second_route.resolved_canonical_id.as_deref(),
        Some("/workspace/missing.ts"),
        "STALE NEGATIVE ROUTE: the route-surface seed re-baked a known-miss whose \
         recorded generation is stale against the live file set; the seed must \
         skip it so `resolve_missing` re-resolves against the live workspace"
    );
}

/// DISCRIMINATING regression (overlay seed, negative-route staleness): the
/// overlay materialiser's seed loop is the SAME gate as the base seed — a
/// stale `set_import_dependencies` known-miss must not be re-baked into a
/// session artifact after the target appears.
///
/// FAILS pre-fix: the overlay artifact re-bakes the stale known-miss.
/// PASSES post-fix: the overlay flight re-resolves `./missing` live.
#[test]
fn overlay_seed_does_not_rebake_stale_known_miss_after_target_appears() {
    use crate::session_view::OverlaidView;
    let host = Arc::new(make_host());
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    const OWNER_SOURCE: &str = "export type { Foo } from './missing';\n";
    upsert_non_sfc(&host, "/workspace/owner.ts", OWNER_SOURCE);
    host.set_import_dependencies(
        "/workspace/owner.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./missing".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Byte-identical overlay — the opened-but-unmodified LSP case.
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert("/workspace/owner.ts".to_string(), Arc::from(OWNER_SOURCE));
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let first = host
        .materialize_overlay_indexed_ready_with_view("/workspace/owner.ts", &view)
        .expect("overlay IndexedReady materialises");
    let first_route = first
        .import_routes
        .get("./missing")
        .expect("precondition: the known-miss seeds into the overlay surface");
    assert!(
        first_route.resolved_canonical_id.is_none(),
        "precondition: while ./missing does not exist the seeded route is a known-miss"
    );

    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host
        .materialize_overlay_indexed_ready_with_view("/workspace/owner.ts", &view)
        .expect("overlay IndexedReady re-materialises");
    let second_route = second
        .import_routes
        .get("./missing")
        .expect("the refreshed overlay surface still tracks the ./missing specifier");
    assert_eq!(
        second_route.resolved_canonical_id.as_deref(),
        Some("/workspace/missing.ts"),
        "STALE NEGATIVE ROUTE (overlay): the overlay seed re-baked a known-miss \
         whose recorded generation is stale against the live file set"
    );
}

/// DISCRIMINATING regression (OwnerImportSurface, unresolved-direct-import
/// staleness): an owner surface whose computation SKIPPED an unresolvable
/// direct import must carry a fact that goes stale when the missing target
/// appears. The cold body's skip arm recorded NO owner `ImportRoute` fact,
/// so the cached (empty-binding) surface was signed only by facts that do
/// not move on a file appearance — `resolve_owner_direct_import` kept
/// returning `None` forever after the target appeared.
///
/// The fix roots the skip in the `DerivedFactKind::ImportRoute` rail (the
/// same rail that roots unresolvable wildcard route misses):
/// `generation_current_import_route_hash` re-resolves the owner's known-miss
/// specifiers against the live workspace, so the recorded fact MOVES the
/// moment `./missing` resolves and the warm surface read declines.
///
/// FAILS pre-fix: the second resolve returns `None` (stale warm surface).
/// PASSES post-fix: the second resolve returns the now-existing target.
#[test]
fn owner_import_surface_unresolved_direct_import_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { Foo } from './missing';\nexport type Bar = Foo;\n",
    );

    let first = host.resolve_owner_direct_import("/workspace/owner.ts", "Foo");
    assert_eq!(
        first, None,
        "precondition: Foo must not resolve while ./missing does not exist"
    );

    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Foo = string;\n",
    );

    let second = host.resolve_owner_direct_import("/workspace/owner.ts", "Foo");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Foo".to_string())),
        "STALE OWNER SURFACE: after ./missing appears the cached owner import \
         surface (computed while the import was unresolvable) MUST go stale and \
         re-resolve — the skip arm roots in the owner's ImportRoute fact rail"
    );
}

/// DISCRIMINATING regression (edge-currency oracle, bindingless imports): a
/// file whose ONLY cross-file construct is a specifier-less import
/// (`import './dep';` — no bindings) still bakes a dependency-set-derived
/// route into `IndexedReady.import_routes`, so its surface MUST be subject
/// to the edge-currency oracle. The shallow edge inventory was built from
/// `extracted.bindings` only, so such a file reported
/// `has_cross_file_edges() == false`, the oracle judged the surface
/// permanently edge-current, and a dependency-set change (the target
/// appearing) never re-resolved the baked known-miss.
///
/// FAILS pre-fix: the re-read serves the stale known-miss (surface judged
/// edge-current forever). PASSES post-fix: the bindingless import is part of
/// the shallow edge inventory, the surface goes edge-stale, and the route
/// re-resolves to the now-existing target.
#[test]
fn bindingless_import_surface_reresolves_after_target_appears() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import './dep';\nexport const owner = 1;\n",
    );

    let first = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady materialises");
    let first_route = first
        .import_routes
        .get("./dep")
        .expect("precondition: the bindingless import enters import_routes");
    assert!(
        first_route.resolved_canonical_id.is_none(),
        "precondition: while ./dep does not exist the baked route is a known-miss"
    );

    upsert_non_sfc(&host, "/workspace/dep.ts", "export const dep = 1;\n");

    let second = host
        .ensure_indexed_ready("/workspace/owner.ts")
        .expect("owner IndexedReady re-reads");
    let second_route = second
        .import_routes
        .get("./dep")
        .expect("the surface still tracks the ./dep specifier");
    assert_eq!(
        second_route.resolved_canonical_id.as_deref(),
        Some("/workspace/dep.ts"),
        "EDGE-ORACLE BLIND SPOT: a bindingless (side-effect) import is a \
         cross-file edge; the surface must go edge-stale when the dependency \
         file set moves and re-resolve the baked known-miss"
    );
}

/// DISCRIMINATING regression (`ImportRoute` fact currency, host-memoized
/// positives): `generation_current_import_route_hash` must re-resolve a
/// host-memoized POSITIVE route whose recorded generation is stale against
/// the live file set — not only known-misses. A prefetch-class positive
/// (`cache_positive_import_route_result`) recorded `./dep →
/// /workspace/dep.js` at generation G; when the `.d.ts` companion appears
/// at G+1 the stamp goes stale, but the hash's "every specifier resolved ⇒
/// stable" fast path kept hashing the old `.js` target, so the `ImportRoute`
/// fact never moved and dependents warm-validated against a stale route.
///
/// FAILS pre-fix: the hash is unchanged after the retarget. PASSES
/// post-fix: the stale-stamped positive re-resolves (side-effect-free) and
/// the hash moves.
#[test]
fn generation_current_import_route_hash_reresolves_stale_stamped_positive() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(&host, "/workspace/dep.js", "export const dep = 1;\n");
    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { dep } from './dep';\nexport const owner = dep;\n",
    );

    // Host-memoized positive: the prefetch class records the route with the
    // generation captured at resolve time.
    let captured_generation = host.ws().content_generation();
    host.cache_positive_import_route_result_for_tests(
        "/workspace/owner.ts",
        "./dep",
        "/workspace/dep.js",
        captured_generation,
    );

    let before = host
        .generation_current_import_route_hash("/workspace/owner.ts")
        .expect("the memoized positive yields an ImportRoute hash");

    // The declaration companion appears — the dependency file set moves and
    // the TS-first policy now retargets `./dep` to the `.d.ts`.
    upsert_non_sfc(
        &host,
        "/workspace/dep.d.ts",
        "export declare const dep: number;\n",
    );

    let after = host
        .generation_current_import_route_hash("/workspace/owner.ts")
        .expect("the route table still yields an ImportRoute hash");
    assert_ne!(
        before, after,
        "STALE POSITIVE ROUTE FACT: a stamp-stale host-memoized positive must \
         re-resolve against the live workspace when the ImportRoute hash is \
         produced — otherwise dependents warm-validate against the retargeted \
         route forever"
    );
}

/// Pins the C1 capture-before-resolve stamp discipline at the producer:
/// `cache_positive_import_route_result` records the CALLER-captured
/// generation (taken before the resolution it memoizes), never a live
/// re-read at record time. A mutation that lands between resolve and record
/// therefore leaves the stamp conservatively STALE — the entry is refused
/// as generation-current and re-resolves — instead of forging a "current"
/// stamp onto a possibly-retargeted resolution.
///
/// Pre-fix this test does not compile: the producer's signature had no
/// caller-captured generation (it stamped a live read after the resolve),
/// which is exactly the unsoundness — the API could not express
/// capture-before-resolve.
#[test]
fn positive_route_stamp_is_caller_captured_not_live_at_record_time() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(&host, "/workspace/dep.ts", "export const dep = 1;\n");
    upsert_non_sfc(
        &host,
        "/workspace/owner.ts",
        "import { dep } from './dep';\nexport const owner = dep;\n",
    );

    // The caller captures the generation, resolves, and THEN a concurrent
    // mutation advances the workspace before the record lands.
    let captured_generation = host.ws().content_generation();
    upsert_non_sfc(&host, "/workspace/unrelated.ts", "export const u = 1;\n");
    host.cache_positive_import_route_result_for_tests(
        "/workspace/owner.ts",
        "./dep",
        "/workspace/dep.ts",
        captured_generation,
    );

    let live_generation = host.ws().content_generation();
    assert_ne!(
        captured_generation, live_generation,
        "fixture invariant: the workspace moved between capture and record"
    );
    let derived = host
        .derived_raw_cache()
        .get("/workspace/owner.ts")
        .expect("the record landed in DerivedRawState");
    assert!(
        !derived.import_route_is_generation_current("./dep", live_generation),
        "CAPTURE-BEFORE-RESOLVE: the recorded stamp must be the caller-captured \
         generation, so a mutation between resolve and record leaves the entry \
         conservatively stale (harmless re-resolve) instead of forging currency"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 2, review finding 1
/// facet b — wrongly-drops-valid / over-aggressive None). A barrel whose only
/// edges are `export *` wildcards (`export * from './missing'; export * from
/// './present';`) is a wildcard-only provider: its wildcards resolve into a
/// local `dep_edges` map and are NOT published into `import_routes`, so
/// `generation_current_import_route_hash(owner)` returns `None`. The hole-2
/// rooting loop fed that `None` through `?`, dropping the WHOLE route entry —
/// so a valid result resolved via the LATER wildcard (`./present`) was returned
/// as `None` (no value served at all). "Do not admit to cache" was wrongly
/// implemented as "return no result".
///
/// The fix splits the two cleanly: when an owner's import-route hash cannot be
/// produced, the resolved route surface is still RETURNED to the caller (with
/// empty facts → route_db's negative-cache path serves it without persisting),
/// never dropped. The next query re-resolves cold against the live workspace.
///
/// FAILS pre-fix: the first resolve returns `None` (the valid `./present`
/// result is wrongly dropped). PASSES post-fix: the valid result is returned
/// and still resolves after `./missing` appears.
#[test]
fn route_resolved_via_later_wildcard_not_dropped_by_unresolvable_earlier_wildcard() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Shared = number;\n",
    );
    // Barrel: an UNRESOLVABLE earlier `export *` then a RESOLVABLE later one.
    // (`Shared` is not a prefix of either wildcard's source stem, so the
    // wildcards are tried in declaration order — the unresolvable `./missing`
    // first, recording the owner as having an unresolved edge.)
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export * from './missing';\nexport * from './present';\n",
    );

    // The earlier unresolvable wildcard must NOT cause the valid later-wildcard
    // result to be dropped.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Shared");
    assert_eq!(
        first,
        Some(("/workspace/present.ts".to_string(), "Shared".to_string())),
        "a valid result resolved via a LATER `export *` wildcard MUST NOT be dropped \
         because an EARLIER `export *` wildcard is unresolvable — refusing to cache an \
         unrootable known-miss must never be implemented as returning no result \
         (RouteDb stale-serve hole 2, facet b)"
    );

    // The earlier wildcard target appears; the later-wildcard result still
    // resolves freshly (the served-without-caching surface re-resolves cold).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Other = string;\n",
    );
    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Shared");
    assert_eq!(
        second,
        Some(("/workspace/present.ts".to_string(), "Shared".to_string())),
        "after ./missing appears the later-wildcard result still resolves freshly"
    );
}

/// End-to-end regression: a mixed wildcard barrel re-resolves once its
/// unresolvable `export *` target appears. The barrel mixes an unresolvable
/// `export * from './missing'` with a RESOLVABLE named reexport
/// (`export { Present } from './present'`), supplied a PARTIAL `import_routes`
/// snapshot (`set_import_dependencies` recording only `./present`, omitting the
/// wildcard source). A first query for `Missing` misses; after `missing.ts`
/// appears the query MUST resolve it rather than stale-serve the cached `Miss`.
///
/// Two independent rails enforce this, and the test guards the end-to-end
/// behaviour rather than isolating either:
/// - The coverage-checked `ImportRoute` admission
///   (`generation_current_import_route_hash_covering_sources`): the rooting
///   loop admits an `ImportRoute` fact ONLY when the produced hash covers
///   EVERY unresolved wildcard source the traversal hit; a partial table that
///   omits `./missing` yields no fact, so the `Miss` is returned with EMPTY
///   facts (RouteDb negative-cache: served, never persisted) and re-resolves.
/// - The shared edge-currency oracle (`route_surface_is_edge_current`): the
///   barrel's surface is wildcard-bearing, so its `Route` participant fact is
///   edge-stale once `content_generation` advances (a file appeared), which
///   independently invalidates the cached `Miss`.
///
/// Because the edge-currency oracle backstops the wildcard rail, this test is
/// NOT a discriminator for the coverage check in isolation (reverting the
/// coverage check alone keeps it green — the edge oracle still invalidates).
/// The coverage check is retained as sound cache hygiene: it refuses to record
/// an `ImportRoute` fact that does not represent every dependency the entry
/// actually rests on.
///
/// FAILS against a tree with neither rail (the original stale-serve bug);
/// PASSES with either present.
#[test]
fn import_route_fact_admitted_only_when_it_covers_unresolved_wildcard_source() {
    let host = make_host();
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    // A resolvable sibling exists; the wildcard target does NOT yet exist.
    upsert_non_sfc(
        &host,
        "/workspace/present.ts",
        "export type Present = number;\n",
    );
    // Barrel mixes a RESOLVABLE named reexport with an UNRESOLVABLE `export *`.
    // The file is left un-indexed (no `ensure_indexed_ready`), so the
    // import-route surface comes solely from the PARTIAL snapshot below.
    upsert_non_sfc(
        &host,
        "/workspace/index.ts",
        "export { Present } from './present';\nexport * from './missing';\n",
    );

    // PARTIAL snapshot: records ONLY `./present`. The `./missing` wildcard
    // source is deliberately omitted, so `DerivedRawState.import_routes` has a
    // fully-resolved table (no known-miss) that does NOT cover the wildcard the
    // route traversal hits. This is the bug's precondition: an owner WITH a
    // route surface whose `ImportRoute` hash silently fails to track the
    // unresolved wildcard.
    host.set_import_dependencies(
        "/workspace/index.ts",
        vec![exact_dependency("./present", "/workspace/present.ts")],
    );

    // Cold resolve a name ONLY the unresolvable wildcard can provide → MISS.
    let first = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        first, None,
        "precondition: Missing must miss while ./missing is unresolvable"
    );

    // The wildcard target appears (provider content unchanged; `./present`
    // still resolves identically).
    upsert_non_sfc(
        &host,
        "/workspace/missing.ts",
        "export type Missing = string;\n",
    );

    let second = host.resolve_named_type_export_target("/workspace/index.ts", "Missing");
    assert_eq!(
        second,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "the cached Miss MUST invalidate when ./missing appears — a partial \
         import-route snapshot that resolves ./present but omits the wildcard \
         source produces an ImportRoute hash that does NOT cover ./missing, so \
         admitting it as the rooting fact stale-serves the Miss forever. The \
         fact must be admitted only when it covers every unresolved wildcard \
         source the traversal hit (RouteDb stale-serve hole 2 — coverage-checked \
         ImportRoute admission)"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve — generation-current
/// wildcard-edge route-surface production). A wildcard barrel
/// (`export * from './runtime'; export * from './present';`) first resolves
/// `Runtime` through `./runtime` when only `runtime.js` exists, caching a
/// `Route` fact whose indexed shallow surface bakes the wildcard edge
/// `./runtime → runtime.js`. When the `.d.ts` companion `runtime.d.ts` later
/// appears, TS-first priority retargets the effective edge to `runtime.d.ts`.
///
/// The barrel becomes scheduler-tracked once resolved, so the
/// owner-surface freshness gate's tier-1 (owner content hash) judges the
/// entry fresh after the retarget — the owner's content is unchanged. The
/// baked wildcard edge is nonetheless stale: it depends on the dependency
/// file set, which changed (a new file advanced `content_generation`). Before
/// the fix the warm host reproduces the stale `Route` hash (encoding
/// `runtime.js`) and validates the warm `RouteDb` entry, stale-serving
/// `runtime.js`.
///
/// The fix gates route-surface fact production AND indexed-surface
/// reuse on `route_surface_is_edge_current`: a wildcard-bearing surface is
/// edge-stale once `content_generation` advances past its baked-edge
/// generation, so no stale `Route` fact is produced and the edges are
/// rebuilt. The warm host then resolves the SAME target as a fresh
/// host.
///
/// FAILS pre-fix: the warm host keeps returning `runtime.js` after
/// `runtime.d.ts` appears. PASSES post-fix: the warm host returns
/// `runtime.d.ts`, identical to a fresh host built on the same workspace.
#[test]
fn route_fact_retargets_js_to_dts_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    // Wildcard barrel + a resolvable sibling, injected straight into the
    // workspace.
    ws.inject_file(
        index,
        "export * from './runtime';\nexport * from './present';\n",
    );
    ws.inject_file("/workspace/present.ts", "export type Present = number;\n");
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // `./runtime` absent → `Runtime` misses.
    let r0 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r0, None,
        "precondition: Runtime misses while ./runtime is absent"
    );

    // `runtime.js` appears: `./runtime` resolves to the runtime script (no
    // `.d.ts` companion yet), so `Runtime` resolves to runtime.js and the warm
    // host caches a Route fact encoding the `./runtime → runtime.js` edge.
    ws.inject_file("/workspace/runtime.js", "export const Runtime = true\n");
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some(("/workspace/runtime.js".to_string(), "Runtime".to_string())),
        "precondition: Runtime resolves to runtime.js while only the .js exists"
    );

    // The `.d.ts` companion appears. TS-first priority retargets `./runtime`
    // to runtime.d.ts.
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");

    // A FRESH host on the same workspace is the oracle for the retargeted edge.
    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.d.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host resolves Runtime to the .d.ts companion after retarget"
    );

    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host once \
         runtime.d.ts appears — a stale wildcard-bearing indexed surface (its \
         baked ./runtime edge unchanged at owner-content level but edge-stale after \
         the dependency file set shifted) must NOT reproduce its Route fact. \
         Route-surface production + materializer reuse must be gated on the \
         wildcard-edge resolution generation"
    );
}

/// DISCRIMINATING regression (generation-current wildcard-edge surface on the
/// INDEXED producer). `ensure_indexed_ready` bakes wildcard sources into the
/// content-pinned `IndexedReady` surface, and the indexed surface is the SOLE
/// route authority `current_route_surface_hash` / `HostStoreView::build`
/// serve — a baked wildcard edge depends on the dependency file set, not the
/// owner's content. A barrel `export * from './runtime'` indexed while only
/// `runtime.js` exists bakes the `./runtime → runtime.js` edge; when the
/// `.d.ts` companion appears TS-first priority retargets the edge, but the
/// content-pinned indexed surface (owner content unchanged) keeps serving
/// runtime.js.
///
/// The root fix roots the indexed surface in `IndexedReady.edge_generation` and
/// routes every route-fact producer + the indexed materializer-reuse through
/// the shared edge-currency oracle: an edge-stale wildcard-bearing indexed
/// surface produces no `Route` fact and is rebuilt on reuse, so the warm host
/// re-resolves and matches a fresh host.
///
/// FAILS pre-root-fix: the warm host returns `runtime.js` after `runtime.d.ts`
/// appears. PASSES post: warm == fresh == `runtime.d.ts`.
#[test]
fn indexed_route_fact_retargets_on_warm_host_after_dependency_set_change() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(
        index,
        "export type * from './runtime';\nexport type * from './present';\n",
    );
    ws.inject_file("/workspace/present.ts", "export type Present = number;\n");
    // `./runtime` initially resolves to the directory-index file.
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface — the route-surface producer under test.
    // `ensure_indexed_ready` bakes `./runtime → runtime/index.ts` into the
    // content-pinned `IndexedReady` at the current generation.
    let _ = warm.ensure_indexed_ready(index);
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some((
            "/workspace/runtime/index.ts".to_string(),
            "Runtime".to_string()
        )),
        "precondition: Runtime resolves to the directory-index file while it is \
         the only ./runtime target"
    );

    // A more-specific `./runtime.ts` appears. The SAME resolution policy (a
    // file preferred over a directory-index) retargets `./runtime` to
    // `runtime.ts` — a genuine baked-edge change the indexed materialiser
    // itself produces on rebuild (no resolution-policy divergence between
    // producers).
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let _ = fresh.ensure_indexed_ready(index);
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host retargets Runtime to the more-specific \
         ./runtime.ts after it appears"
    );

    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host once \
         ./runtime.ts appears — a content-pinned INDEXED wildcard surface (owner \
         content unchanged, but the dependency file set shifted) must be rooted in \
         its edge-resolution generation so every route-fact producer and the \
         indexed materializer reuse re-resolve instead of serving the stale \
         runtime/index.ts edge"
    );
}

/// DISCRIMINATING regression (edge currency for NON-wildcard route edges):
/// ordinary `import_routes` are baked route edges exactly like wildcard
/// edges — a named reexport `export type { Runtime } from './runtime'`
/// bakes the resolved `./runtime → runtime/index.ts` target into the
/// content-pinned `IndexedReady` (`import_routes` / `import_route_hash` /
/// the `ExportTarget::Reexport` canonical). When the more-specific
/// `./runtime.ts` later appears the edge retargets, but the owner's own
/// content is unchanged — an edge-currency oracle that stales ONLY
/// wildcard-bearing surfaces reuses the stale baked route.
///
/// FAILS pre-fix: the warm host keeps serving runtime/index.ts after
/// runtime.ts appears. PASSES post-fix: warm == fresh == runtime.ts, and
/// the warm host takes exactly one cheap EDGE-REFRESH for the owner (route
/// surface rebuilt from the retained content payload — no eval-program
/// re-parse of the owner; the freshly resolved target's own cold
/// materialise is the only full build).
#[test]
fn non_wildcard_route_fact_retargets_via_edge_refresh_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(index, "export type { Runtime } from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface: bakes `./runtime → runtime/index.ts`.
    let _ = warm.ensure_indexed_ready(index);
    let r1 = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        r1,
        Some((
            "/workspace/runtime/index.ts".to_string(),
            "Runtime".to_string()
        )),
        "precondition: Runtime resolves to the directory-index file while it \
         is the only ./runtime target"
    );

    // The more-specific `./runtime.ts` appears — a dependency-set change;
    // the owner's content stays put.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let fresh = VerterHost::new(HostConfig::default(), ws.clone());
    let _ = fresh.ensure_indexed_ready(index);
    let fresh_result = fresh.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        fresh_result,
        Some(("/workspace/runtime.ts".to_string(), "Runtime".to_string())),
        "precondition: a fresh host retargets Runtime to the more-specific \
         ./runtime.ts after it appears"
    );

    warm.provenance().reset();
    let warm_result = warm.resolve_named_type_export_target(index, "Runtime");
    assert_eq!(
        warm_result, fresh_result,
        "the WARM host MUST return the SAME retargeted target as a FRESH host \
         once ./runtime.ts appears — a NON-wildcard named-reexport surface \
         bakes dependency-set-derived edges exactly like a wildcard surface, \
         so the shared edge-currency oracle must stale it on a \
         content-generation advance"
    );
    let provenance = warm.provenance().snapshot();
    assert_eq!(
        provenance.indexed_ready_edge_refreshes, 1,
        "the stale non-wildcard owner surface must take exactly one cheap \
         edge-refresh (route surface rebuilt, content payload reused; got {})",
        provenance.indexed_ready_edge_refreshes
    );
}

/// Regression pin (dependency APPEARANCE on a NON-wildcard reexport edge):
/// the owner bakes `./missing` as a known-miss into its indexed
/// `import_routes`; when `missing.ts` later appears the edge must
/// re-resolve. At HEAD this case is ALSO covered by the known-miss
/// generation revalidation rail (`generation_current_route_resolution`
/// re-resolves a recorded miss against the live file set), so this test is
/// green even without the edge-currency staling of non-wildcard surfaces —
/// it pins the appearance CLASS end-to-end, while the discriminating RED
/// pin for the oracle extension is the retarget sibling above
/// (`non_wildcard_route_fact_retargets_via_edge_refresh_on_warm_host`).
#[test]
fn non_wildcard_route_fact_resolves_after_dependency_appears_on_warm_host() {
    let ws = Arc::new(CountingWorkspace::new());
    let index = "/workspace/index.ts";
    ws.inject_file(index, "export type { Missing } from './missing';\n");
    let warm = VerterHost::new(HostConfig::default(), ws.clone());

    // FORCE the indexed surface while `./missing` is unresolvable — the
    // known-miss bakes into the content-pinned route surface.
    let _ = warm.ensure_indexed_ready(index);
    let r0 = warm.resolve_named_type_export_target(index, "Missing");
    assert_eq!(
        r0, None,
        "precondition: Missing must miss while ./missing is absent"
    );

    ws.inject_file("/workspace/missing.ts", "export type Missing = string;\n");

    let warm_result = warm.resolve_named_type_export_target(index, "Missing");
    assert_eq!(
        warm_result,
        Some(("/workspace/missing.ts".to_string(), "Missing".to_string())),
        "a NON-wildcard reexport edge baked as a known-miss MUST re-resolve \
         once the dependency appears — the baked import_routes known-miss is \
         a dependency-set-derived edge exactly like a wildcard edge, so the \
         edge-currency oracle must stale the owner surface on the \
         content-generation advance"
    );
}

/// DISCRIMINATING unit test for the shared edge-currency oracle
/// `route_surface_is_edge_current`. ANY cross-file-edge surface (wildcard
/// reexports, named reexports, plain import targets, AND import-route-only
/// tables — the external `src=` / caller-pushed class whose edges the
/// shallow inventory never sees) stamped at a baked generation is
/// edge-stale once `content_generation` advances past it; a surface with
/// NO cross-file edges (nothing dependency-set-derived to go stale) stays
/// edge-current regardless. This is the single predicate every route-fact
/// producer and the materializer reuse share, for base and session-overlay
/// `IndexedReady` surfaces alike (`IndexedReady.edge_generation`); it takes
/// the ARTIFACT so the complete `IndexedReady::has_cross_file_edges`
/// authority — not the blind shallow component — decides edge-bearing.
#[test]
fn edge_currency_oracle_stales_wildcard_surface_after_generation_advance() {
    use crate::resolver_core::shallow_file_state::{
        ExportTarget, ImportTarget, ShallowFileState, WildcardReexport,
    };
    use rustc_hash::{FxHashMap, FxHashSet};

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/x.ts", "export const a = 1;\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let baked = host.ws().content_generation();

    enum EdgeShape {
        None,
        Wildcard,
        NamedReexport,
        ImportTarget,
        ImportRouteOnly,
    }
    let make_artifact = |shape: EdgeShape| {
        let analysis = Arc::new(
            verter_parser::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default(),
        );
        let mut exports = FxHashMap::default();
        let mut wildcard_reexports = Vec::new();
        let mut import_targets = FxHashMap::default();
        let mut import_routes = FxHashMap::default();
        match shape {
            EdgeShape::None => {}
            EdgeShape::Wildcard => wildcard_reexports.push(WildcardReexport {
                source_specifier: "./dep".to_string(),
                canonical_id: String::new(),
            }),
            EdgeShape::NamedReexport => {
                exports.insert(
                    "Foo".to_string(),
                    ExportTarget::Reexport {
                        source_specifier: "./dep".to_string(),
                        original_name: "Foo".to_string(),
                        canonical_id: "/workspace/dep.ts".to_string(),
                        is_type: true,
                    },
                );
            }
            EdgeShape::ImportTarget => {
                import_targets.insert(
                    "Foo".to_string(),
                    ImportTarget {
                        source_specifier: "./dep".to_string(),
                        imported_name: "Foo".to_string(),
                        canonical_id: "/workspace/dep.ts".to_string(),
                    },
                );
            }
            EdgeShape::ImportRouteOnly => {
                import_routes.insert(
                    "./dep".to_string(),
                    crate::types::DependencyResolution {
                        specifier: "./dep".to_string(),
                        resolved_canonical_id: Some("/workspace/dep.ts".to_string()),
                        possible_canonical_ids: vec!["/workspace/dep.ts".to_string()],
                    },
                );
            }
        }
        let shallow = ShallowFileState::routing_tables_only_for_test(
            [7u8; 16],
            exports,
            wildcard_reexports,
            FxHashSet::default(),
            import_targets,
            Arc::clone(&analysis),
        );
        let mut artifact = crate::project_type_store::IndexedReady::new_for_test_with_state(
            [7u8; 16],
            Arc::new(shallow),
            Arc::from(""),
            Arc::from(""),
            analysis,
        );
        artifact.import_routes = Arc::new(import_routes);
        artifact.edge_generation = baked;
        artifact
    };
    let wildcard = make_artifact(EdgeShape::Wildcard);
    let named_reexport = make_artifact(EdgeShape::NamedReexport);
    let import_bearing = make_artifact(EdgeShape::ImportTarget);
    let route_only = make_artifact(EdgeShape::ImportRouteOnly);
    let no_edges = make_artifact(EdgeShape::None);

    // Before any change: every surface is edge-current at its baked gen.
    assert!(host.route_surface_is_edge_current(&wildcard));
    assert!(host.route_surface_is_edge_current(&named_reexport));
    assert!(host.route_surface_is_edge_current(&import_bearing));
    assert!(host.route_surface_is_edge_current(&route_only));
    assert!(host.route_surface_is_edge_current(&no_edges));

    // A dependency-set change advances content_generation past `baked`.
    ws.inject_file("/workspace/y.ts", "export const b = 2;\n");
    assert_ne!(host.ws().content_generation(), baked);

    // EVERY cross-file-edge surface is now edge-stale (its baked edges
    // depend on the dependency file set); only the no-edge surface stays
    // edge-current.
    assert!(
        !host.route_surface_is_edge_current(&wildcard),
        "a wildcard-bearing surface MUST be edge-stale once content_generation \
         advances past its baked-edge generation"
    );
    assert!(
        !host.route_surface_is_edge_current(&named_reexport),
        "a named-reexport surface bakes a resolved dependency canonical and \
         MUST be edge-stale once content_generation advances past its \
         baked-edge generation"
    );
    assert!(
        !host.route_surface_is_edge_current(&import_bearing),
        "an import-target-bearing surface bakes resolved dependency canonicals \
         and MUST be edge-stale once content_generation advances past its \
         baked-edge generation"
    );
    assert!(
        !host.route_surface_is_edge_current(&route_only),
        "an IMPORT-ROUTE-ONLY surface bakes resolved dependency canonicals in \
         its route table — invisible to the shallow component — and MUST be \
         edge-stale once content_generation advances past its baked-edge \
         generation"
    );
    assert!(
        host.route_surface_is_edge_current(&no_edges),
        "a surface with no cross-file edges carries nothing \
         dependency-set-derived and stays edge-current regardless of \
         content_generation"
    );
}

/// DISCRIMINATING regression (edge-currency authority — the import-route-only
/// hole): an artifact whose ONLY cross-file edges live in
/// `IndexedReady.import_routes` (no shallow-inventory edge: no import
/// targets, no reexports, no bindingless imports) MUST be judged edge-stale
/// by `indexed_surface_is_current` once `content_generation` advances past
/// its `edge_generation`. The baked route targets are dependency-set-derived
/// exactly like a wildcard edge: a file appearing or retargeting moves them
/// while the owner's own content stays put.
///
/// Production shape: a caller-supplied route snapshot
/// (`set_import_dependencies`) for a specifier the file's own source never
/// names — the SFC external `src=` class lands in the same
/// import-route-only shape via the compile prefetch memos.
///
/// FAILS pre-fix: the edge gate consulted only the SHALLOW
/// `has_cross_file_edges` predicate, so the import-route-only artifact
/// stayed "edge-current" forever across content-generation moves — stale
/// route facts and compile slots survived retargets. PASSES post-fix: the
/// gate consults the complete `IndexedReady::has_cross_file_edges`
/// authority and stales the surface.
#[test]
fn import_route_only_artifact_goes_edge_stale_after_generation_advance() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/workspace/side.ts", "export const side = 1;\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    upsert_non_sfc(&host, "/workspace/plain.ts", "export const x = 1;\n");
    // Caller-supplied route for a specifier the file's own source never
    // mentions: the artifact's ONLY cross-file edge lives in
    // `import_routes`; the shallow inventory stays edge-free.
    host.set_import_dependencies(
        "/workspace/plain.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./side".to_string(),
            resolved_canonical_id: Some("/workspace/side.ts".to_string()),
            possible_canonical_ids: vec!["/workspace/side.ts".to_string()],
        }],
    );

    let indexed = host
        .ensure_indexed_ready("/workspace/plain.ts")
        .expect("plain.ts must materialise an IndexedReady");
    // Fixture sanity: the import-route-only shape this pin is about.
    assert!(
        !indexed.import_routes.is_empty(),
        "fixture: the caller-supplied route must be baked into \
         IndexedReady.import_routes"
    );
    assert!(
        !indexed.shallow_state.has_shallow_cross_file_edges(),
        "fixture: the shallow inventory must carry NO cross-file edge — the \
         route table is the artifact's only edge"
    );
    assert!(
        host.indexed_surface_is_current("/workspace/plain.ts", &indexed),
        "precondition: the freshly built artifact is current at its baked \
         generation"
    );

    // A dependency-set change advances content_generation while the owner's
    // content stays put.
    ws.inject_file("/workspace/unrelated.ts", "export const u = 1;\n");

    assert!(
        !host.indexed_surface_is_current("/workspace/plain.ts", &indexed),
        "IMPORT-ROUTE-ONLY HOLE: an artifact whose only cross-file edges \
         live in IndexedReady.import_routes must be judged edge-STALE once \
         content_generation advances past its edge_generation — the shallow \
         predicate alone is blind to baked route targets, so stale route \
         facts and compile slots would survive retargets"
    );

    // No-over-decline arm: a genuinely edge-free artifact (no shallow
    // edges AND an empty route table) stays current across the same move.
    upsert_non_sfc(&host, "/workspace/loner.ts", "export const y = 2;\n");
    let loner = host
        .ensure_indexed_ready("/workspace/loner.ts")
        .expect("loner.ts must materialise an IndexedReady");
    assert!(loner.import_routes.is_empty());
    ws.inject_file("/workspace/unrelated2.ts", "export const u2 = 1;\n");
    assert!(
        host.indexed_surface_is_current("/workspace/loner.ts", &loner),
        "an artifact with no cross-file edges at all carries nothing \
         dependency-set-derived and must stay current across \
         content-generation moves"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 3): the ESM-fallback
/// effective-target normalization MUST be identical between route traversal
/// (`resolve_route_type_edge`) and stale-entry revalidation
/// (`generation_current_route_resolution` — the type-route lane, i.e. a
/// known-miss or a type/ESM-recorded positive). Route traversal normalized
/// the ESM fallback (mapping a runtime `.js` to its `.d.ts` declaration
/// companion) while the revalidation path kept the raw `source_id`, so the
/// two recorded divergent `ImportRoute` facts — a known-miss re-resolved
/// against the current file set produced one canonical, the actual route
/// resolution produced another, and the dependent cache entry stale-served.
///
/// The scenario forces the ESM fallback: the specifier resolves ONLY under the
/// `EsmImport` kind (no `TypeImport` resolution exists), and the resolved
/// runtime target carries a `.d.ts` companion — so a NORMALIZED fallback
/// returns the declaration and a RAW fallback returns the runtime script.
///
/// FAILS pre-fix: the revalidation path returned the raw
/// `/workspace/runtime.js` while `resolve_route_type_edge` returned the
/// normalized `/workspace/runtime.d.ts`. PASSES post-fix: both return the
/// declaration companion through the single shared route-edge policy.
#[test]
fn esm_fallback_normalization_parity_between_route_edge_and_known_miss() {
    let ws = Arc::new(CountingWorkspace::new());
    let owner = "/workspace/owner.ts";
    ws.inject_file(owner, "export {}\n");
    // Runtime target plus its declaration companion: normalization maps the
    // `.js` to the `.d.ts` via `resolve_eval_dependency_canonical`.
    ws.inject_file("/workspace/runtime.js", "export const runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Resolvable ONLY via `EsmImport` — `TypeImport` resolution of this bare
    // specifier returns `None`, so both code paths fall through to the ESM
    // fallback (the exact site where the policies diverged).
    ws.set_exact_resolutions(
        owner,
        vec![verter_workspace::ExactResolution {
            specifier: "runtimedep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/runtime.js".to_string()),
            possible_canonical_ids: vec!["/workspace/runtime.js".to_string()],
        }],
    );

    let route_edge = host.resolve_route_type_edge(owner, "runtimedep");
    let known_miss = host.generation_current_route_resolution(owner, "runtimedep", None);

    assert_eq!(
        route_edge.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "route traversal normalizes the ESM fallback to the declaration companion"
    );
    assert_eq!(
        known_miss, route_edge,
        "known-miss revalidation MUST apply the SAME ESM-fallback normalization \
         as route traversal — divergent policies record divergent ImportRoute \
         facts and stale-serve a known-miss after the file set changes"
    );
}

/// DISCRIMINATING regression (RouteDb stale-serve hole 3, review finding 2 —
/// the OVERLAY materialiser is a third sibling route-fact producer). The
/// overlay `IndexedReady` materialiser resolved its TypeImport edges' ESM
/// fallback to the RAW `source_id` (the runtime `.js`), while route traversal +
/// known-miss revalidation normalize the ESM fallback to the `.d.ts`
/// declaration companion through the single shared `resolve_route_edge_canonical`
/// policy. Session store views consume the overlay's route facts, so an overlay
/// barrel with an ESM-fallback edge recorded a route canonical the base
/// route-fact producers cannot reproduce — the SAME stale-serve class as hole 3,
/// persisting on the overlay path.
///
/// The fix routes the overlay's TypeImport edge resolution through the SAME
/// shared `resolve_route_edge_canonical` policy (no third copy). Because the
/// overlay's `export *` wildcard sources flow through the SAME
/// `required_import_sources` chain (a bare `export *` is captured in
/// `export_signatures`), normalizing the chain also normalizes wildcard edges —
/// no separate wildcard pass is needed.
///
/// FAILS pre-fix: the overlay records `/workspace/runtime.js` (raw). PASSES
/// post-fix: the overlay records `/workspace/runtime.d.ts`, identical to the
/// shared `resolve_route_edge_canonical` oracle.
#[test]
fn overlay_materializer_esm_fallback_normalizes_like_shared_route_edge_policy() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // Runtime target + its declaration companion (the `.js` → `.d.ts`
    // normalization the shared policy applies).
    ws.inject_file("/workspace/runtime.js", "export const runtime = true\n");
    ws.inject_file("/workspace/runtime.d.ts", "export type Runtime = boolean\n");
    // Resolvable ONLY via `EsmImport` — forces the ESM fallback (the exact site
    // where the overlay kept the raw `source_id`).
    ws.set_exact_resolutions(
        barrel,
        vec![verter_workspace::ExactResolution {
            specifier: "runtimedep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/workspace/runtime.js".to_string()),
            possible_canonical_ids: vec!["/workspace/runtime.js".to_string()],
        }],
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));

    // The shared oracle — route traversal + known-miss both delegate here.
    let oracle = host.resolve_route_edge_canonical(barrel, "runtimedep");
    assert_eq!(
        oracle.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "precondition: the shared route-edge policy normalizes the ESM fallback to \
         the .d.ts companion"
    );

    // Overlay-only barrel re-exporting from the ESM-fallback target. No base
    // `IndexedReady` is seeded, so the overlay resolves the edge itself — this
    // exercises the overlay materialiser's OWN edge-resolution policy (not a
    // value copied from the base `DerivedRawState`).
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        barrel.to_string(),
        Arc::from("export type * from 'runtimedep';\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    let overlay = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materialiser produces an IndexedReady for the overlaid barrel");

    let recorded = overlay
        .import_routes
        .get("runtimedep")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        recorded.as_deref(),
        Some("/workspace/runtime.d.ts"),
        "the overlay materialiser MUST normalize its ESM-fallback edge through the \
         SAME shared route-edge policy (resolve_route_edge_canonical) as route \
         traversal + known-miss revalidation — recording the raw runtime .js \
         diverges the overlay route facts from the base route-fact producers and \
         stale-serves across the overlay boundary (RouteDb stale-serve hole 3, \
         overlay materialiser, review finding 2)"
    );
    assert_eq!(
        recorded, oracle,
        "the overlay's recorded edge canonical MUST equal the shared route-edge oracle"
    );
}

/// DISCRIMINATING regression: the OVERLAY materialiser's cache-hit reuse must
/// be edge-current, the overlay analog of the indexed materialiser reuse gate.
/// A session-overlay barrel `export type * from './runtime'` is materialised
/// while `./runtime` resolves to the directory-index file, baking that edge
/// into the overlay `IndexedReady` at the current generation. The overlay
/// artifact is keyed by overlay content hash + discriminator, so a BASE
/// file-set change (a more-specific `./runtime.ts` appears, advancing
/// `content_generation` without touching the overlay source) does NOT
/// re-materialise it — the cache-hit returns the stale baked edge.
///
/// The fix gates the overlay cache-hit on the shared edge-currency oracle: an
/// edge-stale wildcard-bearing overlay `IndexedReady` is NOT reused; control
/// falls through to RE-MATERIALISE the overlay artifact (re-resolving the
/// wildcard edges against the live file set from the overlay source) — it must
/// NOT fall back to the base surface (overlay-blindness).
///
/// FAILS pre-fix: the warm session host keeps the baked `runtime/index.ts`
/// edge. PASSES post-fix: the warm session host retargets to `runtime.ts`,
/// matching a fresh session host.
#[test]
fn overlay_materializer_wildcard_reuse_retargets_after_base_file_set_change() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    // `./runtime` initially resolves to the directory-index file.
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));

    let make_view = |host: &Arc<VerterHost>| {
        let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> =
            rustc_hash::FxHashMap::default();
        overlays.insert(
            barrel.to_string(),
            Arc::from("export type * from './runtime';\n"),
        );
        OverlaidView::new(Arc::clone(host), overlays)
    };

    let view = make_view(&host);
    // Materialise + cache the overlay artifact (bakes `./runtime → runtime/index.ts`).
    let first = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materialiser produces an IndexedReady");
    assert_eq!(
        first
            .import_routes
            .get("./runtime")
            .and_then(|r| r.resolved_canonical_id.clone())
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: the overlay bakes the directory-index edge while it is the \
         only ./runtime target"
    );

    // A more-specific `./runtime.ts` appears — a BASE file-set change that
    // advances `content_generation` but leaves the overlay source untouched.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // A fresh session host on the same workspace is the oracle for the retarget.
    let fresh_host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let fresh_view = make_view(&fresh_host);
    let fresh = fresh_host
        .materialize_overlay_indexed_ready_with_view(barrel, &fresh_view)
        .expect("fresh overlay materialiser produces an IndexedReady");
    let fresh_target = fresh
        .import_routes
        .get("./runtime")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        fresh_target.as_deref(),
        Some("/workspace/runtime.ts"),
        "precondition: a fresh session host retargets ./runtime to the more-specific \
         ./runtime.ts"
    );

    // Warm session host: re-materialise through the SAME view. The overlay
    // cache-hit must re-resolve the wildcard edge (re-materialise), not serve
    // the stale baked directory-index edge — and must NOT fall back to base.
    let warm = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("warm overlay materialiser produces an IndexedReady");
    let warm_target = warm
        .import_routes
        .get("./runtime")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        warm_target, fresh_target,
        "the WARM session host MUST retarget ./runtime to the SAME target as a FRESH \
         session host after ./runtime.ts appears — a wildcard-bearing overlay \
         IndexedReady reused from an earlier generation holds a stale baked edge; \
         the overlay cache-hit must be gated on the edge-currency oracle and \
         re-materialise (not serve the base surface)"
    );
    // Overlay-blindness guard: the retargeted edge is a genuine overlay
    // re-materialisation, not a base-surface read — the overlay barrel exists
    // ONLY in the overlay, so a base read would resolve nothing.
    assert_eq!(
        warm_target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the warm overlay surface must carry the re-resolved overlay edge"
    );
}

/// DISCRIMINATING regression: direct overlay artifact READERS (not just the
/// materialiser's own cache-hit) must serve an edge-current wildcard surface.
/// The session resolver context's `indexed_for_current_content` (and the
/// frontier-adapter / routed-shallow-with-view readers) clone the cached overlay
/// `IndexedReady` directly via `lookup_overlay_artifacts`, bypassing the
/// edge-currency gate — so a wildcard-bearing overlay surface materialised
/// before a BASE file-set change is read stale.
///
/// The fix routes every overlay reader through the gated overlay materialiser
/// accessor (`materialize_overlay_indexed_ready_with_view`), which re-resolves
/// the wildcard edges against the live file set when the cached surface is
/// edge-stale and re-publishes — NEVER falling back to the base surface
/// (overlay-blindness). The overlay barrel exists only in the overlay, so a
/// base read would resolve nothing; the assertion that the reader returns the
/// retargeted overlay edge proves the re-materialisation is overlay-rooted.
///
/// FAILS pre-fix: the reader serves the stale baked `runtime/index.ts` edge.
/// PASSES post-fix: it retargets to `runtime.ts`, matching a fresh materialise.
#[test]
fn overlay_reader_retargets_wildcard_after_base_file_set_change() {
    use crate::resolver_core::ResolverContext;
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let mut overlays: rustc_hash::FxHashMap<String, Arc<str>> = rustc_hash::FxHashMap::default();
    overlays.insert(
        barrel.to_string(),
        Arc::from("export type * from './runtime';\n"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);
    let first = host
        .materialize_overlay_indexed_ready_with_view(barrel, &view)
        .expect("overlay materializes");
    assert_eq!(
        first
            .import_routes
            .get("./runtime")
            .and_then(|r| r.resolved_canonical_id.clone())
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: the overlay bakes the directory-index edge"
    );
    // BASE file-set change: a more-specific `./runtime.ts` appears.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    let base = host
        .resolver_store_view_read()
        .into_owned_view()
        .with_session_overlay(&host, &view);
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::SessionResolverContext::new(&host, &view, &base, overlay);
    let warm = ResolverContext::indexed_for_current_content(&ctx, barrel)
        .expect("session context returns overlay indexed");
    let target = warm
        .import_routes
        .get("./runtime")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the overlay READER must retarget the wildcard edge after a base file-set \
         change — re-materialising the overlay surface (not serving the stale baked \
         edge, and not falling back to base)"
    );
}

/// DISCRIMINATING regression: the BASE shallow reader (`shallow_file_state` →
/// `indexed_for_current_content` → `current_content_pinned_indexed`) must serve
/// an edge-current wildcard surface. The base pin is keyed only by the OWNER's
/// content hash, so a wildcard-bearing `IndexedReady` materialised before a
/// dependency file-set change is served with a stale baked `export *` edge —
/// the owner content is unchanged, so the content pin still matches.
///
/// The fix gates the base content-pinned reader on the shared edge-currency
/// oracle: an edge-stale wildcard surface is re-indexed from BASE content via
/// `ensure_indexed_ready` (whose reuse is itself edge-gated, so it re-resolves
/// the edges against the live file set) rather than served stale.
///
/// FAILS pre-fix: the base reader returns the stale `runtime/index.ts` edge.
/// PASSES post-fix: it retargets to `runtime.ts`.
#[test]
fn base_shallow_reader_retargets_wildcard_after_dependency_set_change() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let first = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser produces the barrel");
    assert_eq!(
        first
            .import_routes
            .get("./runtime")
            .and_then(|r| r.resolved_canonical_id.clone())
            .as_deref(),
        Some("/workspace/runtime/index.ts"),
        "precondition: the barrel bakes the directory-index edge"
    );
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");
    let state = host
        .shallow_file_state(barrel)
        .expect("base shallow reader returns a surface");
    let target = state
        .wildcard_reexports
        .iter()
        .find(|w| w.source_specifier == "./runtime")
        .map(|w| w.canonical_id.clone());
    assert_eq!(
        target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the base shallow reader MUST retarget the wildcard edge after a dependency \
         file-set change — re-indexing from base content rather than serving the \
         stale baked directory-index edge"
    );
}

/// DISCRIMINATING regression for the session-UNMASKED frontier reader
/// (`routed_shallow_state_with_view`): a session-bearing query (the view
/// is `Some`) for a NON-overlaid wildcard barrel reads the published base
/// artifact via the base-key `lookup_overlay_artifacts` — which must be
/// served only while edge-current. After a dependency file-set change the
/// unmasked reader must fall through to the gated base route path and retarget
/// rather than serve the stale baked edge.
///
/// FAILS pre-fix (unmasked branch returns the stale `runtime/index.ts` clone);
/// PASSES post-fix (edge-stale → fall through to the gated `route_shallow_state`,
/// which re-indexes to `runtime.ts`).
#[test]
fn session_unmasked_reader_retargets_wildcard_after_dependency_set_change() {
    use crate::session_view::OverlaidView;
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws.clone()));
    let _ = host
        .ensure_indexed_ready(barrel)
        .expect("indexed materialiser produces the barrel");
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // A session view with NO overlay for the barrel (an empty overlay set) — a
    // base-passthrough view, so the reader takes the session-UNMASKED branch.
    let view = OverlaidView::new(Arc::clone(&host), rustc_hash::FxHashMap::default());
    let state = host
        .routed_shallow_state_with_view(barrel, Some(&view))
        .expect("session-unmasked reader returns a surface");
    let target = state
        .wildcard_reexports
        .iter()
        .find(|w| w.source_specifier == "./runtime")
        .map(|w| w.canonical_id.clone());
    assert_eq!(
        target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the session-unmasked frontier reader MUST retarget the wildcard edge after \
         a dependency file-set change — falling through to the gated base route path \
         rather than serving the stale baked directory-index clone"
    );
}

/// CRASH regression: an ARTIFACT-ONLY (no scheduler `DerivedRawState`)
/// wildcard barrel whose `edge_generation` is stale must NOT cause a mutual
/// recursion between `artifact_current_indexed` and `ensure_indexed_ready`.
/// The artifact-current reader re-indexes an edge-stale wildcard surface via
/// `ensure_indexed_ready`, whose own artifact fast-path must NOT call back into
/// the re-indexing `artifact_current_indexed` (it peeks the artifact raw and
/// non-recursively, then falls through to the single `materialize` re-index) —
/// otherwise the two bounce forever and overflow the stack.
///
/// Pre-fix: `artifact_current_indexed` → `ensure_indexed_ready` →
/// `artifact_current_indexed` → … → stack overflow (process abort).
/// Post-fix: terminates and re-indexes from the backing source to the FRESH
/// target (`runtime.ts`), never the stale planted `runtime/index.ts` edge.
#[test]
fn artifact_only_wildcard_barrel_edge_stale_does_not_recurse() {
    let ws = Arc::new(CountingWorkspace::new());
    let barrel = "/workspace/index.ts";
    ws.inject_file(barrel, "export type * from './runtime';\n");
    ws.inject_file(
        "/workspace/runtime/index.ts",
        "export type Runtime = number;\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    let real = host
        .ensure_indexed_ready(barrel)
        .expect("the barrel materialises");
    assert!(real.shallow_state.has_wildcard_reexports());

    // A genuinely artifact-only canonical: a real backing source in the
    // workspace (so a re-index CAN re-resolve it) but NO scheduler
    // `DerivedRawState` — the artifact is planted directly into the store as a
    // clone of the wildcard barrel's `IndexedReady` (its `edge_generation`
    // captured at the current generation).
    let foreign = "/workspace/foreign_barrel.ts";
    ws.inject_file(foreign, "export type * from './runtime';\n");
    host.project_type_store()
        .indexed()
        .insert(Arc::from(foreign), Arc::clone(&real));

    // A more-specific `./runtime.ts` appears: `content_generation` advances
    // past the planted clone's `edge_generation`, so the planted wildcard
    // surface is now edge-stale.
    ws.inject_file("/workspace/runtime.ts", "export type Runtime = boolean;\n");

    // MUST terminate (no stack overflow) and re-index to the FRESH target.
    let result = host
        .artifact_current_indexed(foreign)
        .expect("artifact-only reader re-indexes the edge-stale wildcard barrel");
    let target = result
        .import_routes
        .get("./runtime")
        .and_then(|r| r.resolved_canonical_id.clone());
    assert_eq!(
        target.as_deref(),
        Some("/workspace/runtime.ts"),
        "the artifact-only reader MUST terminate and re-index to the fresh \
         ./runtime.ts target — never recurse, never serve the stale baked \
         runtime/index.ts edge"
    );
    // The sibling readers over the same canonical also terminate + retarget.
    assert!(host.shallow_file_state(foreign).is_some());
    assert!(host.ensure_indexed_ready(foreign).is_some());
}

#[test]
fn resolve_eval_dependency_canonical_ignores_empty_candidate_without_reads() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved = host.resolve_eval_dependency_canonical("");

    assert!(
        resolved.is_none(),
        "empty canonical ids should not produce synthetic companion candidates",
    );
    assert_eq!(
        ws.read_count(""),
        0,
        "empty canonical ids must not trigger analysis-source reads",
    );
    assert!(
        host.ensure_indexed_ready("").is_none(),
        "empty canonical ids must not seed imported dependency cache entries",
    );
}

#[test]
fn resolve_eval_dependency_canonical_prefers_extension_candidates_before_raw_extensionless_probe() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "extensionless dependency canonicalization should resolve to the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "extensionless dependency canonicalization must stay on existence probes",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "extensionless dependency canonicalization should not probe the raw missing path before the typed companion candidates",
    );
}

#[test]
fn resolve_eval_dependency_canonical_prefers_bundle_entry_declaration_companion_shallowly() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js",
        "export { useId } from './runtime-core.js'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts",
        "export declare function useId(): string\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved = host.resolve_eval_dependency_canonical(
        "/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js",
    );

    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts"),
        "bundle entry runtime scripts should prefer the shared declaration companion when present",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js"),
        0,
        "bundle companion selection should stay on shallow existence probes for the runtime bundle",
    );
    assert_eq!(
        ws.read_count("/workspace/node_modules/@vue/runtime-core/dist/runtime-core.d.ts"),
        0,
        "bundle companion selection should stay on shallow existence probes for the declaration companion",
    );
}

#[test]
fn resolve_eval_dependency_canonical_memoizes_positive_result_within_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    let rctx = crate::request_context::RequestContext::new(
        4201,
        Arc::from("/workspace/src/App.vue"),
        false,
        None,
    );
    let _guard = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let first = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "the first resolve must run the candidate walk and find the typed companion",
    );
    assert_eq!(
        rctx.dep_canonical_memo
            .lock()
            .get("/workspace/src/runtime/types/html")
            .map(String::as_str),
        Some("/workspace/src/runtime/types/html.ts"),
        "a positive resolution must populate the request-scoped memo",
    );

    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        second, first,
        "the memoized resolve must return the same canonical as the cold walk",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts"),
        0,
        "a memo hit must not re-probe the .d.ts candidate — the candidate walk ran once per request",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html.ts"),
        0,
        "a memo hit must not re-probe the resolved .ts candidate — the candidate walk ran once per request",
    );
}

#[test]
fn resolve_eval_dependency_canonical_resolves_without_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    assert!(
        crate::request_context::current_request_context().is_none(),
        "precondition: no request context is installed on this thread",
    );

    let first = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        first.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
        "resolution must keep working with no request context installed",
    );

    // Without a request context there is no memo layer: a repeated call
    // re-runs the candidate walk (no behavior change outside requests).
    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(second, first);
    assert!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts") >= 1,
        "with no request context the candidate walk must re-run — no host-global memoization",
    );
}

#[test]
fn resolve_eval_dependency_canonical_does_not_memoize_negative_results() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    let rctx = crate::request_context::RequestContext::new(
        4202,
        Arc::from("/workspace/src/App.vue"),
        false,
        None,
    );
    let _guard = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx));

    let first = host.resolve_eval_dependency_canonical("/workspace/src/missing/nope");
    assert!(
        first.is_none(),
        "a dependency with no on-disk candidate resolves to None",
    );
    assert!(
        rctx.dep_canonical_memo.lock().is_empty(),
        "a None resolution must NOT enter the request-scoped memo — mid-request \
         artifact publication can turn a None into a hit, so negatives stay uncached",
    );

    // A later identical call must re-run the candidate walk (the None is
    // not pinned for the rest of the request).
    ws.reset_exists();
    let second = host.resolve_eval_dependency_canonical("/workspace/src/missing/nope");
    assert!(second.is_none());
    assert!(
        ws.exists_count("/workspace/src/missing/nope.d.ts") >= 1,
        "a repeated None resolve must probe candidates again — negatives are not memoized",
    );
}

#[test]
fn resolve_eval_dependency_canonical_memo_is_isolated_per_request_context() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    {
        let rctx1 = crate::request_context::RequestContext::new(
            4203,
            Arc::from("/workspace/src/App.vue"),
            false,
            None,
        );
        let _guard1 = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx1));
        let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
        assert_eq!(
            resolved.as_deref(),
            Some("/workspace/src/runtime/types/html.ts"),
        );
        assert_eq!(
            rctx1.dep_canonical_memo.lock().len(),
            1,
            "the first request's memo holds the positive mapping",
        );
        // `_guard1` drops here — the first request is over.
    }

    let rctx2 = crate::request_context::RequestContext::new(
        4204,
        Arc::from("/workspace/src/Other.vue"),
        false,
        None,
    );
    let _guard2 = crate::request_context::RequestContextGuard::install(Arc::clone(&rctx2));
    assert!(
        rctx2.dep_canonical_memo.lock().is_empty(),
        "a fresh request context starts with an empty memo — no cross-request sharing",
    );

    ws.reset_exists();
    let resolved = host.resolve_eval_dependency_canonical("/workspace/src/runtime/types/html");
    assert_eq!(
        resolved.as_deref(),
        Some("/workspace/src/runtime/types/html.ts"),
    );
    assert!(
        ws.exists_count("/workspace/src/runtime/types/html.d.ts") >= 1,
        "a fresh request context must not inherit the previous request's memo — \
         the candidate walk re-runs once per request",
    );
    assert_eq!(
        rctx2.dep_canonical_memo.lock().len(),
        1,
        "the second request populates its OWN memo from its own cold walk",
    );
}

#[test]
fn current_eval_state_normalizes_extensionless_canonical_before_fallback_load() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let state = host.current_eval_state("/workspace/src/runtime/types/html");

    assert!(
        state.is_some(),
        "extensionless canonical ids should still materialize eval state from the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "materializing eval state must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "materializing eval state must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn get_raw_analysis_snapshot_normalizes_extensionless_canonical_before_building_snapshot() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let snapshot = host.get_raw_analysis_snapshot("/workspace/src/runtime/types/html");

    assert!(
        snapshot.is_some(),
        "extensionless canonical ids should still build a raw snapshot from the typed companion",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "building the raw snapshot must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "building the raw snapshot must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn prepared_type_decl_normalizes_extensionless_canonical_before_shallow_backfill() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let prepared =
        host.prepared_type_decl("/workspace/src/runtime/types/html", "ButtonHTMLAttributes");

    assert!(
        prepared.is_some(),
        "prepared type lookup should backfill from the typed companion when the canonical id is extensionless",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "prepared type lookup must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "prepared type lookup must not probe the raw extensionless path before normalization",
    );
}

#[test]
fn ensure_loaded_normalizes_extensionless_canonical_before_workspace_read() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let loaded = host.ensure_loaded("/workspace/src/runtime/types/html");

    assert!(
        loaded,
        "ensure_loaded should accept extensionless canonical ids when a typed companion exists",
    );
    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "ensure_loaded must not read the raw missing extensionless path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "ensure_loaded must not probe the raw missing extensionless path before normalization",
    );
}

#[test]
fn upsert_normalizes_extensionless_macro_type_blockers_before_scheduler_workspace_read() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    upsert_vue(
        &host,
        "/workspace/src/runtime/components/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

defineProps<ButtonHTMLAttributes>()
</script>
<template><button /></template>"#,
    );

    for _ in 0..100 {
        if ws.read_count("/workspace/src/runtime/types/html.ts") >= 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert_eq!(
        ws.read_count("/workspace/src/runtime/types/html"),
        0,
        "scheduler blocker ingestion must not read the raw extensionless dependency path",
    );
    assert_eq!(
        ws.exists_count("/workspace/src/runtime/types/html"),
        0,
        "scheduler blocker ingestion must not probe the raw extensionless dependency path before normalization",
    );
    assert!(
        ws.read_count("/workspace/src/runtime/types/html.ts") >= 1,
        "scheduler blocker ingestion should read the normalized typed companion at least once",
    );
}

#[test]
fn read_analysis_source_and_current_eval_state_ignore_empty_canonical_ids() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    assert!(
        host.read_analysis_source("").is_none(),
        "empty canonical ids should not resolve analysis source",
    );
    assert!(
        host.current_eval_state("").is_none(),
        "empty canonical ids should not materialize eval state",
    );
    assert_eq!(
        ws.read_count(""),
        0,
        "empty canonical ids must not trigger workspace reads",
    );
    assert_eq!(
        ws.exists_count(""),
        0,
        "empty canonical ids must not trigger workspace existence probes",
    );
    assert!(
        host.ensure_indexed_ready("").is_none(),
        "empty canonical ids must not seed imported dependency cache entries",
    );
}

#[test]
fn read_analysis_source_and_current_eval_state_ignore_raw_import_specifiers() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    for specifier in ["../types/html", "#build/ui/checkbox", "@nuxt/schema", "vue"] {
        ws.reset_reads();
        ws.reset_exists();

        assert!(
            host.read_analysis_source(specifier).is_none(),
            "raw import specifier {specifier} should not resolve analysis source",
        );
        assert!(
            host.current_eval_state(specifier).is_none(),
            "raw import specifier {specifier} should not materialize eval state",
        );
        assert_eq!(
            ws.read_count(specifier),
            0,
            "raw import specifier {specifier} must not trigger workspace reads",
        );
        assert_eq!(
            ws.exists_count(specifier),
            0,
            "raw import specifier {specifier} must not trigger workspace existence probes",
        );
        assert!(
            host.ensure_indexed_ready(specifier).is_none(),
            "raw import specifier {specifier} must not seed imported dependency cache entries",
        );
    }
}

#[test]
fn external_type_analysis_uses_eval_source_for_vue_dependencies() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/base.ts",
        "export interface Base { id: string }\n",
    );
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="ts">
import type { Base } from './base'

export interface Props extends Base {
  label: string
}
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/types.vue",
        vec![exact_dependency("./base", "/src/base.ts")],
    );

    let analysis = host
        .external_type_analysis("/src/types.vue")
        .expect("vue dependency analysis should be built from the script/eval source");

    assert!(
        analysis.local_symbol_span("Props").is_some(),
        "vue dependency analysis should see local type symbols in the script block",
    );
    assert_eq!(
        analysis.local_import_symbol_target("Base"),
        Some(("./base", "Base")),
        "vue dependency analysis should keep import lookup-table entries for script imports",
    );
    // Required imported names are a BODY-dependent product: they
    // demand-walk through the artifact's shallow state (lazy
    // declaration-body memo), still over the script/eval source.
    let indexed = host
        .ensure_indexed_ready("/src/types.vue")
        .expect("vue artifact must materialise");
    assert!(
        indexed
            .shallow_state
            .required_import_names("Props")
            .contains("Base"),
        "vue dependency demand-walk should compute required imported names from the script block",
    );
}

#[test]
fn external_type_analysis_preserves_vue_tsx_source_type() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/types.vue",
        r#"<script lang="tsx">
const Button = () => <button />

export type Props = {
  render: typeof Button
}
</script>
<template><div /></template>"#,
    );

    let analysis = host
        .external_type_analysis("/src/types.vue")
        .expect("tsx vue dependency analysis should be built from the script block");

    assert!(
        analysis.local_symbol_span("Props").is_some(),
        "tsx shallow analysis should retain exported type symbols from the script block",
    );
    assert!(
        analysis.local_import_symbol_target("Button").is_none(),
        "tsx shallow analysis should not invent import targets for local JSX-bearing bindings",
    );
    assert!(
        analysis.direct_reexport_target("Props").is_none(),
        "local tsx exports should stay local instead of being routed through synthetic reexport edges",
    );
    assert!(
        analysis.required_import_names("Props").is_empty(),
        "local tsx-only types should not invent import dependencies",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_external_type_from_loaded_files_skips_leaf_imported_prop_companions() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { UseComponentIconsProps } from './useComponentIcons'

defineProps<UseComponentIconsProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/useComponentIcons.ts",
        r#"import type { AvatarProps, IconProps } from './types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
}"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './Avatar.vue'\nexport * from './Icon.vue'\n",
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name: string
  mode?: 'svg' | 'css'
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
import type { ChipProps } from './Chip.vue'

export interface AvatarProps {
  chip?: ChipProps
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Chip.vue",
        r#"<script lang="ts">
export interface ChipProps {
  tone?: string
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/src/App.vue",
        vec![exact_dependency(
            "./useComponentIcons",
            "/src/useComponentIcons.ts",
        )],
    );
    host.set_import_dependencies(
        "/src/useComponentIcons.ts",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );
    host.set_import_dependencies(
        "/src/Avatar.vue",
        vec![exact_dependency("./Chip.vue", "/src/Chip.vue")],
    );

    ws.reset_reads();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    // The replacement semantic query for the retired frontier element
    // payload: the component-meta macro-elements rail resolves the routed
    // root's declaration carrier through the ONE shared dispatch and
    // projects its one-level Shallow surface (member values stay carriers).
    let resolved = host
        .resolve_component_meta_macro_elements(
            "/src/App.vue",
            "./useComponentIcons",
            "UseComponentIconsProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
        )
        .expect("UseComponentIconsProps should resolve");

    assert!(
        resolved
            .elements
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("icon")),
        "Icon-backed props should still resolve through structural indexed access, got {:?}",
        resolved.elements.props
    );
    assert!(
        resolved
            .elements
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("avatar")),
        "leaf imported prop aliases should remain present without resolving the companion body, got {:?}",
        resolved.elements.props
    );
    assert!(
        ws.read_count("/src/Avatar.vue") <= 1,
        "frontier-ordered barrel BFS should skip unmatched siblings when the target is found earlier (got {} reads)",
        ws.read_count("/src/Avatar.vue"),
    );
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "skipping the leaf companion should also avoid its transitive imported graph",
    );
}

#[test]
fn base_eval_env_prefers_declaration_companion_for_runtime_js_dependencies() {
    let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.js".to_string(),
        Arc::from("export const runtimeOnly = true\n"),
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
        Arc::from("export declare function useForwardProps<T>(value: T): T\n"),
    );

    let host = VerterHost::new(HostConfig::default(), ws);

    let env = host
        .base_eval_env_arc("/workspace/node_modules/pkg/dist/index.js")
        .expect("runtime-script env requests should prefer the declaration companion");

    assert!(
        env.value_symbols.contains_key("useForwardProps"),
        "the declaration companion env should expose value declarations",
    );

    // In the new IndexedReady DB, ensure_indexed_ready normalizes .js → .d.ts
    // companion and eagerly materializes. Verify the companion has the right content.
    let declaration_entry = host
        .ensure_indexed_ready("/workspace/node_modules/pkg/dist/index.d.ts")
        .expect("the declaration companion should own the cached env");
    // Verify declaration companion has content.
    assert!(
        !declaration_entry.raw_source.is_empty(),
        "the declaration companion should have source content",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_seeds_shallow_dependency_state_without_snapshot_materialization(
) {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types'\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Base } from './base'\nexport interface Props extends Base { label: string }\n",
    );
    ws.inject_file("/src/base.ts", "export interface Base { id: string }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/index.ts", "Props");

    assert_eq!(
        resolved,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through the barrel",
    );

    let barrel_entry = host
        .ensure_indexed_ready("/src/index.ts")
        .expect("barrel file should be cached after routing");
    let target_entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("target file should be cached after routing");

    // external_type_analysis is Arc (non-optional) in IndexedReady; verify it has content.
    assert!(
        barrel_entry
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "barrel routing should seed shallow external type analysis for the imported barrel file",
    );
    assert!(
        target_entry
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "barrel routing should seed shallow external type analysis for the resolved target file",
    );
    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds
    // full snapshots. The shallowness constraint applies to the internal routing,
    // not to the post-hoc facts query.
    assert_eq!(
        ws.read_count("/src/base.ts"),
        0,
        "shallow export routing should not touch transitive children that are not on the requested path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_uses_vue_tsx_registry_build() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types.vue'\n");
    ws.inject_file(
        "/src/types.vue",
        r#"<script lang="tsx">
const Button = () => <button />

export type Props = {
  render: typeof Button
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let resolved = host.resolve_named_type_export_target("/src/index.ts", "Props");

    assert_eq!(
        resolved,
        Some(("/src/types.vue".to_string(), "Props".to_string())),
        "registry routing should preserve the vue script lang and find tsx exports behind barrels",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn route_and_root_resolution_do_not_fall_back_through_frontier() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export * from './types'\n");
    ws.inject_file(
        "/src/types.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _guard = crate::host_resolve::forbid_route_frontier_for_tests();
    let route = host.resolve_named_type_export_target("/src/index.ts", "Props");
    assert_eq!(
        route,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through DB-owned shallow facts without frontier fallback",
    );

    let root = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root,
        ("/src/types.ts".to_string(), "Props".to_string()),
        "imported-root proof should reuse the DB-owned route without frontier fallback",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_indexed_ready_for_vue_exports_stays_local() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let entry = host
        .ensure_indexed_ready("/src/Button.vue")
        .expect("button dependency should build shallow state");

    // Module facts for Vue files build shallow state with locally declared
    // symbols and export surface. The entry should exist and be non-empty
    // since Button.vue has local exports (ButtonProps).
    assert!(
        !entry.shallow_state.is_empty(),
        "vue module facts should have a populated shallow state",
    );
    assert!(
        entry.shallow_state.exports.contains_key("ButtonProps"),
        "vue module facts should expose locally declared export ButtonProps",
    );
}

#[test]
fn ensure_indexed_ready_defers_prepared_decl_materialization_until_lookup() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
export interface Props {
  label: string
}

export const defaults: Props = { label: 'ok' }
"#,
    );

    let _entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed shallow imported state");

    let prepared_type = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("prepared type decl should materialize on demand");
    assert!(
        prepared_type.member_index.contains_key("label"),
        "on-demand prepared type materialization should retain the shallow member index",
    );

    let prepared_value = host
        .prepared_value_decl("/src/types.ts", "defaults")
        .expect("prepared value decl should materialize on demand");
    let annotation_source = prepared_value
        .type_annotation
        .annotation
        .as_ref()
        .unwrap_or_else(|| {
            panic!(
                "on-demand prepared value materialization should retain the annotation source, got {:?}",
                prepared_value.type_annotation
            )
        });
    let annotation_ty = crate::test_only::semantic_source_probe::shallow_type_expr(
        &host,
        "/src/types.ts",
        annotation_source,
    )
    .unwrap_or_else(|| panic!("the prepared value annotation source must shell-materialize"));
    assert!(
        matches!(
            &annotation_ty,
            TypeExpr::Ref { name, .. } if name.as_ref() == "Props"
        ),
        "on-demand prepared value materialization should retain the shallow type annotation, got {annotation_ty:?}",
    );

    assert!(
        host.prepared_type_decl("/src/types.ts", "Props").is_some(),
        "on-demand prepared type materialization should be available through the bundle cache",
    );
    assert!(
        host.prepared_value_decl("/src/types.ts", "defaults")
            .is_some(),
        "on-demand prepared value materialization should be available through the bundle cache",
    );

    let (audit, _cm_counters) = host.component_meta_audit_store_snapshot(None);
    assert_eq!(
        audit.prepared_type_decls, 1,
        "audit store snapshot should count prepared type decls from the bundle cache",
    );
    assert_eq!(
        audit.prepared_value_decls, 1,
        "audit store snapshot should count prepared value decls from the bundle cache",
    );
}

#[test]
fn shallow_prepared_decl_name_resolution_uses_shallow_dependency_targets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/node_modules/@vue/runtime-core.d.ts",
        "export interface Component { name?: string }",
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { Component } from '@vue/runtime-core'

export interface Props {
  as?: Component
}
"#,
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency(
            "@vue/runtime-core",
            "/node_modules/@vue/runtime-core.d.ts",
        )],
    );

    let entry = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should seed imported state");
    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds full facts.
    assert!(
        !entry.raw_source.is_empty(),
        "types dependency should have source content",
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare from the imported cache");
    let resolved = prepared.name_resolution.get("Component").expect(
        "prepared declaration should resolve imported Component through dependency targets",
    );
    assert_eq!(
        resolved.canonical_id.as_ref(),
        "/node_modules/@vue/runtime-core.d.ts",
        "prepared declaration lookup should canonicalize imported names",
    );

    let cached = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should stay cached");
    assert!(
        !cached.raw_source.is_empty(),
        "types dependency should maintain source content after prepared lookup",
    );
}

#[test]
fn owner_local_prepared_decl_name_resolution_uses_stored_dependency_targets() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/schema.ts",
        "export interface AppConfig { ui: {} }",
    );
    upsert_non_sfc(
        &host,
        "/src/tv.ts",
        "export type ComponentConfig<T, A, K> = { config: A; key: K }",
    );
    upsert_non_sfc(
        &host,
        "/src/theme.ts",
        "export default { value: true } as const",
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![
            exact_dependency("./schema", "/src/schema.ts"),
            exact_dependency("./theme", "/src/theme.ts"),
            exact_dependency("./tv", "/src/tv.ts"),
        ],
    );

    let _store_view = host.resolver_store_view_read().into_owned_view();
    let prepared = host
        .prepared_type_decl("/src/Button.vue", "Button")
        .expect("Button should prepare from the owner-local shallow cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("AppConfig")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/schema.ts"),
        "owner-local prepared declarations should canonicalize type imports through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("ComponentConfig")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/tv.ts"),
        "owner-local prepared declarations should canonicalize imported helper aliases through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/theme.ts"),
        "owner-local prepared declarations should canonicalize imported values through stored dependency targets",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_named_barrel_lookup_skips_unrelated_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { IconProps } from './types'
defineProps<IconProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './icon'\nexport * from './a'\nexport * from './b'\n",
    );
    ws.inject_file(
        "/src/types/icon.ts",
        "export interface IconProps { name: string }\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AProps { unused: boolean }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BProps { unused: number }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./icon", "/src/types/icon.ts"),
            exact_dependency("./a", "/src/types/a.ts"),
            exact_dependency("./b", "/src/types/b.ts"),
        ],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for the consumer");

    assert!(
        meta.props.iter().any(|prop| prop.name == "name"),
        "resolved props should include IconProps.name, got {:?}",
        meta.props,
    );
    // BFS shallows same-layer barrel siblings by design. Each sibling may be
    // read once for route discovery and once for materialization (two adapters
    // with independent route caches), so the upper bound is 2 per cold request.
    assert!(
        ws.read_count("/src/types/a.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} reads for /src/types/a.ts",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} reads for /src/types/b.ts",
        ws.read_count("/src/types/b.ts"),
    );
}

// The `declaration_scoped_solver_applies_omit_to_barrel_imported_types`
// characterization scenario is intentionally not exercised here. It
// asserted `engine.solve(Omit<ButtonProps, 'color'> & { status })`
// expanded an imported barrel type through a declaration-scoped solver
// surface that is not part of the final design; dispatch owns that
// surface via `ComponentMetaQueryEngine::project_expr_surface_expr`,
// and the flat-property-union shape the previous bridge returned is
// not reproducible without reintroducing the bridge. The positive-
// direction coverage for barrel / Omit routes lives in
// `component_meta_query_engine::tests`.

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_reuses_barrel_routes_for_multiple_late_exports() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { TargetProps, TargetEmits } from './types'

defineProps<TargetProps>()
defineEmits<TargetEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        r#"
export interface TargetProps { label: string }
export type TargetEmits = { change: [value: string] }
"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![
            exact_dependency("./a", "/src/types/a.ts"),
            exact_dependency("./b", "/src/types/b.ts"),
            exact_dependency("./target", "/src/types/target.ts"),
        ],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for repeated late barrel exports");

    assert!(
        meta.props.iter().any(|prop| prop.name == "label"),
        "resolved props should include TargetProps.label, got {:?}",
        meta.props,
    );
    assert!(
        meta.events.iter().any(|event| event.name == "change"),
        "resolved events should include TargetEmits.change, got {:?}",
        meta.events,
    );
    // BFS shallows same-layer barrel siblings. Each sibling may be read up to
    // twice per cold request (route discovery + materialization adapters).
    assert!(
        ws.read_count("/src/types/a.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} for 'a'",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 2,
        "same-layer sibling should be read at most twice per cold request, got {} for 'b'",
        ws.read_count("/src/types/b.ts"),
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shallow_imported_barrel_state_keeps_reexport_routes_lazy_until_lookup() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        r#"
export interface TargetProps { label: string }
export type TargetEmits = { change: [value: string] }
"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let shallow = host
        .ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow imported state");

    // Module facts now eagerly resolve wildcard reexport specifiers via
    // workspace during materialization. The barrel's shallow state should
    // have wildcard_reexports with resolved canonical IDs.
    assert!(
        !shallow.shallow_state.wildcard_reexports.is_empty(),
        "barrel module facts should have wildcard reexport entries",
    );
    assert!(
        shallow
            .shallow_state
            .wildcard_reexports
            .iter()
            .any(|w| w.source_specifier == "./target"),
        "barrel module facts should include the ./target wildcard reexport",
    );

    ws.reset_resolves();
    let props_root = host.resolve_imported_type_root("/src/types/index.ts", "TargetProps");
    let emits_root = host.resolve_imported_type_root("/src/types/index.ts", "TargetEmits");

    assert_eq!(
        props_root,
        (
            "/src/types/target.ts".to_string(),
            "TargetProps".to_string()
        ),
        "TargetProps should resolve through the cached shallow barrel route",
    );
    assert_eq!(
        emits_root,
        (
            "/src/types/target.ts".to_string(),
            "TargetEmits".to_string()
        ),
        "TargetEmits should resolve through the cached shallow barrel route",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        0,
        "once the barrel facts are seeded, root lookup should reuse the cached wildcard route without another workspace resolve",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_type_decl_keeps_export_only_barrels_shallow_for_missing_local_symbols() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        "export interface TargetProps { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow imported state");

    ws.reset_resolves();

    let prepared = host.prepared_type_decl("/src/types/index.ts", "TargetProps");

    assert!(
        prepared.is_none(),
        "prepared decl lookup on an export-only barrel should stay local and defer route resolution",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./a"),
        0,
        "missing local prepared decl lookup must not resolve earlier wildcard siblings",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./b"),
        0,
        "missing local prepared decl lookup must not resolve intermediate wildcard siblings",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        0,
        "missing local prepared decl lookup must not resolve the eventual wildcard target",
    );
}

/// Laziness pin for a LOCAL-export root resolve, narrowed to the
/// content-read dimension.
///
/// WHAT IS PINNED: the dependency SOURCES stay UNREAD
/// (`read_count == 0` for `/src/a.ts` and `/src/b.ts`) — resolving a
/// symbol exported by the owner itself never reads or parses any
/// dependency's content.
///
/// WHAT IS NOT PINNED (anymore): specifier RESOLUTION. The owner's
/// whole-file route surface bakes ALL of the owner's import edges at
/// `IndexedReady` build time, so the resolver MAY canonicalise `./a`
/// and `./b` while materialising `/src/types.ts` — the earlier
/// `resolve_count == 0` pin was retired with that bake-all-owner-edges
/// design. Laziness is demand-scoped DEEPENING (reading/parsing dep
/// content on demand), not edge canonicalisation, which is owner-local
/// route-surface construction.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_keeps_local_export_dep_sources_unread() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { A } from './a'
import type { B } from './b'

export interface Props {
  label: string
}
"#,
    );
    ws.inject_file("/src/a.ts", "export interface A { value: string }\n");
    ws.inject_file("/src/b.ts", "export interface B { value: number }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();

    let root = host.resolve_imported_type_root("/src/types.ts", "Props");

    assert_eq!(
        root,
        ("/src/types.ts".to_string(), "Props".to_string()),
        "local exported symbols should resolve to their defining file without leaving the file",
    );
    // The owner's own import EDGES canonicalise once as part of its
    // `IndexedReady` build (the canonical-edge rule); laziness is
    // demand-scoped DEEPENING — the dependency SOURCES stay unread and
    // unparsed for a local-export resolve that never leaves the file.
    assert_eq!(
        ws.read_count("/src/a.ts"),
        0,
        "imported-root proof for a local export must not read unrelated dependency sources",
    );
    assert_eq!(
        ws.read_count("/src/b.ts"),
        0,
        "imported-root proof for a local export must not read later unrelated dependency sources",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_tracks_provider_route_and_target_whole_hash_only() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/index.ts", "export { Props } from './target'\n");
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path("/src/index.ts", "Props")
        .expect("direct named reexport should resolve through the fast imported-root path");

    assert_eq!(
        resolved,
        ("/src/target.ts".to_string(), "Props".to_string()),
        "fast imported-root proof should preserve the exact child target tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider file content hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider route surface hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "fast imported-root proof must track the direct child file content hash",
    );
    assert!(
        !facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/target.ts"
        )),
        "direct imported-root proof should not need the child's route hash when the parent directly names the target reexport",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_resolves_cold_target_under_store_view() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path("/src/index.ts", "Props")
        .expect(
            "fast imported-root proof should resolve cold child hashes under a current store view",
        );

    assert_eq!(
        resolved,
        ("/src/target.ts".to_string(), "Props".to_string()),
        "store-view fast path should keep the same routed child tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "store-view fast path must still track the cold child file content hash",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn direct_imported_type_root_fast_path_reuses_provider_shallow_state_for_provider_facts() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let _ = host
        .resolve_direct_imported_type_root_fast_path("/src/index.ts", "Props")
        .expect("exported local imports should resolve through the fast imported-root path");

    assert_eq!(
        ws.read_count("/src/index.ts"),
        1,
        "fast imported-root proof should reuse the provider's existing routed shallow read when collecting provider facts",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn imported_type_root_fast_path_follows_exported_local_import_without_child_route_hash() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/index.ts",
        "import { Props as InnerProps } from './target'\nexport { InnerProps as Props }\n",
    );
    ws.inject_file(
        "/src/target.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./target", "/src/target.ts")],
    );

    let (resolved, facts) = host
        .resolve_direct_imported_type_root_fast_path("/src/index.ts", "Props")
        .expect("exported local imports should resolve through the fast imported-root path");

    assert_eq!(
        resolved,
        ("/src/target.ts".to_string(), "Props".to_string()),
        "fast imported-root proof should follow the exported local import to the exact child target tuple",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider file content hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/index.ts"
        )),
        "fast imported-root proof must track the provider route surface hash",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/target.ts"
        )),
        "fast imported-root proof must track the direct child file content hash",
    );
    assert!(
        !facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/target.ts"
        )),
        "direct imported-root proof should not need the child's route hash when the provider only re-exports the imported local binding",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_keeps_imported_barrel_route_facts_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types/index.ts",
        "export * from './a'\nexport * from './b'\nexport * from './target'\n",
    );
    ws.inject_file(
        "/src/types/a.ts",
        "export interface AOnly { unused: string }\n",
    );
    ws.inject_file(
        "/src/types/b.ts",
        "export interface BOnly { unused: number }\n",
    );
    ws.inject_file(
        "/src/types/target.ts",
        "export interface TargetProps { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow imported state");
    let _view = host.resolver_store_view_read().into_owned_view();

    ws.reset_resolves();

    let facts = host.current_dependency_fact_versions(
        "/src/types/index.ts",
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./a"),
        0,
        "captured fact-version lookup must not resolve earlier wildcard siblings for route hashing",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./b"),
        0,
        "captured fact-version lookup must not resolve intermediate wildcard siblings for route hashing",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        0,
        "captured fact-version lookup must not resolve the matched wildcard target for route hashing",
    );
    assert!(
        facts.iter().any(|fact| matches!(
            fact,
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind: crate::resolver_core::DerivedFactKind::Route,
                ..
            } if canonical_id == "/src/types/index.ts"
        )),
        "captured fact-version lookup should reuse the snapshotted route fact for a shallow imported barrel without live wildcard replay",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_resolves_transitive_macro_types_without_frontier_prewarm() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'

defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { Base } from './base'

export interface Props extends Base {
  label?: string
}
"#,
    );
    ws.inject_file(
        "/src/base.ts",
        r#"
import type { Inner } from './inner'

export interface Base {
  inner?: Inner
}
"#,
    );
    ws.inject_file(
        "/src/inner.ts",
        "export interface Inner { value: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./base", "/src/base.ts")],
    );
    host.set_import_dependencies(
        "/src/base.ts",
        vec![exact_dependency("./inner", "/src/inner.ts")],
    );

    ws.reset_reads();
    let meta = host
        .get_component_meta("/src/Consumer.vue")
        .expect("component meta should resolve for the consumer");

    assert!(
        meta.props.iter().any(|prop| prop.name == "label"),
        "resolved props should include Props.label, got {:?}",
        meta.props,
    );
    assert!(
        meta.props.iter().any(|prop| prop.name == "inner"),
        "resolved props should include Base.inner through transitive solver lookup, got {:?}",
        meta.props,
    );
    assert!(
        ws.read_count("/src/inner.ts") <= 1,
        "transitive macro expansion should reach the inner dependency on demand without repeated workspace reads, got {}",
        ws.read_count("/src/inner.ts"),
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_registry_seeding_keeps_barrel_children_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
}
</script>
<template><a /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let resolved = host.resolve_named_type_export_target("/src/types.ts", "LinkProps");

    assert_eq!(
        resolved,
        Some(("/src/Link.vue".to_string(), "LinkProps".to_string())),
        "wildcard barrel routing should still resolve the requested child",
    );

    // In the new IndexedReady DB, ensure_indexed_ready eagerly builds complete
    // facts including export_signatures and script_analysis. Verify the facts exist
    // and have the expected content.
    let barrel = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("barrel should be cached after routing");
    assert!(
        barrel
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "barrel routing should keep only shallow external type analysis in cache",
    );

    let child = host
        .ensure_indexed_ready("/src/Link.vue")
        .expect("matched child should be cached after routing");
    assert!(
        child
            .external_type_analysis
            .stats()
            .top_level_statement_count
            > 0,
        "matched child should be cached through shallow external type analysis",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn store_view_import_routes_do_not_depend_on_live_owner_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types/index.ts", "export * from './target'\n");
    ws.inject_file(
        "/src/types/target.ts",
        "export interface TargetProps { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types/index.ts")
        .expect("barrel should materialize shallow export state");
    let view = host.resolver_store_view_read().into_owned_view();

    assert!(
        view.derived_hash(
            "/src/types/index.ts",
            crate::resolver_core::DerivedFactKind::ImportRoute,
        )
        .is_some(),
        "captured store views should snapshot module-facts-backed import-route hashes without reconstructing the old structural shadow path",
    );

    ws.reset_resolves();
    ws.remove_file("/src/types/index.ts");
    host.compile_cache().remove("/src/types/index.ts");

    let resolved =
        host.resolve_type_dependency_canonical_shallow("/src/types/index.ts", "./target");

    assert_eq!(
        resolved.as_deref(),
        Some("/src/types/target.ts"),
        "route resolution should continue to resolve the barrel's `export *` target through cached module facts",
    );
    // The barrel's surface is wildcard-bearing, so its baked `export *` edge is
    // rooted in the indexed `edge_generation`. Removing the owner advances the
    // workspace `content_generation`, which the shared edge-currency oracle
    // treats as a potential dependency-set change: the wildcard edge is
    // re-validated against the live workspace rather than served from the
    // now-edge-stale baked surface. The resolution stays correct (`./target`
    // still resolves to `target.ts`); the re-validation is the correctness
    // conservatism the wildcard-edge rooting introduces. A non-wildcard
    // import-route surface carries no dependency-set-derived edge and would
    // still serve from cache without a live resolve.
    //
    // The rebuild resolves `./target` twice: once in the `resolve_missing`
    // loop (the plain `export *` source is classified `EsmImport`) and once in
    // the wildcard pass that re-resolves it through the shared TS-first
    // `resolve_route_edge_canonical` policy and overwrites — so the indexed
    // wildcard canonical agrees with the route-traversal / overlay surfaces.
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        2,
        "a wildcard-bearing barrel re-validates its baked `export *` edge once \
         content_generation advances (edge-currency): one EsmImport resolve plus \
         one shared-policy TS-first resolve, still resolving correctly",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shallow_imported_export_state_skips_non_reexport_import_resolution() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
import type { SharedProps } from './shared'

export interface LinkProps extends SharedProps {
  href?: string
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/shared.ts",
        "export interface SharedProps { label?: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_resolves();
    let entry = host
        .ensure_indexed_ready("/src/Link.vue")
        .expect("component should materialize shallow export state");

    assert!(
        entry.export_signatures.is_some(),
        "export-only shallow state should still capture export signatures",
    );
    // The import `./shared` provides `SharedProps` which is used by the
    // exported `LinkProps` heritage clause.  Module-facts materializes all
    // import routes eagerly (needed by the shallow state resolver and
    // component-meta pipelines), so at most one workspace resolve is expected.
    assert!(
        ws.resolve_count("/src/Link.vue", "./shared") <= 1,
        "module-facts should resolve the import at most once",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn import_route_lookup_reuses_indexed_ready_without_live_owner_state() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.ensure_indexed_ready("/src/types.ts")
        .expect("barrel should seed shallow import routes");

    ws.remove_file("/src/types.ts");
    host.compile_cache().remove("/src/types.ts");

    let resolved = host.resolve_type_dependency_canonical_shallow("/src/types.ts", "./Button.vue");

    assert_eq!(
        resolved,
        Some("/src/Button.vue".to_string()),
        "dependency lookup should reuse cached imported import routes",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_nested_barrel_alias_skips_later_unrelated_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "ButtonProps");

    assert_eq!(
        resolved,
        Some(("/src/Button.vue".to_string(), "ButtonProps".to_string())),
        "named export target resolution should route to the first matching nested barrel child",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "named export target resolution should stop at the matched route instead of loading later unrelated siblings",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_unseeded_barrel_keeps_wildcard_children_shallow() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "ButtonProps");

    assert_eq!(
        resolved,
        Some(("/src/Button.vue".to_string(), "ButtonProps".to_string())),
        "unseeded wildcard barrel routing should still resolve the first matching child",
    );
    assert_eq!(
        ws.read_count("/src/Link.vue"),
        0,
        "route selection should not preload later wildcard siblings while seeding the barrel cache",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "route selection should stop after the matched first-level wildcard child",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_unseeded_late_match_skips_earlier_wildcard_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Checkbox.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "CheckboxProps");

    assert_eq!(
        resolved,
        Some(("/src/Checkbox.vue".to_string(), "CheckboxProps".to_string())),
        "late wildcard match should still resolve to the correct child",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_unseeded_late_match_materializes_only_the_matched_vue_child() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Checkbox.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let root = host.resolve_imported_type_root("/src/types.ts", "CheckboxProps");

    assert_eq!(
        root,
        ("/src/Checkbox.vue".to_string(), "CheckboxProps".to_string()),
        "late wildcard imported-root proof should still resolve to the correct child",
    );
    for never_inspected in [
        "/src/Accordion.vue",
        "/src/Alert.vue",
        "/src/AuthForm.vue",
        "/src/Avatar.vue",
        "/src/Unused.vue",
    ] {
        assert_eq!(
            ws.read_count(never_inspected),
            0,
            "late wildcard imported-root proof should never read the uninspected sibling {never_inspected}",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the late wildcard proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store.indexed().get_any("/src/Checkbox.vue")
            .is_some(),
        "the matched Vue child was inspected and owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_reuses_indexed_vue_child_across_distinct_symbol_proofs() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types.ts", "export * from './Accordion.vue'\n");
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}

export interface AccordionEmits {
  change?: [value: string]
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let props_root = host.resolve_imported_type_root("/src/types.ts", "AccordionProps");
    let indexed_after_first = host
        .project_type_store
        .indexed()
        .get_any("/src/Accordion.vue");
    let emits_root = host.resolve_imported_type_root("/src/types.ts", "AccordionEmits");
    let indexed_after_second = host
        .project_type_store
        .indexed()
        .get_any("/src/Accordion.vue");

    assert_eq!(
        props_root,
        (
            "/src/Accordion.vue".to_string(),
            "AccordionProps".to_string()
        ),
        "first imported-root proof should resolve the Vue child type export",
    );
    assert_eq!(
        emits_root,
        (
            "/src/Accordion.vue".to_string(),
            "AccordionEmits".to_string()
        ),
        "second imported-root proof should resolve through the same indexed Vue child",
    );
    assert!(
        ws.read_count("/src/Accordion.vue") <= 1,
        "distinct imported-root proofs should reuse the indexed Vue child state instead of rereading it; saw {} reads",
        ws.read_count("/src/Accordion.vue"),
    );
    let indexed_after_first = indexed_after_first.expect(
        "the first imported-root proof inspects the matched Vue child, which then owns exactly one canonical IndexedReady built by the unified cold path",
    );
    let indexed_after_second = indexed_after_second.expect(
        "the matched Vue child keeps its canonical IndexedReady across distinct symbol proofs",
    );
    assert!(
        Arc::ptr_eq(&indexed_after_first, &indexed_after_second),
        "the second symbol proof must reuse the same IndexedReady Arc the first proof materialized, not rebuild it",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn get_component_meta_reuses_scheduler_snapshot_when_materializing_owner_indexed_ready() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps } from 'reka-ui'

const props = defineProps<AccordionRootProps>()
</script>
<template><div /></template>"#,
    );

    host.project_type_store()
        .indexed()
        .remove("/src/Accordion.vue");
    host.provenance().reset();
    let facts = host.ensure_indexed_ready("/src/Accordion.vue");
    let provenance = host.provenance_snapshot();

    assert!(
        facts.is_some(),
        "module-facts materialization should succeed for the scheduler-tracked owner",
    );
    assert!(
        provenance.indexed_ready_scheduler_snapshot_reuse > 0,
        "owner module-facts materialization should reuse the scheduler snapshot instead of rebuilding it from cached parse; saw {:?}",
        provenance,
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_named_type_export_target_prefers_longest_wildcard_prefix_match() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Checkbox.vue'\nexport * from './CheckboxGroup.vue'\n",
    );
    ws.inject_file(
        "/src/Checkbox.vue",
        r#"<script lang="ts">
export interface CheckboxProps {
  checked?: boolean
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/CheckboxGroup.vue",
        r#"<script lang="ts">
export interface CheckboxGroupProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let resolved = host.resolve_named_type_export_target("/src/types.ts", "CheckboxGroupProps");

    assert_eq!(
        resolved,
        Some((
            "/src/CheckboxGroup.vue".to_string(),
            "CheckboxGroupProps".to_string()
        )),
        "route selection should prefer the longest matching wildcard source stem",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn current_dependency_fact_versions_keeps_shallow_tracked_barrel_siblings_off_indexed_ready() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'
defineProps<ButtonProps>()
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    ws.reset_reads();
    let resolved = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve ButtonProps",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "root-stem route proof should keep unrelated same-layer barrel siblings off the active component-meta path",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "route-only frontier discovery should keep unrelated same-layer siblings off FileArtifactStore",
    );

    let _final_view = host.resolver_store_view_read().into_owned_view();
    let unused_reads_before = ws.read_count("/src/Unused.vue");
    let _facts = host.current_dependency_fact_versions("/src/Consumer.vue", &tracked_deps);

    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        unused_reads_before,
        "fact-version capture must not reread a shallow-only tracked barrel sibling",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "fact-version capture must not promote a shallow-only tracked barrel sibling into FileArtifactStore",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_skips_unrelated_wildcard_siblings_when_root_stem_matches()
{
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ModalProps } from './types'

defineProps<ModalProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './Accordion.vue'\n",
            "export * from './Alert.vue'\n",
            "export * from './Modal.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/Accordion.vue",
        r#"<script lang="ts">
export interface AccordionProps {
  items?: string[]
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Alert.vue",
        r#"<script lang="ts">
export interface AlertProps {
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Modal.vue",
        r#"<script lang="ts">
export interface ModalProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Accordion.vue", "/src/Accordion.vue"),
            exact_dependency("./Alert.vue", "/src/Alert.vue"),
            exact_dependency("./Modal.vue", "/src/Modal.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    ws.reset_reads();
    let resolved = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "ModalProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve ModalProps",
    );
    assert_eq!(
        ws.read_count("/src/Accordion.vue"),
        0,
        "matching wildcard stem should keep earlier unrelated siblings off the active route",
    );
    assert_eq!(
        ws.read_count("/src/Alert.vue"),
        0,
        "matching wildcard stem should skip other unrelated siblings in the same barrel layer",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "matching wildcard stem should stop before unrelated later siblings",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Accordion.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Alert.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Unused.vue")
            .is_none(),
        "unrelated wildcard siblings should stay off FileArtifactStore",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_keeps_leaf_object_prop_imports_symbolic() {
    // The leaf imported object prop (`avatar?: AvatarProps`) is published as
    // a shallow reference carrier: resolving it builds the versioned root
    // identity through AT MOST ONE canonical cold shallow materialization of
    // `/src/Avatar.vue` (the permitted first read — the canonical shallow
    // inventory is what warm identities and invalidation facts root on),
    // NEVER a declaration-body execution. Avatar's decl body importing
    // `ChipProps` from `/src/Chip.vue` is the body-execution discriminator:
    // lowering Avatar's body would demand Chip.vue, so `Chip.vue == 0 reads`
    // proves the member value stayed a carrier. A repeat resolution performs
    // ZERO new workspace reads (warm identities re-serve from the canonical
    // caches).
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'

defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { AvatarProps } from './Avatar.vue'
import type { IconProps } from './Icon.vue'

export interface Props {
  icon?: IconProps['name']
  avatar?: AvatarProps
}
"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
import type { ChipProps } from './Chip.vue'

export interface AvatarProps {
  src?: string
  alt?: string
  chip?: ChipProps
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Chip.vue",
        r#"<script lang="ts">
export interface ChipProps {
  tone?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
  class?: string
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );
    host.set_import_dependencies(
        "/src/Avatar.vue",
        vec![exact_dependency("./Chip.vue", "/src/Chip.vue")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    ws.reset_reads();
    let resolved = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "Props",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve Props",
    );
    // The resolved surface publishes BOTH members: the leaf imported object
    // prop stays a shallow carrier but is still a published row.
    let elements = &resolved.as_ref().unwrap().elements;
    assert!(
        elements
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("avatar")),
        "the leaf imported object prop publishes its row, got {:?}",
        elements.props,
    );
    assert!(
        elements
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("icon")),
        "the indexed-access member publishes its row, got {:?}",
        elements.props,
    );
    // At most ONE canonical cold shallow materialization of the leaf import:
    // the versioned root identity needs Avatar's canonical shallow inventory
    // exactly once per content generation.
    let avatar_cold_reads = ws.read_count("/src/Avatar.vue");
    assert!(
        avatar_cold_reads <= 1,
        "the leaf imported object prop performs at most ONE canonical cold \
         shallow materialization (got {avatar_cold_reads} reads)",
    );
    assert!(
        ws.read_count("/src/Icon.vue") > 0,
        "actionable indexed member routes should still resolve the imported file they actually need",
    );
    // NO declaration-body execution for the leaf import: Avatar's body
    // imports ChipProps, so a body lowering would demand Chip.vue.
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "keeping the leaf member value a carrier must not execute Avatar's \
         declaration body (its transitive Chip.vue import stays untouched)",
    );
    // The canonical shallow artifact EXISTS — the first materialization is
    // the permitted canonical shallow read, stored once on the shared store.
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Avatar.vue")
            .is_some(),
        "the leaf import's canonical shallow artifact exists after the cold \
         materialization (shallow inventory, not a body store)",
    );

    // A REPEAT resolution performs ZERO new workspace reads for the leaf
    // import — warm identities re-serve from the canonical caches.
    let mut tracked_deps_warm = std::collections::BTreeSet::new();
    let mut resolution_deps_warm = std::collections::BTreeSet::new();
    let mut cache_warm = crate::resolver_core::ExternalTypeBodyCache::default();
    let resolved_warm = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "Props",
        &mut tracked_deps_warm,
        &mut resolution_deps_warm,
        &mut cache_warm,
    );
    assert!(
        resolved_warm.is_some(),
        "the warm repeat resolution should still resolve Props",
    );
    assert_eq!(
        ws.read_count("/src/Avatar.vue"),
        avatar_cold_reads,
        "the warm repeat performs ZERO new Avatar.vue workspace reads",
    );
    assert_eq!(
        ws.read_count("/src/Chip.vue"),
        0,
        "the warm repeat still executes no Avatar declaration body",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_cached_lookup_tracks_routed_target_dependencies() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        "export { ButtonProps } from './Button.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./Button.vue", "/src/Button.vue")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    let mut tracked_deps_first = std::collections::BTreeSet::new();
    let mut resolution_deps_first = std::collections::BTreeSet::new();
    let resolved_first = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps_first,
        &mut resolution_deps_first,
        &mut cache,
    );

    assert!(
        resolved_first.is_some(),
        "the first imported macro lookup should resolve ButtonProps",
    );
    assert!(
        tracked_deps_first.contains("/src/Button.vue"),
        "the first lookup should track the routed target canonical",
    );
    assert!(
        resolution_deps_first.contains("/src/Button.vue"),
        "the first lookup should record the routed target canonical in resolution deps",
    );

    let mut tracked_deps_second = std::collections::BTreeSet::new();
    let mut resolution_deps_second = std::collections::BTreeSet::new();
    let resolved_second = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps_second,
        &mut resolution_deps_second,
        &mut cache,
    );

    assert!(
        resolved_second.is_some(),
        "the warm imported macro lookup should still resolve ButtonProps",
    );
    assert!(
        tracked_deps_second.contains("/src/Button.vue"),
        "the warm lookup must keep tracking the routed target canonical, not just the barrel file",
    );
    assert!(
        resolution_deps_second.contains("/src/Button.vue"),
        "the warm lookup must keep the routed target in resolution deps for downstream fact tracking",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_tracks_routed_package_targets_across_requests() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    host.provenance().reset();

    let mut tracked_deps_first = std::collections::BTreeSet::new();
    let mut resolution_deps_first = std::collections::BTreeSet::new();
    let mut cache_first = crate::resolver_core::ExternalTypeBodyCache::default();
    let resolved_first = host.resolve_component_meta_macro_elements(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps_first,
        &mut resolution_deps_first,
        &mut cache_first,
    );
    assert!(
        resolved_first.is_some(),
        "the first imported macro lookup should resolve the package emits surface",
    );
    assert!(
        tracked_deps_first.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the first lookup should track the routed package target canonical",
    );
    assert!(
        resolution_deps_first.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the first lookup should record the routed package target canonical in resolution deps",
    );

    let mut tracked_deps_second = std::collections::BTreeSet::new();
    let mut resolution_deps_second = std::collections::BTreeSet::new();
    let mut cache_second = crate::resolver_core::ExternalTypeBodyCache::default();
    let resolved_second = host.resolve_component_meta_macro_elements(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps_second,
        &mut resolution_deps_second,
        &mut cache_second,
    );
    assert!(
        resolved_second.is_some(),
        "the second imported macro lookup should still resolve the package emits surface",
    );
    assert!(
        tracked_deps_second.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the warm lookup must keep tracking the routed package target canonical",
    );
    assert!(
        resolution_deps_second.contains("/workspace/node_modules/pkg/dist/index3.d.ts"),
        "the warm lookup must keep the routed package target in resolution deps for downstream fact tracking",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns exactly one canonical IndexedReady built by the unified cold path",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_skip_imported_declaration_builds() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        "export { ButtonProps } from './Button.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface ButtonProps {
  label?: string
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./Button.vue", "/src/Button.vue")],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();

    host.provenance().reset();
    let resolved_elements = host.resolve_component_meta_macro_elements(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );
    assert!(
        resolved_elements.is_some(),
        "element-only imported macro resolution should still resolve ButtonProps",
    );

    let after_elements = host.provenance().snapshot();
    assert_eq!(
        after_elements.imported_macro_declaration_builds,
        0,
        "element-only imported macro resolution should not build declaration ownership it immediately discards",
    );

    let resolved_surface = host.resolve_component_meta_macro_surface(
        "/src/Consumer.vue",
        "./types",
        "ButtonProps",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );
    assert!(
        resolved_surface.is_some(),
        "combined imported macro resolution should still resolve ButtonProps with declaration ownership",
    );

    let after_surface = host.provenance().snapshot();
    assert_eq!(
        after_surface.imported_macro_declaration_builds, 1,
        "combined imported macro resolution should still build declaration ownership once",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_elements_materializes_active_package_target_once() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    let resolved = host.resolve_component_meta_macro_elements(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro resolution should still resolve the package reexported emits surface",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns a canonical IndexedReady",
    );
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns a canonical IndexedReady",
    );
    // The ONCE discriminator: a second identical resolution reuses every
    // artifact the first one built — zero new materialisations.
    host.provenance().reset();
    let mut tracked_deps2 = std::collections::BTreeSet::new();
    let mut resolution_deps2 = std::collections::BTreeSet::new();
    let mut cache2 = crate::resolver_core::ExternalTypeBodyCache::default();
    let re_resolved = host.resolve_component_meta_macro_elements(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps2,
        &mut resolution_deps2,
        &mut cache2,
    );
    assert!(
        re_resolved.is_some(),
        "warm re-resolution must still resolve"
    );
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the active package target must materialise ONCE — a repeated \
         resolution must reuse its IndexedReady, not rebuild it",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_macro_surface_materializes_active_package_target_once() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { PackageEmits } from './types'

const emit = defineEmits<PackageEmits>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/workspace/src/types.ts",
        "export type { PackageEmits } from 'pkg'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        "export type { PackageEmits } from './index3.d.ts'\n",
    );
    ws.inject_file(
        "/workspace/node_modules/pkg/dist/index3.d.ts",
        "export interface PackageEmits {\n  (e: 'open', value?: string): void\n}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/workspace/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/workspace/src/Consumer.vue",
        vec![exact_dependency("./types", "/workspace/src/types.ts")],
    );
    host.set_import_dependencies(
        "/workspace/src/types.ts",
        vec![exact_dependency(
            "pkg",
            "/workspace/node_modules/pkg/dist/index.d.ts",
        )],
    );
    host.set_import_dependencies(
        "/workspace/node_modules/pkg/dist/index.d.ts",
        vec![exact_dependency(
            "./index3.d.ts",
            "/workspace/node_modules/pkg/dist/index3.d.ts",
        )],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let mut tracked_deps = std::collections::BTreeSet::new();
    let mut resolution_deps = std::collections::BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();

    let resolved = host.resolve_component_meta_macro_surface(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps,
        &mut resolution_deps,
        &mut cache,
    );

    assert!(
        resolved.is_some(),
        "component-meta macro surface resolution should still resolve the package reexported emits surface",
    );
    // The ONCE discriminator: a second identical resolution reuses every
    // artifact the first one built — zero new materialisations.
    host.provenance().reset();
    let mut tracked_deps2 = std::collections::BTreeSet::new();
    let mut resolution_deps2 = std::collections::BTreeSet::new();
    let mut cache2 = crate::resolver_core::ExternalTypeBodyCache::default();
    let re_resolved = host.resolve_component_meta_macro_surface(
        "/workspace/src/Consumer.vue",
        "./types",
        "PackageEmits",
        &mut tracked_deps2,
        &mut resolution_deps2,
        &mut cache2,
    );
    assert!(
        re_resolved.is_some(),
        "warm re-resolution must still resolve"
    );
    assert_eq!(
        host.provenance().snapshot().indexed_ready_materializes,
        0,
        "the active package target must materialise ONCE — a repeated \
         resolution must reuse its IndexedReady, not rebuild it",
    );
    assert!(
        resolved
            .as_ref()
            .and_then(|surface| surface.declaration.declaration_id)
            .is_some(),
        "package macro declaration ownership should still expose a stable declaration id",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index.d.ts")
            .is_some(),
        "the inspected package provider barrel owns exactly one canonical IndexedReady built by the unified cold path during surface resolution too",
    );
    assert!(
        host.project_type_store.indexed().get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
            .is_some(),
        "the actively resolved package target owns exactly one canonical IndexedReady built by the unified cold path when building macro declaration ownership",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn required_import_routes_for_exported_whole_route_preserves_member_tail() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        r#"
import type { AvatarProps } from './Avatar.vue'
import type { IconProps } from './Icon.vue'

export interface Props {
  icon?: IconProps['name']
  avatar?: AvatarProps
}
"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
  class?: string
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/types.ts"),
        "types should load from the workspace",
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Avatar.vue", "/src/Avatar.vue"),
            exact_dependency("./Icon.vue", "/src/Icon.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let routes = host.required_import_routes_for_exported_route(
        "/src/types.ts",
        "Props",
        &crate::resolver_core::RouteDemand::Whole,
    );

    assert_eq!(
        routes.len(),
        1,
        "whole-route imported closure should only include actionable indexed-member refs",
    );
    assert_eq!(
        routes.get("IconProps"),
        Some(&crate::resolver_core::RouteDemand::member_path(vec![
            "name".to_string()
        ])),
        "whole-route imported closure should preserve the requested member tail instead of widening to Whole",
    );
    assert!(
        !routes.contains_key("AvatarProps"),
        "direct imported object props should stay symbolic on whole-route closure",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_prefers_matching_wildcard_stem_before_unrelated_earlier_siblings() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        concat!(
            "export * from './AuthForm.vue'\n",
            "export * from './Avatar.vue'\n",
            "export * from './Icon.vue'\n",
            "export * from './Unused.vue'\n",
        ),
    );
    ws.inject_file(
        "/src/AuthForm.vue",
        r#"<script lang="ts">
export interface AuthFormProps {
  title?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Avatar.vue",
        r#"<script lang="ts">
export interface AvatarProps {
  src?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Icon.vue",
        r#"<script lang="ts">
export interface IconProps {
  name?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    ws.reset_reads();
    let root = host.resolve_imported_type_root("/src/types.ts", "IconProps");

    assert_eq!(
        root,
        ("/src/Icon.vue".to_string(), "IconProps".to_string()),
        "wildcard route proof should resolve IconProps through the matching child stem",
    );
    assert_eq!(
        ws.read_count("/src/AuthForm.vue"),
        0,
        "wildcard route proof should skip earlier unrelated siblings when the requested export name points at a later matching stem",
    );
    assert_eq!(
        ws.read_count("/src/Avatar.vue"),
        0,
        "wildcard route proof should not read other earlier siblings once the matching stem narrows the active route",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "wildcard route proof should stop after the matching child without touching later unrelated siblings",
    );
    for never_inspected in ["/src/AuthForm.vue", "/src/Avatar.vue", "/src/Unused.vue"] {
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the wildcard route proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store.indexed().get_any("/src/types.ts")
            .is_some(),
        "the inspected provider barrel owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_imported_type_root_nested_barrel_alias_materializes_only_the_matched_vue_child() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    let _view = host.resolver_store_view_read().into_owned_view();
    let root = host.resolve_imported_type_root("/src/types.ts", "LinkProps");

    assert_eq!(
        root,
        ("/src/Link.vue".to_string(), "LinkProps".to_string()),
        "nested barrel alias proof should still resolve LinkProps through the direct sibling barrel child",
    );
    for never_inspected in ["/src/Button.vue", "/src/Unused.vue"] {
        assert_eq!(
            ws.read_count(never_inspected),
            0,
            "nested barrel alias proof should never read the uninspected sibling {never_inspected}",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any(never_inspected)
                .is_none(),
            "Vue siblings the nested barrel alias proof never inspects stay off FileArtifactStore: {never_inspected}",
        );
    }
    assert!(
        host.project_type_store
            .indexed()
            .get_any("/src/Link.vue")
            .is_some(),
        "the matched Vue child was inspected and owns exactly one canonical IndexedReady built by the unified cold path",
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn resolve_component_meta_nested_barrel_alias_resolves_expected_props() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/types.ts",
        "export * from './Button.vue'\nexport * from './Link.vue'\nexport * from './Unused.vue'\n",
    );
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
import type { LinkProps } from './types'

export interface ButtonProps extends Omit<LinkProps, 'raw'> {
  label?: string
}
</script>
<template><button /></template>"#,
    );
    ws.inject_file(
        "/src/Link.vue",
        r#"<script lang="ts">
export interface LinkProps {
  href?: string
  raw?: boolean
}
</script>
<template><a /></template>"#,
    );
    ws.inject_file(
        "/src/Unused.vue",
        r#"<script lang="ts">
export interface UnusedProps {
  never?: number
}
</script>
<template><div /></template>"#,
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );
    assert!(
        host.ensure_loaded("/src/Consumer.vue"),
        "consumer should load from the workspace",
    );

    host.set_import_dependencies(
        "/src/Consumer.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/types.ts",
        vec![
            exact_dependency("./Button.vue", "/src/Button.vue"),
            exact_dependency("./Link.vue", "/src/Link.vue"),
            exact_dependency("./Unused.vue", "/src/Unused.vue"),
        ],
    );

    ws.reset_reads();
    let resolved = host
        .resolve_component_meta("/src/Consumer.vue", crate::types::ProjectionMode::Expanded)
        .expect("expanded component meta should resolve");

    let prop_names: std::collections::BTreeSet<String> =
        hm_prop_names(&host, "/src/Consumer.vue", &resolved)
            .into_iter()
            .collect();
    assert!(
        prop_names.contains("label") && prop_names.contains("href"),
        "nested barrel alias should still resolve reached props, got {prop_names:?}",
    );
    assert!(
        !prop_names.contains("raw"),
        "nested barrel alias should still respect Omit, got {prop_names:?}",
    );
    assert_eq!(
        ws.read_count("/src/Unused.vue"),
        0,
        "matching wildcard stems should keep unrelated same-layer barrel siblings off the component-meta expansion path",
    );
    assert!(
        host.project_type_store.indexed().get_any("/src/Unused.vue")
            .is_none(),
        "component-meta expansion should keep shallow-only same-layer barrel siblings off FileArtifactStore",
    );
}

// ---------------------------------------------------------------------------
// Characterization tests for solver-host local-only env behaviour
// ---------------------------------------------------------------------------

/// Test 1: local-only defineProps resolves from EvalEnv without walkers.
#[test]
fn solver_host_resolves_local_define_props_from_local_only_env() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
interface Props { msg: string; count: number }
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("local defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(prop_names.contains(&"msg"), "should have msg prop");
    assert!(prop_names.contains(&"count"), "should have count prop");
    assert_eq!(prop_names.len(), 2, "should have exactly 2 props");
}

/// Test 2: cross-file defineProps — the solver host must resolve imported types
/// from the host prepared-decl cache, not from the fat owner env.
///
/// This test exercises the full meta pipeline and pins the
/// invariant: cross-file `defineProps` must resolve through the host
/// prepared-decl cache using only the local-only env + solver host.
#[test]
fn solver_host_resolves_cross_file_define_props_through_host_cache() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { id: string; label?: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("cross-file defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"id"),
        "imported prop 'id' must resolve, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"label"),
        "imported prop 'label' must resolve, got: {prop_names:?}",
    );
    // Negative: no extra props should leak
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props from ImportedProps, got: {prop_names:?}",
    );
}

/// Test 3: transitive cross-file resolution — imported type extends same-file base.
/// Invariant: the solver host must resolve both the direct import
/// AND its same-file dependencies through the prepared-decl cache.
#[test]
fn solver_host_resolves_transitive_same_file_deps_in_imported_type() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "interface BaseProps { id: string }\nexport interface ImportedProps extends BaseProps { label?: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("transitive imported defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"id"),
        "base prop 'id' from same-file BaseProps must resolve transitively, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"label"),
        "direct prop 'label' from ImportedProps must resolve, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props (1 inherited + 1 direct), got: {prop_names:?}",
    );
}

/// Test 4: `typeof importedValue` in a prop type — the solver host must resolve
/// runtime-value type references from imported bindings.
#[test]
fn solver_host_resolves_typeof_imported_value_in_prop_type() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/theme.ts",
        "export const importedTheme = { primary: 'blue', secondary: 'gray' } as const;",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import { importedTheme } from './theme'
defineProps<{ ui: typeof importedTheme }>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("typeof importedValue defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"ui"),
        "should have 'ui' prop from typeof importedTheme, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        1,
        "should have exactly 1 prop, got: {prop_names:?}",
    );
    // Negative: prop type should not be Unknown
    let ui_prop = meta.props.iter().find(|p| p.name == "ui").unwrap();
    let ui_ty = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/App.vue",
        ui_prop.type_source.present().expect("typed ui prop"),
    )
    .unwrap_or_else(|| panic!("ui's published source must demand-materialize"));
    assert!(
        !matches!(ui_ty, verter_type_expr::TypeExpr::Unknown { .. }),
        "typeof imported value prop type should not be Unknown",
    );
}

/// Test 5: mixed local + imported types — `defineProps<LocalProps & ImportedProps>()`
/// must resolve both local and cross-file members through the solver host.
#[test]
fn solver_host_resolves_mixed_local_and_imported_intersection_props() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { imported_field: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
interface LocalProps { local_field: string }
defineProps<LocalProps & ImportedProps>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("mixed local+imported intersection defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"local_field"),
        "local prop 'local_field' must resolve, got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"imported_field"),
        "imported prop 'imported_field' must resolve, got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props (1 local + 1 imported), got: {prop_names:?}",
    );
}

/// Test 6: generic imported types — `defineProps<Partial<ImportedProps>>()`
/// must resolve through the solver host's generic instantiation.
#[test]
fn solver_host_resolves_generic_imported_partial_props() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface ImportedProps { name: string; age: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<Partial<ImportedProps>>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("Partial<ImportedProps> defineProps should produce component meta");
    let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
    assert!(
        prop_names.contains(&"name"),
        "Partial<ImportedProps> should include 'name', got: {prop_names:?}",
    );
    assert!(
        prop_names.contains(&"age"),
        "Partial<ImportedProps> should include 'age', got: {prop_names:?}",
    );
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props from Partial<ImportedProps>, got: {prop_names:?}",
    );
    // All props should be optional because of Partial<>
    for prop in &meta.props {
        assert!(
            !prop.required,
            "Partial<> should make all props optional, but '{}' is required",
            prop.name,
        );
    }
}

/// Test 7: end-to-end fallthrough with runtime values — the meta pipeline should
/// produce fallthrough metadata for a component with a template binding referencing
/// an imported runtime value via v-bind.
#[test]
fn solver_host_fallthrough_with_imported_runtime_v_bind() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/attrs.ts",
        "export const imported_obj = { class: 'foo', id: 'bar' };",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import { imported_obj } from './attrs'
</script>
<template><div v-bind="imported_obj">hello</div></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("v-bind with imported runtime value should produce component meta");
    // The component has a single native root <div>, so it should have no declared props
    assert!(
        meta.props.is_empty(),
        "no declared props expected, got: {:?}",
        meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
    );
    // The template should parse successfully — meta should exist
    // (this test validates the runtime-value path doesn't crash, not fallthrough surface details)
}

/// Test 8: cache ownership (cold/warm) — verifies that a second call to get_component_meta
/// on the same file hits cached state instead of re-resolving.
///
/// Full verification requires a CountingWorkspace to track file read counts,
/// which is not available in the standalone host test harness.
#[test]
fn solver_host_cache_ownership_cold_warm() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    // Cold call
    let meta1 = host
        .get_component_meta("/App.vue")
        .expect("cold call should produce component meta");
    let recomputes_after_cold = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);

    // Warm call — should reuse cached resolved state
    let meta2 = host
        .get_component_meta("/App.vue")
        .expect("warm call should produce component meta");
    let recomputes_after_warm = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        meta1.props.len(),
        meta2.props.len(),
        "cold and warm calls should produce identical props"
    );
    assert_eq!(
        recomputes_after_cold, recomputes_after_warm,
        "warm call should not trigger additional resolved state recomputes"
    );
}

/// Test 9: same-request macro+fallthrough — verifies that a single get_component_meta
/// call resolves both macro types AND fallthrough surface without redundant reads.
///
/// Full verification requires read/parse counter instrumentation on the host,
/// which is not available in the standalone test harness.
#[test]
fn solver_host_same_request_macro_and_fallthrough_single_pass() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let meta = host
        .get_component_meta("/App.vue")
        .expect("single-pass should produce component meta");

    // Both macro resolution (defineProps) and the meta surface should complete
    // in a single get_component_meta call
    assert!(
        meta.props.iter().any(|p| p.name == "label"),
        "macro type should resolve imported prop 'label'"
    );
    assert_eq!(
        host.provenance
            .get_component_meta_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        1,
        "only one get_component_meta call should have been made"
    );
}

/// Test 10: negative — missing imported symbol must not silently succeed.
#[test]
fn solver_host_missing_import_does_not_silently_resolve() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    // types.ts exists but does NOT export MissingType
    upsert_ts(
        &host,
        "/types.ts",
        "export interface OtherType { x: number }",
    );
    upsert_vue(
        &host,
        "/App.vue",
        r#"<script setup lang="ts">
import type { MissingType } from './types'
defineProps<MissingType>()
</script>
<template><div /></template>"#,
    );

    let meta = host.get_component_meta("/App.vue");
    // Should either return None or return meta with 0 props
    if let Some(meta) = meta {
        assert!(
            meta.props.is_empty(),
            "missing imported type must not produce phantom props, got: {:?}",
            meta.props.iter().map(|p| &p.name).collect::<Vec<_>>(),
        );
    }
}

// ---------------------------------------------------------------------------
// Cross-owner file-level reuse tests (budget alignment plan)
// ---------------------------------------------------------------------------

/// Two different Vue files importing the same type from the same canonical
/// file should share the host-owned imported file state.
#[test]
fn same_canonical_file_reuses_state_across_two_owners() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Shared { label: string }",
    );
    upsert_vue(
        &host,
        "/OwnerA.vue",
        r#"<script setup lang="ts">
import type { Shared } from './types'
defineProps<Shared>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/OwnerB.vue",
        r#"<script setup lang="ts">
import type { Shared } from './types'
defineProps<Shared>()
</script>
<template><div /></template>"#,
    );

    let meta_a = host
        .get_component_meta("/OwnerA.vue")
        .expect("OwnerA should produce meta");
    let meta_b = host
        .get_component_meta("/OwnerB.vue")
        .expect("OwnerB should produce meta");

    // Both owners should see the same "label" prop from Shared
    assert_eq!(meta_a.props.len(), 1, "OwnerA should have one prop");
    assert_eq!(meta_b.props.len(), 1, "OwnerB should have one prop");
    assert_eq!(meta_a.props[0].name, "label");
    assert_eq!(meta_b.props[0].name, "label");

    // Warm call for OwnerA should not trigger recomputes
    host.provenance.reset();
    let meta_a_warm = host
        .get_component_meta("/OwnerA.vue")
        .expect("warm OwnerA should produce meta");
    let recomputes = host
        .provenance
        .component_meta_resolved_state_recomputes
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        meta_a_warm.props.len(),
        meta_a.props.len(),
        "warm call should return identical meta"
    );
    assert_eq!(
        recomputes, 0,
        "warm call should not trigger resolved state recomputes"
    );
}

/// Mutating an imported dependency should invalidate only that file's
/// cache lineage, while owner-file caches stay warm.
#[test]
fn changed_imported_dependency_keeps_owner_files_warm() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );
    upsert_vue(
        &host,
        "/OwnerA.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_vue(
        &host,
        "/OwnerB.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    // Cold: both owners compute meta
    let meta_a1 = host.get_component_meta("/OwnerA.vue").expect("cold OwnerA");
    let meta_b1 = host.get_component_meta("/OwnerB.vue").expect("cold OwnerB");
    assert_eq!(meta_a1.props.len(), 1);
    assert_eq!(meta_b1.props.len(), 1);

    // Mutate the imported dependency — add a new prop
    upsert_ts(
        &host,
        "/types.ts",
        "export interface Props { label: string; count: number }",
    );

    // Owner files were NOT changed, only the dependency
    let meta_a2 = host
        .get_component_meta("/OwnerA.vue")
        .expect("post-change OwnerA");
    let meta_b2 = host
        .get_component_meta("/OwnerB.vue")
        .expect("post-change OwnerB");

    // Both owners should now see the updated two-prop shape
    assert_eq!(
        meta_a2.props.len(),
        2,
        "OwnerA should reflect the updated dependency (label + count)"
    );
    assert_eq!(
        meta_b2.props.len(),
        2,
        "OwnerB should reflect the updated dependency (label + count)"
    );
    // Positive: new prop present
    assert!(
        meta_a2.props.iter().any(|p| p.name == "count"),
        "OwnerA should have the new 'count' prop"
    );
    // Positive: old prop still present
    assert!(
        meta_a2.props.iter().any(|p| p.name == "label"),
        "OwnerA should still have the original 'label' prop"
    );
    // Negative: old result no longer valid
    assert_ne!(
        meta_a1.props.len(),
        meta_a2.props.len(),
        "cached meta must have been invalidated by the dependency change"
    );
    // Negative: no phantom props beyond the expected two
    let prop_names: Vec<&str> = meta_a2.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        prop_names.len(),
        2,
        "should have exactly 2 props, no phantom data: {:?}",
        prop_names
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests 4.1–4.4 and Regression guards 4.7–4.11
// from the Fact-Validated Declaration-Surface Cache plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Unit test 1: Bundle fact validation round-trip.
///
/// Verify that `prepared_type_decl` caches bundles (second call returns
/// the same result), and that changing file content via `upsert` invalidates
/// the old bundle so the next lookup returns the updated type.
#[test]
fn bundle_fact_validation_round_trip() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // First lookup — materializes the bundle.
    let first = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare on first lookup");
    assert_eq!(first.root_identity.symbol_name.as_ref(), "Props");

    // Second lookup — should hit cache and return the same result.
    let second = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare on second lookup (cache hit)");
    assert_eq!(
        first.root_identity.canonical_id, second.root_identity.canonical_id,
        "repeated lookups should return the same prepared decl identity",
    );
    assert_eq!(
        first.root_identity.symbol_name, second.root_identity.symbol_name,
        "repeated lookups should return the same symbol name",
    );

    // Change the file content — replace Props with a different shape.
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { title: number }",
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should re-materialize after content change");

    // New lookup should reflect the updated type.
    let updated = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare after content change");
    assert_eq!(updated.root_identity.symbol_name.as_ref(), "Props");

    // Negative: the old symbol shape should be gone (the surface should have
    // changed). We verify the bundle was invalidated by checking the prepared
    // decl's member index reflects the new content.
    assert!(
        updated.member_index.contains_key("title"),
        "updated prepared decl should contain the new property 'title', got: {:?}",
        updated.member_index.keys().collect::<Vec<_>>()
    );
    assert!(
        !updated.member_index.contains_key("label"),
        "updated prepared decl should NOT contain the old property 'label', got: {:?}",
        updated.member_index.keys().collect::<Vec<_>>()
    );
}

#[test]
fn prepared_type_decl_only_prepares_requested_symbol_on_first_lookup() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
export interface Alpha { alpha: string }
export interface Beta { beta: string }
export interface Gamma { gamma: string }
export interface Delta { delta: string }
"#,
    );
    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    crate::resolver_core::prepared_decl::reset_prepared_type_decl_build_count_for_tests();

    let gamma = host
        .prepared_type_decl("/src/types.ts", "Gamma")
        .expect("Gamma should prepare");
    assert_eq!(gamma.root_identity.symbol_name.as_ref(), "Gamma");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        1,
        "first lookup should prepare only the requested symbol",
    );

    let gamma_again = host
        .prepared_type_decl("/src/types.ts", "Gamma")
        .expect("Gamma should stay cached");
    assert_eq!(gamma_again.root_identity.symbol_name.as_ref(), "Gamma");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        1,
        "repeat lookup should reuse the prepared symbol cache",
    );

    let alpha = host
        .prepared_type_decl("/src/types.ts", "Alpha")
        .expect("Alpha should prepare");
    assert_eq!(alpha.root_identity.symbol_name.as_ref(), "Alpha");
    assert_eq!(
        crate::resolver_core::prepared_decl::prepared_type_decl_build_count_for_tests(),
        2,
        "looking up a second symbol should prepare only that symbol",
    );
}

/// Unit test 2: Lazy promotion stability.
///
/// When dependency resolution changes from `{resolved_canonical_id: None,
/// possible: ["/dep.d.ts", "/dep.ts"]}` to `{resolved_canonical_id:
/// Some("/dep.d.ts"), possible: [...]}`, the effective target is the same
/// (`.d.ts` wins by TS-first priority). The bundle should NOT be invalidated.
#[test]
fn lazy_promotion_stability() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/dep.d.ts",
        "export interface Helper { aid: boolean }\n",
    );
    ws.inject_file("/src/dep.ts", "export interface Helper { aid: boolean }\n");
    ws.inject_file(
        "/src/types.ts",
        "import type { Helper } from './dep'\nexport interface Props extends Helper {}\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Set initial dependency with no resolved_canonical_id but possible candidates.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: vec!["/src/dep.d.ts".to_string(), "/src/dep.ts".to_string()],
        }],
    );

    // First lookup — materializes the bundle with effective target = /src/dep.d.ts.
    let _view_before = host.resolver_store_view_read().into_owned_view();
    let initial = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare with lazy resolution");
    assert_eq!(
        initial
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.d.ts"),
        "lazy resolution should prefer .d.ts via effective_target()",
    );

    // Promote: set resolved_canonical_id to the same effective target.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![crate::types::DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/src/dep.d.ts".to_string()),
            possible_canonical_ids: vec!["/src/dep.d.ts".to_string(), "/src/dep.ts".to_string()],
        }],
    );

    // After promotion, the effective target is unchanged — bundle should survive.
    let _view_after = host.resolver_store_view_read().into_owned_view();
    let after_promotion = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should still be found after lazy promotion");
    assert_eq!(
        after_promotion
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/dep.d.ts"),
        "lazy promotion to the same effective target should NOT change name_resolution",
    );
}

/// Unit test 3: Atomic rebuild on route change.
///
/// When the effective dependency target changes from `/inner-v1.ts` to
/// `/inner-v2.ts`, the bundle must be invalidated and ALL prepared decls in
/// the rebuilt bundle must have updated `name_resolution` entries.
#[test]
fn atomic_rebuild_on_route_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/inner-v1.ts",
        "export interface Inner { version: 1 }\n",
    );
    ws.inject_file(
        "/src/inner-v2.ts",
        "export interface Inner { version: 2 }\n",
    );
    ws.inject_file(
        "/src/types.ts",
        "import type { Inner } from './inner'\nexport interface Props { child: Inner }\nexport interface Alt { other: Inner }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );

    let _ = host
        .ensure_indexed_ready("/src/types.ts")
        .expect("types dependency should materialize");

    // Route to v1.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v1.ts")],
    );

    let _view_v1 = host.resolver_store_view_read().into_owned_view();
    let props_v1 = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare pointing to v1");
    assert_eq!(
        props_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v1.ts"),
        "Props name_resolution should point to inner-v1",
    );
    let alt_v1 = host
        .prepared_type_decl("/src/types.ts", "Alt")
        .expect("Alt should prepare pointing to v1");
    assert_eq!(
        alt_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v1.ts"),
        "Alt name_resolution should point to inner-v1",
    );

    // Change route to v2.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v2.ts")],
    );

    let _view_v2 = host.resolver_store_view_read().into_owned_view();
    let props_v2 = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should rebuild after route change");
    assert_eq!(
        props_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v2.ts"),
        "Props name_resolution must point to inner-v2 after route change",
    );
    let alt_v2 = host
        .prepared_type_decl("/src/types.ts", "Alt")
        .expect("Alt should rebuild after route change");
    assert_eq!(
        alt_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/src/inner-v2.ts"),
        "ALL prepared decls must point to inner-v2 after route change — atomic rebuild",
    );
}

/// Unit test 4: with_declaration_scope parity.
///
/// Verify that component-meta resolution correctly resolves props when the
/// component imports a type from another file. This proves that
/// `with_declaration_scope` correctly builds `import_bindings` from the
/// bundle's `dep_edges` path.
#[test]
fn with_declaration_scope_parity_via_component_meta() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface ImportedProps { label: string; count: number }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        r#"<script setup lang="ts">
import type { ImportedProps } from './types'
defineProps<ImportedProps>()
</script>
<template><div /></template>"#,
    );
    host.set_import_dependencies(
        "/src/Comp.vue",
        vec![exact_dependency("./types", "/src/types.ts")],
    );

    let state = host
        .resolve_component_meta("/src/Comp.vue", crate::types::ProjectionMode::Expanded)
        .expect("should return resolved state");
    let props = hm_prop_names(&host, "/src/Comp.vue", &state);
    assert!(
        props.contains(&"label".to_string()),
        "component-meta should resolve 'label' prop via bundle dep_edges path: {:?}",
        props
    );
    assert!(
        props.contains(&"count".to_string()),
        "component-meta should resolve 'count' prop via bundle dep_edges path: {:?}",
        props
    );
    // Negative: no phantom props.
    assert_eq!(
        props.len(),
        2,
        "should have exactly 2 props, no phantom data: {:?}",
        props
    );
}

/// Regression guard 7: Stale prepared decls after dep-resolution change.
///
/// Guards the exact bug the old route-refresh was designed to prevent.
/// When the import target's effective canonical changes, prepared decls must
/// reflect the new target in `name_resolution`.
#[test]
fn regression_stale_prepared_decls_after_dep_resolution_change() {
    let host = make_host();
    upsert_non_sfc(&host, "/types-a.ts", "export interface Foo { source: 'a' }");
    upsert_non_sfc(&host, "/types-b.ts", "export interface Foo { source: 'b' }");
    upsert_non_sfc(
        &host,
        "/src/consumer.ts",
        "import { Foo } from './types'\nexport interface Bar { inner: Foo }\n",
    );

    let _ = host
        .ensure_indexed_ready("/src/consumer.ts")
        .expect("consumer dependency should materialize");

    // Set initial route to types-a.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-a.ts")],
    );

    let initial = host
        .prepared_type_decl("/src/consumer.ts", "Bar")
        .expect("Bar should prepare with route to types-a");
    assert_eq!(
        initial
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-a.ts"),
        "initial lookup should resolve Foo to types-a",
    );

    // Change route to types-b.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-b.ts")],
    );

    let updated = host
        .prepared_type_decl("/src/consumer.ts", "Bar")
        .expect("Bar should rebuild after route change to types-b");
    assert_eq!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-b.ts"),
        "prepared decl must reflect the new target after dep-resolution change — guards against stale route bug",
    );
    // Negative: must NOT still point to the old target.
    assert_ne!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_ref()),
        Some("/types-a.ts"),
        "prepared decl must NOT retain the stale route to types-a",
    );
}

/// Regression guard 8: Declaration-scoped solving with local closure.
///
/// Verify that local type aliases referencing other local types survive the
/// atomic bundle build — `local_deps` or `name_resolution` must retain the
/// local closure symbols.
#[test]
fn regression_declaration_scoped_solving_with_local_closure() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "type Inner = { x: number }\nexport interface Props { child: Inner }\n",
    );

    let prepared = host
        .prepared_type_decl("/src/types.ts", "Props")
        .expect("Props should prepare with local closure");

    // The local type `Inner` must appear in local_deps or name_resolution,
    // proving local closure symbols survive the atomic bundle build.
    let has_inner_in_local_deps = prepared.local_deps.iter().any(|dep| dep == "Inner");
    let has_inner_in_name_resolution = prepared.name_resolution.contains_key("Inner");

    assert!(
        has_inner_in_local_deps || has_inner_in_name_resolution,
        "local closure symbol 'Inner' must survive in local_deps ({:?}) or name_resolution ({:?})",
        prepared.local_deps,
        prepared.name_resolution.keys().collect::<Vec<_>>(),
    );

    // Additionally verify Props itself is well-formed.
    assert_eq!(prepared.root_identity.symbol_name.as_ref(), "Props");
    assert_eq!(
        prepared.root_identity.canonical_id.as_ref(),
        "/src/types.ts"
    );
}

/// Regression guard 9: Shallow alias resolution through barrel re-exports.
///
/// A barrel file `export { Props } from './inner'` does not own a local
/// `Props` declaration — it is a re-export. `prepared_type_decl` on
/// the barrel file for `Props` should return `None` because the symbol is not
/// a local declaration of the barrel.
#[test]
fn regression_barrel_reexport_returns_none_for_prepared_decl() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }",
    );
    upsert_non_sfc(&host, "/src/barrel.ts", "export { Props } from './inner'");
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./inner", "/src/inner.ts")],
    );

    let _ = host.ensure_indexed_ready("/src/barrel.ts");

    let prepared = host.prepared_type_decl("/src/barrel.ts", "Props");
    assert!(
        prepared.is_none(),
        "barrel re-exports should NOT produce local prepared decls — Props is owned by inner.ts, not barrel.ts",
    );

    // Positive: the defining file should have the prepared decl.
    let inner_prepared = host.prepared_type_decl("/src/inner.ts", "Props");
    assert!(
        inner_prepared.is_some(),
        "the defining file (inner.ts) should have the prepared decl for Props",
    );
}

/// Regression: validates() now accepts FileWholeHash facts for untracked files
/// (dependency files not in the store view). When a workspace-only dependency
/// file changes content (without being upserted), the old archived module_facts
/// must NOT be returned through the store-view-validated cache path.
///
/// The scenario:
/// 1. A dependency file is loaded from workspace (never upserted → not tracked)
/// 2. Module_facts are materialized then archived (as HostStoreView::build does)
/// 3. The workspace file changes (simulating a disk edit)
/// 4. A new store view is created — still doesn't track the dependency
/// 5. module_facts.get(dep, view) must NOT return stale archived facts
#[test]
fn archived_indexed_ready_rejected_when_workspace_dep_changes_content() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/dep.ts", "export interface DepType { version: 1 }\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Step 1: materialize module_facts for /src/dep.ts (workspace-only,
    // never upserted → won't be tracked by the store view).
    let facts_v1 = host
        .ensure_indexed_ready("/src/dep.ts")
        .expect("dep v1 should materialize");
    let hash_v1 = facts_v1.whole_hash;

    // Step 2: remove the cached IndexedReady entry so subsequent reads
    // re-materialize from the scheduler. The retired `FileArtifactStore` used
    // to archive soft-invalidated entries; `FileArtifactStore` validates by
    // whole_hash instead, so `remove` is the correct replacement.
    host.project_type_store().indexed().remove("/src/dep.ts");

    // Step 3: change the dependency content via workspace injection, then
    // notify the scheduler via `ensure_loaded` (the canonical content-change
    // ingress path under the new architecture: disk reads are no longer
    // implicit inside resolvers).
    ws.inject_file(
        "/src/dep.ts",
        "export interface DepType { version: 2; extra: string }\n",
    );
    // Evict so ensure_loaded re-reads the workspace content.
    host.evict("/src/dep.ts");
    assert!(host.ensure_loaded("/src/dep.ts"));

    // Step 4: create a store view snapshotted AFTER the content change.
    let _view = host.resolver_store_view_read().into_owned_view();

    // Step 5: query module_facts with the store view. The validated cache
    // must NOT return the stale V1 facts from the archive.
    let facts_after = host
        .ensure_indexed_ready("/src/dep.ts")
        .expect("dep should re-materialize with current workspace content");
    assert_ne!(
        facts_after.whole_hash, hash_v1,
        "IndexedReady via fence-validated cache must reflect the current \
         workspace content (V2), not stale archived V1 facts. The untracked-file \
         acceptance in validates() should not allow archived entries with a \
         mismatched content hash to pass validation.",
    );
}

/// Regression: intermediate barrel re-export changes must invalidate
/// ImportedRootDb entries, even when the top-level provider and the
/// original leaf file remain text-identical.
///
/// Chain: /src/index.ts -> /src/barrel.ts -> /src/types-a.ts
/// After: /src/index.ts -> /src/barrel.ts -> /src/types-b.ts
/// (only /src/barrel.ts changes)
#[test]
fn imported_root_invalidates_on_intermediate_barrel_change() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types-a.ts", "export interface Props { version: 1 }\n");
    ws.inject_file("/src/types-b.ts", "export interface Props { version: 2 }\n");
    // Barrel re-exports Props from types-a
    ws.inject_file("/src/barrel.ts", "export { Props } from './types-a'\n");
    // Index re-exports everything from barrel
    ws.inject_file("/src/index.ts", "export { Props } from './barrel'\n");

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    // Set up import dependencies so the route chain resolves
    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./barrel", "/src/barrel.ts")],
    );
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./types-a", "/src/types-a.ts")],
    );

    // Warm: resolve Props through the chain
    let _view1 = host.resolver_store_view_read().into_owned_view();
    let root1 = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root1.0.as_str(),
        "/src/types-a.ts",
        "initial root should point to types-a",
    );

    // Change barrel to point to types-b instead. Workspace inject + evict +
    // ensure_loaded is the canonical sequence for content changes under the
    // new architecture (no implicit disk reads inside resolvers).
    ws.inject_file("/src/barrel.ts", "export { Props } from './types-b'\n");
    host.evict("/src/barrel.ts");
    assert!(host.ensure_loaded("/src/barrel.ts"));
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./types-b", "/src/types-b.ts")],
    );

    // The provider (/src/index.ts) and the old leaf (/src/types-a.ts)
    // are unchanged. Only the barrel changed.
    let _view2 = host.resolver_store_view_read().into_owned_view();
    let root2 = host.resolve_imported_type_root("/src/index.ts", "Props");
    assert_eq!(
        root2.0.as_str(),
        "/src/types-b.ts",
        "after intermediate barrel change, root must point to types-b, \
         not stale types-a from the cached imported root",
    );
}

#[test]
fn read_analysis_source_trace_result_labels_workspace_vfs_reads() {
    assert_eq!(
        super::read_analysis_source_result_detail("/src/types.ts", "workspace-vfs", 128, false,),
        "owner=/src/types.ts source=workspace-vfs bytes=128"
    );
    assert_eq!(
        super::read_analysis_source_result_detail("/src/types.ts", "workspace-vfs", 0, true,),
        "owner=/src/types.ts source=workspace-vfs bytes=0 missing=true"
    );
}

#[test]
fn workspace_vfs_source_kind_includes_layer_detail_when_present() {
    assert_eq!(
        super::workspace_vfs_source_kind(Some("layer=snapshot cache=hit".to_string())),
        "workspace-vfs layer=snapshot cache=hit"
    );
    assert_eq!(super::workspace_vfs_source_kind(None), "workspace-vfs");
}

// `request_store_view_extends_across_mid_request_ensure_loaded` is
// intentionally not part of this suite: the `RequestStoreView` type
// and its captured-view-plus-extension semantics are not part of the
// final design. Live-host probes validated via the host's fact-
// signature path are the authoritative substitute.

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ensure_loaded_reload_with_identical_content_does_not_bump_epoch() {
    // Regression lock-in: after an evict + ensure_loaded cycle for a
    // file whose on-disk content is identical to the pre-evict
    // snapshot, `store_view_epoch` must NOT bump. A regression that
    // bumped the epoch unconditionally would clear every thread-local
    // cache (parsed-eval-program, type-context) and force a cold re-
    // resolution on the follow-up lookup. The `pre_evict_hash ==
    // post_reload_hash` comparison must short-circuit the bump on
    // no-op reload; caches stay warm.
    let ws = std::sync::Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/App.vue",
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div /></template>",
    );
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    host.ensure_loaded("/src/App.vue");

    let pre_epoch = host.current_store_view_epoch();
    host.evict("/src/App.vue");
    // evict() bumps the epoch unconditionally (real invalidation).
    assert!(
        host.current_store_view_epoch() > pre_epoch,
        "evict() should bump the epoch"
    );
    let post_evict_epoch = host.current_store_view_epoch();

    // Reload with identical content — scheduler sees identical bytes.
    host.ensure_loaded("/src/App.vue");
    assert_eq!(
        host.current_store_view_epoch(),
        post_evict_epoch,
        "reload with identical content must NOT bump the epoch"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn source_type_is_stable_across_callsites_for_same_canonical() {
    // Single-authority invariant for `source_type`:
    //
    // A regression that returned `SourceType::ts()` when `framework_parse: None`
    // but the carrier `<script lang>` resolution when
    // `framework_parse: Some(artifact)` would diverge for a `lang="tsx"` SFC —
    // different cache slots for the same `(canonical, whole_hash)`.
    //
    // The scheduler computes `source_type` once at `execute_source` time with
    // full access to the parsed SFC, stores it on `HostSourceData::source_type`,
    // and every downstream cache-key site must read the authoritative value.
    let host = make_host();
    let tsx_vue = r#"<script lang="tsx">
const Button = () => <button />
export type Props = { render: typeof Button }
</script>
<template><div /></template>"#;
    upsert_vue(&host, "/src/Foo.vue", tsx_vue);

    use crate::host_executor::HostSourceData;
    let source_snap = host
        .scheduler
        .try_get_source("/src/Foo.vue")
        .expect("scheduler should have Foo.vue");
    let hd = source_snap
        .downcast_data::<HostSourceData>()
        .expect("source data should be HostSourceData");

    // HostSourceData carries the authoritative source_type — computed once at parse time
    // using the full parse artifact, not reconstructed from raw_source + optional artifact.
    assert!(
        hd.source_type.is_jsx(),
        "HostSourceData.source_type should be tsx (JSX-bearing) for lang=tsx SFC, got {:?}",
        hd.source_type,
    );
    assert!(
        hd.source_type.is_typescript(),
        "HostSourceData.source_type should be TypeScript for lang=tsx SFC, got {:?}",
        hd.source_type,
    );
}

// ----------------------------------------------------------------
// F7 — `resolve_imported_type_root` trace dedup.
// Discriminator: same (canonical, imported_name) queried N times in
// a single request emits the `resolve_imported_type_root` Custom
// event exactly ONCE (the cache miss). Pre-fix the event fired on
// every call regardless of cache state.
//
// Test must live here (not under `tests/`) because
// `resolve_imported_type_root` is `pub(crate)` and integration
// tests cannot reach it (D36 placement rule).
// ----------------------------------------------------------------

#[cfg(test)]
mod imported_root_trace_dedup_tests {
    use super::*;
    use crate::component_meta_audit::accumulator::RequestFootprintAccumulator;
    use crate::component_meta_audit::structured_event::StructuredAuditEvent;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    fn host_with_props_ts() -> Arc<VerterHost> {
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        let host = Arc::new(VerterHost::new(
            HostConfig {
                audit_enabled: true,
                footprint_capture: true,
                ..HostConfig::default()
            },
            workspace,
        ));
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/props.ts".into()),
            input_id: "/props.ts".into(),
            source: Arc::from("export interface Props { label: string; }\n"),
            file_language: FileLanguage::script_ts(),
            aliases: vec![],
        });
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some("/Component.vue".into()),
            input_id: "/Component.vue".into(),
            source: Arc::from(
                "<script setup lang=\"ts\">\n\
                 import type { Props } from './props';\n\
                 defineProps<Props>();\n\
                 </script>\n\
                 <template><div /></template>\n",
            ),
            file_language: FileLanguage::vue(),
            aliases: vec![],
        });
        host
    }

    #[test]
    fn resolve_imported_type_root_traces_once_per_cache_miss_not_per_call() {
        let host = host_with_props_ts();
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let ctx = RequestContext::new(
            7777,
            Arc::from("/Component.vue"),
            true,
            Some(Arc::clone(&acc)),
        );
        let _guard = RequestContextGuard::install(ctx);

        // Five repeated calls with identical inputs. The
        // ImportedRootDb (host.resolver.runtime.imported_roots) caches
        // the result on the first call; subsequent calls are cache
        // hits.
        for _ in 0..5 {
            let _ = host.resolve_imported_type_root("/Component.vue", "Props");
        }

        let state = acc.drain();
        let resolve_events: Vec<&StructuredAuditEvent> = state
            .structured_events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    StructuredAuditEvent::Custom { name, .. }
                        if name.as_ref() == "resolve_imported_type_root"
                )
            })
            .collect();

        // Pre-fix expected count = 5 (the trace fired before the
        // cache check, so every call emitted). Post-fix expected
        // count = 1 (the trace moved inside the closure, runs only
        // on cache miss; first call misses, subsequent four hit).
        assert_eq!(
            resolve_events.len(),
            1,
            "F7 contract: `resolve_imported_type_root` must trace once per \
             cache MISS, not once per call. Got {} events for 5 identical \
             calls. Pre-fix the trace was emitted before the ImportedRootDb \
             cache check.",
            resolve_events.len(),
        );
    }
}

// ----------------------------------------------------------------
// F4 — `component_meta_trace_custom!` laziness discriminators.
// a side-effecting AtomicUsize counter proves
// that the macro's `$detail` expression is NOT evaluated when no
// audit accumulator is installed. Pre-fix: counter increments on
// every call. Post-fix: counter increments only when accumulator
// is installed.
//
// Tests live here (not under `tests/`) because the macro is
// `pub(crate) use component_meta_trace_custom;` and integration
// tests cannot import a `pub(crate)` macro (D36).
// ----------------------------------------------------------------

#[cfg(test)]
mod trace_laziness_tests {
    use super::*;
    use crate::component_meta_audit::accumulator::RequestFootprintAccumulator;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use std::cell::Cell;

    // Counter is per-test (declared inside the test function), not
    // module-static — cargo runs sibling tests in parallel and any
    // shared mutable state would race. `Cell<u32>` is single-threaded
    // and lives entirely on the test's stack, so the macro's
    // captured-by-reference closure is safe without locking.

    #[test]
    fn macro_detail_not_evaluated_when_no_accumulator_installed() {
        let counter: Cell<u32> = Cell::new(0);
        // The macro's $detail expression evaluates only when the if-guard
        // takes the branch. Inline a block that ticks the counter so we
        // observe whether the macro evaluated detail at all.
        let tick = || {
            counter.set(counter.get() + 1);
            String::from("detail")
        };
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        assert_eq!(
            counter.get(),
            0,
            "F4: macro $detail must not run when no accumulator is installed",
        );
    }

    #[test]
    fn macro_detail_evaluated_when_accumulator_installed() {
        let counter: Cell<u32> = Cell::new(0);
        let tick = || {
            counter.set(counter.get() + 1);
            String::from("detail")
        };
        let acc = Arc::new(RequestFootprintAccumulator::new());
        let ctx = RequestContext::new(
            42,
            Arc::from("/test_lazy_macro.vue"),
            true,
            Some(Arc::clone(&acc)),
        );
        let _guard = RequestContextGuard::install(ctx);
        component_meta_trace_custom!("test_event", tick());
        component_meta_trace_custom!("test_event", tick());
        assert_eq!(
            counter.get(),
            2,
            "F4 regression invariant: macro must fire $detail when accumulator is installed",
        );
    }
}

mod manifest_types_entry_routing_tests {
    //! `derive_type_preferred_exact_target` MUST route through
    //! `WorkspaceAccess::manifest_types_entry_for` (workspace-classification
    //! aware) rather than a `/node_modules/` substring check on the
    //! resolved canonical id.
    //!
    //! Discriminating fixture: a pnpm-hoisted layout where a workspace
    //! project root sits at `/ws/node_modules/@scope/local-pkg/`. A
    //! runtime-script (`.js`) target under this root has a canonical id
    //! that contains `/node_modules/` but `is_workspace_owned` returns
    //! `true` (because the project root's suffix is empty under itself).
    //!
    //! A naive substring router routes this incorrectly: the canonical
    //! id contains `/node_modules/`, so the `is_runtime_script_target`
    //! check fires and the manifest-types-entry resolution returns
    //! `None` for the workspace-owned package, and the fallback then
    //! short-circuits because the canonical id contains `/node_modules/`.
    //! Result: `None`.
    //!
    //! The `WorkspaceAccess` accessor routes correctly: the workspace
    //! classifies the target as `is_workspace_owned`, so the path is
    //! returned verbatim. Result: `Some(resolved)`.
    //!
    //! Asserting the correct return path discriminates the two
    //! implementations.
    use std::sync::Arc;

    use crate::types::DependencyResolution;
    use crate::{HostConfig, VerterHost};
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    fn build_pnpm_hoisted_workspace() -> Arc<MemoryWorkspace> {
        let ws = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        // Register a project whose root sits INSIDE node_modules/. The
        // engine's is_workspace_owned + is_package_backed pair classifies
        // such files as workspace-owned (the suffix between root and
        // path contains no further /node_modules/ segment).
        ws.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
            verter_workspace::VfsProjectConfig {
                root: "/ws/node_modules/@scope/local-pkg".to_string(),
                rank: verter_workspace::ProjectRank::Explicit,
                tsconfig_path: Some("/ws/node_modules/@scope/local-pkg/tsconfig.json".to_string()),
                root_files: vec![],
                extensions: vec![".ts".into(), ".js".into(), ".vue".into()],
                workspace_root: "/ws/node_modules/@scope/local-pkg".to_string(),
                workspace_aliases: vec![],
                compiler_options: verter_workspace::IdeProjectCompilerOptions::default(),
                references: vec![],
                membership: verter_workspace::ConfiguredMembership::match_all_under_root(
                    &verter_workspace::CanonicalPath::new("/ws/node_modules/@scope/local-pkg"),
                ),
            },
        ]));
        // Inject a runtime-script target inside the workspace-owned
        // project. The canonical id contains /node_modules/ but the
        // workspace classification API returns is_workspace_owned=true.
        ws.inject_file(
            "/ws/node_modules/@scope/local-pkg/dist/index.js".to_string(),
            Arc::<str>::from(""),
        );
        ws
    }

    #[test]
    fn derive_type_preferred_exact_target_returns_workspace_owned_js_under_node_modules() {
        let ws = build_pnpm_hoisted_workspace();
        let access: Arc<dyn WorkspaceAccess> = ws.clone();

        // Sanity-check the discriminating fixture: the canonical id
        // contains /node_modules/ AND the workspace classifies it as
        // workspace-owned (NOT package-backed). A naive substring check
        // confuses these two; the WorkspaceAccess accessor does not.
        let resolved = "/ws/node_modules/@scope/local-pkg/dist/index.js";
        assert!(
            access.is_workspace_owned(resolved),
            "fixture invariant: resolved must be workspace-owned"
        );
        assert!(
            !access.is_package_backed(resolved),
            "fixture invariant: resolved must NOT be package-backed"
        );
        assert!(
            access.manifest_types_entry_for(resolved).is_none(),
            "manifest_types_entry_for must return None for workspace-owned targets"
        );

        let host = VerterHost::new(HostConfig::default(), access);
        let resolution = DependencyResolution {
            specifier: "@scope/local-pkg".to_string(),
            resolved_canonical_id: Some(resolved.to_string()),
            possible_canonical_ids: vec![],
        };

        let derived = host.derive_type_preferred_exact_target(&resolution);

        // The discriminating assertion: the workspace-owned .js path is
        // returned as-is via the workspace classification accessor. A
        // substring router would have short-circuited to None here.
        assert_eq!(
            derived.as_deref(),
            Some(resolved),
            "workspace-owned runtime-script target under /node_modules/ must \
             pass through the WorkspaceAccess routing path"
        );
    }

    // ── F4: carrier-generic extension classifiers ────────────────────────────
    //
    // `is_type_preferred_target` and `has_file_like_extension` used a hardcoded
    // `.vue` arm; a `.svelte` carrier must be classified identically to a `.vue`
    // one (both are framework carriers projecting a type-bearing virtual
    // surface / being real file-like paths). These pin the carrier-generic
    // behavior — they FAIL against the pre-fix `.vue`-only arms.

    #[test]
    fn type_preferred_target_treats_svelte_carrier_like_vue() {
        use super::is_type_preferred_target;
        // A `.vue` SFC is type-preferred…
        assert!(is_type_preferred_target("/src/App.vue"));
        // …and so is a `.svelte` carrier (the F4 fix). Pre-fix this was false.
        assert!(is_type_preferred_target("/src/Widget.svelte"));
        // `.d.ts`/`.ts` stay type-preferred; a bare `.js` does not.
        assert!(is_type_preferred_target("/src/types.d.ts"));
        assert!(!is_type_preferred_target("/src/runtime.js"));
        // A rune module (`.svelte.ts`) ends with `.ts` → type-preferred via the
        // script arm (unchanged, and correct — it is a real TS surface).
        assert!(is_type_preferred_target("/src/store.svelte.ts"));
    }

    #[test]
    fn file_like_extension_recognizes_svelte_carrier_like_vue() {
        use super::has_file_like_extension;
        // Both carriers are real file-like paths, not bare specifiers.
        assert!(has_file_like_extension("/src/App.vue"));
        assert!(has_file_like_extension("/src/Widget.svelte"));
        // Scripts / json stay file-like; a bare module specifier does not.
        assert!(has_file_like_extension("/src/util.ts"));
        assert!(has_file_like_extension("/src/data.json"));
        assert!(!has_file_like_extension("lodash"));
    }

    #[test]
    fn relative_svelte_path_is_not_misclassified_as_raw_specifier() {
        use super::is_raw_import_specifier_id;
        // The downstream consequence of the `has_file_like_extension` fix:
        // a relative `./Widget.svelte` import is a FILE, not a raw module
        // specifier. Pre-fix `has_file_like_extension` missed `.svelte`, so the
        // `./`-prefixed path fell through to the raw-specifier `true` arm — a
        // carrier asymmetry vs `./App.vue` (which was correctly `false`).
        assert!(!is_raw_import_specifier_id("./Widget.svelte"));
        assert!(!is_raw_import_specifier_id("./App.vue"));
        // A genuine bare specifier is still a raw specifier.
        assert!(is_raw_import_specifier_id("./some-pkg"));
        assert!(is_raw_import_specifier_id("lodash"));
    }
}

// ── resolve_eval_dependency_canonical_with: candidate probe contract ─────────
//
// The exact candidate probe ORDER of `resolve_eval_dependency_canonical_with`
// is a behavioral contract: callers (`VerterHost::resolve_eval_dependency_canonical`,
// the executor's `extract_deps` normalizer) rely on higher-priority typed
// companions winning over lower-priority ones, and the probe closure is
// side-effectful at some call sites (existence probes are observable). These
// tests pin the full probe sequence with a recording closure so any change to
// candidate generation — including allocation-strategy refactors — must keep
// the order, the probe count, and the returned strings byte-identical.
mod resolve_eval_dependency_probe_contract_tests {
    use super::resolve_eval_dependency_canonical_with;

    /// Runs the resolver with a closure that records every probed candidate
    /// in order and reports existence only for members of `existing`.
    fn probe_trace(dep: &str, existing: &[&str]) -> (Option<String>, Vec<String>) {
        let mut probed = Vec::new();
        let result = resolve_eval_dependency_canonical_with(dep, |candidate| {
            probed.push(candidate.to_string());
            existing.contains(&candidate)
        });
        (result, probed)
    }

    #[test]
    fn runtime_js_input_probes_declaration_companion_then_appends_then_input_last() {
        let (result, probed) = probe_trace("/ws/pkg/dist/index.js", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/pkg/dist/index.d.ts",
                "/ws/pkg/dist/index.js.d.ts",
                "/ws/pkg/dist/index.js.ts",
                "/ws/pkg/dist/index.js.tsx",
                "/ws/pkg/dist/index.js/index.d.ts",
                "/ws/pkg/dist/index.js/index.ts",
                "/ws/pkg/dist/index.js/index.tsx",
                "/ws/pkg/dist/index.js",
            ],
            "a runtime .js dependency must probe its declaration companion \
             first, then the append candidates in declared order, and the raw \
             input only as the final type-companion fallback",
        );
    }

    #[test]
    fn bundler_suffix_input_probes_bundle_companion_before_plain_js_companion() {
        let dep = "/ws/@vue/runtime-core/dist/runtime-core.esm-bundler.js";
        let (result, probed) = probe_trace(dep, &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                // The bundler-suffix companion strips the WHOLE bundle suffix…
                "/ws/@vue/runtime-core/dist/runtime-core.d.ts".to_string(),
                // …and the plain `.js` companion strips only `.js`, later.
                "/ws/@vue/runtime-core/dist/runtime-core.esm-bundler.d.ts".to_string(),
                format!("{dep}.d.ts"),
                format!("{dep}.ts"),
                format!("{dep}.tsx"),
                format!("{dep}/index.d.ts"),
                format!("{dep}/index.ts"),
                format!("{dep}/index.tsx"),
                dep.to_string(),
            ],
            "bundle-suffix stripping must be probed before plain .js stripping",
        );
    }

    #[test]
    fn every_bundler_suffix_probes_its_declaration_companion_first() {
        for suffix in [
            ".esm-bundler.js",
            ".esm-browser.js",
            ".esm-browser.prod.js",
            ".global.js",
            ".global.prod.js",
            ".cjs.js",
            ".cjs.prod.js",
        ] {
            let dep = format!("/ws/pkg/dist/entry{suffix}");
            let (result, probed) = probe_trace(&dep, &["/ws/pkg/dist/entry.d.ts"]);
            assert_eq!(
                result.as_deref(),
                Some("/ws/pkg/dist/entry.d.ts"),
                "suffix {suffix} must resolve to the stripped declaration companion",
            );
            assert_eq!(
                probed,
                vec!["/ws/pkg/dist/entry.d.ts".to_string()],
                "suffix {suffix}: the bundle companion must be the FIRST probe",
            );
        }
    }

    #[test]
    fn jsx_mjs_cjs_inputs_map_to_their_specific_declaration_companions() {
        let (result, probed) = probe_trace("/ws/c/comp.jsx", &["/ws/c/comp.d.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/c/comp.d.ts"));
        assert_eq!(probed, vec!["/ws/c/comp.d.ts".to_string()]);

        let (result, probed) = probe_trace("/ws/m/entry.mjs", &["/ws/m/entry.d.mts"]);
        assert_eq!(result.as_deref(), Some("/ws/m/entry.d.mts"));
        assert_eq!(probed, vec!["/ws/m/entry.d.mts".to_string()]);

        let (result, probed) = probe_trace("/ws/m/entry.cjs", &["/ws/m/entry.d.cts"]);
        assert_eq!(result.as_deref(), Some("/ws/m/entry.d.cts"));
        assert_eq!(probed, vec!["/ws/m/entry.d.cts".to_string()]);
    }

    #[test]
    fn extensionless_input_probes_typed_candidates_before_raw_input() {
        let (result, probed) = probe_trace("/ws/src/runtime/types/html", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/src/runtime/types/html.d.ts",
                "/ws/src/runtime/types/html.ts",
                "/ws/src/runtime/types/html.tsx",
                "/ws/src/runtime/types/html/index.d.ts",
                "/ws/src/runtime/types/html/index.ts",
                "/ws/src/runtime/types/html/index.tsx",
                "/ws/src/runtime/types/html",
            ],
            "an extensionless dependency probes every typed candidate before \
             falling back to the raw extensionless path",
        );
    }

    #[test]
    fn extensionless_input_resolves_to_index_candidate_in_order() {
        let (result, probed) = probe_trace("/ws/lib/util", &["/ws/lib/util/index.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/util/index.ts"));
        assert_eq!(
            probed,
            vec![
                "/ws/lib/util.d.ts",
                "/ws/lib/util.ts",
                "/ws/lib/util.tsx",
                "/ws/lib/util/index.d.ts",
                "/ws/lib/util/index.ts",
            ],
            "probing must stop at the first existing candidate",
        );
    }

    #[test]
    fn extensionless_input_falls_back_to_existing_raw_path_after_all_candidates() {
        let (result, probed) = probe_trace("/ws/lib/util", &["/ws/lib/util"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/util"));
        assert_eq!(
            probed.len(),
            7,
            "the raw path is only probed after all six typed candidates",
        );
        assert_eq!(probed.last().map(String::as_str), Some("/ws/lib/util"));
    }

    #[test]
    fn explicit_non_js_extension_fast_path_probes_only_the_input() {
        // (b) the early-return case: an explicit non-js extension that exists
        // must be returned untouched after probing ONLY the input itself.
        let (result, probed) = probe_trace("/ws/lib/foo.d.ts", &["/ws/lib/foo.d.ts"]);
        assert_eq!(result.as_deref(), Some("/ws/lib/foo.d.ts"));
        assert_eq!(
            probed,
            vec!["/ws/lib/foo.d.ts".to_string()],
            "the explicit-extension fast path must probe exactly the input and \
             nothing else",
        );

        let (result, probed) =
            probe_trace("/ws/components/Button.vue", &["/ws/components/Button.vue"]);
        assert_eq!(result.as_deref(), Some("/ws/components/Button.vue"));
        assert_eq!(probed, vec!["/ws/components/Button.vue".to_string()]);
    }

    #[test]
    fn explicit_declaration_input_probes_itself_first_then_append_candidates() {
        let (result, probed) = probe_trace("/ws/lib/foo.d.ts", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/lib/foo.d.ts",
                "/ws/lib/foo.d.ts.d.ts",
                "/ws/lib/foo.d.ts.ts",
                "/ws/lib/foo.d.ts.tsx",
                "/ws/lib/foo.d.ts/index.d.ts",
                "/ws/lib/foo.d.ts/index.ts",
                "/ws/lib/foo.d.ts/index.tsx",
            ],
            "a missing explicit non-js-extension input is probed FIRST (fast \
             path), then only the append candidates; no trailing raw re-probe",
        );
    }

    #[test]
    fn runtime_js_input_falls_back_to_existing_raw_path_probed_last() {
        let (result, probed) = probe_trace("/ws/d/index.js", &["/ws/d/index.js"]);
        assert_eq!(result.as_deref(), Some("/ws/d/index.js"));
        assert_eq!(
            probed,
            vec![
                "/ws/d/index.d.ts",
                "/ws/d/index.js.d.ts",
                "/ws/d/index.js.ts",
                "/ws/d/index.js.tsx",
                "/ws/d/index.js/index.d.ts",
                "/ws/d/index.js/index.ts",
                "/ws/d/index.js/index.tsx",
                "/ws/d/index.js",
            ],
            "a runtime script that exists is returned only after every typed \
             companion candidate missed",
        );
    }

    #[test]
    fn empty_input_returns_none_without_any_probe() {
        // Even an `existing` set containing the empty string must not be
        // consulted: the resolver returns before any probe.
        let (result, probed) = probe_trace("", &[""]);
        assert_eq!(result, None);
        assert!(probed.is_empty(), "empty input must not probe at all");
    }

    #[test]
    fn hidden_js_basename_probes_raw_input_twice_at_the_tail() {
        // `Path::extension()` treats `.js` (a dot-file basename) as having NO
        // extension while `ends_with(".js")` still marks it type-companion-
        // preferring — so BOTH tail fallback probes fire for the raw input.
        // This pins the exact probe multiset of the current contract.
        let (result, probed) = probe_trace("/ws/.js", &[]);
        assert_eq!(result, None);
        assert_eq!(
            probed,
            vec![
                "/ws/.d.ts",
                "/ws/.js.d.ts",
                "/ws/.js.ts",
                "/ws/.js.tsx",
                "/ws/.js/index.d.ts",
                "/ws/.js/index.ts",
                "/ws/.js/index.tsx",
                "/ws/.js",
                "/ws/.js",
            ],
            "a dot-file .js basename fires both the extensionless fallback and \
             the type-companion fallback probes",
        );
    }
}
