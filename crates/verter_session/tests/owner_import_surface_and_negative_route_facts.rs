//! R3/R26/R28 producer-substrate guards. Each guard pins one of the
//! two corrective gap fixes in the cross-file fact-graph wiring:
//!
//! 1. `OwnerImportSurface.fact_dep_signature` records every
//!    barrel-chain participant's `Route` fact.
//! 2. `cached_import_route_resolution` rejects known-miss entries
//!    once the workspace's `content_generation` advances past the
//!    value recorded at admission.
//!
//! These guards are pure source-grep checks: they make the two
//! substrate pieces hard to revert silently.

use std::fs;
use std::path::PathBuf;

fn read_file(rel: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Gap 1 substrate: `build_owner_import_surface` accepts a
/// `chain_facts` parameter and folds it into `fact_dep_signature`.
/// Without that parameter the producer cannot thread barrel-chain
/// facts into the cached surface.
#[test]
fn owner_import_surface_builder_threads_chain_facts() {
    let source = read_file("src/owner_import_surface.rs");
    assert!(
        source.contains("chain_facts: Vec<crate::resolver_core::FactVersionRef>"),
        "build_owner_import_surface MUST accept `chain_facts: Vec<FactVersionRef>` so the \
         producer can thread the route-walk facts into the cached surface's \
         fact_dep_signature (Gap 1, R3/R26/R28)."
    );
    assert!(
        source.contains("for fact in chain_facts"),
        "build_owner_import_surface MUST iterate the supplied chain_facts and de-duplicate \
         them into the surface's fact_dep_signature."
    );
}

/// Gap 1 substrate: the producer site calls the `_with_facts`
/// variant of `resolve_imported_type_root` and threads the
/// returned facts into the surface.
#[test]
fn owner_import_surface_producer_calls_resolve_imported_type_root_with_facts() {
    let source = read_file("src/host_manage/prepared_decl.rs");
    assert!(
        source.contains("resolve_imported_type_root_with_facts"),
        "owner_import_surface producer MUST call \
         `resolve_imported_type_root_with_facts` and thread the returned facts into \
         the surface (Gap 1, R3/R26/R28)."
    );
    assert!(
        source.contains("chain_facts"),
        "owner_import_surface producer MUST accumulate route-walk facts into a \
         `chain_facts` accumulator (Gap 1)."
    );
}

/// Gap 1 substrate: `OwnerImportSurfaceDb` exposes a view-aware
/// lookup (`get_with_view`) that fact-validates the cached entry
/// against the caller's `StoreView`. The legacy `get` stays
/// reserved for tests + introspection.
#[test]
fn owner_import_surface_db_has_view_aware_lookup() {
    let source = read_file("src/owner_import_surface.rs");
    assert!(
        source.contains("pub fn get_with_view"),
        "OwnerImportSurfaceDb MUST expose `get_with_view` so production callers fact-validate \
         the cached entry against the live store view (Gap 1, R3)."
    );
    assert!(
        source.contains("view.validates(fact)"),
        "OwnerImportSurfaceDb::get_with_view MUST iterate the surface's \
         fact_dep_signature and validate each fact against the caller's StoreView."
    );
}

/// Gap 2 substrate: `DerivedRawState` carries a per-specifier
/// `import_routes_known_miss_recorded_at_generation` map that
/// records the workspace `content_generation` at known-miss
/// admission.
#[test]
fn derived_raw_state_records_known_miss_generation() {
    let source = read_file("src/types.rs");
    assert!(
        source.contains("import_routes_known_miss_recorded_at_generation"),
        "DerivedRawState MUST carry a per-specifier \
         `import_routes_known_miss_recorded_at_generation: FxHashMap<String, u64>` so \
         the reader can detect content_generation advancement and force a fresh \
         resolution (Gap 2, R3/R26/R28)."
    );
}

/// Gap 2 substrate: the reader (`cached_import_route_resolution`)
/// gates known-miss resolutions on
/// `workspace.content_generation() > recorded_at`.
#[test]
fn cached_import_route_resolution_gates_known_miss_on_generation() {
    let source = read_file("src/host_manage/prepared_decl.rs");
    assert!(
        source.contains("import_route_is_known_miss"),
        "cached_import_route_resolution MUST invoke `import_route_is_known_miss` to \
         identify cached negatives (Gap 2)."
    );
    assert!(
        source.contains("import_routes_known_miss_recorded_at_generation"),
        "cached_import_route_resolution MUST consult \
         `import_routes_known_miss_recorded_at_generation` before serving a cached \
         negative (Gap 2, R3)."
    );
    assert!(
        source.contains("content_generation()") && source.contains("> recorded_at"),
        "cached_import_route_resolution MUST refuse the cached negative when \
         `current_generation > recorded_at` (Gap 2)."
    );
}

/// Gap 2 substrate: the bundler producer (`set_import_dependencies`)
/// populates the per-specifier generation sidecar for every
/// known-miss it admits.
#[test]
fn set_import_dependencies_records_known_miss_generation() {
    let source = read_file("src/host_manage/analysis_io.rs");
    assert!(
        source.contains("import_routes_known_miss_recorded_at_generation"),
        "set_import_dependencies MUST populate \
         `import_routes_known_miss_recorded_at_generation` for every known-miss admission \
         so the reader can detect content_generation advancement (Gap 2)."
    );
}
