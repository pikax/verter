//! Virtual-file-naming characterization: the `VirtualFileNaming` column
//! reproduces the live PRODUCTION virtual-file derivations byte-for-byte.
//!
//! The column is the single naming authority; the LSP `provider_id_for_source`
//! / `provider_ide_id_for_source` derivations (in `verter_workspace`) and the
//! ts-plugin testing-role naming must agree with it. This pin renders the
//! column's paths for a carrier source and byte-compares against the live
//! production derivations — a drift is a real defect (the column and the live
//! derivation disagree). It is DISCRIMINATING: a column edit that diverges from
//! the production formula fails here.

use verter_session::framework::descriptor::{
    svelte_descriptor, vue_descriptor, VirtualFileNaming, VirtualPathPolicy,
};
use verter_workspace::{IdeProjectConfig, NativeProjectResolver};

/// Apply a `VirtualPathPolicy` to a carrier canonical (append-to-full
/// semantics, `is_jsx = false` — a TypeScript carrier — for the conditional
/// arm). `SelfFile` yields the canonical itself; `None` yields `None`.
fn apply_policy(policy: &VirtualPathPolicy, canonical: &str) -> Option<String> {
    match policy {
        VirtualPathPolicy::None => None,
        VirtualPathPolicy::SelfFile => Some(canonical.to_string()),
        VirtualPathPolicy::Suffix(s) => Some(format!("{canonical}{s}")),
        VirtualPathPolicy::JsxConditional { non_jsx, .. } => Some(format!("{canonical}{non_jsx}")),
    }
}

/// Apply the column's import-surface/ide policies to a carrier canonical,
/// returning `(api_path, ide_path_tsx)`.
fn column_paths(naming: &VirtualFileNaming, canonical: &str) -> (Option<String>, Option<String>) {
    (
        apply_policy(&naming.import_surface, canonical),
        apply_policy(&naming.ide, canonical),
    )
}

fn single_project_resolver() -> NativeProjectResolver {
    let mut project = IdeProjectConfig::new(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    );
    project.membership = verter_workspace::ConfiguredMembership::match_all_under_root(
        &verter_workspace::CanonicalPath::new("/workspace"),
    );
    NativeProjectResolver::new(vec![project])
}

#[test]
fn vue_column_reproduces_production_provider_derivations() {
    let resolver = single_project_resolver();
    let canonical = "/workspace/src/App.vue";

    let naming = vue_descriptor().virtual_file_naming.expect("vue naming");
    let (col_api, col_ide) = column_paths(&naming, canonical);

    // Production derivations (verter_workspace::resolver — the live LSP path).
    let prod_api = resolver.provider_id_for_source(canonical);
    let prod_ide = resolver.provider_ide_id_for_source(canonical, false);

    assert_eq!(
        col_api, prod_api,
        "the Vue column api path must reproduce provider_id_for_source byte-for-byte"
    );
    assert_eq!(
        col_ide, prod_ide,
        "the Vue column ide path (non-jsx) must reproduce provider_ide_id_for_source"
    );
    // The API carrier carries the reserved `.verter.` infix (redirect-reached);
    // the IDE carrier stays the bare-probe-reachable `.tsx`.
    assert_eq!(col_api.as_deref(), Some("/workspace/src/App.vue.verter.ts"));
    assert_eq!(col_ide.as_deref(), Some("/workspace/src/App.vue.tsx"));
}

#[test]
fn svelte_column_reproduces_production_provider_derivations() {
    let resolver = single_project_resolver();
    let canonical = "/workspace/src/Comp.svelte";

    let naming = svelte_descriptor()
        .virtual_file_naming
        .expect("svelte naming");
    let (col_api, col_ide) = column_paths(&naming, canonical);

    let prod_api = resolver.provider_id_for_source(canonical);
    let prod_ide = resolver.provider_ide_id_for_source(canonical, false);

    assert_eq!(col_api, prod_api);
    assert_eq!(col_ide, prod_ide);
    // The API carrier carries the reserved `.verter.` infix (redirect-reached);
    // the IDE carrier stays the bare-probe-reachable `.tsx`.
    assert_eq!(
        col_api.as_deref(),
        Some("/workspace/src/Comp.svelte.verter.ts")
    );
    assert_eq!(col_ide.as_deref(), Some("/workspace/src/Comp.svelte.tsx"));
}

#[test]
fn testing_api_suffix_implies_api_suffix_and_svelte_forms_no_testing_name() {
    // Structural rule: testing is a MODE of the api producer.
    let vue = vue_descriptor().virtual_file_naming.expect("vue naming");
    assert!(vue.is_structurally_valid());
    assert!(
        vue.testing_api_suffix.is_none()
            || matches!(
                vue.import_surface,
                VirtualPathPolicy::Suffix(_) | VirtualPathPolicy::JsxConditional { .. }
            ),
        "testing_api_suffix.is_some() => import_surface is a distinct file"
    );

    // NEGATIVE: Svelte forms NO `.svelte.__verter_test.ts` name (testing
    // surface is Vue-only).
    let svelte = svelte_descriptor()
        .virtual_file_naming
        .expect("svelte naming");
    assert_eq!(svelte.testing_api_suffix, None);
}
