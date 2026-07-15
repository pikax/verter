//! The AUTHORITATIVE manifest-driven metamorphic executor over the manifest's
//! [`SemanticValueFamily`] entries: representation-EQUIVALENT inputs must
//! produce IDENTICAL span-free matcher facts, and each family's GROUNDED
//! expected verdict must hold on EVERY variant (so two identically-wrong
//! implementations cannot pass by mutual equality alone).
//!
//! Each family rendering is placed into its own language slot of one minimal
//! subject fixture — the HTML template-value slot, the CSS selector slot, or
//! the JS expression-value slot, selected EXHAUSTIVELY by the rendering's
//! typed [`RenderingKind`] — and lowered through Verter's PRODUCTION client
//! pipeline under the feature-gated conformance trace. Identity compares the
//! SPAN-FREE matcher-fact projection (per-selector [`MatchCertainty`] rows in
//! prune visit order, used/scoped selector counts, scoped element `(node,
//! tag)` identities): byte spans legitimately differ across variants because
//! the source SPELLING differs, and the IR-arena node ordinal + tag are the
//! variant-stable element identity.
//!
//! The focused grounding-pair discrimination test stays in
//! `verter_compiler`'s `conformance_trace_tests.rs`; THIS executor is the
//! manifest-driven authority (it runs whatever families the manifest
//! declares, not a hand-picked pair).

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::conformance_trace::{
    compile_client_with_conformance_trace, ConformanceTrace, MatchCertainty,
};
use verter_compiler::svelte::runtime::SvelteRuntimeOptions;
use verter_svelte_conformance::manifest::manifest;
use verter_svelte_conformance::model::{
    FamilyRendering, MatchOutcome, RenderingKind, SemanticValueFamily,
};

/// The pinned family inventory (names in manifest order). A manifest family
/// change legitimately moves this pin.
const FAMILY_NAMES: [&str; 4] = [
    "class-token-space-separator",
    "ampersand-class-token",
    "css-escape-spellings",
    "js-string-escapes",
];

// ---------------------------------------------------------------------------
// Variant rendering + lowering
// ---------------------------------------------------------------------------

/// Render one family variant into its minimal subject fixture, EXHAUSTIVE
/// over the typed rendering language:
///
/// - HTML template-language kinds: the rendering IS the authored `class`
///   value; the style rule carries the family's canonical verdict selector.
/// - CSS selector-language kinds: the rendering IS the authored class
///   selector; the template carries the family's decoded base value spelled
///   literally.
/// - JS expression-language kinds: the rendering IS the expression value
///   (`class={…}`); the style rule carries the canonical verdict selector.
fn render_variant(family: &SemanticValueFamily, rendering: &FamilyRendering) -> String {
    let style =
        |selector: &str| format!("<style>\n\t{selector} {{\n\t\tcolor: red;\n\t}}\n</style>");
    match rendering.kind {
        RenderingKind::TemplateLiteral
        | RenderingKind::HtmlNamedEntity
        | RenderingKind::HtmlDecimalEntity
        | RenderingKind::HtmlHexEntity => format!(
            "<div class=\"{}\">x</div>\n\n{}\n",
            rendering.rendered,
            style(family.verdict.selector)
        ),
        RenderingKind::CssEscapeHex | RenderingKind::CssEscapeChar => format!(
            "<div class=\"{}\">x</div>\n\n{}\n",
            family.base_value,
            style(&format!(".{}", rendering.rendered))
        ),
        RenderingKind::JsStringLiteral | RenderingKind::JsStringEscape => format!(
            "<div class={{{}}}>x</div>\n\n{}\n",
            rendering.rendered,
            style(family.verdict.selector)
        ),
    }
}

/// Lower one variant source through the production client pipeline under a
/// capture (identical options for every variant, so the ONLY cross-variant
/// difference is the representation itself) and return its trace.
fn lower_variant(context: &str, source: &str) -> ConformanceTrace {
    let allocator = Allocator::default();
    let parsed = parse_svelte(source);
    let options = SvelteRuntimeOptions {
        filename: Some("Family.svelte".to_string()),
        ..Default::default()
    };
    let (result, trace) =
        compile_client_with_conformance_trace(source, &parsed, &options, &allocator, false);
    result.unwrap_or_else(|error| panic!("{context}: the variant fixture compiles: {error:?}"));
    trace
}

/// The span-free matcher-fact projection used for cross-variant identity
/// (byte spans legitimately differ across representation variants — the
/// variants have different source lengths — so identity compares the DECODED
/// facts: certainty rows, used/scoped counts, scoped element identities).
type SpanFreeFacts = Vec<(Vec<MatchCertainty>, usize, usize, Vec<(u32, String)>)>;

fn span_free_matcher_facts(trace: &ConformanceTrace) -> SpanFreeFacts {
    trace
        .style_matches
        .iter()
        .map(|style| {
            (
                style
                    .selector_certainties
                    .iter()
                    .map(|fact| fact.certainty)
                    .collect(),
                style.used_selector_spans.len(),
                style.scoped_selector_spans.len(),
                style
                    .scoped_elements
                    .iter()
                    .map(|element| (element.node, element.tag.clone()))
                    .collect(),
            )
        })
        .collect()
}

/// Whether a rendering kind carries the subject value through a JS
/// EXPRESSION (`class={…}`) rather than a static spelling.
fn value_via_expression(kind: RenderingKind) -> bool {
    match kind {
        RenderingKind::JsStringLiteral | RenderingKind::JsStringEscape => true,
        RenderingKind::TemplateLiteral
        | RenderingKind::HtmlNamedEntity
        | RenderingKind::HtmlDecimalEntity
        | RenderingKind::HtmlHexEntity
        | RenderingKind::CssEscapeHex
        | RenderingKind::CssEscapeChar => false,
    }
}

/// The grounded facts a declared verdict demands of EVERY variant of a
/// family: the subject selector's certainty row, whether it stays used, and
/// whether the subject `div` is scoped.
///
/// ONE encoded conservatism, the same one the corpus observation gate
/// (`common::expected_subject_certainty`) pins for the manifest's `Dynamic`
/// forms: a declared Match whose value rides a JS EXPRESSION observes
/// `Maybe` — the matcher mirrors the official `get_possible_values`
/// semantics, where an enumerated POSSIBLE value is not a proof the matching
/// branch is taken at runtime — while the production used/scoped projection
/// stays IDENTICAL to Match (kept + scoped).
fn grounded_expectation(
    outcome: MatchOutcome,
    via_expression: bool,
) -> (MatchCertainty, usize, usize) {
    match outcome {
        // Kept + scoped: provably for a static spelling, fail-open for an
        // expression value (identical production projection).
        MatchOutcome::Match => {
            if via_expression {
                (MatchCertainty::Maybe, 1, 1)
            } else {
                (MatchCertainty::Yes, 1, 1)
            }
        }
        // Provably unused: pruned, nothing scoped.
        MatchOutcome::NoMatch => (MatchCertainty::No, 0, 0),
        // Fail-open: kept + scoped without a proof.
        MatchOutcome::Maybe => (MatchCertainty::Maybe, 1, 1),
    }
}

/// Assert one variant trace satisfies the family's grounded verdict.
fn assert_grounded_verdict(
    context: &str,
    family: &SemanticValueFamily,
    kind: RenderingKind,
    trace: &ConformanceTrace,
) {
    let (certainty, used, scoped) =
        grounded_expectation(family.verdict.outcome, value_via_expression(kind));
    assert_eq!(
        trace.style_matches.len(),
        1,
        "{context}: exactly one style matcher run"
    );
    let style = &trace.style_matches[0];
    assert_eq!(
        style
            .selector_certainties
            .iter()
            .map(|fact| fact.certainty)
            .collect::<Vec<_>>(),
        vec![certainty],
        "{context}: the grounded verdict {:?} for selector `{}`",
        family.verdict.outcome,
        family.verdict.selector
    );
    assert_eq!(
        style.used_selector_spans.len(),
        used,
        "{context}: used-selector count for the grounded verdict"
    );
    assert_eq!(
        style.scoped_elements.len(),
        scoped,
        "{context}: scoped-element count for the grounded verdict"
    );
    if scoped > 0 {
        assert_eq!(
            style.scoped_elements[0].tag, "div",
            "{context}: the scoped subject is the `div`"
        );
    }
}

// ---------------------------------------------------------------------------
// Non-vacuity: the family inventory itself.
// ---------------------------------------------------------------------------

/// Every manifest family is a REAL static equivalence class: ≥ 2 renderings,
/// pairwise-DISTINCT representation kinds, pairwise-DISTINCT rendered
/// spellings AND pairwise-distinct produced fixture source bytes (distinct
/// enum kinds alone could silently collapse onto one spelling, turning the
/// equality comparison into self-equality), and no uncertainty form anywhere
/// (`Dynamic` / `Spread` are uncertainty forms, not representation
/// equivalents — [`RenderingKind`] cannot even spell them, and every
/// template-language kind must map to a non-uncertainty template level).
#[test]
fn families_are_nonvacuous_distinct_static_representations() {
    let families = manifest().families();
    assert_eq!(
        families
            .iter()
            .map(|family| family.name)
            .collect::<Vec<_>>(),
        FAMILY_NAMES,
        "the pinned family inventory moved; re-pin FAMILY_NAMES with the manifest"
    );
    for family in families {
        assert!(
            family.renderings.len() >= 2,
            "{}: a family needs ≥ 2 equivalent renderings to be metamorphic",
            family.name
        );
        for (index, rendering) in family.renderings.iter().enumerate() {
            for other in &family.renderings[index + 1..] {
                assert_ne!(
                    rendering.kind, other.kind,
                    "{}: renderings must carry pairwise-distinct representation kinds",
                    family.name
                );
                assert_ne!(
                    rendering.rendered, other.rendered,
                    "{}: renderings must carry pairwise-distinct rendered spellings — \
                     two kinds sharing one spelling collapse the metamorphic \
                     comparison to self-equality",
                    family.name
                );
                assert_ne!(
                    render_variant(family, rendering),
                    render_variant(family, other),
                    "{}: variants `{}` and `{}` must produce pairwise-distinct fixture \
                     SOURCE BYTES — identical sources compare a compilation against \
                     itself and certify nothing",
                    family.name,
                    rendering.kind.id(),
                    other.kind.id()
                );
            }
            if let Some(template) = rendering.kind.template_representation() {
                assert!(
                    !template.is_uncertainty_form(),
                    "{}: uncertainty forms are never static family members",
                    family.name
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The executor: identical span-free facts + the grounded verdict.
// ---------------------------------------------------------------------------

/// For EVERY manifest family: lower every rendering variant through the
/// production pipeline and assert (a) all variants produce IDENTICAL
/// span-free matcher facts, and (b) the family's GROUNDED verdict holds on
/// every variant — not just cross-variant equality.
#[test]
fn family_variants_produce_identical_span_free_matcher_facts() {
    let families = manifest().families();
    assert_eq!(families.len(), FAMILY_NAMES.len(), "family count");
    for family in families {
        let variants: Vec<(RenderingKind, ConformanceTrace)> = family
            .renderings
            .iter()
            .map(|rendering| {
                let context = format!("{} / {}", family.name, rendering.kind.id());
                let source = render_variant(family, rendering);
                (rendering.kind, lower_variant(&context, &source))
            })
            .collect();

        // (a) The family-equality property: every variant's span-free facts
        // are IDENTICAL to the anchor's.
        let (anchor_kind, anchor) = &variants[0];
        let anchor_facts = span_free_matcher_facts(anchor);
        assert!(
            !anchor_facts.is_empty(),
            "{}: the anchor variant must record matcher facts (non-vacuous comparison)",
            family.name
        );
        for (kind, trace) in &variants[1..] {
            assert_eq!(
                span_free_matcher_facts(trace),
                anchor_facts,
                "{}: representation-equivalent variants `{}` and `{}` must produce \
                 IDENTICAL span-free matcher facts",
                family.name,
                anchor_kind.id(),
                kind.id()
            );
        }

        // (b) The grounded verdict, on EVERY variant — cross-variant equality
        // alone cannot certify two identically-wrong implementations.
        for (kind, trace) in &variants {
            let context = format!("{} / {}", family.name, kind.id());
            assert_grounded_verdict(&context, family, *kind, trace);
        }
    }
}

// ---------------------------------------------------------------------------
// Discrimination: a deliberately-different member yields DIFFERENT facts.
// ---------------------------------------------------------------------------

/// The executor's comparison is DISCRIMINATING: a control variant whose
/// spelling DECODES DIFFERENTLY (`a&#33;b` decodes to the single token
/// `a!b`, not the word list `a`,`b`) must produce span-free matcher facts
/// that DIFFER from its family's shared facts, and must FLIP the grounded
/// verdict — the executor cannot be satisfied by two identically-wrong
/// implementations or a decode-insensitive matcher.
#[test]
fn mutated_decode_control_variant_diverges_from_family_facts() {
    let families = manifest().families();
    let family = families
        .iter()
        .find(|family| family.name == "class-token-space-separator")
        .expect("the space-separator family is a manifest family");

    // The family's shared facts, from its canonical first rendering.
    let anchor_source = render_variant(family, &family.renderings[0]);
    let anchor = lower_variant(family.name, &anchor_source);
    let anchor_facts = span_free_matcher_facts(&anchor);

    // The control mutant: same value POSITIONS, one decode-divergent entity
    // (`&#33;` = `!`, not the `&#32;` space separator) — NOT a family member.
    let mutant_source = format!(
        "<div class=\"a&#33;b\">x</div>\n\n<style>\n\t{} {{\n\t\tcolor: red;\n\t}}\n</style>\n",
        family.verdict.selector
    );
    let mutant = lower_variant("decode-divergent control", &mutant_source);
    let mutant_facts = span_free_matcher_facts(&mutant);

    assert_ne!(
        mutant_facts, anchor_facts,
        "a decode-divergent control member must produce DIFFERENT span-free facts \
         (the family-equality comparison is non-vacuous)"
    );
    // And the grounded verdict flips: `.b` cannot match the single token `a!b`.
    assert_eq!(mutant.style_matches.len(), 1);
    assert_eq!(
        mutant.style_matches[0].selector_certainties[0].certainty,
        MatchCertainty::No,
        "the grounded-verdict assertion discriminates: the control member flips Match → No"
    );
    assert!(
        mutant.style_matches[0].scoped_elements.is_empty(),
        "the control member scopes nothing"
    );
}

// ---------------------------------------------------------------------------
// Discrimination, PER FAMILY: every representation family carries a
// decode-divergent negative control.
// ---------------------------------------------------------------------------

/// One decode-divergent NEGATIVE control per manifest family, 1:1 in manifest
/// order. Each control stays in its family's OWN representation language
/// (its kind is one of the family's kinds, so it rides the identical fixture
/// slot) but its DECODED value no longer satisfies the family's grounded
/// selector — the matcher facts and the grounded verdict must FLIP
/// (kept + scoped → `No` + pruned + nothing scoped):
///
/// - `class-token-space-separator`: `a&#33;b` decodes to the single token
///   `a!b` (`&#33;` = `!`, not the `&#32;` space separator) — `.b` is absent.
/// - `ampersand-class-token`: `a&#38;c` decodes to the token `a&c` — the
///   escaped selector `.a\26 b` (ident `a&b`) is absent.
/// - `css-escape-spellings`: the selector `.a\26 c` decodes to the ident
///   `a&c` — it cannot match the authored class token `a&b`.
/// - `js-string-escapes`: the expression value `'a\u0063'` decodes to the
///   single token `ac` (`\u0063` = `c`, not the member spelling's `\u0020`
///   space separator) — its enumerated possible values exclude `b`, so the
///   verdict is a PROVEN `No`, never the fail-open `Maybe` a
///   decode-insensitive treats-every-expression-as-unknown matcher would
///   produce.
const FAMILY_NEGATIVE_CONTROLS: [(&str, FamilyRendering); 4] = [
    (
        "class-token-space-separator",
        FamilyRendering {
            kind: RenderingKind::HtmlDecimalEntity,
            rendered: "a&#33;b",
        },
    ),
    (
        "ampersand-class-token",
        FamilyRendering {
            kind: RenderingKind::HtmlDecimalEntity,
            rendered: "a&#38;c",
        },
    ),
    (
        "css-escape-spellings",
        FamilyRendering {
            kind: RenderingKind::CssEscapeHex,
            rendered: "a\\26 c",
        },
    ),
    (
        "js-string-escapes",
        FamilyRendering {
            kind: RenderingKind::JsStringEscape,
            rendered: "'a\\u0063'",
        },
    ),
];

/// EVERY family's equivalence is a COMPUTED decode property, not a blanket
/// verdict: for each manifest family, a control member of the family's own
/// representation language whose decoded value diverges must (a) produce
/// span-free matcher facts DIFFERENT from the family's shared facts and
/// (b) FLIP the grounded verdict to a proven absence (`No`, pruned, nothing
/// scoped).
///
/// This is the per-family non-vacuity proof: an implementation that never
/// decodes a family's language — e.g. a matcher treating EVERY JS expression
/// value as unknown (`Maybe` + kept + scoped, exactly the JS family's
/// grounded facts) — satisfies the family equality AND the grounded verdict,
/// but FAILS here, because its control cannot flip. The 1:1 completeness
/// pin makes the discrimination total: a new manifest family without a
/// control fails the inventory assertion.
#[test]
fn every_family_has_a_decode_divergent_control_that_flips_the_verdict() {
    let families = manifest().families();
    assert_eq!(
        FAMILY_NEGATIVE_CONTROLS.map(|(name, _)| name).to_vec(),
        families
            .iter()
            .map(|family| family.name)
            .collect::<Vec<_>>(),
        "FAMILY_NEGATIVE_CONTROLS must cover the manifest families 1:1 in manifest \
         order; a new family lands WITH its decode-divergent control"
    );
    for (family, (_, control)) in families.iter().zip(FAMILY_NEGATIVE_CONTROLS.iter()) {
        let context = format!("{} / control {}", family.name, control.kind.id());
        // The control rides the family's OWN language slot: the flip below is
        // attributable to the DECODED VALUE alone, never to a different
        // fixture shape.
        assert!(
            family
                .renderings
                .iter()
                .any(|rendering| rendering.kind == control.kind),
            "{context}: the control kind must be one of the family's own kinds"
        );
        // …and is NOT a member spelling (a member would compare equal and
        // vacuously satisfy nothing).
        assert!(
            family
                .renderings
                .iter()
                .all(|rendering| rendering.rendered != control.rendered),
            "{context}: the control must not be a family member spelling"
        );

        let anchor_source = render_variant(family, &family.renderings[0]);
        let anchor_facts = span_free_matcher_facts(&lower_variant(family.name, &anchor_source));

        let control_source = render_variant(family, control);
        let control_trace = lower_variant(&context, &control_source);
        let control_facts = span_free_matcher_facts(&control_trace);

        // (a) The family-equality comparison is DISCRIMINATING for THIS
        // family's language: a decode-divergent value produces different
        // span-free facts.
        assert_ne!(
            control_facts, anchor_facts,
            "{context}: a decode-divergent control must produce DIFFERENT span-free \
             matcher facts — identical facts mean this family's language is never \
             actually decoded"
        );
        // (b) The grounded verdict FLIPS to a proven absence: certainty is a
        // COMPUTED `No` (a blanket fail-open `Maybe` fails here), the
        // selector prunes, and nothing is scoped.
        assert_eq!(
            control_trace.style_matches.len(),
            1,
            "{context}: exactly one style matcher run"
        );
        let style = &control_trace.style_matches[0];
        assert_eq!(
            style
                .selector_certainties
                .iter()
                .map(|fact| fact.certainty)
                .collect::<Vec<_>>(),
            vec![MatchCertainty::No],
            "{context}: the control's certainty is a computed `No` — a value that \
             provably cannot match never observes a blanket `Maybe`"
        );
        assert_eq!(
            style.used_selector_spans.len(),
            0,
            "{context}: the unmatched selector prunes"
        );
        assert!(
            style.scoped_elements.is_empty(),
            "{context}: the control scopes nothing"
        );
    }
}
