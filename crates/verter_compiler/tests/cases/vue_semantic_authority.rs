//! Vue `FrameworkSemanticAuthority` backend: eval-source and template-fact
//! interpretation, catalog identity lookup, and identity-mismatch refusal.

use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::{
    eval_source_from_catalog, registered_semantic_for, take_template_facts_producer_invocations,
    template_facts_from_catalog, TemplateFactsBasis,
};
use verter_compiler::framework_common::{
    svelte_semantic_authority_registration, vue_semantic_authority_registration,
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
fn vue_semantic_template_facts_match_catalog_payload() {
    let source = SIMPLE;
    let artifact = registered_artifact("file:///facts.vue", source);
    let via_authority = VueSemanticAuthority
        .template_facts(source, &artifact)
        .expect("Vue authority must produce template facts")
        .data;
    let via_catalog =
        template_facts_from_catalog(&artifact, source, TemplateFactsBasis::AdmittedArtifact)
            .expect("one semantic-catalog lookup must serve Vue template facts")
            .data;
    assert_eq!(via_authority.components.len(), via_catalog.components.len());
    for (left, right) in via_authority
        .components
        .iter()
        .zip(via_catalog.components.iter())
    {
        assert_eq!(left.tag_name, right.tag_name);
        assert_eq!(left.span, right.span);
        assert_eq!(left.props.len(), right.props.len());
    }
    assert_eq!(
        via_authority.event_handlers.len(),
        via_catalog.event_handlers.len()
    );
    for (left, right) in via_authority
        .event_handlers
        .iter()
        .zip(via_catalog.event_handlers.iter())
    {
        assert_eq!(left.span, right.span);
    }
    assert_eq!(
        via_authority.binding_occurrences.len(),
        via_catalog.binding_occurrences.len()
    );
    assert!(
        !via_authority.components.is_empty(),
        "simple fixture must retain a component usage"
    );
    let reminted = artifact.remint_epoch_for_tests("unknown-epoch");
    assert!(
        template_facts_from_catalog(&reminted, source, TemplateFactsBasis::AdmittedArtifact)
            .is_none(),
        "an unknown epoch must refuse template facts, never empty success"
    );
}

#[test]
fn vue_template_facts_foreign_artifact_is_typed_refusal() {
    let source = "<script>let name = 'world';</script>\n<h1>Hello {name}!</h1>\n";
    let svelte = {
        let language = verter_language::FileLanguage::svelte();
        let source_authority = RegisteredSourceAuthority::new().expect("source authority");
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///foreign.svelte"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                language.clone(),
                Arc::from(source),
            )
            .expect("registered source");
        let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
        let config = CarrierGrammarConfig::Svelte;
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
    };
    assert!(
        VueSemanticAuthority
            .template_facts(source, &svelte)
            .is_none(),
        "a foreign artifact is producer failure, not Some(empty) success"
    );
}

#[test]
fn vue_template_facts_script_only_is_empty_success() {
    let source = "<script setup lang=\"ts\">const n = 1;</script>";
    let artifact = registered_artifact("file:///script-only.vue", source);
    let facts = VueSemanticAuthority
        .template_facts(source, &artifact)
        .expect("a template-free Vue SFC is valid empty success, not refusal")
        .data;
    assert!(
        facts.components.is_empty() && facts.event_handlers.is_empty(),
        "script-only Vue facts must be empty, got {facts:?}"
    );
    let via_catalog =
        template_facts_from_catalog(&artifact, source, TemplateFactsBasis::AdmittedArtifact)
            .expect("catalog must keep template-free Vue as Some(empty)")
            .data;
    assert!(via_catalog.components.is_empty());
}

#[test]
fn selected_template_facts_bind_only_when_bytes_equal_the_admitted_host() {
    let src_source = concat!(
        "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n",
        "<template src=\"./view.html\"></template>",
    );
    let src_artifact = registered_artifact("file:///src.vue", src_source);
    let _ = take_template_facts_producer_invocations();
    assert!(
        template_facts_from_catalog(
            &src_artifact,
            src_source,
            TemplateFactsBasis::SelectedTemplate("<Child :foo=\"1\" />"),
        )
        .is_none(),
        "non-empty selected content for an external template src must refuse, not Some(empty)"
    );
    assert_eq!(take_template_facts_producer_invocations(), 0);

    let source = concat!(
        "<script setup lang=\"ts\">import Original from './Original.vue'</script>\n",
        "<template><Original /></template>",
    );
    let artifact = registered_artifact("file:///selected.vue", source);
    assert!(
        template_facts_from_catalog(
            &artifact,
            source,
            TemplateFactsBasis::SelectedTemplate("<Replacement />"),
        )
        .is_none(),
        "selected bytes that replace the admitted template must refuse"
    );
    assert_eq!(take_template_facts_producer_invocations(), 0);

    let facts = template_facts_from_catalog(
        &artifact,
        source,
        TemplateFactsBasis::SelectedTemplate("<Original />"),
    )
    .expect("byte-identical selected content must keep admitted carrier facts")
    .data;
    assert!(
        facts
            .components
            .iter()
            .any(|component| component.tag_name == "Original"),
        "byte-identical selection must retain the admitted <Original /> usage"
    );
    assert_eq!(take_template_facts_producer_invocations(), 1);
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
fn combined_compiler_template_data_is_unnameable() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/cases/compile-fail/carrier_compiler_template_data_unnameable.rs");
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

#[test]
fn template_facts_carry_expression_parse_diagnostics() {
    // A structurally valid template whose directive expression does not
    // parse. The fact producer is the only pass that parses template
    // expressions on the analysis route, so its diagnostics must ride
    // with the facts — dropping them silently erases the file's
    // template expression errors from the host snapshot.
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <div v-if=\"count ===\">{{ count }}</div>\n",
        "</template>\n",
    );
    let artifact = registered_artifact("file:///malformed-expr.vue", source);
    let facts =
        template_facts_from_catalog(&artifact, source, TemplateFactsBasis::AdmittedArtifact)
            .expect("a structurally valid template must still produce facts");
    assert!(
        facts
            .diagnostics
            .iter()
            .any(|d| d.code == "XInvalidExpression"),
        "the malformed `v-if` expression must surface an XInvalidExpression \
         diagnostic on the template-facts product, got {:?}",
        facts.diagnostics
    );
}
