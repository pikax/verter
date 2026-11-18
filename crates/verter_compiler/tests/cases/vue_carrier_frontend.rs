//! Vue `CarrierFrontend` parse backend: equivalence with the
//! catalog-selected parse route, typed rejection, and catalog identity.

use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::parse_registered_frontend;
use verter_compiler::framework_common::{
    vue_carrier_frontend_registration, CarrierFrontend, CatalogCapability, CatalogTable,
    FrameworkEpoch, VueCarrierFrontend, VueSfcV3,
};
use verter_language::{
    parse_key_for, syntax_profile_id_for, FileLanguage, FrameworkAdapterId, LanguageId,
    ParseOptions, UnregisteredFrameworkParseArtifact, UnsupportedSyntaxProfileReason,
    VUE_SYNTAX_COMPATIBILITY_DOMAIN, VUE_SYNTAX_COMPATIBILITY_EPOCH,
};

const KITCHEN_SINK: &str = include_str!("../fixtures/kitchen-sink.vue");

const RECOVERY: &str = concat!(
    "<template><div>{{ broken</div></template>",
    "<script setup>const x = </script>",
);

fn parse_via_catalog(source: &str, opts: &ParseOptions) -> Arc<UnregisteredFrameworkParseArtifact> {
    parse_registered_frontend(
        &FrameworkAdapterId::vue(),
        &LanguageId::new("vue"),
        source,
        opts,
    )
    .expect("catalog-selected Vue frontend parse")
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
fn vue_frontend_parse_matches_the_catalog_route_on_kitchen_sink() {
    let opts = ParseOptions::vue_standard();
    let frontend = VueCarrierFrontend;
    let via_frontend = frontend.parse(KITCHEN_SINK, &opts).expect("frontend parse");
    let via_catalog = parse_via_catalog(KITCHEN_SINK, &opts);
    assert_unregistered_equivalent(via_frontend.as_ref(), via_catalog.as_ref());

    let language = FileLanguage::vue();
    let profile = syntax_profile_id_for(&language, &opts).unwrap();
    let expected_key = parse_key_for(
        KITCHEN_SINK,
        &language,
        VUE_SYNTAX_COMPATIBILITY_DOMAIN,
        VUE_SYNTAX_COMPATIBILITY_EPOCH,
        &profile,
    )
    .unwrap();
    assert_eq!(via_frontend.parse_key.as_ref(), &expected_key);
    assert_eq!(via_frontend.adapter_id, FrameworkAdapterId::vue());
    assert_eq!(via_frontend.language_id, LanguageId::new("vue"));
}

#[test]
fn vue_frontend_recovery_diagnostics_match_the_catalog_route() {
    let opts = ParseOptions::vue_standard();
    let via_frontend = VueCarrierFrontend
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
fn vue_frontend_rejects_empty_delimiter_pair_before_an_artifact() {
    let opts = ParseOptions {
        delimiters: (String::new(), String::new()),
        custom_elements: Vec::new(),
        svelte_loose: false,
    };
    let err = VueCarrierFrontend
        .parse(KITCHEN_SINK, &opts)
        .expect_err("empty delimiter pair is untokenizable");
    match err {
        verter_language::SyntaxReject::UnsupportedProfile { reason, .. } => {
            assert_eq!(reason, UnsupportedSyntaxProfileReason::UnsupportedOption);
        }
        other => panic!("expected unsupported profile, got {other:?}"),
    }
}

#[test]
fn vue_frontend_catalog_row_binds_vue_adapter_identity() {
    let row = vue_carrier_frontend_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::vue());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("vue")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Frontend);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().framework_family(), VueSfcV3::MANIFEST.family);
    let _frontend: &VueCarrierFrontend = row.frontend();
    let catalog = CatalogTable::try_from_rows([row]).expect("single Vue frontend row");
    assert_eq!(catalog.len(), 1);
}
