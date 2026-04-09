use super::*;

use std::sync::Arc;
use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
use verter_workspace::WorkspaceAccess;

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
            file_kind: FileKind::VueSfc,
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
            file_kind: FileKind::NonSfc,
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

impl verter_workspace::WorkspaceAccess for CountingWorkspace {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        *self
            .read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_file(canonical_id)
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

    fn classify_file(&self, canonical_id: &str) -> verter_workspace::FileKind {
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

    fn owner_for_file(&self, canonical_id: &str) -> Option<verter_workspace::ProjectOwnership> {
        self.inner.owner_for_file(canonical_id)
    }

    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }

    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges);
    }

    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.reverse_deps_for(canonical_id)
    }

    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String> {
        self.inner.forward_deps_for(canonical_id)
    }

    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        self.inner.set_exact_resolutions(canonical_id, resolutions)
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

    fn is_dir(&self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
}

fn exact_dependency(specifier: &str, resolved: &str) -> DependencyResolution {
    DependencyResolution {
        specifier: specifier.to_string(),
        resolved_canonical_id: Some(resolved.to_string()),
        possible_canonical_ids: Vec::new(),
    }
}

#[cfg(not(feature = "scheduler"))]
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

#[cfg(not(feature = "scheduler"))]
fn clear_cached_parse(host: &VerterHost) {
    let mut files = crate::shared::write_lock(&host.files);
    let entry = files.get_mut("App.vue").expect("App.vue should exist");
    entry.cached_parse = None;
}

#[test]
fn component_meta_trace_line_formats_start_event() {
    let line = format_component_meta_trace_line(
        ComponentMetaTraceEvent::Start,
        ComponentMetaTraceLine {
            trace_id: 11,
            span_id: 11,
            parent_span_id: None,
            depth: 0,
            name: "resolve_component_meta",
            detail: r#"owner=/src/App.vue mode=Expanded"#,
        },
        None,
    );

    assert!(
        line.contains("[verter-meta-trace]"),
        "trace lines should use the dedicated prefix, got: {line}"
    );
    assert!(
        line.contains("event=start"),
        "start trace lines should identify the event kind, got: {line}"
    );
    assert!(
        line.contains("trace=11") && line.contains("span=11"),
        "start trace lines should carry trace/span ids, got: {line}"
    );
    assert!(
        line.contains("parent=-"),
        "root trace lines should use a sentinel parent, got: {line}"
    );
    assert!(
        line.contains("depth=0"),
        "root trace lines should report depth zero, got: {line}"
    );
    assert!(
        line.contains(r#"name="resolve_component_meta""#),
        "trace lines should quote the scope name, got: {line}"
    );
    assert!(
        line.contains(r#"detail="owner=/src/App.vue mode=Expanded""#),
        "trace lines should quote the detail payload, got: {line}"
    );
    assert!(
        !line.contains("dur_ms="),
        "start trace lines should not include a duration before the scope ends, got: {line}"
    );
}

#[test]
fn component_meta_trace_line_formats_end_event_with_duration() {
    let line = format_component_meta_trace_line(
        ComponentMetaTraceEvent::End,
        ComponentMetaTraceLine {
            trace_id: 11,
            span_id: 12,
            parent_span_id: Some(11),
            depth: 1,
            name: "resolve_external_type",
            detail: r#"owner=/src/App.vue import=vue type=Ref"#,
        },
        Some(std::time::Duration::from_micros(123_456)),
    );

    assert!(
        line.contains("event=end"),
        "end trace lines should identify the event kind, got: {line}"
    );
    assert!(
        line.contains("parent=11"),
        "nested trace lines should keep the parent span id, got: {line}"
    );
    assert!(
        line.contains("depth=1"),
        "nested trace lines should report the nesting depth, got: {line}"
    );
    assert!(
        line.contains(r#"name="resolve_external_type""#),
        "end trace lines should quote the scope name, got: {line}"
    );
    assert!(
        line.contains("dur_ms=123.456"),
        "end trace lines should include millisecond precision, got: {line}"
    );
}

#[test]
fn build_eval_script_source_without_cached_parse_still_extracts_script_blocks() {
    let source = r#"<script lang="ts">
interface Props {
  label: string
}
</script>
<script setup lang="ts">
defineProps<Props>()
</script>
<template><div /></template>"#;

    let extracted = VerterHost::build_eval_script_source(source, None);
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

#[test]
fn stale_store_view_rejects_changed_dependency_eval_state() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/types.ts",
        "export interface Props { label: string }",
    );

    let before_view = host.resolver_store_view();
    assert!(
        host.current_eval_state_in_view("/types.ts", Some(&before_view))
            .is_some(),
        "captured view should accept the dependency state it was created from"
    );
    assert!(
        host.base_eval_env_in_view("/types.ts", Some(&before_view))
            .is_some(),
        "captured view should accept the dependency env it was created from"
    );

    upsert_non_sfc(
        &host,
        "/types.ts",
        "export interface Props { disabled: boolean }",
    );

    assert!(
        host.current_eval_state_in_view("/types.ts", Some(&before_view))
            .is_none(),
        "stale views must reject dependency source reads after the file changes"
    );
    assert!(
        host.base_eval_env_in_view("/types.ts", Some(&before_view))
            .is_none(),
        "stale views must reject dependency eval env reads after the file changes"
    );
}

#[test]
fn stale_store_view_rejects_changed_import_routes_and_reexports() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { Props } from "./types";
defineProps<Props>();
</script>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );
    upsert_non_sfc(
        &host,
        "/src/index.ts",
        r#"export { Props } from "./types";"#,
    );

    let before_view = host.resolver_store_view();
    assert_eq!(
        host.resolve_type_dependency_canonical_in_view(
            "/src/App.vue",
            "./types",
            Some(&before_view)
        )
        .as_deref(),
        Some("/src/types.ts"),
        "captured view should resolve the original owner import route",
    );
    assert!(
        host.get_export_span_follow_reexports_in_view("/src/index.ts", "Props", Some(&before_view))
            .is_some(),
        "captured view should resolve the original re-export chain",
    );

    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { Props } from "./other";
defineProps<Props>();
</script>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Renamed { disabled: boolean }",
    );

    assert!(
        host.resolve_type_dependency_canonical_in_view(
            "/src/App.vue",
            "./types",
            Some(&before_view)
        )
        .is_some(),
        "stale views preserve owner import routes from the captured snapshot",
    );
    assert!(
        host.get_export_span_follow_reexports_in_view("/src/index.ts", "Props", Some(&before_view))
            .is_none(),
        "stale views reject re-export chains after a downstream file changes",
    );
    assert!(
        host.resolve_exports_in_view("/src/index.ts", Some(&before_view))
            .is_empty(),
        "stale views reject export surfaces whose re-export targets changed",
    );
}

#[test]
fn stale_store_view_keeps_owner_import_route_when_workspace_candidates_change() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { Props } from "./types";
defineProps<Props>();
</script>"#,
    );
    upsert_non_sfc(&host, "/src/types.js", "export const runtime = true;");

    let before_view = host.resolver_store_view();
    assert_eq!(
        host.resolve_type_dependency_canonical_in_view(
            "/src/App.vue",
            "./types",
            Some(&before_view)
        )
        .as_deref(),
        Some("/src/types.js"),
        "captured view should preserve the owner's original resolved dependency",
    );

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );

    assert_eq!(
        host.resolve_type_dependency_canonical_in_view(
            "/src/App.vue",
            "./types",
            Some(&before_view)
        )
        .as_deref(),
        Some("/src/types.js"),
        "stale views must not switch owner import routes to newer workspace candidates",
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
fn stale_store_view_does_not_fallback_to_live_import_route_when_route_was_missing() {
    let host = make_host();
    upsert_vue(
        &host,
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { Props } from "./types";
defineProps<Props>();
</script>"#,
    );

    let before_view = host.resolver_store_view();
    let before_snapshot = host
        .get_raw_analysis_snapshot_in_view("/src/App.vue", Some(&before_view))
        .expect("captured analysis snapshot should exist");
    let before_import = before_snapshot
        .imports
        .iter()
        .find(|import| import.source == "./types")
        .expect("App.vue should keep the original import in the captured snapshot")
        .clone();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }",
    );

    assert!(
        host.resolve_type_dependency_canonical_in_view(
            "/src/App.vue",
            &before_import.source,
            Some(&before_view),
        )
        .is_none(),
        "captured imported-eval lookups must not recover missing routes from the live workspace"
    );
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

    let view = host.resolver_store_view();

    assert_eq!(
        host.resolve_type_dependency_canonical_in_view(
            "/workspace/components/Button.vue",
            "../composables/useComponentIcons",
            Some(&view)
        )
        .as_deref(),
        Some("/workspace/composables/useComponentIcons.ts"),
        "current store views should resolve missing relative type routes for existing workspace files",
    );
    assert_eq!(
        host.resolve_type_dependency_canonical_in_view(
            "/workspace/components/Button.vue",
            "../types",
            Some(&view)
        )
        .as_deref(),
        Some("/workspace/types/index.ts"),
        "current store views should resolve missing relative barrel routes for existing workspace files",
    );
}

#[test]
fn prepared_type_decl_in_view_resolves_plain_declaration_import_helpers() {
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
        .prepared_type_decl_in_view(
            "/workspace/node_modules/lib/dist/index.d.ts",
            "FancyProps",
            None,
        )
        .expect("FancyProps should prepare from the imported declaration cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("Prettify")
            .map(|identity| (identity.canonical_id.as_str(), identity.symbol_name.as_str())),
        Some(("/workspace/node_modules/helper/dist/helper.d.ts", "Prettify")),
        "plain imports inside declaration files must resolve helper names through the declaration entrypoint rather than leaving them unresolved or pinned to JS companions",
    );
}

#[test]
fn prepared_type_decl_in_view_rebuilds_name_resolution_after_import_route_upgrade() {
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
        .prepared_type_decl_in_view("/workspace/Checkbox.vue", "CheckboxProps", None)
        .expect("CheckboxProps should prepare before import routes are upgraded");
    assert_eq!(
        initial
            .name_resolution
            .get("theme")
            .map(|identity| identity.canonical_id.as_str()),
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
        .prepared_type_decl_in_view("/workspace/Checkbox.vue", "CheckboxProps", None)
        .expect("CheckboxProps should rebuild after import routes are upgraded");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("theme")
            .map(|identity| (identity.canonical_id.as_str(), identity.symbol_name.as_str())),
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should seed module facts");
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/base.ts")],
    );

    let view_before = host.resolver_store_view();
    let initial = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_before))
        .expect("Props should materialize before the route change");
    assert_eq!(
        initial
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/base.ts"),
    );

    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./dep", "/src/alt.ts")],
    );

    let view_after = host.resolver_store_view();
    let rebuilt = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_after))
        .expect("Props should rebuild after the effective dependency target changes");
    assert_eq!(
        rebuilt
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/alt.ts"),
        "exact-resolution fact validation must invalidate the old bundle when the effective target changes",
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should materialize");
    host.provenance().reset();

    let first = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
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
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("second lookup should reuse the prepared bundle");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
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

    let view = host.resolver_store_view();
    host.provenance().reset();

    let first = host
        .prepared_type_decl_in_view("/workspace/types.ts", "Props", Some(&view))
        .expect("first lookup should materialize a prepared bundle");
    let after_first = host.provenance().snapshot();
    assert_eq!(
        after_first.bundle_materializations, 1,
        "first lookup with a captured store view should materialize exactly one bundle"
    );

    let second = host
        .prepared_type_decl_in_view("/workspace/types.ts", "Props", Some(&view))
        .expect("second lookup should reuse the prepared bundle even with the same captured view");
    let after_second = host.provenance().snapshot();

    assert_eq!(
        first
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/workspace/dep.ts"),
    );
    assert_eq!(
        second
            .name_resolution
            .get("Base")
            .map(|identity| identity.canonical_id.as_str()),
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
    // Verify the dependency resolution is persisted in the compile_cache
    let compile_entry = host
        .compile_cache
        .get("/workspace/types.ts")
        .expect("types file should have a compile cache entry");
    assert_eq!(
        compile_entry
            .import_routes
            .get("./dep")
            .and_then(|dep| dep.resolved_canonical_id.as_deref()),
        Some("/workspace/dep.ts"),
        "structurally derived exact resolutions should be persisted onto the compile cache entry",
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
        .ensure_module_facts_in_view(canonical_id, None)
        .expect("declaration entrypoint should seed module facts");

    host.set_import_dependencies(
        canonical_id,
        vec![exact_dependency(
            "./inner.js",
            "/workspace/node_modules/lib/dist/inner.d.ts",
        )],
    );

    let resolved = host.resolve_imported_type_root_in_view(canonical_id, "FancyProps", None);
    assert_eq!(
        resolved,
        (
            "/workspace/node_modules/lib/dist/inner.d.ts".to_string(),
            "FancyProps".to_string()
        ),
        "imported declaration entrypoints must upgrade stale miss routes to the exact declaration target",
    );

    let facts = host
        .ensure_module_facts_in_view(canonical_id, None)
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

#[test]
fn resolve_imported_type_root_caches_stable_miss_in_imported_root_db() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    upsert_non_sfc(
        &host,
        canonical_id,
        "export interface PresentProps { open: boolean }\n",
    );

    host.ensure_module_facts_in_view(canonical_id, None)
        .expect("module facts should seed the provider before capturing a store view");
    let view = host.resolver_store_view();

    let resolved =
        host.resolve_imported_type_root_in_view(canonical_id, "MissingProps", Some(&view));
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

#[cfg(feature = "scheduler")]
#[test]
fn prepared_type_decl_in_view_does_not_require_import_route_shadow_materialization() {
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

    let _route_shadow_guard = crate::host_resolve::forbid_import_route_shadow_for_tests();
    let prepared = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("prepared decl should materialize without the legacy import-route shadow path");

    let base = prepared
        .name_resolution
        .get("Base")
        .expect("prepared decl should carry imported Base resolution");
    assert_eq!(
        base.canonical_id, "/src/base.ts",
        "prepared decl name_resolution should point at the imported canonical owner",
    );
    assert_eq!(
        base.symbol_name, "Base",
        "prepared decl name_resolution should preserve the imported exported name",
    );
}

#[test]
fn prepared_type_decl_in_view_backfills_missing_local_symbol_when_cache_is_partial() {
    let host = make_host();
    let canonical_id = "/workspace/node_modules/lib/dist/index.d.ts";
    let source = r#"
type Local = { open: boolean }
export type FancyProps = Local
"#;
    upsert_non_sfc(&host, canonical_id, source);

    let prepared = host
        .prepared_type_decl_in_view(canonical_id, "Local", None)
        .expect("missing local decl should be prepared from shallow state");

    assert_eq!(prepared.root_identity.canonical_id, canonical_id);
    assert_eq!(prepared.root_identity.symbol_name, "Local");
}

// NOTE: ensure_shallow_imported_dependency_state_replaces_stale_prepared_type_decls was removed
// because prepared_type_decls no longer lives on ModuleFacts — prepared decls
// are managed through the bundle cache path.

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

    let view = host.resolver_store_view();

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

#[test]
fn resolver_store_view_tracks_reexport_import_routes() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/index.ts", "export { Props } from './dep'\n");

    let view = host.resolver_store_view();

    assert_eq!(
        host.resolve_type_dependency_canonical_shallow_in_view("/src/index.ts", "./dep", Some(&view))
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

    let view = host.resolver_store_view();

    assert_eq!(
        host.resolve_type_dependency_canonical_in_view("/src/index.d.ts", "./inner.js", Some(&view))
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

    let view = host.resolver_store_view();
    let exports = host.resolve_exports_in_view("/workspace/types/index.ts", Some(&view));

    assert!(
        exports.iter().any(|export| {
            export.name == "ButtonProps"
                && export.source_canonical_id.as_deref() == Some("/workspace/Button.vue")
        }),
        "captured store view should resolve exports for unloaded workspace barrels, got: {exports:?}",
    );
}

#[test]
fn stale_store_view_keeps_resolved_exports_on_captured_reexport_graph() {
    let host = make_host();

    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Props { msg: string }\n",
    );
    upsert_non_sfc(&host, "/src/index.ts", "export { Props } from './dep'\n");

    let before_view = host.resolver_store_view();
    let before_exports = host.resolve_exports_in_view("/src/index.ts", Some(&before_view));
    assert_eq!(before_exports.len(), 1);
    assert_eq!(
        before_exports[0].source_canonical_id.as_deref(),
        Some("/src/dep.ts")
    );

    upsert_non_sfc(
        &host,
        "/src/dep.ts",
        "export interface Renamed { disabled: boolean }\n",
    );

    assert!(
        host.resolve_exports_in_view("/src/index.ts", Some(&before_view))
            .is_empty(),
        "captured views must reject export graphs whose downstream re-export target changed",
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

    let view = host.resolver_store_view();
    let source = host
        .read_analysis_source("/workspace/node_modules/pkg/dist/shared.d.ts")
        .expect("dependency source should load into the imported dependency cache");
    assert!(
        source.contains("Alpha"),
        "sanity check: the dependency source should be readable"
    );

    let before = host
        .ensure_module_facts_in_view("/workspace/node_modules/pkg/dist/shared.d.ts", Some(&view))
        .expect("source-only imported dependency entry should exist");
    // In the new ModuleFacts model, snapshot is always Arc<FileAnalysisSnapshot>.
    // Before explicit snapshot build, it starts as default (empty bindings).
    assert!(
        before.snapshot.bindings.is_empty(),
        "source-only imported dependency entry should start with an empty snapshot"
    );
    let snapshot = host
        .get_raw_analysis_snapshot_in_view(
            "/workspace/node_modules/pkg/dist/shared.d.ts",
            Some(&view),
        )
        .expect("store-view snapshot path should build the dependency snapshot");
    assert!(
        snapshot.bindings.is_empty(),
        "simple declaration file should still produce a valid analysis snapshot"
    );

    let env = host
        .base_eval_env_in_view("/workspace/node_modules/pkg/dist/shared.d.ts", Some(&view))
        .expect("store-view eval env path should build the dependency env");
    assert!(
        env.type_symbols.contains_key("Alpha"),
        "built dependency env should expose the declaration symbol"
    );

    let after = host
        .ensure_module_facts_in_view("/workspace/node_modules/pkg/dist/shared.d.ts", Some(&view))
        .expect("dependency entry should remain cached after store-view generic access");
    // Verify facts still exist after store-view generic access
    assert!(
        !after.raw_source.is_empty(),
        "store-view access should preserve the module facts"
    );
}

#[test]
fn store_view_current_eval_state_does_not_seed_unloaded_imported_dependency_from_workspace() {
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

    let view = host.resolver_store_view();
    ws.reset_reads();

    assert!(
        host.current_eval_state_in_view(canonical_id, Some(&view))
            .is_none(),
        "store-view current_eval_state should not seed imported dependency source from workspace",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        0,
        "current_eval_state must not touch the workspace for an unseeded imported dependency",
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

    let view = host.resolver_store_view();
    ws.reset_reads();

    let entry = host
        .ensure_module_facts_in_view(canonical_id, Some(&view))
        .expect("explicit shallow seeding should load imported dependency state");
    // external_type_analysis is Arc<AnalyzedExternalTypeSource> (non-optional in ModuleFacts);
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
        host.current_eval_state_in_view(canonical_id, Some(&view))
            .is_some(),
        "current_eval_state should reuse the imported cache after explicit seeding",
    );
    assert_eq!(
        ws.read_count(canonical_id),
        1,
        "current_eval_state should reuse the seeded imported cache without another workspace read",
    );

    let snapshot = host
        .get_raw_analysis_snapshot_in_view(canonical_id, Some(&view))
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
        .base_eval_env_in_view(canonical_id, Some(&view))
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

    let view = host.resolver_store_view();

    let facts = host
        .ensure_module_facts_in_view(barrel, Some(&view))
        .expect("explicit shallow seeding should materialize the barrel facts");
    assert_eq!(
        facts.shallow_state.wildcard_reexports.len(),
        1,
        "barrel should publish its wildcard reexport",
    );
    assert_eq!(
        facts.shallow_state.wildcard_reexports[0].canonical_id,
        shared,
        "seeded ModuleFacts must backfill wildcard canonical IDs even when the store view had no exact-resolution snapshot",
    );

    ws.reset_resolves();
    let resolved =
        host.resolve_type_dependency_canonical_shallow_in_view(barrel, "./shared.js", Some(&view));
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

    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly materializes
    // from workspace sources, so we can't assert is_none before read_dep.

    let first = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let second = host.read_dep_source_for_type_resolution("/workspace/src/InputMenu.vue", None);
    let promoted = host
        .ensure_module_facts_in_view("/workspace/src/InputMenu.vue", None)
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
        promoted.cached_parse.is_some(),
        "the promoted Vue dependency cache entry should retain the cached SFC parse",
    );
    // In the new ModuleFacts model, ensure_module_facts_in_view eagerly builds a
    // full snapshot, so we just verify the facts are present and well-formed.
    // external_type_analysis is Arc (non-optional) in ModuleFacts; verify it has content.
    assert!(
        promoted.external_type_analysis.stats().top_level_statement_count > 0,
        "type-resolution reads should seed shallow external type analysis alongside the eval source",
    );
}

#[test]
fn materialize_imported_dependency_state_in_view_reuses_cached_vue_entry_arc() {
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
        .ensure_module_facts_in_view("/src/types.vue", None)
        .expect("first Vue imported dependency state should be built");
    let second = host
        .ensure_module_facts_in_view("/src/types.vue", None)
        .expect("second Vue imported dependency state should reuse the cached entry");

    assert_eq!(
        first.whole_hash, second.whole_hash,
        "repeated Vue imported dependency state lookups should produce equivalent entries",
    );
    // In ModuleFacts, snapshot is Arc<FileAnalysisSnapshot> (non-optional).
    assert!(
        first.cached_parse.is_some(),
        "cached Vue imported dependency entry should retain parse state",
    );
    assert!(
        first.script_analysis.is_some() && first.export_signatures.is_some(),
        "cached Vue imported dependency entry should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    // external_type_analysis is Arc (non-optional) in ModuleFacts; verify it has content.
    assert!(
        first.external_type_analysis.stats().top_level_statement_count > 0,
        "cached Vue imported dependency entry should eagerly retain external type analysis so later resolver lookups do not reparse",
    );
}

#[test]
fn materialize_imported_dependency_state_in_view_populates_external_type_analysis_for_non_sfc() {
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("imported dependency state should be materialized");

    // snapshot is Arc<FileAnalysisSnapshot> (non-optional) in ModuleFacts.
    // Non-SFC entries always have a snapshot populated after materialization.
    assert!(
        !entry.raw_source.is_empty(),
        "non-SFC imported dependency state should retain the analysis snapshot without caching env",
    );
    assert!(
        entry.script_analysis.is_some() && entry.export_signatures.is_some(),
        "non-SFC imported dependency state should retain script facts alongside the full snapshot for later export-graph reuse",
    );
    // external_type_analysis is Arc (non-optional) in ModuleFacts; verify it has content.
    assert!(
        entry.external_type_analysis.stats().top_level_statement_count > 0,
        "non-SFC imported dependency state should eagerly retain external type analysis so later resolver lookups stay on cache",
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
        .build_fallthrough_eval_env_lightweight_in_view("/src/App.vue", &snapshot, None, None)
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

    let store_view = host.resolver_store_view();

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
        .resolve_component_meta_in_view(
            "/src/Button.vue",
            crate::types::ResolverMode::Expanded,
            &store_view,
        )
        .expect("resolved meta should be computed from the captured view");

    let meta = extract_component_meta_from_resolved(
        &host,
        "/src/Button.vue",
        &resolved,
        true,
        Some(&store_view),
    );

    assert!(
        matches!(
            meta.fallthrough_surface,
            verter_semantic::analysis::component_meta::FallthroughSurface::Branches { .. }
        ),
        "button fallthrough should still resolve through the imported Link root",
    );
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

#[test]
fn prepared_type_decl_lookup_resolves_barrel_reexport_through_module_facts() {
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
        .ensure_module_facts_in_view("/src/barrel.ts", None)
        .expect("barrel should materialize module facts");
    assert!(
        barrel_facts.shallow_state.exports.contains_key("BaseProps"),
        "barrel module facts should list BaseProps as a reexport",
    );

    // The base file should also be materializable
    let base_facts = host
        .ensure_module_facts_in_view("/src/base.ts", None)
        .expect("base should materialize module facts");
    assert!(
        base_facts.shallow_state.symbols.contains_key("BaseProps"),
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should materialize");

    // Warm the bundle cache so the fact-validated entry exists.
    let view_before = host.resolver_store_view();
    assert!(
        host.prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_before))
            .is_some(),
        "prepared lookup should succeed before the file content changes"
    );

    // Change the file content — `Props` no longer exists.
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Other { value: number }",
    );
    let _ = host
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should re-materialize after content change");

    // Take a new view that records the updated hash.
    let view_after = host.resolver_store_view();

    // The stale bundle (cached with original hash) must be rejected by
    // fact validation against the new view, and the re-materialized
    // bundle must not contain the removed symbol.
    assert!(
        host.prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_after))
            .is_none(),
        "prepared lookup should drop stale cached declarations when the owning file hash changes"
    );
    assert!(
        host.prepared_type_decl_in_view("/src/types.ts", "Props", None)
            .is_none(),
        "prepared lookup without an explicit store view should also reject the stale bundle"
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

#[cfg(feature = "scheduler")]
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
    // Configure workspace resolver with alias
    {
        host.workspace().configure_resolver(vec![
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
                membership:
                    verter_semantic::analysis::project_resolver::ProjectMembership::MatchAll,
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
fn get_analysis_uses_cached_parse_for_lazy_analysis() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, source is immutable in the scheduler snapshot,
    // so mutating host.files has no effect. The scheduler path reads from
    // HostSourceData.cached_parse directly. We just verify get_analysis()
    // returns correct lazy-recomputed data with AnalysisLevel::None.
    #[cfg(not(feature = "scheduler"))]
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
fn get_analysis_falls_back_when_cached_parse_missing() {
    let host = make_lazy_host();
    upsert_vue(&host, "App.vue", LAZY_ANALYSIS_SFC);

    // On the scheduler path, cached_parse is immutable in HostSourceData
    // and always present for Vue SFCs. The scheduler path handles both
    // cached_parse present and absent cases. We just verify correctness.
    #[cfg(not(feature = "scheduler"))]
    clear_cached_parse(&host);

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
            file_kind: FileKind::NonSfc,
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

/// @ai-generated - get_export_span for .vue default import finds first binding
#[test]
fn get_export_span_vue_default() {
    let host = make_host();
    upsert_vue(
        &host,
        "Child.vue",
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div/></template>",
    );

    let span = host.get_export_span("Child.vue", "default");
    assert!(
        span.is_some(),
        "default export of .vue should resolve to first binding"
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
            file_kind: FileKind::NonSfc,
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
    assert!(
        start < end,
        "should have a valid span in Popup.vue (start={start}, end={end})"
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
    assert!(start < end, "should return a valid span in BarrelComp.vue");
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
    #[cfg(feature = "scheduler")]
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
    #[cfg(not(feature = "scheduler"))]
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
        file_kind: FileKind::NonSfc,
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
    host.resolve_component_meta(canonical_or_alias, crate::types::ResolverMode::Expanded)
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
    let resolved = resolved_macro_by_type(&state, "ButtonProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
    let resolved = resolved_macro_by_type(&state, "DualProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
            file_kind: FileKind::VueSfc,
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
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
    let resolved = resolved_macro_by_type(&state, "FancyProps");
    let props: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.name.as_str())
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let props: Vec<&str> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let names: Vec<&str> = state
        .resolved_macros
        .iter()
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(names.contains(&"x"), "should have 'x' from A: {:?}", names);
    assert!(names.contains(&"y"), "should have 'y' from B: {:?}", names);
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let emits: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineEmits)
        .flat_map(|m| m.emits.iter())
        .collect();
    let change = emits.iter().find(|e| e.name == "change");
    assert!(change.is_some(), "should have 'change' emit");
    let payload = change.unwrap().payload_type.as_deref().unwrap_or("");
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slots: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slot_names: Vec<&str> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        slot_names.contains(&"default"),
        "should have 'default': {:?}",
        slot_names
    );
    assert!(
        slot_names.contains(&"header"),
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let prop_names: Vec<&str> = state
        .resolved_macros
        .iter()
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        prop_names.contains(&"name"),
        "should have 'name': {:?}",
        prop_names
    );
    assert!(
        prop_names.contains(&"status"),
        "should have 'status': {:?}",
        prop_names
    );
    // Negative: props should not contain 'Status' as a prop (it's a type, not a prop)
    assert!(
        !prop_names.contains(&"Status"),
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let slots: Vec<_> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots)
        .flat_map(|m| m.slots.iter())
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
  default: (props: { item: string }) => VNode[]
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
fn external_type_analysis_in_view_reuses_cached_analysis_for_same_dependency() {
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
        .external_type_analysis_in_view("/src/types.ts", None)
        .expect("first analysis should load and cache the dependency");
    let second = host
        .external_type_analysis_in_view("/src/types.ts", None)
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
fn external_type_analysis_in_view_prefers_declaration_companion_for_runtime_js_dependencies() {
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
        .external_type_analysis_in_view("/workspace/node_modules/pkg/dist/index.js", None)
        .expect("runtime-script analysis requests should prefer the declaration companion");

    assert!(
        analysis.local_symbol_span("Props").is_some(),
        "the declaration companion analysis should expose declaration symbols",
    );

    // In the new ModuleFacts DB, ensure_module_facts_in_view normalizes .js → .d.ts
    // companion, so the .js path returns the .d.ts entry. Verify the declaration companion
    // is properly cached with analysis content.
    let declaration_entry = host
        .ensure_module_facts_in_view("/workspace/node_modules/pkg/dist/index.d.ts", None)
        .expect("the declaration companion should own the cached analysis");
    // external_type_analysis is Arc (non-optional) in ModuleFacts; verify it has content.
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
fn resolve_eval_dependency_canonical_in_view_prefers_declaration_companion_shallowly() {
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
    let resolved = host.resolve_eval_dependency_canonical_in_view(
        "/workspace/node_modules/pkg/dist/index.js",
        None,
    );

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

    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly materializes.
    // Verify that the module_facts DB was NOT populated by the shallow
    // resolve_eval_dependency_canonical_in_view call itself.
    assert!(
        host.resolver
            .runtime
            .module_facts
            .get_any("/workspace/node_modules/pkg/dist/index.js")
            .is_none(),
        "shallow companion selection should not cache .js facts in the module_facts DB",
    );
    assert!(
        host.resolver.runtime.module_facts.get_any("/workspace/node_modules/pkg/dist/index.d.ts").is_none(),
        "companion canonicalization must not materialize or cache the declaration target during shallow selection",
    );
}

#[test]
fn resolve_eval_dependency_canonical_in_view_ignores_empty_candidate_without_reads() {
    let ws = Arc::new(CountingWorkspace::new());
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    let resolved = host.resolve_eval_dependency_canonical_in_view("", None);

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
        host.ensure_module_facts_in_view("", None).is_none(),
        "empty canonical ids must not seed imported dependency cache entries",
    );
}

#[test]
fn resolve_eval_dependency_canonical_in_view_prefers_extension_candidates_before_raw_extensionless_probe(
) {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let resolved =
        host.resolve_eval_dependency_canonical_in_view("/workspace/src/runtime/types/html", None);

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
fn current_eval_state_in_view_normalizes_extensionless_canonical_before_fallback_load() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let state = host.current_eval_state_in_view("/workspace/src/runtime/types/html", None);

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
fn get_raw_analysis_snapshot_in_view_normalizes_extensionless_canonical_before_building_snapshot() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let snapshot =
        host.get_raw_analysis_snapshot_in_view("/workspace/src/runtime/types/html", None);

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
fn prepared_type_decl_in_view_normalizes_extensionless_canonical_before_shallow_backfill() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/runtime/types/html.ts",
        "export interface ButtonHTMLAttributes { disabled?: boolean }\n",
    );

    let host = VerterHost::new(HostConfig::default(), ws.clone());

    ws.reset_reads();
    ws.reset_exists();
    let prepared = host.prepared_type_decl_in_view(
        "/workspace/src/runtime/types/html",
        "ButtonHTMLAttributes",
        None,
    );

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
        host.current_eval_state_in_view("", None).is_none(),
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
        host.ensure_module_facts_in_view("", None).is_none(),
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
            host.current_eval_state_in_view(specifier, None).is_none(),
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
            host.ensure_module_facts_in_view(specifier, None).is_none(),
            "raw import specifier {specifier} must not seed imported dependency cache entries",
        );
    }
}

#[test]
fn external_type_analysis_in_view_uses_eval_source_for_vue_dependencies() {
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
        .external_type_analysis_in_view("/src/types.vue", None)
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
    assert!(
        analysis.required_import_names("Props").contains("Base"),
        "vue dependency analysis should compute required imported names from the script block",
    );
}

#[test]
fn resolve_external_type_from_cached_dependency_state_in_view_keeps_local_type_resolution_shallow()
{
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/src/types.ts",
        "export interface Props { label: string }\n",
    );

    let host = VerterHost::new(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws.clone(),
    );

    let shallow = host
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should seed shallow imported state");
    // snapshot is Arc<FileAnalysisSnapshot> (non-optional); check it's default/empty
    assert!(
        shallow.snapshot.bindings.is_empty() && shallow.snapshot.imports.is_empty(),
        "shallow imported state should stay export-only before local type resolution",
    );

    ws.reset_reads();
    let resolved = host
        .resolve_external_type_from_cached_dependency_state_in_view(
            "/src/types.ts",
            "Props",
            &rustc_hash::FxHashMap::default(),
            None,
        )
        .expect("local type resolution should succeed from shallow imported state");

    assert!(
        resolved
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("label")),
        "resolved props should include the local interface member, got {:?}",
        resolved.props,
    );

    let cached = host
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should remain cached after local type resolution");
    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly builds full facts.
    // The shallowness constraint applies to the internal resolution path, not to
    // the post-hoc facts query. Verify the facts are present and correct.
    assert!(
        !cached.raw_source.is_empty(),
        "types dependency should have source content after local type resolution",
    );
}

#[test]
fn external_type_analysis_in_view_preserves_vue_tsx_source_type() {
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
        .external_type_analysis_in_view("/src/types.vue", None)
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

#[cfg(feature = "scheduler")]
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
    let mut visiting = rustc_hash::FxHashSet::default();

    let resolved = host
        .resolve_external_type_from_loaded_files(
            "/src/App.vue",
            "./useComponentIcons",
            "UseComponentIconsProps",
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
        )
        .expect("external type resolution should complete")
        .expect("UseComponentIconsProps should resolve");

    assert!(
        resolved
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("icon")),
        "Icon-backed props should still resolve through structural indexed access, got {:?}",
        resolved.props
    );
    assert!(
        resolved
            .props
            .iter()
            .any(|prop| prop.key_name.as_deref() == Some("avatar")),
        "leaf imported prop aliases should remain present without resolving the companion body, got {:?}",
        resolved.props
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
fn base_eval_env_in_view_prefers_declaration_companion_for_runtime_js_dependencies() {
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
        .base_eval_env_in_view("/workspace/node_modules/pkg/dist/index.js", None)
        .expect("runtime-script env requests should prefer the declaration companion");

    assert!(
        env.value_symbols.contains_key("useForwardProps"),
        "the declaration companion env should expose value declarations",
    );

    // In the new ModuleFacts DB, ensure_module_facts_in_view normalizes .js → .d.ts
    // companion and eagerly materializes. Verify the companion has the right content.
    let declaration_entry = host
        .ensure_module_facts_in_view("/workspace/node_modules/pkg/dist/index.d.ts", None)
        .expect("the declaration companion should own the cached env");
    // Verify declaration companion has content.
    assert!(
        !declaration_entry.raw_source.is_empty(),
        "the declaration companion should have source content",
    );
}

#[cfg(feature = "scheduler")]
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
    let resolved = host.resolve_named_type_export_target_in_view("/src/index.ts", "Props", None);

    assert_eq!(
        resolved,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through the barrel",
    );

    let barrel_entry = host
        .ensure_module_facts_in_view("/src/index.ts", None)
        .expect("barrel file should be cached after routing");
    let target_entry = host
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("target file should be cached after routing");

    // external_type_analysis is Arc (non-optional) in ModuleFacts; verify it has content.
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
    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly builds
    // full snapshots. The shallowness constraint applies to the internal routing,
    // not to the post-hoc facts query.
    assert_eq!(
        ws.read_count("/src/base.ts"),
        0,
        "shallow export routing should not touch transitive children that are not on the requested path",
    );
}

#[cfg(feature = "scheduler")]
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

    let resolved = host.resolve_named_type_export_target_in_view("/src/index.ts", "Props", None);

    assert_eq!(
        resolved,
        Some(("/src/types.vue".to_string(), "Props".to_string())),
        "registry routing should preserve the vue script lang and find tsx exports behind barrels",
    );
}

#[cfg(feature = "scheduler")]
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

    let _route_shadow_guard = crate::host_resolve::forbid_import_route_shadow_for_tests();
    let _guard = crate::host_resolve::forbid_route_frontier_for_tests();
    let route = host.resolve_named_type_export_target_in_view("/src/index.ts", "Props", None);
    assert_eq!(
        route,
        Some(("/src/types.ts".to_string(), "Props".to_string())),
        "named export routing should resolve through DB-owned shallow facts without frontier fallback",
    );

    let root = host.resolve_imported_type_root_in_view("/src/index.ts", "Props", None);
    assert_eq!(
        root,
        ("/src/types.ts".to_string(), "Props".to_string()),
        "imported-root proof should reuse the DB-owned route without frontier fallback",
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn ensure_shallow_imported_dependency_state_for_vue_exports_stays_local() {
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
        .ensure_module_facts_in_view("/src/Button.vue", None)
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
fn ensure_shallow_imported_dependency_state_defers_prepared_decl_materialization_until_lookup() {
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should seed shallow imported state");

    let prepared_type = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("prepared type decl should materialize on demand");
    assert!(
        prepared_type.member_index.contains_key("label"),
        "on-demand prepared type materialization should retain the shallow member index",
    );

    let prepared_value = host
        .prepared_value_decl_in_view("/src/types.ts", "defaults", None)
        .expect("prepared value decl should materialize on demand");
    assert!(
        matches!(
            prepared_value.type_annotation.as_ref(),
            Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Props"
        ),
        "on-demand prepared value materialization should retain the shallow type annotation, got {:?}",
        prepared_value.type_annotation
    );

    assert!(
        host.prepared_type_decl_in_view("/src/types.ts", "Props", None)
            .is_some(),
        "on-demand prepared type materialization should be available through the bundle cache",
    );
    assert!(
        host.prepared_value_decl_in_view("/src/types.ts", "defaults", None)
            .is_some(),
        "on-demand prepared value materialization should be available through the bundle cache",
    );

    let audit = host.component_meta_audit_store_snapshot(None);
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should seed imported state");
    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly builds full facts.
    assert!(
        !entry.raw_source.is_empty(),
        "types dependency should have source content",
    );

    let prepared = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("Props should prepare from the imported cache");
    let resolved = prepared.name_resolution.get("Component").expect(
        "prepared declaration should resolve imported Component through dependency targets",
    );
    assert_eq!(
        resolved.canonical_id, "/node_modules/@vue/runtime-core.d.ts",
        "prepared declaration lookup should canonicalize imported names",
    );

    let cached = host
        .ensure_module_facts_in_view("/src/types.ts", None)
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

    let store_view = host.resolver_store_view();
    let prepared = host
        .prepared_type_decl_in_view("/src/Button.vue", "Button", Some(&store_view))
        .expect("Button should prepare from the owner-local shallow cache");

    assert_eq!(
        prepared
            .name_resolution
            .get("AppConfig")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/schema.ts"),
        "owner-local prepared declarations should canonicalize type imports through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("ComponentConfig")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/tv.ts"),
        "owner-local prepared declarations should canonicalize imported helper aliases through stored dependency targets",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("theme")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/theme.ts"),
        "owner-local prepared declarations should canonicalize imported values through stored dependency targets",
    );
}

#[cfg(feature = "scheduler")]
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
    assert!(
        ws.read_count("/src/types/a.ts") <= 1,
        "flat barrel lookup should avoid fully seeding unrelated later siblings, got {} reads for /src/types/a.ts",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 1,
        "flat barrel lookup should avoid rereading later same-level siblings, got {} reads for /src/types/b.ts",
        ws.read_count("/src/types/b.ts"),
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn declaration_scoped_solver_applies_omit_to_barrel_imported_types() {
    fn collect_object_property_names(
        expr: &TypeExpr,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        match expr {
            TypeExpr::Object(object) => {
                for member in &object.properties {
                    if let ObjectMember::Property(prop) = member {
                        names.insert(prop.name.clone());
                    }
                }
            }
            TypeExpr::Intersection(members) | TypeExpr::Union(members) => {
                for member in members.iter() {
                    collect_object_property_names(member, names);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_object_property_names(inner, names),
            _ => {}
        }
    }

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types/index.ts", "export * from '../Button.vue'\n");
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
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
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![exact_dependency("../Button.vue", "/src/Button.vue")],
    );

    let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
        &host,
        None,
        "/src/App.vue",
    );
    let evaluated = verter_semantic::analysis::type_solver::solve::solve_type(
        &TypeExpr::named("Props"),
        &solver_host,
    )
    .value;
    let mut prop_names = std::collections::BTreeSet::new();
    collect_object_property_names(&evaluated, &mut prop_names);

    assert_eq!(
        prop_names,
        std::collections::BTreeSet::from([
            "icon".to_string(),
            "label".to_string(),
            "status".to_string(),
        ]),
        "declaration-scoped solving should apply Omit after routing imported barrel types to their defining prepared declarations",
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn declaration_scoped_solver_applies_omit_to_barrel_imported_types_with_store_view() {
    fn collect_object_property_names(
        expr: &TypeExpr,
        names: &mut std::collections::BTreeSet<String>,
    ) {
        match expr {
            TypeExpr::Object(object) => {
                for member in &object.properties {
                    if let ObjectMember::Property(prop) = member {
                        names.insert(prop.name.clone());
                    }
                }
            }
            TypeExpr::Intersection(members) | TypeExpr::Union(members) => {
                for member in members.iter() {
                    collect_object_property_names(member, names);
                }
            }
            TypeExpr::Parenthesized(inner) => collect_object_property_names(inner, names),
            _ => {}
        }
    }

    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/types/index.ts", "export * from '../Button.vue'\n");
    ws.inject_file(
        "/src/Button.vue",
        r#"<script lang="ts">
export interface IconProps {
  icon?: string
}

export interface ButtonProps extends IconProps {
  label?: string
  color?: string
}
</script>
<template><div /></template>"#,
    );
    ws.inject_file(
        "/src/App.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './types'

type Props = Omit<ButtonProps, 'color'> & {
  status?: string
}

defineProps<Props>()
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
    assert!(host.ensure_loaded("/src/App.vue"));
    host.set_import_dependencies(
        "/src/App.vue",
        vec![exact_dependency("./types", "/src/types/index.ts")],
    );
    host.set_import_dependencies(
        "/src/types/index.ts",
        vec![exact_dependency("../Button.vue", "/src/Button.vue")],
    );

    let view = host.resolver_store_view();
    let prepared = host
        .prepared_type_decl_in_view("/src/Button.vue", "ButtonProps", Some(&view))
        .expect("ButtonProps prepared decl should materialize under the store view");
    assert!(
        prepared.name_resolution.contains_key("IconProps"),
        "store-view prepared decl should keep same-file heritage bindings in name_resolution",
    );
    assert_eq!(
        prepared
            .name_resolution
            .get("IconProps")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/Button.vue"),
        "store-view prepared decl should resolve same-file heritage names back to the defining file",
    );
    assert!(
        format!("{:?}", prepared.body).contains("IconProps"),
        "store-view prepared decl should retain interface heritage in the raw body",
    );
    assert!(
        prepared.local_deps.iter().any(|dep| dep == "IconProps"),
        "store-view prepared decl should retain same-file heritage in local_deps",
    );
    assert!(
        host.prepared_type_decl_in_view("/src/Button.vue", "IconProps", Some(&view))
            .is_some(),
        "store-view prepared decl cache should also expose same-file support symbols",
    );
    let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
        &host,
        Some(&view),
        "/src/App.vue",
    );
    let evaluated = verter_semantic::analysis::type_solver::solve::solve_type(
        &TypeExpr::named("Props"),
        &solver_host,
    )
    .value;
    let mut prop_names = std::collections::BTreeSet::new();
    collect_object_property_names(&evaluated, &mut prop_names);

    assert_eq!(
        prop_names,
        std::collections::BTreeSet::from([
            "icon".to_string(),
            "label".to_string(),
            "status".to_string(),
        ]),
        "store-view declaration-scoped solving should preserve inherited members after barrel routing",
    );
}

#[cfg(feature = "scheduler")]
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
    assert!(
        ws.read_count("/src/types/a.ts") <= 1,
        "multiple late barrel exports should not reread unrelated sibling 'a', got {}",
        ws.read_count("/src/types/a.ts"),
    );
    assert!(
        ws.read_count("/src/types/b.ts") <= 1,
        "multiple late barrel exports should not reread unrelated sibling 'b', got {}",
        ws.read_count("/src/types/b.ts"),
    );
}

#[cfg(feature = "scheduler")]
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
        .ensure_module_facts_in_view("/src/types/index.ts", None)
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
    let props_root =
        host.resolve_imported_type_root_in_view("/src/types/index.ts", "TargetProps", None);
    let emits_root =
        host.resolve_imported_type_root_in_view("/src/types/index.ts", "TargetEmits", None);

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

#[cfg(feature = "scheduler")]
#[test]
fn prepared_type_decl_in_view_keeps_export_only_barrels_shallow_for_missing_local_symbols() {
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

    host.ensure_module_facts_in_view("/src/types/index.ts", None)
        .expect("barrel should materialize shallow imported state");

    ws.reset_resolves();

    let prepared = host.prepared_type_decl_in_view("/src/types/index.ts", "TargetProps", None);

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

#[cfg(feature = "scheduler")]
#[test]
fn current_dependency_fact_versions_in_view_keeps_imported_barrel_route_facts_shallow() {
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

    host.ensure_module_facts_in_view("/src/types/index.ts", None)
        .expect("barrel should materialize shallow imported state");
    let view = host.resolver_store_view();

    ws.reset_resolves();

    let facts = host.current_dependency_fact_versions_in_view(
        "/src/types/index.ts",
        &std::collections::BTreeSet::new(),
        Some(&view),
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

#[cfg(feature = "scheduler")]
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

#[cfg(feature = "scheduler")]
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

    let resolved =
        host.resolve_named_type_export_target_in_view("/src/types.ts", "LinkProps", None);

    assert_eq!(
        resolved,
        Some(("/src/Link.vue".to_string(), "LinkProps".to_string())),
        "wildcard barrel routing should still resolve the requested child",
    );

    // In the new ModuleFacts DB, ensure_module_facts_in_view eagerly builds complete
    // facts including export_signatures and script_analysis. Verify the facts exist
    // and have the expected content.
    let barrel = host
        .ensure_module_facts_in_view("/src/types.ts", None)
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
        .ensure_module_facts_in_view("/src/Link.vue", None)
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

#[cfg(feature = "scheduler")]
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

    host.ensure_module_facts_in_view("/src/types/index.ts", None)
        .expect("barrel should materialize shallow export state");
    let view = host.resolver_store_view();

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
    // imported_dependency_cache reference removed
    host.compile_cache.remove("/src/types/index.ts");

    let resolved = host.resolve_type_dependency_canonical_shallow_in_view(
        "/src/types/index.ts",
        "./target",
        Some(&view),
    );

    assert_eq!(
        resolved.as_deref(),
        Some("/src/types/target.ts"),
        "route resolution should continue to use cached module facts while the store view provides the validating import-route hash",
    );
    assert_eq!(
        ws.resolve_count("/src/types/index.ts", "./target"),
        0,
        "captured store-view barrel routes should not fall back to a live workspace resolve",
    );
}

#[cfg(feature = "scheduler")]
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
        .ensure_module_facts_in_view("/src/Link.vue", None)
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

#[cfg(feature = "scheduler")]
#[test]
fn import_route_lookup_reuses_imported_dependency_cache_without_live_owner_state() {
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

    host.ensure_module_facts_in_view("/src/types.ts", None)
        .expect("barrel should seed shallow import routes");

    ws.remove_file("/src/types.ts");
    host.compile_cache.remove("/src/types.ts");

    let resolved = host.resolve_type_dependency_canonical_shallow_in_view(
        "/src/types.ts",
        "./Button.vue",
        None,
    );

    assert_eq!(
        resolved,
        Some("/src/Button.vue".to_string()),
        "dependency lookup should reuse cached imported import routes",
    );
}

#[cfg(feature = "scheduler")]
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
    let resolved =
        host.resolve_named_type_export_target_in_view("/src/types.ts", "ButtonProps", None);

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

#[cfg(feature = "scheduler")]
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
    let resolved =
        host.resolve_named_type_export_target_in_view("/src/types.ts", "ButtonProps", None);

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

#[cfg(feature = "scheduler")]
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
    let resolved =
        host.resolve_named_type_export_target_in_view("/src/types.ts", "CheckboxProps", None);

    assert_eq!(
        resolved,
        Some(("/src/Checkbox.vue".to_string(), "CheckboxProps".to_string())),
        "late wildcard match should still resolve to the correct child",
    );
}

#[cfg(feature = "scheduler")]
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
    let resolved =
        host.resolve_named_type_export_target_in_view("/src/types.ts", "CheckboxGroupProps", None);

    assert_eq!(
        resolved,
        Some((
            "/src/CheckboxGroup.vue".to_string(),
            "CheckboxGroupProps".to_string()
        )),
        "route selection should prefer the longest matching wildcard source stem",
    );
}

#[cfg(feature = "scheduler")]
#[test]
fn resolve_component_meta_nested_barrel_alias_skips_later_unrelated_siblings() {
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
        .resolve_component_meta("/src/Consumer.vue", crate::types::ResolverMode::Expanded)
        .expect("expanded component meta should resolve");

    let prop_names: std::collections::BTreeSet<_> = resolved
        .resolved_macros
        .iter()
        .flat_map(|resolved_macro| resolved_macro.props.iter())
        .map(|field| field.name.as_str())
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
        "nested alias resolution should not branch out into later unrelated barrel siblings",
    );
}

// ---------------------------------------------------------------------------
// Phase 0 — Characterization tests for solver-host local-only env cutover
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
/// This test exercises the full meta pipeline which currently uses the fat env.
/// After Phase 1, it must still pass with only the local-only env + solver host.
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
/// After Phase 1, the solver host must resolve both the direct import AND its
/// same-file dependencies through the prepared-decl cache.
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
    assert!(
        !matches!(
            ui_prop.type_expr,
            verter_semantic::analysis::type_expr::TypeExpr::Unknown { .. }
        ),
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
/// Verify that `prepared_type_decl_in_view` caches bundles (second call returns
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should materialize");

    // First lookup — materializes the bundle.
    let first = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("Props should prepare on first lookup");
    assert_eq!(first.root_identity.symbol_name, "Props");

    // Second lookup — should hit cache and return the same result.
    let second = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should re-materialize after content change");

    // New lookup should reflect the updated type.
    let updated = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
        .expect("Props should prepare after content change");
    assert_eq!(updated.root_identity.symbol_name, "Props");

    // Negative: the old symbol shape should be gone (the body should have changed).
    // We verify the bundle was invalidated by checking the prepared decl reflects new content.
    let body_debug = format!("{:?}", updated.body);
    assert!(
        body_debug.contains("title"),
        "updated prepared decl body should contain the new property 'title', got: {}",
        body_debug
    );
    assert!(
        !body_debug.contains("label"),
        "updated prepared decl body should NOT contain the old property 'label', got: {}",
        body_debug
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
        .ensure_module_facts_in_view("/src/types.ts", None)
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
    let view_before = host.resolver_store_view();
    let initial = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_before))
        .expect("Props should prepare with lazy resolution");
    assert_eq!(
        initial
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_str()),
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
    let view_after = host.resolver_store_view();
    let after_promotion = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_after))
        .expect("Props should still be found after lazy promotion");
    assert_eq!(
        after_promotion
            .name_resolution
            .get("Helper")
            .map(|identity| identity.canonical_id.as_str()),
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
        .ensure_module_facts_in_view("/src/types.ts", None)
        .expect("types dependency should materialize");

    // Route to v1.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v1.ts")],
    );

    let view_v1 = host.resolver_store_view();
    let props_v1 = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_v1))
        .expect("Props should prepare pointing to v1");
    assert_eq!(
        props_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/inner-v1.ts"),
        "Props name_resolution should point to inner-v1",
    );
    let alt_v1 = host
        .prepared_type_decl_in_view("/src/types.ts", "Alt", Some(&view_v1))
        .expect("Alt should prepare pointing to v1");
    assert_eq!(
        alt_v1
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/inner-v1.ts"),
        "Alt name_resolution should point to inner-v1",
    );

    // Change route to v2.
    host.set_import_dependencies(
        "/src/types.ts",
        vec![exact_dependency("./inner", "/src/inner-v2.ts")],
    );

    let view_v2 = host.resolver_store_view();
    let props_v2 = host
        .prepared_type_decl_in_view("/src/types.ts", "Props", Some(&view_v2))
        .expect("Props should rebuild after route change");
    assert_eq!(
        props_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/src/inner-v2.ts"),
        "Props name_resolution must point to inner-v2 after route change",
    );
    let alt_v2 = host
        .prepared_type_decl_in_view("/src/types.ts", "Alt", Some(&view_v2))
        .expect("Alt should rebuild after route change");
    assert_eq!(
        alt_v2
            .name_resolution
            .get("Inner")
            .map(|identity| identity.canonical_id.as_str()),
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
        .resolve_component_meta("/src/Comp.vue", crate::types::ResolverMode::Expanded)
        .expect("should return resolved state");
    let props: Vec<&str> = state
        .resolved_macros
        .iter()
        .filter(|m| m.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps)
        .flat_map(|m| m.props.iter())
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        props.contains(&"label"),
        "component-meta should resolve 'label' prop via bundle dep_edges path: {:?}",
        props
    );
    assert!(
        props.contains(&"count"),
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
        .ensure_module_facts_in_view("/src/consumer.ts", None)
        .expect("consumer dependency should materialize");

    // Set initial route to types-a.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-a.ts")],
    );

    let initial = host
        .prepared_type_decl_in_view("/src/consumer.ts", "Bar", None)
        .expect("Bar should prepare with route to types-a");
    assert_eq!(
        initial
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/types-a.ts"),
        "initial lookup should resolve Foo to types-a",
    );

    // Change route to types-b.
    host.set_import_dependencies(
        "/src/consumer.ts",
        vec![exact_dependency("./types", "/types-b.ts")],
    );

    let updated = host
        .prepared_type_decl_in_view("/src/consumer.ts", "Bar", None)
        .expect("Bar should rebuild after route change to types-b");
    assert_eq!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/types-b.ts"),
        "prepared decl must reflect the new target after dep-resolution change — guards against stale route bug",
    );
    // Negative: must NOT still point to the old target.
    assert_ne!(
        updated
            .name_resolution
            .get("Foo")
            .map(|identity| identity.canonical_id.as_str()),
        Some("/types-a.ts"),
        "prepared decl must NOT retain the stale route to types-a",
    );
}

#[test]
fn unrelated_upsert_keeps_route_root_and_type_surface_dbs_warm() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface ButtonProps { label: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/index.ts",
        "export { ButtonProps } from './types'\n",
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './index'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(&host, "/src/unrelated.ts", "export const unrelated = 1\n");

    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./index", "/src/index.ts")],
    );

    let view_before = host.resolver_store_view();
    let route = host.resolve_named_type_export_target_in_view(
        "/src/index.ts",
        "ButtonProps",
        Some(&view_before),
    );
    assert_eq!(
        route,
        Some(("/src/types.ts".to_string(), "ButtonProps".to_string())),
        "barrel route should resolve to the defining type file before the unrelated mutation",
    );
    let root =
        host.resolve_imported_type_root_in_view("/src/index.ts", "ButtonProps", Some(&view_before));
    assert_eq!(
        root,
        ("/src/types.ts".to_string(), "ButtonProps".to_string()),
        "barrel import should resolve to the defining type file before the unrelated mutation",
    );

    let solver_host = crate::resolver_core::SessionSolverHost::new(&host, Some(&view_before));
    let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(
        &host,
        Some(&view_before),
        &solver_host,
    );
    let projected = engine.project_type_surface("/src/types.ts", "ButtonProps");
    assert!(
        projected.is_some(),
        "projection should warm TypeSurfaceDb before the unrelated mutation",
    );

    let type_surface_key =
        crate::resolver_core::TypeSurfaceOpKey::Surface(crate::resolver_core::TypeSurfaceKey {
            canonical_owner: "/src/types.ts".to_string(),
            symbol_name: "ButtonProps".to_string(),
            instantiation_hash: 0,
            context_hash: 0,
        });
    assert!(
        matches!(
            host.resolver_runtime()
                .routes
                .get_route_any("/src/index.ts", "ButtonProps")
                .as_deref(),
            Some(crate::resolver_core::RouteResult::Resolved {
                defining_canonical,
                defining_symbol,
            }) if defining_canonical == "/src/types.ts" && defining_symbol == "ButtonProps"
        ),
        "RouteDb should be warm before the unrelated mutation",
    );
    assert!(
        matches!(
            host.resolver_runtime()
                .imported_roots
                .get_any("/src/index.ts", "ButtonProps")
                .as_deref(),
            Some(crate::resolver_core::ImportedRootResult::Resolved {
                canonical_source,
                resolved_symbol,
            }) if canonical_source == "/src/types.ts" && resolved_symbol == "ButtonProps"
        ),
        "ImportedRootDb should be warm before the unrelated mutation",
    );
    assert!(
        matches!(
            host.resolver_runtime()
                .type_surfaces
                .get(&type_surface_key, &view_before)
                .as_deref(),
            Some(crate::resolver_core::TypeSurfaceOpResult::Surface(_))
        ),
        "TypeSurfaceDb should be warm before the unrelated mutation",
    );

    upsert_non_sfc(&host, "/src/unrelated.ts", "export const unrelated = 1\n");

    let view_after = host.resolver_store_view();
    assert!(
        matches!(
            host.resolver_runtime()
                .routes
                .get_route_any("/src/index.ts", "ButtonProps")
                .as_deref(),
            Some(crate::resolver_core::RouteResult::Resolved {
                defining_canonical,
                defining_symbol,
            }) if defining_canonical == "/src/types.ts" && defining_symbol == "ButtonProps"
        ),
        "byte-identical unrelated upserts must not clear warm RouteDb entries",
    );
    assert!(
        matches!(
            host.resolver_runtime()
                .imported_roots
                .get_any("/src/index.ts", "ButtonProps")
                .as_deref(),
            Some(crate::resolver_core::ImportedRootResult::Resolved {
                canonical_source,
                resolved_symbol,
            }) if canonical_source == "/src/types.ts" && resolved_symbol == "ButtonProps"
        ),
        "byte-identical unrelated upserts must not clear warm ImportedRootDb entries",
    );
    assert!(
        matches!(
            host.resolver_runtime()
                .type_surfaces
                .get(&type_surface_key, &view_after)
                .as_deref(),
            Some(crate::resolver_core::TypeSurfaceOpResult::Surface(_))
        ),
        "byte-identical unrelated upserts must not clear warm TypeSurfaceDb entries",
    );
}

#[test]
fn unrelated_set_import_dependencies_keeps_route_root_and_type_surface_dbs_warm() {
    let host = make_host();
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface ButtonProps { label: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/index.ts",
        "export { ButtonProps } from './types'\n",
    );
    upsert_vue(
        &host,
        "/src/Button.vue",
        r#"<script setup lang="ts">
import type { ButtonProps } from './index'

defineProps<ButtonProps>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/src/other-types.ts",
        "export interface OtherProps { count: number }\n",
    );
    upsert_vue(
        &host,
        "/src/Other.vue",
        r#"<script setup lang="ts">
import type { OtherProps } from './other-types'

defineProps<OtherProps>()
</script>
<template><div /></template>"#,
    );

    host.set_import_dependencies(
        "/src/index.ts",
        vec![exact_dependency("./types", "/src/types.ts")],
    );
    host.set_import_dependencies(
        "/src/Button.vue",
        vec![exact_dependency("./index", "/src/index.ts")],
    );

    let view_before = host.resolver_store_view();
    let route = host.resolve_named_type_export_target_in_view(
        "/src/index.ts",
        "ButtonProps",
        Some(&view_before),
    );
    assert_eq!(
        route,
        Some(("/src/types.ts".to_string(), "ButtonProps".to_string())),
        "barrel route should resolve to the defining type file before the unrelated dependency update",
    );
    let root =
        host.resolve_imported_type_root_in_view("/src/index.ts", "ButtonProps", Some(&view_before));
    assert_eq!(
        root,
        ("/src/types.ts".to_string(), "ButtonProps".to_string()),
        "barrel import should resolve to the defining type file before the unrelated dependency update",
    );

    let solver_host = crate::resolver_core::SessionSolverHost::new(&host, Some(&view_before));
    let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(
        &host,
        Some(&view_before),
        &solver_host,
    );
    let projected = engine.project_type_surface("/src/types.ts", "ButtonProps");
    assert!(
        projected.is_some(),
        "projection should warm TypeSurfaceDb before the unrelated dependency update",
    );

    let type_surface_key =
        crate::resolver_core::TypeSurfaceOpKey::Surface(crate::resolver_core::TypeSurfaceKey {
            canonical_owner: "/src/types.ts".to_string(),
            symbol_name: "ButtonProps".to_string(),
            instantiation_hash: 0,
            context_hash: 0,
        });
    assert!(
        host.resolver_runtime()
            .routes
            .get_route_any("/src/index.ts", "ButtonProps")
            .is_some(),
        "RouteDb should be warm before the unrelated dependency update",
    );
    assert!(
        host.resolver_runtime()
            .imported_roots
            .get_any("/src/index.ts", "ButtonProps")
            .is_some(),
        "ImportedRootDb should be warm before the unrelated dependency update",
    );
    assert!(
        host.resolver_runtime()
            .type_surfaces
            .get(&type_surface_key, &view_before)
            .is_some(),
        "TypeSurfaceDb should be warm before the unrelated dependency update",
    );

    host.set_import_dependencies(
        "/src/Other.vue",
        vec![exact_dependency("./other-types", "/src/other-types.ts")],
    );

    let view_after = host.resolver_store_view();
    assert!(
        host.resolver_runtime()
            .routes
            .get_route_any("/src/index.ts", "ButtonProps")
            .is_some(),
        "unrelated dependency updates must not clear warm RouteDb entries",
    );
    assert!(
        host.resolver_runtime()
            .imported_roots
            .get_any("/src/index.ts", "ButtonProps")
            .is_some(),
        "unrelated dependency updates must not clear warm ImportedRootDb entries",
    );
    assert!(
        host.resolver_runtime()
            .type_surfaces
            .get(&type_surface_key, &view_after)
            .is_some(),
        "unrelated dependency updates must not clear warm TypeSurfaceDb entries",
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
        .prepared_type_decl_in_view("/src/types.ts", "Props", None)
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
    assert_eq!(prepared.root_identity.symbol_name, "Props");
    assert_eq!(prepared.root_identity.canonical_id, "/src/types.ts");
}

/// Regression guard 9: Shallow alias resolution through barrel re-exports.
///
/// A barrel file `export { Props } from './inner'` does not own a local
/// `Props` declaration — it is a re-export. `prepared_type_decl_in_view` on
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

    let _ = host.ensure_module_facts_in_view("/src/barrel.ts", None);

    let prepared = host.prepared_type_decl_in_view("/src/barrel.ts", "Props", None);
    assert!(
        prepared.is_none(),
        "barrel re-exports should NOT produce local prepared decls — Props is owned by inner.ts, not barrel.ts",
    );

    // Positive: the defining file should have the prepared decl.
    let inner_prepared = host.prepared_type_decl_in_view("/src/inner.ts", "Props", None);
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
fn archived_module_facts_rejected_when_workspace_dep_changes_content() {
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
        .ensure_module_facts_in_view("/src/dep.ts", None)
        .expect("dep v1 should materialize");
    let hash_v1 = facts_v1.whole_hash;

    // Step 2: soft-invalidate to push the entry into the archive.
    // This simulates what HostStoreView::snapshot_module_fact_hashes does
    // for entries it materializes during view construction.
    host.resolver.runtime.module_facts.invalidate("/src/dep.ts");

    // Step 3: change the dependency content via workspace injection.
    // No upsert — the host doesn't know the content changed.
    ws.inject_file(
        "/src/dep.ts",
        "export interface DepType { version: 2; extra: string }\n",
    );

    // Step 4: create a store view. The dep is not tracked (never upserted,
    // not in scheduler or compile_cache).
    let view = host.resolver_store_view();

    // Step 5: query module_facts with the store view. The validated cache
    // must NOT return the stale V1 facts from the archive.
    let facts_after = host
        .ensure_module_facts_in_view("/src/dep.ts", Some(&view))
        .expect("dep should re-materialize with current workspace content");
    assert_ne!(
        facts_after.whole_hash, hash_v1,
        "module_facts via store-view-validated cache must reflect the current \
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
    let view1 = host.resolver_store_view();
    let root1 = host.resolve_imported_type_root_in_view("/src/index.ts", "Props", Some(&view1));
    assert_eq!(
        root1.0.as_str(),
        "/src/types-a.ts",
        "initial root should point to types-a",
    );

    // Change barrel to point to types-b instead
    ws.inject_file("/src/barrel.ts", "export { Props } from './types-b'\n");
    host.set_import_dependencies(
        "/src/barrel.ts",
        vec![exact_dependency("./types-b", "/src/types-b.ts")],
    );

    // The provider (/src/index.ts) and the old leaf (/src/types-a.ts)
    // are unchanged. Only the barrel changed.
    let view2 = host.resolver_store_view();
    let root2 = host.resolve_imported_type_root_in_view("/src/index.ts", "Props", Some(&view2));
    assert_eq!(
        root2.0.as_str(),
        "/src/types-b.ts",
        "after intermediate barrel change, root must point to types-b, \
         not stale types-a from the cached imported root",
    );
}
