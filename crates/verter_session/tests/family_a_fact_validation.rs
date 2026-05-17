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
//! [`engine_fact_signature_for_exported_type`] /
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
/// - `engine_fact_signature_for_materialize_memo` — for the
///   `MaterializeMemoDb` producer; provenance-pure, it roots the
///   keyed scope on the observed materialisation-time content hash
///   plus the observed-version `SyntacticExportSet` parse fact.
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

/// The three central fact-signature helpers
/// (`fact_signature_for_exported_type`,
/// `fact_signature_for_canonical_member`,
/// `fact_signature_for_canonical_surface`) are **provenance-pure**:
/// they root the keyed canonical on a caller-supplied observed content
/// hash, never a current-content re-read. A current-content re-read
/// inside a signature builder reopens the publish race — an `upsert`
/// landing between a producer's value-compute and signature-build
/// would root a stale value on post-edit content, which then
/// validates on warm reads instead of missing.
///
/// This guard extracts each helper's function body and asserts it
/// calls NONE of the current-content-reading primitives — any
/// current-content re-read inside a signature builder MUST route
/// through one of these:
/// - `authoritative_current_content_hash` — the current-content
///   whole-hash oracle (the source the deleted `self_root_fact`
///   re-read helper used).
/// - `current_file_facts` — the current-content parse-fact reader
///   (the source the deleted `parse_fact_ref` re-read helper used).
/// - `parse_fact_ref(` — the deleted current-content parse-fact
///   builder (matched with its opening paren so it does not
///   false-match the provenance-pure
///   `parse_fact_ref_for_observed_current_content`).
///
/// Re-introducing any of these inside a signature builder flips this
/// guard RED.
#[test]
fn central_fact_signature_helpers_are_provenance_pure() {
    let src = read_session_source("fact_signature_helpers.rs");
    const HELPERS: &[&str] = &[
        "pub(crate) fn fact_signature_for_exported_type(",
        "pub(crate) fn fact_signature_for_canonical_member(",
        "pub(crate) fn fact_signature_for_canonical_surface(",
    ];
    // Each token, if present in a helper body, reopens the publish
    // race. `parse_fact_ref(` is matched with its opening paren so it
    // does not false-match `parse_fact_ref_for_observed_current_content`.
    // `self_root_fact` is intentionally NOT listed: it is a deleted
    // symbol, and any re-read in its shape MUST consult
    // `authoritative_current_content_hash` — already banned below.
    const FORBIDDEN: &[&str] = &[
        "authoritative_current_content_hash",
        "current_file_facts",
        "parse_fact_ref(",
    ];
    for helper in HELPERS {
        let start = src
            .find(helper)
            .unwrap_or_else(|| panic!("expected `{helper}` in fact_signature_helpers.rs"));
        // The function body ends at the first `\n}` at column 0 after
        // the signature — these helpers have no nested column-0 `}`.
        let after = &src[start..];
        let end = after
            .find("\n}")
            .unwrap_or_else(|| panic!("expected a column-0 function close for `{helper}`"));
        let body = &after[..end];
        for forbidden in FORBIDDEN {
            assert!(
                !body.contains(forbidden),
                "`{helper}` MUST NOT call `{forbidden}` — it is a current-content read \
                 and reopens the publish race the provenance-pure signature builders \
                 close. Root the keyed canonical on the caller-supplied observed hash \
                 and pin parse facts via `parse_fact_ref_for_observed_current_content` \
                 instead. Body:\n{body}"
            );
        }
    }
}
