//! Unit tests for the CSS scoping analysis — each pinned against the official
//! `svelte@5.56.3` `css-analyze.js` / `css/utils.js` / `css.js` behavior:
//! keyframes collection (with the `-global-` / `:global {}` exclusions),
//! `is_global` / `is_global_like` / `is_global_block` metadata, `has_global`,
//! and the `:global` / nesting placement validation family.

use crate::svelte::runtime::css::analyze::{analyze_stylesheet, is_outer_global, CssAnalysis};
use crate::svelte::runtime::css::parse::parse_style_body;
use crate::svelte::runtime::css::types::{BlockChild, Rule, StyleChild, StyleSheet};
use verter_span::Span;

/// Parse + analyze `css` wrapped in a component (absolute spans), returning
/// `(source, analyzed stylesheet, analysis)`.
fn analyzed(css: &str) -> (String, StyleSheet, CssAnalysis) {
    let source = format!("<div>x</div>\n<style>{css}</style>\n");
    let start = source.find("<style>").expect("open tag") + "<style>".len();
    let end = source.rfind("</style>").expect("close tag");
    let mut sheet = parse_style_body(&source, Span::new(start as u32, end as u32))
        .expect("the fixture body parses");
    let analysis = analyze_stylesheet(&source, &mut sheet).expect("the fixture analyzes clean");
    (source, sheet, analysis)
}

/// Parse + analyze `css`, expecting an analysis ERROR; returns `(source, code,
/// span)`.
fn analysis_error(css: &str) -> (String, &'static str, Span) {
    let source = format!("<div>x</div>\n<style>{css}</style>\n");
    let start = source.find("<style>").expect("open tag") + "<style>".len();
    let end = source.rfind("</style>").expect("close tag");
    let mut sheet = parse_style_body(&source, Span::new(start as u32, end as u32))
        .expect("the fixture body parses");
    let err = analyze_stylesheet(&source, &mut sheet).expect_err("the fixture fails analysis");
    (source, err.code, err.span)
}

/// The first rule of the sheet.
fn first_rule(sheet: &StyleSheet) -> &Rule {
    match &sheet.children[0] {
        StyleChild::Rule(rule) => rule,
        other => panic!("a style rule, got {other:?}"),
    }
}

// ── keyframes collection ─────────────────────────────────────────────────────

#[test]
fn local_keyframes_names_are_collected_with_prelude_spans() {
    let (source, _, analysis) =
        analyzed("@keyframes spin { from { opacity: 0 } }\n@-webkit-keyframes wk { }");
    let names: Vec<&str> = analysis
        .keyframes
        .iter()
        .map(|keyframe| keyframe.name.as_str())
        .collect();
    // A `-webkit-` browser prefix on the AT-RULE NAME still detects keyframes
    // (the official `remove_css_prefix` rule).
    assert_eq!(names, vec!["spin", "wk"]);
    let spin = &analysis.keyframes[0];
    assert_eq!(
        &source[spin.name_span.start as usize..spin.name_span.end as usize],
        "spin"
    );
    assert!(!analysis.has_global);
    assert!(analysis.global_keyframes.is_empty());
}

#[test]
fn global_prefixed_keyframes_are_excluded_and_recorded_for_prefix_strip() {
    let (source, _, analysis) = analyzed("@keyframes -global-fly { from { opacity: 0 } }");
    // `-global-` names never enter the RENAME list…
    assert!(analysis.keyframes.is_empty());
    // …but are recorded for the PREFIX STRIP, name sans prefix, span covering
    // the full prefixed token.
    assert_eq!(analysis.global_keyframes.len(), 1);
    let global = &analysis.global_keyframes[0];
    assert_eq!(global.name, "fly");
    assert_eq!(
        &source[global.name_span.start as usize..global.name_span.end as usize],
        "-global-fly"
    );
    // A top-level `-global-` keyframes flips `has_global` (the official
    // `is_unscoped` over an empty rule path).
    assert!(analysis.has_global);
}

#[test]
fn keyframes_inside_a_global_block_are_excluded_from_both_lists() {
    let (_, sheet, analysis) = analyzed(":global { @keyframes k { from { opacity: 0 } } }");
    assert!(analysis.keyframes.is_empty());
    assert!(analysis.global_keyframes.is_empty());
    // The `:global {}` block rule carries NO declarations, so `has_global`
    // stays false (the official declaration-count gate).
    assert!(!analysis.has_global);
    assert!(first_rule(&sheet).metadata.is_global_block);
}

// ── selector metadata + has_global ──────────────────────────────────────────

#[test]
fn global_selector_rule_sets_metadata_and_has_global() {
    let (_, sheet, analysis) = analyzed(":global(.x) { color: red; }");
    let rule = first_rule(&sheet);
    let complex = &rule.prelude.children[0];
    assert!(complex.metadata.is_global);
    // A global selector is used by definition.
    assert!(complex.metadata.used);
    assert!(complex.children[0].metadata.is_global);
    assert!(rule.metadata.has_global_selectors);
    assert!(!rule.metadata.has_local_selectors);
    // A global-selector rule WITH declarations flips the component-wide
    // `has_global`.
    assert!(analysis.has_global);
}

#[test]
fn local_selectors_stay_unmarked_and_unused_before_matching() {
    let (_, sheet, analysis) = analyzed(".card { color: blue; }\nh2 { font-weight: bold; }");
    for child in &sheet.children {
        let StyleChild::Rule(rule) = child else {
            panic!("a style rule");
        };
        let complex = &rule.prelude.children[0];
        assert!(!complex.metadata.is_global);
        // `used` is the MATCHER's fact; analysis leaves local selectors
        // unmarked.
        assert!(!complex.metadata.used);
        assert!(rule.metadata.has_local_selectors);
        assert!(!rule.metadata.has_global_selectors);
    }
    assert!(!analysis.has_global);
}

#[test]
fn global_with_trailing_scoped_class_is_not_global() {
    // `:global(.x).y` — the `.y` keeps the selector SCOPED (the official
    // `is_global` every()-gate over unscoped pseudo-classes / pseudo-elements).
    let (_, sheet, _) = analyzed(":global(.x).y { color: red; }");
    let rule = first_rule(&sheet);
    assert!(!rule.prelude.children[0].children[0].metadata.is_global);
    assert!(rule.metadata.has_local_selectors);
}

#[test]
fn global_with_trailing_unscoped_pseudo_class_stays_global() {
    // `:global(.x):hover` — `:hover` is an unscoped pseudo-class, so the
    // compound stays global.
    let (_, sheet, _) = analyzed(":global(.x):hover { color: red; }");
    let rule = first_rule(&sheet);
    assert!(rule.prelude.children[0].children[0].metadata.is_global);
    assert!(rule.metadata.has_global_selectors);
}

#[test]
fn global_with_scoped_has_is_not_global_but_is_outer_global() {
    // `:global(.x):has(.y)` — `:has` re-scopes the compound (`is_global`
    // false) while `is_outer_global` stays true (the official distinction).
    let (_, sheet, _) = analyzed(":global(.x):has(.y) { color: red; }");
    let rule = first_rule(&sheet);
    let relative = &rule.prelude.children[0].children[0];
    assert!(!relative.metadata.is_global);
    assert!(is_outer_global(relative));
}

#[test]
fn host_root_and_view_transition_are_global_like() {
    let (_, sheet, _) = analyzed(":host { color: red; }\n:root { color: red; }\n::view-transition { color: red; }\n:root:has(.x) { color: red; }");
    let global_like: Vec<bool> = sheet
        .children
        .iter()
        .map(|child| match child {
            StyleChild::Rule(rule) => rule.prelude.children[0].children[0].metadata.is_global_like,
            other => panic!("a style rule, got {other:?}"),
        })
        .collect();
    // `:root:has(.x)` is NOT global-like — the `:has(...)` contents must stay
    // scoped (the official `:has` exception).
    assert_eq!(global_like, vec![true, true, true, false]);
    // A global-LIKE selector is marked used by the analysis walk.
    let StyleChild::Rule(host_rule) = &sheet.children[0] else {
        panic!("a style rule");
    };
    assert!(host_rule.prelude.children[0].metadata.used);
}

#[test]
fn global_block_descendant_marks_trailing_selectors_global_like() {
    // `:global x { … }` — the block form; the trailing `x` compound is
    // global-LIKE (unscoped) per the official rule-loop marking.
    let (_, sheet, _) = analyzed(":global x { color: red; }");
    let rule = first_rule(&sheet);
    assert!(rule.metadata.is_global_block);
    let complex = &rule.prelude.children[0];
    assert!(complex.children[1].metadata.is_global_like);
    assert!(complex.metadata.is_global);
}

#[test]
fn nesting_selector_inside_global_rule_marks_used() {
    // `&:hover` nested in `:global(.foo) { … }` is marked used (the official
    // nesting-scope rule).
    let (_, sheet, _) = analyzed(":global(.foo) { &:hover { color: green; } }");
    let outer = first_rule(&sheet);
    let BlockChild::Rule(nested) = &outer.block.children[0] else {
        panic!("a nested rule");
    };
    assert!(nested.metadata.is_nested);
    assert!(nested.prelude.children[0].metadata.used);
}

// ── `:global` placement validation (the official `e.css_*` family) ──────────

#[test]
fn argless_global_inside_a_pseudo_class_is_invalid() {
    let (_, code, _) = analysis_error(":is(:global) { color: red; }");
    assert_eq!(code, "css_global_block_invalid_placement");
}

#[test]
fn global_in_the_middle_of_a_selector_is_invalid() {
    let (source, code, span) = analysis_error(".a :global(.x) .b { color: red; }");
    assert_eq!(code, "css_global_invalid_placement");
    assert_eq!(
        &source[span.start as usize..span.end as usize],
        ":global(.x)"
    );
}

#[test]
fn global_at_selector_edges_is_valid() {
    // Leading and trailing `:global(...)` are both legal; consecutive
    // `:global(...)` runs too.
    let (_, sheet, _) = analyzed(
        ":global(.x) .a { color: red; }\n.a :global(.x) { color: red; }\n:global(.x) :global(.y) { color: red; }",
    );
    assert_eq!(sheet.children.len(), 3);
}

#[test]
fn global_element_arg_not_first_in_compound_is_invalid() {
    let (_, code, _) = analysis_error(".x:global(div) { color: red; }");
    assert_eq!(code, "css_global_invalid_selector_list");
}

#[test]
fn type_selector_after_global_arg_is_invalid() {
    let (source, code, span) = analysis_error(":global(.x)div { color: red; }");
    assert_eq!(code, "css_type_selector_invalid_placement");
    assert_eq!(&source[span.start as usize..span.end as usize], "div");
}

#[test]
fn multi_selector_global_arg_in_a_complex_selector_is_invalid() {
    // A multi-arg `:global(.x, .y)` is only legal STANDALONE; inside a larger
    // complex selector it is invalid.
    let (_, code, _) = analysis_error(".a :global(.x, .y) { color: red; }");
    assert_eq!(code, "css_global_invalid_selector");
    // The standalone form analyzes clean.
    let (_, sheet, _) = analyzed(":global(.x, .y) { color: red; }");
    assert_eq!(sheet.children.len(), 1);
}

#[test]
fn leading_combinator_is_invalid_at_top_level_only() {
    let (source, code, span) = analysis_error("> .a { color: red; }");
    assert_eq!(code, "css_selector_invalid");
    assert_eq!(&source[span.start as usize..span.end as usize], ">");
    // …but is LEGAL inside `:has(...)` args and inside a nested rule.
    let (_, sheet, _) = analyzed(".x:has(> .a) { color: red; }\n.b { > .c { color: red; } }");
    assert_eq!(sheet.children.len(), 2);
}

#[test]
fn nesting_selector_outside_a_nested_rule_is_invalid() {
    let (_, code, _) = analysis_error("& { color: red; }");
    assert_eq!(code, "css_nesting_selector_invalid_placement");
    // The one legal top-level form: a lone `:global(&)`.
    let (_, sheet, _) = analyzed(":global(&) { color: red; }");
    assert_eq!(sheet.children.len(), 1);
}

#[test]
fn global_block_modifier_forms_are_invalid() {
    // `:global.x {}` — a modifier directly on the block-form `:global` start.
    let (_, code, _) = analysis_error(":global.x { color: red; }");
    assert_eq!(code, "css_global_block_invalid_modifier_start");
    // `.x:global {}` — the block-form `:global` not first in its compound.
    let (_, code, _) = analysis_error(".x:global { color: red; }");
    assert_eq!(code, "css_global_block_invalid_modifier");
    // `:global { &.foo {} }` — a nesting modifier inside a lone global block.
    let (_, code, _) = analysis_error(":global { &.foo { color: red; } }");
    assert_eq!(code, "css_global_block_invalid_modifier_start");
}

#[test]
fn global_block_with_non_descendant_combinator_is_invalid() {
    let (_, code, _) = analysis_error(".x > :global { color: red; }");
    assert_eq!(code, "css_global_block_invalid_combinator");
    // The DESCENDANT form is legal.
    let (_, sheet, _) = analyzed(".x :global { color: red; }");
    let rule = first_rule(&sheet);
    assert!(rule.metadata.is_global_block);
}

#[test]
fn lone_global_block_with_a_direct_declaration_is_invalid() {
    let (_, code, _) = analysis_error(":global { color: red; }");
    assert_eq!(code, "css_global_block_invalid_declaration");
    // A lone `:global` with only NESTED RULES is legal.
    let (_, sheet, _) = analyzed(":global { .x { color: red; } }");
    assert!(first_rule(&sheet).metadata.is_global_block);
}

#[test]
fn global_block_selector_lists_must_be_all_global() {
    // A lone `:global` in a MULTI-selector prelude is invalid.
    let (_, code, _) = analysis_error(":global, .x { .y { color: red; } }");
    assert_eq!(code, "css_global_block_invalid_list");
    // A global-block prelude mixed with a NON-global-block selector is
    // invalid too (the `:global x, .y` form).
    let (_, code, _) = analysis_error(":global x, .y { color: red; }");
    assert_eq!(code, "css_global_block_invalid_list");
    // Preprocessor-shaped `:global x, :global y { … }` is LEGAL.
    let (_, sheet, _) = analyzed(":global x, :global y { color: red; }");
    assert!(first_rule(&sheet).metadata.is_global_block);
}

#[test]
fn scoped_styles_fixture_css_analyzes_clean_and_local() {
    // The committed `css/scoped_styles.svelte` body: three local rules, no
    // globals, no keyframes.
    let (_, sheet, analysis) = analyzed(
        "\n\t.card {\n\t\tcolor: blue;\n\t\tpadding: 1rem;\n\t}\n\n\t.card.active {\n\t\tcolor: green;\n\t}\n\n\th2 {\n\t\tfont-weight: bold;\n\t}\n",
    );
    assert_eq!(sheet.children.len(), 3);
    assert!(analysis.keyframes.is_empty());
    assert!(!analysis.has_global);
    for child in &sheet.children {
        let StyleChild::Rule(rule) = child else {
            panic!("a style rule");
        };
        assert!(rule.metadata.has_local_selectors);
        assert!(!rule.metadata.has_global_selectors);
    }
}
