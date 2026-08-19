use std::sync::Arc;
use verter_compiler::diagnostics::{
    sort_diagnostics, Diagnostic, SyntaxPluginContext, SyntaxPluginOptions,
};
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::CarrierCompiler;
use verter_compiler::parser::Syntax;
use verter_compiler::svelte::SvelteCarrierCompiler;
use verter_language::{sort_language_diagnostics, LanguageDiagnostic, ParseOptions};

fn vue_diagnostics(source: &str) -> Vec<Diagnostic> {
    let options = SyntaxPluginOptions::default();
    let context = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &options,
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    verter_compiler::tokenizer::byte::tokenize_sfc(source.as_bytes(), |event| {
        syntax.handle(&event, &context);
    });
    syntax.into_parsed_sfc().diagnostics
}

fn vue_signature(diagnostic: &Diagnostic) -> (u32, u32, String, String) {
    (
        diagnostic.span.start,
        diagnostic.span.end,
        format!("{:?}", diagnostic.severity),
        format!("{:?}:{:?}", diagnostic.code, diagnostic.arguments),
    )
}

fn svelte_signature(diagnostic: &LanguageDiagnostic) -> (u32, u32, String, String) {
    (
        diagnostic.span.start,
        diagnostic.span.end,
        format!("{:?}", diagnostic.severity),
        format!("{}:{:?}", diagnostic.code, diagnostic.arguments),
    )
}

fn assert_mapped(source: &str, start: u32, end: u32) {
    assert!(start <= end, "diagnostic range is inverted: {start}..{end}");
    assert!(
        end as usize <= source.len(),
        "diagnostic {start}..{end} exceeds source {source:?} ({} bytes)",
        source.len()
    );
    assert!(source.is_char_boundary(start as usize));
    assert!(source.is_char_boundary(end as usize));
    assert_ne!(
        (start, end),
        (0, 0),
        "this non-empty malformed corpus has no authored empty range at byte zero"
    );
}

#[test]
fn malformed_vue_diagnostics_are_repeatable_mapped_and_permutation_stable() {
    const CORPUS: &[&str] = &[
        "<template><div></span><p v-if></template>",
        "<template><div foo=\"a\" foo=\"b\"></div></template>",
        "<script lang=\"js\"></script><script setup lang=\"ts\"></script>",
    ];

    for source in CORPUS {
        let first = vue_diagnostics(source);
        let second = vue_diagnostics(source);
        assert!(
            !first.is_empty(),
            "fixture must exercise recovery: {source}"
        );
        assert_eq!(
            first.iter().map(vue_signature).collect::<Vec<_>>(),
            second.iter().map(vue_signature).collect::<Vec<_>>()
        );
        for diagnostic in &first {
            assert_mapped(source, diagnostic.span.start, diagnostic.span.end);
        }

        let mut permuted = first.clone();
        permuted.reverse();
        sort_diagnostics(&mut permuted);
        assert_eq!(
            first.iter().map(vue_signature).collect::<Vec<_>>(),
            permuted.iter().map(vue_signature).collect::<Vec<_>>()
        );
    }
}

#[test]
fn registered_vue_parse_boundary_is_repeatable_and_source_bound() {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
        FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };

    let source = "<template><div></span><p v-if></template>";
    let source_authority = RegisteredSourceAuthority::new().unwrap();
    let grammar_authority = CarrierGrammarAuthority::new().unwrap();
    let language = verter_language::FileLanguage::vue();
    let grammar = CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap();
    grammar_authority
        .register_carrier_grammar(
            language.clone(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            grammar.clone(),
        )
        .unwrap();
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new("file:///Malformed.vue"),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language,
            Arc::from(source),
        )
        .unwrap();
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &grammar)
        .unwrap();
    let compiler = VueCarrierCompiler;
    // Match the registered path's real standard `{{`/`}}` grammar —
    // `ParseOptions::default()` no longer means "Vue's standard
    // delimiters", it means "the caller supplied nothing."
    let unregistered = compiler
        .parse(source, &ParseOptions::vue_standard())
        .unwrap();
    let registered = verter_compiler::framework_common::CarrierCompilerRegistry::built_in()
        .project_registered(&accepted)
        .unwrap()
        .into_framework_parse_artifact();

    assert_eq!(registered.parse_key(), unregistered.parse_key.as_ref());
    assert_eq!(registered.inventory().source_spaces().len(), 1);
    assert_eq!(
        registered.inventory().source_spaces()[0].bytes().as_ref(),
        source
    );
}

#[test]
fn recoverable_svelte_diagnostics_are_repeatable_mapped_and_permutation_stable() {
    const CORPUS: &[&str] = &["{/if}", "{:else}"];
    let compiler = SvelteCarrierCompiler;

    for source in CORPUS {
        let first = compiler.parse(source, &ParseOptions::default()).unwrap();
        let second = compiler.parse(source, &ParseOptions::default()).unwrap();
        assert!(
            !first.diagnostics.is_empty(),
            "fixture must exercise recovery: {source}"
        );
        assert_eq!(
            first
                .diagnostics
                .iter()
                .map(svelte_signature)
                .collect::<Vec<_>>(),
            second
                .diagnostics
                .iter()
                .map(svelte_signature)
                .collect::<Vec<_>>()
        );
        for diagnostic in &first.diagnostics {
            assert_mapped(source, diagnostic.span.start, diagnostic.span.end);
        }

        let mut permuted = first.diagnostics.clone();
        permuted.reverse();
        sort_language_diagnostics(&first.parse_key, &mut permuted);
        assert_eq!(
            first
                .diagnostics
                .iter()
                .map(svelte_signature)
                .collect::<Vec<_>>(),
            permuted.iter().map(svelte_signature).collect::<Vec<_>>()
        );
    }
}
