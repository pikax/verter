/**
 * @ai-generated - IDE `<style module>` class-name completion drives
 * `StyleSyntaxIr::complete_static_classes()` via
 * `style_planner::complete_static_class_names`. Fixture set ported 1:1 from
 * `css/mod.rs` inline `extract_*` tests so completion-set parity is proven.
 */
use verter_compiler::compile::{VueExecutionInputs, VueMacroSemanticInput};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest, ProductKind,
    VueCompileRequest,
};
use verter_compiler::standalone::{DirectExecutionInputs, StandaloneCompiler};
use verter_compiler::style_planner::complete_static_class_names;
use verter_css_syntax::CssDialect;

#[test]
fn extract_basic_classes() {
    let css = ".btn { color: red; } .card { padding: 1rem; }";
    let classes = complete_static_class_names(css, CssDialect::Css);
    assert_eq!(classes, vec!["btn", "card"]);
}

#[test]
fn extract_classes_with_media_query() {
    let css =
        ".mobile { display: block; } @media (min-width: 768px) { .desktop { display: block; } }";
    let classes = complete_static_class_names(css, CssDialect::Css);
    assert!(classes.contains(&"mobile".to_string()));
    assert!(classes.contains(&"desktop".to_string()));
}

#[test]
fn extract_deduplicates() {
    let css = ".btn { color: red; } .btn:hover { color: blue; }";
    let classes = complete_static_class_names(css, CssDialect::Css);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0], "btn");
}

#[test]
fn extract_kebab_case_classes() {
    let css = ".card-title { font-weight: bold; } .card-body { padding: 1rem; }";
    let classes = complete_static_class_names(css, CssDialect::Css);
    assert_eq!(classes, vec!["card-title", "card-body"]);
}

#[test]
fn extract_empty_css() {
    let classes = complete_static_class_names("", CssDialect::Css);
    assert!(classes.is_empty());
}

#[test]
fn extract_no_classes() {
    let css = "div { color: red; } p { margin: 0; }";
    let classes = complete_static_class_names(css, CssDialect::Css);
    assert!(classes.is_empty());
}

/// An SCSS `<style module>` block's classes are analyzed even though
/// the byte-level rewrite stays CSS-only (untouched) — the
/// dialect-generic parse still enumerates static class selectors correctly.
#[test]
fn extract_classes_from_scss_dialect() {
    let css = ".active { color: red; }\n.nested { .child { color: blue; } }";
    let classes = complete_static_class_names(css, CssDialect::Scss);
    assert!(classes.contains(&"active".to_string()));
}

/// NEGATIVE: `complete_static_class_names` degrades to an empty list on a
/// parse it cannot recover, rather than propagating an error — matching the
/// retired scanner's total (never-erroring) behavior. `[` alone is not
/// meaningfully "recoverable" CSS; this must not panic.
#[test]
fn malformed_input_degrades_to_empty_rather_than_panicking() {
    let classes = complete_static_class_names(".", CssDialect::Css);
    assert!(classes.is_empty());
}

fn compile_ide_style_module(lang: &str, style: &str, referenced_class: &str) -> String {
    let source = format!(
        r#"<script setup lang="ts">
const label = "module"
</script>
<template><div :class="$style['{referenced_class}']">{{ label }}</div></template>
<style module lang="{lang}">
{style}</style>
"#
    );
    let request = CompileRequest::new(
        vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some(format!("{lang}.vue")),
        None,
        false,
        false,
    )
    .expect("a lone Vue IDE companion request must construct");
    let execution = VueExecutionInputs::default();
    let macros = VueMacroSemanticInput::Unavailable;
    let output = StandaloneCompiler
        .compile(
            &source,
            &request,
            DirectExecutionInputs::Vue {
                execution: &execution,
                macros: &macros,
            },
        )
        .unwrap_or_else(|error| panic!("{lang} IDE compile must succeed: {error:?}"));

    output
        .artifacts
        .artifact(ProductKind::IdeCompanion)
        .expect("the requested IDE companion must be published")
        .code()
        .to_string()
}

#[test]
fn compiler_extracts_css_module_classes_for_every_dialect_including_unrecognised() {
    let cases = [
        (
            "css",
            ".css-only { background: url(icon.css_decoy); }\n",
            "css-only",
            "css_decoy",
        ),
        (
            "scss",
            "$asset: url(icon.scss_decoy);\n.scss-only { background: $asset; }\n",
            "scss-only",
            "scss_decoy",
        ),
        (
            "sass",
            ".sass-only\n  background: url(icon.sass_decoy)\n",
            "sass-only",
            "sass_decoy",
        ),
        (
            "less",
            "@asset: url(icon.less_decoy);\n.less-only { background: @asset; }\n",
            "less-only",
            "less_decoy",
        ),
        (
            "stylus",
            ".stylus-only\n  background url(icon.stylus_decoy)\n",
            "stylus-only",
            "stylus_decoy",
        ),
        (
            "postcss",
            ".unknown-lang-only { background: url(icon.unknown_decoy); }\n",
            "unknown-lang-only",
            "unknown_decoy",
        ),
    ];

    for (lang, style, expected, decoy) in cases {
        let code = compile_ide_style_module(lang, style, expected);
        assert!(
            code.contains(&format!(r#""{expected}": string"#)),
            "{lang} must publish the real module class in the IDE $style type:\n{code}"
        );
        assert!(
            !code.contains(&format!(r#""{decoy}": string"#)),
            "{lang} must not treat a declaration value as a class selector:\n{code}"
        );
    }
}
