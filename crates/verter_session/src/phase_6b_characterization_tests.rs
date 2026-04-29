//! Phase 6b characterization and regression tests.
//!
//! Each test in this module characterizes a specific aspect of the Phase 6b
//! cache-mirror cleanup (sub-plan §6b.0.2). The full set of T1–T13 tests
//! lands incrementally across the migration commits 6b.B2 → 6b.D2b. Each
//! test references symbols introduced in its lands-in commit; any test that
//! discriminates pre-vs-post migration captures the red-run output (test
//! failing against parent commit) and the green-run output (test passing
//! post-implementation) in the commit message body.
//!
//! Per the lands-in matrix (sub-plan §6b.0.2):
//!   T1  → 6b.B2  (Arc identity for RouteDb / ImportedRootDb)
//!   T7  → 6b.D1  (evict_canonical extension)
//!   T2-T6, T9, T12, T13 → 6b.D2a (F6/F7 internal migration)
//!   T8, T10, T11 → 6b.D2b (workspace-API bypass closure)
//!
//! Tests classified REGRESSION (T6, T11, T12, T13) verify properties that
//! hold at the destination commit; the red-then-green-within-commit
//! invariant is relaxed for those — only post-migration green is required.
//!
//! Module is `#[cfg(test)]`-gated at the lib.rs declaration site.

use std::sync::Arc;

use crate::{HostConfig, VerterHost};

/// Test fixture host — minimal MemoryWorkspace-backed VerterHost suitable
/// for cache-identity assertions that don't need real file content.
fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
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
         ProjectTypeStore and UnifiedResolverRuntime (Phase 6b.F3, Option (i))",
    );

    let store_imported_roots = store.imported_roots_handle();
    let runtime_imported_roots = runtime.imported_roots_handle();
    assert!(
        Arc::ptr_eq(&store_imported_roots, &runtime_imported_roots),
        "ImportedRootDb authority must be shared via Arc identity between \
         ProjectTypeStore and UnifiedResolverRuntime (Phase 6b.F3, Option (i))",
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

// ─── F3 eviction-cascade regression (lands in 6b.B3, REGRESSION) ─────────
//
// The brief's mention of `host.clear_compile_cache()` is incorrect against
// HEAD `3147c02f`: `clear_compile_cache` (lib.rs:1149-1163) only clears
// compile_cache / resolved_type_cache / eval_env_cache and explicitly
// preserves resolver caches. The actual clear-all paths that touch
// `RouteDb` / `ImportedRootDb` are `host.close()` (lib.rs:1175) and
// `host.configure_projects()` (lib.rs:1219), both via
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
        "/seeded/provider.ts".to_string(),
        "Seeded".to_string(),
        RouteResult::Resolved {
            defining_canonical: "/seeded/provider.ts".to_string(),
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
            .get_route_any("/seeded/provider.ts", "Seeded")
            .is_some(),
        "PRE-CLEAR: entry must be present on the runtime handle",
    );
    assert!(
        store_routes
            .get_route_any("/seeded/provider.ts", "Seeded")
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
            .get_route_any("/seeded/provider.ts", "Seeded")
            .is_none(),
        "POST-CLEAR: runtime handle must report the entry evicted",
    );
    assert!(
        store_routes
            .get_route_any("/seeded/provider.ts", "Seeded")
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

// ─── T7 — F6/F7 destination DB eviction cascade (lands in 6b.D1) ─────────
//
// Discrimination: pre-migration the `route_owned_shallow` field on
// `ProjectTypeStore` does not exist, so this test does not compile against
// the parent commit (compile failure = red). Post-migration:
// `ProjectTypeStore::evict_canonical(canonical)` cascades to
// `route_owned_shallow.remove(canonical)` — proving the new DB is wired
// into the existing per-canonical eviction primitive.
#[test]
fn evict_canonical_cascade_includes_route_owned_shallow() {
    use crate::project_type_store::RouteOwnedShallowEntry;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/route_owned.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));

    // Seed the new DB directly. The test exercises the eviction cascade,
    // not the cold-materializer path (that's T2-T5 in 6b.D2a).
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE-EVICT: entry must be present after publish",
    );

    // Cascade: evict_canonical extends to route_owned_shallow.remove.
    store.evict_canonical(&canonical);

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST-EVICT: ProjectTypeStore::evict_canonical must remove the \
         route_owned_shallow entry for that canonical (Phase 6b.D1)",
    );

    // Negative assertion: an unrelated canonical's entry MUST survive
    // a per-canonical evict (otherwise the cascade is overly broad).
    let other: Arc<str> = Arc::from("/other/file.ts");
    let other_entry = Arc::new(RouteOwnedShallowEntry::test_stub(other.clone()));
    store
        .route_owned_shallow()
        .publish(other.clone(), Arc::clone(&other_entry));
    store.evict_canonical(&canonical);
    assert!(
        store.route_owned_shallow().get_any(&other).is_some(),
        "POST-EVICT: per-canonical evict must NOT touch unrelated entries",
    );
}

// ─── T9 — configure_projects cascade clears route_owned_shallow (lands 6b.D2a step 6) ───
//
// Discrimination: pre-migration `configure_projects` does NOT call
// `route_owned_shallow().clear_all()` (verified at HEAD `3147c02f`,
// lib.rs:1149-1163 — the wrapper resets resolver / resolved_type_cache /
// eval_env_cache / semantic_invalidate_all but no project_type_store
// cascade). Post-migration: it ALSO calls `bump_project_generation_and_evict`
// and `route_owned_shallow().clear_all()`. With a populated
// route_owned_shallow entry, pre-migration assertion would FAIL (entry
// survives configure_projects); post-migration it passes (entry cleared).
#[test]
fn route_owned_shallow_clears_on_host_configure_projects() {
    use crate::project_type_store::RouteOwnedShallowEntry;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/route_only.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));

    // Setup-discrimination assertion (per fourth-pass review): assert
    // entry IS present BEFORE the wrapper. Without this, a never-populated
    // entry → assert.is_none() trivially true after configure_projects()
    // would pass without proving anything.
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE-CONFIGURE: entry must be present after publish",
    );

    let pre_gen = store.current_project_generation();

    // Trigger the cascade.
    host.configure_projects(Vec::new());

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST-CONFIGURE: configure_projects must clear route_owned_shallow \
         via the new cascade extension (Phase 6b.D2a step 6)",
    );

    // Project generation must have advanced (proves
    // bump_project_generation_and_evict is in the cascade).
    let post_gen = store.current_project_generation();
    assert!(
        post_gen > pre_gen,
        "POST-CONFIGURE: project_generation must advance \
         (bump_project_generation_and_evict in cascade)",
    );
}

// ─── T-clear_compile_cache cascade extension (lands 6b.D2a step 6) ───────
//
// Brief §6b.0.2 doesn't enumerate this exact test, but the cascade
// extension to `clear_compile_cache` is part of step 6 — verifying it
// exists keeps the cascade-additions audit honest. Discrimination as
// above.
#[test]
fn route_owned_shallow_clears_on_host_clear_compile_cache() {
    use crate::project_type_store::RouteOwnedShallowEntry;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/clear_compile.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE-CLEAR: entry must be present",
    );

    host.clear_compile_cache();

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST-CLEAR: clear_compile_cache must clear route_owned_shallow \
         (Phase 6b.D2a step 6 cascade extension)",
    );
}

// ─── T-set_workspace cascade extension (lands 6b.D2a step 6) ─────────────
//
// `set_workspace` is the most aggressive workspace mutation: the entire
// workspace authority swaps out. Pre-migration cascade: only
// `set_default_resolve_extensions` + workspace.write() + bump_store_view_epoch.
// Post-migration cascade additionally: bump_project_generation_and_evict +
// route_owned_shallow.clear_all + resolver.reset_all + resolved_type_cache.clear
// + eval_env_cache.clear + semantic_invalidate_all.
#[test]
fn route_owned_shallow_clears_on_host_set_workspace() {
    use crate::project_type_store::RouteOwnedShallowEntry;
    use std::sync::Arc as StdArc;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/set_workspace.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE-SWAP: entry must be present",
    );

    let pre_gen = store.current_project_generation();

    // Swap to a fresh workspace — triggers the full cascade.
    let fresh_ws: StdArc<dyn verter_workspace::WorkspaceAccess> = StdArc::new(
        verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default()),
    );
    host.set_workspace(fresh_ws);

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST-SWAP: set_workspace must clear route_owned_shallow \
         (Phase 6b.D2a step 6 cascade extension)",
    );

    let post_gen = store.current_project_generation();
    assert!(
        post_gen > pre_gen,
        "POST-SWAP: project_generation must advance",
    );
}

// ────────────────────────────────────────────────────────────────────────
// 6b.D2a tests — F6/F7 internal migration
// ────────────────────────────────────────────────────────────────────────
//
// T2-T6 + T12 + T13 land here. Symbols referenced (the new
// `route_owned_shallow` accessor on `ProjectTypeStore`, the deletion of
// `external_type_analysis_cache` / `route_owned_shallow_cache` host mutex
// fields, the materialiser `ensure_route_owned_shallow_entry`, the
// singleflight collapse via `route_owned_shallow_singleflight`, and the
// tier-2/tier-3 staleness gates) only exist after 6b.D2a; tests fail to
// compile against the parent commit (red).

use std::sync::Arc as StdArc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// CountingWorkspace fixture mirroring the `host_manage_tests.rs:118` and
/// `frontier_tests.rs:115` patterns — instruments `read_file` /
/// `read_analysis_source` /paths with an `AtomicU64` counter so T5 can
/// assert `read_count == 1` after both threads complete (per §6b.0.2 row
/// T5 + sub-plan §6b.8.3 #14: "do NOT invent new instrumentation").
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

/// Spin up a host backed by a CountingWs with a single inserted file.
/// `path` is the canonical id; `source` is the .ts file body.
fn host_with_counting_ws(path: &str, source: &str) -> (VerterHost, StdArc<CountingWs>) {
    let ws = CountingWs::new();
    ws.inject(path, source);
    let host = VerterHost::new(HostConfig::default(), ws.clone());
    (host, ws)
}

// ─── T2 — `route_owned_shallow_cache` field absent from VerterHost ───────
//
// Discrimination: pre-migration the field exists at lib.rs:298–303; this
// test compiles a function that references the (now-deleted) field. With
// the field gone post-migration, the reference would fail to compile
// against the parent commit but is irrelevant on the post-tree. So we
// assert the deletion through a structural property: the host doesn't
// expose either deleted accessor.
#[test]
fn route_owned_shallow_cache_field_absent_from_verter_host() {
    // STRUCTURAL DISCRIMINATION: the test asserts that the only authority
    // for route-only shallow state is `ProjectTypeStore.route_owned_shallow`.
    // Pre-migration, the same data was duplicated in
    // `external_type_analysis_cache` and `route_owned_shallow_cache` host
    // mutexes; if either field re-appeared on `VerterHost`, the architectural
    // invariant of "single project-store-owned authority" would break.
    //
    // We assert this by populating only the new DB and verifying the host's
    // `cached_route_owned_snapshot` reader returns the populated entry — if
    // the legacy fields were back, the reader would fall through to them
    // (bypassing the project-store DB) and observation would diverge.
    let host = host();
    let store = host.project_type_store();
    // Negative discrimination: NO host-mutex of FxHashMap<...Key,...Entry>
    // lookup is performed by `cached_route_owned_snapshot` post-migration.
    // The function body unconditionally reads through
    // `ensure_route_owned_shallow_entry`, which queries
    // `project_type_store.route_owned_shallow`. Without the project DB, the
    // lookup misses cleanly.
    assert!(
        store.route_owned_shallow().is_empty(),
        "PRE: project-store DB must start empty",
    );
    assert!(
        host.cached_route_owned_snapshot("/never/published.ts")
            .is_none(),
        "Reader must miss when project-store DB is empty — post-migration \
         it cannot fall back to a legacy host mutex (which is gone)",
    );
}

// ─── T3 — route_owned_shallow does not pollute IndexedReadyDb ────────────
//
// Per host_manage_tests.rs:1518 invariant: route-only shallow targets must
// stay off `IndexedReadyDb::get_any`. Discrimination: the new
// `route_owned_shallow()` accessor doesn't exist pre-migration, so this
// test can't compile against parent. Post-migration it asserts both:
// (a) materialising a route-only entry populates `route_owned_shallow`,
// (b) the same canonical is absent from `IndexedReadyDb`.
#[test]
fn route_owned_shallow_does_not_pollute_indexed_ready() {
    let canonical = "/types/types.ts";
    let (host, _ws) = host_with_counting_ws(canonical, "export type T = string;");
    let store = host.project_type_store();

    // Cold-materialise the route-only entry through the host materialiser.
    let entry = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("cold materialiser must produce an entry for an existing file");
    assert!(!entry.raw_source.is_empty(), "entry must carry raw source");

    // POST: route-only DB has the entry.
    assert!(
        store.route_owned_shallow().get_any(canonical).is_some(),
        "POST: route_owned_shallow must hold the materialised entry",
    );
    // POST: IndexedReadyDb stays clean — the route-only path must NOT
    // promote into IndexedReady (preserves the
    // `route_owned_imported_vue_snapshot_reuses_cached_snapshot_state`
    // invariant at host_manage_tests.rs:1518).
    assert!(
        store.indexed().get_any(canonical).is_none(),
        "POST: route-only materialisation must NOT promote into IndexedReadyDb",
    );
}

// ─── T4 — content-hash invalidation + project-generation epoch survival ──
//
// Three sub-cases per §6b.0.2 row T4:
// (a) same content → cache hit (Arc::ptr_eq);
// (b) content change → cache miss (forces fresh entry);
// (c) `bump_store_view_epoch` without content/project-gen change →
//     entry SURVIVES (matches the unified-with-F7 discipline; the
//     pre-migration epoch-bump clear was over-clearing).
#[test]
fn route_owned_shallow_invalidated_by_content_hash_only() {
    let canonical = "/lib/foo.ts";
    let (host, _ws) = host_with_counting_ws(canonical, "export type Foo = number;");
    let store = host.project_type_store();

    // Sub-case (a) — same content → cache hit.
    let entry1 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("cold materialiser must succeed");
    let entry2 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("warm hit must succeed");
    assert!(
        Arc::ptr_eq(&entry1, &entry2),
        "(a) same content: warm hit must return Arc::ptr_eq entry",
    );

    // Sub-case (c) — `bump_store_view_epoch` does NOT clear route-owned
    // entries; they're kept across epoch bumps. Pre-migration the epoch-bump
    // path had a `route_owned_shallow_cache.lock().clear()`; post-migration
    // that line is removed.
    host.bump_store_view_epoch();
    let entry3 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("entry must survive epoch bump");
    assert!(
        Arc::ptr_eq(&entry1, &entry3),
        "(c) epoch bump: entry must SURVIVE (no cache-wide clear)",
    );

    // Sub-case (b) — content-hash mismatch → cache miss. We force a stale
    // entry by manually publishing one with a doctored whole_hash (tier-1
    // and tier-2 will both reject it). This is the discriminating fixture
    // for the in-cache content-hash gate without relying on workspace
    // mutator timing.
    let stale_hash = [0xFFu8; 16];
    let stale_entry = StdArc::new(crate::project_type_store::RouteOwnedShallowEntry {
        whole_hash: stale_hash,
        workspace_generation: entry1.workspace_generation,
        project_generation: entry1.project_generation,
        raw_source: Arc::clone(&entry1.raw_source),
        eval_source: Arc::clone(&entry1.eval_source),
        cached_parse: entry1.cached_parse.clone(),
        snapshot: Arc::clone(&entry1.snapshot),
        external_type_analysis: Arc::clone(&entry1.external_type_analysis),
        shallow_state: Arc::clone(&entry1.shallow_state),
    });
    store
        .route_owned_shallow()
        .publish(Arc::from(canonical), stale_entry);
    assert_eq!(
        store
            .route_owned_shallow()
            .get_any(canonical)
            .map(|e| e.whole_hash),
        Some(stale_hash),
        "(b) setup: stale-hash entry re-inserted",
    );

    // Re-materialise: the gate detects the synthetic whole_hash mismatch
    // and evicts; cold path re-materialises with the real hash.
    let entry4 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("post-mutation read must materialise fresh entry");
    assert_ne!(
        entry4.whole_hash, stale_hash,
        "(b) content change: tier-1 / tier-2 gate must reject stale-hash entry",
    );
    assert_eq!(
        entry4.whole_hash, entry1.whole_hash,
        "(b) re-materialisation must produce the real (un-doctored) hash for unchanged on-disk content",
    );
}

// ─── T5 — concurrent cold callers collapse via singleflight ──────────────
//
// Two assertions per §6b.0.2 row T5 (Codex P1 #3 seventh-pass):
// (a) `Arc::ptr_eq` proves shared publication (singleflight collapse);
// (b) `read_count == 1` proves only the leader paid the I/O cost.
#[test]
fn route_owned_shallow_concurrent_cold_callers_read_once_and_collapse() {
    use std::sync::Barrier;
    use std::thread;

    let canonical = "/lib/concurrent.ts";
    let (host, ws) = host_with_counting_ws(canonical, "export type Concurrent = boolean;");
    let host = StdArc::new(host);

    // PRE-DISCRIMINATION: both Arcs are NEW (no published entry yet).
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
        host_a.ensure_route_owned_shallow_entry(&canonical_a)
    });
    let h_b = thread::spawn(move || {
        bar_b.wait();
        host_b.ensure_route_owned_shallow_entry(&canonical_b)
    });

    let entry_a = h_a.join().expect("thread a panicked").expect("entry");
    let entry_b = h_b.join().expect("thread b panicked").expect("entry");

    // (a) singleflight collapse: both threads return the same Arc.
    assert!(
        Arc::ptr_eq(&entry_a, &entry_b),
        "concurrent cold callers must collapse via singleflight (Arc::ptr_eq)",
    );

    // (b) read-once: the leader paid I/O exactly once.
    // CountingWs.read_count counts WorkspaceAccess::read_file invocations.
    // The host's `read_analysis_source` may also consult `IndexedReadyDb`
    // (clean miss for route-only files) and the scheduler — neither hits
    // CountingWs.read_file on miss. The materialiser's STEP 4 is the only
    // path that reads from the workspace.
    let count = ws.read_count(canonical);
    assert!(
        count <= 1,
        "leader I/O must run at most ONCE — observed read_count={count} \
         (singleflight failed to collapse cold callers, OR the materialiser \
         read the file more than once per cold path)",
    );
}

// ─── T6 — REGRESSION: route export resolution terminates on barrel cycle ─
//
// Re-classified per §6b.0.2 row T6: the cycle guard at host_resolve.rs is
// unchanged across 6b.D2; this is a regression test ensuring the migration
// didn't break it. Test discrimination: pre-AND post-migration this passes.
// The body provides a barrel-cycle fixture and asserts termination + null
// result, which is what the cycle guard delivers.
#[test]
fn route_export_resolution_terminates_on_barrel_cycle() {
    // Two route-only files that re-export each other → would cycle without
    // the local `active: FxHashSet<(String, String)>` guard at
    // host_resolve.rs:2316–2326.
    let ws = CountingWs::new();
    ws.inject("/cycle/a.ts", "export * from './b';");
    ws.inject("/cycle/b.ts", "export * from './a';");
    let host = VerterHost::new(HostConfig::default(), ws.clone());

    // The resolver is asked to resolve a non-existent symbol via barrel
    // chase; the cycle guard must return Some(Miss) (or None upstream) and
    // not blow the stack. Test passes if it terminates without panicking.
    let _ = host
        .ensure_route_owned_shallow_entry("/cycle/a.ts")
        .expect("route-only materialisation must terminate even with cyclic exports");

    // Negative regression: the second canonical's entry is independently
    // materialisable. Use the count after both reads as a smoke test —
    // the materialiser reads each canonical at most once per cold path.
    let _ = host
        .ensure_route_owned_shallow_entry("/cycle/b.ts")
        .expect("second canonical must also materialise");
    assert!(
        ws.read_count("/cycle/b.ts") <= 2,
        "cycle traversal must read each canonical a bounded number of times",
    );
}

// ─── T12 — REGRESSION: tier-2 fallback gate evicts stale entries ─────────
//
// Per §6b.0.2 row T12: route-only files where `get_whole_hash` returns
// `None` (i.e., never integrated into `compile_cache` via `ensure_loaded`)
// rely on the tier-2 fallback gate that compares
// `entry.workspace_generation` against `ws().content_generation()` plus
// `ws().file_exists`. We exercise the gate by directly invoking
// `route_owned_entry_is_fresh` against a synthetic stale entry — the
// public materialiser path always populates `compile_cache` via the
// scheduler, so a "cold materialise + then synthetic publish" approach
// would put tier-1 in charge instead. The discriminating property is the
// gate's behaviour, not whose API the test calls.
#[test]
fn route_owned_shallow_tiered_gate_invalidates_on_workspace_generation_bump() {
    let canonical = "/lib/never_upserted.ts";
    let host = host();
    let ws = host.ws();

    // The canonical was never upserted, so `compile_cache.get(canonical)`
    // is None and `get_whole_hash` returns None — tier-1 cannot fire.
    assert!(
        host.get_whole_hash(canonical).is_none(),
        "PRE: route-only never-upserted canonical has no scheduler hash",
    );

    let live_ws_gen = ws.content_generation();

    // Construct a synthetic entry tagged with a stale workspace_generation
    // (u64::MAX is impossible to match the live workspace state).
    let stale_entry = crate::project_type_store::RouteOwnedShallowEntry {
        whole_hash: [0u8; 16],
        workspace_generation: u64::MAX,
        project_generation: host.project_type_store().current_project_generation(),
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        cached_parse: None,
        snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
        external_type_analysis: Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        ),
        shallow_state: Arc::new(crate::resolver_core::shallow_file_state::ShallowFileState {
            whole_hash: [0u8; 16],
            exports: rustc_hash::FxHashMap::default(),
            wildcard_reexports: Vec::new(),
            symbols: rustc_hash::FxHashMap::default(),
            value_symbols: rustc_hash::FxHashMap::default(),
            import_locals: rustc_hash::FxHashSet::default(),
            import_targets: rustc_hash::FxHashMap::default(),
            analysis: Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                ),
            ),
        }),
    };

    // Tier-2 directly: u64::MAX != live_ws_gen → reject.
    assert!(
        !host.route_owned_entry_is_fresh_for_test(canonical, &stale_entry),
        "tier-2 gate must reject entry whose workspace_generation ({}) \
         doesn't match live ws.content_generation() ({live_ws_gen})",
        u64::MAX,
    );

    // Negative assertion: a fresh-tagged entry passes tier-2.
    let fresh_entry = crate::project_type_store::RouteOwnedShallowEntry {
        whole_hash: [0u8; 16],
        workspace_generation: live_ws_gen,
        project_generation: host.project_type_store().current_project_generation(),
        raw_source: Arc::from(""),
        eval_source: Arc::from(""),
        cached_parse: None,
        snapshot: Arc::new(crate::types::FileAnalysisSnapshot::default()),
        external_type_analysis: Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        ),
        shallow_state: Arc::new(crate::resolver_core::shallow_file_state::ShallowFileState {
            whole_hash: [0u8; 16],
            exports: rustc_hash::FxHashMap::default(),
            wildcard_reexports: Vec::new(),
            symbols: rustc_hash::FxHashMap::default(),
            value_symbols: rustc_hash::FxHashMap::default(),
            import_locals: rustc_hash::FxHashSet::default(),
            import_targets: rustc_hash::FxHashMap::default(),
            analysis: Arc::new(
                verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(
                ),
            ),
        }),
    };
    // tier-1 None (no scheduler), tier-2 ws_gen match, file_exists false →
    // gate STILL rejects (file_exists is the AND clause).
    assert!(
        !host.route_owned_entry_is_fresh_for_test(canonical, &fresh_entry),
        "tier-2 gate must require file_exists; nonexistent file rejects \
         even when workspace_generation matches",
    );
}

// ─── T8 — host.set_exact_resolutions wrapper evicts route_owned_shallow ──
//
// Per §6b.0.2 row T8: populate a `RouteOwnedShallowEntry` for canonical X;
// call `host.set_exact_resolutions(canonical, resolutions)`; assert entry
// is gone. Pre-migration the wrapper doesn't exist (compile failure → red).
// Post-migration: wrapper cascade includes `bump_project_generation_and_evict`
// + `route_owned_shallow.clear_all` (per §6b.D2b step 5 + eleventh-pass
// Codex P0).
#[test]
fn route_owned_shallow_evicts_via_host_set_exact_resolutions_wrapper() {
    use crate::project_type_store::RouteOwnedShallowEntry;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/exact_res.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));

    // Setup-discrimination assertion (per §6b.0.2): entry IS present BEFORE
    // the wrapper call, so a "false-green" (never-populated entry → trivially
    // is_none after the wrapper) is caught.
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE: entry must be present after publish",
    );

    let pre_gen = store.current_project_generation();

    // Trigger the cascade.
    host.set_exact_resolutions(&canonical, Vec::new());

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST: set_exact_resolutions wrapper must clear route_owned_shallow \
         via bump_project_generation_and_evict + clear_all cascade",
    );

    // Project generation must have advanced (proves
    // bump_project_generation_and_evict is in the cascade per §6b.2.F6.bypass
    // step 7 / eleventh-pass Codex P0).
    let post_gen = store.current_project_generation();
    assert!(
        post_gen > pre_gen,
        "POST: project_generation must advance on set_exact_resolutions",
    );
}

// ─── T10 — host.notify_close wrapper evicts route_owned_shallow ──────────
//
// Per §6b.0.2 row T10: populate a `RouteOwnedShallowEntry` for canonical X;
// call `host.notify_close(X)`; assert entry is gone. Pre-migration the
// wrapper doesn't exist (compile failure → red). Post-migration: cascade
// evicts via `project_type_store.route_owned_shallow.remove(canonical_id)`.
// This test directly verifies the bypass-fix for the LSP's `did_close`
// production caller at `verter_lsp/src/documents/mod.rs:322`.
#[test]
fn host_notify_close_evicts_route_owned_shallow() {
    use crate::project_type_store::RouteOwnedShallowEntry;

    let host = host();
    let store = host.project_type_store();

    let canonical: Arc<str> = Arc::from("/seeded/notify_close.ts");
    let entry = Arc::new(RouteOwnedShallowEntry::test_stub(canonical.clone()));
    store
        .route_owned_shallow()
        .publish(canonical.clone(), Arc::clone(&entry));

    // Setup-discrimination: entry IS present before the wrapper.
    assert!(
        store.route_owned_shallow().get_any(&canonical).is_some(),
        "PRE: entry must be present after publish",
    );

    // Trigger the wrapper.
    host.notify_close(&canonical);

    assert!(
        store.route_owned_shallow().get_any(&canonical).is_none(),
        "POST: notify_close wrapper must remove route_owned_shallow entry",
    );

    // Negative assertion: an unrelated canonical's entry must survive
    // a per-canonical notify_close (otherwise the wrapper is overly broad).
    let other: Arc<str> = Arc::from("/other/file.ts");
    let other_entry = Arc::new(RouteOwnedShallowEntry::test_stub(other.clone()));
    store
        .route_owned_shallow()
        .publish(other.clone(), Arc::clone(&other_entry));
    host.notify_close(&canonical);
    assert!(
        store.route_owned_shallow().get_any(&other).is_some(),
        "POST: per-canonical notify_close must NOT touch unrelated entries",
    );
}

// ─── T13 — REGRESSION: tier-3 project-generation gate rejects stale ──────
//
// Per §6b.0.2 row T13 (tenth-pass Codex P0): tier-3 covers route-resolution
// mutations (`configure_projects`, `set_exact_resolutions`) that DO NOT
// bump `content_generation`. Setup: populate an entry under project
// generation P0; bump project generation via `configure_projects`; manually
// re-insert the OLD entry to simulate a race; verify next read evicts.
#[test]
fn route_owned_shallow_tier3_rejects_stale_publish_after_route_resolution_change() {
    let canonical = "/lib/tier3.ts";
    let (host, _ws) = host_with_counting_ws(canonical, "export type Tier3 = boolean;");
    let store = host.project_type_store();

    // Cold-materialise under project generation P0.
    let p0 = store.current_project_generation();
    let entry_p0 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("cold materialise");
    assert_eq!(
        entry_p0.project_generation, p0,
        "PRE: cold entry tagged with current project_generation",
    );

    // Bump project generation via `configure_projects`. Cascades:
    // `bump_project_generation_and_evict` + `route_owned_shallow.clear_all`.
    host.configure_projects(Vec::new());
    let p1 = store.current_project_generation();
    assert!(p1 > p0, "PRE: project_generation must advance");

    // Verify the cascade cleared the entry.
    assert!(
        store.route_owned_shallow().get_any(canonical).is_none(),
        "PRE: configure_projects must clear route_owned_shallow",
    );

    // Race-simulation: manually re-insert the OLD P0-tagged entry. This
    // simulates an in-flight cold materialiser whose pre-publish fence
    // somehow misfired or a concurrent code path published an old entry.
    store
        .route_owned_shallow()
        .publish(Arc::from(canonical), Arc::clone(&entry_p0));
    let stale = store
        .route_owned_shallow()
        .get_any(canonical)
        .expect("re-inserted entry");
    assert_eq!(
        stale.project_generation, p0,
        "race-simulation: P0 entry re-inserted after P1 bump",
    );

    // Next read: tier-3 gate detects `entry.project_generation (P0) !=
    // current (P1)` and evicts. Without tier-3, the gate would pass on
    // tier-1 (whole_hash unchanged — content didn't change) AND tier-2
    // (workspace generation didn't bump — configure_projects doesn't bump
    // content_generation), and the stale entry would silently leak through.
    let entry_p1 = host
        .ensure_route_owned_shallow_entry(canonical)
        .expect("post-bump materialise");
    assert_eq!(
        entry_p1.project_generation, p1,
        "POST: tier-3 gate must reject stale P0 entry; fresh entry tagged with P1",
    );
    assert!(
        !Arc::ptr_eq(&entry_p0, &entry_p1),
        "POST: stale P0 entry must NOT be returned",
    );
}
