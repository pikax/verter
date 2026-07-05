//! Unit tests for the faithful `read_options` `customElement` expression engine — the ONE
//! validate+extract walk the parser runs at options finalization and retains on the probe.
//!
//! Each expectation is grounded against the pinned `svelte@5.56.3` compiler (a `None` here ⇔ the
//! pinned compiler ACCEPTS the `customElement` expression; a `Some(code)` ⇔ it throws that exact
//! code). The input is the raw `{…}` inner expression text the parser resolves. The engine runs
//! upstream's TWO stages in order: a SYNTACTICALLY-malformed expression → `js_parse_error` (the
//! acorn attribute-expression parse, before `read_options`); a parseable-but-invalid one → the
//! exact `svelte_options_*` code from the `read_options` `customElement` branch. The ACCEPT side
//! retains the typed value in the SAME walk — the retention rows below pin the extracted
//! descriptor.

use super::{
    resolve_custom_element_expr, AcceptedCustomElementValue, CustomElementDescriptor,
    CustomElementProp, CustomElementShadow,
};

/// The reject-side projection of the one validate+extract engine (`Some(code)` ⇔ the exact
/// official reject code; `None` ⇔ official ACCEPTS) — the shape every exact-code row asserts.
fn options_custom_element_expr_error(expr_src: &str) -> Option<&'static str> {
    resolve_custom_element_expr(expr_src).err()
}

#[test]
fn null_literal_is_accepted() {
    assert_eq!(options_custom_element_expr_error("null"), None);
}

#[test]
fn null_literal_retains_the_backwards_compat_skip() {
    // The ACCEPT side of the same walk: `null` retains the NullSkip marker (sets
    // NOTHING — the compile option still decides), never a descriptor.
    assert_eq!(
        resolve_custom_element_expr("null"),
        Ok(AcceptedCustomElementValue::NullSkip)
    );
}

#[test]
fn valid_object_retains_the_typed_descriptor_in_the_same_walk() {
    // The ACCEPT side extracts the typed descriptor in the SAME walk that
    // validated it: tag + shadow + literal-only props + verbatim extend.
    assert_eq!(
        resolve_custom_element_expr(
            "{ tag: \"my-el\", shadow: \"none\", props: { count: { reflect: true, type: \"Number\" } }, extend: (c) => c }"
        ),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("my-el".to_string()),
                shadow: CustomElementShadow::None,
                props: vec![CustomElementProp {
                    name: "count".to_string(),
                    attribute: None,
                    reflect: true,
                    type_hint: Some("Number".to_string()),
                }],
                extend: Some("(c) => c".to_string()),
                inject_styles: true,
            }
        ))
    );
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

// ── parenthesized forms: upstream `read_expression` returns `remove_parens(node)` — a DEEP
// recursive walk replacing every `ParenthesizedExpression` with its inner expression — BEFORE
// `read_options` classifies, so author parens are transparent at EVERY validation site. OXC
// parses with `preserve_parens` on (as acorn does under `preserveParens: true`), so the engine
// peels the typed `ParenthesizedExpression` nodes the same way. Every row below is grounded
// against pinned svelte@5.56.3 first-hand: the paren-object / paren-null / paren-tag /
// paren-shadow / paren-props-field forms ACCEPT (with the same emission as their unwrapped
// spellings), while a parenthesized INVALID value keeps its exact reject code — parens never
// turn an invalid value valid. ──

#[test]
fn parenthesized_object_is_accepted() {
    // `customElement={({ tag: 'x-paren' })}` — official ACCEPTS (emits
    // `customElements.define('x-paren', …)`), so the paren-wrapped descriptor object must
    // classify as the ObjectExpression, not fall to the invalid-customelement arm.
    assert_eq!(
        options_custom_element_expr_error("({ tag: 'x-paren' })"),
        None
    );
}

#[test]
fn parenthesized_object_retains_the_peeled_descriptor() {
    // The ACCEPT side of the same walk: the descriptor extracted through the paren wrapper is
    // IDENTICAL to the unwrapped spelling's.
    assert_eq!(
        resolve_custom_element_expr("({ tag: 'x-paren' })"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-paren".to_string()),
                shadow: CustomElementShadow::Open,
                props: Vec::new(),
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn nested_parenthesized_object_peels_recursively() {
    // `((({ tag: 'x-dd' })))` — upstream's `remove_parens` walk removes EVERY paren layer
    // (verified pinned: accepted, defines `x-dd`), so the peel must recurse, not strip one layer.
    assert_eq!(
        resolve_custom_element_expr("((({ tag: 'x-dd' })))"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-dd".to_string()),
                shadow: CustomElementShadow::Open,
                props: Vec::new(),
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_null_retains_the_backwards_compat_skip() {
    // `customElement={(null)}` — official ACCEPTS as the Svelte-3 backwards-compat no-op
    // (verified pinned: no custom-element emission), so the peeled `null` retains NullSkip.
    assert_eq!(
        resolve_custom_element_expr("(null)"),
        Ok(AcceptedCustomElementValue::NullSkip)
    );
}

#[test]
fn parenthesized_tag_value_reads_through() {
    // `{ tag: ('x-ptag') }` — the deep `remove_parens` makes the tag VALUE the string literal
    // (verified pinned: defines `x-ptag`), so the string-literal read peels before matching.
    assert_eq!(
        resolve_custom_element_expr("{ tag: ('x-ptag') }"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-ptag".to_string()),
                shadow: CustomElementShadow::Open,
                props: Vec::new(),
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_shadow_string_reads_through() {
    // `{ tag: 'x-a', shadow: ('none') }` — official ACCEPTS and emits the arg5-omitted 4-arg
    // `create_custom_element` (verified pinned), so the peeled `'none'` resolves Shadow::None.
    assert_eq!(
        resolve_custom_element_expr("{ tag: 'x-a', shadow: ('none') }"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-a".to_string()),
                shadow: CustomElementShadow::None,
                props: Vec::new(),
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_shadow_object_slices_the_peeled_span() {
    // `{ shadow: ({ mode: 'open' }) }` — the verbatim ShadowRootInit slice comes from the PEELED
    // ObjectExpression's span, so the emitted arg5 source is `{ mode: 'open' }` WITHOUT the
    // author parens — matching official, which prints the peeled AST (verified pinned:
    // `shadow: ({ mode: 'open', delegatesFocus: true })` emits the object without parens).
    assert_eq!(
        resolve_custom_element_expr("{ tag: 'x-so', shadow: ({ mode: 'open' }) }"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-so".to_string()),
                shadow: CustomElementShadow::ObjectInit("{ mode: 'open' }".to_string()),
                props: Vec::new(),
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_extend_slices_the_peeled_span() {
    // `{ extend: ((c) => c) }` — the verbatim extend slice comes from the PEELED arrow's span:
    // official prints the peeled AST as `(c) => c` (verified pinned), so the wrapper parens are
    // NOT part of the retained source.
    assert_eq!(
        resolve_custom_element_expr("{ tag: 'x-e', extend: ((c) => c) }"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-e".to_string()),
                shadow: CustomElementShadow::Open,
                props: Vec::new(),
                extend: Some("(c) => c".to_string()),
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_props_fields_read_through() {
    // Parens at EVERY props depth — the `props` value, the entry value, and each field value
    // (`reflect: (true)`, `type: ('Number')`, `attribute: ('data-c')`) — read through, exactly
    // as upstream's deep `remove_parens` walk (verified pinned: accepted, full define emission).
    assert_eq!(
        resolve_custom_element_expr(
            "{ tag: 'x-r', props: ({ count: ({ reflect: (true), type: ('Number'), attribute: ('data-c') }) }) }"
        ),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-r".to_string()),
                shadow: CustomElementShadow::Open,
                props: vec![CustomElementProp {
                    name: "count".to_string(),
                    attribute: Some("data-c".to_string()),
                    reflect: true,
                    type_hint: Some("Number".to_string()),
                }],
                extend: None,
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_sequence_extend_keeps_its_syntax_required_parens() {
    // `extend: (0, (c) => c)` — official ACCEPTS and emits the sequence as ONE arg6 WITH its
    // syntax-required parens: `create_custom_element(…, (0, (c) => c))` (verified pinned). A
    // bare `0, (c) => c` splice would parse as TWO arguments (an arity break), so the peeled
    // SequenceExpression — the one expression whose bare text is ambiguous in an argument slot —
    // retains the wrapping parens. (A sequence-valued object member MUST be parenthesized in
    // source, so the parens are always there to keep.)
    assert_eq!(
        resolve_custom_element_expr("{ tag: 'x-seq', extend: (0, (c) => c) }"),
        Ok(AcceptedCustomElementValue::Descriptor(
            CustomElementDescriptor {
                tag: Some("x-seq".to_string()),
                shadow: CustomElementShadow::Open,
                props: Vec::new(),
                extend: Some("(0, (c) => c)".to_string()),
                inject_styles: true,
            }
        ))
    );
}

#[test]
fn parenthesized_number_is_still_invalid_customelement() {
    // NEGATIVE: parens never turn an invalid value valid — `(1)` peels to the number `1`,
    // which is not an ObjectExpression and not `null` (verified pinned:
    // `svelte_options_invalid_customelement`).
    assert_eq!(
        options_custom_element_expr_error("(1)"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn parenthesized_identifier_is_still_invalid_customelement() {
    // NEGATIVE: `(someIdent)` — a dynamic value stays invalid through the parens (verified
    // pinned: `svelte_options_invalid_customelement`).
    assert_eq!(
        options_custom_element_expr_error("(someIdent)"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn parenthesized_string_is_still_invalid_customelement() {
    // NEGATIVE: `('my-el')` — a bare string EXPRESSION stays invalid through the parens
    // (verified pinned: `svelte_options_invalid_customelement`).
    assert_eq!(
        options_custom_element_expr_error("('my-el')"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn parenthesized_invalid_inner_values_keep_their_exact_codes() {
    // NEGATIVE: a parenthesized INVALID inner value keeps the same exact reject code as its
    // unwrapped spelling (each row verified pinned first-hand).
    assert_eq!(
        options_custom_element_expr_error("{ tag: (1) }"),
        Some("svelte_options_invalid_tagname")
    );
    assert_eq!(
        options_custom_element_expr_error("{ tag: 'x-a', shadow: (1) }"),
        Some("svelte_options_invalid_customelement_shadow")
    );
    assert_eq!(
        options_custom_element_expr_error("{ tag: 'x-a', props: (1) }"),
        Some("svelte_options_invalid_customelement_props")
    );
    assert_eq!(
        options_custom_element_expr_error("{ tag: 'x-a', props: { c: { reflect: (1) } } }"),
        Some("svelte_options_invalid_customelement_props")
    );
    assert_eq!(
        options_custom_element_expr_error("{ tag: 'x-a', props: { c: { type: ('Date') } } }"),
        Some("svelte_options_invalid_customelement_props")
    );
    assert_eq!(
        options_custom_element_expr_error("({ ...foo })"),
        Some("svelte_options_invalid_customelement")
    );
}

#[test]
fn parenthesized_prefix_with_trailing_junk_is_still_expected_token() {
    // The CURSOR check runs on the UNPEELED prefix span (upstream sets `parser.index = node.end`
    // of the raw parenthesized node before `remove_parens`), so `(null) 2` is still a clean
    // prefix + trailing junk → `expected_token` (verified pinned).
    assert_eq!(
        options_custom_element_expr_error("(null) 2"),
        Some("expected_token")
    );
}
