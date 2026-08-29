//! Svelte `FrameworkSemanticAuthority` backend: eval-source and
//! template-fact equivalence with the existing Svelte compiler methods,
//! catalog identity, and proof that production request routes still do
//! not consult this row.

use std::sync::Arc;

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

fn assert_eval_equivalent(source: &str, left: &str, right: &str) {
    assert_eq!(left.len(), source.len());
    assert_eq!(right.len(), source.len());
    assert_eq!(left, right);
}

#[test]
fn svelte_semantic_eval_source_matches_existing_compiler() {
    let artifact = registered_artifact("file:///kitchen.svelte", KITCHEN_SINK);
    let via_authority = SvelteSemanticAuthority.eval_source(KITCHEN_SINK, &artifact);
    let via_compiler = SvelteCarrierCompiler.eval_source(KITCHEN_SINK, &artifact);
    assert_eval_equivalent(KITCHEN_SINK, via_authority.as_ref(), via_compiler.as_ref());
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
fn production_request_routes_do_not_call_the_svelte_semantic_row() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    walk_production(&src_root, &mut hits);
    assert!(
        hits.is_empty(),
        "production request routes must not consult the Svelte semantic catalog row yet: {hits:?}"
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
        if rel_str.ends_with("svelte/semantic_authority.rs")
            || rel_str.ends_with("svelte/mod.rs")
            || rel_str.ends_with("framework_common/mod.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read rust");
        if text.contains("SvelteSemanticAuthority")
            || text.contains("svelte_semantic_authority_registration")
        {
            hits.push(rel_str.into_owned());
        }
    }
}
