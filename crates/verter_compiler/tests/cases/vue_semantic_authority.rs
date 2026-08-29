//! Vue `FrameworkSemanticAuthority` backend: eval-source and template-fact
//! equivalence with the existing Vue compiler methods, catalog identity,
//! and proof that production request routes still do not consult this row.

use std::sync::Arc;

use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{
    vue_semantic_authority_registration, CarrierCompiler, CarrierCompilerRegistry,
    CatalogCapability, CatalogRow, FrameworkEpochId, FrameworkParseArtifact,
    FrameworkSemanticAuthority, ImmutableCapabilityCatalog, VueCarrierFrontend,
    VueSemanticAuthority,
};
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

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

fn assert_eval_equivalent(source: &str, left: &str, right: &str) {
    assert_eq!(left.len(), source.len());
    assert_eq!(right.len(), source.len());
    assert_eq!(left, right);
}

#[test]
fn vue_semantic_eval_source_matches_existing_compiler_on_kitchen_sink() {
    let artifact = registered_artifact("file:///kitchen.vue", KITCHEN_SINK);
    let via_authority = VueSemanticAuthority.eval_source(KITCHEN_SINK, &artifact);
    let via_compiler = VueCarrierCompiler.eval_source(KITCHEN_SINK, &artifact);
    assert_eval_equivalent(KITCHEN_SINK, via_authority.as_ref(), via_compiler.as_ref());
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
    assert_eq!(
        row.identity().epoch(),
        &FrameworkEpochId::new(VueCarrierFrontend::EPOCH)
    );
    let _authority: &VueSemanticAuthority = row.semantic();
    let catalog =
        ImmutableCapabilityCatalog::<(), (), VueSemanticAuthority, (), ()>::try_from_rows([
            CatalogRow::Semantic(row),
        ])
        .expect("single Vue semantic row");
    assert_eq!(catalog.len(), 1);
}

#[test]
fn production_request_routes_do_not_call_the_vue_semantic_row() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    walk_production(&src_root, &mut hits);
    assert!(
        hits.is_empty(),
        "production request routes must not consult the Vue semantic catalog row yet: {hits:?}"
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
        if rel_str.ends_with("framework_common/vue_semantic_authority.rs")
            || rel_str.ends_with("framework_common/mod.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read rust");
        if text.contains("VueSemanticAuthority")
            || text.contains("vue_semantic_authority_registration")
        {
            hits.push(rel_str.into_owned());
        }
    }
}
