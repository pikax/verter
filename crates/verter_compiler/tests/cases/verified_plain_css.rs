use std::sync::Arc;

use verter_compiler::style_planner::{transform_vue_style, VerifiedPlainCss};
use verter_css_syntax::{
    parse_style_ir, parse_style_ir_thread_invocations, CssDialect, CssParseMode, CssSource,
};

fn parsed(source: &str, dialect: CssDialect) -> verter_css_syntax::StyleSyntaxIr {
    parse_style_ir(
        CssSource::new(Arc::from(source), 0).expect("test stylesheet fits the parser"),
        dialect,
        CssParseMode::Recover,
    )
    .expect("recover-mode parse")
}

#[test]
fn parsed_native_css_witness_runs_the_vue_transform_without_reparsing() {
    let before = parse_style_ir_thread_invocations();
    let ir = parsed(".card { color: red; }", CssDialect::Css);
    let verified = VerifiedPlainCss::from_parsed_native_css(&ir)
        .expect("a native-CSS parse carries the required provenance");

    let outcome = transform_vue_style(
        verified,
        "component.css",
        "space:component",
        "artifact:component",
        "scope123",
        false,
        true,
        false,
    );

    assert!(outcome.stage_failures.is_empty(), "{outcome:?}");
    assert!(
        outcome.code.contains(".card[data-v-scope123]"),
        "{}",
        outcome.code
    );
    assert!(!outcome.code.contains(".card {"), "{}", outcome.code);
    assert_ne!(outcome.code, ir.source().text());
    assert_eq!(
        parse_style_ir_thread_invocations(),
        before + 1,
        "the transform must consume the witness's existing parse"
    );

    let scss = parsed("$tone: red; .card { color: $tone; }", CssDialect::Scss);
    assert!(
        VerifiedPlainCss::from_parsed_native_css(&scss).is_none(),
        "a non-native dialect tag must not mint the witness"
    );
}
