//! Characterization test for the `RuntimeCompileOptions` -> `SvelteRuntimeOptions`
//! plumbing gap: `SvelteCarrierCompiler::compile_bundle` used to hardcode
//! `runes` / `dev_codegen` / `namespace` / `fragments` / `preserve_whitespace` /
//! `preserve_comments` / `disclose_version` to `None`/`false` regardless of what
//! the neutral carrier's `RuntimeCompileOptions` carried — so a caller could
//! never reach these already-implemented, already-tested (at the
//! `SvelteRuntimeOptions` level — see `runtime_tests.rs`'s
//! `fixture_runtime_options`) Svelte compile options through any production
//! route. This test exercises the CARRIER-LEVEL channel specifically: it
//! would FAIL against the pre-fix carrier (which ignored `svelte_fragments` /
//! `svelte_preserve_comments` entirely) and PASSES now that the channel
//! threads through.

use oxc_allocator::Allocator;
use std::sync::Arc;
use verter_compiler::framework_common::carrier_compiler::{CarrierCompiler, RuntimeCompileOptions};
use verter_compiler::framework_common::{CarrierCompilerRegistry, FrameworkParseArtifact};
use verter_compiler::svelte::SvelteCarrierCompiler;
use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};

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

fn compile_body(canonical: &str, source: &str, opts: RuntimeCompileOptions) -> String {
    let compiler = SvelteCarrierCompiler;
    let artifact = registered_artifact(canonical, source);
    let alloc = Allocator::default();
    let outcome = compiler
        .compile_bundle(source, &artifact, &opts, &alloc)
        .expect("fixture source must compile");
    outcome
        .into_produced()
        .and_then(|bundle| bundle.main.body_code)
        .expect("a two-root static template must produce a runtime body")
}

/// A two-root template: `fragments: 'tree'` only changes emission when the
/// root hoist actually clones a template — a two-sibling root forces a real
/// `$.from_html` / `$.from_tree` hoist rather than the sole-comment/no-op
/// shortcuts.
const SOURCE: &str = "<div>a</div><div>b</div>";

#[test]
fn svelte_fragments_tree_reaches_codegen_through_the_neutral_carrier() {
    let default_body = compile_body(
        "file:///FragmentsDefault.svelte",
        SOURCE,
        RuntimeCompileOptions::default(),
    );
    assert!(
        !default_body.contains("$.from_tree"),
        "default RuntimeCompileOptions (svelte_fragments: None) must emit the \
         default `$.from_html` skeleton, not `$.from_tree` — got:\n{default_body}"
    );

    let tree_opts = RuntimeCompileOptions {
        svelte_fragments: Some("tree".to_string()),
        ..RuntimeCompileOptions::default()
    };
    let tree_body = compile_body("file:///FragmentsTree.svelte", SOURCE, tree_opts);
    assert!(
        tree_body.contains("$.from_tree"),
        "RuntimeCompileOptions.svelte_fragments = Some(\"tree\") must reach the \
         carrier's SvelteRuntimeOptions.fragments and select the `$.from_tree` \
         root-hoist strategy — got:\n{tree_body}"
    );
    assert_ne!(
        default_body, tree_body,
        "the fragments strategy must observably change emitted output"
    );
}

#[test]
fn svelte_preserve_comments_reaches_codegen_through_the_neutral_carrier() {
    const SOURCE_WITH_COMMENT: &str = "<!-- kept --><div>x</div>";

    let default_out = compile_body(
        "file:///CommentsDefault.svelte",
        SOURCE_WITH_COMMENT,
        RuntimeCompileOptions::default(),
    );
    assert!(
        !default_out.contains("kept"),
        "default (svelte_preserve_comments: None) must drop the HTML comment — got:\n{default_out}"
    );

    let preserved_opts = RuntimeCompileOptions {
        svelte_preserve_comments: Some(true),
        ..RuntimeCompileOptions::default()
    };
    let preserved_out = compile_body(
        "file:///CommentsPreserved.svelte",
        SOURCE_WITH_COMMENT,
        preserved_opts,
    );
    assert!(
        preserved_out.contains("kept"),
        "RuntimeCompileOptions.svelte_preserve_comments = Some(true) must reach the \
         carrier and retain the comment text in the static skeleton — got:\n{preserved_out}"
    );
    assert_ne!(default_out, preserved_out);
}
