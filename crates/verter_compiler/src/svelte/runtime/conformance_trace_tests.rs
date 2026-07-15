//! Conformance-trace tests (compiled only under the `conformance-trace`
//! feature — the module itself is feature-gated).
//!
//! Pins the two trace halves: (1) static-attribute PROVENANCE — quoting +
//! HTML entity source representation, captured at the attribute-lowering
//! producer boundary from the raw parse span + its delimiter byte; (2) the
//! CSS matcher's tri-state facts — per-selector [`MatchCertainty`],
//! used/scoped selector spans, scoped element identities. The GROUNDING
//! metamorphic pair (`class="a b"` ≡ `class="a&#32;b"` ≡ `class="a&#x20;b"`)
//! must record DISTINCT source representations while the decoded matcher
//! facts stay IDENTICAL.

use oxc_allocator::Allocator;

use super::{
    capture, compile_client_with_conformance_trace, AttrProvenance, AttrQuoting,
    AttrSourceRepresentation, ConformanceTrace, MatchCertainty,
};
use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::{compile_client, SvelteRuntimeOptions};

/// Compile `source` under an active capture and return the trace (panicking
/// on a compile failure so a broken fixture fails loudly).
fn trace_of(source: &str) -> ConformanceTrace {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    let (result, trace) =
        compile_client_with_conformance_trace(source, &parsed, &opts, &alloc, false);
    result.unwrap_or_else(|e| panic!("the fixture compiles: {e:?}"));
    trace
}

/// One style match's span-free fact row: (per-selector certainties, used
/// count, scoped count, scoped element `(node, tag)` identities).
type SpanFreeStyleFacts = (Vec<MatchCertainty>, usize, usize, Vec<(u32, String)>);

/// The span-free matcher-fact projection used for cross-variant identity
/// (byte spans legitimately differ across representation variants — the
/// variants have different source lengths — so identity compares the DECODED
/// facts: certainties, used/scoped counts, scoped element identities).
fn span_free_matcher_facts(trace: &ConformanceTrace) -> Vec<SpanFreeStyleFacts> {
    trace
        .style_matches
        .iter()
        .map(|m| {
            (
                m.selector_certainties.iter().map(|f| f.certainty).collect(),
                m.used_selector_spans.len(),
                m.scoped_selector_spans.len(),
                m.scoped_elements
                    .iter()
                    .map(|e| (e.node, e.tag.clone()))
                    .collect(),
            )
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// The grounding metamorphic pair.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn grounding_pair_distinct_representations_identical_matcher_facts() {
    let literal = "<div class=\"a b\">x</div>\n<style>.b { color: red; }</style>";
    let decimal = "<div class=\"a&#32;b\">x</div>\n<style>.b { color: red; }</style>";
    let hex = "<div class=\"a&#x20;b\">x</div>\n<style>.b { color: red; }</style>";
    let t_literal = trace_of(literal);
    let t_decimal = trace_of(decimal);
    let t_hex = trace_of(hex);

    // DISTINCT source representations — the lexical provenance the IR erases.
    let rep = |t: &ConformanceTrace| {
        assert_eq!(t.static_attrs.len(), 1, "one static attribute");
        assert_eq!(t.static_attrs[0].name, "class");
        assert_eq!(t.static_attrs[0].quoting, AttrQuoting::Quoted);
        t.static_attrs[0].representation
    };
    assert_eq!(rep(&t_literal), Some(AttrSourceRepresentation::Literal));
    assert_eq!(
        rep(&t_decimal),
        Some(AttrSourceRepresentation::HtmlDecimalEntity)
    );
    assert_eq!(rep(&t_hex), Some(AttrSourceRepresentation::HtmlHexEntity));

    // IDENTICAL decoded matcher facts — the word list `a b` drives all three.
    let facts = span_free_matcher_facts(&t_literal);
    assert_eq!(facts, span_free_matcher_facts(&t_decimal));
    assert_eq!(facts, span_free_matcher_facts(&t_hex));

    // The GROUNDED expected verdict: `.b` provably MATCHES.
    assert_eq!(t_literal.style_matches.len(), 1);
    let m = &t_literal.style_matches[0];
    assert_eq!(m.selector_certainties.len(), 1);
    assert_eq!(m.selector_certainties[0].certainty, MatchCertainty::Yes);
    assert_eq!(m.used_selector_spans.len(), 1, "`.b` is used");
    assert_eq!(m.scoped_elements.len(), 1);
    assert_eq!(m.scoped_elements[0].tag, "div");
}

// ─────────────────────────────────────────────────────────────────────────────
// Quoting + representation provenance.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn quoting_captures_quoted_unquoted_and_boolean_valueless() {
    let source = "<div class=\"a\" id='b' title=c data-x>x</div>";
    let trace = trace_of(source);
    assert_eq!(
        trace.static_attrs,
        vec![
            AttrProvenance {
                name: "class".to_string(),
                quoting: AttrQuoting::Quoted,
                representation: Some(AttrSourceRepresentation::Literal),
            },
            AttrProvenance {
                name: "id".to_string(),
                quoting: AttrQuoting::Quoted,
                representation: Some(AttrSourceRepresentation::Literal),
            },
            AttrProvenance {
                name: "title".to_string(),
                quoting: AttrQuoting::Unquoted,
                representation: Some(AttrSourceRepresentation::Literal),
            },
            AttrProvenance {
                name: "data-x".to_string(),
                quoting: AttrQuoting::BooleanValueless,
                representation: None,
            },
        ],
        "encounter order; a valueless attribute has no representation"
    );
}

#[test]
fn representation_distinguishes_named_and_mixed_entity_forms() {
    // A NAMED reference.
    let trace = trace_of("<div title=\"a&amp;b\">x</div>");
    assert_eq!(
        trace.static_attrs[0].representation,
        Some(AttrSourceRepresentation::HtmlNamedEntity)
    );
    // TWO DISTINCT reference forms (named + decimal) — Mixed. Literal text
    // AROUND a single form does NOT make it Mixed (the grounding pair pins
    // that: `a&#32;b` is Decimal).
    let trace = trace_of("<div title=\"a&amp;&#65;b\">x</div>");
    assert_eq!(
        trace.static_attrs[0].representation,
        Some(AttrSourceRepresentation::Mixed)
    );
    // An UNDECODABLE reference stays literal text: the official numeric
    // pattern accepts a lowercase `x` only, so `&#X41;` is NOT an entity.
    let trace = trace_of("<div title=\"a&#X41;b\">x</div>");
    assert_eq!(
        trace.static_attrs[0].representation,
        Some(AttrSourceRepresentation::Literal)
    );
}

#[test]
fn unquoted_entity_value_records_unquoted_decimal_and_matches_decoded() {
    // `class=a&#32;b` — an UNQUOTED value with no literal whitespace that
    // DECODES to the word list `a b`: provenance says (Unquoted, Decimal),
    // and the matcher proves `.b` from the decoded value.
    let source = "<div class=a&#32;b>x</div>\n<style>.b { color: red; }</style>";
    let trace = trace_of(source);
    assert_eq!(trace.static_attrs.len(), 1);
    assert_eq!(trace.static_attrs[0].quoting, AttrQuoting::Unquoted);
    assert_eq!(
        trace.static_attrs[0].representation,
        Some(AttrSourceRepresentation::HtmlDecimalEntity)
    );
    let m = &trace.style_matches[0];
    assert_eq!(m.selector_certainties[0].certainty, MatchCertainty::Yes);
    assert_eq!(m.scoped_elements[0].tag, "div");
}

#[test]
fn only_static_attributes_are_recorded() {
    let source =
        "<script>let c = $state('x');</script>\n<div id=\"x\" class={c} title=\"a{c}b\">x</div>";
    let trace = trace_of(source);
    assert_eq!(
        trace.static_attrs,
        vec![AttrProvenance {
            name: "id".to_string(),
            quoting: AttrQuoting::Quoted,
            representation: Some(AttrSourceRepresentation::Literal),
        }],
        "dynamic and mixed attributes are matcher facts, not static provenance"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Matcher-fact exposure (per-selector certainty incl. `No`, used/scoped).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn selector_certainty_rows_include_unused_no_rows() {
    let source = "<p class=\"a\">y</p>\n<style>.a { color: red; } .zz { color: blue; }</style>";
    let trace = trace_of(source);
    let m = &trace.style_matches[0];
    assert_eq!(
        m.selector_certainties
            .iter()
            .map(|f| f.certainty)
            .collect::<Vec<_>>(),
        vec![MatchCertainty::Yes, MatchCertainty::No],
        "prune visit order, the unused `.zz` row INCLUDED as No"
    );
    assert_eq!(
        m.used_selector_spans.len(),
        1,
        "the No row is absent from the used set"
    );
    assert_eq!(m.scoped_selector_spans.len(), 1);
    assert_eq!(m.scoped_elements.len(), 1);
    assert_eq!(m.scoped_elements[0].tag, "p");
}

#[test]
fn dynamic_value_records_maybe_and_stays_used() {
    let source = "<script>let c = $state('x');</script>\n<div class={c}>x</div>\n<style>.b { color: red; }</style>";
    let trace = trace_of(source);
    let m = &trace.style_matches[0];
    assert_eq!(m.selector_certainties.len(), 1);
    assert_eq!(m.selector_certainties[0].certainty, MatchCertainty::Maybe);
    assert_eq!(
        m.used_selector_spans,
        vec![m.selector_certainties[0].selector_span],
        "Maybe is never treated as No: the selector stays used"
    );
    assert_eq!(m.scoped_elements.len(), 1, "the element stays scoped");
}

// ─────────────────────────────────────────────────────────────────────────────
// Capture discipline: inert without a capture, deterministic, isolated.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn recording_is_inert_without_an_active_capture() {
    let source = "<div class=\"a b\">x</div>\n<style>.b { color: red; }</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("Test.svelte".to_string()),
        runes: Some(true),
        ..Default::default()
    };
    // A compile WITHOUT a capture records nowhere…
    compile_client(source, &parsed, &opts, &alloc, false, false).expect("compiles");
    // …and a later capture starts EMPTY (no cross-request bleed).
    let ((), trace) = capture(|| ());
    assert_eq!(trace, ConformanceTrace::default());
}

#[test]
fn capture_is_deterministic_across_identical_runs() {
    let source =
        "<div class=\"a&#32;b\">x</div>\n<style>.b { color: red; } .zz { color: blue; }</style>";
    assert_eq!(
        trace_of(source),
        trace_of(source),
        "byte-identical trace (spans included) across identical runs"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Zero-cost: the trace is a SIDE CHANNEL — the production IR carries no trace
// state even WITH the feature enabled (the feature-off half of the proof is
// the default-suite guard `svelte_conformance_trace_zero_cost_guard.rs` plus
// the compiler itself: with the feature off the module does not exist, so an
// ungated production reference cannot compile).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn prod_ir_static_attr_value_carries_no_trace_field_under_the_feature() {
    assert_eq!(
        std::mem::size_of::<crate::svelte::runtime::ir::StaticAttrValue>(),
        std::mem::size_of::<String>(),
        "StaticAttrValue is exactly its decoded String — no trace field"
    );
}
