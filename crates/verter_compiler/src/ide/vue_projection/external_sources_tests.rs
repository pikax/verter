//! Unit tests for the external sources module.

use std::sync::Arc;

use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};

use oxc_allocator::Allocator;
use verter_css_syntax::{ExternalStyleProducer, PreprocessorIdentity};

use super::external_sources::{
    bind_external_blocks, configuration_digest, AuthoredPosition, BoundExternalBlock,
    ExternalBlockKind, ExternalBlockProjectionBinding, ExternalChainRefusal, ExternalLegality,
    ExternalSourceHost, ProcessedSourceChain, SourceChainLink,
};
use crate::code_transform::CodeTransform;
use crate::framework_common::registered_carrier_projection::project_registered_accepted;
use crate::framework_common::FrameworkParseArtifact;
use crate::ide::vue_projection::script_setup::SourceRange;

fn artifact_for(canonical: &str, source: &str) -> Arc<FrameworkParseArtifact> {
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config =
        crate::framework_common::vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
            .expect("config");
    grammar_authority
        .register_carrier_grammar(
            verter_language::FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            verter_language::FileLanguage::vue(),
            Arc::from(source),
        )
        .expect("registered source");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    Arc::new(
        project_registered_accepted(&accepted)
            .expect("fixture parses")
            .into_framework_parse_artifact(),
    )
}

/// Host with a fixed resolution table: (importer, specifier) to the identity of
/// the file it names, plus the identities whose dialect mismatches.
struct FakeHost {
    resolutions: Vec<(&'static str, &'static str, &'static str)>,
    dialect_mismatch: Vec<&'static str>,
}

impl FakeHost {
    fn resolving(resolutions: &[(&'static str, &'static str, &'static str)]) -> Self {
        Self {
            resolutions: resolutions.to_vec(),
            dialect_mismatch: Vec::new(),
        }
    }
}

impl ExternalSourceHost for FakeHost {
    fn bind(&self, importer: &str, kind: ExternalBlockKind, specifier: &str) -> BoundExternalBlock {
        let identity = self
            .resolutions
            .iter()
            .find(|(from, named, _)| *from == importer && *named == specifier)
            .map(|(_, _, identity)| *identity)
            .expect("the fixture resolves every specifier it uses");
        let legality = if self.dialect_mismatch.contains(&identity) {
            ExternalLegality::DialectMismatch
        } else {
            ExternalLegality::Admitted
        };
        BoundExternalBlock {
            kind,
            specifier: String::new(),
            specifier_range: SourceRange { start: 0, end: 0 },
            source_identity: identity.to_string(),
            legality,
            chain: ProcessedSourceChain::default(),
        }
    }
}

fn bind(canonical: &str, source: &str, host: &FakeHost) -> ExternalBlockProjectionBinding {
    let artifact = artifact_for(canonical, source);
    bind_external_blocks(source, &artifact, canonical, host)
}

fn range_text(source: &str, range: SourceRange) -> &str {
    &source[range.start as usize..range.end as usize]
}

#[test]
fn external_legality_refuses_script_setup_src_at_the_src_value() {
    let source =
        "<script setup lang=\"ts\" src=\"./setup.ts\"></script>\n<template><div/></template>";
    let host = FakeHost::resolving(&[("/w/App.vue", "./setup.ts", "/w/setup.ts")]);
    let binding = bind("/w/App.vue", source, &host);

    assert_eq!(binding.blocks.len(), 1);
    let block = &binding.blocks[0];
    assert_eq!(block.kind, ExternalBlockKind::Script);
    assert_eq!(block.legality, ExternalLegality::ScriptSetupSrc);
    assert_eq!(block.specifier, "./setup.ts");
    assert_eq!(range_text(source, block.specifier_range), "./setup.ts");
}

#[test]
fn external_legality_keeps_the_host_judgement_of_a_dialect_mismatch() {
    let source =
        "<script src=\"./logic.js\"></script>\n<script setup lang=\"ts\">let a = 1</script>";
    let host = FakeHost {
        dialect_mismatch: vec!["/w/logic.js"],
        ..FakeHost::resolving(&[("/w/App.vue", "./logic.js", "/w/logic.js")])
    };
    let binding = bind("/w/App.vue", source, &host);

    assert_eq!(binding.blocks.len(), 1);
    let block = &binding.blocks[0];
    assert_eq!(block.legality, ExternalLegality::DialectMismatch);
    assert_eq!(range_text(source, block.specifier_range), "./logic.js");
}

#[test]
fn external_legality_admits_a_plain_script_src() {
    let source = "<script src=\"./logic.ts\"></script>";
    let host = FakeHost::resolving(&[("/w/App.vue", "./logic.ts", "/w/logic.ts")]);
    let binding = bind("/w/App.vue", source, &host);

    assert_eq!(binding.blocks.len(), 1);
    assert_eq!(binding.blocks[0].legality, ExternalLegality::Admitted);
}

#[test]
fn external_share_gives_two_carriers_naming_one_file_one_identity() {
    let host = FakeHost::resolving(&[
        ("/w/a/One.vue", "./shared.ts", "/w/a/shared.ts"),
        ("/w/b/Two.vue", "../a/shared.ts", "/w/a/shared.ts"),
        ("/w/b/Three.vue", "./shared.ts", "/w/b/shared.ts"),
    ]);
    let one = bind(
        "/w/a/One.vue",
        "<script src=\"./shared.ts\"></script>",
        &host,
    );
    let two = bind(
        "/w/b/Two.vue",
        "<script src=\"../a/shared.ts\"></script>",
        &host,
    );
    let other = bind(
        "/w/b/Three.vue",
        "<script src=\"./shared.ts\"></script>",
        &host,
    );

    assert_eq!(one.blocks[0].source_identity, two.blocks[0].source_identity);
    assert_ne!(
        one.blocks[0].source_identity, other.blocks[0].source_identity,
        "a different file is a different identity"
    );
}

#[test]
fn external_sources_read_set_lists_each_file_once_across_block_kinds() {
    let source = "<script src=\"./shared.ts\"></script>\n\
                  <template src=\"./view.html\"></template>\n\
                  <style src=\"./shared.ts\"></style>";
    let host = FakeHost::resolving(&[
        ("/w/App.vue", "./shared.ts", "/w/shared.ts"),
        ("/w/App.vue", "./view.html", "/w/view.html"),
    ]);
    let binding = bind("/w/App.vue", source, &host);

    let kinds: Vec<ExternalBlockKind> = binding.blocks.iter().map(|block| block.kind).collect();
    assert_eq!(
        kinds,
        [
            ExternalBlockKind::Script,
            ExternalBlockKind::Template,
            ExternalBlockKind::Style
        ]
    );
    assert_eq!(
        binding.read_set.source_identities,
        ["/w/shared.ts", "/w/view.html"]
    );
}

#[test]
fn external_sources_ignore_blocks_that_name_no_file() {
    let source = "<script src=\"\"></script>\n\
                  <script setup lang=\"ts\" :src=\"dynamic\">let a = 1</script>\n\
                  <i18n src=\"./messages.json\"></i18n>\n\
                  <style>div{}</style>";
    let binding = bind("/w/App.vue", source, &FakeHost::resolving(&[]));

    assert!(binding.blocks.is_empty());
    assert!(binding.read_set.source_identities.is_empty());
}

#[test]
fn external_sources_take_the_first_of_duplicate_src_attributes_and_keep_the_raw_specifier() {
    let source = "<style src=\"./a&amp;b.css\" src=\"./other.css\"></style>";
    let host = FakeHost::resolving(&[("/w/App.vue", "./a&amp;b.css", "/w/a&amp;b.css")]);
    let binding = bind("/w/App.vue", source, &host);

    assert_eq!(binding.blocks.len(), 1);
    assert_eq!(binding.blocks[0].specifier, "./a&amp;b.css");
    assert_eq!(binding.blocks[0].source_identity, "/w/a&amp;b.css");
    assert_eq!(
        range_text(source, binding.blocks[0].specifier_range),
        "./a&amp;b.css"
    );
}

fn named(identity: &str, version: &str, config: Option<&str>) -> PreprocessorIdentity {
    PreprocessorIdentity::Named(
        ExternalStyleProducer::new(identity, Some(version), config).expect("a named producer"),
    )
}

#[test]
fn preprocess_edit_changes_the_read_set_when_only_the_configuration_changes() {
    let source = "<template src=\"./view.pug\"></template>";
    let host = FakeHost::resolving(&[("/w/App.vue", "./view.pug", "/w/view.pug")]);
    let mut before = bind("/w/App.vue", source, &host).read_set;
    let mut same = bind("/w/App.vue", source, &host).read_set;
    let mut edited = bind("/w/App.vue", source, &host).read_set;

    before.bind_configuration(&[named("pug", "3.0.2", Some("cfg-a"))]);
    same.bind_configuration(&[named("pug", "3.0.2", Some("cfg-a"))]);
    edited.bind_configuration(&[named("pug", "3.0.2", Some("cfg-b"))]);

    assert_eq!(before, same, "the same configuration is the same read set");
    assert_eq!(before.source_identities, edited.source_identities);
    assert_ne!(
        before, edited,
        "a changed configuration fingerprint changes the read set"
    );
}

#[test]
fn configuration_digest_separates_every_part_of_a_preprocessor_configuration() {
    let base = configuration_digest(&[named("pug", "3.0.2", Some("cfg"))]);
    assert!(base.is_some());
    for changed in [
        named("jade", "3.0.2", Some("cfg")),
        named("pug", "3.0.3", Some("cfg")),
        named("pug", "3.0.2", None),
        PreprocessorIdentity::Anonymous,
    ] {
        assert_ne!(base, configuration_digest(&[changed]));
    }
    assert_eq!(configuration_digest(&[]), None, "nothing preprocessed");
    assert_ne!(
        configuration_digest(&[PreprocessorIdentity::Anonymous]),
        configuration_digest(&[
            PreprocessorIdentity::Anonymous,
            PreprocessorIdentity::Anonymous
        ]),
    );
}

fn chain_with_maps(maps: &[Option<&str>]) -> ProcessedSourceChain {
    ProcessedSourceChain {
        links: maps
            .iter()
            .map(|map| SourceChainLink {
                from_space: "authored".to_string(),
                to_space: "processed".to_string(),
                map: map.map(str::to_string),
                has_holes: false,
            })
            .collect(),
    }
}

fn map_json(mappings: &str) -> String {
    format!("{{\"version\":3,\"sources\":[\"view.pug\"],\"names\":[],\"mappings\":\"{mappings}\"}}")
}

fn at(line: u32, column: u32) -> Option<AuthoredPosition> {
    Some(AuthoredPosition { line, column })
}

#[test]
fn source_navigation_follows_the_chain_to_the_authored_external_position() {
    let text = "let a = 1;\nlet b = 2;\n";
    let allocator = Allocator::default();
    let transform = CodeTransform::new(text, &allocator);
    let chain = chain_with_maps(&[Some(&map_json("AAGA;AAKA"))]);

    let authored = chain
        .authored_map(&transform)
        .expect("a retained-only transform chains")
        .expect("the chain carries a map");

    assert_eq!(
        authored.locate(&[(0, 0), (0, 4), (1, 0), (1, 9)]),
        [at(3, 0), at(3, 0), at(8, 0), at(8, 0)]
    );
}

#[test]
fn source_navigation_follows_a_transform_that_removes_bytes_before_the_position() {
    let text = "AB\nlet a = 1;\n";
    let allocator = Allocator::default();
    let mut transform = CodeTransform::new(text, &allocator);
    transform.overwrite(0, 3, "");
    let chain = chain_with_maps(&[Some(&map_json("AAAA;AAGA"))]);

    let authored = chain
        .authored_map(&transform)
        .expect("an overwrite chains")
        .expect("the chain carries a map");

    assert_eq!(
        authored.locate(&[(0, 0)]),
        [at(3, 0)],
        "the first projected line is the authored line the removed prefix preceded"
    );
}

#[test]
fn map_hole_gives_unmapped_spans_no_source_position() {
    let text = "x\ny\nz\n";
    let allocator = Allocator::default();
    let transform = CodeTransform::new(text, &allocator);
    // Line 0 maps, line 1 has no segment at all, line 2 maps; and the first
    // line carries a sourceless segment at column 5.
    let chain = chain_with_maps(&[Some(&map_json("AAGA;;AAKA"))]);
    let authored = chain
        .authored_map(&transform)
        .expect("chains")
        .expect("map");

    assert_eq!(
        authored.locate(&[(0, 0), (1, 0), (2, 0)]),
        [at(3, 0), None, at(8, 0)],
        "the line the map leaves unmapped gets no position"
    );

    let sourceless = chain_with_maps(&[Some(&map_json("AAGA,K"))]);
    let text = "let a = 1;\n";
    let transform = CodeTransform::new(text, &allocator);
    let authored = sourceless
        .authored_map(&transform)
        .expect("chains")
        .expect("map");
    assert_eq!(
        authored.locate(&[(0, 2), (0, 5), (0, 8)]),
        [at(3, 0), None, None],
        "a sourceless segment is a hole, not a position to look through"
    );
}

#[test]
fn a_chain_without_a_map_has_no_authored_map() {
    let allocator = Allocator::default();
    let transform = CodeTransform::new("let a = 1;\n", &allocator);
    let chain = chain_with_maps(&[None, None]);
    assert!(chain
        .authored_map(&transform)
        .expect("no refusal")
        .is_none());
}

#[test]
fn a_chain_that_cannot_be_expressed_is_refused_not_approximated() {
    let allocator = Allocator::default();
    let transform = CodeTransform::new("let a = 1;\n", &allocator);

    let stacked = chain_with_maps(&[Some(&map_json("AAAA")), None, Some(&map_json("AAAA"))]);
    assert_eq!(
        stacked.authored_map(&transform).unwrap_err(),
        ExternalChainRefusal::SeveralMappedLinks { mapped: 2 }
    );

    let malformed = chain_with_maps(&[None, Some("not a source map")]);
    assert_eq!(
        malformed.authored_map(&transform).unwrap_err(),
        ExternalChainRefusal::InvalidMap { link: 1 }
    );

    let mut inserting = CodeTransform::new("let a = 1;\n", &allocator);
    inserting.prepend_left(0, "// ");
    let chain = chain_with_maps(&[Some(&map_json("AAAA"))]);
    assert!(matches!(
        chain.authored_map(&inserting).unwrap_err(),
        ExternalChainRefusal::Transform(_)
    ));
}
