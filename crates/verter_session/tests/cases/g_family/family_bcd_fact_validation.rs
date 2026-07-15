//! R3/R26/R28 arch guard for the Family B/C/D inner caches: each
//! entry carries a single `read_set_signature: ReadSetSignature`
//! whose `facts: Arc<[FactVersionRef]>` rail is the sole
//! cache-validity oracle.
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
//! Cache validity is one oracle — the path-precise fact signature
//! `ReadSetSignature.facts`. No entry carries a separate public
//! `dep_signature: DepSignature` validity rail and no entry carries a
//! separate `fact_dep_signature: Arc<[FactVersionRef]>` field — the
//! carrier consolidates the path-precise signature into
//! `read_set_signature.facts`.

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

/// Assert `ty` carries the carrier `read_set_signature: ReadSetSignature`
/// and NO separate public `dep_signature` / `fact_dep_signature`
/// validity field — the fact carrier is the sole cache-validity rail.
fn assert_struct_carries_fact_carrier(src: &str, ty: &str) {
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
    // The entry must store the carrier `read_set_signature:
    // ReadSetSignature` — its `facts` rail is the sole cache-validity
    // oracle.
    assert!(
        window.contains("read_set_signature: ReadSetSignature")
            || window
                .contains("read_set_signature: crate::fact_signature_helpers::ReadSetSignature",),
        "{ty} must carry the carrier `read_set_signature: ReadSetSignature` — its `facts` rail \
         is the sole cache-validity oracle. Window:\n{window}"
    );
    // Negative assertion: no separate public `dep_signature:
    // DepSignature` validity rail. The legacy bundled rail is retired.
    assert!(
        !window.contains("    pub dep_signature: DepSignature")
            && !window.contains("    pub dep_signature: crate::semantic_query::DepSignature")
            && !window.contains("    pub(super) dep_signature: DepSignature"),
        "{ty} must NOT carry a separate `dep_signature: DepSignature` validity field — the \
         legacy bundled cache-validity rail is retired; `read_set_signature.facts` is the sole \
         oracle. Window:\n{window}"
    );
    // Negative assertion: no separate `fact_dep_signature` field — the
    // path-precise signature lives inside `read_set_signature.facts`.
    assert!(
        !window.contains("fact_dep_signature: Arc<[FactVersionRef]>")
            && !window.contains("fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"),
        "{ty} must NOT carry a separate `fact_dep_signature: Arc<[FactVersionRef]>` field — the \
         path-precise signature lives inside `read_set_signature.facts`. Window:\n{window}"
    );
}

/// Family B: MaterializeStructureEntry + RefCycleEntry + MemoEntry
/// each carry the `read_set_signature` fact carrier as their sole
/// cache-validity rail. Source-grep arch guard.
#[test]
fn family_b_entries_carry_fact_carrier() {
    let cache = read_session_source("component_meta_caches.rs");
    assert_struct_carries_fact_carrier(&cache, "MaterializeStructureEntry");
    assert_struct_carries_fact_carrier(&cache, "RefCycleEntry");

    let memo = read_session_source("semantic_query_memo/family.rs");
    assert_struct_carries_fact_carrier(&memo, "MemoEntry");
}

/// Family C: OwnerImportSurface carries the `read_set_signature` fact
/// carrier as its sole cache-validity rail.
#[test]
fn family_c_entries_carry_fact_carrier() {
    let owner = read_session_source("owner_import_surface.rs");
    assert_struct_carries_fact_carrier(&owner, "OwnerImportSurface");
}

/// Family D: AppConfigNoOverrideProofEntry carries
/// `fact_dep_signature: Arc<[FactVersionRef]>` as its path-precise
/// cache-validity rail and no legacy `dep_signature` field.
#[test]
fn family_d_app_config_proof_entry_uses_fact_signature_only() {
    let app_config = read_session_source("app_config_proof_db.rs");
    let needle = "pub struct AppConfigNoOverrideProofEntry {";
    let idx = app_config
        .find(needle)
        .expect("expected AppConfigNoOverrideProofEntry struct decl");
    let after = &app_config[idx..];
    let end = after
        .find("\n}")
        .expect("expected struct close for AppConfigNoOverrideProofEntry");
    let window = &after[..end];
    assert!(
        window.contains("fact_dep_signature: Arc<[FactVersionRef]>")
            || window.contains("fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"),
        "AppConfigNoOverrideProofEntry must carry `fact_dep_signature: Arc<[FactVersionRef]>` \
         as its path-precise cache-validity rail. Window:\n{window}"
    );
    assert!(
        !window.contains("dep_signature: DepSignature")
            && !window.contains("dep_signature: crate::semantic_query::DepSignature"),
        "AppConfigNoOverrideProofEntry must NOT carry a legacy `dep_signature: DepSignature` \
         field — the path-precise fact signature is the sole cache-validity rail. \
         Window:\n{window}"
    );
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
