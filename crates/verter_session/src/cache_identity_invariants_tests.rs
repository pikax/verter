//! Cache-identity invariant and regression tests for the host's
//! cache-mirror surface.
//!
//! Each test characterises a specific aspect of the cache-identity
//! contract:
//!   - Arc identity for RouteDb / ImportedRootDb is shared between
//!     `ProjectTypeStore` and `UnifiedResolverRuntime`, and clears are
//!     observable through both handles.
//!   - The canonical `IndexedReady` artifact path (`ensure_indexed_ready`)
//!     is content-pinned (warm hits are Arc-identical, stale candidates
//!     are rejected by the current-hash gate), collapses concurrent cold
//!     callers onto one materialisation, and terminates on barrel cycles.
//!
//! Module is `#[cfg(test)]`-gated at the lib.rs declaration site.

use std::sync::Arc;

use crate::{HostConfig, VerterHost};

/// Test fixture host — minimal MemoryWorkspace-backed VerterHost suitable
/// for cache-identity assertions that don't need real file content.
fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn rk(provider: &str, name: &str) -> crate::resolver_core::RouteNameKey {
    crate::resolver_core::RouteNameKey::new(
        provider,
        name,
        verter_semantic::facts::registry::SymbolSpace::Type,
        crate::file_artifact_store::ProjectIdentity([0u8; 16]),
        [0u8; 16],
        [0u8; 16],
    )
}

// ─── T1 — F3 Arc identity (lands in 6b.B2) ───────────────────────────────
//
// Discrimination: pre-migration the RouteDb / ImportedRootDb fields on
// `UnifiedResolverRuntime` are owned by-value (not behind `Arc`), AND the
// `routes_handle()` / `imported_roots_handle()` accessors do not exist on
// `ProjectTypeStore` or `UnifiedResolverRuntime`, so this test does not
// compile against the parent commit (compile failure = red).
// Post-migration: the runtime borrows the project-type-store-owned `Arc`s,
// so `Arc::ptr_eq(store_handle, runtime_handle)` is true.
#[test]
fn route_db_and_imported_root_db_share_arc_identity_across_runtime_and_store() {
    let host = host();

    let store = host.project_type_store();
    let runtime = &host.resolver.runtime;

    let store_routes = store.routes_handle();
    let runtime_routes = runtime.routes_handle();
    assert!(
        Arc::ptr_eq(&store_routes, &runtime_routes),
        "RouteDb authority must be shared via Arc identity between \
         ProjectTypeStore and UnifiedResolverRuntime",
    );

    let store_imported_roots = store.imported_roots_handle();
    let runtime_imported_roots = runtime.imported_roots_handle();
    assert!(
        Arc::ptr_eq(&store_imported_roots, &runtime_imported_roots),
        "ImportedRootDb authority must be shared via Arc identity between \
         ProjectTypeStore and UnifiedResolverRuntime",
    );

    // Negative assertion (per CLAUDE.md "always include negative assertions"):
    // the runtime must not hold a clone-by-value of the same DB; that pattern
    // would silently disconnect from the project-store authority. Asserting
    // ptr_eq with a SECOND fresh `Arc` mint would not catch this — only the
    // ptr_eq above (between two cloned Arcs of the same handle) discriminates.
    let store_routes_again = store.routes_handle();
    assert!(
        Arc::ptr_eq(&store_routes_again, &runtime_routes),
        "Successive store.routes_handle() calls must return Arcs that ptr_eq \
         the runtime's handle — proving a single project-shared authority",
    );
}

// ─── F3 eviction-cascade regression ──────────────────────────────────────
//
// `clear_compile_cache` only clears the compile-side caches and explicitly
// preserves resolver caches. The actual clear-all paths that touch
// `RouteDb` / `ImportedRootDb` are `host.close()` and
// `host.configure_projects()`, both via
// `self.resolver.reset_all() -> runtime.clear_caches() ->
// routes.clear() + imported_roots.clear()`.
//
// This regression test uses `host.close()` (the actual clear path) to
// verify the project-shared semantics established by F3: a clear on the
// runtime's RouteDb is observable from the project-store's handle (and
// vice versa), because they're the same `Arc`-shared instance. Pre-F3
// the two were distinct instances; clearing one wouldn't affect the
// other.
//
// Classified REGRESSION (CLAUDE.md "always include negative assertions"):
// the post-clear assertion confirms the entry vanished AND the pre-clear
// assertion confirms it was present (so a "false-green" from a never-
// populated cache is impossible).
#[test]
fn route_db_eviction_visible_via_both_handles_after_close() {
    use crate::resolver_core::RouteResult;

    let host = host();
    let store = host.project_type_store();

    // Seed an entry via the runtime handle.
    let runtime_routes = host.resolver.runtime.routes_handle();
    runtime_routes.insert_route(
        rk("/seeded/provider.ts", "Seeded"),
        RouteResult::Resolved {
            defining_canonical: "/seeded/provider.ts".to_string(),
            defining_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            defining_symbol: "Seeded".to_string(),
        },
    );

    // Pre-clear discrimination assertion: the entry IS present, observable
    // from BOTH handles. Without this, a "false-green" from an empty cache
    // (entry never populated → assert.is_none() trivially true after
    // close()) would pass without proving anything.
    let store_routes = store.routes_handle();
    assert!(
        runtime_routes
            .get_route_any(&rk("/seeded/provider.ts", "Seeded"))
            .is_some(),
        "PRE-CLEAR: entry must be present on the runtime handle",
    );
    assert!(
        store_routes
            .get_route_any(&rk("/seeded/provider.ts", "Seeded"))
            .is_some(),
        "PRE-CLEAR: same entry must be observable from the project-store \
         handle (proves Arc identity / project-shared semantics)",
    );

    // Trigger the clear-all cascade via host.close(). Internally:
    // close() -> resolver.reset_all() -> runtime.clear_caches() ->
    //   routes.clear() (and imported_roots.clear()).
    host.close();

    // Post-clear assertion: entry is gone, observable from BOTH handles.
    assert!(
        runtime_routes
            .get_route_any(&rk("/seeded/provider.ts", "Seeded"))
            .is_none(),
        "POST-CLEAR: runtime handle must report the entry evicted",
    );
    assert!(
        store_routes
            .get_route_any(&rk("/seeded/provider.ts", "Seeded"))
            .is_none(),
        "POST-CLEAR: project-store handle must observe the same eviction \
         (single project-shared RouteDb instance)",
    );

    // Negative assertion: the handles are still ptr_eq after close()
    // (close does not swap the underlying Arc, only mutates the inner DB).
    let runtime_routes_after = host.resolver.runtime.routes_handle();
    let store_routes_after = store.routes_handle();
    assert!(
        Arc::ptr_eq(&runtime_routes_after, &store_routes_after),
        "POST-CLEAR: handles must remain Arc::ptr_eq — close() must NOT \
         re-allocate the RouteDb (which would break stable references \
         held by external callers)",
    );
}

// ────────────────────────────────────────────────────────────────────────
// `ensure_indexed_ready` invariants — content pinning, singleflight,
// cycle termination
// ────────────────────────────────────────────────────────────────────────

use std::sync::Arc as StdArc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// CountingWorkspace fixture mirroring the `host_manage_tests.rs` and
/// `frontier_tests.rs` patterns — instruments `read_file` with a
/// per-path counter so the singleflight test can assert the leader paid
/// the workspace read at most once after both threads complete.
struct CountingWs {
    inner: StdArc<MemoryWorkspace>,
    read_counts: Mutex<FxHashMap<String, u64>>,
}

impl CountingWs {
    fn new() -> StdArc<Self> {
        StdArc::new(Self {
            inner: StdArc::new(MemoryWorkspace::new(MemoryOptions::default())),
            read_counts: Mutex::new(FxHashMap::default()),
        })
    }

    fn read_count(&self, path: &str) -> u64 {
        self.read_counts.lock().get(path).copied().unwrap_or(0)
    }

    fn inject(&self, path: &str, source: &str) {
        self.inner
            .inject_file(path.to_string(), Arc::<str>::from(source));
    }
}

impl verter_workspace::WorkspaceRead for CountingWs {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        *self
            .read_counts
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.read_file(canonical_id)
    }
    fn file_exists(&self, canonical_id: &str) -> bool {
        self.inner.file_exists(canonical_id)
    }
    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
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
    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }
}

impl WorkspaceAccess for CountingWs {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges)
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
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps)
    }
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id)
    }
    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions)
    }
    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source)
    }
    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id)
    }
    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id)
    }
}

/// Workspace shim that records the host's `project_generation` AT THE
/// MOMENT the workspace `set_exact_resolutions` mutator runs — the probe
/// for the bump-after-mutate ordering invariant on the host wrapper.
struct BumpOrderProbeWs {
    inner: StdArc<MemoryWorkspace>,
    store: Mutex<Option<StdArc<crate::project_type_store::ProjectTypeStore>>>,
    generation_at_mutation: Mutex<Option<u64>>,
}

impl BumpOrderProbeWs {
    fn new() -> StdArc<Self> {
        StdArc::new(Self {
            inner: StdArc::new(MemoryWorkspace::new(MemoryOptions::default())),
            store: Mutex::new(None),
            generation_at_mutation: Mutex::new(None),
        })
    }
}

impl verter_workspace::WorkspaceRead for BumpOrderProbeWs {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.inner.read_file(canonical_id)
    }
    fn file_exists(&self, canonical_id: &str) -> bool {
        self.inner.file_exists(canonical_id)
    }
    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
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
    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }
}

impl WorkspaceAccess for BumpOrderProbeWs {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        self.inner.record_parsed_edges(canonical_id, edges)
    }
    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        if let Some(store) = self.store.lock().as_ref() {
            *self.generation_at_mutation.lock() = Some(store.current_project_generation());
        }
        self.inner.set_exact_resolutions(canonical_id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        if let Some(store) = self.store.lock().as_ref() {
            *self.generation_at_mutation.lock() = Some(store.current_project_generation());
        }
        self.inner
            .record_parsed_edges_with_exact_resolutions(canonical_id, edges, resolutions)
    }
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps)
    }
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id)
    }
    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions)
    }
    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source)
    }
    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id)
    }
    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id)
    }
}

// ─── Bump-after-mutate ordering on `set_exact_resolutions` ──────────────
//
// The pre-publish fence compares a flight's start-of-flight
// `project_generation` capture against the live generation at publish
// time. The bump therefore must STRICTLY FOLLOW the workspace mutation it
// announces: with bump-BEFORE-mutate, a flight born between the bump and
// the mutation captures the NEW stamp, resolves against the OLD
// resolution table, passes the fence, and is served as current forever.
#[test]
fn set_exact_resolutions_bumps_project_generation_after_the_workspace_mutation() {
    let canonical = "/lib/bump_order_owner.ts";
    let ws = BumpOrderProbeWs::new();
    ws.inner
        .inject_file(canonical.to_string(), Arc::from("export const x = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    *ws.store.lock() = Some(StdArc::clone(host.project_type_store()));

    let pre = host.project_type_store().current_project_generation();
    host.set_exact_resolutions(
        canonical,
        vec![verter_workspace::ExactResolution {
            specifier: "./dep".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/lib/dep.ts".to_string()),
            possible_canonical_ids: vec!["/lib/dep.ts".to_string()],
        }],
    );
    let post = host.project_type_store().current_project_generation();
    let at_mutation = ws
        .generation_at_mutation
        .lock()
        .expect("the workspace mutator must have run");

    assert!(
        post > pre,
        "set_exact_resolutions must bump project_generation (pre={pre}, post={post})",
    );
    assert_eq!(
        at_mutation, pre,
        "the project_generation bump must land AFTER the workspace \
         mutator (mutate-first): observed generation {at_mutation} at \
         mutation time, pre-call generation {pre} — a premature bump \
         opens the captured-new-stamp-over-old-table fence bypass",
    );
}

/// Workspace shim counting per-canonical route-mutation calls — the probe
/// for `integrate_scheduler_snapshot`'s atomic route re-sync.
struct RouteSyncProbeWs {
    inner: StdArc<MemoryWorkspace>,
    record_calls: Mutex<FxHashMap<String, u64>>,
    set_exact_calls: Mutex<FxHashMap<String, u64>>,
    combined_calls: Mutex<FxHashMap<String, u64>>,
}

impl RouteSyncProbeWs {
    fn new() -> StdArc<Self> {
        StdArc::new(Self {
            inner: StdArc::new(MemoryWorkspace::new(MemoryOptions::default())),
            record_calls: Mutex::new(FxHashMap::default()),
            set_exact_calls: Mutex::new(FxHashMap::default()),
            combined_calls: Mutex::new(FxHashMap::default()),
        })
    }

    fn reset_counts(&self) {
        self.record_calls.lock().clear();
        self.set_exact_calls.lock().clear();
        self.combined_calls.lock().clear();
    }

    fn count(map: &Mutex<FxHashMap<String, u64>>, canonical: &str) -> u64 {
        map.lock().get(canonical).copied().unwrap_or(0)
    }
}

impl verter_workspace::WorkspaceRead for RouteSyncProbeWs {
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.inner.read_file(canonical_id)
    }
    fn file_exists(&self, canonical_id: &str) -> bool {
        self.inner.file_exists(canonical_id)
    }
    fn realpath(&self, canonical_id: &str) -> Option<String> {
        self.inner.realpath(canonical_id)
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
    fn content_generation(&self) -> u64 {
        self.inner.content_generation()
    }
}

impl WorkspaceAccess for RouteSyncProbeWs {
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[verter_workspace::ParsedEdge]) {
        *self
            .record_calls
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.record_parsed_edges(canonical_id, edges)
    }
    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        *self
            .set_exact_calls
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner.set_exact_resolutions(canonical_id, resolutions)
    }
    fn record_parsed_edges_with_exact_resolutions(
        &self,
        canonical_id: &str,
        edges: &[verter_workspace::ParsedEdge],
        resolutions: Vec<verter_workspace::ExactResolution>,
    ) -> verter_workspace::ExactResolutionResult {
        *self
            .combined_calls
            .lock()
            .entry(canonical_id.to_string())
            .or_default() += 1;
        self.inner
            .record_parsed_edges_with_exact_resolutions(canonical_id, edges, resolutions)
    }
    fn replace_semantic_transitive(
        &self,
        canonical_id: &str,
        deps: std::collections::BTreeSet<String>,
    ) {
        self.inner.replace_semantic_transitive(canonical_id, deps)
    }
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str) {
        self.inner.record_ambient_dependency(consumer, virtual_id)
    }
    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>) {
        self.inner.set_default_resolve_extensions(host_extensions)
    }
    fn notify_upsert(&self, canonical_id: &str, source: Arc<str>) {
        self.inner.notify_upsert(canonical_id, source)
    }
    fn notify_close(&self, canonical_id: &str) {
        self.inner.notify_close(canonical_id)
    }
    fn notify_delete(&self, canonical_id: &str) {
        self.inner.notify_delete(canonical_id)
    }
}

/// `integrate_scheduler_snapshot`'s route re-sync (record parsed edges +
/// re-apply preserved bundler exacts) must land as ONE atomic workspace
/// mutation. The two-call sequence (`record_parsed_edges` then
/// `set_exact_resolutions`) exposes a window in which the canonical's
/// exact resolutions are cleared but not yet re-applied: a cold flight
/// starting inside the window resolves against the half-applied table,
/// reaches its pre-publish fence with no generation moved, publishes,
/// and passes `indexed_surface_is_current` indefinitely.
#[test]
fn integrate_re_syncs_bundler_routes_via_one_atomic_workspace_mutation() {
    let canonical = "/lib/atomic_route_owner.ts";
    let target = "/lib/atomic_route_target.ts";
    let ws = RouteSyncProbeWs::new();
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    host.notify_upsert(
        canonical,
        Arc::from("import type { T } from './alias';\nexport type U = T;\n"),
    );
    host.notify_upsert(target, Arc::from("export type T = 1;\n"));

    // Pre-load bundler push (the preserved-routes precondition).
    host.set_import_dependencies(
        canonical,
        vec![crate::types::DependencyResolution {
            specifier: "./alias".to_string(),
            resolved_canonical_id: Some(target.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    ws.reset_counts();
    let loaded = host.ensure_loaded(canonical);
    assert!(loaded, "ensure_loaded must succeed");

    assert!(
        RouteSyncProbeWs::count(&ws.combined_calls, canonical) >= 1,
        "integrate must re-sync preserved bundler routes through the \
         atomic combined mutation",
    );
    assert_eq!(
        RouteSyncProbeWs::count(&ws.record_calls, canonical),
        0,
        "integrate must not record parsed edges through the separate \
         mutator when preserved routes exist (torn exacts-cleared window)",
    );
    assert_eq!(
        RouteSyncProbeWs::count(&ws.set_exact_calls, canonical),
        0,
        "integrate must not re-apply exacts through the separate mutator \
         (second half of the torn window)",
    );

    // Functional control: the bundler-injected exact edge survived the
    // integrate (the atomic path must preserve the two-call semantics).
    let owners = host.workspace().reverse_deps_for(target);
    assert!(
        owners.contains(&canonical.to_string()),
        "workspace exact-resolution edge must survive integrate; got {owners:?}",
    );
}

/// The module-augmentation cache-mode probe
/// (`owner_has_module_augmentation_dependency`) walks side-effect-import
/// re-export chains by reading barrel artifacts from the store. Those
/// reads must honour the SAME artifact authority gates as every other
/// cross-file-edge reader: a stale leftover artifact the accessors
/// reject (absent file → `artifact_only_candidate_is_fresh` false) must
/// not feed the BFS its baked re-export edges — pre-fix the ungated
/// `get_any` walked the rejected barrel and "discovered" an augmenter
/// through state no serving path would ever return.
#[test]
fn augmentation_probe_rejects_stale_artifact_the_authority_gate_rejects() {
    use crate::types::UpsertRequest;

    let owner = "/workspace/src/aug_owner.ts";
    let phantom = "/workspace/src/phantom_barrel.ts";
    let real_aug = "/workspace/src/real_aug.d.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(
        owner.to_string(),
        Arc::from("import \"./phantom_barrel\";\nexport type Owner = 1;\n"),
    );
    ws.inject_file(
        real_aug.to_string(),
        Arc::from("declare global { interface PhantomAug { x: 1 } }\nexport {};\n"),
    );
    // The phantom barrel file is NEVER present in the workspace.
    let host = StdArc::new(VerterHost::new(HostConfig::default(), ws));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(owner.to_string()),
            input_id: owner.to_string(),
            source: Arc::from("import \"./phantom_barrel\";\nexport type Owner = 1;\n"),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(owner)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");
    // Bundler route: the side-effect specifier resolves to the phantom
    // canonical (resolution succeeds; materialisation cannot).
    host.set_import_dependencies(
        owner,
        vec![crate::types::DependencyResolution {
            specifier: "./phantom_barrel".to_string(),
            resolved_canonical_id: Some(phantom.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );

    // Seed the stale leftover: a barrel artifact whose baked wildcard
    // edge points at the REAL augmenter file.
    let shallow =
        crate::resolver_core::shallow_file_state::ShallowFileState::routing_tables_only_for_test(
            [9u8; 16],
            FxHashMap::default(),
            vec![crate::resolver_core::shallow_file_state::WildcardReexport {
                source_specifier: "./real_aug".to_string(),
                canonical_id: real_aug.to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            }],
            rustc_hash::FxHashSet::default(),
            FxHashMap::default(),
            StdArc::new(
                verter_parser::utils::oxc::script::route_inventory::ScriptRouteInventory::default(),
            ),
        );
    let indexed = crate::project_type_store::IndexedReady::new_for_test_with_state(
        [9u8; 16],
        StdArc::new(shallow),
        Arc::from("export * from \"./real_aug\";\n"),
        Arc::from("export * from \"./real_aug\";\n"),
    );
    host.project_type_store()
        .indexed()
        .insert(Arc::from(phantom), StdArc::new(indexed));

    // Precondition: every serving accessor rejects the phantom artifact
    // (its file does not exist).
    assert!(
        host.artifact_current_indexed_raw(phantom).is_none(),
        "precondition: the authority gate must reject the phantom artifact",
    );

    // The probe must NOT discover the augmenter through the rejected
    // artifact's baked edges.
    assert!(
        !host.owner_has_module_augmentation_dependency(owner),
        "the augmentation BFS must not traverse baked re-export edges of \
         an artifact the authority gate rejects",
    );
}

// ─── Content freshness for artifact-only canonicals ─────────────────────
//
// Pins `artifact_only_candidate_is_fresh`: a canonical the scheduler has
// NEVER tracked (no `DerivedRawState`) has the workspace as its sole
// content authority, so its retained `FileArtifactStore` artifact serves
// ONLY while the authority gate holds (`artifact_only_authority_allows`:
// artifact-only scope AND `ws().file_exists(canonical)`) AND its build
// generation (`IndexedReady.edge_generation`) is at-or-after the
// canonical's last recorded workspace content transition. Without the
// gate, artifact-only canonicals serve stale / deleted-file artifacts
// across `set_workspace` / `notify_close`.

/// Seed a fresh-stamped artifact-only `IndexedReady` for `canonical`
/// directly into `FileArtifactStore` (no scheduler ingress).
fn seed_artifact_only(
    host: &VerterHost,
    canonical: &str,
) -> StdArc<crate::project_type_store::IndexedReady> {
    let mut artifact = crate::project_type_store::IndexedReady::new_for_test([7u8; 16]);
    artifact.edge_generation = host.ws().content_generation();
    artifact.project_generation = host.project_type_store().current_project_generation();
    let artifact = StdArc::new(artifact);
    host.project_type_store()
        .indexed()
        .insert(Arc::from(canonical), StdArc::clone(&artifact));
    artifact
}

#[test]
fn artifact_only_canonical_is_evicted_on_own_overlay_change() {
    let canonical = "/seeded/tier2_change.ts";
    let unrelated = "/seeded/tier2_change_unrelated.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type T2 = 1;"));
    ws.inject_file(unrelated.to_string(), Arc::from("export type U = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = seed_artifact_only(&host, canonical);

    // Control: existing file → the artifact-only authority answers.
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "PRE: a fresh artifact-only artifact must serve",
    );

    // Negative: an UNRELATED workspace signal must NOT evict this
    // canonical's artifact — package-style artifact-only surfaces keep
    // serving across unrelated transitions.
    host.notify_upsert(unrelated, Arc::from("export const u = 1;"));
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "an unrelated overlay change must not evict the artifact-only \
         artifact (untracked surfaces reuse across unrelated transitions)",
    );

    // The canonical's OWN overlay change is its content-supersession
    // signal: the workspace is this canonical's sole content authority,
    // so the retained artifact must stop serving.
    host.notify_upsert(canonical, Arc::from("export type T2 = 2;"));
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "POST: the canonical's own overlay change must evict its \
         artifact-only artifact",
    );
}

#[test]
fn artifact_only_canonical_requires_file_presence() {
    let canonical = "/seeded/tier2_missing.ts";
    // The canonical is NOT injected — file_exists is false while the
    // generation stamp matches, discriminating the `file_exists`
    // conjunct of the authority gate's AND-clause.
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = seed_artifact_only(&host, canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "tier-2 must require file_exists: a nonexistent file rejects even \
         when the generation stamp matches",
    );
}

#[test]
fn artifact_only_canonical_is_rejected_after_notify_close() {
    let canonical = "/seeded/tier2_close.ts";
    let unrelated = "/seeded/tier2_close_unrelated.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type T2 = 1;"));
    ws.inject_file(unrelated.to_string(), Arc::from("export type U = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = seed_artifact_only(&host, canonical);
    let _ = seed_artifact_only(&host, unrelated);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "PRE: the seeded artifact must serve before the close",
    );

    // The LSP `did_close` pin: the close is the canonical's PER-CANONICAL
    // content-supersession signal — the host wrapper evicts the
    // artifact-only payload (memory release) and the workspace records a
    // content transition for the canonical (the read-side authority the
    // serving gate consults).
    host.notify_close(canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "POST: notify_close must invalidate the artifact-only artifact",
    );
    // Negative half: a per-canonical close must NOT take down an
    // UNRELATED canonical's artifact (per-canonical rail, not an epoch
    // equality clause).
    assert!(
        host.artifact_current_indexed_raw(unrelated).is_some(),
        "an unrelated canonical's artifact must survive a per-canonical \
         close",
    );
}

/// The tier-2 freshness perimeter must hold for mutators that BYPASS the
/// host wrappers: a JS embedder holds the `Workspace` object and fires
/// `notifyUpsert` / `writeFile` on it directly (the NAPI shape), so no
/// host-side eviction runs. The artifact-only authority is read-side:
/// the workspace records a per-canonical content-transition generation
/// at its own mutation chokepoints, and a retained artifact built before
/// the canonical's last transition must not serve.
#[test]
fn artifact_only_canonical_rejected_after_direct_workspace_mutation() {
    use verter_workspace::WorkspaceAccess as _;

    let canonical = "/seeded/tier2_direct_upsert.ts";
    let written = "/seeded/tier2_direct_write.ts";
    let copied = "/seeded/tier2_direct_copy.ts";
    let unrelated = "/seeded/tier2_direct_unrelated.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type T2 = 1;"));
    ws.inject_file(written.to_string(), Arc::from("export type W = 1;"));
    ws.inject_file(copied.to_string(), Arc::from("export type C = 1;"));
    ws.inject_file(unrelated.to_string(), Arc::from("export type U = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    for seeded in [canonical, written, copied, unrelated] {
        let _ = seed_artifact_only(&host, seeded);
        assert!(
            host.artifact_current_indexed_raw(seeded).is_some(),
            "PRE: the seeded artifact for {seeded} must serve",
        );
    }

    // (b)-class: direct workspace notify — the host wrapper (and its
    // eviction) never runs.
    ws.notify_upsert(canonical, Arc::from("export type T2 = 2;"));
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "a DIRECT workspace notify_upsert must stop the artifact-only \
         serve (the read-side transition rail, not host-wrapper eviction)",
    );

    // (a)-class: write_file replaces backing content; file_exists stays
    // true, so only the transition rail can catch it.
    ws.write_file(written, "export type W = 2;")
        .expect("write_file must succeed");
    assert!(
        host.artifact_current_indexed_raw(written).is_none(),
        "write_file must stop the artifact-only serve",
    );

    // (a)-class: copy_file replaces the DESTINATION's content.
    ws.copy_file(canonical, copied)
        .expect("copy_file must succeed");
    assert!(
        host.artifact_current_indexed_raw(copied).is_none(),
        "copy_file must stop the destination's artifact-only serve",
    );

    // Package-reuse negative: the rail is PER-CANONICAL — the unrelated
    // canonical's artifact keeps serving across all of the above.
    assert!(
        host.artifact_current_indexed_raw(unrelated).is_some(),
        "unrelated transitions must not invalidate an untouched \
         artifact-only canonical (per-canonical rail, not an epoch \
         equality clause)",
    );
}

#[test]
fn close_clears_retained_artifact_store() {
    let canonical = "/seeded/tier2_host_close.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type T2 = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = seed_artifact_only(&host, canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "PRE: the seeded artifact must serve before close()",
    );

    // `close()` releases ALL cached data. An artifact-only canonical is
    // untouched by the per-tracked-file notify_delete loop (it has no
    // scheduler node) and its backing file stays present, so without an
    // explicit artifact-store clear its `IndexedReady` both keeps the
    // backing memory resident (breaking close()'s memory-release
    // contract) and keeps SERVING through the artifact-only authority
    // gate.
    host.close();
    assert!(
        host.project_type_store().indexed().is_empty(),
        "close() must clear the FileArtifactStore (memory-release \
         contract: every IndexedReady lives there, including its shallow \
         index and DeclBodyMemo, which owns the lazily-materialised \
         whole_env)",
    );
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "POST: an artifact-only canonical must not keep serving after \
         close()",
    );
}

#[test]
fn artifact_only_canonical_is_rejected_across_set_workspace() {
    let canonical = "/seeded/tier2_swap.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type T2 = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = seed_artifact_only(&host, canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "PRE: the seeded artifact must serve before the swap",
    );

    // Swap the entire workspace authority for one that does NOT carry the
    // file. `IndexedReady` deliberately survives the project-generation
    // evict (content-addressed), so WITHOUT the tier-2 gate the stale
    // artifact keeps serving against a workspace it never came from.
    host.set_workspace(StdArc::new(MemoryWorkspace::new(MemoryOptions::default())));
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "POST: a workspace swap must invalidate artifact-only artifacts \
         that have no presence in the new workspace",
    );
}

/// The DANGEROUS `set_workspace` case (named in the `set_workspace`
/// in-code rationale): the NEW workspace carries the SAME path with
/// DIFFERENT content. The `file_exists` freshness leg passes against the
/// new workspace, the generation stamps reset per-workspace, and the new
/// workspace's transition ledger is empty — so ONLY the `set_workspace`
/// artifact-store clear (`indexed().clear_all()`) stops the stale
/// artifact from serving content the new authority never produced.
/// (The empty-workspace sibling test above is rejected by `file_exists`
/// alone and cannot discriminate the clear.)
#[test]
fn set_workspace_same_path_different_content_does_not_serve_stale_artifact() {
    let canonical = "/seeded/tier2_swap_same_path.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type Old = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let _ = seed_artifact_only(&host, canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_some(),
        "PRE: the seeded artifact must serve before the swap",
    );

    let replacement = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    replacement.inject_file(canonical.to_string(), Arc::from("export type New = 2;"));
    host.set_workspace(replacement);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "POST: a workspace swap must invalidate the stale artifact even \
         when the new workspace carries the same path (different \
         content) — only the artifact-store clear catches this case",
    );
}

/// The base `HostStoreView` snapshot must apply the SAME artifact-only
/// freshness gate the accessors apply. Without it, `build` manufactures a
/// tracked `whole_hash` from the retained artifact itself for a canonical
/// `artifact_current_indexed_raw` rejects — letting stale
/// FileWholeHash/Route/file facts validate against state no read path
/// will serve.
#[test]
fn store_view_rejects_artifact_only_state_the_accessor_rejects() {
    // Never-present file: the artifact-only authority rejects it.
    let canonical = "/seeded/tier2_view_missing.ts";
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = seed_artifact_only(&host, canonical);
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "precondition: the accessor must reject the never-present canonical",
    );
    let view = host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();
    assert!(
        view.whole_hash(canonical).is_none(),
        "the base store view must not manufacture a tracked whole_hash for \
         an artifact-only canonical the freshness gate rejects",
    );

    // Positive control: a FRESH artifact-only canonical (file present,
    // accessor serves) stays base-visible through the same gate.
    let fresh_canonical = "/seeded/tier2_view_fresh.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(fresh_canonical.to_string(), Arc::from("export type V = 1;"));
    let fresh_host = VerterHost::new(HostConfig::default(), ws);
    let seeded = seed_artifact_only(&fresh_host, fresh_canonical);
    assert!(
        fresh_host
            .artifact_current_indexed_raw(fresh_canonical)
            .is_some(),
        "precondition: the accessor must serve the fresh canonical",
    );
    let fresh_view = fresh_host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();
    assert_eq!(
        fresh_view.whole_hash(fresh_canonical),
        Some(seeded.whole_hash),
        "a fresh artifact-only canonical must stay base-visible (the gate \
         filters STALE state, not the artifact-only lane itself)",
    );

    // Per-canonical close on the fresh host: the view must stop seeing it,
    // in lockstep with the accessor.
    fresh_host.notify_close(fresh_canonical);
    let closed_view = fresh_host
        .resolver_store_view_read()
        .into_cold_seed_view()
        .into_inner();
    assert!(
        closed_view.whole_hash(fresh_canonical).is_none(),
        "after notify_close the base view must reject the canonical \
         exactly as the accessor does",
    );
}

#[test]
fn artifact_only_superseded_artifact_rebuilds_instead_of_serving() {
    let canonical = "/seeded/tier2_rebuild.ts";
    let ws = StdArc::new(MemoryWorkspace::new(MemoryOptions::default()));
    ws.inject_file(canonical.to_string(), Arc::from("export type Stale = 1;"));
    let host = VerterHost::new(HostConfig::default(), ws);
    let seeded = seed_artifact_only(&host, canonical);

    // The canonical's own overlay change supersedes the seeded artifact
    // (the per-canonical signal eviction).
    host.notify_upsert(canonical, Arc::from("export type Rebuilt = 1;"));

    // The stale stub must not serve anywhere; the unified cold build
    // re-materialises from the live workspace content (`ensure_loaded`
    // ingests the overlay).
    assert!(
        host.artifact_current_indexed_raw(canonical).is_none(),
        "the superseded stub must not serve after the signal eviction",
    );
    let rebuilt = host
        .ensure_indexed_ready(canonical)
        .expect("a superseded artifact for a live file must rebuild");
    assert_ne!(
        rebuilt.whole_hash, seeded.whole_hash,
        "the rebuild must come from the workspace content, not the stale stub",
    );
    assert!(
        rebuilt.shallow_state.symbol("Rebuilt").is_some(),
        "the rebuilt artifact must carry the live overlay's surface",
    );
}

/// Spin up a host backed by a CountingWs with a single inserted file.
/// `path` is the canonical id; `source` is the .ts file body.
fn host_with_counting_ws(path: &str, source: &str) -> (VerterHost, StdArc<CountingWs>) {
    let ws = CountingWs::new();
    ws.inject(path, source);
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    (host, ws)
}

// ─── Content-hash pinning + epoch survival for `ensure_indexed_ready` ────
//
// Three sub-cases:
// (a) same content → cache hit (Arc::ptr_eq);
// (b) a planted stale-hash candidate → the current-hash gate rejects it
//     and re-materialises at the real hash;
// (c) `bump_store_view_epoch` without content/project-gen change →
//     the artifact SURVIVES (content-addressed storage is not cleared by
//     an epoch bump).
#[test]
fn indexed_ready_warm_hit_is_pinned_to_content_hash() {
    let canonical = "/lib/foo.ts";
    let (host, _ws) = host_with_counting_ws(canonical, "export type Foo = number;");
    let store = host.project_type_store();

    // Sub-case (a) — same content → cache hit.
    let entry1 = host
        .ensure_indexed_ready(canonical)
        .expect("cold materialiser must succeed");
    let entry2 = host
        .ensure_indexed_ready(canonical)
        .expect("warm hit must succeed");
    assert!(
        Arc::ptr_eq(&entry1, &entry2),
        "(a) same content: warm hit must return Arc::ptr_eq artifact",
    );

    // Sub-case (c) — `bump_store_view_epoch` does NOT clear the
    // content-addressed artifact store; the artifact survives epoch bumps.
    host.bump_store_view_epoch();
    let entry3 = host
        .ensure_indexed_ready(canonical)
        .expect("artifact must survive epoch bump");
    assert!(
        Arc::ptr_eq(&entry1, &entry3),
        "(c) epoch bump: artifact must SURVIVE (no cache-wide clear)",
    );

    // Sub-case (b) — content-hash mismatch → cache miss. Plant a stale
    // candidate with a doctored whole_hash (`FileArtifactStore::insert`
    // drains the real entry, so the store holds ONLY the stale candidate).
    // The current-hash gate must reject it and re-materialise.
    let stale_hash = [0xFFu8; 16];
    let mut stale = (*entry1).clone();
    stale.whole_hash = stale_hash;
    store
        .indexed()
        .insert(Arc::from(canonical), StdArc::new(stale));
    assert_eq!(
        store.indexed().get_any(canonical).map(|e| e.whole_hash),
        Some(stale_hash),
        "(b) setup: stale-hash candidate planted",
    );

    // Re-materialise: the content-pinned read detects the synthetic
    // whole_hash mismatch; the cold path re-materialises at the real hash.
    let entry4 = host
        .ensure_indexed_ready(canonical)
        .expect("post-plant read must materialise fresh artifact");
    assert_ne!(
        entry4.whole_hash, stale_hash,
        "(b) the content-pinned read must reject the stale-hash candidate",
    );
    assert_eq!(
        entry4.whole_hash, entry1.whole_hash,
        "(b) re-materialisation must produce the real (un-doctored) hash for unchanged on-disk content",
    );
}

// ─── Concurrent cold callers collapse onto one materialisation ───────────
//
// Two assertions:
// (a) `Arc::ptr_eq` proves shared publication (concurrent cold requests
//     to the same canonical collapse onto one materialisation path);
// (b) `read_count <= 1` proves only the leader paid the I/O cost.
#[test]
fn indexed_ready_concurrent_cold_callers_read_once_and_collapse() {
    use std::sync::Barrier;
    use std::thread;

    let canonical = "/lib/concurrent.ts";
    let (host, ws) = host_with_counting_ws(canonical, "export type Concurrent = boolean;");
    let host = StdArc::new(host);

    // PRE-DISCRIMINATION: both Arcs are NEW (no published artifact yet).
    assert_eq!(
        ws.read_count(canonical),
        0,
        "PRE: no reads of the canonical have happened",
    );

    let barrier = StdArc::new(Barrier::new(2));

    let host_a = StdArc::clone(&host);
    let host_b = StdArc::clone(&host);
    let bar_a = StdArc::clone(&barrier);
    let bar_b = StdArc::clone(&barrier);

    let canonical_a = canonical.to_string();
    let canonical_b = canonical.to_string();

    let h_a = thread::spawn(move || {
        bar_a.wait();
        host_a.ensure_indexed_ready(&canonical_a)
    });
    let h_b = thread::spawn(move || {
        bar_b.wait();
        host_b.ensure_indexed_ready(&canonical_b)
    });

    let entry_a = h_a.join().expect("thread a panicked").expect("artifact");
    let entry_b = h_b.join().expect("thread b panicked").expect("artifact");

    // (a) singleflight collapse: both threads return the same Arc.
    assert!(
        Arc::ptr_eq(&entry_a, &entry_b),
        "concurrent cold callers must collapse onto one materialisation (Arc::ptr_eq)",
    );

    // (b) read-once: the leader paid I/O exactly once.
    // CountingWs.read_count counts WorkspaceAccess::read_file invocations;
    // the cold materialisation path is the only path that reads from the
    // workspace.
    let count = ws.read_count(canonical);
    assert!(
        count <= 1,
        "leader I/O must run at most ONCE — observed read_count={count} \
         (concurrent cold callers failed to collapse, OR the materialiser \
         read the file more than once per cold path)",
    );
}

// ─── REGRESSION: route export resolution terminates on barrel cycle ──────
//
// The body provides a barrel-cycle fixture and asserts termination —
// materialising either canonical of a two-file `export *` cycle must not
// recurse unboundedly or blow the stack.
#[test]
fn route_export_resolution_terminates_on_barrel_cycle() {
    // Two route-only files that re-export each other → would cycle without
    // the resolver's active-set cycle guard.
    let ws = CountingWs::new();
    ws.inject("/cycle/a.ts", "export * from './b';");
    ws.inject("/cycle/b.ts", "export * from './a';");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // Materialising the cyclic barrel must terminate without panicking or
    // blowing the stack.
    let _ = host
        .ensure_indexed_ready("/cycle/a.ts")
        .expect("route-only materialisation must terminate even with cyclic exports");

    // Negative regression: the second canonical's artifact is independently
    // materialisable. Use the count after both reads as a smoke test —
    // the materialiser reads each canonical a bounded number of times.
    let _ = host
        .ensure_indexed_ready("/cycle/b.ts")
        .expect("second canonical must also materialise");
    assert!(
        ws.read_count("/cycle/b.ts") <= 2,
        "cycle traversal must read each canonical a bounded number of times",
    );
}
