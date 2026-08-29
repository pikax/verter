//! Svelte `FrameworkSemanticAuthority` backend: eval-source and
//! template-fact interpretation, catalog identity lookup, and
//! identity-mismatch refusal.

use std::sync::Arc;

use verter_compiler::framework_common::registered_carrier_projection::{
    eval_source_from_catalog, registered_semantic_for,
};
use verter_compiler::framework_common::{
    svelte_semantic_authority_registration, vue_semantic_authority_registration, CarrierCompiler,
    CarrierCompilerRegistry, CatalogCapability, CatalogRow, FrameworkEpoch, FrameworkParseArtifact,
    FrameworkSemanticAuthority, ImmutableCapabilityCatalog, SvelteSemanticAuthority,
    VueSemanticAuthority,
};
use verter_compiler::svelte::{SvelteCarrierCompiler, SvelteSfc5};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

const KITCHEN_SINK: &str = concat!(
    "<script>let name = $state('world'); let count = $state(0);</script>\n",
    "<h1>Hello {name}!</h1>\n",
    "<input bind:value={name} />\n",
    "<button onclick={() => count += 1}>clicks: {count}</button>\n",
);

const COMPONENT_FACTS: &str = concat!(
    "<script lang=\"ts\">let value = 0; function handler() {}</script>\n",
    "<Button size=\"sm\" bind:value onclick={handler}>{#snippet icon()}x{/snippet}</Button>",
);

fn registered_artifact(canonical: &str, source: &str) -> FrameworkParseArtifact {
    let language = verter_language::FileLanguage::svelte();
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
}

#[test]
fn svelte_semantic_eval_source_is_position_preserving() {
    let source = KITCHEN_SINK;
    let artifact = registered_artifact("file:///preserve.svelte", source);
    let eval = SvelteSemanticAuthority.eval_source(source, &artifact);
    assert_eq!(eval.len(), source.len());
    for region in artifact.script_regions() {
        let (s, e) = (region.span.start as usize, region.span.end as usize);
        assert_eq!(&eval[s..e], &source[s..e]);
    }
    let markup = source.find("<h1>").expect("markup tag");
    assert_eq!(eval.as_bytes()[markup], b' ');
}

#[test]
fn svelte_semantic_template_facts_match_existing_compiler() {
    let source = COMPONENT_FACTS;
    let artifact = registered_artifact("file:///facts.svelte", source);
    let via_authority = SvelteSemanticAuthority.template_facts(source, &artifact);
    let via_compiler = SvelteCarrierCompiler.template_data(source, &artifact);
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
        assert_eq!(left.bindings.len(), right.bindings.len());
        assert_eq!(left.events.len(), right.events.len());
        assert_eq!(left.slots_used, right.slots_used);
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
    assert_eq!(
        via_authority.data.snippet_definitions.len(),
        via_compiler.data.snippet_definitions.len()
    );
    for (left, right) in via_authority
        .data
        .snippet_definitions
        .iter()
        .zip(via_compiler.data.snippet_definitions.iter())
    {
        assert_eq!(left.name, right.name);
        assert_eq!(left.name_span, right.name_span);
    }
    assert!(
        !via_authority.data.components.is_empty(),
        "fixture must retain a component usage"
    );
}

#[test]
fn svelte_semantic_catalog_row_binds_svelte_adapter_identity() {
    let row = svelte_semantic_authority_registration();
    assert_eq!(row.identity().adapter_id(), &FrameworkAdapterId::svelte());
    assert_eq!(
        row.identity().carrier_language_id(),
        &LanguageId::new("svelte")
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Semantic);
    assert!(row.identity().host_epoch().is_none());
    assert_eq!(row.identity().epoch().as_str(), SvelteSfc5::ID);
    let _authority: &SvelteSemanticAuthority = row.semantic();
    let catalog =
        ImmutableCapabilityCatalog::<(), (), SvelteSemanticAuthority, (), ()>::try_from_rows([
            CatalogRow::Semantic(row),
        ])
        .expect("single Svelte semantic row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn vue_and_svelte_semantic_rows_coexist_as_independent_catalogs() {
    let svelte = svelte_semantic_authority_registration();
    let vue = vue_semantic_authority_registration();
    assert_ne!(svelte.identity().adapter_id(), vue.identity().adapter_id());
    let svelte_catalog =
        ImmutableCapabilityCatalog::<(), (), SvelteSemanticAuthority, (), ()>::try_from_rows([
            CatalogRow::Semantic(svelte),
        ])
        .expect("svelte catalog");
    let vue_catalog =
        ImmutableCapabilityCatalog::<(), (), VueSemanticAuthority, (), ()>::try_from_rows([
            CatalogRow::Semantic(vue),
        ])
        .expect("vue catalog");
    assert_eq!(svelte_catalog.len(), 1);
    assert_eq!(vue_catalog.len(), 1);
}

#[test]
fn svelte_semantic_catalog_lookup_is_adapter_and_epoch_identity() {
    let artifact = registered_artifact("file:///lookup.svelte", KITCHEN_SINK);
    assert_eq!(artifact.epoch().as_str(), SvelteSfc5::ID);
    let selected = registered_semantic_for(artifact.adapter_id(), artifact.epoch())
        .expect("Svelte adapter × Svelte epoch must select a semantic row");
    let via_catalog = selected.eval_source(KITCHEN_SINK, &artifact);
    let via_authority = SvelteSemanticAuthority.eval_source(KITCHEN_SINK, &artifact);
    assert_eq!(
        via_catalog.as_ref(),
        via_authority.as_ref(),
        "lookup must invoke the selected row's eval-source payload directly"
    );
    let via_one_lookup = eval_source_from_catalog(&artifact, KITCHEN_SINK)
        .expect("one semantic-catalog lookup must serve the Svelte artifact");
    assert_eq!(via_one_lookup.as_ref(), via_authority.as_ref());
}
