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
                 <style lang=\"postcss\">.unresolved\n  color: red\n</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();

    // `postcss` is a real dialect the carrier names, and it has no native
    // grammar in the shared syntax authority — the block's facts genuinely
    // depend on a tool nothing here runs. It is deliberately NOT an alias of a
    // native dialect: `styl` reads that way and IS Stylus everywhere the
    // spelling owner is asked, so using it here would pin a drift rather than
    // this boundary.
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

    // A case variant of a native name is NOT that dialect. Every preprocessor
    // table the ecosystem hands these blocks to is keyed by exact bytes, so
    // `lang="SCSS"` has nothing that can compile it, and the pipeline that
    // compiles the block fails closed on it. Case-folding on this route alone
    // made the same block report a complete, natively-parsed surface here
    // while nothing downstream could build it.
    let folded = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Folded.vue".to_string(),
            source: Arc::from(
                "<template><div class=\"folded-root\"/></template>\
                 <style lang=\"SCSS\">$c: red;\n.folded { color: $c; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let folded_analysis = host.get_analysis(&folded.canonical_id).unwrap();
    let folded_style = folded_analysis.styles.first().expect("style analysis");
    assert_eq!(
        folded_style.content_availability,
        BlockContentAvailability::ProcessedContentRequired,
        "a case variant of a native name has no preprocessor entry, so it must fail closed"
    );
    assert!(
        folded_style.css.is_none(),
        "and must publish no natively-parsed facts: {:?}",
        folded_style.css
    );

    // Both spellings of the same dialect are the same answer. `styl` is a real
    // key in every preprocessor table and Stylus to the carrier parse and the
    // rewrite pipeline alike; classifying it as needing an external tool on
    // this route alone is the drift a private spelling list reintroduces.
    for (name, lang) in [("Stylus", "stylus"), ("Styl", "styl")] {
        let stylus = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: format!("/workspace/{name}.vue"),
                source: Arc::from(
                    format!(
                        "<template><div class=\"stylus-root\"/></template>\
                         <style lang=\"{lang}\">\n.native-stylus\n  color red\n</style>"
                    )
                    .as_str(),
                ),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let stylus_analysis = host.get_analysis(&stylus.canonical_id).unwrap();
        let stylus_style = stylus_analysis.styles.first().expect("stylus style");
        assert_eq!(
            stylus_style.content_availability,
            BlockContentAvailability::NativeAvailable,
            "lang={lang} names a dialect with a native grammar, so it must be natively available"
        );
        assert!(
            stylus_style.css.is_some(),
            "lang={lang} must publish native css facts"
        );
    }

    // The host-level block-content query (the LSP's real read path) must
    // independently agree on typed unavailability. The raw AUTHORED bytes
    // stay readable as `InlineAuthored` (unprocessed authored source, useful
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

/// Byte-exact `lang` reporting belongs to the style axis alone.
///
/// The style pipeline's tables are keyed by exact bytes, so `lang="SCSS"` has
/// to fail closed. A script or template block's dialect classifier resolves an
/// unrecognised spelling to a language rather than refusing it, so
/// `<script lang="TS">` is compiled, type-checked and served as TypeScript by
/// every other route. Applying the style rule to those roles makes THIS route
/// the only one that calls the block non-native: it demands preprocessed
/// content for a script no preprocessor claims, and the block's content — and
/// every IDE surface derived from it — fails closed on a file that builds
/// everywhere else.
#[test]
fn only_the_style_axis_reports_its_lang_byte_exactly() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/Cased.vue".to_string(),
            source: Arc::from(
                "<template lang=\"HTML\"><div class=\"cased-root\"/></template>\
                 <script lang=\"TS\">const tone: string = 'red';\nexport default {};</script>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    assert!(
        update.preprocessor_requests.is_empty(),
        "a case variant of a native script/template lang is still that language \
         everywhere downstream, so this route must not demand a preprocessor for \
         it: {:?}",
        update.preprocessor_requests
    );

    // The same SFC with a case-variant STYLE lang still fails closed, so the
    // containment above did not put the widening step back.
    let styled = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/workspace/CasedStyle.vue".to_string(),
            source: Arc::from(
                "<template lang=\"HTML\"><div class=\"cased-root\"/></template>\
                 <script lang=\"TS\">const tone: string = 'red';\nexport default {};</script>\
                 <style lang=\"SCSS\">$c: red;\n.cased { color: $c; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .unwrap();
    let requested: Vec<_> = styled
        .preprocessor_requests
        .iter()
        .map(|request| (request.lang.clone(), request.availability))
        .collect();
    assert_eq!(
        requested,
        vec![(
            "SCSS".to_string(),
            BlockContentAvailability::ProcessedContentRequired
        )],
        "exactly the style block fails closed, and it does so under the spelling \
         the author wrote"
    );
}
