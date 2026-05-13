//! R3/R26/R28 arch guard for the Family B/C/D inner caches that
//! carry `fact_dep_signature: Arc<[FactVersionRef]>` as a sibling
//! field to their legacy `dep_signature: DepSignature` after Stage
//! 7C.A1b.
//!
//! Family B:
//!   - `MaterializeStructureEntry` (component_meta_materialize)
//!   - `RefCycleEntry` (transitive cycle BFS results)
//!   - `MemoEntry` (semantic_query_memo)
//!
//! Family C/D:
//!   - `OwnerImportSurface` (owner-import bindings)
//!   - `AppConfigNoOverrideProofEntry` (component-meta proof cache)
//!
//! These caches retain the legacy `dep_signature` because the
//! ecosystem of consumers around them
//! (`accumulate_dispatch_dep_signature`, `dep_signature_valid_for_host`,
//! `observe_dep_signature` audit hook) is broader than the inner
//! Family A caches and a clean cutover would extend beyond the
//! Stage 7C.A1b scope. The AND-gate model — both signatures in
//! place — is the architecturally correct transitional state per
//! codex's Q1 analysis (recorded in the corrective plan v2
//! amendments).

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

fn assert_struct_carries_both_fields(src: &str, ty: &str) {
    let needle = format!("pub struct {ty} {{");
    let public_idx = src.find(&needle);
    let needle_priv = format!("pub(super) struct {ty} {{");
    let priv_idx = src.find(&needle_priv);
    let idx = public_idx
        .or(priv_idx)
        .unwrap_or_else(|| panic!("expected `{ty}` struct decl in source"));
    let after = &src[idx..];
    let end = after
        .find("\n}")
        .unwrap_or_else(|| panic!("expected struct close for {ty}"));
    let window = &after[..end];
    assert!(
        window.contains("dep_signature: DepSignature")
            || window.contains("dep_signature: crate::semantic_query::DepSignature"),
        "{ty} must carry the legacy `dep_signature: DepSignature` (AND-gate model). \
         Window:\n{window}"
    );
    assert!(
        window.contains("fact_dep_signature: Arc<[FactVersionRef]>")
            || window.contains("fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"),
        "{ty} must carry the new `fact_dep_signature: Arc<[FactVersionRef]>` \
         (R3/R26/R28 substrate). Window:\n{window}"
    );
}

/// Family B: MaterializeStructureEntry + RefCycleEntry + MemoEntry
/// each carry both `dep_signature` (legacy) and `fact_dep_signature`
/// (R3/R26/R28). Source-grep arch guard.
#[test]
fn family_b_entries_carry_both_signatures() {
    let cache = read_session_source("component_meta_caches.rs");
    assert_struct_carries_both_fields(&cache, "MaterializeStructureEntry");
    assert_struct_carries_both_fields(&cache, "RefCycleEntry");

    let memo = read_session_source("semantic_query_memo/family.rs");
    assert_struct_carries_both_fields(&memo, "MemoEntry");
}

/// Family C/D: OwnerImportSurface + AppConfigNoOverrideProofEntry
/// each carry both signatures.
#[test]
fn family_cd_entries_carry_both_signatures() {
    let owner = read_session_source("owner_import_surface.rs");
    assert_struct_carries_both_fields(&owner, "OwnerImportSurface");

    let app_config = read_session_source("app_config_proof_db.rs");
    assert_struct_carries_both_fields(&app_config, "AppConfigNoOverrideProofEntry");
}

/// The `fact_signature_from_fence` materialiser helper exists and
/// converts a `[(Arc<str>, DepVersion)]` slice into an
/// `Arc<[FactVersionRef]>` for the Family B/C/D entry constructors.
#[test]
fn fact_signature_from_fence_helper_exists() {
    let src = read_session_source("component_meta_materialize.rs");
    assert!(
        src.contains("pub fn fact_signature_from_fence("),
        "component_meta_materialize must expose `fact_signature_from_fence(fence: &[(Arc<str>, \
         DepVersion)]) -> Arc<[FactVersionRef]>` for the Family B/C/D producers."
    );
}
