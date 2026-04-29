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
