//! Selector-to-template matcher tests. Every construct asserts BOTH a
//! should-match (selector used / element scoped) and a should-NOT-match
//! (selector unused / element not scoped) so a wrong decision in either
//! direction fails.

use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::css::build_style_scope_plan;
use crate::svelte::runtime::css::types::{
    Block, BlockChild, CssMode, SelectorList, SimpleSelector, StyleChild,
};
use crate::svelte::runtime::expr::MatcherExpr;
use crate::svelte::runtime::ir::{IrNode, SpecialKind};
use crate::svelte::runtime::{lower_parsed_svelte_to_ir, SvelteRuntimeOptions};
use oxc_allocator::Allocator;
use verter_span::Span;

/// The extracted matcher facts of one component: per-selector used verdicts
/// (keyed by selector source text, EVERY complex selector including nested
/// pseudo-class arguments), the scoped selector texts, and the scoped element
/// tag names (multiset, sorted).
struct Facts {
    used: Vec<(String, bool)>,
    scoped_selectors: Vec<String>,
    scoped_tags: Vec<String>,
    unprovable: Option<&'static str>,
}

impl Facts {
    /// The used verdict of the selector whose source text is `text` (panics
    /// on an unknown selector so a typo'd test fails loudly).
    fn used(&self, text: &str) -> bool {
        self.used
            .iter()
            .find(|(t, _)| t == text)
            .unwrap_or_else(|| panic!("selector `{text}` not found in {:?}", self.used))
            .1
    }

    fn scoped_selector(&self, text: &str) -> bool {
        self.scoped_selectors.iter().any(|t| t == text)
    }

    fn scoped_tag(&self, tag: &str) -> bool {
        self.scoped_tags.iter().any(|t| t == tag)
    }
}

fn body_span(source: &str) -> Span {
    let start = source.find("<style>").expect("open tag") + "<style>".len();
    let end = source.rfind("</style>").expect("close tag");
    Span::new(start as u32, end as u32)
}

fn slice(source: &str, span: Span) -> String {
    source[span.start as usize..span.end as usize].to_string()
}

fn collect_selector_facts(
    source: &str,
    list: &SelectorList,
    used: &mut Vec<(String, bool)>,
    scoped: &mut Vec<String>,
) {
    for complex in &list.children {
        used.push((slice(source, complex.span), complex.metadata.used));
        for relative in &complex.children {
            if relative.metadata.scoped {
                // A relative selector's span starts at its leading combinator
                // (the whitespace run for the descendant combinator) — trim
                // so lookups key on the visible compound text.
                scoped.push(slice(source, relative.span).trim().to_string());
            }
            for simple in &relative.selectors {
                if let SimpleSelector::PseudoClass {
                    args: Some(args), ..
                } = simple
                {
                    collect_selector_facts(source, args, used, scoped);
                }
            }
        }
    }
}

fn collect_rule_facts(
    source: &str,
    children: &[StyleChild],
    used: &mut Vec<(String, bool)>,
    scoped: &mut Vec<String>,
) {
    for child in children {
        match child {
            StyleChild::Rule(rule) => {
                collect_selector_facts(source, &rule.prelude, used, scoped);
                collect_block_facts(source, &rule.block, used, scoped);
            }
            StyleChild::Atrule(at) => {
                if let Some(block) = &at.block {
                    collect_block_facts(source, block, used, scoped);
                }
            }
        }
    }
}

fn collect_block_facts(
    source: &str,
    block: &Block,
    used: &mut Vec<(String, bool)>,
    scoped: &mut Vec<String>,
) {
    for item in &block.children {
        match item {
            BlockChild::Rule(rule) => {
                collect_selector_facts(source, &rule.prelude, used, scoped);
                collect_block_facts(source, &rule.block, used, scoped);
            }
            BlockChild::Atrule(at) => {
                if let Some(inner) = &at.block {
                    collect_block_facts(source, inner, used, scoped);
                }
            }
            BlockChild::Declaration(_) => {}
        }
    }
}

/// Parse + lower + plan (the production wiring) and extract the matcher
/// facts.
fn facts(source: &str) -> Facts {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .unwrap_or_else(|e| panic!("lowering succeeds: {e:?}"));
    match build_style_scope_plan(
        source,
        body_span(source),
        Some("Test.svelte"),
        CssMode::External,
        &ir,
        false,
    ) {
        Ok(plan) => {
            let mut used = Vec::new();
            let mut scoped_selectors = Vec::new();
            collect_rule_facts(source, &plan.ast.children, &mut used, &mut scoped_selectors);
            let mut scoped_tags: Vec<String> = plan
                .facts
                .scoped
                .iter()
                .map(|id| match ir.node(*id) {
                    IrNode::Element(el) => el.tag.clone(),
                    IrNode::Special(sp) if sp.kind == SpecialKind::Element => {
                        "svelte:element".to_string()
                    }
                    other => panic!("a scoped node is always an element: {other:?}"),
                })
                .collect();
            scoped_tags.sort();
            Facts {
                used,
                scoped_selectors,
                scoped_tags,
                unprovable: None,
            }
        }
        // An unprovable relation never constructs a plan: NO facts exist at
        // all (no used verdicts, no scoped tags) — the typed failure carries
        // the construct description. Any other failure class is a broken
        // test fixture.
        Err(failure) => {
            assert_eq!(
                failure.class,
                crate::svelte::runtime::css::StylePlanFailureClass::SelectorUnprovable,
                "a match-test fixture only fails on the selector surface: {failure:?}"
            );
            Facts {
                used: Vec::new(),
                scoped_selectors: Vec::new(),
                scoped_tags: Vec::new(),
                unprovable: Some(
                    failure
                        .construct
                        .expect("a matcher refusal names its construct"),
                ),
            }
        }
    }
}

fn assert_proven(f: &Facts) {
    assert!(
        f.unprovable.is_none(),
        "expected a proven match, got unprovable: {:?}",
        f.unprovable
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Simple selectors.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn type_selector_matches_the_tag_and_not_others() {
    let f = facts("<div>x</div>\n<style>div { color: red; }\nspan { color: red; }</style>");
    assert_proven(&f);
    assert!(f.used("div"));
    assert!(!f.used("span"));
    assert!(f.scoped_tag("div"));
    assert!(!f.scoped_tag("span"));
    assert!(f.scoped_selector("div"));
    assert!(!f.scoped_selector("span"));
}

#[test]
fn type_selector_matches_case_insensitively() {
    let f = facts("<div>x</div>\n<style>DIV { color: red; }</style>");
    assert_proven(&f);
    assert!(f.used("DIV"));
}

#[test]
fn universal_selector_matches_any_element() {
    let f = facts("<p>x</p>\n<style>* { color: red; }</style>");
    assert_proven(&f);
    assert!(f.used("*"));
    assert!(f.scoped_tag("p"));
}

#[test]
fn class_selector_matches_static_class_list_words() {
    let f = facts(
        "<div class=\"a b\">x</div>\n<style>.a { color: red; }\n.b { color: red; }\n.c { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a"));
    assert!(f.used(".b"));
    assert!(!f.used(".c"));
    assert!(f.scoped_tag("div"));
}

#[test]
fn class_directive_matches_its_own_name_only() {
    let f = facts(
        "<script>let on = $state(true);</script>\n<div class:active={on}>x</div>\n<style>.active { color: red; }\n.idle { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".active"));
    assert!(!f.used(".idle"));
}

#[test]
fn id_selector_matches_exact_id_only() {
    let f = facts("<div id=\"x\">x</div>\n<style>#x { color: red; }\n#y { color: red; }</style>");
    assert_proven(&f);
    assert!(f.used("#x"));
    assert!(!f.used("#y"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Attribute selectors (all operators + case flags + whitelist).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn attribute_operators_discriminate() {
    let source = "<div data-k=\"one two-three\">x</div>\n<style>\
[data-k=\"one two-three\"] { color: red; }\n\
[data-k=\"one\"] { color: red; }\n\
[data-k~=\"one\"] { color: red; }\n\
[data-k~=\"two\"] { color: red; }\n\
[data-k|=\"one two\"] { color: red; }\n\
[data-k|=\"two\"] { color: red; }\n\
[data-k^=\"one\"] { color: red; }\n\
[data-k^=\"two\"] { color: red; }\n\
[data-k$=\"three\"] { color: red; }\n\
[data-k$=\"two\"] { color: red; }\n\
[data-k*=\"two-th\"] { color: red; }\n\
[data-k*=\"four\"] { color: red; }\n\
</style>";
    let f = facts(source);
    assert_proven(&f);
    assert!(f.used("[data-k=\"one two-three\"]"));
    assert!(!f.used("[data-k=\"one\"]"));
    assert!(f.used("[data-k~=\"one\"]"));
    assert!(
        !f.used("[data-k~=\"two\"]"),
        "`~=` is word-exact, not prefix"
    );
    assert!(
        f.used("[data-k|=\"one two\"]"),
        "`|=` matches value or value+dash prefix"
    );
    assert!(!f.used("[data-k|=\"two\"]"));
    assert!(f.used("[data-k^=\"one\"]"));
    assert!(!f.used("[data-k^=\"two\"]"));
    assert!(f.used("[data-k$=\"three\"]"));
    assert!(!f.used("[data-k$=\"two\"]"));
    assert!(f.used("[data-k*=\"two-th\"]"));
    assert!(!f.used("[data-k*=\"four\"]"));
}

#[test]
fn attribute_case_flags_discriminate() {
    // `data-k` is NOT in the html case-insensitive set: the default is
    // case-sensitive, `i` forces insensitive.
    let f = facts(
        "<div data-k=\"Value\">x</div>\n<style>[data-k=\"value\"] { color: red; }\n[data-k=\"value\" i] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(!f.used("[data-k=\"value\"]"));
    assert!(f.used("[data-k=\"value\" i]"));
}

#[test]
fn html_case_insensitive_attribute_defaults_and_s_flag() {
    // `type` IS in the html case-insensitive set: insensitive by default,
    // `s` forces sensitive.
    let f = facts(
        "<input type=\"text\" />\n<style>[type=\"TEXT\"] { color: red; }\n[type=\"TEXT\" s] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("[type=\"TEXT\"]"));
    assert!(!f.used("[type=\"TEXT\" s]"));
}

#[test]
fn valueless_attribute_selector_requires_attribute_presence() {
    let f = facts(
        "<button disabled>x</button>\n<style>[disabled] { color: red; }\n[hidden] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("[disabled]"));
    assert!(!f.used("[hidden]"));
}

#[test]
fn details_open_is_whitelisted_without_the_attribute() {
    // The runtime toggles `open` on `<details>`/`<dialog>` — always a match
    // there, and NOT elsewhere.
    let f = facts(
        "<details>x</details>\n<div>y</div>\n<style>details[open] { color: red; }\ndiv[open] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("details[open]"));
    assert!(!f.used("div[open]"));
}

#[test]
fn spread_attributes_match_any_attribute_selector() {
    let f = facts(
        "<script>let rest = $state({});</script>\n<div {...rest}>x</div>\n<style>[data-anything=\"v\"] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("[data-anything=\"v\"]"));
}

#[test]
fn style_directive_matches_style_attribute_selector() {
    let f = facts(
        "<div style:color=\"red\">x</div>\n<style>[style] { color: red; }\n[title] { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("[style]"));
    assert!(!f.used("[title]"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Dynamic value enumeration (get_possible_values).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn conditional_class_expression_enumerates_both_branches_only() {
    let f = facts(
        "<script>let cond = $state(true);</script>\n<div class={cond ? 'a' : 'b'}>x</div>\n<style>.a { color: red; }\n.b { color: red; }\n.c { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a"));
    assert!(f.used(".b"));
    assert!(
        !f.used(".c"),
        "an enumerable conditional must NOT match other classes"
    );
}

#[test]
fn unknown_class_expression_matches_anything() {
    let f = facts(
        "<script>let dynamic = $state('x');</script>\n<div class={dynamic}>x</div>\n<style>.whatever { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".whatever"));
}

#[test]
fn mixed_class_value_combines_text_and_expression_chunks() {
    let f = facts(
        "<script>let cond = $state(true);</script>\n<div class=\"static {cond ? 'a' : 'b'}\">x</div>\n<style>.static { color: red; }\n.a { color: red; }\n.b { color: red; }\n.c { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".static"));
    assert!(f.used(".a"));
    assert!(f.used(".b"));
    assert!(!f.used(".c"));
}

#[test]
fn finite_mixed_class_product_over_twenty_still_enumerates() {
    // Two adjacent 5-way expression chunks combine into 25 (> 20) FINITE
    // values. The official matcher fully enumerates a finite combination
    // product — the `> 20` exponential bail guards only the fresh-append
    // growth path (the combine branch `continue`s past it) — so a class
    // that no combination produces still prunes.
    let f = facts(
        "<script>let a = $state(0); let b = $state(0);</script>\n<div class=\"{a === 1 ? 'pa' : a === 2 ? 'pb' : a === 3 ? 'pc' : a === 4 ? 'pd' : 'pe'}{b === 1 ? 'qa' : b === 2 ? 'qb' : b === 3 ? 'qc' : b === 4 ? 'qd' : 'qe'}\">x</div>\n<style>.paqa { color: red; }\n.peqe { color: red; }\n.zz { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".paqa"));
    assert!(f.used(".peqe"));
    assert!(
        !f.used(".zz"),
        "a finite 25-combination class value must fully enumerate — `.zz` is not producible"
    );
}

#[test]
fn over_threshold_class_expression_bails_to_may_match() {
    // A single chunk enumerating MORE than 20 values trips the official
    // exponential bail: the selector may match anything (stays used).
    // Exactly 20 values sits on the boundary and still enumerates.
    let chain = |leaves: usize| -> String {
        let mut expr = String::new();
        for i in 0..leaves - 1 {
            expr.push_str(&format!("a === {i} ? 'v{i}' : "));
        }
        expr.push_str(&format!("'v{}'", leaves - 1));
        expr
    };
    let source = |leaves: usize| -> String {
        format!(
            "<script>let a = $state(0);</script>\n<div class={{{}}}>x</div>\n<style>.v0 {{ color: red; }}\n.zz {{ color: red; }}</style>",
            chain(leaves)
        )
    };

    let over = facts(&source(21));
    assert_proven(&over);
    assert!(over.used(".v0"));
    assert!(
        over.used(".zz"),
        "21 possible values exceed the bail threshold — the selector may match anything"
    );

    let at = facts(&source(20));
    assert_proven(&at);
    assert!(at.used(".v0"));
    assert!(
        !at.used(".zz"),
        "exactly 20 possible values still enumerate — `.zz` is not producible"
    );
}

#[test]
fn class_array_expression_enumerates_entries() {
    let f = facts(
        "<script>let cond = $state(true);</script>\n<div class={['x', cond && 'y']}>x</div>\n<style>.x { color: red; }\n.y { color: red; }\n.z { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".x"));
    assert!(f.used(".y"));
    assert!(!f.used(".z"));
}

#[test]
fn numeric_literal_values_stringify_per_ecma262() {
    // Known vectors for the JS `String(number)` spelling a numeric literal
    // enumerates through — each expected value verified against
    // `node -e "console.log(String(x))"`. The `String()` form is computed at
    // PROJECTION LOWERING (the single template-expression parse); the matcher
    // walk hands it through unchanged.
    let spell = |src: &str| -> Vec<String> {
        super::values::expression_possible_values(&matcher_expr_of(src), false)
            .expect("a numeric literal always enumerates")
            .expect("a numeric literal is never UNKNOWN")
    };
    assert_eq!(spell("1e21"), ["1e+21"]);
    assert_eq!(spell("1e-6"), ["0.000001"]);
    assert_eq!(spell("1e-7"), ["1e-7"]);
    assert_eq!(spell("0.001"), ["0.001"]);
    assert_eq!(spell("1.5"), ["1.5"]);
    assert_eq!(spell("100"), ["100"]);
    assert_eq!(spell("9007199254740991"), ["9007199254740991"]);
    // ECMA-262 equidistant tie-break: among the shortest denoting digit
    // strings the EVEN candidate wins, so `String(161647069304469.12)` is
    // `"161647069304469.12"` — not the raw shortest-round-trip `…13`.
    assert_eq!(spell("161647069304469.12"), ["161647069304469.12"]);
    // Negative controls — the plausible-wrong spellings must not appear.
    assert_ne!(spell("1e21"), ["1e21"]);
    assert_ne!(spell("1e-7"), ["0.0000001"]);
}

// ─────────────────────────────────────────────────────────────────────────────
// The typed matcher projection (`MatcherExpr`): the value enumeration and the
// expression-attribute classification walk the OWNED projection lowered by
// `collect_expr_references`'s single parse — the matcher never re-parses
// expression source. Each walk reproduces the official
// `gather_possible_values` set exactly.
// ─────────────────────────────────────────────────────────────────────────────

/// Lower one template-expression source through the SAME single-parse
/// analysis the runtime lowering uses (`collect_expr_references`) and return
/// its owned matcher projection — the input `expression_possible_values` /
/// `expression_attr_shape` walk.
fn matcher_expr_of(src: &str) -> MatcherExpr {
    crate::svelte::runtime::expr::collect_expr_references(src)
        .expect("the template expression parses")
        .matcher_expr
}

/// `expression_possible_values` over the lowered projection of `src`.
fn possible_values(src: &str, is_class: bool) -> Result<Option<Vec<String>>, &'static str> {
    super::values::expression_possible_values(&matcher_expr_of(src), is_class)
}

/// The enumerated set as owned strings (panics on UNKNOWN / refusal so a
/// wrong verdict fails loudly).
fn enumerated(src: &str, is_class: bool) -> Vec<String> {
    possible_values(src, is_class)
        .expect("the expression enumerates")
        .expect("the expression is not UNKNOWN")
}

#[test]
fn regex_literal_value_stringifies_canonically_per_string_conversion() {
    // The official matcher enumerates `String(node.value)` — and JS
    // canonicalizes the FLAG ORDER through the `RegExp.prototype.flags`
    // getter (`d g i m s u v y`), regardless of the written order:
    // `String(/a/ig) === "/a/gi"` (verified first-hand). The projection must
    // carry the CANONICAL spelling, never the raw source text.
    assert_eq!(enumerated("/a/ig", false), ["/a/gi"]);
    // An already-canonical literal is unchanged (and a flagless one keeps
    // the bare `/pattern/`).
    assert_eq!(enumerated("/a/gi", false), ["/a/gi"]);
    assert_eq!(enumerated("/a/", false), ["/a/"]);
    // A fuller scramble: `ysumig` → canonical `gimsuy`.
    assert_eq!(enumerated("/x/ysumig", false), ["/x/gimsuy"]);
    // NEGATIVE: the raw spelling must not surface.
    assert_ne!(enumerated("/a/ig", false), ["/a/ig"]);
}

#[test]
fn regex_class_value_matches_the_canonical_flag_order_selector_only() {
    // First-hand official verdicts for `class={/a/ig}`:
    // `[class="/a/gi"]` is USED (+ scoped); `[class="/a/ig"]` is UNUSED
    // (pruned) — the matcher compares against the canonical `String()` form.
    let f = facts(
        "<div class={/a/ig}>x</div>\n<style>[class=\"/a/gi\"] { color: red; }\n[class=\"/a/ig\"] { color: blue; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("[class=\"/a/gi\"]"),
        "the canonical String() spelling matches"
    );
    assert!(
        !f.used("[class=\"/a/ig\"]"),
        "the RAW source spelling must NOT match"
    );
}

#[test]
fn projection_conditional_enumerates_both_result_branches() {
    assert_eq!(enumerated("x ? 'a' : 'b'", false), ["a", "b"]);
    // Nested conditionals distribute through both result branches.
    assert_eq!(
        enumerated("x ? 'a' : y ? 'b' : 'c'", false),
        ["a", "b", "c"]
    );
}

#[test]
fn projection_logical_and_falsy_fill_depends_on_nested_class_position() {
    // Non-class: an unknown `&&` left adds ALL non-nullish falsy fill-ins
    // before the right side's values.
    assert_eq!(
        enumerated("x && 'a'", false),
        ["", "false", "NaN", "0", "a"]
    );
    // A TOP-LEVEL class value still fills (the suppression is NESTED-only).
    assert_eq!(enumerated("x && 'a'", true), ["", "false", "NaN", "0", "a"]);
    // NESTED inside a class array: clsx removes falsy entries — no fill.
    assert_eq!(enumerated("[x && 'a']", true), ["a"]);
}

#[test]
fn projection_logical_or_enumerates_both_sides() {
    assert_eq!(enumerated("'a' || 'b'", false), ["a", "b"]);
    // An unknown side keeps the whole set unknown ("may match anything").
    assert_eq!(possible_values("x || 'b'", false), Ok(None));
    // `??` enumerates like `||` (both sides, no falsy fill).
    assert_eq!(enumerated("'a' ?? 'b'", false), ["a", "b"]);
}

#[test]
fn projection_class_array_enumerates_entries_only_for_class_values() {
    assert_eq!(enumerated("['a', b && 'c']", true), ["a", "c"]);
    // An elision contributes nothing (and does not poison the set).
    assert_eq!(enumerated("['a', , 'b']", true), ["a", "b"]);
    // A NON-class array is not enumerated — unknown.
    assert_eq!(possible_values("['a', b && 'c']", false), Ok(None));
}

#[test]
fn projection_object_keys_enumerate_only_for_class_values() {
    assert_eq!(
        enumerated("{ active: x, 'b-c': y }", true),
        ["active", "b-c"]
    );
    // A numeric key enumerates its exact `String()` form.
    assert_eq!(enumerated("{ 1.5: x }", true), ["1.5"]);
    // A NON-class object is not enumerated — unknown.
    assert_eq!(possible_values("{ active: x, 'b-c': y }", false), Ok(None));
}

#[test]
fn projection_spread_and_computed_shapes_are_unknown() {
    // A spread array element makes the set unknown.
    assert_eq!(possible_values("[...xs]", true), Ok(None));
    // A computed object key makes the key set unknown.
    assert_eq!(possible_values("{ [key]: x }", true), Ok(None));
    // A bare identifier is unknown in value position.
    assert_eq!(possible_values("x", false), Ok(None));
}

#[test]
fn projection_bigint_overflow_fails_closed() {
    // 2^128 overflows the u128 conversion — fail closed, never guessed.
    assert_eq!(
        possible_values("0x100000000000000000000000000000000n", false),
        Err("a bigint literal beyond the reproducible stringification range")
    );
    // The same refusal from OBJECT-KEY position (a class value).
    assert_eq!(
        possible_values("{ 0x100000000000000000000000000000000n: x }", true),
        Err("a bigint literal beyond the reproducible stringification range")
    );
    // An in-range radix bigint still enumerates its exact decimal form.
    assert_eq!(enumerated("0x10n", false), ["16"]);
}

#[test]
fn projection_peels_transparent_parens_at_every_level() {
    // The projection roots at the paren-peeled node (estree has no paren
    // nodes) — a wrapped identifier stays an Identifier…
    assert_eq!(
        matcher_expr_of("((x))"),
        MatcherExpr::Identifier("x".to_string())
    );
    // …and nested author parens peel at every level of the walk.
    assert_eq!(enumerated("(x ? ('a') : (('b')))", false), ["a", "b"]);
}

#[test]
fn projection_attr_shape_classifies_identifier_literal_other() {
    use super::values::{expression_attr_shape, ExprAttrShape};
    assert!(matches!(
        expression_attr_shape(&matcher_expr_of("(snip)")),
        ExprAttrShape::Identifier(name) if name == "snip"
    ));
    assert!(matches!(
        expression_attr_shape(&matcher_expr_of("'lit'")),
        ExprAttrShape::Literal
    ));
    assert!(matches!(
        expression_attr_shape(&matcher_expr_of("42")),
        ExprAttrShape::Literal
    ));
    assert!(matches!(
        expression_attr_shape(&matcher_expr_of("a.b")),
        ExprAttrShape::Other
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// :global.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn global_selector_is_used_and_its_descendant_arm_still_discriminates() {
    let f = facts(
        "<div><p>x</p></div>\n<style>:global(.anything) { color: red; }\n:global(.x) p { color: red; }\n:global(.x) em { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(":global(.anything)"),
        "a lone :global is used by definition"
    );
    assert!(
        f.used(":global(.x) p"),
        "the scoped tail matches the template <p>"
    );
    assert!(
        !f.used(":global(.x) em"),
        "the scoped tail must still match"
    );
}

#[test]
fn outer_global_compound_is_never_marked_scoped() {
    // `:global(.foo) .bar` — the `.bar` compound is scoped; the outer-global
    // `:global(.foo)` compound must NOT be (is_outer_global gates the mark).
    let f = facts(
        "<div class=\"foo\"><p class=\"bar\">x</p></div>\n<style>:global(.foo) .bar { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(":global(.foo) .bar"));
    assert!(f.scoped_selector(".bar"));
    assert!(
        !f.scoped_selector(":global(.foo)"),
        "an outer-global compound never receives the scope class"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Combinators: descendant / child.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn descendant_combinator_requires_ancestry() {
    let f = facts(
        "<div class=\"a\"><section><p class=\"b\">x</p></section></div>\n<em class=\"c\"></em>\n<style>.a .b { color: red; }\n.c .b { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a .b"));
    assert!(
        !f.used(".c .b"),
        "a non-ancestor must not satisfy the descendant combinator"
    );
}

#[test]
fn child_combinator_requires_nearest_element_ancestor() {
    let f = facts(
        "<div class=\"a\"><p class=\"b\">x</p><section><p class=\"d\">y</p></section></div>\n<style>.a > .b { color: red; }\n.a > .d { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a > .b"));
    assert!(
        !f.used(".a > .d"),
        "a grandchild behind another element must not satisfy `>`"
    );
}

#[test]
fn child_combinator_sees_through_block_boundaries() {
    // Blocks are transparent for ancestry: `{#if}` between parent and child
    // does not break `>` (the official behavior).
    let f = facts(
        "<script>let x = $state(true);</script>\n<div class=\"a\">{#if x}<p class=\"b\">y</p>{/if}</div>\n<style>.a > .b { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a > .b"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Combinators: next-sibling / subsequent-sibling + existence tri-state.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn next_sibling_combinator_discriminates_order() {
    let f = facts(
        "<p>x</p>\n<span>y</span>\n<style>p + span { color: red; }\nspan + p { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("p + span"));
    assert!(!f.used("span + p"), "sibling order matters for `+`");
}

#[test]
fn subsequent_sibling_combinator_skips_intermediates() {
    let f = facts(
        "<p>x</p>\n<em>y</em>\n<span>z</span>\n<style>p ~ span { color: red; }\nspan ~ p { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("p ~ span"));
    assert!(!f.used("span ~ p"));
}

#[test]
fn probable_if_block_does_not_block_adjacency() {
    // `{#if}` without `{:else}`: the branch element only PROBABLY exists, so
    // the scan continues past it — BOTH `z + c` and `a + c` are possible.
    let f = facts(
        "<script>let x = $state(true);</script>\n<b>w</b>{#if x}<i>y</i>{/if}<u>v</u>\n<style>b + u { color: red; }\ni + u { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("i + u"),
        "the branch element is a possible previous sibling"
    );
    assert!(
        f.used("b + u"),
        "a PROBABLY-existing branch must not block the sibling scan"
    );
}

#[test]
fn exhaustive_if_else_blocks_adjacency_behind_it() {
    // `{#if}{:else}` with definite elements in EVERY branch: one of them
    // DEFINITELY precedes `<u>`, so `b + u` is impossible.
    let f = facts(
        "<script>let x = $state(true);</script>\n<b>w</b>{#if x}<i>y</i>{:else}<em>e</em>{/if}<u>v</u>\n<style>b + u { color: red; }\ni + u { color: red; }\nem + u { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("i + u"));
    assert!(f.used("em + u"));
    assert!(
        !f.used("b + u"),
        "a DEFINITELY-existing exhaustive branch set blocks the `+` scan"
    );
}

#[test]
fn else_if_chain_without_final_else_stays_probable() {
    // The branch fold: `{#if}{:else if}` WITHOUT a trailing `{:else}` is
    // non-exhaustive — everything stays PROBABLY, so `b + u` survives.
    let f = facts(
        "<script>let x = $state(1);</script>\n<b>w</b>{#if x === 1}<i>y</i>{:else if x === 2}<em>e</em>{/if}<u>v</u>\n<style>b + u { color: red; }\ni + u { color: red; }\nem + u { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("i + u"));
    assert!(f.used("em + u"));
    assert!(
        f.used("b + u"),
        "a missing else keeps the chain non-exhaustive"
    );
}

#[test]
fn each_block_elements_are_self_adjacent_across_iterations() {
    let f = facts(
        "<script>let xs = $state([1]);</script>\n{#each xs as x}<li>{x}</li>{/each}\n<style>li + li { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("li + li"),
        "each-body elements wrap around as siblings"
    );

    let single = facts("<li>x</li>\n<style>li + li { color: red; }</style>");
    assert_proven(&single);
    assert!(
        !single.used("li + li"),
        "a single static element is not its own sibling"
    );
}

#[test]
fn each_fallback_elements_match() {
    let f = facts(
        "<script>let xs = $state([]);</script>\n{#each xs as x}<li>{x}</li>{:else}<p>empty</p>{/each}\n<style>p { color: red; }\nem { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("p"));
    assert!(!f.used("em"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Components.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn component_boundary_is_transparent_for_sibling_climb() {
    let f = facts(
        "<script>import C from './C.svelte';</script>\n<div>a</div>\n<C><p>b</p></C>\n<style>div + p { color: red; }\np + div { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("div + p"),
        "a component boundary is transparent climbing out"
    );
    assert!(
        !f.used("p + div"),
        "direction still discriminates through it"
    );
}

#[test]
fn component_sibling_matches_only_lone_global_selectors() {
    let f = facts(
        "<script>import C from './C.svelte';</script>\n<div>a</div>\n<C />\n<span>b</span>\n<style>div + span { color: red; }\n:global(.x) + span { color: red; }\n.x + span { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("div + span"),
        "the scan continues past the component"
    );
    assert!(
        f.used(":global(.x) + span"),
        "a component sibling satisfies a lone :global"
    );
    assert!(
        !f.used(".x + span"),
        "a component sibling never satisfies a scoped compound"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Snippets: invisible defs, rendered neighborhoods, sites.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn snippet_definition_renders_nothing_in_place() {
    let f = facts(
        "{#snippet s()}<p>x</p>{/snippet}\n<em>y</em>\n<style>p + em { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        !f.used("p + em"),
        "a snippet definition is not a rendered sibling"
    );
}

#[test]
fn rendered_snippet_contributes_sibling_neighborhood() {
    let f = facts(
        "{#snippet s()}<p>x</p>{/snippet}\n{@render s()}\n<em>y</em>\n<style>p + em { color: red; }\nu + em { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("p + em"), "the rendered snippet body precedes <em>");
    assert!(!f.used("u + em"));
}

#[test]
fn snippet_body_ancestry_resolves_through_render_sites() {
    let f = facts(
        "{#snippet s()}<p class=\"x\">x</p>{/snippet}\n<div class=\"host\">{@render s()}</div>\n<style>.host .x { color: red; }\n.other .x { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(".host .x"),
        "ancestry climbs out of the snippet body into its call site"
    );
    assert!(!f.used(".other .x"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Pseudo-classes: :is / :where / :not / :has.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn is_pseudo_class_discriminates_its_arguments() {
    let f = facts(
        "<div class=\"a\">x</div>\n<style>:is(.a, .zz) { color: red; }\n:is(.yy, .zz) { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(":is(.a, .zz)"));
    assert!(f.used(".a"), "the matching :is argument is marked used");
    assert!(!f.used(":is(.yy, .zz)"));
}

#[test]
fn where_pseudo_class_discriminates_its_arguments() {
    let f = facts(
        "<div class=\"a\">x</div>\n<style>:where(.a) { color: red; }\n:where(.zz) { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(":where(.a)"));
    assert!(!f.used(":where(.zz)"));
}

#[test]
fn not_pseudo_class_stays_unscoped_and_accepts() {
    let f = facts(
        "<div class=\"a\">x</div>\n<style>div:not(.zz) { color: red; }\nspan:not(.a) { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("div:not(.zz)"));
    assert!(f.used(".zz"), ":not arguments are marked used, not matched");
    assert!(
        !f.used("span:not(.a)"),
        "the outer compound still has to match"
    );
}

#[test]
fn has_pseudo_class_requires_a_matching_descendant() {
    let f = facts(
        "<div class=\"a\"><p class=\"b\">x</p></div>\n<style>.a:has(.b) { color: red; }\n.a:has(.zz) { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".a:has(.b)"));
    assert!(f.used(".b"), "the matching :has argument is marked used");
    assert!(!f.used(".a:has(.zz)"), ":has must find a real descendant");
}

// ─────────────────────────────────────────────────────────────────────────────
// Nesting + global blocks.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn nested_rule_matches_through_the_implied_parent() {
    let f = facts(
        "<div class=\"a\"><p class=\"b\">x</p></div>\n<style>.a { .b { color: red; } .zz { color: red; } }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".b"));
    assert!(!f.used(".zz"));
    assert!(
        f.used(".a"),
        "the parent prelude is marked used by the nested match"
    );
}

#[test]
fn explicit_nesting_child_combinator_discriminates() {
    let f = facts(
        "<div class=\"a\"><p class=\"b\">x</p><section><p class=\"d\">y</p></section></div>\n<style>.a { & > .b { color: red; } & > .d { color: red; } }</style>",
    );
    assert_proven(&f);
    assert!(f.used("& > .b"));
    assert!(!f.used("& > .d"));
}

#[test]
fn global_block_inner_rules_are_not_pruned() {
    // The official prune skips a `:global {}` block's body — inner selectors
    // keep `used == false` even when a template element would match them.
    let f = facts("<div>x</div>\n<style>:global { div { color: red; } }</style>");
    assert_proven(&f);
    assert!(
        f.used(":global"),
        "the :global prelude is used by definition"
    );
    assert!(
        !f.used("div"),
        "global-block bodies are exempt from pruning (official parity)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// <svelte:element>.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn svelte_element_matches_any_type_selector_but_not_absent_classes() {
    let f = facts(
        "<script>let tag = $state('div');</script>\n<svelte:element this={tag} class=\"known\" />\n<style>article { color: red; }\n.known { color: red; }\n.absent { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("article"), "a svelte:element may be any tag");
    assert!(f.used(".known"));
    assert!(!f.used(".absent"));
}

#[test]
fn svelte_element_sibling_is_probable_not_blocking() {
    let f = facts(
        "<script>let tag = $state('div');</script>\n<b>a</b>\n<svelte:element this={tag} />\n<u>b</u>\n<style>b + u { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used("b + u"),
        "a PROBABLY-existing svelte:element does not block the scan"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Unused-selector facts.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn genuinely_unused_rules_carry_used_false() {
    let f = facts(
        "<div class=\"present\">x</div>\n<style>.present { color: red; }\n.absent-one { color: red; }\nsection article { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".present"));
    assert!(!f.used(".absent-one"));
    assert!(!f.used("section article"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Fail-closed unprovable constructs.
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// `<slot>` block-semantic projection (official `SlotElement` — css-prune.js).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn slot_fallback_element_matches_and_scopes() {
    // A selector matching an element INSIDE the slot fallback: the fallback
    // fragment projects (kept + the fallback element scoped); the slot itself
    // NEVER enters the element inventory (no scope hash on a non-element).
    let f = facts(
        "<div><slot><p class=\"fb\">x</p></slot></div>\n<style>.fb { color: red; }\n.absent { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used(".fb"), "the fallback element matches: {:?}", f.used);
    assert!(!f.used(".absent"), "an absent class still prunes");
    assert!(
        f.scoped_tag("p"),
        "the fallback <p> is scoped: {:?}",
        f.scoped_tags
    );
    assert!(
        !f.scoped_tag("slot"),
        "the slot itself never receives the scope hash: {:?}",
        f.scoped_tags
    );
}

#[test]
fn outer_before_slot_to_fallback_first_adjacency_matches() {
    // `.outer + .inner` where `.inner` is the FIRST fallback element: the
    // sibling walk climbs OUT of the fallback fragment (the slot is a block
    // boundary, not a stop) and finds the definite outer sibling.
    let f = facts(
        "<div><i class=\"outer\">a</i><slot><p class=\"inner\">x</p></slot></div>\n<style>.outer + .inner { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(".outer + .inner"),
        "the fallback-first adjacency crosses the slot boundary: {:?}",
        f.used
    );
    assert!(f.scoped_tag("p") && f.scoped_tag("i"));
}

#[test]
fn fallback_last_to_outer_after_slot_adjacency_is_fail_open() {
    // `.last + .after` where `.last` is the LAST fallback element and `.after`
    // follows the slot: the slot sibling projects its fallback boundary
    // candidates NON-exhaustively (supplied content may render instead), so the
    // relation is kept fail-open and both elements scope.
    let f = facts(
        "<div><slot><p class=\"last\">x</p></slot><em class=\"after\">y</em></div>\n<style>.last + .after { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(".last + .after"),
        "the fallback-last adjacency is kept (fail-open): {:?}",
        f.used
    );
    assert!(f.scoped_tag("p") && f.scoped_tag("em"));
}

#[test]
fn empty_fallback_slot_between_outer_siblings_keeps_adjacency() {
    // `.a + .b` with an EMPTY-fallback slot between: the sibling walk records
    // the slot as PROBABLY (it may render supplied content) and keeps stepping
    // to the definite `.a` sibling — official keeps the adjacency.
    let f = facts(
        "<div><i class=\"a\">a</i><slot></slot><em class=\"b\">b</em></div>\n<style>.a + .b { color: red; }\n.zz + .b { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(".a + .b"),
        "the walk steps past the slot to the definite sibling: {:?}",
        f.used
    );
    // NEGATIVE: a never-present left side still prunes — the slot's PROBABLY
    // entry alone must not fail the walk open for a non-global selector.
    assert!(
        !f.used(".zz + .b"),
        "a non-matching adjacency with only the slot between stays pruned: {:?}",
        f.used
    );
}

#[test]
fn sibling_walk_continues_past_a_slot_with_definite_fallback_content() {
    // `.pre + .after` across a slot whose FALLBACK carries a definite element:
    // the slot's boundary candidates are NON-exhaustive (supplied content may
    // render instead), so the adjacent walk must NOT early-return on them — it
    // keeps stepping to the definite `.pre` sibling BEFORE the slot. An
    // (incorrect) exhaustive fallback projection would stop at the fallback
    // `<p>` and prune the relation.
    let f = facts(
        "<div><span class=\"pre\">a</span><slot><p class=\"last\">x</p></slot><em class=\"after\">y</em></div>\n<style>.pre + .after { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(".pre + .after"),
        "the walk continues past the slot's non-exhaustive fallback: {:?}",
        f.used
    );
    assert!(f.scoped_tag("span") && f.scoped_tag("em"));
}

#[test]
fn global_sibling_over_slot_is_kept_by_the_slot_uncertainty() {
    // `:global(.x) + em`: the slot sibling takes the official
    // `SlotElement`-uncertainty arm — a SINGLE all-global remainder matches
    // (the slot may render a `.x`), so the rule is kept and `em` scopes. The
    // NON-global twin (`.x + em` with no `.x` anywhere) stays pruned — the
    // uncertainty arm is global-remainder-ONLY.
    let f = facts(
        "<div><slot></slot><em>y</em></div>\n<style>:global(.x) + em { color: red; }\n.x + em { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(
        f.used(":global(.x) + em"),
        "the global sibling over a slot is kept: {:?}",
        f.used
    );
    assert!(
        !f.used(".x + em"),
        "the non-global twin stays pruned: {:?}",
        f.used
    );
    assert!(f.scoped_tag("em"));
}

#[test]
fn named_slot_filler_fails_closed() {
    let f = facts(
        "<script>import C from './C.svelte';</script>\n<C><p slot=\"x\">a</p></C>\n<style>p { color: red; }</style>",
    );
    assert!(
        f.unprovable.is_some_and(|c| c.contains("named-slot")),
        "a named-slot filler is unprovable, got {:?}",
        f.unprovable
    );
    // No plan ⇒ no used verdicts published (the matching `p` included).
    assert!(f.used.is_empty(), "{:?}", f.used);
}

#[test]
fn svelte_fragment_hoisting_fails_closed() {
    let f = facts(
        "<script>import C from './C.svelte';</script>\n<C><svelte:fragment slot=\"default\"><p>a</p></svelte:fragment></C>\n<style>p { color: red; }</style>",
    );
    assert!(
        f.unprovable.is_some(),
        "hoisted svelte:fragment slot content is unprovable"
    );
    // No plan ⇒ no used verdicts published (the matching `p` included).
    assert!(f.used.is_empty(), "{:?}", f.used);
}

#[test]
fn svelte_head_title_fails_closed() {
    let f =
        facts("<svelte:head><title>t</title></svelte:head>\n<style>div { color: red; }</style>");
    assert!(
        f.unprovable.is_some_and(|c| c.contains("<title>")),
        "a decomposed head <title> is unprovable, got {:?}",
        f.unprovable
    );
}

#[test]
fn plain_svelte_head_without_title_stays_provable() {
    let f = facts(
        "<svelte:head><meta name=\"a\" content=\"b\" /></svelte:head>\n<style>meta { color: red; }\nlink { color: red; }</style>",
    );
    assert_proven(&f);
    assert!(f.used("meta"));
    assert!(!f.used("link"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Matcher ↔ emitter agreement on ENTITY-DECODED static attribute values.
// ─────────────────────────────────────────────────────────────────────────────

/// The matcher and the emitters consume the SAME decoded static attribute
/// value. `class="a&#32;b"` decodes to `a b` (the skeleton serializes the
/// decoded, re-escaped text), so the class-word matcher must see the two words
/// `a` / `b`: `.b` is used, the div is scoped, and `.c` stays pruned. Under a
/// RAW (undecoded) matcher value the single token `a&#32;b` matches neither
/// selector and the div silently emits UNSCOPED while the official compiler
/// scopes it — the matcher↔emitter asymmetry this test pins.
#[test]
fn entity_decoded_class_value_scopes_the_div_and_matches_word_selectors() {
    let source =
        "<div class=\"a&#32;b\">x</div>\n<style>.b { color: red; }\n.c { color: blue; }</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .unwrap_or_else(|e| panic!("lowering succeeds: {e:?}"));
    let plan = build_style_scope_plan(
        source,
        body_span(source),
        Some("Test.svelte"),
        CssMode::External,
        &ir,
        false,
    )
    .expect("the css body plans");

    // The matcher proved the relation (a static class value is never
    // fail-closed — the plan exists) and publishes the scope facts.
    let scope = plan.scope_facts();
    // The div's NodeId — the sole element node in the IR — is SCOPED (the
    // emitters read this same fact to bake `class="a b svelte-<hash>"`).
    let div_id = (0..ir.nodes.len() as u32)
        .map(crate::svelte::runtime::ir::NodeId)
        .find(|id| matches!(ir.node(*id), IrNode::Element(el) if el.tag == "div"))
        .expect("the IR contains the div element");
    assert!(
        scope.scoped.contains(&div_id),
        "the div must be scoped: `.b` matches the DECODED class word list `a b`"
    );
    assert_eq!(
        scope.hash_for(div_id),
        Some(scope.hash.as_str()),
        "the per-element injection read must agree with the scoped set"
    );

    // The per-selector used verdicts: `.b` matches a decoded word; `.c`
    // matches nothing (the negative — it must stay pruned).
    let mut used = Vec::new();
    let mut scoped_selectors = Vec::new();
    collect_rule_facts(source, &plan.ast.children, &mut used, &mut scoped_selectors);
    let verdict = |text: &str| {
        used.iter()
            .find(|(t, _)| t == text)
            .unwrap_or_else(|| panic!("selector `{text}` not found in {used:?}"))
            .1
    };
    assert!(
        verdict(".b"),
        "`.b` must be used: the matcher sees the DECODED value `a b`, not the raw `a&#32;b`"
    );
    assert!(
        !verdict(".c"),
        "`.c` matches no decoded class word and must stay unused"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Metamorphic entity equivalence: every encoding of the SAME decoded attribute
// value yields IDENTICAL matcher facts — the property, not just one fixture.
// ─────────────────────────────────────────────────────────────────────────────

/// `class="a b"` and `class="a&#32;b"` encode the SAME decoded value, so the
/// matcher facts must be IDENTICAL: the same per-selector used verdicts, the
/// same scoped selector texts, and the same scoped tag multiset. Under a RAW
/// (undecoded) matcher value the encoded variant's single token `a&#32;b`
/// contains no space, `.b` flips to unused and the div drops from the scoped
/// set — the equality fails. The literal variant grounds the shared verdicts,
/// so an identically-wrong pair cannot pass either.
#[test]
fn entity_encoded_space_class_yields_facts_identical_to_the_literal_form() {
    let style = "<style>.b { color: red; }\n.c { color: blue; }</style>";
    let literal = format!("<div class=\"a b\">x</div>\n{style}");
    let encoded = format!("<div class=\"a&#32;b\">x</div>\n{style}");
    let fl = facts(&literal);
    let fe = facts(&encoded);
    assert_proven(&fl);
    assert_proven(&fe);

    assert_eq!(
        fl.used, fe.used,
        "the numeric-space encoding must not change any used verdict"
    );
    assert_eq!(
        fl.scoped_selectors, fe.scoped_selectors,
        "the numeric-space encoding must not change the scoped selector set"
    );
    assert_eq!(
        fl.scoped_tags, fe.scoped_tags,
        "the numeric-space encoding must not change the scoped element set"
    );

    // Ground the shared verdicts on the literal side (equality alone would
    // also hold for an identically-broken pair).
    assert!(fl.used(".b"), "`.b` matches the decoded word list `a b`");
    assert!(!fl.used(".c"), "`.c` matches no word and stays unused");
    assert!(fl.scoped_tag("div"), "the div carries the scope class");
}

/// The named (`&lowbar;`), numeric (`&#95;`), and literal (`_`) encodings of
/// U+005F in an `id` value are the SAME decoded value, so `#a_b` must produce
/// IDENTICAL matcher facts for all three sources (used verdicts, scoped
/// selectors, scoped tags). Under a raw matcher value the two entity variants
/// fail the exact `id` comparison against `a_b` and diverge from the literal.
#[test]
fn named_and_numeric_underscore_entities_yield_facts_identical_to_the_literal_id() {
    let style = "<style>#a_b { color: red; }\n#a_c { color: blue; }</style>";
    let literal = facts(&format!("<div id=\"a_b\">x</div>\n{style}"));
    assert_proven(&literal);
    assert!(literal.used("#a_b"), "`#a_b` matches the decoded id `a_b`");
    assert!(
        !literal.used("#a_c"),
        "`#a_c` matches nothing and is unused"
    );
    assert!(literal.scoped_tag("div"), "the div carries the scope class");

    for encoded_source in [
        format!("<div id=\"a&#95;b\">x</div>\n{style}"),
        format!("<div id=\"a&lowbar;b\">x</div>\n{style}"),
    ] {
        let encoded = facts(&encoded_source);
        assert_proven(&encoded);
        assert_eq!(
            literal.used, encoded.used,
            "an entity encoding of `_` must not change any used verdict: {encoded_source}"
        );
        assert_eq!(
            literal.scoped_selectors, encoded.scoped_selectors,
            "an entity encoding of `_` must not change the scoped selectors: {encoded_source}"
        );
        assert_eq!(
            literal.scoped_tags, encoded.scoped_tags,
            "an entity encoding of `_` must not change the scoped elements: {encoded_source}"
        );
    }
}

/// `CssMode` selects emission/pruning only — it never changes the matcher
/// facts. The SAME component planned under `External` and `Injected` must
/// publish the SAME scoped node set and the SAME scope hash (whole
/// [`CssScopeFacts`] equality). The fixture's class value is entity-encoded,
/// so a raw (undecoded) matcher value empties the scoped set in both modes
/// and the non-vacuousness assertions fail.
#[test]
fn external_and_injected_modes_agree_on_scoped_set_and_hash() {
    let source =
        "<div class=\"a&#32;b\">x</div>\n<span>y</span>\n<style>.b { color: red; }</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .unwrap_or_else(|e| panic!("lowering succeeds: {e:?}"));
    let plan_facts = |mode: CssMode| {
        build_style_scope_plan(
            source,
            body_span(source),
            Some("Test.svelte"),
            mode,
            &ir,
            false,
        )
        .expect("the css body plans")
        .scope_facts()
    };

    let external = plan_facts(CssMode::External);
    let injected = plan_facts(CssMode::Injected);
    assert_eq!(
        external, injected,
        "External and Injected must agree on the scoped set AND the scope hash"
    );

    // Non-vacuousness: exactly the div is scoped (the span is not), and the
    // hash is a real scope hash.
    let div_id = (0..ir.nodes.len() as u32)
        .map(crate::svelte::runtime::ir::NodeId)
        .find(|id| matches!(ir.node(*id), IrNode::Element(el) if el.tag == "div"))
        .expect("the IR contains the div element");
    assert!(
        external.scoped.contains(&div_id),
        "the entity-encoded class `a&#32;b` decodes to `a b`, so `.b` scopes the div"
    );
    assert_eq!(
        external.scoped.len(),
        1,
        "only the div is scoped — the span matches no selector"
    );
    assert!(
        external.hash.starts_with("svelte-"),
        "the shared hash is a real scope hash, got {:?}",
        external.hash
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// MatchCertainty: the matcher's internal three-valued verdict. `Yes` =
// provably matches, `No` = provably does not match, `Maybe` = the official
// fail-open "cannot prove" verdicts (the former unconditional `true`). The
// PRODUCTION projection is `Yes | Maybe ⇒ used/scoped`, `No ⇒ pruned` —
// asserted against the same plan facts the pre-tri-state bool produced.
// ─────────────────────────────────────────────────────────────────────────────

use super::MatchCertainty;

/// The per-top-level-complex-selector certainties of one component, keyed by
/// selector source text (prune visit order).
fn certainties(source: &str) -> Vec<(String, MatchCertainty)> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc)
        .unwrap_or_else(|e| panic!("lowering succeeds: {e:?}"));
    crate::svelte::runtime::css::style_selector_certainties_for_test(source, body_span(source), &ir)
        .unwrap_or_else(|e| panic!("the matcher proves: {e:?}"))
        .into_iter()
        .map(|(span, certainty)| (slice(source, span), certainty))
        .collect()
}

/// The certainty of the selector whose source text is `text` (panics on an
/// unknown selector so a typo'd test fails loudly).
fn certainty_of(rows: &[(String, MatchCertainty)], text: &str) -> MatchCertainty {
    rows.iter()
        .find(|(t, _)| t == text)
        .unwrap_or_else(|| panic!("selector `{text}` not found in {rows:?}"))
        .1
}

#[test]
fn certainty_three_valued_logic_truth_tables() {
    use MatchCertainty::{Maybe, No, Yes};
    // AND = min (a compound is only as certain as its weakest constraint).
    assert_eq!(Yes.and(Yes), Yes);
    assert_eq!(Yes.and(Maybe), Maybe);
    assert_eq!(Maybe.and(Yes), Maybe);
    assert_eq!(Maybe.and(Maybe), Maybe);
    assert_eq!(Yes.and(No), No);
    assert_eq!(No.and(Yes), No);
    assert_eq!(Maybe.and(No), No);
    assert_eq!(No.and(Maybe), No);
    assert_eq!(No.and(No), No);
    // OR = max (one proven branch proves the disjunction).
    assert_eq!(Yes.or(No), Yes);
    assert_eq!(No.or(Yes), Yes);
    assert_eq!(Maybe.or(No), Maybe);
    assert_eq!(No.or(Maybe), Maybe);
    assert_eq!(Yes.or(Maybe), Yes);
    assert_eq!(Maybe.or(Yes), Yes);
    assert_eq!(No.or(No), No);
    // The PRODUCTION projection: exactly the pre-tri-state bool (`Maybe` used
    // to be `true`). `Maybe` is NEVER treated as `No`.
    assert!(Yes.might_match());
    assert!(Maybe.might_match());
    assert!(!No.might_match());
}

#[test]
fn certainty_yes_for_a_provable_static_match() {
    let source = "<div class=\"a b\">x</div>\n<style>.b { color: red; }</style>";
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".b"), MatchCertainty::Yes);
}

#[test]
fn certainty_no_for_a_provable_static_no_match() {
    let source = "<div class=\"a\">x</div>\n<style>.zz { color: red; }</style>";
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".zz"), MatchCertainty::No);
    // The production projection: a provable no-match is pruned as unused.
    let f = facts(source);
    assert!(!f.used(".zz"));
    assert!(!f.scoped_tag("div"));
}

#[test]
fn certainty_maybe_for_a_dynamic_unknown_value() {
    // `class={c}` — `expression_possible_values` returns UNKNOWN (`Ok(None)`),
    // the official "may match anything" bail: the FORMER unconditional `true`
    // is now OBSERVED as `Maybe`.
    let source = "<script>let c = $state('x');</script>\n<div class={c}>x</div>\n<style>.b { color: red; }</style>";
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".b"), MatchCertainty::Maybe);
}

#[test]
fn certainty_maybe_for_a_spread_attribute() {
    let source = "<script>let rest = $state({});</script>\n<div {...rest}>x</div>\n<style>.b { color: red; }</style>";
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".b"), MatchCertainty::Maybe);
}

#[test]
fn certainty_maybe_never_projects_to_no() {
    // The Maybe verdict keeps the official fail-open behavior: the selector
    // stays USED and the element stays SCOPED — byte-identical to the
    // pre-tri-state `true`.
    let source = "<script>let c = $state('x');</script>\n<div class={c}>x</div>\n<style>.b { color: red; }</style>";
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".b"), MatchCertainty::Maybe);
    let f = facts(source);
    assert!(f.used(".b"), "a Maybe selector is used, never pruned");
    assert!(
        f.scoped_tag("div"),
        "a Maybe element is scoped, never dropped"
    );
}

#[test]
fn certainty_yes_for_a_provable_type_and_enumerated_values_stay_maybe() {
    // A TYPE selector on a static intrinsic element is a proof.
    let source = "<p>x</p>\n<style>p { color: red; }</style>";
    assert_eq!(certainty_of(&certainties(source), "p"), MatchCertainty::Yes);
    // An ENUMERATED dynamic value (`cond ? 'a' : 'b'`) that CAN match stays
    // `Maybe` — the enumeration lists POSSIBLE runtime values, not a proof
    // that the matching branch is taken.
    let source = "<script>let cond = $state(true);</script>\n<div class={cond ? 'a' : 'b'}>x</div>\n<style>.a { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".a"),
        MatchCertainty::Maybe
    );
    // An enumerated value where NO possible value matches is a provable
    // no-match: every runtime value is known and none matches.
    let source = "<script>let cond = $state(true);</script>\n<div class={cond ? 'a' : 'b'}>x</div>\n<style>.zz { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".zz"),
        MatchCertainty::No
    );
}

#[test]
fn certainty_maybe_for_class_directive_and_whitelisted_attribute() {
    // A `class:b={cond}` directive toggles at runtime — official treats it as
    // matching without proof.
    let source = "<script>let cond = $state(true);</script>\n<div class:b={cond}>x</div>\n<style>.b { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".b"),
        MatchCertainty::Maybe
    );
    // `details[open]` is officially whitelisted (the runtime may toggle it) —
    // assumed matching even when the attribute is absent in the template.
    let source = "<details>x</details>\n<style>details[open] { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), "details[open]"),
        MatchCertainty::Maybe
    );
    let f = facts(source);
    assert!(
        f.used("details[open]"),
        "the whitelist keeps the selector used"
    );
}

#[test]
fn certainty_composes_across_compounds_and_combinators() {
    // `.a .b` with both provable: Yes.
    let source =
        "<div class=\"a\"><p class=\"b\">x</p></div>\n<style>.a .b { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".a .b"),
        MatchCertainty::Yes
    );
    // `.a .b` where the ANCESTOR hop is dynamic: the compound is only as
    // certain as its weakest constraint — Maybe, still used.
    let source = "<script>let c = $state('x');</script>\n<div class={c}><p class=\"b\">x</p></div>\n<style>.a .b { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".a .b"),
        MatchCertainty::Maybe
    );
    let f = facts(source);
    assert!(f.used(".a .b"));
    // `.a .b` where the ancestor provably can't match: No, pruned.
    let source =
        "<div class=\"zz\"><p class=\"b\">x</p></div>\n<style>.a .b { color: red; }</style>";
    assert_eq!(
        certainty_of(&certainties(source), ".a .b"),
        MatchCertainty::No
    );
    let f = facts(source);
    assert!(!f.used(".a .b"));
}

#[test]
fn certainty_rows_cover_every_top_level_selector_in_source_order() {
    let source = "<div class=\"a\">x</div>\n<style>.a { color: red; } .zz { color: blue; }</style>";
    let rows = certainties(source);
    assert_eq!(
        rows,
        vec![
            (".a".to_string(), MatchCertainty::Yes),
            (".zz".to_string(), MatchCertainty::No),
        ],
        "one row per top-level complex selector, prune visit order, No included"
    );
}

/// The production projection over the HAND-VENDORED corpus fixture
/// `css/entity_class_scoped.svelte`: the scope-prune / used-selector output
/// is byte-identical to the pre-tri-state matcher (`.b` used via the decoded
/// `a&#32;b` ⇒ `a b`, `.c` pruned), and the certainties discriminate the two.
#[test]
fn corpus_entity_class_scoped_projection_is_unchanged_by_the_tri_state() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/svelte_oracle_corpus/fixtures/css/entity_class_scoped.svelte"
    ));
    let f = facts(source);
    assert!(f.used(".b"), "the decoded `a b` word list matches `.b`");
    assert!(!f.used(".c"), "`.c` stays pruned");
    assert!(f.scoped_selector(".b"));
    assert!(!f.scoped_selector(".c"));
    assert!(f.scoped_tag("div"));
    let rows = certainties(source);
    assert_eq!(certainty_of(&rows, ".b"), MatchCertainty::Yes);
    assert_eq!(certainty_of(&rows, ".c"), MatchCertainty::No);
}

/// The production projection over the HAND-VENDORED corpus fixture
/// `css/combinators_attributes.svelte` is unchanged: the case-insensitive
/// attribute test and the `^=` prefix test keep their verdicts.
#[test]
fn corpus_combinators_attributes_projection_is_unchanged_by_the_tri_state() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/svelte_oracle_corpus/fixtures/css/combinators_attributes.svelte"
    ));
    let f = facts(source);
    assert!(f.used(".list > li[data-kind=\"a\" i]"));
    assert!(f.used("p[title^=\"no\"]"));
    assert!(f.scoped_tag("li"));
    assert!(f.scoped_tag("p"));
    let rows = certainties(source);
    assert_eq!(
        certainty_of(&rows, ".list > li[data-kind=\"a\" i]"),
        MatchCertainty::Yes
    );
    assert_eq!(certainty_of(&rows, "p[title^=\"no\"]"), MatchCertainty::Yes);
}
