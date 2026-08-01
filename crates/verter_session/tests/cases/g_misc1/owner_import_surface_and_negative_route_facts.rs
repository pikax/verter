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

/// `build_owner_import_surface` accepts a
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
         fact_dep_signature (R3/R26/R28)."
    );
    assert!(
        source.contains("for fact in chain_facts"),
        "build_owner_import_surface MUST iterate the supplied chain_facts and de-duplicate \
         them into the surface's fact_dep_signature."
    );
}

/// The producer site calls the `_with_facts`
/// variant of `resolve_imported_type_root` and threads the
/// returned facts into the surface.
#[test]
fn owner_import_surface_producer_calls_resolve_imported_type_root_with_facts() {
    let source = read_file("src/host_manage/prepared_decl.rs");
    assert!(
        source.contains("resolve_imported_type_root_with_facts"),
        "owner_import_surface producer MUST call \
         `resolve_imported_type_root_with_facts` and thread the returned facts into \
         the surface (R3/R26/R28)."
    );
    assert!(
        source.contains("chain_facts"),
        "owner_import_surface producer MUST accumulate route-walk facts into a \
         `chain_facts` accumulator."
    );
}

/// `OwnerImportSurfaceDb` exposes a view-aware
/// lookup (`get_with_view`) that fact-validates the cached entry
/// against the caller's `StoreView`. The legacy `get` stays
/// reserved for tests + introspection.
#[test]
fn owner_import_surface_db_has_view_aware_lookup() {
    let source = read_file("src/owner_import_surface.rs");
    assert!(
        source.contains("pub fn get_with_view"),
        "OwnerImportSurfaceDb MUST expose `get_with_view` so production callers fact-validate \
         the cached entry against the live store view (R3)."
    );
    assert!(
        source.contains("view.validates(fact)"),
        "OwnerImportSurfaceDb::get_with_view MUST iterate the surface's \
         fact_dep_signature and validate each fact against the caller's StoreView."
    );
}

/// INVERTED-POLARITY successor to the per-entry freshness-oracle guard.
///
/// The oracle it pinned (`DerivedRawState::import_route_entry_is_generation_current`)
/// existed because `DerivedRawState.import_routes` held HOST-MEMOISED
/// positives — a duplicate of the workspace's own bounded owner-edge
/// candidate slot — and, being a plain map with no witness, needed a
/// global `content_generation` equality to decide whether one was still
/// true. That was the last global-generation warm-resolution validity
/// test in the session; the memo and the oracle are DELETED together.
///
/// What survives, and is asserted here:
///
/// * the oracle and its `PositiveRouteStamp` sidecar are gone;
/// * the surviving positives are CALLER-SUPPLIED authoritative routes
///   only, which serve until the caller replaces them.
///
/// This scan asserts only ABSENCE — deleted symbols really are deleted,
/// which a source scan can decide. The surviving positive property (the
/// reader refuses every known-miss and still serves a positive) is NOT
/// asserted here: a scan for the literal `if resolution.is_known_miss()
/// {` branch cannot distinguish an arm that refuses from an arm that
/// falls through to `Some(resolution)`, because the text is identical in
/// both trees. That property is owned behaviourally by
/// `verter_session::negative_import_route_tests::\
/// cached_import_route_resolution_refuses_a_known_miss_and_serves_a_positive`,
/// which calls the reader and goes RED on the fall-through.
#[test]
fn cached_import_route_resolution_carries_no_generation_oracle() {
    let reader = read_file("src/host_manage/prepared_decl.rs");
    assert!(
        !reader.contains("import_route_entry_is_generation_current"),
        "the per-entry generation oracle is DELETED — a global content \
         generation is not a warm-resolution validity test."
    );
    let oracle = read_file("src/types.rs");
    for retired in [
        "import_route_entry_is_generation_current",
        "import_route_is_generation_current",
        "import_routes_positive_recorded_at_generation",
        "PositiveRouteStamp",
        "import_routes_known_miss_recorded_at_generation",
    ] {
        assert!(
            !oracle.contains(retired),
            "`{retired}` is DELETED: global-generation equality is no longer a \
             warm-resolution validity test."
        );
    }
}
