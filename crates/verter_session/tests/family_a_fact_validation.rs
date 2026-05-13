//! R3/R26/R28 arch guard for the 9 Family A inner caches that
//! migrated from `dep_signature: DepSignature` to
//! `fact_dep_signature: Arc<[FactVersionRef]>` in Stage 7C.A1b.
//!
//! Family A caches:
//!   - `ImportedRegistryEntry`
//!   - `DeclarationLookupEntry`
//!   - `ResolvabilityEntry`
//!   - `OwnerCollectionEntry`
//!   - `PreparedTargetEntry`
//!   - `MaterializeMemoEntry`
//!   - `PreparedSurfaceEntry`
//!   - `PreparedMemberEntry`
//!   - `RoutedExprSurfaceEntry`
//!
//! Each entry MUST carry `fact_dep_signature: Arc<[FactVersionRef]>`
//! and MUST NOT carry the legacy `dep_signature: DepSignature`. The
//! validator routes through
//! [`crate::fact_signature_helpers::validate_fact_signature`] on
//! warm hits and the producer through
//! [`engine_fact_signature_for_canonical_member`] /
//! [`engine_fact_signature_for_canonical_surface`] on cold compute.
//!
//! ## Source-grep arch guards
//!
//! The first test scans `component_meta_caches.rs` for the migrated
//! field name shape; the second confirms the legacy field name is
//! gone. The third pair confirms the producer call-sites use the
//! new `engine_fact_signature_*` helpers (not the legacy
//! `engine_dep_signature_for_canonical`).

use std::fs;
use std::path::PathBuf;

fn read_session_source(relative: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Every Family A entry struct carries `fact_dep_signature:
/// Arc<[FactVersionRef]>`. Source-grep arch guard.
#[test]
fn family_a_entries_carry_fact_dep_signature() {
    let src = read_session_source("component_meta_caches.rs");
    const ENTRIES: &[&str] = &[
        "ImportedRegistryEntry",
        "DeclarationLookupEntry",
        "ResolvabilityEntry",
        "OwnerCollectionEntry",
        "PreparedTargetEntry",
        "MaterializeMemoEntry",
        "PreparedSurfaceEntry",
        "PreparedMemberEntry",
        "RoutedExprSurfaceEntry",
    ];
    for entry in ENTRIES {
        let struct_decl = format!("pub struct {entry} {{");
        let idx = src
            .find(&struct_decl)
            .unwrap_or_else(|| panic!("expected `{struct_decl}` in component_meta_caches.rs"));
        // Window from struct start to the next `}` at column 0
        // (struct close).
        let after = &src[idx..];
        let end = after
            .find("\n}")
            .unwrap_or_else(|| panic!("expected struct close for {entry}"));
        let window = &after[..end];
        assert!(
            window.contains("fact_dep_signature: Arc<[FactVersionRef]>"),
            "{entry} must carry `fact_dep_signature: Arc<[FactVersionRef]>` (R28 migration), \
             but the struct body did not contain that field. Window:\n{window}"
        );
        assert!(
            !window.contains("dep_signature: DepSignature"),
            "{entry} must NOT carry the legacy `dep_signature: DepSignature` field after the \
             R28 migration. Both fields coexisting would violate the clean cutover. Window:\n{window}"
        );
    }
}

/// The legacy `engine_dep_signature_for_canonical` helper is no
/// longer called by Family A producers. Per the R28 path-precise
/// contract, callers select one of:
/// - `engine_fact_signature_for_canonical_member` — for caches
///   keyed on a single member of an exporter type
///   (`MemberPresence + Member`).
/// - `engine_fact_signature_for_exported_type` — for caches keyed
///   on a top-level type identity
///   (`Export + LocalDecl + MemberShape`).
/// - `engine_fact_signature_for_canonical_surface` — for caches
///   whose validity depends on the file's surface fingerprint
///   (`SyntacticExportSet`).
#[test]
fn family_a_producers_call_new_fact_helpers() {
    let registry =
        read_session_source("resolver_core/component_meta_query_engine/registry_decl.rs");
    assert!(
        !registry.contains("engine_dep_signature_for_canonical("),
        "registry_decl.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use engine_fact_signature_for_exported_type instead."
    );
    assert!(
        registry.contains("engine_fact_signature_for_exported_type("),
        "registry_decl.rs must call engine_fact_signature_for_exported_type for its 4 \
         (canonical, name)-keyed cache producers (imported_registry_db, declaration_lookup_db, \
         resolvability_db, owner_collection_db) — these track top-level type identity."
    );

    let prepared_surface =
        read_session_source("resolver_core/component_meta_query_engine/prepared_surface.rs");
    assert!(
        !prepared_surface.contains("engine_dep_signature_for_canonical("),
        "prepared_surface.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use the engine_fact_signature_* helpers instead."
    );
    // PreparedSurface and PreparedTarget observe top-level identity;
    // PreparedMember observes per-member facts.
    assert!(
        prepared_surface.contains("engine_fact_signature_for_exported_type("),
        "prepared_surface.rs must call engine_fact_signature_for_exported_type for the \
         prepared_surface_db and prepared_target_db cache producers (top-level identity)."
    );
    assert!(
        prepared_surface.contains("engine_fact_signature_for_canonical_member("),
        "prepared_surface.rs must call engine_fact_signature_for_canonical_member for the \
         prepared_member_db cache producer (path-precise member observation per R28)."
    );

    let routed_expr =
        read_session_source("resolver_core/component_meta_query_engine/routed_expr.rs");
    assert!(
        !routed_expr.contains("engine_dep_signature_for_canonical("),
        "routed_expr.rs must NOT call engine_dep_signature_for_canonical after the R28 \
         migration — use engine_fact_signature_for_exported_type instead."
    );
    assert!(
        routed_expr.contains("engine_fact_signature_for_exported_type("),
        "routed_expr.rs must call engine_fact_signature_for_exported_type for the \
         routed_expr_surface_db cache producer (top-level identity)."
    );

    let materialize = read_session_source("meta_resolve/materialize/field_types.rs");
    assert!(
        !materialize.contains("engine_dep_signature_for_canonical("),
        "meta_resolve/materialize/field_types.rs must NOT call \
         engine_dep_signature_for_canonical after the R28 migration — use \
         engine_fact_signature_for_canonical_surface for the materialize_memo_db producer."
    );
    assert!(
        materialize.contains("engine_fact_signature_for_canonical_surface("),
        "meta_resolve/materialize/field_types.rs must call \
         engine_fact_signature_for_canonical_surface for the materialize_memo_db producer."
    );
}

/// The legacy `ctx.validate_dep_signature` warm-hit validator is no
/// longer called for Family A entries. The new
/// `validate_fact_signature` covers every site.
#[test]
fn family_a_warm_hit_uses_fact_validation() {
    let src = read_session_source("component_meta_caches.rs");
    // Family A get_or_compute closures must validate the
    // fact_dep_signature, not the legacy dep_signature. The 9
    // get_or_compute methods each ought to call validate_fact_signature
    // at least once (the warm-hit predicate). Count occurrences as a
    // structural gate; the migration commit lands exactly 18
    // (validator + post-publish revalidator per cache × 9 caches) +
    // the per-peek validators on caches that expose `peek()` (5
    // caches: PreparedTarget, MaterializeMemo, PreparedSurface,
    // PreparedMember, RoutedExprSurface). Use a lower bound to keep
    // the gate stable against minor refactors.
    let validate_count = src.matches("validate_fact_signature(ctx,").count();
    assert!(
        validate_count >= 18,
        "expected at least 18 `validate_fact_signature(ctx, ...)` call sites in \
         component_meta_caches.rs (validator + post-publish per Family A cache), \
         got {validate_count}"
    );
    // The bubble-up helper ALSO appears on every warm-hit path so
    // outer tracers see the inner observation set.
    let bubble_count = src.matches("bubble_fact_signature(ctx,").count();
    assert!(
        bubble_count >= 18,
        "expected at least 18 `bubble_fact_signature(ctx, ...)` call sites in \
         component_meta_caches.rs (warm-hit + cold-compute bubble per Family A cache), \
         got {bubble_count}"
    );
}
