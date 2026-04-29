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
