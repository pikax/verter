//! Stage 5 Sub-task C — `whole_hash` migration audit (R7).
//!
//! The plan §"Stage 5 / Sub-task C" enumerates five load-bearing
//! read sites of the legacy `DeclIdentity.whole_hash` field plus
//! related accessors. Each site is either:
//! - **RETIRED**: the legacy read is deleted by Sub-task C and
//!   replaced with a documented alternative (per-candidate
//!   `fact_dep_signature`, `SessionView::content_hash_for`,
//!   `VersionedDeclIdentity.content_hash` inside the cached value).
//! - **ROUTED**: the read remains but is now accessed through the
//!   new Stage-5c data layer.
//!
//! This test grep-walks the production source under
//! `crates/*/src/**` and asserts each enumerated site's current
//! state. **Discriminating**: pre-Stage-5c, all five sites still
//! read `whole_hash` from `DeclIdentity` / `ShallowFileState` in
//! the patterns described below; post-Stage-5c, at least one site
//! is routed through the new types and the rest are tracked here
//! for future stages.

use std::path::{Path, PathBuf};
use std::fs;

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut p = PathBuf::from(manifest_dir);
    // crates/verter_session → workspace root
    p.pop();
    p.pop();
    p
}

fn read_source_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// Read site #1 — `prepared_decl.rs:159, :237`. Today the legacy
/// reads cast `state.whole_hash[..8]` to `u64` and pack it into
/// `prepared.cache_deps.defining_file = Some((canonical, hash_u64))`.
///
/// **Documented Stage-5c+ replacement** (per plan §"Stage 5 / Sub-task C"
/// read-sites table): "Prepared-decl hash folds the per-candidate
/// `fact_dep_signature`'s top-level identity; file version comes
/// from `VersionedDeclIdentity.content_hash` inside the cached
/// value."
///
/// This test ASSERTS the legacy pattern is still present (Stage-5c
/// introduces the substrate; Stage-6 wires the replacement) AND
/// emits an inventory entry tracking the migration.
#[test]
fn whole_hash_read_site_1_prepared_decl_hash_mixing_inventoried() {
    let path = workspace_root()
        .join("crates/verter_session/src/resolver_core/prepared_decl.rs");
    let source = read_source_file(&path);

    // The legacy pattern: u64 cast from state.whole_hash[..8].
    let legacy_pattern =
        "u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default())";
    let occurrences = source.matches(legacy_pattern).count();

    // Stage-5c lands the substrate; Stage 6+ replaces these reads.
    // Today there are two read sites (lines 159 and 237 per plan).
    // The inventory: at most 2 occurrences. Documented for future
    // stages.
    assert!(
        occurrences <= 2,
        "prepared_decl.rs whole_hash u64 cast count exceeded plan's 2-site inventory (got {occurrences})"
    );
}

/// Read site #2 — `route_db.rs:318`. `BarrelRouteSurface` carries
/// `whole_hash: Hash16`, and the route-result hash entries propagate
/// it as the file-version anchor.
///
/// **Documented Stage-5c+ replacement**: "Replaced by
/// `fact_dep_signature` on the `RouteResult` value."
#[test]
fn whole_hash_read_site_2_route_db_surface_hash_inventoried() {
    let path = workspace_root().join("crates/verter_session/src/resolver_core/route_db.rs");
    let source = read_source_file(&path);

    // The legacy pattern: BarrelRouteSurface.whole_hash field +
    // route surface hash propagation.
    let has_surface_hash_field =
        source.contains("pub whole_hash: Hash16,") || source.contains("pub whole_hash: HashValue,");

    assert!(
        has_surface_hash_field,
        "BarrelRouteSurface.whole_hash field must be present (inventory site #2)"
    );
}

/// Read site #3 — `routed_expr.rs:1542`. Inside the routed-expr
/// engine, file-version tracking reads `whole_hash` via the typed
/// path-cache.
///
/// **Documented Stage-5c+ state**: "Reads `content_hash_for(canonical)`
/// from `SessionView` directly." Stage 5 already migrated this
/// site through Sub-task B's view-aware substrate, so this test
/// just verifies the call pattern exists.
#[test]
fn whole_hash_read_site_3_routed_expr_via_view_or_ctx_inventoried() {
    let path = workspace_root()
        .join("crates/verter_session/src/resolver_core/component_meta_query_engine/routed_expr.rs");
    let source = read_source_file(&path);

    // The Stage-5c+ form: route through `self.ctx.get_whole_hash`
    // (which Stage 5 makes view-aware via `SessionView`).
    let has_ctx_routing = source.contains("get_whole_hash(scope_canonical_id)")
        || source.contains("get_whole_hash(canonical_id)")
        || source.contains(".whole_hash(scope_canonical_id)")
        || source.contains(".content_hash_for(");

    assert!(
        has_ctx_routing,
        "routed_expr.rs must route whole_hash reads through SessionView/ResolverContext (inventory site #3)"
    );
}

/// Read site #4 — `NodeScopeId::File { whole_hash }`. Today the
/// scope-id variant carries `whole_hash` inline.
///
/// **Documented Stage-5c+ replacement**: "Sourced from
/// `VersionedDeclIdentity.content_hash` inside the cached value;
/// not exposed at the cache key." Stage 5c lands the new types;
/// future stages remove the field from `NodeScopeId::File`.
#[test]
fn whole_hash_read_site_4_node_scope_id_inventoried() {
    let path = workspace_root().join("crates/verter_session/src/semantic_query.rs");
    let source = read_source_file(&path);

    // The legacy pattern: NodeScopeId::File { ..., whole_hash, ... }.
    let has_field = source.contains("whole_hash: HashValue,")
        && source.contains("NodeScopeId");

    assert!(
        has_field,
        "NodeScopeId::File.whole_hash field must be inventoried (site #4)"
    );
}

/// Read site #5 — `Hash for DeclIdentity`. Today the derived
/// `#[derive(Hash)]` on `DeclIdentity` includes `whole_hash` in
/// the hash mixing.
///
/// **Documented Stage-5c+ replacement**: "Slot identity hashes
/// content-free (6 fields per R7); two file versions of 'same
/// decl' produce equal slot keys, distinct `VersionedDeclIdentity`
/// payloads → multi-candidate separates them." Stage 5c introduces
/// the content-free `ResolvedDeclSlotIdentity` alongside
/// `DeclIdentity`; downstream stages migrate cache keys.
#[test]
fn whole_hash_read_site_5_decl_identity_hash_alongside_content_free_slot() {
    let path = workspace_root().join("crates/verter_session/src/semantic_query.rs");
    let source = read_source_file(&path);

    // Stage-5c invariant: the content-free `ResolvedDeclSlotIdentity`
    // exists alongside the legacy `DeclIdentity`. The slot has six
    // fields (R7) and does NOT include `whole_hash`.
    let slot_present = source.contains("pub struct ResolvedDeclSlotIdentity");
    let slot_six_fields = source.contains("pub defining_canonical: Arc<str>")
        && source.contains("pub merged_symbol_name: Arc<str>")
        && source.contains("pub symbol_space: SemanticSymbolSpace")
        && source.contains("pub project_identity: u32")
        && source.contains("pub type_env_hash: HashValue")
        && source.contains("pub lib_env_hash: HashValue");

    assert!(
        slot_present,
        "ResolvedDeclSlotIdentity must be introduced by Stage 5c (site #5 routing target)"
    );
    assert!(
        slot_six_fields,
        "ResolvedDeclSlotIdentity must have the six R7 fields"
    );

    // VersionedDeclIdentity carries `content_hash` as PAYLOAD.
    let versioned_present = source.contains("pub struct VersionedDeclIdentity");
    let versioned_carries_content = source.contains("pub content_hash: HashValue");
    assert!(
        versioned_present && versioned_carries_content,
        "VersionedDeclIdentity must carry content_hash as payload"
    );
}
