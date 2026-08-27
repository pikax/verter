//! Private three-way style boundary: prepare / transform / analyze.
//! These helpers are Rust-only and must not be `#[napi]` exports.

use napi::bindgen_prelude::Buffer;
use verter_napi::{
    analyze_style, prepare_style_for_preprocessor, process_style, transform_vue_style,
    AnalyzeStyleOptions, PrepareStyleForPreprocessorOptions, ProcessStyleOptions,
    ProcessStyleResult, TransformVueStyleOptions,
};

#[test]
fn prepare_style_for_preprocessor_rewrites_v_bind_and_keeps_authored_scss() {
    let css = "$tone: red; .card { color: v-bind(primary); }";
    let result = prepare_style_for_preprocessor(
        css,
        PrepareStyleForPreprocessorOptions {
            scope_id: "a4f2eed6".to_string(),
            dialect: Some("scss".to_string()),
            filename: Some("Card.scss".to_string()),
        },
    )
    .expect("trusted authored scss rewrites v-bind");

    assert!(
        result.code.contains("$tone: red;"),
        "authored scss must stay: {}",
        result.code
    );
    assert!(
        result.code.contains("var(--a4f2eed6-primary)"),
        "v-bind must lower to a scope var: {}",
        result.code
    );
    assert!(
        !result.code.contains("v-bind("),
        "v-bind call must not survive: {}",
        result.code
    );
    assert_eq!(result.v_bind_vars.len(), 1);
    assert_eq!(result.v_bind_vars[0].expression, "primary");
    assert_eq!(result.v_bind_vars[0].var_name, "--a4f2eed6-primary");
}

#[test]
fn prepare_style_for_preprocessor_passthrough_is_byte_identical() {
    let css = "$tone: red; .card { color: $tone; }";
    let result = prepare_style_for_preprocessor(
        css,
        PrepareStyleForPreprocessorOptions {
            scope_id: "a4f2eed6".to_string(),
            dialect: Some("scss".to_string()),
            filename: None,
        },
    )
    .expect("trusted passthrough");
    assert_eq!(result.code, css);
    assert!(result.v_bind_vars.is_empty());
}

#[test]
fn transform_vue_style_scopes_plain_css() {
    let css = ".x { color: red; }";
    let result = transform_vue_style(
        css,
        TransformVueStyleOptions {
            scope_id: "probe1234".to_string(),
            scoped: Some(true),
            is_module: Some(false),
            module_name: None,
            filename: Some("x.css".to_string()),
            sourcemap: Some(false),
        },
    )
    .expect("trusted plain css scopes");
    assert!(
        result.code.contains("[data-v-probe1234]"),
        "scoped rewrite missing: {}",
        result.code
    );
    assert_ne!(result.code, css);
    assert!(result.source_map.is_none());
    assert!(result.v_bind_vars.is_empty());
    assert!(result.module_classes.is_empty());
    assert!(result.module_name.is_none());
}

#[test]
fn transform_vue_style_rewrites_v_bind_on_plain_css() {
    let css = ".x { color: v-bind(tone); }";
    let result = transform_vue_style(
        css,
        TransformVueStyleOptions {
            scope_id: "probe1234".to_string(),
            scoped: Some(false),
            is_module: Some(false),
            module_name: None,
            filename: Some("x.css".to_string()),
            sourcemap: Some(false),
        },
    )
    .expect("trusted plain css rewrites v-bind");
    assert!(
        result.code.contains("var(--probe1234-tone)"),
        "v-bind must lower: {}",
        result.code
    );
    assert!(
        !result.code.contains("v-bind("),
        "v-bind call must not survive: {}",
        result.code
    );
    assert_eq!(result.v_bind_vars.len(), 1);
    assert_eq!(result.v_bind_vars[0].expression, "tone");
    assert_eq!(result.v_bind_vars[0].var_name, "--probe1234-tone");
}

#[test]
fn transform_vue_style_passthrough_is_byte_identical() {
    let css = ".x { color: red; }";
    let result = transform_vue_style(
        css,
        TransformVueStyleOptions {
            scope_id: "probe1234".to_string(),
            scoped: Some(false),
            is_module: Some(false),
            module_name: None,
            filename: Some("x.css".to_string()),
            sourcemap: Some(true),
        },
    )
    .expect("plain css with no vue rewrite");
    assert_eq!(result.code, css);
    assert!(
        result.source_map.is_some(),
        "a requested map on byte-identical passthrough must still be published"
    );
}

#[test]
fn analyze_style_keeps_valid_classes_beside_an_untrusted_selector() {
    let css = ".good { color: red; } .bad-#{$name} { color: blue; }";
    let result = analyze_style(
        css,
        AnalyzeStyleOptions {
            scope_id: "probe1234".to_string(),
            dialect: Some("scss".to_string()),
            filename: Some("Probe.scss".to_string()),
        },
    )
    .expect("read-only analysis must not inherit rewrite refusal");
    assert!(result.static_classes.contains(&"good".to_string()));
    assert_eq!(result.module_classes.len(), 1);
    assert_eq!(result.module_classes[0][0], "good");
    assert_ne!(result.module_classes[0][1], "good");
    assert!(
        result
            .module_classes
            .iter()
            .all(|entry| !entry[0].starts_with("bad")),
        "a dynamic selector must not invent a static module-class fact"
    );
}

#[test]
fn public_process_style_remains_exported() {
    let _: fn(Buffer, ProcessStyleOptions) -> napi::Result<ProcessStyleResult> = process_style;
}
