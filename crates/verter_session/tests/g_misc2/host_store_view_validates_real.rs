//! R26 arch guard: `HostStoreView` per-domain validators return
//! REAL results based on producer state, not `false`-returning
//! placeholders.
//!
//! Three domains:
//! - `validates_parse_domain` — reads from
//!   [`crate::file_artifact_store::FileFacts`] snapshot captured at
//!   view-build time.
//! - `validates_resolve_imports_domain` — composes
//!   [`crate::resolved_import_facts::ResolvedImportFactsKey`] from
//!   the fact + view env + tracked content hash; consults the
//!   captured `ResolvedImportFactsDb` handle.
//! - `validates_route_surface_domain` — discriminates
//!   `ModuleAugmentationIndexShape` (snapshot of augmentation-index
//!   fingerprints) and `EffectiveExportSet` (cached
//!   `augmenter_set_fingerprint` in `RouteDb.effective_export_sets`).
//!
//! The placeholder shape (`false` for resolve-imports; permissive
//! `true` for `EffectiveExportSet`) is forbidden.

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

/// Source-grep arch guard: `validates_resolve_imports_domain`
/// MUST have a real body that composes `ResolvedImportFactsKey`
/// and consults `resolved_import_facts` — the placeholder
/// `_ => false` shape is forbidden.
#[test]
fn validates_resolve_imports_domain_real_body() {
    let src = read_session_source("resolver_store.rs");
    // Hard requirement: the validator must compose a
    // `ResolvedImportFactsKey` from the fact + view env. This is
    // the discriminating signal that distinguishes the real body
    // from a placeholder.
    assert!(
        src.contains("fn validates_resolve_imports_domain("),
        "`HostStoreView::validates_resolve_imports_domain` must be implemented",
    );
    assert!(
        src.contains("ResolvedImportFactsKey {"),
        "`validates_resolve_imports_domain` must compose `ResolvedImportFactsKey` from the fact + view env (R26)",
    );
    assert!(
        src.contains("RESOLVED_IMPORT_FACTS_RESOLVER_VERSION"),
        "`validates_resolve_imports_domain` must pin `resolver_version` in the composed key (R28 substrate bump invariant)",
    );
    assert!(
        src.contains("FactKey::ResolvedImportClause"),
        "`validates_resolve_imports_domain` must discriminate `ResolvedImportClause` keys",
    );
    assert!(
        src.contains("FactKey::ResolvedReexportBinding"),
        "`validates_resolve_imports_domain` must discriminate `ResolvedReexportBinding` keys",
    );
}

/// Source-grep arch guard: `validates_route_surface_domain` for
/// `FactKey::EffectiveExportSet` MUST consult the cached
/// `EffectiveExportSetEntry` fingerprint — the permissive
/// `true` placeholder is forbidden.
#[test]
fn validates_route_surface_effective_export_set_real_body() {
    let src = read_session_source("resolver_store.rs");
    assert!(
        src.contains("fn validates_route_surface_domain("),
        "`HostStoreView::validates_route_surface_domain` must be implemented",
    );
    assert!(
        src.contains("EffectiveExportSetKey {"),
        "`validates_route_surface_domain` must compose `EffectiveExportSetKey` for `EffectiveExportSet` facts (R26)",
    );
    assert!(
        src.contains("lookup_effective_export_set_fingerprint"),
        "`validates_route_surface_domain` must call `RouteDb::lookup_effective_export_set_fingerprint` for the `EffectiveExportSet` arm (view-free permissive read; the view-aware `get_effective_export_set` would recurse)",
    );
    // Negative: the legacy permissive `true` placeholder must NOT
    // remain. The pattern `FactKey::EffectiveExportSet => true,`
    // was the removed placeholder shape.
    assert!(
        !src.contains("FactKey::EffectiveExportSet => true,"),
        "`FactKey::EffectiveExportSet => true,` placeholder is forbidden — the real validator is wired (R26)",
    );
}

/// Source-grep arch guard: the trait's `false`-returning default
/// impls must not be the LAST word for `HostStoreView`. The
/// concrete impl block must override BOTH per-domain methods.
#[test]
fn host_store_view_overrides_all_per_domain_validators() {
    let src = read_session_source("resolver_store.rs");
    let host_impl_start = src
        .find("impl crate::resolver_core::StoreView for HostStoreView {")
        .expect("expected `impl StoreView for HostStoreView` block");
    let host_impl_window = &src[host_impl_start..];
    let host_impl_end = host_impl_window
        .find("\n}")
        .expect("expected closing brace for impl StoreView for HostStoreView");
    let window = &host_impl_window[..host_impl_end];
    assert!(
        window.contains("fn validates_parse_domain("),
        "HostStoreView must override `validates_parse_domain` (R26 parse-domain producer)",
    );
    assert!(
        window.contains("fn validates_resolve_imports_domain("),
        "HostStoreView must override `validates_resolve_imports_domain` (R26 resolve-imports-domain producer)",
    );
    assert!(
        window.contains("fn validates_route_surface_domain("),
        "HostStoreView must override `validates_route_surface_domain` (R26 route-surface-domain producer)",
    );
}
