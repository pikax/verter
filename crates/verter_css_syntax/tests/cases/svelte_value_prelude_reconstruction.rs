//! Oracle-grounded tests for [`svelte_read_value_text`] /
//! [`svelte_first_significant_value_span`] — the compat text-reconstruction
//! functions a Svelte-side `analyze`/`render` convergence reads a
//! declaration value / at-rule prelude / keyframe-name span through, instead
//! of a second raw-source scan.
//!
//! Every expected string below was captured directly from the pinned
//! `svelte@5.56.10` compiler's own `parse()` output (`ast.css` — the
//! official CSS AST), NOT hand-derived from reading `read_value`'s source —
//! see the reproduction script in the accompanying commit for the exact
//! invocations. In particular this pins the confirmed divergence the J1 row-5
//! review found: the general CSS grammar recognizes `url(`
//! case-INsensitively, but upstream's own `read_value` switches into its
//! "inside `url(...)`" no-comment-strip mode on a literal, case-SENSITIVE
//! `value.ends_with("url")` byte check — so `URL(/*c*/x)` strips the comment
//! (case mismatch keeps it OUT of url mode) while `url(/*c*/x)` preserves it
//! verbatim (byte match puts it IN url mode).

use std::sync::Arc;

use verter_css_syntax::{
    svelte_first_significant_value_span, svelte_read_value_text, CssDialect, CssParseMode,
    CssSource, StyleStatement,
};

fn parse(css: &str) -> verter_css_syntax::StyleSyntaxIr {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    verter_css_syntax::parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap()
}

fn first_declaration_value_text(css: &str) -> String {
    let ir = parse(css);
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    let StyleStatement::Declaration(declaration) = &rule.body().statements()[0] else {
        panic!("expected a declaration");
    };
    svelte_read_value_text(ir.source(), declaration.value().span())
}

#[test]
fn uppercase_url_strips_an_embedded_comment_lowercase_url_preserves_it() {
    // `URL(...)`: the case-sensitive `ends_with("url")` check fails, so the
    // reader is NOT in url-mode and the `/* … */` IS stripped.
    assert_eq!(
        first_declaration_value_text(".a{opacity:URL(/*x*/foo)}"),
        "URL(foo)"
    );
    // `url(...)`: the check matches, url-mode suppresses the comment-skip,
    // so the comment survives verbatim in the reconstructed value.
    assert_eq!(
        first_declaration_value_text(".a{background:url(/*y*/bar.png)}"),
        "url(/*y*/bar.png)"
    );
}

#[test]
fn a_css_escape_spelling_url_never_enters_url_mode() {
    // `\75rl(...)` (a CSS escape for `u`) accumulates as literal escape text
    // — `ends_with("url")` never matches escape-spelled text, so the
    // `/* … */` inside is stripped exactly like a non-url function call.
    assert_eq!(
        first_declaration_value_text(r".a{background:\75rl(/*z*/x.png)}"),
        r"\75rl(x.png)"
    );
}

#[test]
fn backslash_escapes_are_re_encoded_not_decoded() {
    // Two literal backslashes: each is re-emitted as `\` + the following
    // character (a pass-through pair), never collapsed/decoded.
    assert_eq!(
        first_declaration_value_text(r#".a{content:"\\x"}"#),
        r#""\\x""#
    );
}

#[test]
fn an_escaped_raw_newline_inside_a_quoted_value_is_preserved() {
    let css = ".a{content:\"x\\\ny\"}";
    assert_eq!(first_declaration_value_text(css), "\"x\\\ny\"");
}

#[test]
fn a_raw_unescaped_newline_inside_a_quoted_value_is_preserved() {
    // Unlike a CSS Syntax Module string token (`BadString` on a raw
    // newline), upstream's own reader keeps scanning through an embedded
    // literal newline inside quotes.
    let css = ".a{content:\"x\ny\"}";
    assert_eq!(first_declaration_value_text(css), "\"x\ny\"");
}

#[test]
fn keyframes_name_span_skips_a_leading_comment_the_old_byte_scan_would_misread() {
    // A skip-spaces-then-collect-to-space byte scan over the raw prelude
    // text would stop at the FIRST space, landing on `/*` — the shared
    // typed-value lookup instead walks the already-parsed prelude's value
    // list and finds the real `spin` token, skipping the leading comment
    // value entirely.
    let ir = parse("@keyframes /* c */ spin { from { opacity: 0 } }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    let name_span =
        svelte_first_significant_value_span(atrule.opaque_args()).expect("a significant value");
    assert_eq!(ir.source().slice(name_span), "spin");

    let prelude_text = svelte_read_value_text(ir.source(), atrule.opaque_args().span());
    assert_eq!(
        prelude_text, "spin",
        "the comment is stripped from the trimmed prelude, matching the oracle's `Atrule.prelude`"
    );
}

#[test]
fn keyframes_name_span_skips_a_trailing_comment_too() {
    let ir = parse("@keyframes /*a*/ spin /*b*/ { from { opacity: 0 } }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    let name_span =
        svelte_first_significant_value_span(atrule.opaque_args()).expect("a significant value");
    assert_eq!(ir.source().slice(name_span), "spin");
}

#[test]
fn at_rule_prelude_with_an_embedded_comment_collapses_to_the_oracle_trimmed_text() {
    // Oracle: `@media screen /* x */ and (min-width:1px) {}` → prelude
    // `"screen  and (min-width:1px)"` (the comment contributes zero bytes;
    // the surrounding single spaces on EITHER side are both retained, so the
    // trimmed text has a doubled space where the comment used to be).
    let ir = parse("@media screen /* x */ and (min-width:1px) { }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    let prelude_text = svelte_read_value_text(ir.source(), atrule.opaque_args().span());
    assert_eq!(prelude_text, "screen  and (min-width:1px)");
}

#[test]
fn atrule_prelude_text_is_decided_by_the_parser_at_build_time() {
    // The prelude reconstruction is decided ONCE, by the parser, when the
    // at-rule's own `StyleDirective` node is built — `StyleDirective::prelude_text`
    // is a straight field read, never a second `svelte_read_value_text` call
    // over the same span.
    let ir = parse("@media screen /* x */ and (min-width:1px) { }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    assert_eq!(atrule.prelude_text(), "screen  and (min-width:1px)");
}

#[test]
fn first_significant_value_span_is_none_for_an_entirely_trivial_prelude() {
    let ir = parse("@media /* only comments */ { }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    assert_eq!(
        svelte_first_significant_value_span(atrule.opaque_args()),
        None
    );
}
