use crate::code_transform::{
    code_transform_construction_count, reset_code_transform_construction_count,
};
/**
 * @ai-generated - Exercises the typed two-stage framework style planner boundary.
 */
use crate::style_planner::{
    analyze_css_module_classes, analyze_style, build_string_invocation_count,
    cascade_output_is_publishable, cascade_requested_source_map, last_parse_ir_dialect,
    parse_ir_invocation_count, parse_plain_css_for_verification,
    reset_build_string_invocation_count, reset_last_parse_ir_dialect,
    reset_parse_ir_invocation_count, reset_style_ir_stage_observations,
    run_vue_style_authored_only, run_vue_style_cascade, style_ir_stage_observations,
    transform_vue_css_modules, transform_vue_scoped_css, transform_vue_style, transform_vue_v_bind,
    AuthoredStyleInput, CascadeInput, PlainCssInput, StyleRewriteFailure, StyleRewriteFailureClass,
    StyleRewriteOutcome, StyleRewriteStage, VerifiedPlainCss, VueStyleCascadeOutcome,
};
use crate::{
    compile::{types::VueExecutionInputs, VueMacroSemanticInput},
    compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
        VueCompileRequest,
    },
};
use verter_css_syntax::CssDialect;

use crate::svelte::{
    parse_svelte,
    runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions},
};
use oxc_allocator::Allocator;

#[path = "../../tests/support/style_planner_gen.rs"]
mod style_planner_gen;

fn rewritten(outcome: StyleRewriteOutcome) -> (String, String) {
    match outcome {
        StyleRewriteOutcome::Rewritten {
            code, source_map, ..
        } => (code, source_map),
        StyleRewriteOutcome::Unchanged { .. } => panic!("expected a rewrite"),
    }
}

fn nesting_overflow_css() -> String {
    const DEPTH: usize = 130;
    let mut source = String::with_capacity(DEPTH * 3);
    for _ in 0..DEPTH {
        source.push_str("a{");
    }
    for _ in 0..DEPTH {
        source.push('}');
    }
    source
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

fn selector_head(source: &str) -> String {
    normalize_selector(source.split_once('{').expect("probe contains a rule").0)
}

fn normalize_selector(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn animation_value_identifiers(source: &str) -> Vec<&str> {
    let mut identifiers = Vec::new();
    for property in [
        "animation:",
        "animation-name:",
        "-webkit-animation:",
        "-webkit-animation-name:",
    ] {
        let mut remaining = source;
        while let Some(start) = remaining.find(property) {
            let value = &remaining[start + property.len()..];
            let end = value.find([';', '}']).unwrap_or(value.len());
            identifiers.extend(
                value[..end]
                    .split(|character: char| {
                        !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
                    })
                    .filter(|identifier| !identifier.is_empty()),
            );
            remaining = &value[end..];
        }
    }
    identifiers
}

fn compile_style(source: &str) -> crate::compile::VerterCompileResult {
    // Style-planner tests need a valid carrier around the authored style. A style-only SFC is
    // intentionally diagnosed as missing its template/script entry, which is unrelated to the
    // rewrite behavior these fixtures exercise.
    //
    // Drives the pre-assembly `compile()` entry directly (this module lives
    // inside the crate under `#[cfg(test)]` precisely so it can — the raw
    // per-style-block `VerterCompileResult` this file inspects has no
    // equivalent on the public one-shot `StandaloneCompiler::compile` atomic
    // contract).
    let carrier = format!("<template></template>{source}");
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        Some("sc1".to_string()),
        false,
        false,
    )
    .expect("a lone RuntimeClient product must construct");
    let allocator = Allocator::new();
    crate::compile::compile(
        &carrier,
        &request,
        &VueExecutionInputs::default(),
        &VueMacroSemanticInput::Unavailable,
        &allocator,
    )
    .expect("a plain RuntimeClient compile must not be refused")
}

// @ai-generated - R2-2 follows the official Vue compiler's whole-selector global replacement.
#[test]
fn vue_global_rewrite_matches_official_selector_replacement() {
    for (source, expected) in [
        (":global(.a) .b { color: red }", ".a"),
        (":global(.a) > .b { color: red }", ".a"),
        (".a :global(.b) { color: red }", ".b"),
    ] {
        let actual = scoped(source, "sc1");
        assert_eq!(selector_head(&actual), expected, "{source}");
        assert!(!actual.contains(":global("), "{actual}");
    }
}

// @ai-generated - R3-10 preserves the authored rule-boundary trivia after global replacement.
#[test]
fn vue_global_rewrite_preserves_rule_boundary_whitespace() {
    assert_eq!(
        scoped(":global(.a) { color: red }", "sc1"),
        ".a { color: red }"
    );
}

#[derive(serde::Deserialize)]
struct VuePseudoOracleRow {
    selector: String,
    expected: String,
}

// @ai-generated - R2-1..R2-4 pin the pseudo-selector matrix generated by @vue/compiler-sfc.
#[test]
fn vue_pseudo_selector_conformance_matrix() {
    let rows: Vec<VuePseudoOracleRow> =
        serde_json::from_str(include_str!("vue_style_pseudo_oracle.json"))
            .expect("valid generated Vue style oracle");
    assert!(rows.len() >= 30, "pseudo matrix unexpectedly shrank");

    for row in rows {
        let source = format!("{} {{ color: red }}", row.selector);
        let actual = scoped(&source, "sc1");
        assert_eq!(
            selector_head(&actual),
            normalize_selector(&row.expected),
            "{source}"
        );
    }
}

// @ai-generated - R2-3 keeps Vue's scope anchor on the preceding compound.
#[test]
fn vue_deep_scope_anchor_stays_on_the_scoped_element() {
    for (source, expected) in [
        (".a :deep(.b) { color: red }", ".a[data-v-sc1] .b"),
        (
            ".a[data-x] :deep(.b) { color: red }",
            ".a[data-x][data-v-sc1] .b",
        ),
        ("a:hover :deep(.b) { color: red }", "a[data-v-sc1]:hover .b"),
        (".a > :deep(.b) { color: red }", ".a[data-v-sc1] > .b"),
    ] {
        let actual = scoped(source, "sc1");
        assert_eq!(selector_head(&actual), expected, "{source}: {actual}");
        assert!(!actual.contains(".a [data-v-sc1]"), "{actual}");
    }
}

// @ai-generated - SP-03 refuses only the unsafe rule and scopes its trusted sibling.
#[test]
fn vue_stage_two_refusal_is_fail_closed_per_rule() {
    let result = compile_style("<style scoped>.good { color: red } .bad { color red; }</style>");
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
    assert!(!code.contains(".bad"), "unsafe rule shipped: {code}");
    assert!(!code.contains("color red"), "unsafe rule shipped: {code}");
}

// @ai-generated - Unknown/unsupported style lang must fail closed: no CSS cascade rewrite.
#[test]
fn vue_unknown_style_lang_does_not_produce_a_css_cascade_rewrite() {
    let v_bind = compile_style("<style lang=\"postcss\">.a { color: v-bind(t) }</style>");
    let v_bind_code = &v_bind.styles[0].code;
    assert!(
        !v_bind_code.contains("var(--"),
        "unknown lang must not rewrite v-bind as CSS: {v_bind_code}"
    );
    assert!(
        v_bind_code.contains("v-bind(t)") || v_bind_code.is_empty(),
        "unknown lang must not emit a CSS cascade rewrite: {v_bind_code}"
    );
    assert!(
        v_bind.errors.iter().any(|diagnostic| {
            diagnostic.message.contains("unknown") || diagnostic.message.contains("dialect")
        }),
        "unknown lang must fail closed with a diagnostic: {:?}",
        v_bind.errors
    );

    let scoped = compile_style("<style lang=\"postcss\" scoped>.a { color: red }</style>");
    let scoped_code = &scoped.styles[0].code;
    assert!(
        !scoped_code.contains("[data-v-"),
        "unknown lang must not receive a CSS scoped rewrite: {scoped_code}"
    );
    assert!(
        scoped.errors.iter().any(|diagnostic| {
            diagnostic.message.contains("unknown") || diagnostic.message.contains("dialect")
        }),
        "unknown scoped lang must fail closed with a diagnostic: {:?}",
        scoped.errors
    );
}

// @ai-generated - R2-6 refuses scoped authored dialects until plain CSS is supplied.
//
// Mutation recipe: in `CssStageRequest::gated`, drop the refusal from the
// `Err` arm (`Self { module: false, scoped: false, refusal: None }`) — the
// CSS-only stages are still dropped, so every non-CSS dialect publishes its
// authored bytes with neither a rewrite nor a recorded refusal, which is the
// silent unscoped-publication outcome the sweep exists to name.
#[test]
fn vue_scoped_non_css_never_publishes_unscoped_css() {
    // Every dialect answers a `<style scoped>` request one of exactly two
    // ways: it rewrites the bytes, or it records a refusal. Publishing the
    // authored bytes unscoped with neither is the wrong-complete outcome —
    // selectors that apply to the whole document — and it is what a dialect
    // that is neither plain CSS nor externally preprocessed would get if the
    // cascade's entry gate were not the exact complement of the non-CSS
    // branch's refusal predicate. Swept over the dialect owner's own variant
    // list, and asserted before the Less case below, so a dialect added there
    // has to answer this question on its own rather than behind a refusal one
    // named dialect already proves.
    const AUTHORED: &str = ".a { color: red }";
    for dialect in CssDialect::ALL {
        let outcome = run_vue_style_cascade(
            AuthoredStyleInput::new(
                AUTHORED,
                dialect,
                "probe.style",
                "space:probe",
                "artifact:probe",
            ),
            "sc1",
            false,
            true,
            false,
        );
        assert!(
            !outcome.stage_failures.is_empty() || outcome.code() != AUTHORED,
            "{dialect:?} published authored bytes unscoped without recording a refusal"
        );
    }

    // And the refusal reaches a real consumer through the public compile
    // boundary, cleared rather than published.
    let less = compile_style("<style lang=\"less\" scoped>.a { color: red }</style>");
    assert!(less.styles[0].code.is_empty(), "{}", less.styles[0].code);
    assert!(
        less.errors
            .iter()
            .any(|diagnostic| diagnostic.message.contains("StageRequiresPlainCss")),
        "{:?}",
        less.errors
    );
}

// @ai-generated - SP-05 keeps standalone CSS Modules output aligned with $style lookups.
#[test]
fn vue_style_module_hashes_emitted_classes() {
    let direct = PlainCssInput::try_new(
        ".active { color: red }",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .expect("plain CSS stage input");
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

// @ai-generated - R2-5 includes interpolation hidden inside a quoted v-bind expression.
#[test]
fn vue_v_bind_refuses_quoted_dialect_interpolation() {
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind('#{$x}'); }",
        CssDialect::Scss,
        "probe.scss",
        "space:probe",
        "artifact:probe",
    );
    let failure =
        transform_vue_v_bind(input, "sc1").expect_err("quoted interpolation must be typed-refused");
    assert_eq!(
        failure.class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );
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

// @ai-generated - R2-9 leaves quoted animation names unchanged, matching Vue.
#[test]
fn vue_scoping_preserves_quoted_animation_names() {
    let code = scoped(
        "@keyframes f { from { opacity: 0 } } .a { animation-name: \"f\" }",
        "sc1",
    );
    assert!(code.contains("@keyframes f-sc1"), "{code}");
    assert!(code.contains("animation-name: \"f\""), "{code}");
    assert!(!code.contains("animation-name: \"f-sc1\""), "{code}");
}

// @ai-generated - R2-4 scopes native CSS nesting instead of deleting the containing rule.
#[test]
fn vue_scoping_handles_native_css_nesting() {
    let result = compile_style("<style scoped>.a { &:hover { color: blue } }</style>");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = &result.styles[0].code;
    assert!(code.contains("&[data-v-sc100000]:hover"), "{code}");
    assert!(!code.contains("&:hover"), "{code}");
}

// @ai-generated - SP-15 removes the deprecated deep combinator instead of publishing invalid CSS.
#[test]
fn vue_scoping_rewrites_deprecated_deep_combinator() {
    let code = scoped(".a >>> .b { color: red }", "sc1");
    assert!(!code.contains(">>>"), "{code}");
    assert!(code.starts_with(".a[data-v-sc1] .b"), "{code}");
    assert!(!code.contains("   "), "{code}");
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
    let source = "<template/><style>.bad { color: v-bind(tone; }</style>";
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("a lone RuntimeClient product must construct");
    let allocator = Allocator::new();
    let result = crate::compile::compile(
        source,
        &request,
        &VueExecutionInputs::default(),
        &VueMacroSemanticInput::Unavailable,
        &allocator,
    )
    .expect("a plain RuntimeClient compile must not be refused");

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

/// A refusal planned beside an earlier edit still addresses authored bytes.
/// The shared plan never materializes the v-bind rewrite before CSS Modules
/// inspects the stylesheet, so the refusal keeps its exact authored anchor.
///
/// Mutation recipe: make `finish_vue_style_cascade` project diagnostics with
/// `StyleStage::FrameworkRewritten` instead of `input_stage` — the carrier
/// then anchors on the whole block and the exact `}` span assertion fails.
#[test]
fn shared_plan_refusal_keeps_its_authored_anchor_after_prior_edits() {
    let style = "<style module>.a { color: v-bind(tone); }\n}\n</style>";
    let source = format!("<template/>{style}");
    let refusal_start = u32::try_from(source.rfind("}\n</style>").expect("stray close brace"))
        .expect("fixture offset fits u32");

    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        Some("sc1".to_string()),
        false,
        false,
    )
    .expect("a lone RuntimeClient product must construct");
    let allocator = Allocator::new();
    let result = crate::compile::compile(
        &source,
        &request,
        &VueExecutionInputs::default(),
        &VueMacroSemanticInput::Unavailable,
        &allocator,
    )
    .expect("a plain RuntimeClient compile must not be refused");

    let refusal = result
        .errors
        .iter()
        .find(|diagnostic| diagnostic.message.contains("UntrustedRewriteTarget"))
        .unwrap_or_else(|| panic!("planner refusal missing: {:?}", result.errors));
    let span = refusal
        .span
        .expect("a refusal reaches the carrier positioned");

    assert_eq!(
        (span.start, span.end),
        (refusal_start, refusal_start + 1),
        "the refusal must retain its authored byte anchor"
    );
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        "}",
        "the anchor is the unsupported authored token"
    );
}

// ─── J1 §3.1 vue-benchmarks probe findings (A10d-h) ────────────────────────
//
// Oracle: literal, exact byte capture from the workspace's pinned
// `@vue/compiler-sfc@3.6.0-rc.5` (J1.md §3.1). These assert the SEMANTIC content
// the oracle proves — attribute-injection point/presence, renamed
// identifiers, reference-rewrite targets — token-normalized via `scoped()`'s
// `selector_head()`/`normalize_selector()` helpers, never raw string
// equality against Vue's own cosmetic reprint (Compiled-Output Conformance).

// @ai-generated - A10d: `:deep()` as its own space-separated segment attaches
// the scope attribute to the preceding compound with NO combinator space.
#[test]
fn deep_segment_no_combinator_space() {
    let actual = scoped(".deep-host :deep(.deep-target) { color:red }", "x");
    assert_eq!(selector_head(&actual), ".deep-host[data-v-x] .deep-target");
    assert!(!actual.contains(".deep-host ["), "{actual}");
}

// @ai-generated - A10e: `:global()` composed with surrounding local selector
// segments discards EVERYTHING outside the parens.
#[test]
fn global_discards_surrounding_context() {
    let actual = scoped(".foo :global(.bar) .baz { color:red }", "x");
    assert_eq!(selector_head(&actual), ".bar");
    assert!(!actual.contains(":global("), "{actual}");
    assert!(!actual.contains(".foo"), "{actual}");
    assert!(!actual.contains(".baz"), "{actual}");
}

// @ai-generated - A10f (anchored branch): with an outer local anchor, the
// anchor is scoped and the `:is()`/`:where()` argument list stays unscoped.
#[test]
fn is_where_anchored_argument_list_unscoped() {
    let is_actual = scoped(".item:is(.a, .b) { color:red }", "x");
    assert_eq!(selector_head(&is_actual), ".item[data-v-x]:is(.a, .b)");

    let where_actual = scoped(".item:where(.a, .b) { color:red }", "x");
    assert_eq!(
        selector_head(&where_actual),
        ".item[data-v-x]:where(.a, .b)"
    );
}

// @ai-generated - A10f (bare branch): with no outer local anchor, EACH
// `:is()`/`:where()` argument is scoped individually.
#[test]
fn is_where_bare_selector_scopes_each_argument() {
    let is_actual = scoped(":is(.a, .b) { color:red }", "x");
    assert_eq!(selector_head(&is_actual), ":is(.a[data-v-x], .b[data-v-x])");

    let where_actual = scoped(":where(.a, .b) { color:red }", "x");
    assert_eq!(
        selector_head(&where_actual),
        ":where(.a[data-v-x], .b[data-v-x])"
    );
}

// @ai-generated - A10g: scoped `@keyframes` identifier uniqueing renames the
// keyframes name and rewrites every `animation`/`animation-name` reference,
// including each entry of a comma-separated list — `none` stays untouched.
#[test]
fn scoped_keyframes_rename_and_rewrite_references() {
    let source = "@keyframes fade { from { opacity: 0; } to { opacity: 1; } } \
                  @keyframes spin { to { transform: rotate(1turn); } } \
                  .x { animation: fade 1s, 2s linear spin; \
                  animation-name: fade, none, spin; \
                  -webkit-animation: spin 3s; \
                  -webkit-animation-name: fade, spin; }";
    let actual = scoped(source, "x");
    assert!(actual.contains("@keyframes fade-x"), "{actual}");
    assert!(actual.contains("@keyframes spin-x"), "{actual}");
    assert!(!actual.contains("@keyframes fade {"), "{actual}");
    assert!(!actual.contains("@keyframes spin {"), "{actual}");
    let animation_identifiers = animation_value_identifiers(&actual);
    assert!(
        !animation_identifiers.contains(&"fade") && !animation_identifiers.contains(&"spin"),
        "stale keyframe identifier in animation declarations: {animation_identifiers:?}; {actual}"
    );
    for stale in [
        "animation: fade 1s",
        "2s linear spin;",
        "animation-name: fade,",
        "none, spin;",
        "-webkit-animation: spin 3s",
        "-webkit-animation-name: fade, spin",
    ] {
        assert!(
            !actual.contains(stale),
            "stale keyframe reference {stale:?}: {actual}"
        );
    }
    assert!(
        actual.contains("animation: fade-x 1s, 2s linear spin-x"),
        "{actual}"
    );
    assert!(
        actual.contains("animation-name: fade-x, none, spin-x"),
        "{actual}"
    );
    assert!(actual.contains("-webkit-animation: spin-x 3s"), "{actual}");
    assert!(
        actual.contains("-webkit-animation-name: fade-x, spin-x"),
        "{actual}"
    );
}

// @ai-generated - A10h (highest severity): the client `_useCssVars` runtime
// prepends `--` itself, so the JS-registered key must be bare while the CSS
// `var()` reference carries the full `--`-prefixed custom-property name.
// Baking `--` into both sides (the pre-fix bug) makes the runtime
// double-prepend it and the binding silently never applies.
#[test]
fn v_bind_js_key_and_css_var_reference_agree() {
    let source = "<script setup>\nimport { ref } from 'vue'\nconst color = ref('red')\n</script>\n\
         <template><div/></template>\n\
         <style scoped>.x { color: v-bind(color); }</style>";
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        Some("sc1".to_string()),
        false,
        false,
    )
    .expect("a lone RuntimeClient product must construct");
    let allocator = Allocator::new();
    let result = crate::compile::compile(
        source,
        &request,
        &VueExecutionInputs::default(),
        &VueMacroSemanticInput::Unavailable,
        &allocator,
    )
    .expect("a plain RuntimeClient compile must not be refused");

    let css = &result.styles[0].code;
    assert!(css.contains("var(--sc100000-color)"), "CSS side: {css}");

    let script = &result.script.expect("script block emitted").code;
    assert!(
        script.contains("\"sc100000-color\":"),
        "JS key must be bare (no --): {script}"
    );
    assert!(
        !script.contains("\"--sc100000-color\":"),
        "JS key must not carry the CSS `--` prefix: {script}"
    );
}

// ─── A10a/A10b: Native CSS-Modules class *analysis* for all 5 dialects ─────

// @ai-generated - A10a/A10b: class analysis is unconditional over all 5
// native dialects (Native), not silently restricted to `dialect == Css` —
// runtime class-name rewriting (row 19) is untouched, this is analysis only.
#[test]
fn css_modules_class_analysis_native_for_all_five_dialects() {
    let cases = [
        (
            CssDialect::Css,
            "@layer components { .active { color: red; } }",
        ),
        (
            CssDialect::Scss,
            "// scss-native line comment\n$tone: red; @mixin paint { color: $tone; } .active { @include paint; }",
        ),
        (
            CssDialect::Sass,
            "$tone: red\n=paint\n  color: $tone\n.active\n  +paint\n",
        ),
        (
            CssDialect::Less,
            "@tone: red; .paint() { color: @tone; } .active { .paint(); }",
        ),
        (CssDialect::Stylus, ".active\n  color: red\n"),
    ];

    let mut hashed_names = Vec::new();
    for (dialect, source) in cases {
        reset_last_parse_ir_dialect();
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "probe.style",
            "space:probe",
            "artifact:probe",
        );
        let classes = analyze_css_module_classes(input, "sc1")
            .unwrap_or_else(|e| panic!("{dialect:?} class analysis must not be refused: {e}"));
        assert_eq!(
            last_parse_ir_dialect(),
            Some(dialect),
            "{dialect:?} analysis did not use its authored parser path"
        );
        if dialect != CssDialect::Css {
            assert_ne!(
                last_parse_ir_dialect(),
                Some(CssDialect::Css),
                "{dialect:?} analysis was routed through CSS"
            );
        }
        assert_eq!(classes.len(), 1, "{dialect:?}: {classes:?}");
        assert_eq!(classes[0].0, "active", "{dialect:?}: {classes:?}");
        assert!(
            classes.iter().all(|(name, _)| name != "paint"),
            "{dialect:?}: native mixin/function syntax must not become a class: {classes:?}"
        );
        hashed_names.push(classes[0].1.clone());
    }

    // Same class name + scope id must hash identically regardless of the
    // authoring dialect — the class-selector shape is dialect-neutral.
    assert!(
        hashed_names.iter().all(|name| name == &hashed_names[0]),
        "{hashed_names:?}"
    );
    assert_ne!(
        hashed_names[0], "active",
        "must actually hash, not pass through"
    );
}

#[test]
fn read_only_analysis_keeps_valid_classes_beside_an_untrusted_selector() {
    let source = ".good { color: red; } .bad-#{$name} { color: blue; }";
    let input =
        || AuthoredStyleInput::new(source, CssDialect::Scss, "Probe.scss", "probe", "probe");

    assert!(
        analyze_css_module_classes(input(), "probe1234").is_err(),
        "the fixture must exercise the stricter rewrite-oriented refusal"
    );

    let analysis = analyze_style(input(), "probe1234")
        .expect("read-only analysis must not inherit rewrite refusal");
    assert!(analysis.static_classes.contains(&"good".to_string()));
    assert_eq!(analysis.module_classes.len(), 1);
    assert_eq!(analysis.module_classes[0].0, "good");
    assert_ne!(analysis.module_classes[0].1, "good");
    assert!(
        analysis
            .module_classes
            .iter()
            .all(|(name, _)| !name.starts_with("bad")),
        "a dynamic selector must not invent a static module-class fact"
    );
}

// ─── A10 directive-required evidence categories ────────────────────────────

// @ai-generated - A10: an untouched style block with no Vue-owned construct
// stays byte-identical (`Unchanged`) across all 5 native dialects.
#[test]
fn plain_passthrough_preserves_authored_bytes_all_five_dialects() {
    let cases = [
        (
            CssDialect::Css,
            "@media (width >= 1px) { .a { color: red; } }\n",
        ),
        (
            CssDialect::Scss,
            "$c: red;\n@mixin paint { color: $c; }\n.a { @include paint; }\n",
        ),
        (CssDialect::Sass, "$c: red\n.a\n  color: $c\n"),
        (CssDialect::Less, "@c: red;\n.a { color: @c; }\n"),
        (CssDialect::Stylus, "c = red\n.a\n  color c\n"),
    ];

    for (dialect, source) in cases {
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "probe.style",
            "space:probe",
            "artifact:probe",
        );
        let outcome = transform_vue_v_bind(input, "sc1")
            .unwrap_or_else(|e| panic!("{dialect:?} plain style must not be refused: {e}"));
        match outcome {
            StyleRewriteOutcome::Unchanged { .. } => {}
            StyleRewriteOutcome::Rewritten { code, .. } => {
                panic!("{dialect:?} expected byte-identical passthrough, got: {code}")
            }
        }

        let cascaded = run_vue_style_cascade(input, "sc1", false, false, false);
        assert_eq!(
            cascaded.code(),
            source,
            "{dialect:?} authored bytes changed"
        );
        for forbidden in ["data-v-sc1", "var(--sc1-", "active_"] {
            assert!(
                !cascaded.code().contains(forbidden),
                "{dialect:?} unexpected rewrite {forbidden:?}: {}",
                cascaded.code()
            );
        }
    }
}

// @ai-generated - A10: native CSS nesting is scoped correctly (Css, the only
// dialect the post-preprocess scoping stage runs under); the authored v-bind
// stage round-trips native `&` nesting syntax byte-for-byte for the other
// four dialects, proving the shared IR walk never corrupts nesting content
// regardless of authoring dialect.
#[test]
fn css_nesting_transforms_correctly_all_five_dialects() {
    let css = scoped(".a { &:hover { color: blue } }", "sc1");
    assert!(css.contains("&[data-v-sc1]:hover"), "{css}");
    assert!(!css.contains("&:hover"), "{css}");

    for (dialect, source) in [
        (CssDialect::Scss, ".a { &:hover { color: blue; } }"),
        (CssDialect::Sass, ".a\n  &:hover\n    color: blue\n"),
        (CssDialect::Less, ".a { &:hover { color: blue; } }"),
        (CssDialect::Stylus, ".a\n  &:hover\n    color: blue\n"),
    ] {
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "probe.style",
            "space:probe",
            "artifact:probe",
        );
        let outcome = transform_vue_v_bind(input, "sc1")
            .unwrap_or_else(|e| panic!("{dialect:?} nesting must not be refused: {e}"));
        let code = match outcome {
            StyleRewriteOutcome::Unchanged { .. } => source.to_string(),
            StyleRewriteOutcome::Rewritten { code, .. } => code,
        };
        assert_eq!(
            code, source,
            "{dialect:?}: nesting syntax must round-trip byte-for-byte"
        );
    }
}

// @ai-generated - A10 / §2 regression guard: no lightningcss-style
// modern-syntax normalization of an untouched declaration survives — e.g.
// `@media (min-width:1px)` must never silently become `(width >= 1px)`.
#[test]
fn no_modern_syntax_normalization_of_untouched_declarations() {
    let source =
        "@media (min-width:1px) { .a { color: color(display-p3 1 0 0); width: calc(1px + 2%); } }";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    match transform_vue_v_bind(input, "sc1").expect("plain media query must not be refused") {
        StyleRewriteOutcome::Unchanged { .. } => {}
        StyleRewriteOutcome::Rewritten { code, .. } => {
            panic!("must not rewrite an untouched declaration: {code}")
        }
    }

    let scoped_code = scoped(source, "sc1");
    assert!(scoped_code.contains("(min-width:1px)"), "{scoped_code}");
    assert!(
        scoped_code.contains("color: color(display-p3 1 0 0)"),
        "{scoped_code}"
    );
    assert!(scoped_code.contains("calc(1px + 2%)"), "{scoped_code}");
    assert!(!scoped_code.contains("(width >= 1px)"), "{scoped_code}");
    assert!(!scoped_code.contains("color: red"), "{scoped_code}");
    assert!(!scoped_code.contains("calc(2% + 1px)"), "{scoped_code}");
}

// ─── §2 Bounds: Edit topology ───────────────────────────────────────────────

// @ai-generated - Edit topology bound: a style block with 0 edits returns
// `Unchanged` via an early return before any `CodeTransform` is constructed.
#[test]
fn zero_edit_style_block_returns_unchanged_variant() {
    let source = "body { color: red; }";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    assert!(matches!(
        transform_vue_v_bind(input, "sc1").expect("plain css must not be refused"),
        StyleRewriteOutcome::Unchanged { .. }
    ));

    let plain = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    assert!(matches!(
        transform_vue_css_modules(plain, "sc1").expect("no class selectors, no edits"),
        StyleRewriteOutcome::Unchanged { .. }
    ));

    let empty = PlainCssInput::try_new(
        "",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    assert!(matches!(
        transform_vue_scoped_css(empty, "sc1").expect("empty style has no selectors to scope"),
        StyleRewriteOutcome::Unchanged { .. }
    ));
}

// @ai-generated - Every direct and cascaded zero-edit route returns before
// `CodeTransform::new`; outcome-variant checks alone cannot establish this.
#[test]
fn zero_edit_routes_construct_no_code_transform() {
    for (dialect, source) in [
        (CssDialect::Css, "body { color: red; }"),
        (CssDialect::Scss, "$tone: red; body { color: $tone; }"),
        (CssDialect::Sass, "body\n  color: red\n"),
        (CssDialect::Less, "@tone: red; body { color: @tone; }"),
        (CssDialect::Stylus, "body\n  color red\n"),
    ] {
        reset_code_transform_construction_count();
        let input = AuthoredStyleInput::new(
            source,
            dialect,
            "probe.style",
            "space:probe",
            "artifact:probe",
        );
        let outcome = transform_vue_v_bind(input, "sc1")
            .unwrap_or_else(|error| panic!("{dialect:?} must not be refused: {error}"));
        assert!(!matches!(outcome, StyleRewriteOutcome::Rewritten { .. }));
        assert_eq!(
            code_transform_construction_count(),
            0,
            "{dialect:?} authored zero-edit route constructed CodeTransform"
        );
    }

    reset_code_transform_construction_count();
    let plain = PlainCssInput::try_new(
        "body { color: red; }",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    let outcome = transform_vue_css_modules(plain, "sc1").expect("no module edits");
    assert!(!matches!(outcome, StyleRewriteOutcome::Rewritten { .. }));
    assert_eq!(
        code_transform_construction_count(),
        0,
        "module zero-edit route constructed CodeTransform"
    );

    reset_code_transform_construction_count();
    let empty = PlainCssInput::try_new(
        "",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    let outcome = transform_vue_scoped_css(empty, "sc1").expect("no scoped edits");
    assert!(!matches!(outcome, StyleRewriteOutcome::Rewritten { .. }));
    assert_eq!(
        code_transform_construction_count(),
        0,
        "scoped zero-edit route constructed CodeTransform"
    );

    for (module, scoped) in [(false, false), (true, false), (false, true), (true, true)] {
        reset_code_transform_construction_count();
        let input = AuthoredStyleInput::new(
            "",
            CssDialect::Css,
            "probe.css",
            "space:probe",
            "artifact:probe",
        );
        let outcome = run_vue_style_cascade(input, "sc1", module, scoped, true);
        assert_eq!(outcome.code(), "");
        assert_eq!(
            code_transform_construction_count(),
            0,
            "authored cascade module={module} scoped={scoped} constructed CodeTransform"
        );

        reset_code_transform_construction_count();
        let parsed = parse_plain_css_for_verification("", StyleRewriteStage::AuthoredVBind)
            .expect("empty sheet parses");
        let outcome = transform_vue_style(
            VerifiedPlainCss::from_parsed_native_css(&parsed).expect("native-CSS provenance"),
            CascadeInput::Preprocessed(verter_css_syntax::PreprocessorIdentity::Anonymous),
            "probe.css",
            "space:probe",
            "artifact:probe",
            "sc1",
            module,
            scoped,
            true,
        );
        assert_eq!(outcome.code(), "");
        assert_eq!(
            code_transform_construction_count(),
            0,
            "preprocessed cascade module={module} scoped={scoped} constructed CodeTransform"
        );
    }
}

// @ai-generated - Edit topology bound: compatible cascade stages contribute
// one shared edit plan and terminal `build_string()`.
//
// Mutation recipe: give `shared_vue_style_plan` a per-stage `apply_cascade_stage`
// (materialize after each plan instead of merging into one terminal edit
// vector) — every `build_string_invocation_count()` assertion below reads the
// stage count instead of 1.
//
// `:slotted()` argument scoping contributes
// absolute-span edits directly to the outer emit's edit vector, so N
// `:slotted()` occurrences still cost one outer emit build — whether an
// occurrence contributes one argument edit (`.a`) or several
// (`:is(.a, .b)` fans out to one insert per arm).
#[test]
fn build_string_call_count_matches_edit_composition_depth() {
    reset_build_string_invocation_count();
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind(tone); }",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    let v_bind = rewritten(transform_vue_v_bind(input, "sc1").expect("v-bind rewrite")).0;
    assert!(!v_bind.contains("v-bind("), "{v_bind}");
    assert_eq!(build_string_invocation_count(), 1, "authored v-bind emit");

    reset_build_string_invocation_count();
    let plain = PlainCssInput::try_new(
        ".a { color: red; }",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    let modules = rewritten(transform_vue_css_modules(plain, "sc1").expect("module rewrite")).0;
    assert!(!modules.contains(".a {"), "{modules}");
    assert_eq!(build_string_invocation_count(), 1, "CSS Modules emit");

    reset_build_string_invocation_count();
    let flat = scoped(".a { color: red }", "sc1");
    assert!(!flat.contains(".a {"), "{flat}");
    assert_eq!(build_string_invocation_count(), 1, "flat scope insertion");

    reset_build_string_invocation_count();
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind(tone); }",
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    let cascaded = run_vue_style_cascade(input, "sc1", true, true, true);
    assert!(!cascaded.code().contains("v-bind("), "{}", cascaded.code());
    assert!(!cascaded.code().contains(".a {"), "{}", cascaded.code());
    assert_eq!(
        build_string_invocation_count(),
        1,
        "compatible v-bind, module, and scoped edits share one terminal build"
    );

    // A request the plain-CSS gate refused publishes nothing, so the plan must
    // not materialize bytes first. The v-bind edits are still planned (the
    // facts feed `_useCssVars`), which is exactly what makes 0 the answer under
    // test rather than a consequence of there being nothing to emit: a route
    // that ran the authored transform and then discarded its output reads 1.
    reset_build_string_invocation_count();
    let gate_refused = run_vue_style_cascade(
        AuthoredStyleInput::new(
            ".a { color: v-bind(tone); }",
            CssDialect::Scss,
            "probe.scss",
            "space:probe",
            "artifact:probe",
        ),
        "sc1",
        true,
        false,
        true,
    );
    assert!(
        gate_refused.result.is_refused(),
        "an SCSS <style module> request refuses at the plain-CSS gate"
    );
    assert_eq!(
        gate_refused.facts.v_bind_vars.len(),
        1,
        "the v-bind plan still ran, so the emit count is not vacuously zero"
    );
    assert_eq!(
        build_string_invocation_count(),
        0,
        "a cleared output must never be materialized first"
    );

    reset_build_string_invocation_count();
    let source = ".outer :global(.inner) { color: red; }";
    let input = AuthoredStyleInput::new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    );
    let global = run_vue_style_cascade(input, "sc1", true, true, true);
    let shared_builds = build_string_invocation_count();
    let modules = rewritten(
        transform_vue_css_modules(
            PlainCssInput::try_new(
                source,
                CssDialect::Css,
                "probe.css",
                "space:probe",
                "artifact:probe",
            )
            .unwrap(),
            "sc1",
        )
        .expect("module rewrite"),
    )
    .0;
    let staged = rewritten(
        transform_vue_scoped_css(
            PlainCssInput::try_new(
                &modules,
                CssDialect::Css,
                "probe.css",
                "space:probe",
                "artifact:probe",
            )
            .unwrap(),
            "sc1",
        )
        .expect("scoped rewrite"),
    )
    .0;
    let inner = global
        .facts
        .module_classes
        .iter()
        .find(|(name, _)| name == "inner")
        .map(|(_, hashed)| hashed)
        .expect("module plan must retain the global argument class");
    assert!(global.code().contains(inner), "{}", global.code());
    assert!(!global.code().contains(":global("), "{}", global.code());
    assert_eq!(
        global.code(),
        staged,
        "shared and staged semantics diverged"
    );
    assert_eq!(
        shared_builds, 1,
        "a containing global rewrite must absorb the earlier module edit"
    );

    reset_build_string_invocation_count();
    let _ = scoped(".a :deep(.b) { color: red }", "sc1");
    assert_eq!(
        build_string_invocation_count(),
        1,
        ":deep()'s argument is never itself scoped, so no nested build"
    );

    reset_build_string_invocation_count();
    let _ = scoped(":slotted(.a) { color: red }", "sc1");
    assert_eq!(
        build_string_invocation_count(),
        1,
        ":slotted() argument scoping rides the outer transform, not a nested one"
    );

    reset_build_string_invocation_count();
    let _ = scoped(":slotted(:is(.a, .b)) { color: red }", "sc1");
    assert_eq!(
        build_string_invocation_count(),
        1,
        "a multi-edit :slotted() argument (one insert per :is() arm) still \
         rides the one outer transform"
    );

    reset_build_string_invocation_count();
    let code = scoped(":slotted(:is(.a, .b, .c)) { color: red }", "sc1");
    assert!(
        code.contains(":is(.a[data-v-sc1-s], .b[data-v-sc1-s], .c[data-v-sc1-s])"),
        "control: the three-arm argument must really fan out to three edits: {code}"
    );
    assert_eq!(
        build_string_invocation_count(),
        1,
        "a three-edit :slotted() argument still rides the one outer transform — \
         a regression gated on more than two argument edits is caught here while \
         the one- and two-edit cases above stay green"
    );
}

// @ai-generated - A later stage must ignore source bytes replaced by an
// earlier rewrite while preserving staged output and one terminal build.
//
// Mutation recipe: make `merge_shared_stage_edits` return `None` for a later
// edit strictly inside an earlier overwrite (instead of discarding it) — the
// cascade records a stage failure and `shared.code()` no longer equals the
// staged output.
#[test]
fn later_rewrite_inside_v_bind_replacement_matches_staged_semantics() {
    let source = "@keyframes pulse { to { opacity: 0; } }\n\
                  .item { animation-name: v-bind(pulse); }";

    reset_build_string_invocation_count();
    let shared = run_vue_style_cascade(
        AuthoredStyleInput::new(
            source,
            CssDialect::Css,
            "overlap.css",
            "space:overlap",
            "artifact:overlap",
        ),
        "sc1",
        false,
        true,
        true,
    );
    let shared_builds = build_string_invocation_count();

    let v_bind = rewritten(
        transform_vue_v_bind(
            AuthoredStyleInput::new(
                source,
                CssDialect::Css,
                "overlap.css",
                "space:overlap",
                "artifact:overlap",
            ),
            "sc1",
        )
        .expect("v-bind rewrite"),
    )
    .0;
    let staged = rewritten(
        transform_vue_scoped_css(
            PlainCssInput::try_new(
                &v_bind,
                CssDialect::Css,
                "overlap.css",
                "space:overlap",
                "artifact:overlap",
            )
            .unwrap(),
            "sc1",
        )
        .expect("scoped rewrite"),
    )
    .0;

    assert!(
        shared.stage_failures.is_empty(),
        "shared plan refused a later edit whose source bytes no longer survive: {:?}",
        shared.stage_failures
    );
    assert_eq!(
        shared.code(),
        staged,
        "shared and staged semantics diverged"
    );
    assert_eq!(
        shared_builds, 1,
        "overlap resolution must retain one terminal materialization"
    );
}

// @ai-generated - The multi-edit `:slotted()` shape really is multi-edit: an
// `:is()` argument fans out to one scope-attribute insert per arm, applied at
// absolute source offsets by the one outer transform, with the `:slotted(`
// prefix and `)` suffix deleted around them.
#[test]
fn multi_edit_slotted_argument_scopes_each_is_arm() {
    let code = scoped(":slotted(:is(.a, .b)) { color: red }", "sc1");
    assert!(
        code.contains(":is(.a[data-v-sc1-s], .b[data-v-sc1-s])"),
        "each :is() arm must carry its own slotted attribute: {code}"
    );
    assert!(!code.contains(":slotted("), "{code}");
}

// @ai-generated - Three-arm variant of the shape control above: a three-arm
// `:is()` argument fans out to THREE scope-attribute inserts. This pins that
// the three-edit fixtures used by the build-count and allocation canaries
// really do produce three argument edits — reachable from ordinary CSS
// (`:slotted(:is(.a, .b, .c))`), not an assumed shape.
#[test]
fn three_edit_slotted_argument_scopes_each_is_arm() {
    let code = scoped(":slotted(:is(.a, .b, .c)) { color: red }", "sc1");
    assert!(
        code.contains(":is(.a[data-v-sc1-s], .b[data-v-sc1-s], .c[data-v-sc1-s])"),
        "each of the three :is() arms must carry its own slotted attribute: {code}"
    );
    assert!(!code.contains(":slotted("), "{code}");
}

// @ai-generated - Map-anchor guard: the emitted source map must carry
// mappings whose SOURCE offsets are the `:slotted()` argument bytes' own
// authored offsets, each sitting at a GENERATED position holding those very
// bytes. What this establishes is that the argument's authored offsets
// survive into the emitted map — which the known whole-component-overwrite
// splice provably does not produce: its only mapping for the component
// points at the component START, and no mapping's source position lands
// inside the argument, so that implementation fails this test. What it does
// NOT establish is that the argument bytes remained Original chunks of the
// one outer transform, nor the sole-edit-mechanism architecture itself: a
// re-rendering implementation that deliberately re-anchored its inserted
// content to the authored offsets would reproduce these anchors and pass.
// That residual is accepted; the sole-edit-mechanism property is held by the
// production structure, with this guard as evidence against the concrete
// splice regression, not as a proof of the architecture.
//
// Map decoding: the stage map is a standard JSON source map whose `mappings`
// field is Base64-VLQ; `oxc_sourcemap::SourceMap::from_json_string` decodes
// it into tokens exposing (dst_line, dst_col, src_line, src_col). The fixture
// is single-line, so authored byte offsets equal source columns on line 0.
#[test]
fn slotted_argument_bytes_map_to_their_own_authored_offsets() {
    let source = ":slotted(:is(.a, .b)) { color: red }";
    let input = PlainCssInput::try_new(
        source,
        CssDialect::Css,
        "probe.css",
        "space:probe",
        "artifact:probe",
    )
    .unwrap();
    let (code, map) =
        rewritten(transform_vue_scoped_css(input, "sc1").expect("trusted CSS scopes"));
    assert!(
        code.contains(":is(.a[data-v-sc1-s], .b[data-v-sc1-s])"),
        "control: the rewrite itself must be unchanged: {code}"
    );

    // Authored offsets derived from the fixture, never read off a run:
    // the argument starts at `:is(`, and the argument's second original run
    // (`, .b`) starts right after `.a`, where the first arm's attribute
    // insert splits the argument bytes.
    let argument_start = source.find(":is(").expect("fixture has the argument") as u32;
    let after_first_arm = (source.find(".a").expect("fixture has .a") + ".a".len()) as u32;

    let sm = oxc_sourcemap::SourceMap::from_json_string(&map).expect("valid stage map");
    let tokens: Vec<(u32, u32, u32, u32)> = sm
        .get_tokens()
        .map(|token| {
            (
                token.get_dst_line(),
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
            )
        })
        .collect();

    // Both authored positions must appear as mapping SOURCE positions, and
    // each mapping's GENERATED position must sit on the very bytes it claims
    // to preserve. This pins the anchors, not the chunk kind: it rules out
    // the whole-component-overwrite splice (which maps neither position),
    // while a re-rendering that deliberately re-anchored its inserted
    // content would still satisfy it.
    let generated_text_at = |dst_line: u32, dst_col: u32| -> &str {
        assert_eq!(dst_line, 0, "single-line fixture stays single-line");
        &code[dst_col as usize..]
    };
    let argument_token = tokens
        .iter()
        .find(|(_, _, src_line, src_col)| *src_line == 0 && *src_col == argument_start)
        .unwrap_or_else(|| {
            panic!(
                "no mapping points at the argument's own authored offset \
                 {argument_start}; the argument's authored offsets did not \
                 survive into the emitted map: {tokens:?}"
            )
        });
    assert!(
        generated_text_at(argument_token.0, argument_token.1).starts_with(":is(.a"),
        "the argument-start mapping must sit on the preserved argument bytes: {tokens:?}"
    );
    let second_run_token = tokens
        .iter()
        .find(|(_, _, src_line, src_col)| *src_line == 0 && *src_col == after_first_arm)
        .unwrap_or_else(|| {
            panic!(
                "no mapping points at the argument's post-`.a` authored offset \
                 {after_first_arm}; the per-arm insert did not split preserved \
                 argument bytes: {tokens:?}"
            )
        });
    assert!(
        generated_text_at(second_run_token.0, second_run_token.1).starts_with(", .b"),
        "the second-run mapping must sit on the preserved `, .b` bytes: {tokens:?}"
    );
}

#[test]
fn many_slotted_occurrences_share_one_emit_build_string() {
    reset_build_string_invocation_count();
    let many = (0..20)
        .map(|i| format!(":slotted(.slot-{i}) {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let code = scoped(&many, "sc1");
    // Every occurrence must be rewritten, not just the first: the build-count
    // assertion alone passes an implementation that scopes one occurrence and
    // drops the rest.
    assert_eq!(
        code.matches("[data-v-sc1-s]").count(),
        20,
        "every one of the 20 occurrences must carry its scope attribute: {code}"
    );
    assert_eq!(
        build_string_invocation_count(),
        1,
        "N :slotted() occurrences must not each mint a nested CodeTransform build"
    );
}

// @ai-generated - The multi-edit variant of the occurrence-count bound: every
// occurrence's argument produces TWO scope inserts (`:is()` with two arms), so
// a regression that minted a per-occurrence transform only when an argument
// carries more than one edit is caught here while the single-edit fixtures
// above stay green.
#[test]
fn many_multi_edit_slotted_occurrences_share_one_emit_build_string() {
    reset_build_string_invocation_count();
    let many = (0..20)
        .map(|i| format!(":slotted(:is(.a-{i}, .b-{i})) {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let code = scoped(&many, "sc1");
    assert!(
        code.contains(":is(.a-0[data-v-sc1-s], .b-0[data-v-sc1-s])"),
        "control: the fixture's arguments must really fan out to two edits: {code}"
    );
    // Every occurrence, not just the first: 20 occurrences x 2 arms.
    assert_eq!(
        code.matches("[data-v-sc1-s]").count(),
        40,
        "every arm of every one of the 20 occurrences must be scoped: {code}"
    );
    assert_eq!(
        build_string_invocation_count(),
        1,
        "N multi-edit :slotted() occurrences must not each mint a nested \
         CodeTransform build"
    );
}

// @ai-generated - Three-edit variant of the occurrence-count bound: every
// occurrence's argument produces THREE scope inserts (`:is()` with three
// arms — the exact rule shape the three-edit allocation canaries generate),
// so a regression gated on more than two argument edits is caught here while
// both the single-edit and two-edit fixtures above stay green.
#[test]
fn many_three_edit_slotted_occurrences_share_one_emit_build_string() {
    reset_build_string_invocation_count();
    let many = (0..20)
        .map(|i| format!(":slotted(:is(.a-{i}, .b-{i}, .c-{i})) {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let code = scoped(&many, "sc1");
    assert!(
        code.contains(":is(.a-0[data-v-sc1-s], .b-0[data-v-sc1-s], .c-0[data-v-sc1-s])"),
        "control: the fixture's arguments must really fan out to three edits: {code}"
    );
    // Every occurrence, not just the first: 20 occurrences x 3 arms.
    assert_eq!(
        code.matches("[data-v-sc1-s]").count(),
        60,
        "every arm of every one of the 20 occurrences must be scoped: {code}"
    );
    assert_eq!(
        build_string_invocation_count(),
        1,
        "N three-edit :slotted() occurrences must not each mint a nested \
         CodeTransform build"
    );
}

// @ai-generated - The edit-count family is UNBOUNDED: `:is()` takes any number
// of arms, so each fixture at N arms leaves a regression gated on `> N` free.
// Fixtures at 1, 2 and 3 arms closed three rungs of that ladder one at a time;
// this sweep closes the SWEPT RANGE in one assertion instead — it bounds the
// ladder at ARM_SWEEP_MAX rather than closing an unbounded family. `build_string`
// is the sensitive signal — a per-occurrence nested transform raises the count
// above 1 for the shape that trips it — so sweeping the arm count is what
// discriminates, and the byte canaries at 1/2/3 arms supply the magnitude.
// Residual, stated rather than left to be rediscovered: a regression gated
// above ARM_SWEEP_MAX arms is not caught here. That is inherent to
// example-based testing over an unbounded family, not an oversight.
#[test]
fn slotted_argument_edit_count_sweep_never_mints_a_nested_build() {
    const ARM_SWEEP_MAX: usize = 8;
    for arms in 1..=ARM_SWEEP_MAX {
        let selector = if arms == 1 {
            ".a0".to_string()
        } else {
            let list = (0..arms)
                .map(|i| format!(".a{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(":is({list})")
        };
        let source = format!(":slotted({selector}) {{ color: red; }}");
        reset_build_string_invocation_count();
        let code = scoped(&source, "sc1");
        assert_eq!(
            code.matches("[data-v-sc1-s]").count(),
            arms,
            "every one of the {arms} arm(s) must be scoped: {code}"
        );
        assert_eq!(
            build_string_invocation_count(),
            1,
            "a {arms}-arm :slotted() argument must not mint a nested build: {code}"
        );
    }
}

// ─── The Vue-owned cascade parses each content identity once ──────────────

// @ai-generated - compatible stages consume one `StyleSyntaxIr`; unchanged
// stages and authored-coordinate edit plans never force a second parse.
//
// Mutation recipe: re-parse the materialized bytes between the v-bind and
// scoping plans in `shared_vue_style_plan` — `parse_ir_invocation_count()`
// rises above 1 and the shared IR-identity assertions stop matching.
#[test]
fn style_pipeline_shares_parsed_ir_across_compatible_plans() {
    // 0 stages change (empty style block): a single initial parse, retained
    // through modules and scoping untouched.
    reset_parse_ir_invocation_count();
    reset_style_ir_stage_observations();
    let input = AuthoredStyleInput::new("", CssDialect::Css, "p.css", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);
    assert_eq!(outcome.code(), "");
    let observations = style_ir_stage_observations();
    assert_eq!(
        observations
            .iter()
            .map(|(stage, _)| *stage)
            .collect::<Vec<_>>(),
        vec![
            StyleRewriteStage::AuthoredVBind,
            StyleRewriteStage::PostPreprocessModules,
            StyleRewriteStage::PostPreprocessScoping,
        ],
        "all three stages must consume an IR"
    );
    assert!(
        observations
            .iter()
            .all(|(_, identity)| *identity == observations[0].1),
        "unchanged stages received different parsed IR values: {observations:?}"
    );
    assert!(
        !observations.windows(2).any(|pair| pair[0].1 != pair[1].1),
        "retained IR identity changed across an unchanged handoff: {observations:?}"
    );
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "0 changed stages must cost exactly 1 parse"
    );

    // Only the sole applicable stage changes (modules/scoping disabled: S=1).
    // Nothing follows it to force a re-parse.
    reset_parse_ir_invocation_count();
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind(c); }",
        CssDialect::Css,
        "p.css",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", false, false, true);
    assert!(outcome.facts.rewrites.v_bind);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "the only applicable stage changing costs exactly 1 parse"
    );

    // Only the LAST applicable stage changes (an element selector with no
    // v-bind, no class): the first two stages both hand their retained IR
    // forward unchanged; scoping's own change has no successor to force a re-parse
    // for, so this still costs exactly 1 parse — even though scoping IS the
    // stage that ends up rewriting.
    reset_parse_ir_invocation_count();
    let input = AuthoredStyleInput::new(
        "body { color: red; }",
        CssDialect::Css,
        "p.css",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);
    assert!(outcome.facts.rewrites.scoped_selector);
    assert!(!outcome.facts.rewrites.v_bind);
    assert!(!outcome.facts.rewrites.css_modules);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "only the last applicable stage changing costs exactly 1 parse"
    );

    // v-bind and scoping edit disjoint regions, so both plans retain authored
    // coordinates and consume one shared IR.
    reset_parse_ir_invocation_count();
    reset_style_ir_stage_observations();
    let input = AuthoredStyleInput::new(
        "body { color: v-bind(c); }",
        CssDialect::Css,
        "p.css",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);
    assert!(outcome.facts.rewrites.v_bind);
    assert!(!outcome.facts.rewrites.css_modules);
    assert!(outcome.facts.rewrites.scoped_selector);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "compatible v-bind and scoping plans must cost one parse"
    );
    let observations = style_ir_stage_observations();
    assert_eq!(observations.len(), 3, "{observations:?}");
    assert_eq!(
        observations[0].1, observations[1].1,
        "shared plans must consume the same parsed IR: {observations:?}"
    );
    assert_eq!(
        observations[1].1, observations[2].1,
        "all shared plans must consume the same parsed IR: {observations:?}"
    );
    // All three stages edit disjoint regions in this fixture and therefore
    // share one parsed identity and one terminal materialization.
    reset_parse_ir_invocation_count();
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind(c); }",
        CssDialect::Css,
        "p.css",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);
    assert!(outcome.facts.rewrites.v_bind);
    assert!(outcome.facts.rewrites.css_modules);
    assert!(outcome.facts.rewrites.scoped_selector);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "compatible three-stage planning must cost one parse"
    );
}

// @ai-generated - the cascade must express a stage-specific partial failure
// itself (no second, per-stage-independent orchestrator): a v-bind edit that
// already applied does not stop the modules stage from running against
// v-bind's rewritten bytes, and a hard modules-stage failure clears the
// final output and skips the scoped-selector stage entirely rather than
// running it against unsafe, already-cleared bytes.
//
// Mutation recipe: let the scoping plan run after the modules plan pushed a
// hard failure — a third `style_ir_stage_observations()` entry appears and the
// cleared-output assertion fails.
#[test]
fn style_pipeline_module_stage_failure_clears_output_and_skips_scoping() {
    reset_parse_ir_invocation_count();
    reset_style_ir_stage_observations();
    let input = AuthoredStyleInput::new(
        ".good { color: v-bind(c); } .bad { color red; }",
        CssDialect::Css,
        "p.css",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);

    assert!(
        outcome.facts.rewrites.v_bind,
        "the v-bind stage itself must still have rewritten .good's v-bind() call"
    );
    assert_eq!(
        outcome.stage_failures.len(),
        1,
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::PostPreprocessModules
    );
    assert_eq!(
        outcome.code(),
        "",
        "a hard modules-stage failure must clear the final output, matching \
         running each stage independently"
    );
    assert!(
        !outcome.facts.rewrites.scoped_selector,
        "the scoped-selector stage must be skipped once the output is \
         cleared by the modules stage's failure"
    );
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "a failed shared plan must not enter a staged fallback"
    );
    assert_eq!(
        style_ir_stage_observations(),
        vec![
            (StyleRewriteStage::AuthoredVBind, 1),
            (StyleRewriteStage::PostPreprocessModules, 1),
        ],
        "each attempted stage must plan once over the shared IR"
    );
}

/// A soft (per-selector) refusal in the scoping plan is reported against the
/// shared authored IR. It never restages the cascade over materialized bytes,
/// so neither planner runs twice and no second parse happens.
///
/// Mutation recipe: re-enter `shared_vue_style_plan` (or re-parse) once a
/// refusal is recorded — `parse_ir_invocation_count()` becomes 2 and the
/// stage-observation vector repeats a planner.
#[test]
fn scoped_soft_refusal_with_prior_edits_does_not_enter_a_staged_fallback() {
    reset_parse_ir_invocation_count();
    reset_style_ir_stage_observations();
    let source = ".good { color: v-bind(c); } .bad { color red; }";
    let outcome = run_vue_style_cascade(
        AuthoredStyleInput::new(source, CssDialect::Css, "p.css", "space:p", "artifact:p"),
        "sc1",
        false,
        true,
        true,
    );

    assert!(outcome.facts.rewrites.v_bind);
    assert_eq!(
        outcome.facts.refusals.len(),
        1,
        "{:?}",
        outcome.facts.refusals
    );
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "a soft refusal must remain in the shared authored-coordinate plan"
    );
    assert_eq!(
        style_ir_stage_observations(),
        vec![
            (StyleRewriteStage::AuthoredVBind, 1),
            (StyleRewriteStage::PostPreprocessScoping, 1),
        ],
        "a soft refusal must not repeat either planner"
    );
}

// @ai-generated - A hard v-bind-stage failure alone does not clear the
// output: the modules/scoped-selector stages below it still run against the
// original authored bytes, matching running each stage independently.
#[test]
fn style_pipeline_v_bind_stage_failure_does_not_clear_output() {
    let input = AuthoredStyleInput::new(
        ".a { color: v-bind(#{$x}); }",
        CssDialect::Scss,
        "p.scss",
        "space:p",
        "artifact:p",
    );
    let outcome = run_vue_style_cascade(input, "sc1", false, false, true);

    assert_eq!(
        outcome.stage_failures.len(),
        1,
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::AuthoredVBind
    );
    assert_eq!(
        outcome.code(),
        ".a { color: v-bind(#{$x}); }",
        "a v-bind-stage failure alone must not clear the accumulated output"
    );
}

#[test]
fn cascade_publishability_gates_post_preprocess_failure_but_not_authored_v_bind_failure() {
    let post_failure_source = ".good { color: v-bind(c); } .bad { color red; }";
    let post_failure = run_vue_style_cascade(
        AuthoredStyleInput::new(
            post_failure_source,
            CssDialect::Css,
            "p.css",
            "space:p",
            "artifact:p",
        ),
        "sc1",
        true,
        true,
        true,
    );
    assert!(
        !cascade_output_is_publishable(&post_failure, post_failure_source),
        "wiped output after a post-preprocessor failure must not be publishable"
    );
    assert_eq!(
        post_failure.code(),
        "",
        "the refusal fixture must wipe output"
    );

    let authored_failure_source = ".a { color: v-bind(#{$x}); }";
    let authored_failure = run_vue_style_cascade(
        AuthoredStyleInput::new(
            authored_failure_source,
            CssDialect::Scss,
            "p.scss",
            "space:p",
            "artifact:p",
        ),
        "sc1",
        false,
        false,
        true,
    );
    assert!(
        cascade_output_is_publishable(&authored_failure, authored_failure_source),
        "an authored-v-bind refusal is upstream of the supplied bytes"
    );
    assert_ne!(
        authored_failure.stage_failures[0].stage,
        StyleRewriteStage::PostPreprocessModules,
        "the non-gating control must not be a post-preprocessor failure"
    );
}

#[test]
fn cascade_passthrough_is_byte_identical_and_maps_after_non_bmp_character() {
    let source = "/* 😀x */\n.a { color: red; }";
    let outcome = run_vue_style_cascade(
        AuthoredStyleInput::new(
            source,
            CssDialect::Css,
            "emoji.css",
            "space:emoji",
            "artifact:emoji",
        ),
        "sc1",
        false,
        false,
        true,
    );
    assert_eq!(outcome.code().as_bytes(), source.as_bytes());

    let map_json = cascade_requested_source_map(&outcome, source, "emoji.css")
        .expect("byte-identical passthrough must have an identity map");
    assert!(!map_json.is_empty(), "identity map must not be empty");
    let map =
        oxc_sourcemap::OwnedSourceMap::from_json_string(&map_json).expect("valid identity map");
    let lookup = map.generate_lookup_table();
    let token = map
        .lookup_token(&lookup, 0, 5)
        .expect("the x after the emoji must resolve at UTF-16 column 5");
    assert_eq!((token.get_src_line(), token.get_src_col()), (0, 5));
    assert!(
        !map.get_tokens()
            .any(|token| token.get_dst_line() == 0 && token.get_dst_col() == 4),
        "an identity token at column 4 would split the emoji's UTF-16 surrogate pair"
    );
}

#[test]
fn cascade_requested_map_returns_the_single_rewrite_map() {
    let source = ".a { color: v-bind(theme); }";
    let outcome = run_vue_style_cascade(
        AuthoredStyleInput::new(
            source,
            CssDialect::Css,
            "single.css",
            "space:single",
            "artifact:single",
        ),
        "sc1",
        false,
        false,
        true,
    );
    let requested = cascade_requested_source_map(&outcome, source, "single.css")
        .expect("a single rewrite has an honest cascade map");
    assert_eq!(requested, outcome.source_map);
    assert!(
        outcome.code().contains("var(--sc1-theme)"),
        "{}",
        outcome.code()
    );
    assert!(
        !outcome.code().contains("v-bind("),
        "rewritten CSS must not retain the authored v-bind call"
    );
}

/// One terminal transform yields one exact authored-to-final map, so the
/// requested map is the outcome's own map and still names the authored source.
///
/// Mutation recipe: have `cascade_requested_source_map` return `None` for a
/// multi-stage rewrite (the pre-shared-planning behaviour) — `expect` panics.
#[test]
fn cascade_requested_map_covers_shared_multi_stage_rewrite() {
    let source = ".a { color: v-bind(theme); }";
    let outcome = run_vue_style_cascade(
        AuthoredStyleInput::new(
            source,
            CssDialect::Css,
            "multi.css",
            "space:multi",
            "artifact:multi",
        ),
        "sc1",
        true,
        true,
        true,
    );
    assert!(outcome.facts.rewrites.v_bind);
    assert!(outcome.facts.rewrites.css_modules);
    assert!(outcome.facts.rewrites.scoped_selector);
    assert!(
        outcome.code().contains("[data-v-sc1]"),
        "{}",
        outcome.code()
    );
    let requested = cascade_requested_source_map(&outcome, source, "multi.css")
        .expect("one terminal transform has an exact authored-to-final map");
    assert_eq!(requested, outcome.source_map);
    assert!(
        outcome.source_map.contains("multi.css"),
        "the terminal map must retain authored source provenance"
    );
}

/// Unwrapping `:global(...)` / `:deep(...)` moves the argument's bytes, and the
/// module-class rewrite inside that argument must travel with them: the
/// rewritten class still maps back to the authored `inner` column.
///
/// Mutation recipe: emit the unwrap as one whole-selector overwrite instead of
/// `collect_unwrapped_argument_edits`' per-region edits — the nested class
/// loses its own mapping and `lookup_token` lands on the selector start.
#[test]
fn shared_special_selector_rewrites_preserve_nested_class_anchors() {
    for source in [
        ".outer :global(.inner) { color: red; }",
        ".outer :deep(.inner) { color: red; }",
    ] {
        let outcome = run_vue_style_cascade(
            AuthoredStyleInput::new(
                source,
                CssDialect::Css,
                "special.css",
                "space:special",
                "artifact:special",
            ),
            "sc1",
            true,
            true,
            true,
        );
        let inner = outcome
            .facts
            .module_classes
            .iter()
            .find(|(name, _)| name == "inner")
            .map(|(_, hashed)| hashed)
            .expect("module plan retains the nested class");
        let generated_column = outcome
            .code()
            .find(inner)
            .expect("rewritten output retains the nested class")
            as u32;
        let authored_column = source.find("inner").expect("fixture has inner") as u32;
        let map = oxc_sourcemap::OwnedSourceMap::from_json_string(&outcome.source_map)
            .expect("valid shared cascade map");
        let lookup = map.generate_lookup_table();
        let token = map
            .lookup_token(&lookup, 0, generated_column)
            .expect("nested class has an authored anchor");

        assert_eq!(
            (token.get_src_line(), token.get_src_col()),
            (0, authored_column),
            "nested class mapped to the containing selector for {source}"
        );
    }
}

#[test]
fn cascade_requested_source_map_builds_a_real_identity_map_for_an_unchanged_outcome() {
    let source = ".x { color: red; }";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "Passthrough.css", "sp", "art");
    let outcome = run_vue_style_cascade(input, "probe1234", false, false, true);
    assert!(
        outcome.stage_failures.is_empty(),
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(
        outcome.code(),
        source,
        "an unmarked block must pass through byte-identical"
    );
    assert!(
        outcome.source_map.is_empty(),
        "the cascade itself must not fabricate a map for a stage it never ran"
    );

    let map_json = cascade_requested_source_map(&outcome, source, "Passthrough.css")
        .expect("an unchanged outcome with no stage failures must still yield a real map");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(&map_json)
        .expect("the identity map must be valid source-map JSON");
    assert_eq!(
        map.get_source_content(0),
        Some(source),
        "the identity map must retain the authored source"
    );
    let mut checked = 0;
    for column in 0..source.len() as u32 {
        let position = map
            .get_tokens()
            .filter(|token| token.get_dst_line() == 0 && token.get_dst_col() <= column)
            .max_by_key(|token| token.get_dst_col())
            .unwrap_or_else(|| panic!("no token covers generated column {column}"));
        assert_eq!(
            position.get_dst_col(),
            column,
            "column {column} resolved to a token at {}, not its own exact position — the map is \
             sparser than byte-accurate identity",
            position.get_dst_col()
        );
        assert_eq!(
            position.get_src_col(),
            column,
            "generated column {column} must map to the SAME authored column, not {}",
            position.get_src_col()
        );
        checked += 1;
    }
    assert_eq!(checked, source.len() as u32, "sanity: covered every byte");
}

#[test]
fn cascade_requested_source_map_builds_a_real_identity_map_across_multiple_lines() {
    let source = ".a {\n  color: red;\n}\n.b {\n  color: blue;\n}";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "Multiline.css", "sp", "art");
    let outcome = run_vue_style_cascade(input, "probe1234", false, false, true);
    assert!(
        outcome.stage_failures.is_empty(),
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(outcome.code(), source);

    let map_json = cascade_requested_source_map(&outcome, source, "Multiline.css").expect(
        "an unchanged multi-line outcome with no stage failures must still yield a real map",
    );
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(&map_json)
        .expect("the identity map must be valid source-map JSON");

    let mut expected: Vec<(u32, u32)> = Vec::with_capacity(source.len());
    let mut line = 0u32;
    let mut column = 0u32;
    for ch in source.chars() {
        expected.push((line, column));
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += ch.len_utf16() as u32;
        }
    }
    assert!(
        line > 0,
        "sanity: the fixture must actually span multiple lines"
    );

    let mut checked = 0;
    for (dst_line, dst_col) in expected {
        let token = map
            .get_tokens()
            .filter(|t| t.get_dst_line() == dst_line && t.get_dst_col() <= dst_col)
            .max_by_key(|t| t.get_dst_col())
            .unwrap_or_else(|| panic!("no token covers generated ({dst_line}, {dst_col})"));
        assert_eq!(
            (token.get_dst_line(), token.get_dst_col()),
            (dst_line, dst_col),
            "generated ({dst_line}, {dst_col}) resolved to a token at ({}, {}), not its own \
             exact position",
            token.get_dst_line(),
            token.get_dst_col()
        );
        assert_eq!(
            (token.get_src_line(), token.get_src_col()),
            (dst_line, dst_col),
            "generated ({dst_line}, {dst_col}) must map to the SAME authored (line, column), \
             got ({}, {})",
            token.get_src_line(),
            token.get_src_col()
        );
        checked += 1;
    }
    assert_eq!(
        checked,
        source.chars().count(),
        "sanity: covered every character"
    );
}

#[test]
fn cascade_requested_source_map_passes_through_a_real_rewrite_map_unchanged() {
    let source = ".x { color: red; }";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "Transformed.css", "sp", "art");
    let outcome = run_vue_style_cascade(input, "probe1234", false, true, true);
    assert!(
        outcome.stage_failures.is_empty(),
        "{:?}",
        outcome.stage_failures
    );
    assert_ne!(outcome.code(), source, "scoping must rewrite the selector");
    assert!(
        !outcome.source_map.is_empty(),
        "a rewriting stage must already produce a real map via emit()"
    );

    let map_json = cascade_requested_source_map(&outcome, source, "Transformed.css")
        .expect("a rewritten outcome must yield its own map");
    assert_eq!(
        map_json, outcome.source_map,
        "cascade_requested_source_map must not alter a map a stage actually produced"
    );
}

#[test]
fn cascade_requested_source_map_is_none_after_a_hard_stage_failure_clears_output() {
    let source = ".x[";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "Broken.css", "sp", "art");
    let outcome = run_vue_style_cascade(input, "probe1234", true, false, true);
    assert!(
        !outcome.stage_failures.is_empty(),
        "malformed module input must hard-fail a stage"
    );
    assert_eq!(
        outcome.code(),
        "",
        "a hard stage failure must clear the output"
    );
    assert_eq!(
        cascade_requested_source_map(&outcome, source, "Broken.css"),
        None,
        "no identity map may be published over output the cascade discarded"
    );
}

#[test]
fn cascade_output_is_publishable_refuses_a_hard_failure_that_wiped_non_empty_content() {
    let source = ".good { color: v-bind(c); } .bad { color red; }";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "p.css", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", true, true, true);
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::PostPreprocessModules
    );
    assert_eq!(
        outcome.code(),
        "",
        "sanity: the modules stage must have cleared the output"
    );
    assert!(
        !cascade_output_is_publishable(&outcome, source),
        "a hard stage failure that wiped non-empty authored content must not be publishable"
    );
}

/// A non-CSS `<style module>`/`<style scoped>` request refuses at
/// `PlainCssInput` — it never reparses the authored dialect's bytes as CSS.
///
/// Mutation recipe: drop the `PlainCssInput` dialect gate in the shared plan —
/// `last_parse_ir_dialect()` becomes `Css` and the parse count rises to 2.
#[test]
fn cascade_output_is_publishable_refuses_stage_requires_plain_css() {
    reset_parse_ir_invocation_count();
    reset_last_parse_ir_dialect();
    let source = ".a { color: red; }";
    let input =
        AuthoredStyleInput::new(source, CssDialect::Scss, "p.scss", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", false, true, true);
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::StageRequiresPlainCss
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::PostPreprocessScoping
    );
    assert_eq!(outcome.code(), "");
    assert!(!cascade_output_is_publishable(&outcome, source));
    assert_eq!(
        last_parse_ir_dialect(),
        Some(CssDialect::Scss),
        "non-CSS module/scoped must refuse at PlainCssInput without a CSS parse"
    );
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "the authored dialect parse is the only parse"
    );
}

#[test]
fn cascade_output_is_publishable_accepts_an_indented_layout_mutation_refusal() {
    let source = ".a\n  color: rgba(v-bind(\n    tone\n  ), 1)\n";
    let input =
        AuthoredStyleInput::new(source, CssDialect::Sass, "p.sass", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", false, false, true);
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::IndentedLayoutMutation
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::AuthoredVBind
    );
    assert_eq!(
        outcome.code(),
        source,
        "sanity: an authored-v-bind-stage failure must not clear the output"
    );
    assert!(cascade_output_is_publishable(&outcome, source));
}

/// Publication reads the refusal, not the failing stage's identity: a parse
/// miss recorded as `AuthoredVBind` still blocks publication of wiped bytes.
///
/// Mutation recipe: make `cascade_output_is_publishable` ignore
/// `AuthoredVBind` stage failures (or read `code().is_empty()` instead of
/// `is_refused()`) — the wiped output becomes publishable.
#[test]
fn cascade_output_is_publishable_refuses_a_parse_failure_that_wiped_non_empty_content() {
    let source = ".a { color: red; }";
    let outcome = VueStyleCascadeOutcome {
        // The shape the runner really produces when a parse miss wipes the
        // output: a refusal whose only identity is the parse that ran.
        result: verter_css_syntax::QualifiedStyleResult::refused(
            verter_css_syntax::StyleStage::Authored,
            CssDialect::Css,
            Vec::new(),
        ),
        source_map: String::new(),
        facts: crate::style_planner::VueStyleFacts::default(),
        stage_failures: vec![StyleRewriteFailure {
            class: StyleRewriteFailureClass::ParseFailure,
            stage: StyleRewriteStage::AuthoredVBind,
            dialect: CssDialect::Css,
            span: None,
        }],
    };
    assert!(
        !cascade_output_is_publishable(&outcome, source),
        "a parse miss that wiped non-empty authored content must not be \
         publishable, even when the only recorded identity is AuthoredVBind"
    );
}

/// A parse miss is recorded once, by the parse that ran, whichever stages
/// were requested — consumers see one diagnostic, never a per-stage clone.
///
/// What it does to the OUTPUT depends on the request. `module`/`scoped` change
/// what the block means — unhashed class names, or selectors that would apply
/// document-wide instead of to this component — so unrewritten authored bytes
/// are actively wrong and the output is cleared. With neither attribute the
/// only work was `v-bind()` lowering, nothing was rewritten, and deleting the
/// author's CSS buys nothing: the bytes publish beside the diagnostic.
///
/// Mutation recipe: have each requested stage push its own `ParseFailure`
/// (the per-stage-parse shape) — `stage_failures.len()` becomes 2 or 3 and the
/// diagnostic count follows. Return `ClearedByRefusal` unconditionally from
/// the parse-miss arm and the plain-`<style>` leg loses the authored bytes.
#[test]
fn cascade_parse_failure_is_recorded_once_and_clears_only_a_rewritten_request() {
    let source = nesting_overflow_css();
    for (module, scoped) in [(true, true), (true, false), (false, true), (false, false)] {
        let outcome = run_vue_style_cascade(
            AuthoredStyleInput::new(&source, CssDialect::Css, "p.css", "space:p", "artifact:p"),
            "sc1",
            module,
            scoped,
            true,
        );
        assert_eq!(
            outcome.stage_failures.len(),
            1,
            "module={module} scoped={scoped}: {:?}",
            outcome.stage_failures
        );
        assert_eq!(
            outcome.stage_failures[0].class,
            StyleRewriteFailureClass::ParseFailure
        );
        assert_eq!(
            outcome.stage_failures[0].stage,
            StyleRewriteStage::AuthoredVBind,
            "a parse miss must keep the identity of the parse that ran"
        );
        assert_eq!(
            outcome.result.diagnostics().len(),
            1,
            "consumers must see one parse diagnostic, not a restaged clone"
        );

        if module || scoped {
            assert!(
                outcome.result.is_refused(),
                "module={module} scoped={scoped}"
            );
            assert_eq!(outcome.code(), "");
            assert!(!cascade_output_is_publishable(&outcome, &source));
        } else {
            assert!(
                !outcome.result.is_refused(),
                "a plain <style> block has no rewrite to be unsafe about"
            );
            assert_eq!(
                outcome.code(),
                source,
                "unparseable plain CSS publishes verbatim beside its diagnostic"
            );
            assert!(cascade_output_is_publishable(&outcome, &source));
        }
    }
}

/// `RuntimeStyleProcessing::AuthoredOnly` and a `Complete` request with
/// neither `module` nor `scoped` are the same request — `v-bind()` lowering
/// and nothing else — so the bundler entry point owes a plain block's parse
/// miss the plain block's answer: one recorded miss, no refusal, and the
/// authored bytes published beside the diagnostic.
///
/// Asserted as ABSOLUTE outcomes, not against what `run_vue_style_cascade`
/// answers for the same input. `run_vue_style_authored_only` delegates to it,
/// so an equality between the two compares one function's output to itself and
/// holds under every mutation — including the one that breaks both routes at
/// once. The cascade's own leg of this contract is
/// `cascade_parse_failure_is_recorded_once_and_clears_only_a_rewritten_request`;
/// what is left to pin here is that the second entry point still reaches it.
///
/// Mutation recipe: give `run_vue_style_authored_only` a route of its own that
/// clears the output on a parse miss (return
/// `VueStyleCascadeOutcome` from `run_vue_style_cascade(input, scope_id, false, true, want_source_map)`,
/// which is the `<style scoped>` answer) — the bundler entry point loses the
/// author's CSS and every assertion below reports it.
#[test]
fn the_authored_only_entry_point_publishes_a_plain_blocks_parse_miss() {
    let source = nesting_overflow_css();
    let outcome = run_vue_style_authored_only(
        AuthoredStyleInput::new(&source, CssDialect::Css, "p.css", "space:p", "artifact:p"),
        "sc1",
        false,
    );

    assert_eq!(
        outcome.stage_failures.len(),
        1,
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::ParseFailure
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::AuthoredVBind,
        "a parse miss must keep the identity of the parse that ran"
    );
    assert_eq!(
        outcome.result.diagnostics().len(),
        1,
        "consumers must see one parse diagnostic, not a restaged clone"
    );
    assert!(
        !outcome.result.is_refused(),
        "a v-bind-only request has no rewrite to be unsafe about"
    );
    assert_eq!(
        outcome.code(),
        source,
        "unparseable bytes publish verbatim beside the diagnostic"
    );
    assert!(cascade_output_is_publishable(&outcome, &source));
}

#[test]
fn cascade_output_is_publishable_accepts_an_overlapping_edits_refusal_on_v_bind_stage() {
    let source = ".a { color: v-bind(c); }";
    let outcome = VueStyleCascadeOutcome {
        result: verter_css_syntax::QualifiedStyleResult::authored(
            CssDialect::Css,
            source,
            Vec::new(),
        ),
        source_map: String::new(),
        facts: crate::style_planner::VueStyleFacts::default(),
        stage_failures: vec![StyleRewriteFailure {
            class: StyleRewriteFailureClass::OverlappingEdits,
            stage: StyleRewriteStage::AuthoredVBind,
            dialect: CssDialect::Css,
            span: None,
        }],
    };
    assert!(
        cascade_output_is_publishable(&outcome, source),
        "an authored-v-bind-stage OverlappingEdits refusal must never gate \
         publication, same as any other v-bind-stage-only failure"
    );
}

#[test]
fn cascade_output_is_publishable_accepts_a_v_bind_only_failure() {
    let source = ".a { color: v-bind(#{$x}); }";
    let input =
        AuthoredStyleInput::new(source, CssDialect::Scss, "p.scss", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", false, false, true);
    assert_eq!(
        outcome.stage_failures.len(),
        1,
        "{:?}",
        outcome.stage_failures
    );
    assert_eq!(
        outcome.stage_failures[0].class,
        StyleRewriteFailureClass::UntrustedRewriteTarget
    );
    assert_eq!(
        outcome.stage_failures[0].stage,
        StyleRewriteStage::AuthoredVBind
    );
    assert_eq!(
        outcome.code(),
        source,
        "sanity: the v-bind failure must not clear the output"
    );
    assert!(
        cascade_output_is_publishable(&outcome, source),
        "a v-bind-only failure that left the output intact must stay publishable"
    );
}

#[test]
fn cascade_output_is_publishable_accepts_a_clean_outcome() {
    let source = ".x { color: red; }";
    let input = AuthoredStyleInput::new(source, CssDialect::Css, "p.css", "space:p", "artifact:p");
    let outcome = run_vue_style_cascade(input, "sc1", false, true, true);
    assert!(
        outcome.stage_failures.is_empty(),
        "{:?}",
        outcome.stage_failures
    );
    assert!(cascade_output_is_publishable(&outcome, source));
}

// @ai-generated - one parse per content identity must hold through the REAL
// production compile() entry point, not just the standalone
// `run_vue_style_cascade` orchestrator: a `<style scoped module>` block costs
// exactly 1 parse end-to-end, unconditionally — not "when only the last stage
// rewrites". Every compatible stage plans over the same IR in authored
// coordinates, so how many of them rewrite bytes cannot change the count.
// Calling `transform_vue_v_bind`/`transform_vue_css_modules`/
// `transform_vue_scoped_css` independently re-parses the same content identity
// per stage — 3 parses for this fixture, not 1.
#[test]
fn production_compile_reuses_parsed_style_ir_across_cascade_stages() {
    reset_parse_ir_invocation_count();
    let result = compile_style("<style scoped module>\nbody { color: red; }\n</style>");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "the real compile() entry point must plan the v-bind/module/scoped \
         stages over one parse of the block, not re-parse per stage"
    );

    // The same fixture with every stage rewriting: still one parse. A count
    // that tracked "stages that changed bytes" would read 2 or 3 here.
    reset_parse_ir_invocation_count();
    let rewriting = compile_style(
        "<style scoped module>\n.card { color: v-bind(tone); }\n:deep(.child) { color: red; }\n</style>",
    );
    assert!(rewriting.errors.is_empty(), "{:?}", rewriting.errors);
    assert_eq!(
        parse_ir_invocation_count(),
        1,
        "a block where every requested stage rewrites bytes must still cost \
         one parse"
    );
}

// @ai-generated - A10a's dialect-unconditional CSS-Modules class analysis
// must be wired into the real production compile() entry point, not just
// exist as a standalone function: an SCSS `<style module>` block's `$style`
// classes must be analyzed end to end even though the byte-level class-name
// *rewrite* stays CSS-only (row 19, `css/modules.rs`, untouched — the
// emitted code still contains the unrewritten authored class name).
#[test]
fn production_compile_analyzes_module_classes_for_scss_dialect() {
    let result = compile_style("<style lang=\"scss\" module>\n.active { color: red; }\n</style>");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let style = result.styles.first().expect("one style block");
    assert_eq!(
        style.module_classes.len(),
        1,
        "SCSS module block must analyze its authored classes: {:?}",
        style.module_classes
    );
    assert_eq!(style.module_classes[0].0, "active");
    assert_ne!(
        style.module_classes[0].1, "active",
        "hashed class name must not pass the authored name through unhashed"
    );
    assert!(
        style.code.contains(".active"),
        "the byte-level rewrite stays CSS-only; SCSS output is left for \
         external preprocessing: {}",
        style.code
    );
}

/// Every construct family the shared generator corpus produces, plus the
/// overlap-prone and nested shapes it does not, run through the shared plan and
/// through the stages one after the other, must land on the same bytes and the
/// same refusal answer.
///
/// One terminal transform is equivalent to a staged pipeline only while no
/// stage's REPLACEMENT bytes contain something a later stage would have acted
/// on — and nothing structurally enforces that. It holds today because
/// `v-bind(x)` becomes a `var()` function token the animation scanner ignores,
/// and because the CSS-Modules planner never enters a `@keyframes` prelude or
/// body the scoping planner owns. Change either replacement and the two models
/// diverge on a construct that may have no point fixture. The merge answers
/// only the SAME-SPAN half of that divergence: two stages emitting coincident
/// non-empty overwrites refuse, so the shared plan reports a refusal where the
/// staged pipeline produced valid output. The DIFFERENT-SPAN half is likelier
/// and worse — rename an `@keyframes` name in one stage and the
/// `animation-name:` rewrite that answers it sits in another span entirely, so
/// the merge sees no intersection, refuses nothing, and the shared plan emits
/// an animation reference to a name that no longer exists. That is
/// wrong-complete output, not a refusal, and no merge rule can see it.
/// Comparing the models across the corpus is what makes either half a test
/// failure instead of a production report. Any change to a stage's replacement
/// vocabulary must extend this test in the same change: the `shared_families
/// == 11` pin detects a corpus-size change, not new planner behavior omitted
/// from that corpus.
///
/// The construct families come from the shared generator corpus the allocation
/// canaries measure, not a second hand-written list, so a family added on one
/// side cannot go unswept on the other. Read what that corpus actually
/// enumerates: TOP-LEVEL rules only — classes, descendants, pseudos, selector
/// lists, `v-bind()` plain and dotted, `:deep`, `:slotted`, `:global`, the mixed
/// set, and repeated classes. It carries no nested-rule family, and it cannot
/// grow one cheaply: its category list is also the allocation ceiling's
/// universe, whose per-category counts are recaptured legacy measurements. The
/// fixtures below it therefore carry the shapes those generators do not
/// produce — an earlier stage's replacement bytes that a later stage would
/// otherwise have targeted, and a class rule nested inside a directive body,
/// which is where both planners recurse and where the corpus never goes.
///
/// Mutation recipe: in `merge_shared_stage_edits`, drop the
/// `keep_later[later_index] = false` assignment in the strictly-contained
/// overwrite arm (so a later edit inside an earlier overwrite is retained
/// rather than discarded) — the v-bind-bearing categories stop matching the
/// staged chain. For the non-vacuity legs, make `parse_ir` hand the CSS parser
/// an empty source: both models then plan nothing and answer passthrough on
/// every fixture, so the equality legs still agree and only the non-vacuity
/// legs report it.
#[test]
fn a_shared_plan_matches_running_the_stages_one_after_the_other() {
    const SCOPE: &str = "a4f2eed6";

    fn authored(code: &str) -> AuthoredStyleInput<'_> {
        AuthoredStyleInput::new(
            code,
            CssDialect::Css,
            "probe.style",
            "space:probe",
            "artifact:probe",
        )
    }

    fn plain(code: &str) -> PlainCssInput<'_> {
        PlainCssInput::try_new(
            code,
            CssDialect::Css,
            "probe.style",
            "space:probe",
            "artifact:probe",
        )
        .expect("plain css")
    }

    fn shared(code: &str, module: bool, scoped: bool) -> Option<String> {
        let outcome = run_vue_style_cascade(authored(code), SCOPE, module, scoped, false);
        (!outcome.result.is_refused()).then(|| outcome.code().to_string())
    }

    fn apply_stage(
        code: &str,
        run: impl FnOnce(&str) -> Result<StyleRewriteOutcome, StyleRewriteFailure>,
    ) -> Option<String> {
        match run(code).ok()? {
            StyleRewriteOutcome::Rewritten { code, .. } => Some(code),
            StyleRewriteOutcome::Unchanged { .. } => Some(code.to_string()),
        }
    }

    fn staged(code: &str, module: bool, scoped: bool) -> Option<String> {
        let mut current = apply_stage(code, |code| transform_vue_v_bind(authored(code), SCOPE))?;
        if module {
            current = apply_stage(&current, |code| {
                transform_vue_css_modules(plain(code), SCOPE)
            })?;
        }
        if scoped {
            current = apply_stage(&current, |code| {
                transform_vue_scoped_css(plain(code), SCOPE)
            })?;
        }
        Some(current)
    }

    // The construct families the allocation canaries already generate, read
    // from the one shared corpus so a family added there cannot silently go
    // unswept here.
    let mut corpus: Vec<(String, String)> = style_planner_gen::all_categories()
        .into_iter()
        .map(|(name, css)| (name.to_string(), css))
        .collect();
    let shared_families = corpus.len();
    let mut push = |name: &str, css: String| corpus.push((name.to_string(), css));

    // The overlap-prone shapes: a class whose animation name the scoping stage
    // renames while the modules stage hashes the class beside it, and a
    // `v-bind()` whose replacement sits in the same declaration block as an
    // animation reference.
    push(
        "keyframes_and_animation",
        "@keyframes spin { from { transform: rotate(0); } to { transform: rotate(1turn); } }\n\
         .spinner { animation: spin 1s linear infinite; }\n"
            .to_string(),
    );
    push(
        "v_bind_beside_animation",
        "@keyframes fade { from { opacity: 0; } to { opacity: 1; } }\n\
         .card { color: v-bind(tone); animation: fade 1s; }\n"
            .to_string(),
    );
    // The one shape where a later stage genuinely targets bytes an earlier
    // stage replaced: the scoping planner renames `pulse` in
    // `animation-name: pulse`, and that ident sits inside the span the v-bind
    // planner overwrites with a `var()` call.
    push(
        "later_rewrite_inside_a_v_bind_replacement",
        "@keyframes pulse { to { opacity: 0; } }\n\
         .item { animation-name: v-bind(pulse); }\n"
            .to_string(),
    );
    push(
        "later_rewrite_inside_an_animation_shorthand_v_bind",
        "@keyframes pulse { to { opacity: 0; } }\n\
         .item { animation: v-bind(pulse) 1s linear; }\n"
            .to_string(),
    );
    // Both planners recurse through directive bodies — `collect_module_statements`
    // to hash a nested class, `VueScopePlanner::plan_statements` to scope the
    // selector beside it — and the shared corpus generates only top-level rules,
    // so nesting is swept here or nowhere.
    push(
        "at_rule_nested_class_and_v_bind",
        "@media (min-width: 40rem) {\n\
         .card { color: v-bind(tone); }\n\
         .card .title { padding: 1px; }\n\
         }\n"
        .to_string(),
    );
    push(
        "deep_with_v_bind_body",
        ":deep(.inner) { color: v-bind(tone); }\n.outer { padding: 1px; }\n".to_string(),
    );
    push(
        "global_with_v_bind_body",
        ":global(.reset) { color: v-bind(tone); }\n".to_string(),
    );

    for (name, css) in &corpus {
        for (module, scoped) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                shared(css, module, scoped),
                staged(css, module, scoped),
                "{name} (module={module} scoped={scoped}): the shared plan and \
                 the staged pipeline must answer identically"
            );
        }
    }

    // An equality over two `Option<String>`s passes vacuously when both models
    // refuse, and the overlap-prone fixtures are exactly the shapes this sweep
    // exists for — a vacuous leg there would retire the evidence without
    // retiring the test. Each is valid CSS carrying a class selector or a
    // `v-bind()`, so the full request must resolve AND rewrite.
    for (name, css) in &corpus[shared_families..] {
        let rewritten = shared(css, true, true)
            .unwrap_or_else(|| panic!("{name}: the overlap-prone fixture must not refuse"));
        assert_ne!(
            &rewritten, css,
            "{name}: the overlap-prone fixture must exercise a real rewrite, \
             not compare two passthroughs"
        );
    }

    // The sweep is only evidence if the two models can actually disagree, so
    // pin that at least one family exercises a real multi-stage rewrite rather
    // than passing through unchanged on every leg.
    let (_, v_bind_css) = corpus
        .iter()
        .find(|(name, _)| name == "v_bind_rules")
        .expect("the shared corpus must still name the v-bind family");
    let rewritten = shared(v_bind_css, true, true).expect("v_bind_rules must not refuse");
    assert_ne!(
        &rewritten, v_bind_css,
        "sanity: the corpus must contain a case both models actually rewrite"
    );
    // The count is pinned to a literal rather than re-read from
    // `all_categories`, which would compare the corpus against itself and hold
    // however many families were dropped. Resizing the shared corpus must be
    // looked at here, because this sweep is what makes "every construct family
    // the planners rewrite" a checked claim rather than a comment.
    assert_eq!(
        shared_families, 11,
        "the shared construct-family corpus changed size; re-read what this equivalence sweep now claims to cover"
    );
    assert!(
        corpus.len() > shared_families,
        "the overlap-prone fixtures must be swept beside the shared families"
    );
}
