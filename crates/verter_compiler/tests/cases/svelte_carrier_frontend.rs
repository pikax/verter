//! Svelte `CarrierFrontend` parse backend: equivalence with the
//! catalog-selected parse route, typed rejection, and catalog identity.

use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::parse_registered_frontend;
use verter_compiler::framework_common::{
    CarrierFrontend, CatalogCapability, CatalogTable, FrameworkEpoch,
};
use verter_compiler::svelte::{
    svelte_carrier_frontend_registration, SvelteCarrierFrontend, SvelteSfc5,
};
use verter_language::{
    parse_key_for, syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId,
    ParseOptions, UnregisteredFrameworkParseArtifact, UnsupportedSyntaxProfileReason,
    SVELTE_SYNTAX_COMPATIBILITY_DOMAIN, SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
};

const KITCHEN_SINK: &str = concat!(
    "<script>let name = $state('world'); let count = $state(0);</script>\n",
    "<h1>Hello {name}!</h1>\n",
    "<input bind:value={name} />\n",
    "<button onclick={() => count += 1}>clicks: {count}</button>\n",
);

const RECOVERY: &str = concat!(
    "<script>let c = $state(0);</script>\n",
    "<div><button onclick={() => c++}>{c}</button></div x>\n",
);

fn parse_via_catalog(source: &str, opts: &ParseOptions) -> Arc<UnregisteredFrameworkParseArtifact> {
    parse_registered_frontend(
        &FrameworkAdapterId::svelte(),
        &LanguageId::new("svelte"),
        source,
        opts,
    )
    .expect("catalog-selected Svelte frontend parse")
}

fn assert_unregistered_equivalent(
    left: &UnregisteredFrameworkParseArtifact,
    right: &UnregisteredFrameworkParseArtifact,
) {
    assert_eq!(left.adapter_id, right.adapter_id);
    assert_eq!(left.language_id, right.language_id);
    assert_eq!(left.parse_key.as_ref(), right.parse_key.as_ref());
    assert_eq!(left.syntax_profile.as_ref(), right.syntax_profile.as_ref());
    assert_eq!(left.diagnostics, right.diagnostics);
}

#[test]
fn svelte_frontend_parse_matches_the_catalog_route_on_kitchen_sink() {
    let opts = ParseOptions::default();
    let frontend = SvelteCarrierFrontend;
    let via_frontend = frontend.parse(KITCHEN_SINK, &opts).expect("frontend parse");
    let via_catalog = parse_via_catalog(KITCHEN_SINK, &opts);
    assert_unregistered_equivalent(via_frontend.as_ref(), via_catalog.as_ref());

    let language = FileLanguage::svelte();
    let profile = syntax_profile_id_for(&language, &opts).unwrap();
    let expected_key = parse_key_for(
        KITCHEN_SINK,
        &language,
        SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
        SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
        &profile,
    )
    .unwrap();
    assert_eq!(via_frontend.parse_key.as_ref(), &expected_key);
    assert_eq!(via_frontend.adapter_id, FrameworkAdapterId::svelte());
    assert_eq!(via_frontend.language_id, LanguageId::new("svelte"));
}

#[test]
fn svelte_frontend_recovery_diagnostics_match_the_catalog_route() {
    let opts = ParseOptions::default();
    let via_frontend = SvelteCarrierFrontend
        .parse(RECOVERY, &opts)
        .expect("recoverable malformed source still parses");
    let via_catalog = parse_via_catalog(RECOVERY, &opts);
    assert_unregistered_equivalent(via_frontend.as_ref(), via_catalog.as_ref());
    assert!(
        !via_frontend.diagnostics.is_empty(),
        "recovery fixture must retain mapped parse diagnostics"
    );
}

#[test]
fn svelte_frontend_rejects_loose_mode_before_an_artifact() {
    let opts = ParseOptions {
        delimiters: (String::new(), String::new()),
        custom_elements: Vec::new(),
        svelte_loose: true,
    };
    let err = SvelteCarrierFrontend
        .parse(KITCHEN_SINK, &opts)
        .expect_err("loose mode is unimplemented");
    match err {
        verter_language::SyntaxReject::UnsupportedProfile { reason, .. } => {
            assert_eq!(reason, UnsupportedSyntaxProfileReason::UnsupportedOption);
        }
        other => panic!("expected unsupported profile, got {other:?}"),
    }
}

#[test]
fn svelte_frontend_catalog_row_binds_svelte_adapter_identity() {
    let row = svelte_carrier_frontend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::svelte());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("svelte")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Frontend);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(
        row.identity().framework_family(),
        SvelteSfc5::MANIFEST.family
    );
    let _frontend: &SvelteCarrierFrontend = row.frontend();
    let catalog = CatalogTable::try_from_rows([row]).expect("single Svelte frontend row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn vue_and_svelte_frontend_rows_coexist_as_independent_catalogs() {
    let svelte = svelte_carrier_frontend_registration();
    let vue = verter_compiler::framework_common::vue_carrier_frontend_registration();
    assert_ne!(svelte.identity().adapter_id(), vue.identity().adapter_id());
    let svelte_catalog = CatalogTable::try_from_rows([svelte]).expect("svelte catalog");
    let vue_catalog = CatalogTable::try_from_rows([vue]).expect("vue catalog");
    assert_eq!(svelte_catalog.len(), 1);
    assert_eq!(vue_catalog.len(), 1);
}
