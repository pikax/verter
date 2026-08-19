use verter_compiler::framework_common::CarrierCompiler;
use verter_compiler::svelte::runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions};
use verter_compiler::svelte::{parse_svelte, SvelteCarrierCompiler};
use verter_language::{ParseOptions, SyntaxReject};

fn parse_diagnostic_codes(source: &str) -> Vec<&'static str> {
    let compiler = SvelteCarrierCompiler;
    compiler
        .parse(source, &ParseOptions::default())
        .expect("recoverable Svelte syntax must publish its carrier artifact")
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn assert_runtime_official_reject(source: &str, expected_code: &str) {
    let parsed = parse_svelte(source);
    let allocator = oxc_allocator::Allocator::default();
    let result = compile_client(
        source,
        &parsed,
        &SvelteRuntimeOptions::default(),
        &allocator,
        false,
        false,
    );
    let Err(ClientCompileError::OfficialReject(rejection)) = result else {
        panic!("{source:?} must produce no runtime module and an OfficialReject, got {result:?}");
    };
    assert_eq!(rejection.official_code, expected_code, "source: {source:?}");
}

#[test]
fn carrier_parse_publishes_a_real_strict_svelte_defect() {
    // An unterminated comment is a strict Svelte syntax defect official
    // rejects for the CLIENT runtime — but the carrier PARSE/PUBLISH seam
    // stays recoverable (the tokenizer is intentionally infallible, which is
    // correct for the IDE projection): `parse()` still publishes. The strict
    // fact ALSO still surfaces on the carrier's own mapped-diagnostic
    // channel (see `svelte_parse_diagnostics`'s doc), matching official
    // Svelte's own parser, which likewise recovers a usable tree after most
    // syntax errors while still recording the diagnostic — the
    // official-reject-PARITY verdict (does this source get a `Main` at all)
    // is a separate, later concern, decided at compile time.
    let compiler = SvelteCarrierCompiler;
    let artifact = compiler
        .parse("<!--", &ParseOptions::default())
        .expect("an unterminated comment is a recoverable strict-parse defect");
    assert_ne!(artifact.parse_key.digest().as_bytes(), &[0; 32]);
    assert_ne!(artifact.syntax_profile.digest().as_bytes(), &[0; 32]);
    assert!(
        artifact
            .diagnostics
            .iter()
            .any(|d| d.code == "unexpected_eof"),
        "the strict-parse fact must still surface as a mapped diagnostic, got: {:?}",
        artifact.diagnostics
    );
}

#[test]
fn recoverable_svelte_syntax_stays_publishable() {
    assert!(parse_diagnostic_codes("{/if}").contains(&"unexpected-block-close"));
}

#[test]
fn recoverable_close_and_duplicate_attribute_publish_exact_diagnostics() {
    assert_eq!(parse_diagnostic_codes("<div>"), ["element_unclosed"]);
    assert_eq!(
        parse_diagnostic_codes("<div class=\"a\" class=\"b\"></div>"),
        ["attribute_duplicate"]
    );
}

#[test]
fn recoverable_deferred_parse_checks_publish_exact_diagnostics() {
    // A CSS-domain style-body defect (a mismatched quote inside `<style>`)
    // is CSS-authority territory, never the carrier's mapped-diagnostic
    // channel: carrier geometry recognizes a `<style>` block's byte
    // boundaries and stops there, so this publishes NOTHING through the
    // parse-diagnostic channel.
    assert_eq!(
        parse_diagnostic_codes(
            "<div></div>\n\n<style>\n\tdiv {\n\t\tbackground-image: url(\"star.gif');\n\t}\n</style>\n"
        ),
        Vec::<&str>::new()
    );
    assert_eq!(
        parse_diagnostic_codes("<svelte:options customElement={42}/>\n"),
        ["svelte_options_invalid_customelement"]
    );
}

#[test]
fn a_later_strict_defect_does_not_stop_an_earlier_recoverable_defect_from_publishing() {
    // A trailing strict defect (the unterminated `<!--`) does not mask, or
    // get masked by, the earlier recoverable `attribute_duplicate` defect at
    // the carrier PARSE/PUBLISH seam: the carrier publishes once, with BOTH
    // defects mapped (the strict fact surfaces on this channel too — see
    // `svelte_parse_diagnostics`'s doc). The reject-parity ordering (which
    // single defect wins as THE official reject) is `official_reject_gate`'s
    // separate concern at compile time.
    let source = "<div class=\"a\" class=\"b\"><!--";
    let compiler = SvelteCarrierCompiler;
    let artifact = compiler
        .parse(source, &ParseOptions::default())
        .expect("a retained strict defect stays publishable at the carrier seam");
    assert!(artifact
        .diagnostics
        .iter()
        .any(|d| d.code == "attribute_duplicate"));
    assert!(
        artifact
            .diagnostics
            .iter()
            .any(|d| d.code == "unexpected_eof"),
        "the trailing strict-parse defect must also surface, got: {:?}",
        artifact.diagnostics
    );
}

#[test]
fn runtime_keeps_the_full_ordered_official_reject_oracle() {
    assert_runtime_official_reject("<div>", "element_unclosed");
    assert_runtime_official_reject("<div class=\"a\" class=\"b\"></div>", "attribute_duplicate");
    assert_runtime_official_reject("<div class=\"a\" class=\"b\"><!--", "attribute_duplicate");
}

#[test]
fn requesting_the_official_loose_parse_mode_is_typed_and_non_publishing() {
    let compiler = SvelteCarrierCompiler;
    let opts = verter_language::ParseOptions {
        svelte_loose: true,
        ..verter_language::ParseOptions::default()
    };
    let reject = compiler
        .parse("<div>hi</div>", &opts)
        .expect_err("the loose parse profile is unsupported and must reject before parsing");

    match reject {
        SyntaxReject::UnsupportedProfile {
            parse_key,
            syntax_profile,
            reason,
        } => {
            assert_eq!(
                reason,
                verter_language::UnsupportedSyntaxProfileReason::UnsupportedOption
            );
            assert_ne!(parse_key.digest().as_bytes(), &[0; 32]);
            assert_ne!(syntax_profile.digest().as_bytes(), &[0; 32]);
        }
        other => panic!("unexpected rejection: {other:?}"),
    }
}

#[test]
fn requesting_strict_parsing_on_well_formed_svelte_publishes_normally() {
    // Negative control discriminating the rejection above: the same
    // well-formed source with the DEFAULT (strict) profile stays
    // publishable — the reject is about the requested profile, not the
    // source.
    let compiler = SvelteCarrierCompiler;
    compiler
        .parse("<div>hi</div>", &ParseOptions::default())
        .expect("strict parsing of well-formed Svelte source must publish");
}

#[test]
fn loose_and_strict_requests_for_identical_source_have_different_syntax_profiles() {
    let strict = verter_language::syntax_profile_id_for(
        &verter_language::FileLanguage::svelte(),
        &ParseOptions::default(),
    )
    .expect("Svelte is a carrier frontend");
    let loose = verter_language::syntax_profile_id_for(
        &verter_language::FileLanguage::svelte(),
        &verter_language::ParseOptions {
            svelte_loose: true,
            ..verter_language::ParseOptions::default()
        },
    )
    .expect("Svelte is a carrier frontend");
    assert_ne!(
        strict, loose,
        "requested loose-ness is part of the syntax profile identity"
    );
}

#[test]
fn comments_inside_an_open_tag_do_not_create_attributes() {
    let compiler = SvelteCarrierCompiler;
    compiler
        .parse(
            "<div data-one=\"1\" // data-one=\"2\"\n data-two=\"2\"></div>",
            &ParseOptions::default(),
        )
        .expect("comments inside an open tag are trivia, not duplicate attributes");
}
