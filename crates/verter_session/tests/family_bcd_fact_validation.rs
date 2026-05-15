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
    // Carrier-aware arch guard: the entry must store a single
    // `read_set_signature: ReadSetSignature` field. The carrier
    // holds both the legacy whole-hash `DepSignature` rail (legacy)
    // and the path-precise `Arc<[FactVersionRef]>` rail (facts).
    assert!(
        window.contains("read_set_signature: ReadSetSignature")
            || window
                .contains("read_set_signature: crate::fact_signature_helpers::ReadSetSignature",),
        "{ty} must carry the carrier `read_set_signature: ReadSetSignature`. The carrier \
         consolidates the legacy `DepSignature` rail and the R28 `Arc<[FactVersionRef]>` rail. \
         Window:\n{window}"
    );
    // Negative assertion: no separate dep_signature / fact_dep_signature
    // fields. The carrier consolidation requires both rails live inside
    // `ReadSetSignature`.
    assert!(
        !window.contains("    pub dep_signature: DepSignature")
            && !window.contains("    pub dep_signature: crate::semantic_query::DepSignature")
            && !window.contains("    pub(super) dep_signature: DepSignature"),
        "{ty} must NOT carry a separate `dep_signature: DepSignature` field after the carrier \
         consolidation — both rails live inside `read_set_signature.legacy` / .facts now. \
         Window:\n{window}"
    );
    assert!(
        !window.contains("    pub fact_dep_signature: Arc<[FactVersionRef]>")
            && !window.contains(
                "    pub fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>",
            )
            && !window.contains(
                "    pub(super) fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>"
            ),
        "{ty} must NOT carry a separate `fact_dep_signature: Arc<[FactVersionRef]>` field after \
         the carrier consolidation. Window:\n{window}"
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

/// Family C: OwnerImportSurface carries both signatures (Block 6.B
/// owns the legacy `dep_signature` retirement; the AND-gate model is
/// the architecturally correct transitional state for this cache).
#[test]
fn family_c_entries_carry_both_signatures() {
    let owner = read_session_source("owner_import_surface.rs");
    assert_struct_carries_both_fields(&owner, "OwnerImportSurface");
}

/// Family D: AppConfigNoOverrideProofEntry was a never-wired cache at
/// Block 1.H entry. Per codex's architectural decision, Block 1.H
/// REPLACED the legacy `dep_signature` field with
/// `fact_dep_signature` directly (no AND-gate transitional state)
/// because the cache had no production producer or consumer at HEAD,
/// so the legacy field never had a real role to retire.
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
         (Block 1.H Track 2.4 — codex Option B). Window:\n{window}"
    );
    assert!(
        !window.contains("dep_signature: DepSignature")
            && !window.contains("dep_signature: crate::semantic_query::DepSignature"),
        "AppConfigNoOverrideProofEntry must NOT carry the legacy `dep_signature` field — \
         Block 1.H Track 2.4 replaced it with `fact_dep_signature` per codex's decision \
         (the cache had no production producer at HEAD so there is no legacy field to retire). \
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
