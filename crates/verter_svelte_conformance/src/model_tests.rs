use super::*;
use crate::covering_array::{self, Partition};

/// A fully-specified baseline row every test derives from: the corpus-
/// canonical supported cell (`.b` class selector over a quoted literal
/// `class="a b"` on a plain static element, external css, plain rule, Match).
fn baseline() -> RowLevels {
    RowLevels {
        selector_kind: SelectorKind::Class,
        template_value: TemplateValueRepresentation::Literal,
        selector_value: SelectorValueRepresentation::Literal,
        target: Target::Class,
        quoting: Quoting::Quoted,
        region: ElementRegion::StaticElement,
        css_source: CssSource::External,
        structural: StructuralKind::Plain,
        outcome: MatchOutcome::Match,
    }
}

fn enumerate_all_rows(mut visit: impl FnMut(RowLevels)) {
    let cards = factor_cardinalities();
    let mut levels = [0u16; FACTOR_COUNT];
    loop {
        visit(RowLevels::decode(Row(levels)).expect("in-range row decodes"));
        let mut factor = FACTOR_COUNT;
        loop {
            if factor == 0 {
                return;
            }
            factor -= 1;
            levels[factor] += 1;
            if levels[factor] < cards[factor] {
                break;
            }
            levels[factor] = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Enum exhaustiveness
// ---------------------------------------------------------------------------

/// Every level inventory carries the exact declared count, dense unique
/// ordinals, a lossless ordinal round-trip, and unique non-empty ids.
#[test]
fn level_inventories_are_dense_unique_and_complete() {
    fn check<T: Copy + Eq + std::fmt::Debug>(
        all: &'static [T],
        expected_len: usize,
        ordinal: impl Fn(T) -> u16,
        from_ordinal: impl Fn(u16) -> Option<T>,
        id: impl Fn(T) -> &'static str,
    ) {
        assert_eq!(all.len(), expected_len, "variant count for {all:?}");
        let mut ids = std::collections::BTreeSet::new();
        for (index, &variant) in all.iter().enumerate() {
            assert_eq!(
                usize::from(ordinal(variant)),
                index,
                "dense ordinal for {variant:?}"
            );
            assert_eq!(
                from_ordinal(index as u16),
                Some(variant),
                "ordinal round-trip for {variant:?}"
            );
            let fragment = id(variant);
            assert!(!fragment.is_empty(), "empty id for {variant:?}");
            assert!(
                fragment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "id fragment {fragment:?} must be lowercase-ascii/digit/hyphen"
            );
            assert!(ids.insert(fragment), "duplicate id {fragment:?}");
        }
        // No gap and no overhang past the dense range.
        assert_eq!(from_ordinal(all.len() as u16), None, "overhang ordinal");
    }

    check(
        SelectorKind::ALL,
        6,
        SelectorKind::ordinal,
        SelectorKind::from_ordinal,
        SelectorKind::id,
    );
    check(
        TemplateValueRepresentation::ALL,
        7,
        TemplateValueRepresentation::ordinal,
        TemplateValueRepresentation::from_ordinal,
        TemplateValueRepresentation::id,
    );
    check(
        SelectorValueRepresentation::ALL,
        4,
        SelectorValueRepresentation::ordinal,
        SelectorValueRepresentation::from_ordinal,
        SelectorValueRepresentation::id,
    );
    check(
        Target::ALL,
        3,
        Target::ordinal,
        Target::from_ordinal,
        Target::id,
    );
    check(
        Quoting::ALL,
        3,
        Quoting::ordinal,
        Quoting::from_ordinal,
        Quoting::id,
    );
    check(
        ElementRegion::ALL,
        6,
        ElementRegion::ordinal,
        ElementRegion::from_ordinal,
        ElementRegion::id,
    );
    check(
        CssSource::ALL,
        2,
        CssSource::ordinal,
        CssSource::from_ordinal,
        CssSource::id,
    );
    check(
        StructuralKind::ALL,
        5,
        StructuralKind::ordinal,
        StructuralKind::from_ordinal,
        StructuralKind::id,
    );
    check(
        MatchOutcome::ALL,
        3,
        MatchOutcome::ordinal,
        MatchOutcome::from_ordinal,
        MatchOutcome::id,
    );
    check(
        CompileTarget::ALL,
        2,
        CompileTarget::ordinal,
        CompileTarget::from_ordinal,
        CompileTarget::id,
    );
    // `RefusalKind` is UNINHABITED (no refusal cells remain) — its contract
    // degenerates to the empty ALL + the always-`None` inverse.
    assert!(RefusalKind::ALL.is_empty());
    assert_eq!(RefusalKind::from_ordinal(0), None);
    check(
        DiagnosticKind::ALL,
        1,
        DiagnosticKind::ordinal,
        DiagnosticKind::from_ordinal,
        DiagnosticKind::id,
    );
    check(
        ConstraintKind::ALL,
        11,
        ConstraintKind::ordinal,
        ConstraintKind::from_ordinal,
        ConstraintKind::id,
    );
    check(
        RenderingKind::ALL,
        8,
        RenderingKind::ordinal,
        RenderingKind::from_ordinal,
        RenderingKind::id,
    );
}

/// Row encode/decode round-trips across the whole space; an out-of-range
/// level fails to decode.
#[test]
fn row_levels_round_trip_and_reject_out_of_range() {
    let mut count = 0u64;
    enumerate_all_rows(|levels| {
        assert_eq!(RowLevels::decode(levels.encode()), Some(levels));
        count += 1;
    });
    let expected: u64 = factor_cardinalities()
        .iter()
        .map(|&c| u64::from(c))
        .product();
    assert_eq!(count, expected, "enumeration covers the full product");

    let mut bad = baseline().encode();
    bad.0[FACTOR_SELECTOR_KIND] = SelectorKind::ALL.len() as u16;
    assert_eq!(
        RowLevels::decode(bad),
        None,
        "out-of-range level decodes to None"
    );
}

// ---------------------------------------------------------------------------
// Role separation
// ---------------------------------------------------------------------------

/// HTML entity spellings appear ONLY in template attribute values; CSS escape
/// spellings appear ONLY in style selectors — the two languages never leak
/// across the boundary in a rendered fixture.
#[test]
fn representation_languages_never_cross_the_template_style_boundary() {
    // A decimal-entity template row: the entity lives in the markup, never in
    // the style block.
    let dec = RowLevels {
        template_value: TemplateValueRepresentation::HtmlDecimalEntity,
        structural: StructuralKind::Pruning,
        ..baseline()
    };
    assert_eq!(classify(&dec), Disposition::Supported);
    let source = render_fixture(&dec);
    let (markup, style) = split_fixture(&source);
    assert!(
        markup.contains("&#32;"),
        "decimal entity spelled in the template: {source}"
    );
    assert!(
        !style.contains("&#"),
        "HTML entities must never appear in the style block: {source}"
    );

    // A css-hex-escape selector row: the escape lives in the style block,
    // never in the markup.
    let escaped = RowLevels {
        selector_value: SelectorValueRepresentation::CssEscapeHex,
        ..baseline()
    };
    assert_eq!(classify(&escaped), Disposition::Supported);
    let source = render_fixture(&escaped);
    let (markup, style) = split_fixture(&source);
    assert!(
        style.contains("\\26 "),
        "hex escape spelled in the selector: {source}"
    );
    assert!(
        !markup.contains('\\'),
        "CSS escapes must never appear in the markup: {source}"
    );
    // The template spells the SAME semantic value in ITS language (a literal
    // ampersand), not in the CSS language.
    assert!(
        markup.contains("a&b"),
        "template spells the ampersand literally: {source}"
    );
}

/// `Dynamic` / `Spread` are uncertainty forms; every static representation
/// says otherwise.
#[test]
fn uncertainty_forms_are_exactly_dynamic_and_spread() {
    for &representation in TemplateValueRepresentation::ALL {
        let expected = matches!(
            representation,
            TemplateValueRepresentation::Dynamic | TemplateValueRepresentation::Spread
        );
        assert_eq!(representation.is_uncertainty_form(), expected);
    }
    // No rendering kind maps onto an uncertainty form (families are static
    // by construction).
    for &kind in RenderingKind::ALL {
        if let Some(representation) = kind.template_representation() {
            assert!(
                !representation.is_uncertainty_form(),
                "{kind:?} must not map onto an uncertainty form"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Constraint faithfulness
// ---------------------------------------------------------------------------

/// Hand-picked rows classify to their expected dispositions (one per rule,
/// plus Supported controls that differ by exactly one level).
#[test]
fn constraint_functions_classify_hand_picked_rows() {
    // Supported baseline.
    assert_eq!(classify(&baseline()), Disposition::Supported);

    // The corpus-canonical entity cell (entity_class_scoped shape).
    let entity_cell = RowLevels {
        template_value: TemplateValueRepresentation::HtmlDecimalEntity,
        structural: StructuralKind::Pruning,
        ..baseline()
    };
    assert_eq!(classify(&entity_cell), Disposition::Supported);

    // Boolean quoting on a Class target is unconstructible.
    let boolean_class = RowLevels {
        quoting: Quoting::Boolean,
        ..baseline()
    };
    assert_eq!(
        classify(&boolean_class),
        Disposition::Invalid(ConstraintKind::BooleanQuotingOnValuedTarget)
    );
    // ... while Boolean on the Attr target (presence selector) is Supported.
    let boolean_attr = RowLevels {
        quoting: Quoting::Boolean,
        target: Target::Attr,
        selector_kind: SelectorKind::Attribute,
        ..baseline()
    };
    assert_eq!(classify(&boolean_attr), Disposition::Supported);

    // A valueless attribute cannot carry a non-literal value representation.
    let boolean_dynamic = RowLevels {
        quoting: Quoting::Boolean,
        target: Target::Attr,
        selector_kind: SelectorKind::Attribute,
        template_value: TemplateValueRepresentation::Dynamic,
        ..baseline()
    };
    assert_eq!(
        classify(&boolean_dynamic),
        Disposition::Invalid(ConstraintKind::BooleanQuotingCarriesNoValue)
    );

    // A spread attribute has no authored quoting.
    let spread_unquoted = RowLevels {
        template_value: TemplateValueRepresentation::Spread,
        quoting: Quoting::Unquoted,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(
        classify(&spread_unquoted),
        Disposition::Invalid(ConstraintKind::SpreadCarriesNoQuoting)
    );

    // CSS escapes on a valueless selector (universal / type / nesting /
    // presence-attribute).
    let escaped_universal = RowLevels {
        selector_kind: SelectorKind::Universal,
        selector_value: SelectorValueRepresentation::CssEscapeHex,
        ..baseline()
    };
    assert_eq!(
        classify(&escaped_universal),
        Disposition::Invalid(ConstraintKind::SelectorEscapeOnValuelessSelector)
    );
    let escaped_presence = RowLevels {
        selector_kind: SelectorKind::Attribute,
        target: Target::Attr,
        quoting: Quoting::Boolean,
        selector_value: SelectorValueRepresentation::CssEscapeChar,
        ..baseline()
    };
    assert_eq!(
        classify(&escaped_presence),
        Disposition::Invalid(ConstraintKind::SelectorEscapeOnValuelessSelector)
    );
    // ... and a Match through an ESCAPED attribute-selector value is
    // unconstructible (the official matcher compares the value text raw), but
    // the SAME escaped spelling with a NoMatch or Maybe declaration is a real
    // supported cell.
    let escaped_attr_match = RowLevels {
        selector_kind: SelectorKind::Attribute,
        target: Target::Attr,
        selector_value: SelectorValueRepresentation::CssEscapeChar,
        ..baseline()
    };
    assert_eq!(
        classify(&escaped_attr_match),
        Disposition::Invalid(ConstraintKind::AttrSelectorValueEscapeNeverMatches)
    );
    let escaped_attr_nomatch = RowLevels {
        outcome: MatchOutcome::NoMatch,
        ..escaped_attr_match
    };
    assert_eq!(classify(&escaped_attr_nomatch), Disposition::Supported);
    let escaped_attr_maybe = RowLevels {
        template_value: TemplateValueRepresentation::Dynamic,
        outcome: MatchOutcome::Maybe,
        ..escaped_attr_match
    };
    assert_eq!(classify(&escaped_attr_maybe), Disposition::Supported);

    // A parentless nesting selector is an official oracle reject; the SAME
    // selector inside a parent rule (Nested) or as `:global(&)` is not.
    for structural in [
        StructuralKind::Plain,
        StructuralKind::Pruning,
        StructuralKind::Combinator,
    ] {
        let parentless = RowLevels {
            selector_kind: SelectorKind::Nesting,
            structural,
            ..baseline()
        };
        assert_eq!(
            classify(&parentless),
            Disposition::OracleRejected(DiagnosticKind::CssNestingSelectorInvalidPlacement),
            "parentless nesting under {structural:?}"
        );
    }
    let nested_nesting = RowLevels {
        selector_kind: SelectorKind::Nesting,
        structural: StructuralKind::Nested,
        ..baseline()
    };
    assert_eq!(classify(&nested_nesting), Disposition::Supported);
    let global_nesting = RowLevels {
        selector_kind: SelectorKind::Nesting,
        structural: StructuralKind::Global,
        ..baseline()
    };
    assert_eq!(classify(&global_nesting), Disposition::Supported);

    // `*` always matches.
    let universal_nomatch = RowLevels {
        selector_kind: SelectorKind::Universal,
        outcome: MatchOutcome::NoMatch,
        ..baseline()
    };
    assert_eq!(
        classify(&universal_nomatch),
        Disposition::Invalid(ConstraintKind::UniversalSelectorAlwaysMatches)
    );

    // `:global(…)` never prunes.
    let global_nomatch = RowLevels {
        structural: StructuralKind::Global,
        outcome: MatchOutcome::NoMatch,
        ..baseline()
    };
    assert_eq!(
        classify(&global_nomatch),
        Disposition::Invalid(ConstraintKind::GlobalSelectorNeverPrunes)
    );

    // A class selector cannot certainly match an id-only subject — but the
    // NoMatch declaration for the same pair is a real supported cell.
    let cross_match = RowLevels {
        target: Target::Id,
        ..baseline()
    };
    assert_eq!(
        classify(&cross_match),
        Disposition::Invalid(ConstraintKind::SelectorCannotReadTarget)
    );
    let cross_nomatch = RowLevels {
        target: Target::Id,
        outcome: MatchOutcome::NoMatch,
        ..baseline()
    };
    assert_eq!(classify(&cross_nomatch), Disposition::Supported);

    // Maybe needs an uncertainty source.
    let static_maybe = RowLevels {
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(
        classify(&static_maybe),
        Disposition::Invalid(ConstraintKind::MaybeNeedsUncertainSource)
    );
    let dynamic_maybe = RowLevels {
        template_value: TemplateValueRepresentation::Dynamic,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(classify(&dynamic_maybe), Disposition::Supported);
    // A dynamic value on a NON-read target does not make the selector
    // uncertain (no class attribute exists at all).
    let dynamic_cross_maybe = RowLevels {
        template_value: TemplateValueRepresentation::Dynamic,
        target: Target::Id,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(
        classify(&dynamic_cross_maybe),
        Disposition::Invalid(ConstraintKind::MaybeNeedsUncertainSource)
    );

    // Spread makes every attribute-reading verdict uncertain.
    let spread_match = RowLevels {
        template_value: TemplateValueRepresentation::Spread,
        ..baseline()
    };
    assert_eq!(
        classify(&spread_match),
        Disposition::Invalid(ConstraintKind::SpreadOutcomeAlwaysUncertain)
    );
    let spread_maybe = RowLevels {
        template_value: TemplateValueRepresentation::Spread,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(classify(&spread_maybe), Disposition::Supported);
    // ... while a TYPE selector is untouched by spread: certain verdicts stay
    // constructible.
    let spread_type_match = RowLevels {
        template_value: TemplateValueRepresentation::Spread,
        selector_kind: SelectorKind::Type,
        ..baseline()
    };
    assert_eq!(classify(&spread_type_match), Disposition::Supported);

    // A type selector against a dynamic element tag is always uncertain.
    let dyn_tag_match = RowLevels {
        selector_kind: SelectorKind::Type,
        region: ElementRegion::SvelteElement,
        ..baseline()
    };
    assert_eq!(
        classify(&dyn_tag_match),
        Disposition::Invalid(ConstraintKind::SvelteElementTagUncertain)
    );
    let dyn_tag_maybe = RowLevels {
        selector_kind: SelectorKind::Type,
        region: ElementRegion::SvelteElement,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(classify(&dyn_tag_maybe), Disposition::Supported);

    // The `<slot>` fallback region is a SUPPORTED conformance cell — it
    // classifies identically to its static-element twin.
    let slot_region = RowLevels {
        region: ElementRegion::LegacySlot,
        ..baseline()
    };
    assert_eq!(classify(&slot_region), Disposition::Supported);
    // An officially-incoherent slot row stays Invalid (coherence precedes
    // the refusal).
    let slot_incoherent = RowLevels {
        region: ElementRegion::LegacySlot,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    assert_eq!(
        classify(&slot_incoherent),
        Disposition::Invalid(ConstraintKind::MaybeNeedsUncertainSource)
    );
    // A parentless-nesting slot row is the oracle's reject, not a refusal.
    let slot_oracle = RowLevels {
        region: ElementRegion::LegacySlot,
        selector_kind: SelectorKind::Nesting,
        ..baseline()
    };
    assert_eq!(
        classify(&slot_oracle),
        Disposition::OracleRejected(DiagnosticKind::CssNestingSelectorInvalidPlacement)
    );
}

/// Every declared constraint kind actually fires somewhere in the space, and
/// every produced partition id round-trips onto a declared model enum.
#[test]
fn every_constraint_kind_fires_and_partitions_round_trip() {
    let mut seen_constraints = std::collections::BTreeSet::new();
    let mut seen_refusals = std::collections::BTreeSet::new();
    let mut seen_diagnostics = std::collections::BTreeSet::new();
    let mut seen_supported = false;

    enumerate_all_rows(|levels| match classify(&levels) {
        Disposition::Supported => seen_supported = true,
        Disposition::Refused(kind) => {
            seen_refusals.insert(kind);
        }
        Disposition::OracleRejected(kind) => {
            seen_diagnostics.insert(kind);
        }
        Disposition::Invalid(kind) => {
            seen_constraints.insert(kind);
        }
    });

    assert!(seen_supported, "the space carries Supported rows");
    assert_eq!(
        seen_constraints.into_iter().collect::<Vec<_>>(),
        ConstraintKind::ALL.to_vec(),
        "every declared constraint fires at least once"
    );
    assert_eq!(
        seen_refusals.into_iter().collect::<Vec<_>>(),
        RefusalKind::ALL.to_vec(),
        "the refusal partition stays uninhabited"
    );
    assert_eq!(
        seen_diagnostics.into_iter().collect::<Vec<_>>(),
        DiagnosticKind::ALL.to_vec(),
        "every declared diagnostic kind occurs"
    );
}

// ---------------------------------------------------------------------------
// Disposition ↔ Partition bridge
// ---------------------------------------------------------------------------

/// The covering-array partition ids ARE the model ordinals — 1:1 in both
/// directions, for every declared kind.
#[test]
fn disposition_partition_bridge_is_one_to_one() {
    assert_eq!(Disposition::Supported.partition(), Partition::Supported);

    for &kind in RefusalKind::ALL {
        let partition = Disposition::Refused(kind).partition();
        let Partition::Refused(covering_array::RefusalKind(ordinal)) = partition else {
            panic!("refusal maps to a refusal partition, got {partition:?}");
        };
        assert_eq!(RefusalKind::from_ordinal(ordinal), Some(kind));
    }
    for &kind in DiagnosticKind::ALL {
        let partition = Disposition::OracleRejected(kind).partition();
        let Partition::OracleRejected(covering_array::DiagnosticKind(ordinal)) = partition else {
            panic!("diagnostic maps to an oracle partition, got {partition:?}");
        };
        assert_eq!(DiagnosticKind::from_ordinal(ordinal), Some(kind));
    }
    for &kind in ConstraintKind::ALL {
        assert_eq!(Disposition::Invalid(kind).partition(), Partition::Invalid);
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Split a fixture into (markup, style-block) halves.
fn split_fixture(source: &str) -> (&str, &str) {
    let start = source
        .find("<style>")
        .expect("fixture carries a style block");
    (&source[..start], &source[start..])
}

/// The canonical supported rows render to compilable `.svelte` shapes with
/// the load-bearing pieces present and the excluded pieces absent.
#[test]
fn render_fixture_produces_the_declared_source_shapes() {
    // Baseline: quoted literal class, plain rule, external css.
    let source = render_fixture(&baseline());
    assert!(
        source.contains("<div class=\"a b\">x</div>"),
        "markup: {source}"
    );
    assert!(source.contains(".b {"), "subject rule: {source}");
    assert!(source.contains("color: red;"), "declaration: {source}");
    assert!(!source.contains("svelte:options"), "external css: {source}");
    assert!(!source.contains("<script>"), "no script needed: {source}");
    assert!(!source.contains(".zz"), "no NoMatch selector: {source}");

    // The corpus-canonical decimal-entity + pruning cell.
    let entity = RowLevels {
        template_value: TemplateValueRepresentation::HtmlDecimalEntity,
        structural: StructuralKind::Pruning,
        ..baseline()
    };
    let source = render_fixture(&entity);
    assert!(
        source.contains("class=\"a&#32;b\""),
        "entity value: {source}"
    );
    assert!(source.contains(".b {"), "subject rule: {source}");
    assert!(
        source.contains(".unused-prune {"),
        "pruned extra rule: {source}"
    );

    // Injected css mode is an in-source options tag.
    let injected = RowLevels {
        css_source: CssSource::Injected,
        ..baseline()
    };
    let source = render_fixture(&injected);
    assert!(
        source.contains("<svelte:options css=\"injected\" />"),
        "injected options: {source}"
    );

    // Unquoted literal: single token, no quotes on the attribute.
    let unquoted = RowLevels {
        quoting: Quoting::Unquoted,
        ..baseline()
    };
    let source = render_fixture(&unquoted);
    assert!(
        source.contains("<div class=b>x</div>"),
        "unquoted: {source}"
    );

    // Boolean presence attribute + presence selector.
    let boolean = RowLevels {
        quoting: Quoting::Boolean,
        target: Target::Attr,
        selector_kind: SelectorKind::Attribute,
        ..baseline()
    };
    let source = render_fixture(&boolean);
    assert!(
        source.contains("<div data-x>x</div>"),
        "boolean attr: {source}"
    );
    assert!(source.contains("[data-x] {"), "presence selector: {source}");
    assert!(
        !source.contains("data-x="),
        "no value on a boolean attr: {source}"
    );

    // Dynamic (uncertain) value: a prop-driven expression, quoted form.
    let dynamic_maybe = RowLevels {
        template_value: TemplateValueRepresentation::Dynamic,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    let source = render_fixture(&dynamic_maybe);
    assert!(
        source.contains("let { value } = $props();"),
        "prop decl: {source}"
    );
    assert!(
        source.contains("class=\"{value}\""),
        "quoted expression: {source}"
    );

    // Dynamic enumerable Match: a conditional whose branches BOTH carry the
    // subject token, one spelled through the JS-string language.
    let dynamic_match = RowLevels {
        template_value: TemplateValueRepresentation::Dynamic,
        quoting: Quoting::Unquoted,
        ..baseline()
    };
    let source = render_fixture(&dynamic_match);
    assert!(
        source.contains("let { flag } = $props();"),
        "prop decl: {source}"
    );
    assert!(
        source.contains("class={flag ? 'a b' : '\\u0062'}"),
        "unquoted conditional expression: {source}"
    );

    // Spread region shape.
    let spread = RowLevels {
        template_value: TemplateValueRepresentation::Spread,
        outcome: MatchOutcome::Maybe,
        ..baseline()
    };
    let source = render_fixture(&spread);
    assert!(
        source.contains("let { rest } = $props();"),
        "rest prop: {source}"
    );
    assert!(
        source.contains("<div {...rest}>x</div>"),
        "spread attr: {source}"
    );

    // Regions.
    let block = RowLevels {
        region: ElementRegion::Block,
        ..baseline()
    };
    let source = render_fixture(&block);
    assert!(
        source.contains("let open = $state(true);"),
        "block state: {source}"
    );
    assert!(source.contains("{#if open}"), "if block: {source}");
    assert!(source.contains("{/if}"), "if close: {source}");

    let snippet = RowLevels {
        region: ElementRegion::Snippet,
        ..baseline()
    };
    let source = render_fixture(&snippet);
    assert!(
        source.contains("{#snippet subject()}"),
        "snippet decl: {source}"
    );
    assert!(
        source.contains("{@render subject()}"),
        "render tag: {source}"
    );

    let component = RowLevels {
        region: ElementRegion::Component,
        ..baseline()
    };
    let source = render_fixture(&component);
    assert!(
        source.contains("import Child from './Child.svelte';"),
        "component import: {source}"
    );
    assert!(source.contains("<Child>"), "component children: {source}");

    let svelte_element = RowLevels {
        region: ElementRegion::SvelteElement,
        ..baseline()
    };
    let source = render_fixture(&svelte_element);
    assert!(
        source.contains("let tag = $state('div');"),
        "tag state: {source}"
    );
    assert!(
        source.contains("<svelte:element this={tag} class=\"a b\">x</svelte:element>"),
        "dynamic element: {source}"
    );

    let slot = RowLevels {
        region: ElementRegion::LegacySlot,
        ..baseline()
    };
    let source = render_fixture(&slot);
    assert!(source.contains("<slot>"), "slot region: {source}");
    assert!(source.contains("</slot>"), "slot close: {source}");

    // Structural shapes.
    let combinator = RowLevels {
        structural: StructuralKind::Combinator,
        ..baseline()
    };
    let source = render_fixture(&combinator);
    assert!(
        source.contains("<div class=\"wrap\">"),
        "wrapper element: {source}"
    );
    assert!(
        source.contains(".wrap .b {"),
        "descendant selector: {source}"
    );

    let nested = RowLevels {
        structural: StructuralKind::Nested,
        ..baseline()
    };
    let source = render_fixture(&nested);
    assert!(source.contains("&:hover {"), "nested rule: {source}");

    let global = RowLevels {
        structural: StructuralKind::Global,
        ..baseline()
    };
    let source = render_fixture(&global);
    assert!(
        source.contains(":global(.b) {"),
        "global selector: {source}"
    );

    // Nesting selector inside a parent rule: the parent reads the target.
    let nesting = RowLevels {
        selector_kind: SelectorKind::Nesting,
        structural: StructuralKind::Nested,
        ..baseline()
    };
    let source = render_fixture(&nesting);
    assert!(source.contains(".b {"), "parent selector: {source}");
    assert!(source.contains("& {"), "nesting subject: {source}");

    // NoMatch: the selector targets the never-present value.
    let nomatch = RowLevels {
        outcome: MatchOutcome::NoMatch,
        ..baseline()
    };
    let source = render_fixture(&nomatch);
    assert!(source.contains(".zz {"), "absent-value selector: {source}");
    assert!(
        source.contains("class=\"a b\""),
        "subject value unchanged: {source}"
    );
    assert!(
        !source.contains(".b {"),
        "no subject-matching rule: {source}"
    );

    // Type selectors.
    let type_match = RowLevels {
        selector_kind: SelectorKind::Type,
        ..baseline()
    };
    let source = render_fixture(&type_match);
    assert!(source.contains("div {"), "type selector: {source}");
    let type_nomatch = RowLevels {
        selector_kind: SelectorKind::Type,
        outcome: MatchOutcome::NoMatch,
        ..baseline()
    };
    let source = render_fixture(&type_nomatch);
    assert!(source.contains("p {"), "absent type selector: {source}");
    assert!(
        !source.contains("<p"),
        "the absent type never renders: {source}"
    );

    // Every officially-compilable row renders with exactly one style block
    // and balanced markup.
    enumerate_all_rows(|levels| {
        if matches!(classify(&levels), Disposition::Invalid(_)) {
            return;
        }
        let source = render_fixture(&levels);
        assert_eq!(
            source.matches("<style>").count(),
            1,
            "exactly one style block for {levels:?}"
        );
        assert!(
            source.ends_with('\n'),
            "fixture ends with a newline for {levels:?}"
        );
    });
}

/// Escape spellings land in the selector for every escaped representation.
#[test]
fn selector_escape_spellings_render_per_representation() {
    let hex = RowLevels {
        selector_value: SelectorValueRepresentation::CssEscapeHex,
        ..baseline()
    };
    let source = render_fixture(&hex);
    assert!(source.contains(".a\\26 b {"), "hex escape: {source}");
    assert!(
        source.contains("class=\"a a&b\""),
        "literal template amp: {source}"
    );

    let ch = RowLevels {
        selector_value: SelectorValueRepresentation::CssEscapeChar,
        template_value: TemplateValueRepresentation::HtmlNamedEntity,
        ..baseline()
    };
    let source = render_fixture(&ch);
    assert!(source.contains(".a\\&b {"), "identity escape: {source}");
    assert!(
        source.contains("class=\"a a&amp;b\""),
        "named entity template: {source}"
    );

    let mixed = RowLevels {
        selector_value: SelectorValueRepresentation::Mixed,
        template_value: TemplateValueRepresentation::HtmlHexEntity,
        ..baseline()
    };
    let source = render_fixture(&mixed);
    assert!(source.contains(".a\\26 b\\&c {"), "mixed escapes: {source}");
    assert!(
        source.contains("class=\"a a&#x26;b&#x26;c\""),
        "hex entity template: {source}"
    );

    // The named-entity + literal-selector pairing spells a copyright sign:
    // ident-legal on the CSS side, named reference on the HTML side.
    let named_literal = RowLevels {
        template_value: TemplateValueRepresentation::HtmlNamedEntity,
        ..baseline()
    };
    let source = render_fixture(&named_literal);
    assert!(
        source.contains("class=\"a a&copy;b\""),
        "named entity: {source}"
    );
    assert!(
        source.contains(".a\u{a9}b {"),
        "literal copyright ident: {source}"
    );
}

/// Rendering an Invalid row is a caller bug and panics.
#[test]
#[should_panic(expected = "unconstructible")]
fn render_fixture_panics_on_invalid_rows() {
    let invalid = RowLevels {
        quoting: Quoting::Boolean,
        ..baseline()
    };
    let _ = render_fixture(&invalid);
}

// ---------------------------------------------------------------------------
// Slugs
// ---------------------------------------------------------------------------

/// Slugs are stable, unique across the whole space, and cross-platform-safe.
#[test]
fn slugs_are_stable_unique_and_path_safe() {
    assert_eq!(slug(&baseline()), "cls-lit-lit-cls-q-el-ext-plain-m");

    let mut seen = std::collections::BTreeSet::new();
    let mut rows = 0u64;
    enumerate_all_rows(|levels| {
        rows += 1;
        let value = slug(&levels);
        assert!(
            value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "slug {value:?} restricted to lowercase/digit/hyphen"
        );
        assert!(
            !value.is_empty() && value.len() < 64,
            "slug length: {value:?}"
        );
        assert!(
            !value.starts_with('-') && !value.ends_with('-'),
            "trim: {value:?}"
        );
        assert!(seen.insert(value), "duplicate slug for {levels:?}");
    });
    assert_eq!(seen.len() as u64, rows, "one distinct slug per row");
}

// ---------------------------------------------------------------------------
// Compile options
// ---------------------------------------------------------------------------

/// The typed compile options serialize deterministically; only the scoped-CSS
/// tree cell carries a non-default option (`fragments: 'tree'`).
#[test]
fn compile_options_serialize_deterministically() {
    assert_eq!(ManifestCompileOptions::default().to_json(), "{}");
    assert_eq!(
        ManifestCompileOptions {
            custom_element: true,
            ..ManifestCompileOptions::default()
        }
        .to_json(),
        "{\"customElement\":true}"
    );
    assert_eq!(
        ManifestCompileOptions {
            custom_element: true,
            filename_undefined: true,
            ..ManifestCompileOptions::default()
        }
        .to_json(),
        "{\"customElement\":true,\"filename\":null}"
    );
    assert_eq!(
        ManifestCompileOptions {
            fragments_tree: true,
            ..ManifestCompileOptions::default()
        }
        .to_json(),
        "{\"fragments\":\"tree\"}"
    );
    // A canonical CLASS-selector scoped static cell stays html (default).
    assert_eq!(
        compile_options(&baseline()),
        ManifestCompileOptions::default()
    );
    // The scoped-CSS tree cell (a plain, certainly-matching TYPE selector on a
    // static element with an external `<style>` and a class target) flips to
    // `fragments: 'tree'`.
    let tree_cell = RowLevels {
        selector_kind: SelectorKind::Type,
        ..baseline()
    };
    assert_eq!(
        compile_options(&tree_cell),
        ManifestCompileOptions {
            fragments_tree: true,
            ..ManifestCompileOptions::default()
        }
    );
}

// ---------------------------------------------------------------------------
// Semantic value families
// ---------------------------------------------------------------------------

/// Families are grounded: ≥2 renderings of distinct kinds each, a concrete
/// verdict, the mandated space-entity family present, and no uncertainty
/// forms anywhere.
#[test]
fn semantic_value_families_are_grounded_and_static() {
    let families = semantic_value_families();
    assert!(
        families.len() >= 4,
        "at least four families, got {}",
        families.len()
    );

    let mut names = std::collections::BTreeSet::new();
    for family in &families {
        assert!(
            names.insert(family.name),
            "duplicate family {:?}",
            family.name
        );
        assert!(
            !family.base_value.is_empty(),
            "base value for {:?}",
            family.name
        );
        assert!(
            family.renderings.len() >= 2,
            "family {:?} needs ≥2 renderings",
            family.name
        );
        let kinds: std::collections::BTreeSet<_> =
            family.renderings.iter().map(|r| r.kind).collect();
        assert!(
            kinds.len() >= 2,
            "family {:?} needs ≥2 DISTINCT representation kinds",
            family.name
        );
        // Distinct KINDS alone are not enough: two kinds carrying ONE
        // spelling would collapse the metamorphic comparison to
        // self-equality, so the rendered VALUES must be pairwise-distinct
        // too (the fixture-source-byte distinctness is asserted where the
        // renderer lives, in `tests/metamorphic.rs`).
        let rendered_values: std::collections::BTreeSet<_> =
            family.renderings.iter().map(|r| r.rendered).collect();
        assert_eq!(
            rendered_values.len(),
            family.renderings.len(),
            "family {:?} renderings must be pairwise-DISTINCT spellings",
            family.name
        );
        assert!(
            !family.verdict.selector.is_empty(),
            "grounded selector for {:?}",
            family.name
        );
        for rendering in &family.renderings {
            assert!(
                !rendering.rendered.is_empty(),
                "rendering text in {:?}",
                family.name
            );
            // NEGATIVE: no uncertainty form is ever a family member — neither
            // via the kind mapping nor via dynamic/spread source syntax.
            if let Some(representation) = rendering.kind.template_representation() {
                assert!(
                    !representation.is_uncertainty_form(),
                    "uncertainty form in family {:?}",
                    family.name
                );
            }
            assert!(
                !rendering.rendered.contains("{...") && !rendering.rendered.contains("{`"),
                "spread/interpolation syntax leaked into family {:?}",
                family.name
            );
        }
    }

    // The mandated space family: literal / decimal / hex spellings of the
    // token pair `a b`, grounded on `.b` matching.
    let space = families
        .iter()
        .find(|family| family.base_value == "a b")
        .expect("the space-entity family is present");
    let has = |kind: RenderingKind, rendered: &str| {
        space
            .renderings
            .iter()
            .any(|r| r.kind == kind && r.rendered == rendered)
    };
    assert!(has(RenderingKind::TemplateLiteral, "a b"), "literal member");
    assert!(
        has(RenderingKind::HtmlDecimalEntity, "a&#32;b"),
        "decimal member"
    );
    assert!(has(RenderingKind::HtmlHexEntity, "a&#x20;b"), "hex member");
    assert_eq!(space.verdict.selector, ".b");
    assert_eq!(space.verdict.outcome, MatchOutcome::Match);

    // A JS-string family exists and stays in the JS language.
    let js = families
        .iter()
        .find(|family| {
            family
                .renderings
                .iter()
                .any(|r| r.kind == RenderingKind::JsStringEscape)
        })
        .expect("a JS-string-escape family is present");
    assert!(
        js.renderings.iter().all(|r| matches!(
            r.kind,
            RenderingKind::JsStringLiteral | RenderingKind::JsStringEscape
        )),
        "JS family renderings stay in the JS language"
    );

    // A CSS-escape family exists and stays in the CSS language.
    let css = families
        .iter()
        .find(|family| {
            family
                .renderings
                .iter()
                .any(|r| r.kind == RenderingKind::CssEscapeHex)
        })
        .expect("a CSS-escape family is present");
    assert!(
        css.renderings.iter().all(|r| matches!(
            r.kind,
            RenderingKind::CssEscapeHex | RenderingKind::CssEscapeChar
        )),
        "CSS family renderings stay in the CSS language"
    );
}
