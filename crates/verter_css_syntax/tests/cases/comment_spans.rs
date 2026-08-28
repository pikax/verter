//! `StyleSyntaxIr::comment_spans_in` — the retained comment-span inventory a
//! consumer (a Svelte-side renderer escaping an embedded comment's close
//! before wrapping a rule) queries instead of re-lexing the source for
//! comment/string state itself.

use std::sync::Arc;

use verter_css_syntax::{CombinatorKind, CssDialect, CssParseMode, CssSource, StyleStatement};
use verter_span::Span;

fn parse(css: &str) -> verter_css_syntax::StyleSyntaxIr {
    let source = CssSource::new(Arc::from(css), 0).unwrap();
    verter_css_syntax::parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap()
}

#[test]
fn comment_spans_in_finds_a_comment_fully_contained_in_range() {
    let css = ".a { /* note */ color: red; }";
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(ir.source().slice(found[0]), "/* note */");
}

#[test]
fn comment_spans_in_excludes_a_comment_outside_the_queried_range() {
    let css = "/* before */ .a { color: red; }";
    let ir = parse(css);
    let rule_start = css.find(".a").unwrap() as u32;
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(rule_start, css.len() as u32))
        .collect();
    assert!(
        found.is_empty(),
        "a comment entirely before the queried range must not be returned"
    );
}

#[test]
fn comment_spans_in_never_reports_a_comment_shaped_string_literal() {
    // `/*b*/` inside a string is string content, never a real comment — the
    // tokenizer never emits a comment token for it, so the retained
    // inventory must not report one either.
    let css = r#".a { content: "x/*b*/y"; }"#;
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    assert!(
        found.is_empty(),
        "a string literal must never be misreported as a comment: {found:?}"
    );
}

#[test]
fn comment_spans_in_finds_every_comment_in_source_order() {
    let css = "/* a */ .x { color: red; } /* b */ .y { color: blue; }";
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    let texts: Vec<&str> = found.iter().map(|span| ir.source().slice(*span)).collect();
    assert_eq!(texts, vec!["/* a */", "/* b */"]);
}

#[test]
fn comment_spans_in_finds_a_comment_inside_a_selector_prelude() {
    // A comment between two compounds is part of the SELECTOR prelude, not
    // the declaration block. The retained inventory is a whole-source fact:
    // a consumer wrapping the whole rule queries the rule's span and must
    // see this comment, or the wrap it emits terminates at this comment's
    // own close.
    let css = ".a /* note */ .b { color: red; }";
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    let texts: Vec<&str> = found.iter().map(|span| ir.source().slice(*span)).collect();
    assert_eq!(texts, vec!["/* note */"]);
}

#[test]
fn comment_spans_in_finds_a_comment_inside_a_functional_pseudo_prelude() {
    let css = ":is(.a /* n */ , .b) { color: red; }";
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    let texts: Vec<&str> = found.iter().map(|span| ir.source().slice(*span)).collect();
    assert_eq!(texts, vec!["/* n */"]);
}

#[test]
fn comment_spans_stay_in_source_order_across_prelude_and_block() {
    // The inventory is binary-searched by `comment_spans_in`, so a prelude
    // comment must land at its TRUE source position, not appended after the
    // block comments that the parser happened to observe first.
    let css = "/* a */ .x /* b */ .y { /* c */ color: red; } /* d */";
    let ir = parse(css);
    let found: Vec<Span> = ir
        .comment_spans_in(Span::new(0, css.len() as u32))
        .collect();
    let texts: Vec<&str> = found.iter().map(|span| ir.source().slice(*span)).collect();
    assert_eq!(texts, vec!["/* a */", "/* b */", "/* c */", "/* d */"]);
    assert!(
        found.windows(2).all(|pair| pair[0].start <= pair[1].start),
        "the retained inventory must be sorted by start: {found:?}"
    );
}

#[test]
fn unpaired_cdo_span_is_cleared_by_a_cdc_inside_a_selector_prelude() {
    // `<!--` opens at stylesheet top level and the matching `-->` is consumed
    // as part of the following rule's selector prelude. The CDO is PAIRED, so
    // no unpaired-CDO fact survives.
    let css = "<!-- c --> .a { color: red; }";
    let ir = parse(css);
    assert_eq!(
        ir.unpaired_cdo_span(),
        None,
        "a `-->` inside a selector prelude still closes the `<!--`"
    );
}

#[test]
fn unpaired_cdo_span_reports_a_cdo_opened_inside_a_selector_prelude() {
    let css = ".a <!-- .b { color: red; }";
    let ir = parse(css);
    let span = ir
        .unpaired_cdo_span()
        .expect("an unclosed `<!--` in a selector prelude is still an unpaired CDO");
    assert_eq!(ir.source().slice(span), "<!--");
}

#[test]
fn style_ir_keeps_descendant_combinator_distinct_from_compound_classes() {
    let descendant = parse(".a .b { color: red; }");
    let compound = parse(".a.b { color: red; }");

    let StyleStatement::Rule(descendant_rule) = &descendant.statements()[0] else {
        panic!("descendant stylesheet must start with a rule");
    };
    let StyleStatement::Rule(compound_rule) = &compound.statements()[0] else {
        panic!("compound stylesheet must start with a rule");
    };
    let descendant_selector = &descendant_rule.selector_list().selectors()[0];
    let compound_selector = &compound_rule.selector_list().selectors()[0];

    assert_eq!(descendant_selector.compounds().len(), 2);
    assert_eq!(descendant_selector.combinators().len(), 1);
    assert_eq!(
        descendant_selector.combinators()[0].kind(),
        CombinatorKind::Descendant
    );
    assert_eq!(
        descendant
            .source()
            .slice(descendant_selector.combinators()[0].span()),
        " "
    );

    assert_eq!(compound_selector.compounds().len(), 1);
    assert!(
        compound_selector.combinators().is_empty(),
        "`.a.b` is one compound, not a descendant pair: {:?}",
        compound_selector
            .combinators()
            .iter()
            .map(|c| c.kind())
            .collect::<Vec<_>>()
    );
}

#[test]
fn style_ir_keeps_comment_spans_when_whitespace_is_not_an_ir_token() {
    let ir = parse("/* a */ .x { color: red; /* b */ }");
    let texts: Vec<&str> = ir
        .comment_spans_in(Span::new(0, ir.source().text().len() as u32))
        .map(|span| ir.source().slice(span))
        .collect();
    assert_eq!(texts, vec!["/* a */", "/* b */"]);
}
