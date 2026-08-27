//! Unit tests for the typed Svelte-lenient selector-shape projection helpers
//! (`svelte_percentage_selector_span` / `svelte_nth_of_selector_span`) — the classification a
//! Svelte-side selector projection reads instead of independently pattern-matching raw source
//! text itself. The matchers run at parse time when minting [`CompoundTail`] /
//! [`SvelteNthArg`] / `argument_is_empty`; reject projection reads those stored
//! facts and never re-invokes the matchers.

use std::sync::Arc;

use verter_css_syntax::{
    svelte_nth_of_selector_span, svelte_percentage_selector_span, svelte_reject_from_ir,
    svelte_trailing_type_selector_span, svelte_trim_js_whitespace, ComplexSelectorPart,
    CompoundTail, CssDialect, CssParseMode, CssSource, StyleStatement, SvelteNthArg,
};
use verter_span::Span;

fn source(text: &str) -> CssSource {
    CssSource::new(Arc::from(text), 0).expect("source fits the size budget")
}

fn span(text: &str) -> Span {
    Span::new(0, text.len() as u32)
}

fn parse_style(css: &str) -> verter_css_syntax::StyleSyntaxIr {
    let src = CssSource::new(Arc::from(css), 0).expect("source fits the size budget");
    verter_css_syntax::parse_style_ir(src, CssDialect::Css, CssParseMode::Recover)
        .expect("a lenient style body parses")
}

#[test]
fn percentage_span_matches_whole_integer_percentage() {
    let text = "50%";
    let src = source(text);
    assert_eq!(
        svelte_percentage_selector_span(&src, span(text)),
        Some(Span::new(0, 3))
    );
}

#[test]
fn percentage_span_matches_fractional_percentage() {
    let text = "12.5%";
    let src = source(text);
    assert_eq!(
        svelte_percentage_selector_span(&src, span(text)),
        Some(Span::new(0, 5))
    );
}

#[test]
fn percentage_span_is_a_prefix_when_trailing_bytes_remain() {
    // A keyframe-step compound's raw span can include trailing whitespace inside its region; the
    // helper reports only the matched PREFIX, letting the caller compare it against the whole
    // span.
    let text = "50% ";
    let src = source(text);
    assert_eq!(
        svelte_percentage_selector_span(&src, span(text)),
        Some(Span::new(0, 3))
    );
}

#[test]
fn percentage_span_rejects_a_dangling_fraction_dot() {
    // `5.` has no digit after the `.`, so upstream's `(\.\d+)?` does not match the fraction, and
    // the following byte (`.`, not `%`) fails the mandatory `%` — the WHOLE match fails, not a
    // fallback to matching just `5`.
    let text = "5.%";
    let src = source(text);
    assert_eq!(svelte_percentage_selector_span(&src, span(text)), None);
}

#[test]
fn percentage_span_requires_at_least_one_leading_digit() {
    let text = "%";
    let src = source(text);
    assert_eq!(svelte_percentage_selector_span(&src, span(text)), None);
}

#[test]
fn percentage_span_rejects_non_percentage_text() {
    let text = "from";
    let src = source(text);
    assert_eq!(svelte_percentage_selector_span(&src, span(text)), None);
}

#[test]
fn nth_of_span_matches_even_and_odd_keywords() {
    let text = "even)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 4))
    );
    let text = "odd)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 3))
    );
}

#[test]
fn nth_of_span_matches_plain_an_plus_b() {
    let text = "2n+1)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 4))
    );
}

#[test]
fn nth_of_span_matches_negative_arm_with_mandatory_plus_offset() {
    let text = "-n+2)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 4))
    );
}

#[test]
fn nth_of_span_rejects_negative_arm_without_a_plus_offset() {
    // `-2n-1` has a `-` offset, which the negative arm does not accept (offset is `+`-only).
    let text = "-2n-1)";
    let src = source(text);
    assert_eq!(svelte_nth_of_selector_span(&src, span(text)), None);
}

#[test]
fn nth_of_span_includes_the_consuming_of_arm() {
    let text = "3n + 1 of .a)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 10)),
        "the match includes the ` of ` arm so a `<selector>` can be read after it"
    );
    assert_eq!(&text[..10], "3n + 1 of ");
}

#[test]
fn nth_of_span_matches_of_arm_with_simple_selector_lookahead() {
    // Pinned svelte@5.56.10: `\s+of(\s+|(?=[.#[*:&]))` — `of.x` is a match.
    let text = "2n of.x)";
    let src = source(text);
    assert_eq!(
        svelte_nth_of_selector_span(&src, span(text)),
        Some(Span::new(0, 5)),
        "the match ends after `of` so `.x` is read as a selector"
    );
    assert_eq!(&text[..5], "2n of");
}

#[test]
fn trim_js_whitespace_trims_the_js_whitespace_set() {
    // Ordinary ASCII whitespace, like Rust's `str::trim`.
    assert_eq!(svelte_trim_js_whitespace("  spin  "), "spin");
    // U+FEFF (BOM) is JS `\s` and must trim, unlike Rust's Unicode `White_Space` set.
    assert_eq!(svelte_trim_js_whitespace("\u{FEFF}spin\u{FEFF}"), "spin");
    // U+0085 (NEL) is Unicode `White_Space` but NOT JS `\s`, so it must NOT trim.
    assert_eq!(
        svelte_trim_js_whitespace("\u{0085}spin\u{0085}"),
        "\u{0085}spin\u{0085}"
    );
}

#[test]
fn nth_of_span_rejects_a_normal_selector_list_argument() {
    // `:not(.a, .b)`-shaped arguments must NOT be swallowed as an nth-formula.
    let text = ".a, .b)";
    let src = source(text);
    assert_eq!(svelte_nth_of_selector_span(&src, span(text)), None);
}

// Oracle: `:global(.x)div { color: red; }` compiles to
// `css_type_selector_invalid_placement` (confirmed against the pinned
// `svelte@5.56.10` compiler's `compile()`); `:global(.x) div { ... }` (a
// combinator between them) compiles clean.
#[test]
fn trailing_type_selector_span_matches_a_bare_identifier_with_no_combinator() {
    let text = "div";
    let src = source(text);
    assert_eq!(
        svelte_trailing_type_selector_span(&src, span(text)),
        Some(Span::new(0, 3))
    );
}

#[test]
fn trailing_type_selector_span_accepts_a_leading_hyphen_identifier() {
    // `:global(.x)-x` also compiles to `css_type_selector_invalid_placement`
    // — a leading hyphen alone (not hyphen-then-digit) is a valid CSS
    // identifier shape.
    let text = "-x";
    let src = source(text);
    assert_eq!(
        svelte_trailing_type_selector_span(&src, span(text)),
        Some(Span::new(0, 2))
    );
}

#[test]
fn trailing_type_selector_span_rejects_a_leading_digit() {
    // `:global(.x)0div` fails to PARSE at all (`css_expected_identifier`) —
    // never reaches this classifier's use site, but the classifier itself
    // must not misreport a digit-leading run as an identifier.
    let text = "0div";
    let src = source(text);
    assert_eq!(svelte_trailing_type_selector_span(&src, span(text)), None);
}

#[test]
fn trailing_type_selector_span_rejects_an_empty_span() {
    let text = "div";
    let src = source(text);
    assert_eq!(
        svelte_trailing_type_selector_span(&src, Span::new(0, 0)),
        None
    );
}

#[test]
fn trailing_type_selector_span_rejects_a_partial_match() {
    // A trailing run that is not WHOLLY an identifier (e.g. it ends with a
    // combinator/punctuation byte the caller's span incorrectly widened to
    // include) is not the lenient implicit-type-selector shape.
    let text = "div.x";
    let src = source(text);
    assert_eq!(svelte_trailing_type_selector_span(&src, span(text)), None);
}

// The classifications above feed `SelectorCompound::tail` — decided ONCE by
// the parser when a compound's own node is built, never re-derived by a
// downstream reader. These cases pin that the parser's own compound carries
// the correct classification directly, with no second pass required.

#[test]
fn compound_tail_is_percentage_for_a_keyframe_step_with_zero_components() {
    let ir = parse_style("@keyframes spin { 50% { opacity: 0; } }");
    let StyleStatement::AtRule(atrule) = &ir.statements()[0] else {
        panic!("expected an at-rule");
    };
    let block = atrule.body().expect("a keyframes block");
    let StyleStatement::Rule(step) = &block.statements()[0] else {
        panic!("expected a keyframe-step rule");
    };
    let compound = step.selector_list().selectors()[0].compounds()[0];
    assert!(
        compound.components().is_empty(),
        "the general grammar recognizes no typed component for `50%`"
    );
    assert!(
        matches!(compound.tail(), CompoundTail::Percentage(_)),
        "expected a Percentage classification, got {:?}",
        compound.tail()
    );
}

#[test]
fn compound_tail_is_trailing_identifier_after_a_global_pseudo() {
    // `:global(.x)div` — the general grammar recognizes `:global(.x)` as one
    // component and leaves `div` an unclaimed trailing run.
    let ir = parse_style(":global(.x)div { color: red; }");
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    let compound = rule.selector_list().selectors()[0].compounds()[0];
    assert_eq!(compound.components().len(), 1);
    let tail_span = match compound.tail() {
        CompoundTail::TrailingIdentifier(span) => span,
        other => panic!("expected a TrailingIdentifier classification, got {other:?}"),
    };
    assert_eq!(ir.source().slice(tail_span), "div");
}

#[test]
fn compound_tail_is_claimed_for_an_ordinary_fully_recognized_compound() {
    let ir = parse_style(".card { color: red; }");
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    let compound = rule.selector_list().selectors()[0].compounds()[0];
    assert!(!compound.components().is_empty());
    assert_eq!(compound.tail(), CompoundTail::Claimed);
}

#[test]
fn compound_tail_reports_malformed_leading_dot_for_a_digit_led_class_name() {
    // `.1bad` — the general grammar never recognizes `.` as starting ANY
    // simple selector unless an adjacent `Ident` token follows, so the
    // compound closes with zero components and the whole span unclassified.
    let ir = parse_style(".1bad { color: red; }");
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    let compound = rule.selector_list().selectors()[0].compounds()[0];
    assert!(compound.components().is_empty());
    let starts_with_dot = match compound.tail() {
        CompoundTail::Unclassified {
            starts_with_dot, ..
        } => starts_with_dot,
        other => panic!("expected an Unclassified classification, got {other:?}"),
    };
    assert!(starts_with_dot);
}

// Parse-time fact minting. Assertions that cannot fail independently:
// - the four Formula cases share one match arm; one plant (always-Other /
//   skip `nth_consumes_arg_or_of`) fails them together.
// - the three LeadingHyphenOrDigit cases share one match arm.
// - `starts_with_dot: true` implies `expected_identifier: true` by
//   `svelte_unclassified_expected_identifier`'s constructor; not asserted
//   separately on `.1bad`.
// - empty `:nth-child()` is `SvelteNthArg::Empty`; `argument_is_empty` for
//   that same empty span is the same check and is asserted on `:global()` /
//   `:lang( )` instead.
// Reject-reads-fact is independently proven by planting
// `classify_svelte_nth_arg` / `classify_argument_is_empty` /
// `svelte_unclassified_expected_identifier` and watching the existing
// `svelte_compat_profile` reject-code tests go RED (`None` instead of the
// official code) — if reject still re-read bytes those tests would stay GREEN.

fn first_rule_compound(
    ir: &verter_css_syntax::StyleSyntaxIr,
) -> &verter_css_syntax::SelectorCompound {
    let StyleStatement::Rule(rule) = &ir.statements()[0] else {
        panic!("expected a rule");
    };
    rule.selector_list().selectors()[0]
        .parts()
        .iter()
        .find_map(|part| match part {
            ComplexSelectorPart::Compound(compound) => Some(compound),
            ComplexSelectorPart::Combinator(_) => None,
        })
        .expect("expected a compound")
}

fn first_nth_arg(css: &str) -> SvelteNthArg {
    let ir = parse_style(css);
    first_rule_compound(&ir)
        .components()
        .iter()
        .find_map(|component| component.pseudo())
        .and_then(|pseudo| pseudo.svelte_nth_arg())
        .expect("an nth-child argument fact")
}

fn first_functional_argument_is_empty(css: &str) -> bool {
    let ir = parse_style(css);
    first_rule_compound(&ir)
        .components()
        .iter()
        .find_map(|component| component.pseudo())
        .expect("a functional pseudo")
        .argument_is_empty()
}

#[test]
fn nth_child_formula_is_minted_on_the_pseudo() {
    assert_eq!(
        first_nth_arg("p:nth-child(2n+1) { color: red; }"),
        SvelteNthArg::Formula
    );
    assert_eq!(
        first_nth_arg("p:nth-child(2n+1 of .x) { color: red; }"),
        SvelteNthArg::Formula
    );
    assert_eq!(
        first_nth_arg("p:nth-child(even) { color: red; }"),
        SvelteNthArg::Formula
    );
    assert_eq!(
        first_nth_arg("p:nth-last-child(-2n+1) { color: red; }"),
        SvelteNthArg::Formula
    );
}

#[test]
fn nth_child_negative_integer_mints_leading_hyphen_or_digit() {
    assert_eq!(
        first_nth_arg("p:nth-child(-2) { color: red; }"),
        SvelteNthArg::LeadingHyphenOrDigit
    );
    assert_eq!(
        first_nth_arg("p:nth-child(-2n) { color: red; }"),
        SvelteNthArg::LeadingHyphenOrDigit
    );
    assert_eq!(
        first_nth_arg("p:nth-child(2n+) { color: red; }"),
        SvelteNthArg::LeadingHyphenOrDigit
    );
}

#[test]
fn nth_child_identifier_argument_mints_trailing_identifier() {
    assert_eq!(
        first_nth_arg("p:nth-child(foo) { color: red; }"),
        SvelteNthArg::TrailingIdentifier
    );
}

#[test]
fn nth_child_n_plus_mints_other() {
    assert_eq!(
        first_nth_arg("p:nth-child(n+) { color: red; }"),
        SvelteNthArg::Other
    );
}

#[test]
fn empty_nth_child_mints_empty() {
    assert_eq!(
        first_nth_arg("p:nth-child() { color: red; }"),
        SvelteNthArg::Empty
    );
}

#[test]
fn global_and_lang_empty_arguments_mint_argument_is_empty() {
    assert!(first_functional_argument_is_empty(
        ":global() { color: red; }"
    ));
    assert!(first_functional_argument_is_empty(
        ":global( ) { color: red; }"
    ));
    assert!(first_functional_argument_is_empty(
        ":global(/**/) { color: red; }"
    ));
    assert!(first_functional_argument_is_empty(
        ":lang( ) { color: red; }"
    ));
    assert!(!first_functional_argument_is_empty(
        ":lang(en) { color: red; }"
    ));
    assert!(!first_functional_argument_is_empty(
        ":foo(.a) { color: red; }"
    ));
}

#[test]
fn unclassified_at_and_hash_delims_mint_expected_identifier() {
    let at_ir = parse_style("@ { color: red; }");
    let at_compound = first_rule_compound(&at_ir);
    match at_compound.tail() {
        CompoundTail::Unclassified {
            expected_identifier: true,
            starts_with_dot: false,
            ..
        } => {}
        other => panic!("expected unclassified expected_identifier for `@ {{}}`, got {other:?}"),
    }
    let hash_ir = parse_style("# { color: red; }");
    let hash_compound = first_rule_compound(&hash_ir);
    match hash_compound.tail() {
        CompoundTail::Unclassified {
            expected_identifier: true,
            starts_with_dot: false,
            ..
        } => {}
        other => panic!("expected unclassified expected_identifier for `# {{}}`, got {other:?}"),
    }
    let digit_ir = parse_style("1px { color: red; }");
    let digit_compound = first_rule_compound(&digit_ir);
    match digit_compound.tail() {
        CompoundTail::Unclassified {
            expected_identifier: true,
            ..
        } => {}
        other => panic!("expected unclassified expected_identifier for `1px`, got {other:?}"),
    }
}

#[test]
fn reject_follows_the_minted_nth_arg_fact() {
    // Discriminates "reject reads the stored fact": forcing
    // `classify_svelte_nth_arg` to always return `Formula` makes
    // `:nth-child(-2)` project `None` instead of `css_expected_identifier`.
    let ir = parse_style("p:nth-child(-2) { color: red; }");
    let compound = first_rule_compound(&ir);
    assert_eq!(
        compound
            .components()
            .iter()
            .find_map(|component| component.pseudo())
            .and_then(|pseudo| pseudo.svelte_nth_arg()),
        Some(SvelteNthArg::LeadingHyphenOrDigit)
    );
    assert_eq!(svelte_reject_from_ir(&ir), Some("css_expected_identifier"));
}

#[test]
fn reject_follows_the_minted_trailing_identifier_fact() {
    // Discriminates "reject reads the stored trailing-identifier fact":
    // forcing `classify_svelte_nth_arg` to always return `Other` makes
    // `:nth-child(foo)` project `css_selector_invalid` instead of `None`.
    let ir = parse_style("p:nth-child(foo) { color: red; }");
    let compound = first_rule_compound(&ir);
    assert_eq!(
        compound
            .components()
            .iter()
            .find_map(|component| component.pseudo())
            .and_then(|pseudo| pseudo.svelte_nth_arg()),
        Some(SvelteNthArg::TrailingIdentifier)
    );
    assert_eq!(svelte_reject_from_ir(&ir), None);
}

#[test]
fn reject_follows_the_minted_unclassified_expected_identifier_flag() {
    // Discriminates "reject reads the stored tail flag": forcing
    // `svelte_unclassified_expected_identifier` to always return `false`
    // makes `@ {}` project `None` instead of `css_expected_identifier`.
    let ir = parse_style("@ { color: red; }");
    let compound = first_rule_compound(&ir);
    match compound.tail() {
        CompoundTail::Unclassified {
            expected_identifier: true,
            ..
        } => {}
        other => panic!("expected expected_identifier, got {other:?}"),
    }
    assert_eq!(svelte_reject_from_ir(&ir), Some("css_expected_identifier"));
}

#[test]
fn style_syntax_ir_clone_keeps_bump_backed_nodes_readable() {
    let ir = parse_style(".card { color: red; }");
    let kept = ir.clone();
    drop(ir);
    let compound = first_rule_compound(&kept);
    assert_eq!(compound.components().len(), 1);
    assert_eq!(compound.tail(), CompoundTail::Claimed);
}

#[test]
fn bump_backed_compound_cannot_clone_out_of_the_ir() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/compile-fail/bump_backed_node_clone_outlives_ir.rs");
}
