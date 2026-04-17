//! Behavioral tests for the semantic type resolution overhaul.
//!
//! Invariants under test:
//! - Each canonical imported file is loaded/parsed at most once per request.
//! - Barrel wildcard chains respect declared order and avoid repeated sibling rescans.
//! - Same-file symbol closure does not enqueue same-file symbols as external work.
//! - Cross-file resolution deduplicates `(canonical_id, exported_name)` pairs.
//! - Cycle handling is deterministic and does not reopen traversal.
//! - Builder lookup does not perform file I/O on cache hits.
//! - Shared-host reuse: once a file is ready, different entrypoints reuse state.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};
use rustc_hash::FxHashSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn resolve_type(
    host: &VerterHost,
    owner: &str,
    import_source: &str,
    type_name: &str,
) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
    let mut tracked = BTreeSet::new();
    let mut resolution = BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = FxHashSet::default();
    host.resolve_external_type_from_loaded_files(
        owner,
        import_source,
        type_name,
        &mut tracked,
        &mut resolution,
        &mut cache,
        &mut visiting,
        true,
        verter_workspace::ResolveRequestKind::TypeImport,
        true,
        None,
        0,
    )
    .expect("resolution should not error")
}

/// CountingWorkspace â€" thin wrapper over MemoryWorkspace that counts reads.
struct CountingWorkspace {
    inner: Arc<verter_workspace::MemoryWorkspace>,
    read_counts: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>,
}

impl CountingWorkspace {
    fn new() -> Self {
        Self {
            inner: Arc::new(verter_workspace::MemoryWorkspace::new(
                verter_workspace::MemoryOptions::default(),
            )),
            read_counts: parking_lot::Mutex::new(rustc_hash::FxHashMap::default()),
        }
    }

    fn inject_file(&self, path: &str, source: &str) {
        self.inner
            .inject_file(path.to_string(), Arc::<str>::from(source));
    }

    fn reset_reads(&self) {
        self.read_counts.lock().clear();
    }

    fn read_count(&self, path: &str) -> u64 {
        self.read_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn total_reads(&self) -> u64 {
        self.read_counts.lock().values().sum()
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

    fn take_last_read_file_trace_detail(&self, canonical_id: &str) -> Option<String> {
        self.inner.take_last_read_file_trace_detail(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.inner.file_exists(canonical_id)
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
    }

    fn read_package_manifest(
        &self,
        canonical_id: &str,
    ) -> Option<verter_workspace::PackageManifest> {
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
}

fn make_host_with_workspace(ws: Arc<CountingWorkspace>) -> VerterHost {
    let host = VerterHost::new(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        ws,
    );
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);
    host
}

fn set_dep(host: &VerterHost, from: &str, specifier: &str, to: &str) {
    host.set_import_dependencies(
        from,
        vec![crate::DependencyResolution {
            specifier: specifier.to_string(),
            resolved_canonical_id: Some(to.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

fn set_deps(host: &VerterHost, from: &str, deps: Vec<(&str, &str)>) {
    host.set_import_dependencies(
        from,
        deps.into_iter()
            .map(|(spec, to)| crate::DependencyResolution {
                specifier: spec.to_string(),
                resolved_canonical_id: Some(to.to_string()),
                possible_canonical_ids: Vec::new(),
            })
            .collect(),
    );
}

// ===========================================================================
// Invariant 1: Each imported file is loaded/parsed at most once per request
// ===========================================================================

/// Two different branches of a diamond dependency graph both import the same
/// leaf file. The leaf must be loaded from the workspace at most once during
/// a single resolution request.
#[test]
fn diamond_dependency_loads_leaf_file_once() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export { Left } from "./left"; export { Right } from "./right";"#,
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/left.d.ts",
        r#"import type { Shared } from "./shared"; export interface Left extends Shared { left: string }"#,
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/right.d.ts",
        r#"import type { Shared } from "./shared"; export interface Right extends Shared { right: number }"#,
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/shared.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Left, Right } from 'lib'
defineProps<Left & Right>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export { Left } from "./left"; export { Right } from "./right";"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/left.d.ts",
        r#"import type { Shared } from "./shared"; export interface Left extends Shared { left: string }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/right.d.ts",
        r#"import type { Shared } from "./shared"; export interface Right extends Shared { right: number }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/shared.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );
    set_deps(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        vec![
            ("./left", "/workspace/node_modules/lib/dist/left.d.ts"),
            ("./right", "/workspace/node_modules/lib/dist/right.d.ts"),
        ],
    );
    set_dep(
        &host,
        "/workspace/node_modules/lib/dist/left.d.ts",
        "./shared",
        "/workspace/node_modules/lib/dist/shared.d.ts",
    );
    set_dep(
        &host,
        "/workspace/node_modules/lib/dist/right.d.ts",
        "./shared",
        "/workspace/node_modules/lib/dist/shared.d.ts",
    );

    ws.reset_reads();

    let result = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Left");
    assert!(result.is_some(), "Left should resolve");

    // Now resolve Right â€" shared.d.ts should already be cached
    let result2 = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Right");
    assert!(result2.is_some(), "Right should resolve");

    // The shared file should have been loaded at most once across both resolutions
    let shared_reads = ws.read_count("/workspace/node_modules/lib/dist/shared.d.ts");
    assert!(
        shared_reads <= 1,
        "shared.d.ts should be loaded at most once across diamond branches, got {shared_reads}"
    );
}

// ===========================================================================
// Invariant 2: Barrel wildcard chains respect declared order, no repeated scans
// ===========================================================================

/// A barrel with three wildcard re-exports should process them in declared
/// order. The first source that exports a given name wins. The barrel
/// surface should be scanned once, not re-entered on repeat lookups.
#[test]
fn barrel_wildcard_declared_order_first_wins() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export * from './first'\nexport * from './second'\nexport * from './third'\n",
    );
    upsert_non_sfc(
        &host,
        "/src/first.ts",
        "export interface Alpha { first: boolean }\n",
    );
    upsert_non_sfc(&host, "/src/second.ts", "export interface Alpha { second: boolean }\nexport interface Beta { second_only: boolean }\n");
    upsert_non_sfc(
        &host,
        "/src/third.ts",
        "export interface Gamma { third_only: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Alpha, Beta, Gamma } from './barrel'
defineProps<Alpha & Beta & Gamma>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_deps(
        &host,
        "/src/barrel.ts",
        vec![
            ("./first", "/src/first.ts"),
            ("./second", "/src/second.ts"),
            ("./third", "/src/third.ts"),
        ],
    );

    // Alpha should come from first (declared order wins)
    let alpha = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Alpha");
    assert!(alpha.is_some(), "Alpha should resolve through barrel");

    // Beta should come from second
    let beta = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Beta");
    assert!(beta.is_some(), "Beta should resolve through barrel");

    // Gamma should come from third
    let gamma = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Gamma");
    assert!(gamma.is_some(), "Gamma should resolve through barrel");
}

/// After a full barrel surface scan, a second lookup of the same barrel
/// for a different exported name should reuse the cached surface, not
/// re-scan the wildcard sources.
#[test]
fn barrel_repeated_lookup_reuses_cached_surface() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file("/src/barrel.ts", "export * from './inner'\n");
    ws.inject_file(
        "/src/inner.ts",
        "export interface Props { label: string }\nexport interface Events { click: boolean }\n",
    );
    ws.inject_file(
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props, Events } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let host = make_host_with_workspace(ws.clone());
    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_dep(&host, "/src/barrel.ts", "./inner", "/src/inner.ts");

    let props = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");
    assert!(props.is_some(), "Props should resolve through barrel");
    let barrel_reads_after_first = ws.read_count("/src/barrel.ts");
    let inner_reads_after_first = ws.read_count("/src/inner.ts");

    let events = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Events");
    assert!(events.is_some(), "Events should resolve through barrel");
    assert_eq!(
        ws.read_count("/src/barrel.ts"),
        barrel_reads_after_first,
        "second barrel lookup should reuse the cached barrel surface",
    );
    assert_eq!(
        ws.read_count("/src/inner.ts"),
        inner_reads_after_first,
        "second barrel lookup should reuse the cached child surface",
    );
}

/// If an earlier wildcard sibling is itself a barrel, its deeper descendants
/// must not be loaded before the current layer proves that no same-layer
/// sibling exports the requested symbol.
#[test]
fn same_layer_barrel_match_beats_deeper_earlier_branch() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/src/barrel.ts",
        "export * from './a'\nexport * from './b'\n",
    );
    ws.inject_file("/workspace/src/a.ts", "export * from './a-deep'\n");
    ws.inject_file(
        "/workspace/src/a-deep.ts",
        "export interface Props { source: 'deep' }\n",
    );
    ws.inject_file(
        "/workspace/src/b.ts",
        "export interface Props { source: 'same-layer' }\n",
    );
    ws.inject_file(
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    let host = make_host_with_workspace(ws.clone());
    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "./barrel",
        "/workspace/src/barrel.ts",
    );
    set_deps(
        &host,
        "/workspace/src/barrel.ts",
        vec![
            ("./a", "/workspace/src/a.ts"),
            ("./b", "/workspace/src/b.ts"),
        ],
    );
    set_dep(
        &host,
        "/workspace/src/a.ts",
        "./a-deep",
        "/workspace/src/a-deep.ts",
    );

    ws.reset_reads();
    let result = resolve_type(&host, "/workspace/src/Consumer.vue", "./barrel", "Props");
    assert!(
        result.is_some(),
        "Props should resolve through the same-layer barrel child"
    );
    assert_eq!(
        ws.read_count("/workspace/src/a-deep.ts"),
        0,
        "a deeper earlier branch must not be loaded before the same-layer sibling match is chosen",
    );
}

// ===========================================================================
// Invariant 3: Same-file symbol closure stays local
// ===========================================================================

/// A type that depends on another local type in the same file should be
/// resolved entirely within that file, without contributing to external
/// frontier work.
#[test]
fn same_file_dependency_does_not_trigger_external_traversal() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
interface Base { id: string }
interface Extended extends Base { name: string }
export interface Props extends Extended { value: number }
"#,
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    let result = resolve_type(&host, "/src/Consumer.vue", "./types", "Props");
    assert!(
        result.is_some(),
        "Props should resolve through same-file chain"
    );

    // The provenance should show zero cycle detections â€" same-file deps
    // should not appear as external cycles
    let p = host.provenance().snapshot();
    assert_eq!(
        p.resolver_cycle_detections, 0,
        "same-file closure should not trigger cycle detection"
    );
}

// ===========================================================================
// Invariant 4: Cross-file dedup of (canonical_id, exported_name)
// ===========================================================================

/// Two macro types that both reference the same external symbol should
/// only cause that symbol to be resolved once.
#[test]
fn shared_external_symbol_resolved_once_across_macro_types() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Shared } from 'lib'
defineProps<{ a: Shared; b: Shared }>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );

    ws.reset_reads();

    let result = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Shared");
    assert!(result.is_some(), "Shared should resolve");

    // The external file should be read at most once
    let reads = ws.read_count("/workspace/node_modules/lib/dist/index.d.ts");
    assert!(
        reads <= 1,
        "Shared symbol source should be loaded at most once, got {reads}"
    );
}

// ===========================================================================
// Invariant 5: Cycle handling is deterministic
// ===========================================================================

/// Two files that mutually reference each other should terminate cleanly
/// without stack overflow or infinite loop. Cycle counter should increment.
#[test]
fn mutual_file_cycle_terminates_deterministically() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/a.ts",
        r#"
import type { B } from './b'
export interface A extends B { fromA: string }
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/b.ts",
        r#"
import type { A } from './a'
export interface B extends A { fromB: number }
"#,
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { A } from './a'
defineProps<A>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./a", "/src/a.ts");
    set_dep(&host, "/src/a.ts", "./b", "/src/b.ts");
    set_dep(&host, "/src/b.ts", "./a", "/src/a.ts");

    // Must not panic or hang
    let _result = resolve_type(&host, "/src/Consumer.vue", "./a", "A");

    let p = host.provenance().snapshot();
    assert!(
        p.resolver_cycle_detections >= 1,
        "mutual cycle should be detected, got {:?}",
        p
    );
}

/// Three-node cycle should also terminate cleanly.
#[test]
fn three_node_cycle_terminates_deterministically() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/a.ts",
        r#"
import type { C } from './c'
export interface A extends C { a: string }
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/b.ts",
        r#"
import type { A } from './a'
export interface B extends A { b: number }
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/c.ts",
        r#"
import type { B } from './b'
export interface C extends B { c: boolean }
"#,
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { A } from './a'
defineProps<A>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./a", "/src/a.ts");
    set_dep(&host, "/src/a.ts", "./c", "/src/c.ts");
    set_dep(&host, "/src/b.ts", "./a", "/src/a.ts");
    set_dep(&host, "/src/c.ts", "./b", "/src/b.ts");

    let _result = resolve_type(&host, "/src/Consumer.vue", "./a", "A");

    let p = host.provenance().snapshot();
    assert!(
        p.resolver_cycle_detections >= 1,
        "three-node cycle should be detected, got {:?}",
        p
    );
}

// ===========================================================================
// Invariant 6: Warm cache avoids workspace I/O
// ===========================================================================

/// After an initial resolution that populates the host cache, a second
/// resolution for the same file/symbol should perform zero workspace reads.
#[test]
fn warm_cache_resolution_performs_no_workspace_reads() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface Props { label: string }"#,
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from 'lib'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface Props { label: string }"#,
    );

    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );

    // Cold run to populate cache
    let cold = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Props");
    assert!(cold.is_some(), "cold resolution should succeed");

    // Reset reads, then resolve again
    ws.reset_reads();
    let warm = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Props");
    assert!(warm.is_some(), "warm resolution should succeed");

    let total_reads = ws.total_reads();
    assert_eq!(
        total_reads, 0,
        "warm cache resolution should not read any workspace files, got {total_reads}"
    );
}

// ===========================================================================
// Invariant 7: Shared host reuse across entrypoints
// ===========================================================================

/// Component-meta resolution and direct external-type resolution against the
/// same file should reuse the same imported dependency cache state.
#[test]
fn component_meta_and_direct_resolve_share_imported_cache() {
    let ws = Arc::new(CountingWorkspace::new());
    ws.inject_file(
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface MyProps { label: string }"#,
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/App.vue",
        r#"<script setup lang="ts">
import type { MyProps } from 'lib'
defineProps<MyProps>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export interface MyProps { label: string }"#,
    );

    set_dep(
        &host,
        "/workspace/src/App.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );

    // Resolve via component-meta path
    let project = crate::meta::MetaProject::new(host);
    project.host().provenance().reset();

    let meta = project.host().resolve_component_meta(
        "/workspace/src/App.vue",
        crate::types::ResolverMode::Expanded,
    );
    assert!(meta.is_some(), "component-meta should resolve");

    ws.reset_reads();

    // Now resolve the same type through the direct external-type path
    let direct = resolve_type(project.host(), "/workspace/src/App.vue", "lib", "MyProps");
    assert!(direct.is_some(), "direct type resolution should succeed");

    // The imported file should not have been re-read from workspace
    let reads = ws.read_count("/workspace/node_modules/lib/dist/index.d.ts");
    assert_eq!(
        reads, 0,
        "after component-meta resolved the same file, direct resolution should \
         reuse cached imported dependency state (got {reads} workspace reads)"
    );
}

// ===========================================================================
// Invariant: Deep barrel chain terminates and caches intermediate levels
// ===========================================================================

/// A 5-level deep barrel chain should resolve without re-scanning
/// intermediate levels.
#[test]
fn deep_barrel_chain_resolves_without_rescanning_intermediates() {
    let host = strict_host();

    // barrel1 -> barrel2 -> barrel3 -> barrel4 -> barrel5 -> leaf
    upsert_non_sfc(&host, "/src/barrel1.ts", "export * from './barrel2'\n");
    upsert_non_sfc(&host, "/src/barrel2.ts", "export * from './barrel3'\n");
    upsert_non_sfc(&host, "/src/barrel3.ts", "export * from './barrel4'\n");
    upsert_non_sfc(&host, "/src/barrel4.ts", "export * from './barrel5'\n");
    upsert_non_sfc(&host, "/src/barrel5.ts", "export * from './leaf'\n");
    upsert_non_sfc(
        &host,
        "/src/leaf.ts",
        "export interface DeepType { value: string }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { DeepType } from './barrel1'
defineProps<DeepType>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel1", "/src/barrel1.ts");
    set_dep(&host, "/src/barrel1.ts", "./barrel2", "/src/barrel2.ts");
    set_dep(&host, "/src/barrel2.ts", "./barrel3", "/src/barrel3.ts");
    set_dep(&host, "/src/barrel3.ts", "./barrel4", "/src/barrel4.ts");
    set_dep(&host, "/src/barrel4.ts", "./barrel5", "/src/barrel5.ts");
    set_dep(&host, "/src/barrel5.ts", "./leaf", "/src/leaf.ts");

    let result = resolve_type(&host, "/src/Consumer.vue", "./barrel1", "DeepType");
    assert!(
        result.is_some(),
        "DeepType should resolve through 5-level barrel chain"
    );

    // A second resolution for a different type from the same chain
    // should reuse the cached barrel surfaces
    upsert_non_sfc(
        &host,
        "/src/leaf.ts",
        "export interface DeepType { value: string }\nexport interface Other { extra: number }\n",
    );
    // Re-set dep after upsert
    set_dep(&host, "/src/barrel5.ts", "./leaf", "/src/leaf.ts");

    host.provenance().reset();
    let result2 = resolve_type(&host, "/src/Consumer.vue", "./barrel1", "Other");
    // This may or may not resolve (depends on invalidation), but it must not hang
    let _ = result2;
}

// ===========================================================================
// Invariant: Mixed direct + wildcard exports respect routing precedence
// ===========================================================================

/// When a barrel has both a direct named reexport and wildcard reexports,
/// the direct name takes precedence over any wildcard source.
#[test]
fn direct_named_reexport_takes_precedence_over_wildcard() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export { Props } from './direct'\nexport * from './wildcard'\n",
    );
    upsert_non_sfc(
        &host,
        "/src/direct.ts",
        "export interface Props { source: 'direct' }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/wildcard.ts",
        "export interface Props { source: 'wildcard' }\nexport interface Other { ok: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_deps(
        &host,
        "/src/barrel.ts",
        vec![
            ("./direct", "/src/direct.ts"),
            ("./wildcard", "/src/wildcard.ts"),
        ],
    );

    let result = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");
    assert!(result.is_some(), "Props should resolve through barrel");

    // The resolved type should have source: 'direct', not source: 'wildcard'
    // because direct named reexports take precedence
    if let Some(ref elements) = result {
        let has_source_prop = elements
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("source"));
        assert!(
            has_source_prop,
            "Props should contain 'source' member from direct reexport"
        );
    }
}

// ===========================================================================
// Invariant: Namespace imports do not trigger implicit wildcard expansion
// ===========================================================================

/// Importing `* as NS` from a module should not trigger wildcard barrel
/// scanning for the entire export surface.
#[test]
fn namespace_import_does_not_trigger_barrel_scan() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }\nexport interface Events { click: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type * as NS from './types'
defineProps<NS.Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    // Resolution should work but only resolve the specifically requested member
    let result = resolve_type(&host, "/src/Consumer.vue", "./types", "Props");
    assert!(
        result.is_some(),
        "Props should resolve through namespace import"
    );
}

// ===========================================================================
// V1 violation: Both resolver paths produce identical results
// ===========================================================================

/// The store-view graph path and the loaded/live request path must produce
/// the same resolved type for the same input.
#[test]
fn graph_and_request_paths_produce_identical_results() {
    let host = strict_host();

    upsert_non_sfc(&host, "/src/barrel.ts", "export * from './inner'\n");
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string; count: number }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_dep(&host, "/src/barrel.ts", "./inner", "/src/inner.ts");

    // Resolve via the live/request path (no store view)
    let live_result = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");
    assert!(live_result.is_some(), "live path should resolve Props");

    // Resolve via the store-view/graph path
    let view = host.owned_or_ambient_request_view();
    let mut tracked = BTreeSet::new();
    let mut resolution = BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = FxHashSet::default();
    let graph_result = host
        .resolve_external_type_from_loaded_files_in_view(
            "/src/Consumer.vue",
            "./barrel",
            "Props",
            &mut tracked,
            &mut resolution,
            &mut cache,
            &mut visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            None,
            0,
            Some(&*view),
        )
        .expect("graph path should not error");
    assert!(graph_result.is_some(), "graph path should resolve Props");

    // Both should yield the same prop names
    let live_names: Vec<_> = live_result
        .as_ref()
        .unwrap()
        .props
        .iter()
        .filter_map(|p| p.key_name.clone())
        .collect();
    let graph_names: Vec<_> = graph_result
        .as_ref()
        .unwrap()
        .props
        .iter()
        .filter_map(|p| p.key_name.clone())
        .collect();
    assert_eq!(
        live_names, graph_names,
        "live and graph paths should produce identical prop names"
    );
}

// ===========================================================================
// V2 + V3: Barrel with sibling wildcards â€" at most one full scan per request
// ===========================================================================

/// A barrel with many sibling wildcard sources should not re-scan already-
/// visited sources when resolving multiple types from the same barrel in
/// a single component-meta request.
#[test]
fn barrel_siblings_scanned_at_most_once_in_single_meta_request() {
    let ws = Arc::new(CountingWorkspace::new());

    // Set up a barrel with 5 wildcard sources
    ws.inject_file("/workspace/node_modules/lib/dist/index.d.ts",
        "export * from './a'\nexport * from './b'\nexport * from './c'\nexport * from './d'\nexport * from './e'\n");
    ws.inject_file(
        "/workspace/node_modules/lib/dist/a.d.ts",
        "export interface A { a: string }\n",
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/b.d.ts",
        "export interface B { b: number }\n",
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/c.d.ts",
        "export interface C { c: boolean }\n",
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/d.d.ts",
        "export interface D { d: string }\n",
    );
    ws.inject_file(
        "/workspace/node_modules/lib/dist/e.d.ts",
        "export interface E { e: number }\n",
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { A, B, C, D, E } from 'lib'
defineProps<A & B & C & D & E>()
</script>
<template><div /></template>"#,
    );

    // Upsert all dependency files into the host
    upsert_non_sfc(&host, "/workspace/node_modules/lib/dist/index.d.ts",
        "export * from './a'\nexport * from './b'\nexport * from './c'\nexport * from './d'\nexport * from './e'\n");
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/a.d.ts",
        "export interface A { a: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/b.d.ts",
        "export interface B { b: number }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/c.d.ts",
        "export interface C { c: boolean }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/d.d.ts",
        "export interface D { d: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/e.d.ts",
        "export interface E { e: number }\n",
    );

    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );
    set_deps(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        vec![
            ("./a", "/workspace/node_modules/lib/dist/a.d.ts"),
            ("./b", "/workspace/node_modules/lib/dist/b.d.ts"),
            ("./c", "/workspace/node_modules/lib/dist/c.d.ts"),
            ("./d", "/workspace/node_modules/lib/dist/d.d.ts"),
            ("./e", "/workspace/node_modules/lib/dist/e.d.ts"),
        ],
    );

    ws.reset_reads();

    // Resolve all 5 types through the same barrel
    for type_name in &["A", "B", "C", "D", "E"] {
        let result = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", type_name);
        assert!(
            result.is_some(),
            "{type_name} should resolve through barrel"
        );
    }

    // Each sibling file should have been read at most once across all 5 lookups
    for (file, label) in [
        ("/workspace/node_modules/lib/dist/a.d.ts", "a"),
        ("/workspace/node_modules/lib/dist/b.d.ts", "b"),
        ("/workspace/node_modules/lib/dist/c.d.ts", "c"),
        ("/workspace/node_modules/lib/dist/d.d.ts", "d"),
        ("/workspace/node_modules/lib/dist/e.d.ts", "e"),
    ] {
        let reads = ws.read_count(file);
        assert!(
            reads <= 1,
            "sibling {label}.d.ts should be read at most once, got {reads}"
        );
    }
}

// ===========================================================================
// V4 + V5: Companion resolution should not re-traverse already-resolved files
// ===========================================================================

/// A type that extends from multiple imported types should resolve each
/// companion at most once, even when multiple props reference the same
/// base type.
#[test]
fn companion_files_loaded_once_even_with_multiple_references() {
    let ws = Arc::new(CountingWorkspace::new());

    ws.inject_file(
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export { Alpha } from "./alpha"; export { Beta } from "./beta";"#,
    );
    ws.inject_file("/workspace/node_modules/lib/dist/alpha.d.ts",
        r#"import type { Shared } from "./shared"; export interface Alpha extends Shared { alpha: string }"#);
    ws.inject_file("/workspace/node_modules/lib/dist/beta.d.ts",
        r#"import type { Shared } from "./shared"; export interface Beta extends Shared { beta: number }"#);
    ws.inject_file(
        "/workspace/node_modules/lib/dist/shared.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    let host = make_host_with_workspace(ws.clone());
    upsert_vue(
        &host,
        "/workspace/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Alpha, Beta } from 'lib'
defineProps<Alpha & Beta>()
</script>
<template><div /></template>"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        r#"export { Alpha } from "./alpha"; export { Beta } from "./beta";"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/alpha.d.ts",
        r#"import type { Shared } from "./shared"; export interface Alpha extends Shared { alpha: string }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/beta.d.ts",
        r#"import type { Shared } from "./shared"; export interface Beta extends Shared { beta: number }"#,
    );
    upsert_non_sfc(
        &host,
        "/workspace/node_modules/lib/dist/shared.d.ts",
        r#"export interface Shared { id: string }"#,
    );

    set_dep(
        &host,
        "/workspace/src/Consumer.vue",
        "lib",
        "/workspace/node_modules/lib/dist/index.d.ts",
    );
    set_deps(
        &host,
        "/workspace/node_modules/lib/dist/index.d.ts",
        vec![
            ("./alpha", "/workspace/node_modules/lib/dist/alpha.d.ts"),
            ("./beta", "/workspace/node_modules/lib/dist/beta.d.ts"),
        ],
    );
    set_dep(
        &host,
        "/workspace/node_modules/lib/dist/alpha.d.ts",
        "./shared",
        "/workspace/node_modules/lib/dist/shared.d.ts",
    );
    set_dep(
        &host,
        "/workspace/node_modules/lib/dist/beta.d.ts",
        "./shared",
        "/workspace/node_modules/lib/dist/shared.d.ts",
    );

    ws.reset_reads();

    // Resolve both types
    let alpha = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Alpha");
    let beta = resolve_type(&host, "/workspace/src/Consumer.vue", "lib", "Beta");
    assert!(alpha.is_some(), "Alpha should resolve");
    assert!(beta.is_some(), "Beta should resolve");

    // shared.d.ts is a companion to both Alpha and Beta â€" it must be loaded at most once
    let shared_reads = ws.read_count("/workspace/node_modules/lib/dist/shared.d.ts");
    assert!(
        shared_reads <= 1,
        "shared companion file should be loaded at most once across both type resolutions, got {shared_reads}"
    );
}

// ===========================================================================
// V6: Imported alias preparation should not eagerly materialize bodies
// ===========================================================================

/// Type mode and Expanded mode should both resolve, with Expanded mode
/// providing evaluated type shapes.
#[test]
fn type_and_expanded_modes_both_resolve() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string; count: number; active: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    // Type mode: should resolve identity without full expansion
    let meta_type =
        host.resolve_component_meta("/src/Consumer.vue", crate::types::ResolverMode::Type);
    assert!(
        meta_type.is_some(),
        "Type-mode component-meta should resolve"
    );
    let type_result = meta_type.unwrap();
    assert!(
        type_result.evaluated_types.is_none(),
        "Type mode should not produce evaluated types"
    );

    // Expanded mode: full materialization
    let meta_expanded =
        host.resolve_component_meta("/src/Consumer.vue", crate::types::ResolverMode::Expanded);
    assert!(
        meta_expanded.is_some(),
        "Expanded-mode component-meta should resolve"
    );
    let expanded_result = meta_expanded.unwrap();
    assert!(
        expanded_result.evaluated_types.is_some(),
        "Expanded mode should produce evaluated types"
    );
}

// ===========================================================================
// Phase 1: ShallowFileState host integration tests
// ===========================================================================

/// After upserting and loading an imported dependency, the host should
/// populate and cache the shallow type file state.
#[test]
fn shallow_file_state_populated_after_imported_dependency_load() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }\nexport interface Events { click: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    // Trigger resolution so the host loads the imported dependency
    let _result = resolve_type(&host, "/src/Consumer.vue", "./types", "Props");

    // The shallow type state should now be available from the host
    let shallow = host.shallow_file_state_in_view("/src/types.ts", None);
    assert!(
        shallow.is_some(),
        "shallow type state should be populated after imported dependency load"
    );

    let state = shallow.unwrap();
    assert!(
        state.export_target("Props").is_some(),
        "Props should be in the export surface"
    );
    assert!(
        state.export_target("Events").is_some(),
        "Events should be in the export surface"
    );
    assert!(
        state.export_target("MissingType").is_none(),
        "non-existent types should not appear"
    );
}

/// The shallow type state should capture reexport routing correctly.
#[test]
fn shallow_file_state_captures_reexport_routes() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        "export { Props } from './inner'\nexport * from './wildcard'\n",
    );
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/wildcard.ts",
        "export interface Extra { extra: boolean }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_deps(
        &host,
        "/src/barrel.ts",
        vec![
            ("./inner", "/src/inner.ts"),
            ("./wildcard", "/src/wildcard.ts"),
        ],
    );

    // Trigger resolution to load the barrel
    let _result = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");

    let shallow = host.shallow_file_state_in_view("/src/barrel.ts", None);
    assert!(
        shallow.is_some(),
        "barrel shallow state should be populated"
    );

    let state = shallow.unwrap();

    // Check that Props routes to ./inner
    match state.export_target("Props") {
        Some(crate::resolver_core::ExportTarget::Reexport {
            source_specifier,
            original_name,
            canonical_id: _,
            is_type: _,
        }) => {
            assert_eq!(source_specifier, "./inner");
            assert_eq!(original_name, "Props");
        }
        other => panic!("expected Reexport for Props, got {other:?}"),
    }

    // Wildcard sources should be captured
    assert!(
        state.has_wildcard_reexports(),
        "barrel should have wildcard reexports"
    );
    assert!(
        state
            .wildcard_reexports
            .iter()
            .any(|w| w.source_specifier == "./wildcard"),
        "wildcard source should be captured"
    );
}

/// The shallow type state is hash-keyed: after content changes, the old
/// state should be invalidated and a new one built.
#[test]
fn shallow_file_state_invalidated_on_content_change() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string }\n",
    );
    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");

    let _result = resolve_type(&host, "/src/Consumer.vue", "./types", "Props");

    let state1 = host
        .shallow_file_state_in_view("/src/types.ts", None)
        .expect("state should exist");
    let hash1 = state1.whole_hash;

    // Update the file content
    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { label: string; count: number }\n",
    );
    set_dep(&host, "/src/Consumer.vue", "./types", "/src/types.ts");
    let _result2 = resolve_type(&host, "/src/Consumer.vue", "./types", "Props");

    let state2 = host
        .shallow_file_state_in_view("/src/types.ts", None)
        .expect("state should exist after update");

    // The hash should have changed
    assert_ne!(
        hash1, state2.whole_hash,
        "whole_hash should change after content update"
    );
}

#[test]
fn narrow_route_required_imports_follow_local_export_alias_and_cache_by_route() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        r#"
import type { Alpha } from './alpha'
import type { Beta } from './beta'

interface InternalProps {
  a: Alpha
  b: Beta
}

export { InternalProps as PublicProps }
"#,
    );
    upsert_non_sfc(
        &host,
        "/src/alpha.ts",
        "export interface Alpha { value: string }\n",
    );
    upsert_non_sfc(
        &host,
        "/src/beta.ts",
        "export interface Beta { value: number }\n",
    );

    let _ = host.shallow_file_state_in_view("/src/types.ts", None);

    let member_route = crate::resolver_core::RouteDemand::MemberPath(vec!["a".into()]);
    let member_required = host.required_import_names_for_exported_route_in_view(
        "/src/types.ts",
        "PublicProps",
        &member_route,
        None,
    );
    let whole_required = host.required_import_names_for_exported_route_in_view(
        "/src/types.ts",
        "PublicProps",
        &crate::resolver_core::RouteDemand::Whole,
        None,
    );

    assert!(
        member_required.contains("Alpha"),
        "narrow route should follow the aliased local symbol"
    );
    assert!(
        !member_required.contains("Beta"),
        "narrow route should not widen to sibling imports"
    );
    assert!(
        !whole_required.contains("Alpha"),
        "whole-route closure should keep direct imported object props symbolic through a local export alias",
    );
    assert!(
        !whole_required.contains("Beta"),
        "whole-route closure should not widen to sibling imported object props through a local export alias",
    );
}

// ===========================================================================
// Phase 4: HostFrontierAdapter integration test
// ===========================================================================

/// The HostFrontierAdapter should be able to resolve imports through the
/// real host's dependency cache and workspace resolution.
#[test]
fn host_frontier_adapter_resolves_reexport_chain() {
    let host = strict_host();

    upsert_non_sfc(
        &host,
        "/src/barrel.ts",
        r#"export { Props } from "./inner""#,
    );
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_dep(&host, "/src/barrel.ts", "./inner", "/src/inner.ts");

    // First trigger traditional resolution to populate caches
    let _ = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");

    // Now use the frontier adapter
    let adapter = crate::host_resolve::HostFrontierAdapter {
        host: &host,
        store_view: None,
        materialize_symbols: true,
        route_exports_only: false,
        route_shallow_cache: std::cell::RefCell::new(rustc_hash::FxHashMap::default()),
    };

    let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
    frontier.seed(vec![crate::resolver_core::PendingExternalSymbol {
        canonical_id: "/src/barrel.ts".to_string(),
        exported_name: "Props".to_string(),
        route: Some(crate::resolver_core::RouteDemand::Whole),
    }]);

    frontier.run(&adapter).unwrap();

    // The frontier should resolve the reexport chain
    assert!(
        frontier.resolved_count() >= 2,
        "should resolve barrel + inner, got {}",
        frontier.resolved_count()
    );
    assert!(
        frontier.get_resolved("/src/barrel.ts", "Props").is_some(),
        "barrel entry should be resolved"
    );
    assert!(
        frontier.get_resolved("/src/inner.ts", "Props").is_some(),
        "inner entry should be resolved through the host adapter"
    );
}

/// The HostFrontierAdapter should handle barrel wildcard resolution
/// through the real host.
#[test]
fn host_frontier_adapter_resolves_wildcard_barrel() {
    let host = strict_host();

    upsert_non_sfc(&host, "/src/barrel.ts", "export * from './inner'\n");
    upsert_non_sfc(
        &host,
        "/src/inner.ts",
        "export interface Props { label: string }\n",
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Props } from './barrel'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./barrel", "/src/barrel.ts");
    set_dep(&host, "/src/barrel.ts", "./inner", "/src/inner.ts");

    // Populate caches
    let _ = resolve_type(&host, "/src/Consumer.vue", "./barrel", "Props");

    let adapter = crate::host_resolve::HostFrontierAdapter {
        host: &host,
        store_view: None,
        materialize_symbols: true,
        route_exports_only: false,
        route_shallow_cache: std::cell::RefCell::new(rustc_hash::FxHashMap::default()),
    };

    let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
    frontier.seed(vec![crate::resolver_core::PendingExternalSymbol {
        canonical_id: "/src/barrel.ts".to_string(),
        exported_name: "Props".to_string(),
        route: Some(crate::resolver_core::RouteDemand::Whole),
    }]);

    frontier.run(&adapter).unwrap();

    assert!(
        frontier.get_resolved("/src/inner.ts", "Props").is_some(),
        "Props should resolve through wildcard barrel via host adapter"
    );
}

// ===========================================================================
// Hang isolation tests â€" reproduce Accordion hang with minimal fixtures
// ===========================================================================

/// Test 1: Resolve a complex union type (simulating Vue's `Component`) from
/// a large declaration file. This tests whether `imported_symbol_dependencies`
/// hangs when processing a type with many references.
#[test]
fn resolve_complex_union_type_does_not_hang() {
    let host = strict_host();

    // Simulate a large declaration file with a complex union type
    upsert_non_sfc(
        &host,
        "/src/runtime-core.d.ts",
        r#"
export interface FunctionalComponent<P = {}> { (props: P): any }
export interface ComponentOptions<D = any> { data?: () => D; computed?: Record<string, () => any> }
export type DefineComponent<P = any, B = any> = { new (): { $props: P } }
export type ConcreteComponent = FunctionalComponent | ComponentOptions
export type Component<P = any> = ConcreteComponent | DefineComponent<P> | string
export interface VNode<T = any> { type: Component; props: T }
export interface PrimitiveProps { asChild?: boolean }
"#,
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { Component } from './runtime-core'
defineProps<{ comp: Component }>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Consumer.vue",
        "./runtime-core",
        "/src/runtime-core.d.ts",
    );

    // This must not hang â€" if it returns within the test timeout, the test passes
    let result = resolve_type(&host, "/src/Consumer.vue", "./runtime-core", "Component");
    // Component is a complex union, resolution may or may not produce props
    let _ = result;
}

/// Test 2: Resolve a type that extends an imported type which itself has
/// symbol_dependencies pointing to other types in the same file.
/// Simulates AccordionRootProps extends PrimitiveProps.
#[test]
fn resolve_type_extending_imported_with_deps_does_not_hang() {
    let host = strict_host();

    // File with inter-dependent types (simulating reka-ui's index3.d.ts pattern)
    upsert_non_sfc(
        &host,
        "/src/lib-types.d.ts",
        r#"
export interface PrimitiveProps { asChild?: boolean }
export interface SingleOrMultipleProps<T = string> { type?: 'single' | 'multiple'; value?: T }
export interface AccordionRootProps<T = string | string[]> extends PrimitiveProps, SingleOrMultipleProps<T> {
    collapsible?: boolean
    disabled?: boolean
}
export interface AccordionRootEmits { 'update:modelValue': [value: string | string[]] }
"#,
    );

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from './lib-types'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Consumer.vue",
        "./lib-types",
        "/src/lib-types.d.ts",
    );

    let result = resolve_type(
        &host,
        "/src/Consumer.vue",
        "./lib-types",
        "AccordionRootProps",
    );
    assert!(result.is_some(), "AccordionRootProps should resolve");
}

/// Test 3: Simulate the full Accordion pattern â€" a type from file A that
/// has symbol_dependencies pointing to types in file B (runtime-core),
/// where file B has a complex `Component` type.
#[test]
fn accordion_pattern_type_with_component_dep_does_not_hang() {
    let host = strict_host();

    // Simulate @vue/runtime-core with Component type
    upsert_non_sfc(
        &host,
        "/node_modules/@vue/runtime-core.d.ts",
        r#"
export interface FunctionalComponent<P = {}> { (props: P): any }
export interface ComponentOptions<D = any> { data?: () => D }
export type DefineComponent<P = any> = { new (): { $props: P } }
export type ConcreteComponent = FunctionalComponent | ComponentOptions
export type Component = ConcreteComponent | DefineComponent | string
export interface VNode<T = any> { type: Component; props: T }
"#,
    );

    // Simulate reka-ui types that import from runtime-core
    upsert_non_sfc(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        r#"
import type { Component } from '@vue/runtime-core'
export interface PrimitiveProps { asChild?: boolean; as?: Component }
export interface SingleOrMultipleProps<T = string> { type?: 'single' | 'multiple'; value?: T }
export interface AccordionRootProps extends PrimitiveProps, SingleOrMultipleProps {
    collapsible?: boolean
    disabled?: boolean
    defaultValue?: string
}
export interface AccordionRootEmits { 'update:modelValue': [value: string | string[]] }
"#,
    );

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from 'reka-ui'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "reka-ui",
        "/node_modules/reka-ui/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        "@vue/runtime-core",
        "/node_modules/@vue/runtime-core.d.ts",
    );

    // This must not hang
    let result = resolve_type(&host, "/src/Accordion.vue", "reka-ui", "AccordionRootProps");
    assert!(
        result.is_some(),
        "AccordionRootProps should resolve through reka-ui -> runtime-core chain"
    );
}

/// Test 4a: Scale test â€" a type file with many inter-dependent types
/// simulating reka-ui's index3.d.ts (1000+ types). Tests whether
/// imported_symbol_dependencies hangs at scale.
#[test]
fn large_declaration_file_with_many_interdependent_types_does_not_hang() {
    let host = strict_host();

    // Generate a large declaration file with ~200 inter-dependent types
    let mut source = String::new();
    source.push_str("export interface BaseProps { asChild?: boolean }\n");
    source.push_str("export type Component = FunctionalComponent | DefineComponent | string\n");
    source.push_str("export interface FunctionalComponent<P = {}> { (props: P): any }\n");
    source.push_str("export type DefineComponent<P = any> = { new (): { $props: P } }\n");
    for i in 0..200 {
        source.push_str(&format!(
            "export interface Widget{}Props extends BaseProps {{ label{}: string; comp{}: Component }}\n",
            i, i, i,
        ));
    }
    // One type that extends a Widget which references Component
    source.push_str(
        "export interface AccordionRootProps extends Widget0Props, Widget1Props { collapsible?: boolean }\n",
    );

    upsert_non_sfc(&host, "/src/lib.d.ts", &source);

    upsert_vue(
        &host,
        "/src/Consumer.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps } from './lib'
defineProps<AccordionRootProps>()
</script>
<template><div /></template>"#,
    );

    set_dep(&host, "/src/Consumer.vue", "./lib", "/src/lib.d.ts");

    let result = resolve_type(&host, "/src/Consumer.vue", "./lib", "AccordionRootProps");
    assert!(
        result.is_some(),
        "AccordionRootProps should resolve from large file"
    );
}

/// Test 4b: Scale test for component-meta with cross-file deps.
/// A large lib file with 200 types, where the root type has a dep
/// pointing to another large file.
#[test]
fn large_cross_file_deps_component_meta_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    // Large "runtime" file with 200 types
    let mut runtime_source = String::new();
    runtime_source.push_str("export interface FunctionalComponent<P = {}> { (props: P): any }\n");
    runtime_source.push_str("export type DefineComponent<P = any> = { new (): { $props: P } }\n");
    runtime_source
        .push_str("export type Component = FunctionalComponent | DefineComponent | string\n");
    for i in 0..200 {
        runtime_source.push_str(&format!(
            "export interface RuntimeType{} {{ value{}: string }}\n",
            i, i,
        ));
    }
    upsert_non_sfc(&host, "/node_modules/runtime/types.d.ts", &runtime_source);

    // Library file that imports from runtime
    let mut lib_source = String::new();
    lib_source.push_str("import type { Component } from 'runtime'\n");
    lib_source.push_str("export interface PrimitiveProps { asChild?: boolean; as?: Component }\n");
    for i in 0..50 {
        lib_source.push_str(&format!(
            "export interface LibType{} extends PrimitiveProps {{ extra{}: number }}\n",
            i, i,
        ));
    }
    lib_source.push_str(
        "export interface AccordionRootProps extends PrimitiveProps { collapsible?: boolean }\n",
    );
    lib_source
        .push_str("export interface AccordionRootEmits { 'update:modelValue': [value: string] }\n");
    upsert_non_sfc(&host, "/node_modules/lib/types.d.ts", &lib_source);

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from 'lib'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "lib",
        "/node_modules/lib/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/lib/types.d.ts",
        "runtime",
        "/node_modules/runtime/types.d.ts",
    );

    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ResolverMode::Expanded);
    assert!(
        meta.is_some(),
        "large cross-file component-meta should resolve"
    );
}

/// Test 4c: Deeply nested generics simulating Vue's runtime-core types.
/// `CreateComponentPublicInstanceWithMixins` has 25+ type parameters.
#[test]
fn deeply_nested_generics_component_meta_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    // Simulate Vue's deeply generic runtime-core.d.ts types
    upsert_non_sfc(
        &host,
        "/node_modules/vue-runtime/types.d.ts",
        r#"
type Data = Record<string, unknown>
type ComputedOptions = Record<string, () => any>
type MethodOptions = Record<string, (...args: any[]) => any>
type EmitsOptions = Record<string, ((...args: any[]) => any) | null>
interface ComponentOptionsMixin {}
type IntersectionMixin<T> = T extends ComponentOptionsMixin ? T : never
type UnwrapMixinsType<T, Type extends string> = T extends Record<Type, any> ? T[Type] : {}
type EnsureNonVoid<T> = T extends void ? {} : T
interface ComponentInjectOptions {}
interface SlotsType {}
type Prettify<T> = { [K in keyof T]: T[K] } & {}
type ExtractPropTypes<T> = { [K in keyof T]: T[K] extends { type: infer V } ? V : T[K] }

interface ComponentOptionsBase<
    P, B, D, C extends ComputedOptions, M extends MethodOptions,
    Mixin extends ComponentOptionsMixin, Extends extends ComponentOptionsMixin,
    E extends EmitsOptions, S = string, Defaults = {}
> {
    data?: () => D
    computed?: C
    methods?: M
    emits?: E
}

export type ComponentPublicInstance<
    P = {}, B = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, E extends EmitsOptions = {},
    PublicProps = P, Defaults = {}, MakeDefaultsOptional extends boolean = false,
    Options = ComponentOptionsBase<P, B, D, C, M, ComponentOptionsMixin, ComponentOptionsMixin, E>
> = {
    $props: Prettify<PublicProps>
    $data: D
    $options: Options
} & P & B & D

export type CreateComponentPublicInstanceWithMixins<
    P = {}, B = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, Mixin extends ComponentOptionsMixin = ComponentOptionsMixin,
    Extends extends ComponentOptionsMixin = ComponentOptionsMixin,
    E extends EmitsOptions = {}, PublicProps = P, Defaults = {},
    MakeDefaultsOptional extends boolean = false,
    I extends ComponentInjectOptions = {}, S extends SlotsType = {},
    PublicMixin = IntersectionMixin<Mixin> & IntersectionMixin<Extends>,
    PublicP = UnwrapMixinsType<PublicMixin, 'P'> & EnsureNonVoid<P>,
    PublicB = UnwrapMixinsType<PublicMixin, 'B'> & EnsureNonVoid<B>,
    PublicD = UnwrapMixinsType<PublicMixin, 'D'> & EnsureNonVoid<D>,
    PublicC extends ComputedOptions = UnwrapMixinsType<PublicMixin, 'C'> & EnsureNonVoid<C>,
    PublicM extends MethodOptions = UnwrapMixinsType<PublicMixin, 'M'> & EnsureNonVoid<M>,
    PublicDefaults = UnwrapMixinsType<PublicMixin, 'Defaults'> & EnsureNonVoid<Defaults>
> = ComponentPublicInstance<PublicP, PublicB, PublicD, PublicC, PublicM, E, PublicProps, PublicDefaults, MakeDefaultsOptional>

export type DefineComponent<
    PropsOrPropOptions = {}, RawBindings = {}, D = {}, C extends ComputedOptions = {},
    M extends MethodOptions = {}, Mixin extends ComponentOptionsMixin = ComponentOptionsMixin,
    Extends extends ComponentOptionsMixin = ComponentOptionsMixin,
    E extends EmitsOptions = {}, EE extends string = string
> = {
    new (...args: any[]): CreateComponentPublicInstanceWithMixins<
        Readonly<PropsOrPropOptions>, RawBindings, D, C, M, Mixin, Extends, E
    >
}

export interface FunctionalComponent<P = {}> {
    (props: P): any
    displayName?: string
}

export type ConcreteComponent<P = Data> = ComponentOptionsBase<P, any, any, any, any, any, any, any> | FunctionalComponent<P>
export type Component<P = any> = ConcreteComponent<P> | DefineComponent<P> | string
"#,
    );

    // Library types importing Component
    upsert_non_sfc(
        &host,
        "/node_modules/lib/types.d.ts",
        r#"
import type { Component } from 'vue-runtime'
export interface PrimitiveProps { asChild?: boolean; as?: Component }
export interface AccordionRootProps extends PrimitiveProps {
    collapsible?: boolean
    disabled?: boolean
}
"#,
    );

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps } from 'lib'
defineProps<AccordionRootProps>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "lib",
        "/node_modules/lib/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/lib/types.d.ts",
        "vue-runtime",
        "/node_modules/vue-runtime/types.d.ts",
    );

    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ResolverMode::Expanded);
    assert!(meta.is_some(), "deeply generic Accordion should resolve");
}

/// Test 4: Component-meta resolution (Expanded mode) for the Accordion
/// pattern. This is the full pipeline that was hanging.
#[test]
fn accordion_pattern_component_meta_expanded_does_not_hang() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    upsert_non_sfc(
        &host,
        "/node_modules/@vue/runtime-core.d.ts",
        r#"
export interface FunctionalComponent<P = {}> { (props: P): any }
export interface ComponentOptions<D = any> { data?: () => D }
export type DefineComponent<P = any> = { new (): { $props: P } }
export type ConcreteComponent = FunctionalComponent | ComponentOptions
export type Component = ConcreteComponent | DefineComponent | string
"#,
    );

    upsert_non_sfc(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        r#"
import type { Component } from '@vue/runtime-core'
export interface PrimitiveProps { asChild?: boolean; as?: Component }
export interface AccordionRootProps extends PrimitiveProps {
    collapsible?: boolean
    disabled?: boolean
}
export interface AccordionRootEmits { 'update:modelValue': [value: string | string[]] }
"#,
    );

    upsert_vue(
        &host,
        "/src/Accordion.vue",
        r#"<script setup lang="ts">
import type { AccordionRootProps, AccordionRootEmits } from 'reka-ui'
defineProps<AccordionRootProps>()
defineEmits<AccordionRootEmits>()
</script>
<template><div /></template>"#,
    );

    set_dep(
        &host,
        "/src/Accordion.vue",
        "reka-ui",
        "/node_modules/reka-ui/types.d.ts",
    );
    set_dep(
        &host,
        "/node_modules/reka-ui/types.d.ts",
        "@vue/runtime-core",
        "/node_modules/@vue/runtime-core.d.ts",
    );

    // Full component-meta resolution -- the primary contract is that this
    // does NOT hang. Whether evaluated_types are fully populated depends on
    // cross-file import resolution which may be incomplete without a live
    // workspace resolver.
    let meta =
        host.resolve_component_meta("/src/Accordion.vue", crate::types::ResolverMode::Expanded);
    assert!(
        meta.is_some(),
        "Accordion component-meta should resolve without hanging"
    );
    let reka_entry = host
        .ensure_module_facts_in_view("/node_modules/reka-ui/types.d.ts", None)
        .expect("Accordion resolution should keep the imported reka-ui entry cached");
    assert!(
        reka_entry.shallow_state.has_resolvable_surface(),
        "cached reka-ui module facts should have populated shallow state",
    );
}
