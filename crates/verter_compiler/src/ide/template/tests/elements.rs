use super::*;

#[test]
fn text_content() {
    let result = gen_tsx_template("<template><div>hello</div></template>");
    let facts = jsx_element_body_facts(&result);
    assert_eq!(facts.element_count, 1);
    assert!(
        facts.non_empty_elements.is_empty(),
        "got: {facts:?}\n{result}"
    );
    assert_eq!(
        facts.hello_string_literals, 1,
        "the detached template text must remain a JSX string expression: {facts:?}\n{result}"
    );
}

#[test]
fn text_content_with_lt_wrapped() {
    let result = gen_tsx_template("<template>2 < 1</template>");
    assert!(
        result.contains("{\"2 < 1\"}")
            || (result.contains("{\"2\"}") && result.contains("{\"< 1\"}")),
        "got: {}",
        result
    );
}

#[test]
fn text_content_escapes_quote() {
    let result = gen_tsx_template("<template>\"</template>");
    assert!(result.contains("{\"\\\"\"}"), "got: {}", result);
}

#[test]
fn self_closing_element() {
    let result = gen_tsx_template("<template><br/></template>");
    assert!(result.contains("<br/>"), "got: {}", result);
}

#[test]
fn void_element_without_self_closing_slash() {
    // HTML void elements like <br> (no slash) must become self-closing in JSX
    let result = gen_tsx_template("<template><br></template>");
    // Must be self-closing in JSX output (either <br/> or <br />)
    assert!(
        result.contains("<br/>") || result.contains("<br />"),
        "void element <br> must be self-closing in JSX: {result}"
    );
    // Must NOT have unclosed <br> (which is invalid JSX)
    assert!(
        !result.contains("<br>"),
        "raw <br> must not appear in JSX output: {result}"
    );

    // Multiple adjacent void elements
    let result2 = gen_tsx_template("<template><br><br></template>");
    assert!(
        !result2.contains("<br>"),
        "adjacent void <br><br> must both be self-closing: {result2}"
    );

    // <input> with attributes
    let result3 = gen_tsx_template(r#"<template><input type="text"></template>"#);
    assert!(
        !result3.contains("<input type=\"text\">"),
        "void <input> with attrs must be self-closing: {result3}"
    );
}

#[test]
fn multiline_text_escapes_newlines_in_string_literal() {
    let result = gen_tsx_template("<template><p>\n  Hello\n  World\n</p></template>");
    // Text IS wrapped in {"..."} — but newlines must be escaped as \n
    assert!(
        result.contains("{\""),
        "text should be wrapped in string literal: {result}"
    );
    // Must contain escaped newlines, not raw newlines inside the string
    assert!(
        result.contains("\\n"),
        "newlines in text must be escaped as \\n: {result}"
    );
    // The {"..."} expression must be on a single line (no raw newlines)
    for line in result.lines() {
        if line.contains("{\"") {
            assert!(
                line.contains("\"}"),
                "text string literal must be on single line (no raw newlines): {result}"
            );
        }
    }
}

#[test]
fn nested_elements() {
    let result = gen_tsx_template("<template><div><span></span></div></template>");
    let facts = jsx_element_body_facts(&result);
    assert_eq!(
        facts.element_count, 2,
        "div and span must both remain typed"
    );
    assert!(
        facts.non_empty_elements.is_empty(),
        "nested Vue elements must become fragment siblings, not JSX element children: {facts:?}\n{result}"
    );
}

#[test]
fn multiple_root_elements() {
    let result = gen_tsx_template("<template><div></div><span></span></template>");
    assert!(
        result.contains("<>") && result.contains("</>"),
        "Multiple root elements should be wrapped in fragment, got: {}",
        result
    );
}

#[test]
fn v_else_if_attribute_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    assert!(
        !result.contains("v-if"),
        "v-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else-if"),
        "v-else-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else must not appear in output, got: {}",
        result
    );
}

#[test]
fn valid_interpolations_unaffected_by_broken_expr_handling() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Valid expression should still work normally (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should produce {{count}}: {result}"
    );
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
}

#[test]
fn mixed_broken_and_valid_interpolations() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}<span>{{ count }}</span></div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("mixed output: {}", result);
    // Valid expression should be fully patched (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should be patched: {result}"
    );
    // Broken expression: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should still preserve identifiers: {result}"
    );
    // No raw mustache delimiters anywhere
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
    assert_valid_tsx(&result, "mixed-broken-and-valid-interpolations");
}
