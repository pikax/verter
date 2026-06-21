//! The EXPRESSION-valued `<svelte:options customElement={EXPR}>` disposition reproducer for the
//! official `svelte@5.56.3` parse pipeline (`phases/1-parse/read/expression.js` +
//! `read/options.js`), scoped to the `customElement` axis only.
//!
//! The parser classifies every other `<svelte:options>` attribute directly (it needs no
//! expression AST), but `customElement={EXPR}`'s disposition depends on the JS expression: first
//! whether it PARSES at all (the acorn attribute-expression parse, before `read_options` —
//! `js_parse_error`), then, for a parseable expression, its shape (a number / `null` / object
//! literal — the `read_options` `customElement` codes). The parser reserves an
//! [`OptionsCustomElementProbe`] at the options-finalization position; this module is what the
//! official-reject gate runs to FILL that slot, parsing the expression with OXC and reproducing
//! upstream's exact code for BOTH checks.
//!
//! [`OptionsCustomElementProbe`]: crate::svelte::parser::OptionsCustomElementProbe

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};
use oxc_span::{GetSpan, SourceType};

use crate::svelte::parser::validate_custom_element_tag;

/// Validate a `<svelte:options customElement={EXPR}>` expression exactly as upstream, returning
/// the EXACT official code on a reject, or `None` when upstream ACCEPTS the expression (a `null`
/// literal, or a valid object). `expr_src` is the raw `{…}` inner expression text.
///
/// Upstream parses the `customElement` value through `read_expression` (`read/expression.js`) →
/// `read_attribute_value`'s mustache reader, which performs a CURSOR parse: `parse_expression_at`
/// parses ONE prefix expression starting at the `{`, records `node.end`, sets `parser.index =
/// node.end`, `allow_whitespace`s, then `eat('}', true)`. So there are THREE outcomes, in upstream's
/// order on the encounter timeline:
/// - the prefix expression FAILS to parse (`{<}`, `{ tag: }`, `{}` empty, `{1 + }`): the acorn parse
///   throws `js_parse_error` (BEFORE `read_options`).
/// - the prefix expression PARSES but TRAILING non-`}` content follows it (`{1 2}`, `{foo bar}`,
///   `{a.b c}`, `{1;2}`): the required `eat('}', true)` fails on the trailing token →
///   `expected_token`. (A comma/sequence `{1,2}` or call `{(1)(2)}` is ONE expression that consumes
///   the whole inner — no trailing junk — so it does NOT take this arm.)
/// - a SINGLE clean expression consumes the whole inner (only whitespace/comments before the `}`):
///   it reaches `read_options` `customElement` VALIDATION (`read/options.js`, parse finalization) —
///   `null` / a conforming object ACCEPTS; anything else is `svelte_options_*`.
///
/// OXC's `parse_expression` already models the cursor: it parses the prefix expression and reports
/// its `span().end`, IGNORING trailing tokens (it does not require full consumption), exactly as
/// acorn's `parse_expression_at` does — so the prefix end is `expr.span().end` and the
/// "remaining is only trivia" test is the faithful `allow_whitespace` + `eat('}')` check.
///
/// The parser already handled the boolean-shorthand (`svelte_options_invalid_customelement`) and
/// Text-value (`validate_tag`) cases — this is the `value[0].expression` branch only, where `v`
/// is the mustache expression, so upstream's `value = [v]` makes `value[0].type !== 'Text'`.
#[must_use]
pub(super) fn options_custom_element_expr_error(expr_src: &str) -> Option<&'static str> {
    let alloc = Allocator::default();
    let parsed = oxc_parser::Parser::new(&alloc, alloc.alloc_str(expr_src), SourceType::mjs())
        .parse_expression();
    let Ok(expr) = parsed else {
        // FIRST CHECK — the PREFIX expression does not PARSE. Upstream reads the attribute
        // expression with acorn during element parsing — BEFORE `read_options` — so a
        // syntactically-malformed `customElement={EXPR}` (including an EMPTY / whitespace-only inner
        // and an incomplete prefix like `1 +`) is `js_parse_error`, NOT a `read_options` code.
        // (Verified pinned svelte@5.56.3: `{{ tag: }}`, `{<}`, `{}`, `{1 + }` all raise
        // `js_parse_error`.)
        return Some("js_parse_error");
    };

    // SECOND CHECK — the prefix expression parsed, but upstream's `eat('}', true)` runs at
    // `node.end` after `allow_whitespace`. If non-trivia content remains between the prefix
    // expression's end and the `}`, the required `}` is missing → `expected_token`. (Verified pinned:
    // `{1 2}` / `{foo bar}` / `{a.b c}` / `{1;2}` → `expected_token`.)
    let prefix_end = (expr.span().end as usize).min(expr_src.len());
    if !is_only_expression_trivia(&expr_src[prefix_end..]) {
        return Some("expected_token");
    }

    // VALIDATION CHECK — a single clean expression consuming the whole inner: run the `read_options`
    // `customElement` branch.
    match &expr {
        // `value[0].expression.type === 'Literal' && value === null` → backwards-compat skip
        // (ACCEPT). OXC models a `null` literal as `Expression::NullLiteral`.
        Expression::NullLiteral(_) => None,
        // An ObjectExpression — validate its `tag` / `props` / `shadow` members.
        Expression::ObjectExpression(obj) => validate_custom_element_object(&obj.properties),
        // Any OTHER expression (a number, string, identifier, call, …) is not an ObjectExpression
        // and not `null` → `svelte_options_invalid_customelement`.
        _ => Some("svelte_options_invalid_customelement"),
    }
}

/// Whether the bytes after the prefix expression's end (up to the `customElement={…}` closing brace)
/// are ONLY trivia — the whitespace `allow_whitespace` consumes plus the `/* … */` and `// …`
/// comments acorn's `read_expression` skips past before the required `eat('}', true)`. Any other
/// remaining content is the trailing token that makes the `}` missing (`expected_token`).
///
/// This is a TRIVIA scan, NOT an expression / type re-parse: it never interprets the remaining text
/// as a type or splits it on operators — it only classifies whitespace / comment runs and stops at
/// the first real token.
///
/// An unterminated `/* … */` inside the value (`customElement={1 /* unterminated} />`) never
/// reaches this scan as the deciding factor: the `customElement={…}` inner span is delimited by the
/// COMMENT-AWARE brace matcher [`find_matching_brace_in`], whose `/*`-to-EOF skip consumes the `}`
/// into the comment, so no closing `}` is found and the span runs to true EOF. The required
/// `eat('}', true)` is therefore missing → `expected_token` (the upstream compiler reaches its
/// `js_parse_error` along a different path; both REJECT, and the exact-code divergence is recorded
/// in the parser-parity debt ledger — see `docs/arch/svelte-native-compiler-plan.md`).
///
/// [`find_matching_brace_in`]: crate::svelte::parser::tokenizer_scan::find_matching_brace_in
fn is_only_expression_trivia(rest: &str) -> bool {
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            // a block comment `/* … */` — skip to its close (or to the end if unterminated).
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if b[i] == b'/' && b.get(i + 1) == Some(&b'/') {
            // a line comment `// …` — skip to end-of-line (or to the end).
            i += 2;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else {
            return false; // a real trailing token — the `}` is missing.
        }
    }
    true
}

/// Validate the `customElement` ObjectExpression's properties, mirroring upstream EXACTLY:
/// every member must be a non-computed identifier-keyed `Property` (else
/// `svelte_options_invalid_customelement`), then the `tag` / `props` / `shadow` members are
/// validated to their specific codes. Unknown members are IGNORED (upstream only reads
/// `tag`/`props`/`shadow`/`extend`).
fn validate_custom_element_object(
    properties: &oxc_allocator::Vec<ObjectPropertyKind>,
) -> Option<&'static str> {
    // (1) every property must be a plain, non-computed, identifier-keyed `Property`.
    for property in properties {
        let ObjectPropertyKind::ObjectProperty(p) = property else {
            // A SpreadProperty (`{...x}`) is not a `Property` → invalid.
            return Some("svelte_options_invalid_customelement");
        };
        if p.computed || property_key_identifier(&p.key).is_none() {
            return Some("svelte_options_invalid_customelement");
        }
    }

    // (2) `tag` → validate_tag on the static string value (or `None` when not a string literal).
    if let Some(tag_value) = find_property(properties, "tag") {
        let tag = string_literal_value(tag_value);
        if let Some(code) = validate_custom_element_tag(tag) {
            return Some(code);
        }
    }

    // (3) `props` → must be an ObjectExpression of `{ [key]: { attribute?, reflect?, type? } }`.
    if let Some(props_value) = find_property(properties, "props") {
        if let Some(code) = validate_custom_element_props(props_value) {
            return Some(code);
        }
    }

    // (4) `shadow` → `"open"` / `"none"` / an ObjectExpression; otherwise invalid shadow.
    if let Some(shadow_value) = find_property(properties, "shadow") {
        if let Some(code) = validate_custom_element_shadow(shadow_value) {
            return Some(code);
        }
    }

    // `extend` and any unknown member: no validation (upstream ignores them).
    None
}

/// Validate the `props` member value, mirroring upstream's `svelte_options_invalid_customelement_props`
/// rules: `props` must be an ObjectExpression; each entry's value must be an ObjectExpression of
/// only `type` (`"String"`/`"Number"`/`"Boolean"`/`"Array"`/`"Object"`), `reflect` (boolean), or
/// `attribute` (string) literal members.
fn validate_custom_element_props(props: &Expression) -> Option<&'static str> {
    const INVALID: &str = "svelte_options_invalid_customelement_props";
    let Expression::ObjectExpression(obj) = props else {
        return Some(INVALID);
    };
    for property in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = property else {
            return Some(INVALID);
        };
        if p.computed || property_key_identifier(&p.key).is_none() {
            return Some(INVALID);
        }
        let Expression::ObjectExpression(entry) = &p.value else {
            return Some(INVALID);
        };
        for entry_prop in &entry.properties {
            let ObjectPropertyKind::ObjectProperty(ep) = entry_prop else {
                return Some(INVALID);
            };
            let (Some(key), false) = (property_key_identifier(&ep.key), ep.computed) else {
                return Some(INVALID);
            };
            match key {
                "type" => {
                    let valid = matches!(
                        string_literal_value(&ep.value),
                        Some("String" | "Number" | "Boolean" | "Array" | "Object")
                    );
                    if !valid {
                        return Some(INVALID);
                    }
                }
                "reflect" => {
                    if !matches!(ep.value, Expression::BooleanLiteral(_)) {
                        return Some(INVALID);
                    }
                }
                "attribute" => {
                    if string_literal_value(&ep.value).is_none() {
                        return Some(INVALID);
                    }
                }
                _ => return Some(INVALID),
            }
        }
    }
    None
}

/// Validate the `shadow` member value, mirroring upstream's
/// `svelte_options_invalid_customelement_shadow` rule: a `"open"`/`"none"` string literal, or any
/// ObjectExpression (a `ShadowRootInit`), is accepted; anything else is invalid.
fn validate_custom_element_shadow(shadow: &Expression) -> Option<&'static str> {
    match shadow {
        Expression::StringLiteral(s) if s.value == "open" || s.value == "none" => None,
        Expression::ObjectExpression(_) => None,
        _ => Some("svelte_options_invalid_customelement_shadow"),
    }
}

/// The value of the FIRST object property whose identifier key equals `name`, or `None` —
/// mirroring upstream's `properties.find(([n]) => n === name)?.[1]` over the
/// `[key.name, property.value]` pairs it collected. The caller already asserted (in the loop-(1)
/// pass) that every property is a non-computed identifier-keyed `Property`, so this just selects
/// by name and returns the property VALUE (for a getter/shorthand the value is the function /
/// identifier node, which the downstream `string_literal_value` correctly treats as non-string).
fn find_property<'a>(
    properties: &'a oxc_allocator::Vec<ObjectPropertyKind<'a>>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    properties.iter().find_map(|property| match property {
        ObjectPropertyKind::ObjectProperty(p) if property_key_identifier(&p.key) == Some(name) => {
            Some(&p.value)
        }
        _ => None,
    })
}

/// The identifier name of a non-computed object-property key (`{ tag: … }` → `"tag"`), or `None`
/// for a computed / non-identifier key. Mirrors upstream's `key.type === 'Identifier'`.
fn property_key_identifier<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// The string value of a STRING-LITERAL expression (`"my-el"` → `Some("my-el")`), or `None` for a
/// non-string-literal — faithful to upstream reading `value` only off a static string Literal.
fn string_literal_value<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "options_reject_tests.rs"]
mod options_reject_tests;
