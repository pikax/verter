use verter_compiler::css::{process_style, types::ProcessStyleOptions};
/**
 * @ai-generated - Exercises the typed two-stage framework style planner boundary.
 */
use verter_compiler::style_planner::{
    transform_vue_css_modules, transform_vue_scoped_css, transform_vue_v_bind, AuthoredStyleInput,
    PlainCssInput, StyleRewriteFailureClass, StyleRewriteOutcome,
};
use verter_compiler::{
    compile::{CodegenOptions, CompileTarget, VerterCompileOptions, VueMacroSemanticInput},
    standalone::{StandaloneCompiler, StandaloneSourceBytes},
};
use verter_css_syntax::CssDialect;

use oxc_allocator::Allocator;
use verter_compiler::svelte::{
    parse_svelte,
    runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions},
};

fn rewritten(outcome: StyleRewriteOutcome) -> (String, String) {
    match outcome {
        StyleRewriteOutcome::Rewritten {
            code, source_map, ..
        } => (code, source_map),
        StyleRewriteOutcome::Unchanged { .. } => panic!("expected a rewrite"),
    }
}

fn scoped(source: &str, scope_id: &str) -> String {
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    rewritten(transform_vue_scoped_css(input, scope_id).expect("trusted CSS scopes")).0
}

fn legacy_scoped(source: &str, scope_id: &str) -> String {
    process_style(
        source,
        &ProcessStyleOptions {
            scope_id,
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        },
    )
    .expect("pre-change engine accepts probe")
    .code
    .into_owned()
}

fn selector_head(source: &str) -> String {
    source
        .split_once('{')
        .expect("probe contains a rule")
        .0
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compile_style(source: &str) -> verter_compiler::compile::VerterCompileResult {
    let source = StandaloneSourceBytes::copied_from(source);
    StandaloneCompiler.compile_source(
        &source,
        &CodegenOptions {
            target: CompileTarget::STYLE,
            component_id: Some("sc1".to_string()),
            ..CodegenOptions::default()
        },
        &VerterCompileOptions::default(),
        &VueMacroSemanticInput::Unavailable,
    )
}

// @ai-generated - SP-01 preserves the surrounding complex selector exactly as the old engine did.
#[test]
fn vue_global_rewrite_preserves_surrounding_selector() {
    for source in [
        ":global(.a) .b { color: red }",
        ":global(.a) > .b { color: red }",
        ".a :global(.b) { color: red }",
    ] {
        let actual = scoped(source, "sc1");
        let oracle = legacy_scoped(source, "sc1");
        assert_eq!(selector_head(&actual), selector_head(&oracle), "{source}");
        assert!(!actual.contains(":global("), "{actual}");
    }
}

// @ai-generated - SP-02 rewrites every special pseudo, including nested selector-list pseudos.
#[test]
fn vue_scoping_rewrites_every_special_pseudo_hop() {
    for source in [
        ".a :deep(.b) :deep(.c) { color: red }",
        ":deep(.b) :slotted(.c) { color: red }",
        ".x :deep(.a) .y :deep(.b) { color: red }",
        ":is(:deep(.b)) { color: red }",
    ] {
        let actual = scoped(source, "sc1");
        let oracle = legacy_scoped(source, "sc1").replace("[__v_slotted__]", "[data-v-sc1-s]");
        assert_eq!(selector_head(&actual), selector_head(&oracle), "{source}");
        assert!(!actual.contains(":deep("), "{actual}");
        assert!(!actual.contains(":slotted("), "{actual}");
        assert!(!actual.contains(":global("), "{actual}");
    }
}

// @ai-generated - SP-03 refuses only the unsafe rule and scopes its trusted sibling.
#[test]
fn vue_stage_two_refusal_is_fail_closed_per_rule() {
    let result =
        compile_style("<style scoped>.good { color: red } .a { &:hover { color: blue } }</style>");
    assert!(
        result
            .errors
            .iter()
            .any(|diagnostic| diagnostic.message.contains("UntrustedRewriteTarget")),
        "missing typed refusal: {:?}",
        result.errors
    );
    let code = &result.styles[0].code;
    assert!(code.contains(".good[data-v-sc100000]"), "{code}");
    assert!(!code.contains(".a"), "unsafe rule shipped: {code}");
    assert!(!code.contains("&:hover"), "unsafe rule shipped: {code}");
}

// @ai-generated - SP-04 retains the old all-language v-bind behavior for CSS-like unknown langs.
#[test]
fn vue_unknown_css_like_lang_still_rewrites_v_bind() {
    let result = compile_style("<style lang=\"postcss\">.a { color: v-bind(t) }</style>");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.styles[0].code, ".a { color: var(--sc100000-t) }");
}

// @ai-generated - SP-05 keeps standalone CSS Modules output aligned with $style lookups.
#[test]
fn vue_style_module_hashes_emitted_classes() {
    let direct = PlainCssInput::new_css(
        ".active { color: red }",
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    let (direct_code, _) =
        rewritten(transform_vue_css_modules(direct, "sc1").expect("trusted CSS Modules rewrite"));
    assert_eq!(direct_code, ".active_d7ff28b8 { color: red }");

    let result = compile_style("<style module>.active { color: red }</style>");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.styles[0].code, ".active_20fc662e { color: red }");
}

// @ai-generated - SP-06 refuses dialect interpolation inside the v-bind argument.
#[test]
fn vue_v_bind_refuses_interpolated_arguments_without_an_artifact() {
    for (dialect, source) in [
        (CssDialect::Scss, ".a { color: v-bind(#{$x}); }"),
        (CssDialect::Less, ".a { color: v-bind(@{x}); }"),
    ] {
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "probe.style",
            "space:probe",
            "artifact:probe",
        );
        let failure = transform_vue_v_bind(input, "sc1")
            .expect_err("interpolated v-bind must be typed-refused");
        assert_eq!(
            failure.class,
            StyleRewriteFailureClass::UntrustedRewriteTarget
        );
    }
}

// @ai-generated - SP-07 applies the rule-level completeness gate before visiting values.
#[test]
fn vue_v_bind_refuses_recovered_rule_bodies() {
    for source in [
        ".a { color: v-bind(tone)",
        "@media x { .a { color: v-bind(tone); }",
        "} .a { color: v-bind(tone); }",
    ] {
        let input = AuthoredStyleInput::new(
            source,
            CssDialect::Css,
            "probe.css",
            "space:probe",
            "artifact:probe",
        );
        let failure =
            transform_vue_v_bind(input, "sc1").expect_err("recovered rule must be typed-refused");
        assert_eq!(
            failure.class,
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            "{source}"
        );
    }
}

// @ai-generated - SP-08 applies the indented-layout guard inside nested functions too.
#[test]
fn vue_sass_nested_v_bind_refuses_newline_collapse() {
    let source = ".a\n  color: rgba(v-bind(\n    tone\n  ), 1)\n";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Sass,
        "probe.sass",
        "space:probe",
        "artifact:probe",
    );
    let failure = transform_vue_v_bind(input, "sc1")
        .expect_err("nested multiline v-bind must preserve indented layout by refusing");
    assert_eq!(
        failure.class,
        StyleRewriteFailureClass::IndentedLayoutMutation
    );
}

// @ai-generated - SP-11 accepts idiomatic colon-less Stylus declarations.
#[test]
fn vue_v_bind_rewrites_colonless_stylus() {
    let source = ".a\n  color v-bind(tone)\n  background v-bind(bg)\n";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Stylus,
        "probe.styl",
        "space:probe",
        "artifact:probe",
    );
    let (code, _) = rewritten(transform_vue_v_bind(input, "sc1").expect("valid Stylus rewrites"));
    assert_eq!(
        code,
        ".a\n  color var(--sc1-tone)\n  background var(--sc1-bg)\n"
    );
}

// @ai-generated - SP-14 renames quoted animation names alongside keyframe identifiers.
#[test]
fn vue_scoping_renames_quoted_animation_names() {
    let code = scoped(
        "@keyframes f { from { opacity: 0 } } .a { animation-name: \"f\" }",
        "sc1",
    );
    assert!(code.contains("@keyframes f-sc1"), "{code}");
    assert!(code.contains("animation-name: \"f-sc1\""), "{code}");
}

// @ai-generated - SP-15 removes the deprecated deep combinator instead of publishing invalid CSS.
#[test]
fn vue_scoping_rewrites_deprecated_deep_combinator() {
    let code = scoped(".a >>> .b { color: red }", "sc1");
    assert!(!code.contains(">>>"), "{code}");
    assert_eq!(selector_head(&code), ".a[data-v-sc1] .b");
}

// @ai-generated - Every authored dialect uses the shared syntax IR and preserves its dialect.
#[test]
fn vue_v_bind_rewrites_all_five_authored_dialects() {
    let cases = [
        (CssDialect::Css, ".css { color: v-bind(tone); }", ".css"),
        (
            CssDialect::Scss,
            "$tone: red; .scss { color: v-bind(tone); }",
            "$tone: red;",
        ),
        (
            CssDialect::Sass,
            ".sass\n  color: v-bind(tone)\n",
            ".sass\n  color:",
        ),
        (
            CssDialect::Less,
            "@tone: red; .less { color: v-bind(tone); }",
            "@tone: red;",
        ),
        (
            CssDialect::Stylus,
            ".stylus\n  color: v-bind(tone)\n",
            ".stylus\n  color:",
        ),
    ];

    for (dialect, source, untouched) in cases {
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "external/theme.style",
            "space:external-theme",
            "artifact:external-theme",
        );
        let outcome = transform_vue_v_bind(input, "scope123").expect("trusted input rewrites");
        let (code, map) = rewritten(outcome);
        assert!(code.contains(untouched), "{dialect:?}: {code}");
        assert!(code.contains("var(--scope123-tone)"), "{dialect:?}: {code}");
        assert!(!code.contains("v-bind("), "{dialect:?}: {code}");
        assert_eq!(code.matches('\n').count(), source.matches('\n').count());

        let decoded: serde_json::Value = serde_json::from_str(&map).expect("valid stage map");
        assert_eq!(decoded["sources"][0], "external/theme.style");
    }
}

// @ai-generated - Stage two is structurally constructible only for plain CSS.
#[test]
fn stage_two_refuses_authored_sass_before_scoping() {
    let failure = PlainCssInput::try_new(
        ".x\n  color: red\n",
        CssDialect::Sass,
        "external/theme.sass",
        "space:theme-sass",
        "artifact:theme-sass",
    )
    .expect_err("authored Sass must never enter post-preprocess scoping");

    assert_eq!(
        failure.class,
        StyleRewriteFailureClass::StageRequiresPlainCss
    );
    assert_eq!(failure.dialect, CssDialect::Sass);
}

// @ai-generated - Recovered/dynamic targets fail closed instead of publishing partial edits.
#[test]
fn vue_planners_refuse_untrusted_targets_without_an_artifact() {
    let authored = AuthoredStyleInput::new(
        ".bad { color: v-bind(tone; }",
        CssDialect::Css,
        "broken.css",
        "space:broken",
        "artifact:broken",
    );
    let failure = transform_vue_v_bind(authored, "scope123")
        .expect_err("an incomplete v-bind target must fail closed");
    assert_eq!(
        failure.class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );

    let css = PlainCssInput::try_new(
        ".bad:where(.x { color: red; }",
        CssDialect::Css,
        "broken.css",
        "space:broken",
        "artifact:broken",
    )
    .unwrap();
    let failure = transform_vue_scoped_css(css, "scope123")
        .expect_err("a recovered selector must not receive a guessed scope span");
    assert_eq!(
        failure.class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );
}

// @ai-generated - Vue stage two scopes selectors and keyframes from trusted CSS IR.
#[test]
fn vue_stage_two_scopes_selectors_pseudos_and_keyframes() {
    let source = ".box:hover { animation: fade 1s; animation-name: fade; } \
                  :deep(.inner) { color: red; } \
                  :slotted(.slot) { color: blue; } \
                  :global(.reset) { margin: 0; } \
                  @keyframes fade { from { opacity: 0; } to { opacity: 1; } }";
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "postprocessed.css",
        "space:postprocessed",
        "artifact:postprocessed",
    )
    .unwrap();
    let outcome = transform_vue_scoped_css(input, "scope123").expect("trusted CSS scopes");
    let (code, map) = rewritten(outcome);

    assert!(code.contains(".box[data-v-scope123]:hover"), "{code}");
    assert!(code.contains("[data-v-scope123] .inner"), "{code}");
    assert!(code.contains(".slot[data-v-scope123-s]"), "{code}");
    assert!(code.contains(".reset"), "{code}");
    assert!(!code.contains(":deep("), "{code}");
    assert!(!code.contains(":slotted("), "{code}");
    assert!(!code.contains(":global("), "{code}");
    assert!(code.contains("@keyframes fade-scope123"), "{code}");
    assert!(code.contains("animation: fade-scope123 1s"), "{code}");
    assert!(code.contains("animation-name: fade-scope123"), "{code}");

    let decoded: serde_json::Value = serde_json::from_str(&map).expect("valid stage map");
    assert_eq!(decoded["sources"][0], "postprocessed.css");
}

// @ai-generated - Rewritten descriptors declare the exact external source space.
#[test]
fn external_rewrite_descriptor_stays_in_external_source_space() {
    let input = AuthoredStyleInput::new(
        ".x { color: v-bind(tone); }",
        CssDialect::Css,
        "D:/project/theme.css",
        "space:external-file",
        "artifact:external-file",
    );
    let outcome = transform_vue_v_bind(input, "scope123").unwrap();
    let StyleRewriteOutcome::Rewritten {
        output_descriptor, ..
    } = outcome
    else {
        panic!("expected rewritten external CSS");
    };

    assert_eq!(
        output_descriptor.source_map.declared_space_tokens,
        vec!["space:external-file"]
    );
    assert_eq!(
        output_descriptor.source_space.source_token,
        "artifact:external-file"
    );
    assert_ne!(output_descriptor.source_space.token, "space:external-file");
}

// @ai-generated - Svelte's stage-two planner refuses authored preprocessor syntax.
#[test]
fn svelte_stage_two_refuses_authored_scss() {
    let source = "<div class=\"x\"></div><style lang=\"scss\">.x { color: $tone; }</style>";
    let allocator = Allocator::default();
    let parsed = parse_svelte(source);
    let error = compile_client(
        source,
        &parsed,
        &SvelteRuntimeOptions {
            filename: Some("App.svelte".to_string()),
            is_production: true,
            ..SvelteRuntimeOptions::default()
        },
        &allocator,
        false,
        true,
    )
    .expect_err("authored SCSS cannot enter Svelte scoping");

    let ClientCompileError::Unsupported(surface) = error else {
        panic!("expected a typed unsupported style surface: {error:?}");
    };
    assert_eq!(
        surface.diagnostic_code(),
        "svelte-runtime-style-stage-requires-plain-css"
    );
}

// @ai-generated - The user-facing Vue compile route inherits planner fail-closed behavior.
#[test]
fn vue_compile_routes_authored_styles_through_the_ir_planner() {
    let source = StandaloneSourceBytes::copied_from(
        "<template/><style>.bad { color: v-bind(tone; }</style>",
    );
    let result = StandaloneCompiler.compile_source(
        &source,
        &CodegenOptions {
            target: CompileTarget::STYLE,
            ..CodegenOptions::default()
        },
        &VerterCompileOptions::default(),
        &VueMacroSemanticInput::Unavailable,
    );

    assert!(
        result
            .errors
            .iter()
            .any(|diagnostic| diagnostic.message.contains("UntrustedRewriteTarget")),
        "planner refusal missing: {:?}",
        result.errors
    );
    assert_eq!(result.styles[0].code, ".bad { color: v-bind(tone; }");
}
