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
//! warm-read validator routes through
//! [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`]
//! — the strict self-root validator, passing the entry's keyed
//! canonical(s) as the self-root set — and the producer through
//! [`engine_fact_signature_for_canonical_member`] /
//! [`engine_fact_signature_for_canonical_surface`] /
//! [`engine_fact_signature_for_prepared_target`] /
//! [`engine_fact_signature_for_materialize_memo`] on cold compute.
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
    // PreparedSurface observes top-level identity directly;
    // PreparedTarget observes top-level identity for both keyed
    // canonicals via the engine_fact_signature_for_prepared_target
    // helper; PreparedMember observes per-member facts.
    assert!(
        prepared_surface.contains("engine_fact_signature_for_exported_type("),
        "prepared_surface.rs must call engine_fact_signature_for_exported_type for the \
         prepared_surface_db cache producer (top-level identity)."
    );
    assert!(
        prepared_surface.contains("engine_fact_signature_for_prepared_target("),
        "prepared_surface.rs must call engine_fact_signature_for_prepared_target for the \
         prepared_target_db cache producer — it roots BOTH the active scope and the \
         declaring canonical as self-roots."
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
         engine_fact_signature_for_materialize_memo for the materialize_memo_db producer."
    );
    assert!(
        materialize.contains("engine_fact_signature_for_materialize_memo("),
        "meta_resolve/materialize/field_types.rs must call \
         engine_fact_signature_for_materialize_memo for the materialize_memo_db \
         producer — it roots the keyed scope canonical AND merges every canonical \
         observed during materialization as a cross-file dependency fact."
    );
}

/// The legacy `ctx.validate_dep_signature` warm-hit validator is no
/// longer called for Family A entries. Every Family A warm-read and
/// post-compute revalidation site validates the `fact_dep_signature`
/// through `validate_fact_signature_with_self_roots` — the strict
/// self-root validator: the entry's keyed canonical(s) are passed as
/// the self-root set, so the leading self-root `FileWholeHash` is
/// validated strictly (a same-canonical edit, or a keyed canonical
/// untracked by the live store view, rejects the entry) while
/// cross-file dependency facts keep lazy permissiveness.
#[test]
fn family_a_warm_hit_uses_fact_validation() {
    let src = read_session_source("component_meta_caches.rs");
    // Family A warm-read closures must validate the fact_dep_signature
    // strictly via `validate_fact_signature_with_self_roots`, NOT the
    // lazy `validate_fact_signature` (which would route a self-root
    // `FileWholeHash` through the untracked-accept rule). The 9
    // get_or_compute methods each carry a warm-hit predicate AND a
    // post-compute revalidator; the 5 caches exposing `peek()`
    // (PreparedTarget, MaterializeMemo, PreparedSurface,
    // PreparedMember, RoutedExprSurface) carry one more. Use a lower
    // bound to keep the gate stable against minor refactors.
    let strict_count = src
        .matches("validate_fact_signature_with_self_roots(")
        .count();
    assert!(
        strict_count >= 18,
        "expected at least 18 `validate_fact_signature_with_self_roots(...)` call \
         sites in component_meta_caches.rs (strict self-root validator + post-publish \
         revalidator per Family A cache), got {strict_count}"
    );
    // The lazy `validate_fact_signature` must NOT be used for a Family
    // A warm/revalidation site: it would accept a self-root
    // `FileWholeHash` for an untracked keyed canonical and serve a
    // stale entry. Only the strict self-root variant is permitted.
    assert!(
        !src.contains("validate_fact_signature(ctx,"),
        "component_meta_caches.rs must NOT call the lazy `validate_fact_signature(ctx, \
         ...)` for any Family A cache — the lazy validator routes a self-root \
         FileWholeHash through the untracked-accept rule and serves stale entries. \
         Use `validate_fact_signature_with_self_roots` with the entry's keyed \
         canonical(s) as the self-root set."
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
