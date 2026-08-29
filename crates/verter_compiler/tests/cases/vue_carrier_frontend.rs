//! Vue `CarrierFrontend` parse backend: equivalence with the existing
//! Vue parser constructor, typed rejection, catalog identity, and proof
//! that production request routes still do not consult this row.

use std::sync::Arc;

use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    vue_carrier_frontend_registration, CarrierCompiler, CarrierFrontend, CatalogCapability,
    CatalogRow, FrameworkEpochId, ImmutableCapabilityCatalog, VueCarrierFrontend,
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

fn parse_via_existing(
    source: &str,
    opts: &ParseOptions,
) -> Arc<UnregisteredFrameworkParseArtifact> {
    VueCarrierCompiler
        .parse(source, opts)
        .expect("existing Vue parse constructor")
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
fn vue_frontend_parse_matches_existing_constructor_on_kitchen_sink() {
    let opts = ParseOptions::vue_standard();
    let frontend = VueCarrierFrontend;
    let via_frontend = frontend.parse(KITCHEN_SINK, &opts).expect("frontend parse");
    let via_compiler = parse_via_existing(KITCHEN_SINK, &opts);
    assert_unregistered_equivalent(via_frontend.as_ref(), via_compiler.as_ref());

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
fn vue_frontend_recovery_diagnostics_match_existing_constructor() {
    let opts = ParseOptions::vue_standard();
    let via_frontend = VueCarrierFrontend
        .parse(RECOVERY, &opts)
        .expect("recoverable malformed source still parses");
    let via_compiler = parse_via_existing(RECOVERY, &opts);
    assert_unregistered_equivalent(via_frontend.as_ref(), via_compiler.as_ref());
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
    assert_eq!(
        row.identity().epoch(),
        &FrameworkEpochId::new(VueCarrierFrontend::EPOCH)
    );
    let _frontend: &VueCarrierFrontend = row.frontend();
    let catalog =
        ImmutableCapabilityCatalog::<VueCarrierFrontend, (), (), (), ()>::try_from_rows([
            CatalogRow::Frontend(row),
        ])
        .expect("single Vue frontend row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn production_request_routes_do_not_call_the_vue_frontend_row() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    walk_production(&src_root, &mut hits);
    assert!(
        hits.is_empty(),
        "production request routes must not consult the Vue frontend catalog row yet: {hits:?}"
    );
}

fn walk_production(dir: &std::path::Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("src walk") {
        let entry = entry.expect("dirent");
        let path = entry.path();
        if path.is_dir() {
            walk_production(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&path);
        let rel_str = rel.to_string_lossy();
        if rel_str.ends_with("framework_common/vue_carrier_frontend.rs")
            || rel_str.ends_with("framework_common/mod.rs")
            || rel_str.ends_with("framework_common/vue_semantic_authority.rs")
            || rel_str.ends_with("framework_common/registered_carrier_projection.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read rust");
        if text.contains("VueCarrierFrontend") || text.contains("vue_carrier_frontend_registration")
        {
            hits.push(rel_str.into_owned());
        }
    }
}
