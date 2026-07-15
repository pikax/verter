//! Unit tests for the span-bearing Svelte CSS parser: every node kind carries
//! ABSOLUTE byte spans into the original component source, node payloads match
//! the official `svelte@5.56.3` `read/style.js` reader, and a malformed body
//! returns a typed error (never a panic).

use crate::svelte::runtime::css::parse::parse_style_body;
use crate::svelte::runtime::css::types::{BlockChild, Declaration, SimpleSelector, StyleChild};
use verter_span::Span;

/// Wrap `css` in a component with a leading template so the parsed spans are
/// provably ABSOLUTE (offset by the prefix), and return `(source, body_span)`.
fn component_with_css(css: &str) -> (String, Span) {
    let source = format!("<div>x</div>\n<style>{css}</style>\n");
    let start = source.find("<style>").expect("open tag") + "<style>".len();
    let end = source.rfind("</style>").expect("close tag");
    (source, Span::new(start as u32, end as u32))
}

/// Slice `source` by `span`.
fn at(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

#[test]
fn rule_selector_block_declaration_carry_absolute_spans() {
    let (source, body) = component_with_css("\n\t.card { color: blue; }\n");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    assert_eq!(sheet.span, body, "stylesheet span is the body span");
    assert_eq!(sheet.children.len(), 1);
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    assert_eq!(at(&source, rule.span), ".card { color: blue; }");
    // The prelude selector list ends at the last selector (pre-whitespace).
    assert_eq!(at(&source, rule.prelude.span), ".card");
    let complex = &rule.prelude.children[0];
    assert_eq!(at(&source, complex.span), ".card");
    let relative = &complex.children[0];
    assert_eq!(at(&source, relative.span), ".card");
    assert!(relative.combinator.is_none());
    let SimpleSelector::Class { span, name } = &relative.selectors[0] else {
        panic!("a class selector");
    };
    assert_eq!(name, "card");
    assert_eq!(at(&source, *span), ".card");
    // The block spans `{ … }`; the declaration span ends BEFORE the `;`.
    assert_eq!(at(&source, rule.block.span), "{ color: blue; }");
    let BlockChild::Declaration(decl) = &rule.block.children[0] else {
        panic!("a declaration");
    };
    assert_eq!(decl.property, "color");
    assert_eq!(decl.value, "blue");
    assert_eq!(at(&source, decl.span), "color: blue");
}

#[test]
fn simple_selector_family_parses_with_spans() {
    let (source, body) =
        component_with_css("div, #top, *, svg|rect, [a], [a=b], [a~=\"v w\" i] { color: red }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let names: Vec<&SimpleSelector> = rule
        .prelude
        .children
        .iter()
        .map(|complex| &complex.children[0].selectors[0])
        .collect();
    // `div` — a type selector.
    assert!(matches!(names[0], SimpleSelector::Type { name, .. } if name == "div"));
    // `#top` — an id selector.
    assert!(matches!(names[1], SimpleSelector::Id { name, .. } if name == "top"));
    // `*` — the universal type selector.
    assert!(matches!(names[2], SimpleSelector::Type { name, .. } if name == "*"));
    // `svg|rect` — the namespace is read and IGNORED (official keeps the local
    // name); the span covers the whole `svg|rect`.
    let SimpleSelector::Type { span, name } = names[3] else {
        panic!("a namespaced type selector");
    };
    assert_eq!(name, "rect");
    assert_eq!(at(&source, *span), "svg|rect");
    // `[a]` — no matcher, no value, no flags.
    assert!(matches!(
        names[4],
        SimpleSelector::Attribute { name, matcher: None, value: None, flags: None, .. }
            if name == "a"
    ));
    // `[a=b]` — the `=` matcher, the unquoted value.
    let SimpleSelector::Attribute {
        matcher,
        value,
        span,
        ..
    } = names[5]
    else {
        panic!("an attribute selector");
    };
    assert_eq!(matcher.as_deref(), Some("="));
    assert_eq!(value.as_deref(), Some("b"));
    assert_eq!(at(&source, *span), "[a=b]");
    // `[a~="v w" i]` — the `~=` matcher, the QUOTE-STRIPPED value, the flags.
    let SimpleSelector::Attribute {
        matcher,
        value,
        flags,
        span,
        ..
    } = names[6]
    else {
        panic!("an attribute selector");
    };
    assert_eq!(matcher.as_deref(), Some("~="));
    assert_eq!(value.as_deref(), Some("v w"));
    assert_eq!(flags.as_deref(), Some("i"));
    assert_eq!(at(&source, *span), "[a~=\"v w\" i]");
}

#[test]
fn pseudo_selectors_parse_args_and_span_quirks() {
    let (source, body) =
        component_with_css(":global(.x) ::before { a: b } ::highlight(y) { a: b }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let global = &rule.prelude.children[0].children[0].selectors[0];
    // `:global(.x)` — a pseudo-CLASS whose span INCLUDES the args.
    let SimpleSelector::PseudoClass { span, name, args } = global else {
        panic!("a pseudo-class selector");
    };
    assert_eq!(name, "global");
    assert_eq!(at(&source, *span), ":global(.x)");
    let args = args.as_ref().expect("parenthesized args");
    assert_eq!(at(&source, args.span), ".x");
    assert!(matches!(
        &args.children[0].children[0].selectors[0],
        SimpleSelector::Class { name, .. } if name == "x"
    ));
    // `::before` — a pseudo-ELEMENT.
    let before = &rule.prelude.children[0].children[1].selectors[0];
    let SimpleSelector::PseudoElement { span, name } = before else {
        panic!("a pseudo-element selector");
    };
    assert_eq!(name, "before");
    assert_eq!(at(&source, *span), "::before");
    // `::highlight(y)` — the official node is pushed BEFORE its args are read
    // and discarded, so the span EXCLUDES `(y)`.
    let StyleChild::Rule(second) = &sheet.children[1] else {
        panic!("a style rule");
    };
    let highlight = &second.prelude.children[0].children[0].selectors[0];
    let SimpleSelector::PseudoElement { span, name } = highlight else {
        panic!("a pseudo-element selector");
    };
    assert_eq!(name, "highlight");
    assert_eq!(at(&source, *span), "::highlight");
}

#[test]
fn combinators_split_relative_selectors_with_official_spans() {
    let (source, body) = component_with_css("a > b + c ~ d || e f { g: h }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let complex = &rule.prelude.children[0];
    let combinators: Vec<Option<&str>> = complex
        .children
        .iter()
        .map(|relative| {
            relative
                .combinator
                .as_ref()
                .map(|combinator| combinator.name.as_str())
        })
        .collect();
    assert_eq!(
        combinators,
        vec![None, Some(">"), Some("+"), Some("~"), Some("||"), Some(" ")]
    );
    // An EXPLICIT combinator span covers its token; the relative selector
    // starts AT the combinator.
    let second = &complex.children[1];
    let combinator = second.combinator.as_ref().expect("an explicit combinator");
    assert_eq!(at(&source, combinator.span), ">");
    assert_eq!(at(&source, second.span), "> b");
    // The DESCENDANT combinator span is the whitespace run; the relative
    // selector span starts at that whitespace.
    let last = &complex.children[5];
    let descendant = last.combinator.as_ref().expect("a descendant combinator");
    assert_eq!(descendant.name, " ");
    assert_eq!(at(&source, descendant.span), " ");
    assert_eq!(at(&source, last.span), " f");
}

#[test]
fn nth_and_percentage_selectors_parse_inside_their_positions() {
    let (source, body) = component_with_css(
        ":nth-child(2n+1) { a: b }\n:nth-child(2n+1 of .x) { a: b }\n@keyframes spin { 50% { a: b } }",
    );
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    // `:nth-child(2n+1)` — an Nth arg.
    let StyleChild::Rule(first) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { args, .. } =
        &first.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    let nth = &args.as_ref().expect("args").children[0].children[0].selectors[0];
    let SimpleSelector::Nth { span, value } = nth else {
        panic!("an nth selector");
    };
    assert_eq!(value, "2n+1");
    assert_eq!(at(&source, *span), "2n+1");
    // `:nth-child(2n+1 of .x)` — the official read CONSUMES ` of ` into the
    // value; the `.x` continues the SAME compound.
    let StyleChild::Rule(second) = &sheet.children[1] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { args, .. } =
        &second.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    let compound = &args.as_ref().expect("args").children[0].children[0];
    let SimpleSelector::Nth { value, .. } = &compound.selectors[0] else {
        panic!("an nth selector");
    };
    assert_eq!(value, "2n+1 of ");
    assert!(matches!(
        &compound.selectors[1],
        SimpleSelector::Class { name, .. } if name == "x"
    ));
    // `50%` — a percentage step selector inside the keyframes block.
    let StyleChild::Atrule(keyframes) = &sheet.children[2] else {
        panic!("an at-rule");
    };
    let block = keyframes.block.as_ref().expect("a keyframes block");
    let BlockChild::Rule(step) = &block.children[0] else {
        panic!("a step rule");
    };
    let SimpleSelector::Percentage { span, value } =
        &step.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a percentage selector");
    };
    assert_eq!(value, "50%");
    assert_eq!(at(&source, *span), "50%");
}

#[test]
fn at_rules_parse_name_prelude_and_block_forms() {
    let (source, body) = component_with_css(
        "@import 'nested.css';\n@media (min-width: 100px) {\n\t.a { color: red; }\n}\n@keyframes spin { from { a: b } }",
    );
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    // `@import` — a statement at-rule, no block.
    let StyleChild::Atrule(import) = &sheet.children[0] else {
        panic!("an at-rule");
    };
    assert_eq!(import.name, "import");
    assert_eq!(at(&source, import.name_span), "import");
    assert_eq!(import.prelude, "'nested.css'");
    assert!(import.block.is_none());
    assert_eq!(at(&source, import.span), "@import 'nested.css';");
    // `@media` — a block at-rule with a NESTED rule.
    let StyleChild::Atrule(media) = &sheet.children[1] else {
        panic!("an at-rule");
    };
    assert_eq!(media.name, "media");
    assert_eq!(media.prelude, "(min-width: 100px)");
    let media_block = media.block.as_ref().expect("a media block");
    assert!(matches!(&media_block.children[0], BlockChild::Rule(_)));
    // `@keyframes spin` — the prelude is the raw TRIMMED value; the raw
    // prelude region starts right after the name identifier.
    let StyleChild::Atrule(keyframes) = &sheet.children[2] else {
        panic!("an at-rule");
    };
    assert_eq!(keyframes.name, "keyframes");
    assert_eq!(keyframes.prelude, "spin");
    assert_eq!(at(&source, keyframes.prelude_span), " spin ");
}

#[test]
fn nesting_selector_and_nested_rule_parse() {
    let (source, body) = component_with_css(".a { &:hover { color: red; } }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(outer) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let BlockChild::Rule(nested) = &outer.block.children[0] else {
        panic!("a nested rule");
    };
    let compound = &nested.prelude.children[0].children[0];
    let SimpleSelector::Nesting { span } = &compound.selectors[0] else {
        panic!("a nesting selector");
    };
    assert_eq!(at(&source, *span), "&");
    assert!(matches!(
        &compound.selectors[1],
        SimpleSelector::PseudoClass { name, args: None, .. } if name == "hover"
    ));
}

#[test]
fn declaration_forms_parse_important_custom_property_and_comments() {
    let (source, body) = component_with_css(
        ".a {\n\tcolor: red !important;\n\t--x: ;\n\tborder: 1px /* c */ solid;\n\tpadding: 0\n}",
    );
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let decls: Vec<&Declaration> = rule
        .block
        .children
        .iter()
        .map(|child| match child {
            BlockChild::Declaration(declaration) => declaration,
            other => panic!("a declaration, got {other:?}"),
        })
        .collect();
    assert_eq!(decls[0].property, "color");
    assert_eq!(decls[0].value, "red !important");
    // An EMPTY custom-property value is legal (`--x: ;`).
    assert_eq!(decls[1].property, "--x");
    assert_eq!(decls[1].value, "");
    // A `/* … */` comment inside the value is SKIPPED by the official
    // `read_value` (not part of the value text).
    assert_eq!(decls[2].property, "border");
    assert_eq!(decls[2].value, "1px  solid");
    // The final declaration closes on `}` with no `;` — the official `end`
    // sits at the boundary char, so the span keeps the trailing whitespace
    // the (trimmed) value text drops.
    assert_eq!(decls[3].property, "padding");
    assert_eq!(decls[3].value, "0");
    assert_eq!(at(&source, decls[3].span), "padding: 0\n");
}

#[test]
fn escaped_identifiers_decode_like_official_read_identifier() {
    // `:\67 lobal` decodes the `\67 ` unicode sequence to `g` — the official
    // `read_identifier` resolves escapes, so `:global` detection later operates
    // on DECODED names.
    let (source, body) = component_with_css(":\\67 lobal(.x) { color: red } .a\\.b { color: red }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { name, .. } =
        &rule.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    assert_eq!(name, "global");
    // `.a\.b` — a NON-unicode escape keeps the backslash + char, matching the
    // official `identifier += '\\' + char` branch.
    let StyleChild::Rule(second) = &sheet.children[1] else {
        panic!("a style rule");
    };
    let SimpleSelector::Class { name, .. } = &second.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a class selector");
    };
    assert_eq!(name, "a\\.b");
}

#[test]
fn multibyte_identifiers_carry_byte_correct_spans() {
    // Identifier chars with codepoints ≥ 160 are legal (official
    // `codePointAt >= 160`); spans stay BYTE offsets.
    let (source, body) = component_with_css(".émoji { color: red }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::Class { span, name } = &rule.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a class selector");
    };
    assert_eq!(name, "émoji");
    assert_eq!(at(&source, *span), ".émoji");
}

#[test]
fn empty_body_parses_to_an_empty_stylesheet() {
    let (source, body) = component_with_css("  \n\t/* only a comment */\n  ");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    assert!(sheet.children.is_empty());
    assert_eq!(sheet.span, body);
}

#[test]
fn global_block_form_parses_argless_global() {
    let (source, body) = component_with_css(":global { .x { color: red; } }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { name, args, .. } =
        &rule.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    assert_eq!(name, "global");
    assert!(args.is_none(), "the block form has NO args");
    assert!(matches!(&rule.block.children[0], BlockChild::Rule(_)));
}

#[test]
fn malformed_bodies_return_typed_errors_never_panic() {
    // A digit-leading class name — `css_expected_identifier`.
    let (source, body) = component_with_css(".1bad { color: red }");
    let err = parse_style_body(&source, body).expect_err("a digit-leading identifier fails");
    assert_eq!(err.code, "css_expected_identifier");
    let expected = source.find("1bad").expect("error offset") as u32;
    assert_eq!(
        err.span.start, expected,
        "the error span points at the name"
    );
    // An empty declaration — `css_empty_declaration`.
    let (source, body) = component_with_css(".a { color: }");
    let err = parse_style_body(&source, body).expect_err("an empty declaration fails");
    assert_eq!(err.code, "css_empty_declaration");
    // A combinator immediately before `{` — `css_selector_invalid`.
    let (source, body) = component_with_css(".a > { color: red }");
    let err = parse_style_body(&source, body).expect_err("a trailing combinator fails");
    assert_eq!(err.code, "css_selector_invalid");
    // An unterminated block — the reader runs PAST `</style>` exactly like
    // the official one (the next block-item `read_value` consumes the rest of
    // the source), so the code is `unexpected_eof`.
    let (source, body) = component_with_css(".a { color: red;");
    let err = parse_style_body(&source, body).expect_err("an unterminated block fails");
    assert_eq!(err.code, "unexpected_eof");
    // An unterminated comment — `expected_token` for the missing `*/`.
    let (source, body) = component_with_css("/* never closed");
    let err = parse_style_body(&source, body).expect_err("an unterminated comment fails");
    assert_eq!(err.code, "expected_token");
}

#[test]
fn selector_rewind_keeps_trailing_whitespace_out_of_spans() {
    let (source, body) = component_with_css(".a ,\t.b\n{ color: red }");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    assert_eq!(rule.prelude.children.len(), 2);
    // Each complex selector's span excludes the whitespace before `,` / `{`.
    assert_eq!(at(&source, rule.prelude.children[0].span), ".a");
    assert_eq!(at(&source, rule.prelude.children[1].span), ".b");
    // The selector-LIST span runs from the first selector to the LAST
    // selector's end.
    assert_eq!(at(&source, rule.prelude.span), ".a ,\t.b");
}

// ─── JS-`\s` (Unicode) whitespace parity — the official readers' `\s`-bearing
// scans accept the FULL JS regex whitespace set (NBSP and the other Unicode
// spaces), not just ASCII whitespace. Each test pairs the NBSP form with its
// ASCII-space metamorphic twin: identical parse FACTS where svelte treats the
// two as equivalent. ───────────────────────────────────────────────────────

#[test]
fn declaration_property_scan_stops_at_nbsp_like_js_s() {
    // Official `read_declaration`: `parser.read_until(/[\s:]/)` — JS `\s`
    // includes NBSP (\u{a0}), so the property is `animation`, NOT
    // `animation\u{a0}`.
    let (source, body) = component_with_css(".x{animation\u{a0}: spin 1s}");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let BlockChild::Declaration(decl) = &rule.block.children[0] else {
        panic!("a declaration");
    };
    assert_eq!(decl.property, "animation");
    assert_eq!(decl.value, "spin 1s");
    // Metamorphic twin: the ASCII-space form yields the SAME facts.
    let (ascii_source, ascii_body) = component_with_css(".x{animation : spin 1s}");
    let ascii_sheet = parse_style_body(&ascii_source, ascii_body).expect("clean body parses");
    let StyleChild::Rule(ascii_rule) = &ascii_sheet.children[0] else {
        panic!("a style rule");
    };
    let BlockChild::Declaration(ascii_decl) = &ascii_rule.block.children[0] else {
        panic!("a declaration");
    };
    assert_eq!(
        (&ascii_decl.property, &ascii_decl.value),
        (&decl.property, &decl.value)
    );
}

#[test]
fn unquoted_attribute_value_closes_at_nbsp_like_js_s() {
    // Official `read_attribute_value`: an unquoted value closes on
    // `REGEX_CLOSING_BRACKET = /[\s\]]/` — JS `\s` includes NBSP, so
    // `[data-x=a\u{a0}b]` reads value `a`, then (after `allow_whitespace`
    // skips the NBSP) flags `b`. A byte-ASCII scan would fail open: value
    // `a\u{a0}b`, no flags — different matcher semantics.
    let (source, body) = component_with_css("[data-x=a\u{a0}b]{color:red}");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::Attribute {
        name,
        matcher,
        value,
        flags,
        ..
    } = &rule.prelude.children[0].children[0].selectors[0]
    else {
        panic!("an attribute selector");
    };
    assert_eq!(name, "data-x");
    assert_eq!(matcher.as_deref(), Some("="));
    assert_eq!(value.as_deref(), Some("a"));
    assert_eq!(flags.as_deref(), Some("b"));
    // Metamorphic twin: the ASCII-space form yields the SAME facts.
    let (ascii_source, ascii_body) = component_with_css("[data-x=a b]{color:red}");
    let ascii_sheet = parse_style_body(&ascii_source, ascii_body).expect("clean body parses");
    let StyleChild::Rule(ascii_rule) = &ascii_sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::Attribute {
        value: ascii_value,
        flags: ascii_flags,
        ..
    } = &ascii_rule.prelude.children[0].children[0].selectors[0]
    else {
        panic!("an attribute selector");
    };
    assert_eq!((ascii_value, ascii_flags), (value, flags));
}

#[test]
fn nth_of_scans_accept_nbsp_like_js_s() {
    // Official `REGEX_NTH_OF` uses `\s` (Unicode) in the An+B offset
    // (`\s*[+-]\s*`), the end lookahead (`(?=\s*[,)])`), and the consuming
    // ` of ` arm (`\s+of\s+`) — NBSP counts everywhere.
    let (source, body) = component_with_css(
        ":nth-child(2n\u{a0}+\u{a0}1) { a: b }\n:nth-child(2n\u{a0}of\u{a0}.x) { a: b }\n:nth-child(2n\u{a0}) { a: b }",
    );
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    // The An+B offset: value carries the NBSP bytes verbatim.
    let StyleChild::Rule(first) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { args, .. } =
        &first.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    let SimpleSelector::Nth { value, .. } =
        &args.as_ref().expect("args").children[0].children[0].selectors[0]
    else {
        panic!("an nth selector");
    };
    assert_eq!(value, "2n\u{a0}+\u{a0}1");
    // The consuming ` of ` arm with NBSP runs; `.x` continues the compound.
    let StyleChild::Rule(second) = &sheet.children[1] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { args, .. } =
        &second.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    let compound = &args.as_ref().expect("args").children[0].children[0];
    let SimpleSelector::Nth { value, .. } = &compound.selectors[0] else {
        panic!("an nth selector");
    };
    assert_eq!(value, "2n\u{a0}of\u{a0}");
    assert!(matches!(
        &compound.selectors[1],
        SimpleSelector::Class { name, .. } if name == "x"
    ));
    // The zero-width end lookahead `(?=\s*[,)])` skips NBSP to see `)` —
    // the Nth match is exactly `2n` (the NBSP is NOT consumed).
    let StyleChild::Rule(third) = &sheet.children[2] else {
        panic!("a style rule");
    };
    let SimpleSelector::PseudoClass { args, .. } =
        &third.prelude.children[0].children[0].selectors[0]
    else {
        panic!("a pseudo-class selector");
    };
    let SimpleSelector::Nth { value, .. } =
        &args.as_ref().expect("args").children[0].children[0].selectors[0]
    else {
        panic!("an nth selector");
    };
    assert_eq!(value, "2n");
}

#[test]
fn read_value_trims_the_js_trim_set_not_rust_whitespace() {
    // Official `read_value` ends with `value.trim()` — the JS trim set equals
    // the JS `\s` set: it INCLUDES U+FEFF (ZWNBSP) and EXCLUDES U+0085 (NEL).
    // Rust `str::trim` (Unicode White_Space) diverges in BOTH directions.
    // A trailing FEFF is trimmed (declaration value + at-rule prelude)…
    let (source, body) = component_with_css(".x{color:a\u{feff}}\n@keyframes spin\u{feff} {}");
    let sheet = parse_style_body(&source, body).expect("clean body parses");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let BlockChild::Declaration(decl) = &rule.block.children[0] else {
        panic!("a declaration");
    };
    assert_eq!(decl.value, "a", "a trailing U+FEFF is JS-trimmed");
    let StyleChild::Atrule(keyframes) = &sheet.children[1] else {
        panic!("an at-rule");
    };
    assert_eq!(
        keyframes.prelude, "spin",
        "the at-rule prelude JS-trims a trailing U+FEFF (the keyframes rename list key)"
    );
    // …and a NEL-only value is KEPT (JS trim does not remove U+0085), so the
    // declaration is NOT empty — svelte compiles it; `css_empty_declaration`
    // here would be a fail-closed divergence.
    let (source, body) = component_with_css(".x{color:\u{85}}");
    let sheet = parse_style_body(&source, body).expect("a NEL-only value is non-empty");
    let StyleChild::Rule(rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    let BlockChild::Declaration(decl) = &rule.block.children[0] else {
        panic!("a declaration");
    };
    assert_eq!(decl.value, "\u{85}", "U+0085 survives the JS trim");
}
