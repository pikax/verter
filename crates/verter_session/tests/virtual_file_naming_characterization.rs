//! D-x / D-al characterization: the `VirtualFileNaming` column reproduces the
//! live PRODUCTION virtual-file derivations byte-for-byte.
//!
//! The column is the single naming authority; the LSP `provider_id_for_source`
//! / `provider_ide_id_for_source` derivations (in `verter_workspace`) and the
//! ts-plugin testing-role naming must agree with it. This pin renders the
//! column's paths for a carrier source and byte-compares against the live
//! production derivations — a drift is a real defect (the column and the live
//! derivation disagree). It is DISCRIMINATING: a column edit that diverges from
//! the production formula fails here.

use verter_session::framework::descriptor::{
    svelte_descriptor, vue_descriptor, IdeSuffixPolicy, VirtualFileNaming,
};
use verter_workspace::{IdeProjectConfig, NativeProjectResolver, ProjectMembership};

/// Apply the column's api/ide suffixes to a carrier canonical (append-to-full
/// semantics), returning `(api_path, ide_path_tsx)`.
fn column_paths(naming: &VirtualFileNaming, canonical: &str) -> (Option<String>, Option<String>) {
    let api = naming.api_suffix.map(|s| format!("{canonical}{s}"));
    let ide = naming.ide.as_ref().map(|policy| match policy {
        IdeSuffixPolicy::Fixed(s) => format!("{canonical}{s}"),
        // `is_jsx = false` (a TypeScript carrier) selects the non-JSX suffix.
        IdeSuffixPolicy::JsxConditional { non_jsx, .. } => format!("{canonical}{non_jsx}"),
    });
    (api, ide)
}

fn single_project_resolver() -> NativeProjectResolver {
    let mut project = IdeProjectConfig::new(
        "/workspace".to_string(),
        "/workspace".to_string(),
        Some("/workspace/tsconfig.json".to_string()),
    );
    project.membership = ProjectMembership::MatchAll;
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
    assert_eq!(col_api.as_deref(), Some("/workspace/src/App.vue.ts"));
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
    assert_eq!(col_api.as_deref(), Some("/workspace/src/Comp.svelte.ts"));
    assert_eq!(col_ide.as_deref(), Some("/workspace/src/Comp.svelte.tsx"));
}

#[test]
fn testing_api_suffix_implies_api_suffix_and_svelte_forms_no_testing_name() {
    // D-al structural rule: testing is a MODE of the api producer.
    let vue = vue_descriptor().virtual_file_naming.expect("vue naming");
    assert!(vue.is_structurally_valid());
    assert!(
        vue.testing_api_suffix.is_none() || vue.api_suffix.is_some(),
        "testing_api_suffix.is_some() => api_suffix.is_some()"
    );

    // NEGATIVE: Svelte forms NO `.svelte.__verter_test.ts` name (testing
    // surface is Vue-only).
    let svelte = svelte_descriptor()
        .virtual_file_naming
        .expect("svelte naming");
    assert_eq!(svelte.testing_api_suffix, None);
}
