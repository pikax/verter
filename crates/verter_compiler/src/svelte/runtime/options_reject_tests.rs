//! Unit tests for the faithful `read_options` `customElement` expression validator.
//!
//! Each expectation is grounded against the pinned `svelte@5.56.3` compiler (a `None` here ⇔ the
//! pinned compiler ACCEPTS the `customElement` expression; a `Some(code)` ⇔ it throws that exact
//! code). The input is the raw `{…}` inner expression text the parser reserves. The validator runs
//! upstream's TWO stages in order: a SYNTACTICALLY-malformed expression → `js_parse_error` (the
//! acorn attribute-expression parse, before `read_options`); a parseable-but-invalid one → the
//! exact `svelte_options_*` code from the `read_options` `customElement` branch.

use super::options_custom_element_expr_error;

#[test]
fn null_literal_is_accepted() {
    assert_eq!(options_custom_element_expr_error("null"), None);
}

#[test]
fn number_is_invalid_customelement() {
    assert_eq!(
        options_custom_element_expr_error("42"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn identifier_is_invalid_customelement() {
    assert_eq!(
        options_custom_element_expr_error("foo"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn string_literal_is_invalid_customelement() {
    // A bare string EXPRESSION (`customElement={"my-el"}`) is NOT a Text value — upstream's
    // `value[0].expression.type !== 'ObjectExpression'` and not `null` → invalid customElement.
    // (A Text-valued `customElement="my-el"` is the ACCEPT case, handled by the parser.)
    assert_eq!(
        options_custom_element_expr_error("\"my-el\""),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn malformed_object_expression_is_js_parse_error() {
    // `{ tag: }` is SYNTACTICALLY malformed (a property with no value) — upstream's acorn
    // attribute-expression parse fails BEFORE `read_options` with `js_parse_error`, NOT a
    // `read_options` `customElement` code. (Verified pinned: `customElement={{ tag: }}` →
    // `js_parse_error`.)
    assert_eq!(
        options_custom_element_expr_error("{ tag: }"),
        Some("js_parse_error")
    );
}

#[test]
fn malformed_bare_lt_expression_is_js_parse_error() {
    // `<` is not a valid expression — `js_parse_error` (verified pinned:
    // `customElement={<}` → `js_parse_error`).
    assert_eq!(
        options_custom_element_expr_error("<"),
        Some("js_parse_error")
    );
}

#[test]
fn empty_expression_is_js_parse_error() {
    // An EMPTY `customElement={}` carries an empty inner expression — acorn parses no expression
    // → `js_parse_error` (verified pinned: `customElement={}` → `js_parse_error`). (Distinct from
    // `customElement={{}}`, whose inner is the VALID empty-object literal `{}` — see
    // `empty_object_is_accepted`.)
    assert_eq!(
        options_custom_element_expr_error(""),
        Some("js_parse_error")
    );
}

#[test]
fn valid_object_with_tag_is_accepted() {
    assert_eq!(
        options_custom_element_expr_error("{ tag: \"my-el\" }"),
        None
    );
}

#[test]
fn empty_object_is_accepted() {
    assert_eq!(options_custom_element_expr_error("{}"), None);
}

#[test]
fn object_with_bad_tag_is_invalid_tagname() {
    assert_eq!(
        options_custom_element_expr_error("{ tag: \"nodash\" }"),
        Some("svelte_options_invalid_tagname")
    );
}

#[test]
fn object_with_reserved_tag_is_reserved_tagname() {
    assert_eq!(
        options_custom_element_expr_error("{ tag: \"annotation-xml\" }"),
        Some("svelte_options_reserved_tagname")
    );
}

#[test]
fn object_with_non_string_tag_is_invalid_tagname() {
    // `tag: 1` — a non-string-literal tag value → upstream `validate_tag(typeof tag !== 'string')`
    // → `svelte_options_invalid_tagname`.
    assert_eq!(
        options_custom_element_expr_error("{ tag: 1 }"),
        Some("svelte_options_invalid_tagname")
    );
}

#[test]
fn object_with_spread_member_is_invalid_customelement() {
    assert_eq!(
        options_custom_element_expr_error("{ ...foo }"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn object_with_computed_key_is_invalid_customelement() {
    assert_eq!(
        options_custom_element_expr_error("{ [foo]: 1 }"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn object_with_non_object_props_is_invalid_customelement_props() {
    assert_eq!(
        options_custom_element_expr_error("{ props: 1 }"),
        Some("svelte_options_invalid_customelement_props")
    );
}

#[test]
fn object_with_bad_props_entry_is_invalid_customelement_props() {
    // a props entry value that is not an object → invalid props.
    assert_eq!(
        options_custom_element_expr_error("{ props: { foo: 1 } }"),
        Some("svelte_options_invalid_customelement_props")
    );
}

#[test]
fn object_with_bad_props_type_is_invalid_customelement_props() {
    assert_eq!(
        options_custom_element_expr_error("{ props: { foo: { type: \"Date\" } } }"),
        Some("svelte_options_invalid_customelement_props")
    );
}

#[test]
fn object_with_valid_props_is_accepted() {
    assert_eq!(
        options_custom_element_expr_error(
            "{ tag: \"my-el\", props: { foo: { type: \"String\", reflect: true, attribute: \"foo\" } } }"
        ),
        None
    );
}

#[test]
fn object_with_string_shadow_open_is_accepted() {
    assert_eq!(
        options_custom_element_expr_error("{ shadow: \"open\" }"),
        None
    );
}

#[test]
fn object_with_object_shadow_is_accepted() {
    assert_eq!(
        options_custom_element_expr_error("{ shadow: { mode: \"open\" } }"),
        None
    );
}

#[test]
fn object_with_bad_shadow_is_invalid_customelement_shadow() {
    assert_eq!(
        options_custom_element_expr_error("{ shadow: 1 }"),
        Some("svelte_options_invalid_customelement_shadow")
    );
}

#[test]
fn object_with_getter_tag_is_invalid_tagname() {
    // `{ get tag() { … } }` — the tag VALUE is a getter function, not a string literal →
    // upstream `validate_tag(typeof tag !== 'string')` → `svelte_options_invalid_tagname`.
    assert_eq!(
        options_custom_element_expr_error("{ get tag() { return \"x\" } }"),
        Some("svelte_options_invalid_tagname")
    );
}

#[test]
fn object_with_shorthand_tag_is_invalid_tagname() {
    // `{ tag }` shorthand — the value is the identifier `tag`, not a string literal →
    // `svelte_options_invalid_tagname`.
    assert_eq!(
        options_custom_element_expr_error("{ tag }"),
        Some("svelte_options_invalid_tagname")
    );
}

#[test]
fn object_with_method_tag_is_invalid_tagname() {
    // `{ tag() {} }` — a method value is not a string literal → `svelte_options_invalid_tagname`.
    assert_eq!(
        options_custom_element_expr_error("{ tag() {} }"),
        Some("svelte_options_invalid_tagname")
    );
}

// ── attribute-expression CURSOR parse: trailing junk after a clean prefix → expected_token ──
// Upstream's `read_expression` (`read/expression.js`) parses ONE prefix expression, advances
// `parser.index` to its end, `allow_whitespace`s, then `eat('}', true)` — so a CLEAN prefix
// followed by trailing non-`}` content throws `expected_token` (the `}` the brace expected), while
// a syntactically-MALFORMED prefix throws `js_parse_error`, and only a single clean expression
// consuming the whole `{…}` reaches the `read_options` validation. A full-span parse that does not
// model the prefix cursor can only yield `js_parse_error` for `{1 2}` — these rows discriminate
// that. Grounded against pinned svelte@5.56.3: `{1 2}`/`{foo bar}`/`{"a" "b"}`/`{a.b c}`/`{1;2}` →
// `expected_token`; `{1 + }` → `js_parse_error`; `{1,2}`/`{(1)(2)}` → the WHOLE source is one
// expression (sequence / call), so they reach validation (`svelte_options_invalid_customelement`).
#[test]
fn clean_prefix_then_trailing_token_is_expected_token() {
    assert_eq!(
        options_custom_element_expr_error("1 2"),
        Some("expected_token")
    );
}

#[test]
fn clean_identifier_prefix_then_trailing_token_is_expected_token() {
    assert_eq!(
        options_custom_element_expr_error("foo bar"),
        Some("expected_token")
    );
}

#[test]
fn clean_string_prefix_then_trailing_string_is_expected_token() {
    assert_eq!(
        options_custom_element_expr_error("\"a\" \"b\""),
        Some("expected_token")
    );
}

#[test]
fn clean_member_prefix_then_trailing_token_is_expected_token() {
    // `a.b c` — the prefix `a.b` is a complete member expression; the trailing `c` is junk.
    assert_eq!(
        options_custom_element_expr_error("a.b c"),
        Some("expected_token")
    );
}

#[test]
fn clean_prefix_then_semicolon_is_expected_token() {
    // `1;2` — the prefix expression is `1`; the `;` is a statement terminator, not part of an
    // expression, so it is trailing junk (NOT a sequence) → `expected_token`.
    assert_eq!(
        options_custom_element_expr_error("1;2"),
        Some("expected_token")
    );
}

#[test]
fn incomplete_binary_prefix_is_js_parse_error() {
    // `1 + ` — the prefix itself is syntactically incomplete (a binary with no RHS), so the parse
    // FAILS (not a clean prefix + trailing junk) → `js_parse_error`.
    assert_eq!(
        options_custom_element_expr_error("1 + "),
        Some("js_parse_error")
    );
}

#[test]
fn sequence_expression_consumes_whole_and_validates() {
    // `1,2` — the comma operator makes the WHOLE source one sequence expression (end consumes
    // `2`), so it reaches `read_options` validation: a non-object non-null expression →
    // `svelte_options_invalid_customelement`. (NOT `expected_token` — there is no trailing junk.)
    assert_eq!(
        options_custom_element_expr_error("1,2"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn call_expression_consumes_whole_and_validates() {
    // `(1)(2)` — a single call expression consumes the whole source → validation
    // (`svelte_options_invalid_customelement`), NOT `expected_token`.
    assert_eq!(
        options_custom_element_expr_error("(1)(2)"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn clean_prefix_with_trailing_whitespace_validates() {
    // `1   ` (trailing whitespace only) — `allow_whitespace` consumes it, the `}` follows, so the
    // single expression `1` reaches validation (NOT `expected_token`).
    assert_eq!(
        options_custom_element_expr_error("1   "),
        Some("svelte_options_invalid_customelement")
    );
}
