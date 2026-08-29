//! Vue `FrameworkSemanticAuthority` backend: eval-source and template-fact
//! interpretation, catalog identity lookup, and identity-mismatch refusal.

use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::{
    eval_source_from_catalog, registered_semantic_for,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    svelte_semantic_authority_registration, vue_semantic_authority_registration, CarrierCompiler,
    CarrierCompilerRegistry, CatalogCapability, CatalogRow, FrameworkEpoch, FrameworkParseArtifact,
    FrameworkSemanticAuthority, ImmutableCapabilityCatalog, VueSemanticAuthority, VueSfcV3,
};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::FrameworkAdapterId;
use verter_language::LanguageId;

const KITCHEN_SINK: &str = include_str!("../fixtures/kitchen-sink.vue");

const SIMPLE: &str = concat!(
    "<script setup lang=\"ts\">\n",
    "const count = 1;\n",
    "</script>\n",
    "<template>\n",
    "  <Foo :n=\"count\" @click=\"count\" />\n",
    "</template>\n",
);

fn registered_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::vue();
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config =
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).expect("vue grammar");
    grammar_authority
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    CarrierCompilerRegistry::built_in()
        .project_registered(&accepted)
        .expect("registered projection")
        .into_framework_parse_artifact()
}

#[test]
fn vue_semantic_eval_source_is_position_preserving_on_kitchen_sink() {
    let artifact = registered_artifact("file:///kitchen.vue", KITCHEN_SINK);
    let eval = VueSemanticAuthority.eval_source(KITCHEN_SINK, &artifact);
    assert_eq!(eval.len(), KITCHEN_SINK.len());
    for region in artifact.script_regions() {
        let (s, e) = (region.span.start as usize, region.span.end as usize);
        assert_eq!(&eval[s..e], &KITCHEN_SINK[s..e]);
    }
}

#[test]
fn vue_semantic_eval_source_does_not_inject_newline_between_adjacent_scripts() {
    let source = concat!(
        "<script lang=\"ts\">const a = 1</script>",
        "<script setup lang=\"ts\">const b = a</script>",
        "<template><div /></template>",
    );
    let artifact = registered_artifact("file:///adjacent.vue", source);
    let eval = VueSemanticAuthority.eval_source(source, &artifact);
    assert_eq!(eval.len(), source.len());
    let a_end = eval.find("const a = 1").expect("first script body") + "const a = 1".len();
    assert_eq!(
        eval.as_bytes()[a_end],
        b' ',
        "eval-source blanks the inter-script markup; it does not inject a newline, got: {eval:?}"
    );
    assert!(
        !eval.contains('<'),
        "markup must not leak into eval source: {eval:?}"
    );
}

#[test]
fn vue_semantic_eval_source_is_position_preserving() {
    let source = SIMPLE;
    let artifact = registered_artifact("file:///simple.vue", source);
    let eval = VueSemanticAuthority.eval_source(source, &artifact);
    assert_eq!(eval.len(), source.len());
    for region in artifact.script_regions() {
        let (s, e) = (region.span.start as usize, region.span.end as usize);
        assert_eq!(&eval[s..e], &source[s..e]);
    }
    let markup = source.find("<Foo").expect("component tag");
    assert_eq!(eval.as_bytes()[markup], b' ');
}

#[test]
fn vue_semantic_template_facts_match_existing_compiler() {
    let source = SIMPLE;
    let artifact = registered_artifact("file:///facts.vue", source);
    let via_authority = VueSemanticAuthority.template_facts(source, &artifact);
    let via_compiler = VueCarrierCompiler.template_data(source, &artifact);
    assert_eq!(
        via_authority.data.components.len(),
        via_compiler.data.components.len()
    );
    for (left, right) in via_authority
        .data
        .components
        .iter()
        .zip(via_compiler.data.components.iter())
    {
        assert_eq!(left.tag_name, right.tag_name);
        assert_eq!(left.span, right.span);
        assert_eq!(left.props.len(), right.props.len());
    }
    assert_eq!(
        via_authority.data.event_handlers.len(),
        via_compiler.data.event_handlers.len()
    );
    for (left, right) in via_authority
        .data
        .event_handlers
        .iter()
        .zip(via_compiler.data.event_handlers.iter())
    {
        assert_eq!(left.span, right.span);
    }
    assert_eq!(
        via_authority.data.binding_occurrences.len(),
        via_compiler.data.binding_occurrences.len()
    );
    assert!(
        !via_authority.data.components.is_empty(),
        "simple fixture must retain a component usage"
    );
}

#[test]
fn vue_semantic_catalog_row_binds_vue_adapter_identity() {
    let row = vue_semantic_authority_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::vue());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("vue")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Semantic);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), VueSfcV3::ID);
    let _authority: &VueSemanticAuthority = row.semantic();
    let catalog =
        ImmutableCapabilityCatalog::<(), (), VueSemanticAuthority, (), ()>::try_from_rows([
            CatalogRow::Semantic(row),
        ])
        .expect("single Vue semantic row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn vue_semantic_catalog_lookup_is_adapter_and_epoch_identity() {
    let artifact = registered_artifact("file:///lookup.vue", SIMPLE);
    assert_eq!(artifact.epoch().as_str(), VueSfcV3::ID);
    let selected = registered_semantic_for(artifact.adapter_id(), artifact.epoch())
        .expect("Vue adapter × Vue epoch must select a semantic row");
    let via_catalog = selected.eval_source(SIMPLE, &artifact);
    let via_authority = VueSemanticAuthority.eval_source(SIMPLE, &artifact);
    assert_eq!(
        via_catalog.as_ref(),
        via_authority.as_ref(),
        "lookup must invoke the selected row's eval-source payload directly"
    );
    let via_one_lookup = eval_source_from_catalog(&artifact, SIMPLE)
        .expect("one semantic-catalog lookup must serve the Vue artifact");
    assert_eq!(via_one_lookup.as_ref(), via_authority.as_ref());
}

#[test]
fn combined_compiler_eval_source_is_unnameable() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/carrier_compiler_eval_source_unnameable.rs");
}

#[test]
fn planted_vue_epoch_does_not_select_a_svelte_semantic_authority() {
    let vue_row = vue_semantic_authority_registration();
    assert!(
        registered_semantic_for(&FrameworkAdapterId::svelte(), vue_row.identity().epoch())
            .is_none(),
        "a Vue epoch must not select a Svelte semantic row"
    );
    let svelte_row = svelte_semantic_authority_registration();
    assert!(
        registered_semantic_for(&FrameworkAdapterId::vue(), svelte_row.identity().epoch())
            .is_none(),
        "a Svelte epoch must not select a Vue semantic row"
    );
}
