use super::*;
use verter_css_syntax::parse_inline_style_declarations_thread_invocations;

#[test]
fn a_quoted_semicolon_in_a_style_value_is_not_a_declaration_boundary() {
    let result = css_to_js_object(r#"content: "a;b"; color: red;"#);
    assert_eq!(result, r#"{"content":"\"a;b\"","color":"red"}"#);
}

#[test]
fn a_style_object_parses_the_declarations_once() {
    let before = parse_inline_style_declarations_thread_invocations();
    css_to_js_object("color: red; font-size: 14px");
    let after = parse_inline_style_declarations_thread_invocations();
    assert_eq!(after - before, 1);
}

#[test]
fn server_text_decodes_then_reencodes_then_escapes_the_literal() {
    // Decoded `&lt;` re-encodes; a decoded named entity stays decoded; the
    // literal delimiter and an interpolation opener are escaped.
    assert_eq!(
        escape_ssr_text("&lt;b&gt; &copy; `x` ${y}", false),
        "&lt;b&gt; \u{a9} \\`x\\` \\${y}"
    );
    assert_eq!(escape_ssr_text("a & 'b'", false), "a &amp; &#39;b&#39;");
    // Preformatted text keeps its line breaks and indentation.
    assert_eq!(escape_ssr_text("a\n  &lt;b", true), "a\n  &lt;b");
    assert_eq!(escape_ssr_text("a\n  &lt;b", false), "a &lt;b");
}

#[test]
fn hyphenated_attribute_names_are_quoted_keys() {
    assert_eq!(html_attr_to_js_key("aria-label"), "\"aria-label\"");
    assert_eq!(html_attr_to_js_key("for"), "for");
}

#[test]
fn directive_names_camelize_with_a_v_prefix() {
    assert_eq!(directive_to_camel("click-outside"), "vClickOutside");
    assert_eq!(directive_to_camel("focus"), "vFocus");
}
