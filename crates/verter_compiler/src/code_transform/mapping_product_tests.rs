//! Contract tests for the dual-surface mapping product.
//!
//! Each case pins one boundary the product exists to hold and that nothing else
//! in this crate discriminates: totality over BOTH surfaces, the
//! class/preimage agreement that makes the two fail-closed classes readable
//! either way, the completeness of the carrier-to-projected direction, and
//! deterministic ordering.

use oxc_allocator::Allocator;

use super::mapping_product::{CarrierClass, MappingProduct, ProjectedClass};
use super::{CodeTransform, SegmentAnchor};
use crate::template::code_gen::types::SegmentedOverwriteAuthority;

/// Every chunk kind in one transform, so the partitions are exercised against a
/// mixed emission record rather than a single-edit toy.
fn kitchen_sink<'a>(allocator: &'a Allocator) -> CodeTransform<'a> {
    //                     0         1         2         3
    //                     0123456789012345678901234567890123
    let source = "const a = 1; const b = 2; tail;";
    let mut ct = CodeTransform::new(source, allocator);
    ct.prepend("/* preamble */\n");
    ct.append("\n/* epilogue */");
    // Rewritten: an ordinary overwrite keeps one carrier region responsible.
    ct.overwrite(10, 11, "42");
    // Elided source + Synthesized output: wholly synthetic replacement text.
    ct.overwrite_unmapped(13, 25, "/*gone*/");
    // Synthesized: a plain insertion.
    ct.prepend_left(26, "// note\n");
    ct
}

fn assert_partitions_are_total(product: &MappingProduct, output_len: u32, source_len: u32) {
    let mut cursor = 0u32;
    for region in product.projected() {
        assert_eq!(
            region.generated.start, cursor,
            "projected partition has a gap or overlap at {cursor}"
        );
        assert!(
            region.generated.end > region.generated.start,
            "projected partition published an empty region at {cursor}"
        );
        cursor = region.generated.end;
    }
    assert_eq!(
        cursor, output_len,
        "the projected partition must account for every built output byte"
    );
    assert_eq!(product.projected_len(), output_len);

    let mut cursor = 0u32;
    for region in product.carrier() {
        assert_eq!(
            region.source.start, cursor,
            "carrier partition has a gap or overlap at {cursor}"
        );
        assert!(
            region.source.end > region.source.start,
            "carrier partition published an empty region at {cursor}"
        );
        cursor = region.source.end;
    }
    assert_eq!(
        cursor, source_len,
        "the carrier partition must account for every authored byte, including \
         the ones that reach no output at all"
    );
    assert_eq!(product.carrier_len(), source_len);
}

#[test]
fn both_surfaces_are_partitioned_end_to_end() {
    let allocator = Allocator::default();
    let ct = kitchen_sink(&allocator);
    let output = ct.build_string();
    let product = MappingProduct::of(&ct);
    assert_partitions_are_total(&product, output.len() as u32, ct.original().len() as u32);
}

#[test]
fn an_untouched_source_is_one_identity_region_on_each_side() {
    let allocator = Allocator::default();
    let ct = CodeTransform::new("const a = 1;", &allocator);
    let product = MappingProduct::of(&ct);
    // Compactness: an unedited transform publishes ONE region per surface, not
    // one per internal chunk.
    assert_eq!(product.projected().len(), 1);
    assert_eq!(product.projected()[0].class, ProjectedClass::Identity);
    assert_eq!(product.carrier().len(), 1);
    assert_eq!(product.carrier()[0].class, CarrierClass::Identity);
    assert_eq!(product.carrier()[0].projected, vec![0]);
}

#[test]
fn a_missing_carrier_preimage_is_exactly_the_synthesized_class() {
    let allocator = Allocator::default();
    let ct = kitchen_sink(&allocator);
    let product = MappingProduct::of(&ct);
    let mut synthesized = 0usize;
    for region in product.projected() {
        assert_eq!(
            region.carrier.is_none(),
            region.class == ProjectedClass::Synthesized,
            "class and preimage must state the SAME fact: a consumer branching \
             on either must read the same fail-closed disposition"
        );
        if region.class == ProjectedClass::Synthesized {
            synthesized += 1;
        }
    }
    assert!(
        synthesized >= 3,
        "the fixture emits an intro, an outro, a plain insertion and an \
         unmapped overwrite; found {synthesized} synthesized regions"
    );
}

#[test]
fn wholly_synthetic_replacement_text_elides_the_source_it_stands_in_for() {
    let allocator = Allocator::default();
    let source = "keep <script setup>gone</script> keep";
    let mut ct = CodeTransform::new(source, &allocator);
    let start = source.find("<script").expect("fixture anchor") as u32;
    let end = source.find("</script>").expect("fixture anchor") as u32 + 9;
    ct.overwrite_unmapped(start, end, "const __sfc__ = {};");
    // A source-anchored insertion whose carrier POINT falls at the start of the
    // elided span. The neighbour it would otherwise be answered with is exactly
    // the mis-mapping the elided class refuses, so the region must still report
    // no correlate.
    ct.batch_prepend_left_with_source_map(&[(0, Some((start, 0)), "/* anchored */")]);
    let product = MappingProduct::of(&ct);

    let elided = product
        .carrier_at(start)
        .expect("the replaced span is inside the carrier partition");
    assert_eq!(elided.class, CarrierClass::Elided);
    assert_eq!(elided.source.start, start);
    assert_eq!(elided.source.end, end);
    assert!(
        elided.projected.is_empty(),
        "an elided carrier byte has NO projection: answering with a neighbour \
         is the mis-mapping this class refuses"
    );
    assert!(
        product.projections_at_carrier(start).is_empty(),
        "a position inside an elided region must report no correlate"
    );

    // The replacement text itself is projected, and it is synthesized.
    let generated_start = product
        .projected()
        .iter()
        .find(|region| region.class == ProjectedClass::Synthesized)
        .expect("the replacement text is a projected region");
    assert!(generated_start.carrier.is_none());
}

#[test]
fn a_carrier_region_answers_with_every_projection_derived_from_it() {
    let allocator = Allocator::default();
    let source = "count";
    let mut ct = CodeTransform::new(source, &allocator);
    // The same authored expression emitted a second time at a source-anchored
    // position — the read/assignment-target shape a `v-model` lowering emits.
    ct.batch_prepend_left_with_source_map(&[(0, Some((0, 2)), "__(count)")]);
    let product = MappingProduct::of(&ct);

    let region = product.carrier_at(0).expect("the source has a region at 0");
    assert_eq!(
        region.projected.len(),
        2,
        "one carrier region emitted twice must answer with BOTH projections, \
         never the first one found"
    );
    let mut previous = None;
    for index in &region.projected {
        if let Some(previous) = previous {
            assert!(previous < *index, "projections must be in ascending order");
        }
        previous = Some(*index);
    }
    let projections = product.projections_at_carrier(0);
    assert_eq!(projections.len(), 2);
    assert!(
        projections
            .iter()
            .any(|region| region.class == ProjectedClass::Rewritten),
        "the source-anchored insertion is a rewritten projection of the same \
         carrier position"
    );
    assert!(
        projections
            .iter()
            .any(|region| region.class == ProjectedClass::Identity),
        "the verbatim emission is still an identity projection"
    );
}

#[test]
fn a_declared_zero_width_insertion_anchor_is_part_of_the_product() {
    let allocator = Allocator::default();
    let source = "<script>const value = 1;</script>";
    let mut ct = CodeTransform::new(source, &allocator);
    let preamble = ct.alloc_str("import { ref } from \"vue\";\n");
    ct.batch_prepend_left_static(&[(8, preamble)]);
    ct.set_helper_preamble_content_at(preamble, 8);

    let product = MappingProduct::of(&ct);
    assert_eq!(
        product.insertion_anchors(),
        &[super::mapping_product::InsertionAnchor {
            projected: 8,
            carrier: 8,
        }]
    );
    assert_eq!(
        product.projected_at(8).expect("preamble region").class,
        ProjectedClass::Synthesized,
        "the zero-width edit anchor must not fabricate a preimage for synthesized bytes"
    );
}

#[test]
fn moving_a_slice_backwards_relocates_it_instead_of_claiming_identity() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ABCDEF", &allocator);
    ct.move_slice(2, 4, 0);
    assert_eq!(ct.build_string(), "CDABEF");
    let product = MappingProduct::of(&ct);
    assert_partitions_are_total(&product, 6, 6);

    let moved = product
        .projected_at(0)
        .expect("the moved bytes open the output");
    assert_eq!(
        moved.carrier.map(|span| (span.start, span.end)),
        Some((2, 4))
    );
    let displaced = product
        .projected_at(2)
        .expect("the bytes the move jumped over follow it");
    assert_eq!(
        displaced.class,
        ProjectedClass::Relocated,
        "source bytes emitted after later source bytes are not unmoved \
         relative to their block"
    );
    assert_eq!(
        product.carrier_at(0).expect("carrier region").class,
        CarrierClass::Relocated
    );
}

#[test]
fn segmented_scaffolding_is_synthesized_around_relocated_authored_lexemes() {
    let allocator = Allocator::default();
    let source = "{{ name }}";
    let mut ct = CodeTransform::new(source, &allocator);
    let content = "_toDisplayString(name)";
    let anchors = [SegmentAnchor::new(17, 4, 3)];
    ct.try_overwrite_segmented(
        0,
        source.len() as u32,
        content,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("the fixture range is one live original chunk");
    let product = MappingProduct::of(&ct);
    assert_partitions_are_total(&product, content.len() as u32, source.len() as u32);

    let lexeme = product
        .projected_at(17)
        .expect("the authored lexeme is projected");
    assert_eq!(lexeme.class, ProjectedClass::Relocated);
    assert_eq!(
        lexeme.carrier.map(|span| (span.start, span.end)),
        Some((3, 7))
    );
    assert_eq!(
        product
            .projected_at(0)
            .expect("the helper call opens the output")
            .class,
        ProjectedClass::Synthesized,
        "generated scaffolding around an authored lexeme has no preimage"
    );
    // The delimiters the lexeme was lifted out of reach no output.
    assert_eq!(
        product.carrier_at(0).expect("carrier region").class,
        CarrierClass::Elided
    );
    assert_eq!(
        product.carrier_at(3).expect("carrier region").class,
        CarrierClass::Relocated
    );
}

/// The pre-change comparison reference for this path.
///
/// Before the product existed, the only geometry `CodeTransform` published was
/// [`CodeTransform::build_string_with_source_ranges`]: the source-bearing
/// ranges, with pure insertions absent. That is the current-path reference the
/// product must not lose provenance against.
///
/// Workload: the transforms below, one per emission shape the crate actually
/// produces (untouched source, replacement, relocation, source-anchored
/// insertion, segmented scaffolding). Metric: for EVERY range the reference
/// publishes, the product must publish a projected region of a carrier-bearing
/// class whose generated span CONTAINS the reference range and whose carrier
/// preimage CONTAINS the reference preimage. Comparison rule: containment, not
/// equality — the product coalesces regions the reference splits at source-map
/// token boundaries, and losing provenance is what the comparison detects, not
/// publishing it more compactly. No absolute machine threshold and no Git
/// identity enters the comparison.
#[test]
fn the_product_preserves_the_pre_change_source_range_geometry() {
    let allocator = Allocator::default();

    let mut replaced = CodeTransform::new("const a = 1; const b = 2;", &allocator);
    replaced.overwrite(10, 11, "42");
    replaced.prepend_left(13, "// note\n");

    let mut relocated = CodeTransform::new("ABCDEF", &allocator);
    relocated.move_slice(2, 4, 0);

    let mut anchored = CodeTransform::new("count", &allocator);
    anchored.batch_prepend_left_with_source_map(&[(0, Some((0, 2)), "__(count)")]);

    let mut segmented = CodeTransform::new("{{ name }}", &allocator);
    segmented
        .try_overwrite_segmented(
            0,
            10,
            "_toDisplayString(name)",
            &[SegmentAnchor::new(17, 4, 3)],
            SegmentedOverwriteAuthority::new_for_test(),
        )
        .expect("the fixture range is one live original chunk");

    let untouched = CodeTransform::new("const a = 1;", &allocator);

    for ct in [&replaced, &relocated, &anchored, &segmented, &untouched] {
        let (_, reference) = ct.build_string_with_source_ranges();
        assert!(
            !reference.is_empty(),
            "each fixture must publish reference geometry to compare against"
        );
        let product = MappingProduct::of(ct);
        for range in &reference {
            let region = product
                .projected_at(range.generated_start)
                .unwrap_or_else(|| {
                    panic!(
                        "the product lost a reference range at generated {}",
                        range.generated_start
                    )
                });
            let carrier = region.carrier.unwrap_or_else(|| {
                panic!(
                    "the product dropped the provenance of the reference range at \
                     generated {}",
                    range.generated_start
                )
            });
            assert!(
                region.generated.start <= range.generated_start
                    && region.generated.end >= range.generated_end,
                "the product region must cover the reference range at generated {}",
                range.generated_start
            );
            assert!(
                carrier.start <= range.source_start && carrier.end >= range.source_end,
                "the product preimage must cover the reference preimage for the \
                 range at generated {}",
                range.generated_start
            );
        }
    }
}

/// The one recorded divergence from the reference, and its direction.
///
/// The reference geometry attributes an authored preimage to wholly synthetic
/// replacement text, while `generate_map` — the transform's own source-map
/// authority — deliberately emits no token for it. The product sides with the
/// map: those bytes are Synthesized and the span they stand in for is Elided.
/// Recording the divergence is the comparison rule; leaving it unstated would
/// make the reference and the product disagree silently.
#[test]
fn the_product_sides_with_the_source_map_where_the_reference_over_claims() {
    let allocator = Allocator::default();
    let source = "keep <script setup>gone</script> keep";
    let mut ct = CodeTransform::new(source, &allocator);
    let start = source.find("<script").expect("fixture anchor") as u32;
    let end = source.find("</script>").expect("fixture anchor") as u32 + 9;
    ct.overwrite_unmapped(start, end, "const __sfc__ = {};");

    let (_, reference) = ct.build_string_with_source_ranges();
    let over_claimed = reference
        .iter()
        .find(|range| range.source_start == start && range.source_end == end)
        .expect("the reference geometry attributes the synthetic text to the span");

    let product = MappingProduct::of(&ct);
    let region = product
        .projected_at(over_claimed.generated_start)
        .expect("the synthetic bytes are projected");
    assert_eq!(region.class, ProjectedClass::Synthesized);
    assert!(region.carrier.is_none());
}

#[test]
fn the_product_is_a_deterministic_function_of_the_emission_record() {
    let allocator = Allocator::default();
    let first = MappingProduct::of(&kitchen_sink(&allocator));
    let second = MappingProduct::of(&kitchen_sink(&allocator));
    assert_eq!(
        first, second,
        "two transforms with the same edits must publish the same geometry, \
         byte for byte and index for index"
    );
}
