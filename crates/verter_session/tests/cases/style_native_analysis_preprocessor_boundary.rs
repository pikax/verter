//! J1-A13: the Rust-LSP baseline analyzes authored CSS-family syntax
//! natively, without any preprocessor provider, for every one of the five
//! dialects `StyleSyntaxIr` owns (CSS/SCSS/Sass/Less/Stylus) — and fails
//! CLOSED (typed `ProcessedContentRequired`/`External`, never a fabricated
//! or silently-internally-preprocessed result) for a `lang` the shared
//! syntax authority does not natively parse.

use std::sync::Arc;

use verter_session::{
    BlockContentAvailability, BlockContentQuery, CompileProfile, FileLanguage, HostConfig,
    UpsertRequest, VerterHost,
};

/// Positive half of A13: every native dialect gets real, non-trivial
/// structural facts (`CssAnalysis`) directly from `StyleSyntaxIr`, published
/// through the host's ordinary analysis surface, with zero preprocessor
/// *requirement* for the block. CSS issues no request at all. SCSS/Sass/Less/
/// Stylus may offer an optional CSS overlay request, but that overlay is
/// NativeAvailable (not a compile blocker) and analysis facts are present
/// without applying any override.
#[test]
fn native_style_analysis_available_without_preprocessor() {
    let cases: &[(&str, &str)] = &[
        ("css", "<style>.native-css { color: red; }</style>"),
        (
            "scss",
            "<style lang=\"scss\">$c: red;\n.native-scss { color: $c; }</style>",
        ),
        (
            "sass",
            "<style lang=\"sass\">\n.native-sass\n  color: red\n</style>",
        ),
        (
            "less",
            "<style lang=\"less\">@c: red;\n.native-less { color: @c; }</style>",
        ),
        (
            "stylus",
            "<style lang=\"stylus\">\n.native-stylus\n  color red\n</style>",
        ),
    ];

    for (dialect, style_block) in cases {
        let host = VerterHost::new_standalone(HostConfig::default());
        let source = format!("<template><div class=\"{dialect}-root\"/></template>{style_block}",);
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: format!("/workspace/{dialect}.vue"),
                source: Arc::from(source.as_str()),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();

        if *dialect == "css" {
            // A native CSS dialect must never widen the external-preprocessor
            // request surface — authored-syntax analysis works without a
            // preprocessor, and CSS has no optional overlay.
            assert!(
                update.preprocessor_requests.is_empty(),
                "{dialect}: a native CSS dialect must not request external preprocessing, got {:?}",
                update.preprocessor_requests
            );
        } else {
            // Optional overlay requests for the four preprocessed native
            // dialects must not reclassify the block as preprocessing-dependent.
            for request in &update.preprocessor_requests {
                assert_eq!(
                    request.availability,
                    BlockContentAvailability::NativeAvailable,
                    "{dialect}: optional overlay must stay NativeAvailable, not ProcessedContentRequired: {request:?}"
                );
                assert_ne!(
                    request.availability,
                    BlockContentAvailability::ProcessedContentRequired,
                    "{dialect}: native authored syntax must not fail closed as external"
                );
            }
        }

        let analysis = host
            .get_analysis(&update.canonical_id)
            .unwrap_or_else(|| panic!("{dialect}: host analysis"));
        let style = analysis
            .styles
            .first()
            .unwrap_or_else(|| panic!("{dialect}: style analysis"));
        assert_eq!(
            style.content_availability,
            BlockContentAvailability::NativeAvailable,
            "{dialect}: authored syntax must be natively available without a preprocessor"
        );
        let css = style
            .css
            .as_ref()
            .unwrap_or_else(|| panic!("{dialect}: css facts must be present, not deferred"));
        assert!(
            css.classes
                .iter()
                .any(|class| class.name == format!("native-{dialect}")),
            "{dialect}: real class facts must be projected from the authored bytes, got {:?}",
            css.classes
        );
    }
}

/// Negative half of A13: a style block whose `lang` the shared syntax
/// authority does not natively parse (an unrecognized preprocessor alias,
/// distinct from all five native dialect names) fails CLOSED — typed
/// `ProcessedContentRequired` unavailability, `css: None` — rather than
/// silently treating it as CSS or fabricating an empty-but-positive result.
/// There is no Rust-side preprocessing capability to fabricate an answer
/// with in the first place; this guards the latent regression where a
/// future code path treats an absent `BlockOverrideEntry` as if the block
/// were Css-native.
#[test]
fn preprocess_dependent_request_fails_closed_as_external() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/App.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"unresolved-root\"/></template>\
                 <style lang=\"styl\">.unresolved\n  color: red\n</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    // "styl" is not one of the five recognized native dialect strings
    // (css/scss/sass/less/stylus) — the host must issue a preprocessor
    // request rather than silently treat it as native CSS. The Vue parser
    // aliases `styl` onto Stylus for *tag classification*, but analysis_lang
    // and the block-content native set key off the authored string, so this
    // spelling stays preprocessing-dependent.
    assert_eq!(
        update.preprocessor_requests.len(),
        1,
        "an unrecognized style lang must request external preprocessing, got {:?}",
        update.preprocessor_requests
    );
    let request = &update.preprocessor_requests[0];
    assert_eq!(
        request.availability,
        BlockContentAvailability::ProcessedContentRequired
    );

    let analysis = host.get_analysis(&update.canonical_id).unwrap();
    let style = analysis.styles.first().expect("style analysis");
    assert_eq!(
        style.content_availability,
        BlockContentAvailability::ProcessedContentRequired,
        "a preprocessing-dependent block must report typed unavailability"
    );
    assert!(
        style.css.is_none(),
        "preprocessing-dependent facts must fail closed, never be fabricated \
         from unprocessed bytes: {:?}",
        style.css
    );

    // The host-level block-content query (the LSP's real read path) must
    // independently agree on typed unavailability. The raw AUTHORED bytes
    // stay readable as `InlineAuthored` (unprocessed Stylus source, useful
    // on its own) — it is specifically the PROCESSED result and any facts
    // derived from it that fail closed, never a silent internal preprocess
    // standing in for the real one.
    let content = host
        .get_block_content(BlockContentQuery {
            canonical_id: update.canonical_id.clone(),
            block_token: request.block_token.to_string(),
            compile_profile: CompileProfile::default(),
            expected_basis_token: None,
        })
        .unwrap();
    assert_eq!(
        content.availability,
        BlockContentAvailability::ProcessedContentRequired
    );
    assert!(matches!(
        content.origin,
        Some(verter_session::BlockContentOrigin::InlineAuthored)
    ));
    assert_eq!(
        content.content.as_deref(),
        Some(".unresolved\n  color: red\n"),
        "the raw authored bytes must be exactly what was authored, never \
         preprocessed output fabricated in its place"
    );
}
