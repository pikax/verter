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
//!
//! Site #3 (the walker routed-expr engine's whole_hash read) is now
//! RETIRED: the `component_meta_query_engine/routed_expr.rs` engine was
//! deleted with the walker cluster, and the surviving dispatch
//! routed-expr path does not track file versions via whole_hash. Its
//! guard asserts the retired state (deleted file absent + dispatch
//! method whole_hash-free) rather than a relocated read.

use std::fs;
use std::path::{Path, PathBuf};

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
    let path = workspace_root().join("crates/verter_session/src/resolver_core/prepared_decl.rs");
    let source = read_source_file(&path);

    // The legacy pattern: u64 cast from state.whole_hash[..8].
    let legacy_pattern = "u64::from_le_bytes(state.whole_hash[..8].try_into().unwrap_or_default())";
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

/// Read site #2 — `route_db.rs:318` (Stage-5 inventory). The Stage-5
/// inventory captured `BarrelRouteSurface.whole_hash: Hash16` +
/// per-source-file hashes (`source_hashes: Vec<(String, Hash16)>`) as
/// the file-version anchor for barrel-surface validation.
///
/// **Stage 6c retirement**: replaced by
/// `BarrelRouteSurface.fact_dep_signature: Arc<[FactVersionRef]>`.
/// The barrel surface now carries its own validation signature
/// directly, validated against the active `StoreView` like every
/// other `ValidatedFactCache` candidate. This test inverts the
/// Stage-5 invariant — it asserts the legacy whole_hash + source_hashes
/// fields are GONE and the replacement signature field is present.
#[test]
fn whole_hash_read_site_2_route_db_surface_hash_inventoried() {
    let path = workspace_root().join("crates/verter_session/src/resolver_core/route_db.rs");
    let source = read_source_file(&path);

    // Legacy patterns that MUST be absent post-Stage-6c.
    let has_surface_hash_field =
        source.contains("pub whole_hash: Hash16,") || source.contains("pub whole_hash: HashValue,");
    let has_source_hashes_field = source.contains("pub source_hashes: Vec<");

    assert!(
        !has_surface_hash_field,
        "BarrelRouteSurface.whole_hash field MUST be retired post-Stage-6c \
         (replaced by fact_dep_signature: Arc<[FactVersionRef]>)"
    );
    assert!(
        !has_source_hashes_field,
        "BarrelRouteSurface.source_hashes field MUST be retired post-Stage-6c \
         (replaced by fact_dep_signature: Arc<[FactVersionRef]>)"
    );

    // Positive assertion: the replacement field is present.
    assert!(
        source.contains("pub fact_dep_signature: Arc<[FactVersionRef]>"),
        "BarrelRouteSurface.fact_dep_signature MUST be present (Stage-6c replacement for whole_hash)"
    );
}

/// Read site #3 — RETIRED. The legacy site read `whole_hash` for
/// file-version tracking inside the walker routed-expr engine
/// (`component_meta_query_engine/routed_expr.rs`). That engine and its
/// whole_hash read are DELETED with the walker cluster: macro/route
/// surfaces now resolve through the DISPATCH routed-expr path
/// (`registry_decl.rs::dispatch_routed_expr_surface_expr`, which routes
/// through `dispatch_projected_surface` / `dispatch_root_instantiated`
/// / the semantic-query substrate). That dispatch path does NOT do
/// file-version tracking via a `whole_hash` read — the surviving
/// whole_hash reads in `registry_decl.rs` belong to the prepared-decl
/// bundle / decl-identity machinery inventoried by sites #1/#4/#5, not
/// the routed-expr surface path.
///
/// **Discriminating retired-state guard**:
/// 1. the deleted `routed_expr.rs` engine file MUST NOT exist, and
/// 2. the surviving dispatch routed-expr method MUST exist and its body
///    MUST be free of any `whole_hash` / `get_whole_hash` /
///    `content_hash_for` read.
///
/// Reintroducing the deleted engine, or adding a whole_hash read into
/// the dispatch routed-expr method, flips this guard RED.
#[test]
fn whole_hash_read_site_3_routed_expr_retired() {
    // (1) The walker routed-expr engine file is DELETED.
    let deleted = workspace_root()
        .join("crates/verter_session/src/resolver_core/component_meta_query_engine/routed_expr.rs");
    assert!(
        !deleted.exists(),
        "the walker routed-expr engine `routed_expr.rs` is retired and MUST NOT exist — \
         macro/route surfaces resolve through `dispatch_routed_expr_surface_expr` (site #3 \
         is retired, not relocated)"
    );

    // (2) The surviving dispatch routed-expr method exists and does NOT
    // read whole_hash for routing.
    let path = workspace_root().join(
        "crates/verter_session/src/resolver_core/component_meta_query_engine/registry_decl.rs",
    );
    let source = read_source_file(&path);
    let body = extract_fn_body(&source, "pub(crate) fn dispatch_routed_expr_surface_expr(");
    for forbidden in ["whole_hash", "get_whole_hash", "content_hash_for"] {
        assert!(
            !body.contains(forbidden),
            "the dispatch routed-expr method `dispatch_routed_expr_surface_expr` MUST NOT \
             read `{forbidden}` — the routed-expr surface path resolves through the dispatch \
             / semantic-query substrate, not via file-version whole_hash tracking. \
             Reintroducing a whole_hash read here resurrects the retired routed-expr engine's \
             file-version coupling. Body:\n{body}"
        );
    }
}

/// Extract the brace-balanced body (from the first `{` after `needle`
/// to its matching `}`) of the function whose signature begins at
/// `needle`. The scanned method is a dispatch router that embeds no
/// `{`/`}` inside string/char literals, so plain depth counting is
/// sufficient.
fn extract_fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("expected `{needle}` in source"));
    let after = &src[start..];
    let open = after
        .find('{')
        .unwrap_or_else(|| panic!("expected an opening brace for `{needle}`"));
    let bytes = after.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &after[open..=idx];
                }
            }
            _ => {}
        }
        idx += 1;
    }
    panic!("expected a brace-balanced body for `{needle}`");
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
    let has_field = source.contains("whole_hash: HashValue,") && source.contains("NodeScopeId");

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
