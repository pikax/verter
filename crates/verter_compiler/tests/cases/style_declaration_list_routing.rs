//! A17 routing proof: VDOM's public `emit_static_style_object` reads its
//! inline `style="..."` declarations through the shared
//! `verter_css_syntax::parse_inline_style_declarations` entry point, not a
//! hand-rolled `split(';')`/`find(':')` scan.

use verter_compiler::emit_static_style_object;
use verter_css_syntax::parse_inline_style_declarations_thread_invocations;

fn style_to_obj(style: &str) -> String {
    let mut buf = String::new();
    emit_static_style_object(&mut buf, style);
    buf
}

// Discriminating positive: a value containing a semicolon inside a quoted
// string must not be treated as a statement boundary — a real bug in the
// hand-rolled `split(';')` loop this superseded (`props.rs:137`).
#[test]
fn emit_static_style_object_quoted_semicolon_in_value_parses_correctly() {
    assert_eq!(
        style_to_obj(r#"content: "a;b"; color: red;"#),
        r#"{ "content": "\"a;b\"", "color": "red" }"#
    );
}

// Routing proof: the shared declaration-list parser is invoked EXACTLY once
// per call — not zero (a private scanner still producing the output), not
// two-or-more (a redundant re-parse, itself a parse-once violation).
#[test]
fn emit_static_style_object_shared_parser_invoked_exactly_once() {
    let before = parse_inline_style_declarations_thread_invocations();
    style_to_obj("color: red; font-size: 14px");
    let after = parse_inline_style_declarations_thread_invocations();
    assert_eq!(after - before, 1);
}
