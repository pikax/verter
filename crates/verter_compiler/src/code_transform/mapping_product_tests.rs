//! Contract tests for the dual-surface mapping product.
//!
//! Each case pins one boundary the product exists to hold and that nothing else
//! in this crate discriminates: totality over BOTH surfaces, the
//! class/preimage agreement that makes the two fail-closed classes readable
//! either way, the completeness of the carrier-to-projected direction, and
//! deterministic ordering.

use oxc_allocator::Allocator;

use super::mapping_product::{CarrierClass, MappingProduct, ProjectedClass};
use super::{AnchorKind, CodeTransform, SegmentAnchor};

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
    let product = MappingProduct::of(&ct).expect("mapped transform");
    assert_partitions_are_total(&product, output.len() as u32, ct.original().len() as u32);
}

#[test]
fn an_untouched_source_is_one_identity_region_on_each_side() {
    let allocator = Allocator::default();
    let ct = CodeTransform::new("const a = 1;", &allocator);
    let product = MappingProduct::of(&ct).expect("mapped transform");
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
    let product = MappingProduct::of(&ct).expect("mapped transform");
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
    let product = MappingProduct::of(&ct).expect("mapped transform");

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
    let product = MappingProduct::of(&ct).expect("mapped transform");

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

    let product = MappingProduct::of(&ct).expect("mapped transform");
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
    let product = MappingProduct::of(&ct).expect("mapped transform");
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
fn segmented_scaffolding_is_synthesized_around_exact_authored_lexemes() {
    let allocator = Allocator::default();
    let source = "{{ name }}";
    let mut ct = CodeTransform::new(source, &allocator);
    let content = "_toDisplayString(name)";
    let anchors = [SegmentAnchor::new(17, 4, 3, AnchorKind::Exact)];
    let correspondence = ct
        .check_correspondence(content, &anchors)
        .expect("the anchor copies the authored lexeme");
    ct.try_overwrite_segmented(0, source.len() as u32, correspondence)
        .expect("the fixture range is one live original chunk");
    let product = MappingProduct::of(&ct).expect("mapped transform");
    assert_partitions_are_total(&product, content.len() as u32, source.len() as u32);

    let lexeme = product
        .projected_at(17)
        .expect("the authored lexeme is projected");
    // The lexeme is the authored bytes, emitted in source order: an identity
    // correspondence, like any other exact run.
    assert_eq!(lexeme.class, ProjectedClass::Identity);
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
        CarrierClass::Identity
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
    let segmented_anchors = [SegmentAnchor::new(17, 4, 3, AnchorKind::Exact)];
    let correspondence = segmented
        .check_correspondence("_toDisplayString(name)", &segmented_anchors)
        .expect("the anchor copies the authored lexeme");
    segmented
        .try_overwrite_segmented(0, 10, correspondence)
        .expect("the fixture range is one live original chunk");

    let untouched = CodeTransform::new("const a = 1;", &allocator);

    for ct in [&replaced, &relocated, &anchored, &segmented, &untouched] {
        let (_, reference) = ct
            .build_string_with_source_ranges()
            .expect("mapped transform");
        assert!(
            !reference.is_empty(),
            "each fixture must publish reference geometry to compare against"
        );
        let product = MappingProduct::of(ct).expect("mapped transform");
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

/// Wholly synthetic replacement text has no authored preimage in any
/// product: the source ranges leave it out, and the mapping product calls its
/// bytes Synthesized and the span they stand in for Elided.
#[test]
fn every_product_leaves_synthetic_replacement_text_unclaimed() {
    let allocator = Allocator::default();
    let source = "keep <script setup>gone</script> keep";
    let mut ct = CodeTransform::new(source, &allocator);
    let start = source.find("<script").expect("fixture anchor") as u32;
    let end = source.find("</script>").expect("fixture anchor") as u32 + 9;
    ct.overwrite_unmapped(start, end, "const __sfc__ = {};");

    let (_, reference) = ct
        .build_string_with_source_ranges()
        .expect("mapped transform");
    assert!(
        !reference
            .iter()
            .any(|range| range.source_start == start && range.source_end == end),
        "no range attributes the synthetic text to the span it replaced"
    );

    let product = MappingProduct::of(&ct).expect("mapped transform");
    let region = product
        .projected_at(start)
        .expect("the synthetic bytes are projected");
    assert_eq!(region.class, ProjectedClass::Synthesized);
    assert!(region.carrier.is_none());
}

#[test]
fn the_product_is_a_deterministic_function_of_the_emission_record() {
    let allocator = Allocator::default();
    let first = MappingProduct::of(&kitchen_sink(&allocator)).expect("mapped transform");
    let second = MappingProduct::of(&kitchen_sink(&allocator)).expect("mapped transform");
    assert_eq!(
        first, second,
        "two transforms with the same edits must publish the same geometry, \
         byte for byte and index for index"
    );
}

/// Generated columns that carry a source position in the transform's map.
fn mapped_columns(ct: &CodeTransform<'_>) -> Vec<(u32, u32, u32)> {
    let map = ct
        .generate_map(super::SourceMapOptions::new().with_source("a.ts"))
        .expect("mapped transform");
    map.get_tokens()
        .filter(|token| token.get_source_id().is_some())
        .map(|token| {
            (
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
            )
        })
        .collect()
}

#[test]
fn a_relocated_mapped_insertion_keeps_its_source() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    // A mapped insertion at 3 that stands for source offset 0.
    ct.batch_prepend_left_with_source_map(&[(3, Some((0, 0)), "XY")]);
    ct.move_slice(2, 4, 6);
    assert_eq!(ct.build_string(), "abefcXYd");

    assert!(
        mapped_columns(&ct).contains(&(5, 0, 0)),
        "the relocated insertion still names its authored position"
    );
    let product = MappingProduct::of(&ct).expect("mapped transform");
    let region = product.projected_at(5).expect("the insertion is projected");
    assert_eq!(region.class, ProjectedClass::Rewritten);
    assert_eq!(
        region.carrier.map(|span| (span.start, span.end)),
        Some((0, 0))
    );
}

#[test]
fn a_relocated_segmented_lexeme_keeps_its_anchor() {
    let allocator = Allocator::default();
    let source = "{{ name }} tail";
    let mut ct = CodeTransform::new(source, &allocator);
    let anchors = [SegmentAnchor::new(17, 4, 3, AnchorKind::Exact)];
    let correspondence = ct
        .check_correspondence("_toDisplayString(name)", &anchors)
        .expect("the anchor copies the authored lexeme");
    ct.try_overwrite_segmented(0, 10, correspondence)
        .expect("one live original chunk");
    ct.move_slice(0, 10, 15);
    assert_eq!(ct.build_string(), " tail_toDisplayString(name)");

    // `name` sits at generated column 5 + 17.
    assert!(
        mapped_columns(&ct).contains(&(22, 0, 3)),
        "the relocated lexeme keeps its own anchor"
    );
    let product = MappingProduct::of(&ct).expect("mapped transform");
    let lexeme = product.projected_at(22).expect("the lexeme is projected");
    assert_eq!(lexeme.class, ProjectedClass::Relocated);
    assert_eq!(
        lexeme.carrier.map(|span| (span.start, span.end)),
        Some((3, 7))
    );
}

#[test]
fn a_relocated_synthetic_overwrite_stays_unmapped() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.overwrite_unmapped(2, 4, "ZZ");
    ct.move_slice(2, 4, 6);
    assert_eq!(ct.build_string(), "abefZZ");

    assert!(
        !mapped_columns(&ct).iter().any(|(column, ..)| *column == 4),
        "synthetic text gains no authored position by moving"
    );
    let product = MappingProduct::of(&ct).expect("mapped transform");
    assert_eq!(
        product.projected_at(4).expect("projected").class,
        ProjectedClass::Synthesized
    );
}

/// What one generated byte stands for, as one product reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteKind {
    Exact,
    Rewritten,
    Anchored,
    Unmapped,
}

/// Per generated byte: the kind the source ranges report, the kind the
/// mapping product reports, and whether the V3 map maps the byte.
fn products_by_byte(ct: &CodeTransform<'_>) -> Vec<(ByteKind, ByteKind, bool)> {
    let (text, ranges) = ct
        .build_string_with_source_ranges()
        .expect("mapped transform");
    let product = MappingProduct::of(ct).expect("mapped transform");
    let map = ct
        .generate_map(super::SourceMapOptions::new().with_source("a.ts"))
        .expect("mapped transform");
    let tokens: Vec<_> = map.get_tokens().collect();
    let mut line = 0u32;
    let mut column = 0u32;
    let mut out = Vec::with_capacity(text.len());
    for (offset, ch) in text.char_indices() {
        let offset = offset as u32;
        let from_ranges = ranges
            .iter()
            .find(|range| range.generated_start <= offset && offset < range.generated_end)
            .map_or(ByteKind::Unmapped, |range| match range.replacement {
                false => ByteKind::Exact,
                true if range.source_end > range.source_start => ByteKind::Rewritten,
                true => ByteKind::Anchored,
            });
        let region = product.projected_at(offset).expect("total partition");
        let from_product = match region.class {
            ProjectedClass::Identity | ProjectedClass::Relocated => ByteKind::Exact,
            ProjectedClass::Rewritten
                if region.carrier.is_some_and(|span| span.end > span.start) =>
            {
                ByteKind::Rewritten
            }
            ProjectedClass::Rewritten => ByteKind::Anchored,
            ProjectedClass::Synthesized => ByteKind::Unmapped,
        };
        // The token in force at this position: the last at or before it on
        // the same line.
        let mapped = tokens
            .iter()
            .rfind(|token| token.get_dst_line() == line && token.get_dst_col() <= column)
            .is_some_and(|token| token.get_source_id().is_some());
        for _ in 0..ch.len_utf8() {
            out.push((from_ranges, from_product, mapped));
        }
        super::advance_generated_position(ch.encode_utf8(&mut [0; 4]), &mut line, &mut column);
    }
    out
}

#[test]
fn every_product_reads_one_emission_walk() {
    type Edit = fn(&mut CodeTransform<'_>);
    let rows: [(&str, Edit); 9] = [
        ("authored source", |_| {}),
        ("insertion", |ct| {
            ct.prepend_left(3, "ins");
        }),
        ("anchored insertion", |ct| {
            ct.try_append_left(3, "anc").expect("in range");
        }),
        ("replacement", |ct| {
            ct.overwrite(2, 5, "réplacé");
        }),
        ("synthetic replacement", |ct| {
            ct.overwrite_unmapped(2, 5, "synth");
        }),
        ("mapped insertion", |ct| {
            ct.batch_prepend_left_with_source_map(&[(3, Some((1, 2)), "__x")]);
        }),
        ("segmented overwrite", |ct| {
            let anchors = [
                SegmentAnchor::new(2, 2, 6, AnchorKind::Exact),
                SegmentAnchor::new(5, 1, 0, AnchorKind::Point),
            ];
            let correspondence = ct
                .check_correspondence("<(gh)!>", &anchors)
                .expect("the exact anchor copies the source");
            ct.try_overwrite_segmented(6, 8, correspondence)
                .expect("one live chunk");
        }),
        ("relocated run", |ct| {
            ct.overwrite(4, 5, "E");
            ct.move_slice(3, 6, 9);
        }),
        ("extracted source", |ct| {
            ct.overwrite(1, 3, "");
            ct.extract_original(1, 3, 9, "[", "]");
        }),
    ];
    for (name, edit) in rows {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("abcdefghi", &allocator);
        edit(&mut ct);
        for (offset, (ranges, product, mapped)) in products_by_byte(&ct).into_iter().enumerate() {
            assert_eq!(ranges, product, "{name}: byte {offset}");
            assert_eq!(
                mapped,
                ranges != ByteKind::Unmapped,
                "{name}: byte {offset}"
            );
        }
    }
}

#[test]
fn a_synthetic_overwrite_never_reaches_a_range_consumer() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("<script setup>x</script>", &allocator);
    ct.overwrite_unmapped(0, 14, "const __sfc__ = {");
    let (text, ranges) = ct
        .build_string_with_source_ranges()
        .expect("mapped transform");
    let synthetic_end = "const __sfc__ = {".len() as u32;
    assert!(text.starts_with("const __sfc__ = {"));
    assert!(
        ranges
            .iter()
            .all(|range| range.generated_start >= synthetic_end),
        "no range covers the synthetic wrapper: {ranges:?}"
    );
}

#[test]
fn text_and_ranges_come_from_one_pass() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("let a = 1;\nlet b = 2;", &allocator);
    ct.overwrite(4, 5, "x");
    ct.prepend_left(11, "// b\n");
    let chunks = ct.chunk_count();
    super::code_transform::take_code_transform_chunk_visits();
    let (text, _) = ct
        .build_string_with_source_ranges()
        .expect("mapped transform");
    assert_eq!(
        super::code_transform::take_code_transform_chunk_visits(),
        chunks
    );
    assert_eq!(text, ct.build_string());
}
