//! Contract cases for the content-mapper projection plane.
//!
//! Each case pins one boundary the plane exists to hold and that the mapping
//! product alone does not discriminate: the refusal-not-clamping rule that
//! decides what a position question may be answered with, the two fail-closed
//! classes read from the wire side, the completeness and ordering of the
//! carrier-to-projected direction, and the separation of the carrier's lineage
//! from the map construction over it.

use oxc_allocator::Allocator;

use verter_compiler::code_transform::{CodeTransform, MappingProduct, MappingSpan};
use verter_identity::identity::{ContentId, SourceId, SourceUnitId};
use verter_session::content_mapper::{CarrierAnswer, ContentMapper, ProjectionAnswer, Refusal};

fn span(start: u32, end: u32) -> MappingSpan {
    MappingSpan { start, end }
}

fn unit_of(path: &str) -> SourceUnitId {
    let source = SourceId::from_canonical(&Lineage(path));
    SourceUnitId::from_lineage(&source, "script")
}

struct Lineage<'a>(&'a str);

impl verter_identity::encoding::CanonicalEncode for Lineage<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.tests.content_mapper.lineage.v1";

    fn encode_fields(&self, encoder: &mut verter_identity::encoding::CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

fn mapper_of(path: &str, source: &str, product: MappingProduct) -> ContentMapper {
    ContentMapper::project(
        unit_of(path),
        &ContentId::from_content_bytes(source.as_bytes()),
        product,
    )
}

/// One transform carrying every disposition the wire semantics branch on:
/// identity source, an ordinary (region-to-region) rewrite, a synthesized
/// intro, and wholly synthetic replacement text whose carrier extent is
/// therefore elided.
///
/// Output: `/*p*/const a = 42; /*gone*/tail;`
/// | projected | class       | carrier |
/// | 0..5      | Synthesized | none    |
/// | 5..15     | Identity    | 0..10   |
/// | 15..17    | Rewritten   | 10..11  |
/// | 17..19    | Identity    | 11..13  |
/// | 19..27    | Synthesized | none    |
/// | 27..33    | Identity    | 25..31  |
const MIXED_SOURCE: &str = "const a = 1; const b = 2; tail;";

fn mixed<'a>(allocator: &'a Allocator) -> CodeTransform<'a> {
    let mut ct = CodeTransform::new(MIXED_SOURCE, allocator);
    ct.prepend("/*p*/");
    ct.overwrite(10, 11, "42");
    ct.overwrite_unmapped(13, 25, "/*gone*/");
    ct
}

#[test]
fn a_projected_offset_past_the_surface_is_refused_rather_than_clamped_to_its_end() {
    let allocator = Allocator::default();
    let ct = mixed(&allocator);
    let built = ct.build_string();
    let product = MappingProduct::of(&ct);
    assert_eq!(product.projected_len(), built.len() as u32);
    let mapper = mapper_of("mixed.vue", MIXED_SOURCE, product);
    let end = mapper.product().projected_len();

    // This range starts inside the last mapped region and runs past the end.
    // Truncating it to the surface answers `Span(28..31)` — a well-formed
    // correspondence to a range the caller never named, indistinguishable at
    // the call site from a real one, and applied at the wrong place.
    assert_eq!(
        mapper.to_carrier(span(30, end + 7)),
        CarrierAnswer::Refused(Refusal::OutOfRange),
        "a range overrunning the projected surface is refused, not truncated \
         to the part of itself that happens to map"
    );
    assert_eq!(
        mapper.to_carrier(span(end, end + 4)),
        CarrierAnswer::Refused(Refusal::OutOfRange),
        "a range wholly past the projected surface has no correspondence to \
         clamp to"
    );
    assert_eq!(
        mapper.to_carrier(span(end, end)),
        CarrierAnswer::Refused(Refusal::OutOfRange),
        "the one-past-the-end insertion point is covered by no region and \
         declares no anchor, so it has no authored destination"
    );
    assert_eq!(
        mapper.to_carrier(span(end - 2, end - 6)),
        CarrierAnswer::Refused(Refusal::InvertedRange),
        "an inverted range asked no question and is not silently normalised"
    );
    assert_eq!(
        mapper.to_projected(mapper.product().carrier_len()),
        ProjectionAnswer::Refused(Refusal::OutOfRange),
        "a carrier offset past the source is refused, not answered with its \
         last region"
    );
}

#[test]
fn a_range_reaching_synthesized_bytes_is_refused_whole_rather_than_narrowed() {
    let allocator = Allocator::default();
    let ct = mixed(&allocator);
    let mapper = mapper_of("mixed.vue", MIXED_SOURCE, MappingProduct::of(&ct));

    // 17..19 is authored, 19..27 is the wholly synthetic replacement text. A
    // mapper that answered with the mapped part alone would apply the caller's
    // edit to a strictly smaller region than the one it named.
    assert_eq!(
        mapper.to_carrier(span(17, 27)),
        CarrierAnswer::Refused(Refusal::NoCarrierPreimage)
    );
    assert_eq!(
        mapper.to_carrier(span(17, 19)),
        CarrierAnswer::Span(span(11, 13)),
        "the same range stopping short of the synthesized bytes still answers"
    );
    assert_eq!(
        mapper.to_carrier(span(20, 22)),
        CarrierAnswer::Refused(Refusal::NoCarrierPreimage),
        "synthesized bytes have no preimage at any granularity"
    );
}

#[test]
fn an_elided_carrier_position_answers_with_no_projection_rather_than_a_neighbour() {
    let allocator = Allocator::default();
    let ct = mixed(&allocator);
    let mapper = mapper_of("mixed.vue", MIXED_SOURCE, MappingProduct::of(&ct));

    // Carrier 13..25 reaches no output at all. Its neighbours do, so a mapper
    // that fell through to the nearest correspondence would return a
    // confident, wrong answer here rather than a visible gap.
    for offset in [13u32, 18, 24] {
        assert_eq!(
            mapper.to_projected(offset),
            ProjectionAnswer::Refused(Refusal::NoProjection),
            "carrier offset {offset} is elided"
        );
    }
    assert_eq!(
        mapper.to_projected(12),
        ProjectionAnswer::Complete(vec![span(18, 19)]),
        "the byte immediately before the elided run still projects"
    );
    assert_eq!(
        mapper.to_projected(25),
        ProjectionAnswer::Complete(vec![span(27, 28)]),
        "the byte immediately after it does too"
    );
}

#[test]
fn rewritten_text_answers_at_region_granularity_and_refuses_inside_itself() {
    let allocator = Allocator::default();
    let ct = mixed(&allocator);
    let mapper = mapper_of("mixed.vue", MIXED_SOURCE, MappingProduct::of(&ct));

    assert_eq!(
        mapper.to_carrier(span(15, 17)),
        CarrierAnswer::Span(span(10, 11)),
        "the whole rewritten region maps to the whole carrier region"
    );
    // The rewritten text is two bytes standing for one; there is no byte-to-byte
    // correspondence inside it to slice.
    assert_eq!(
        mapper.to_carrier(span(16, 17)),
        CarrierAnswer::Refused(Refusal::NotByteAddressable)
    );
    assert_eq!(
        mapper.to_carrier(span(16, 16)),
        CarrierAnswer::Refused(Refusal::NotByteAddressable),
        "an insertion point interior to rewritten text has no exact authored \
         offset either"
    );
    assert_eq!(
        mapper.to_carrier(span(6, 9)),
        CarrierAnswer::Span(span(1, 4)),
        "identity bytes are still exact at byte granularity"
    );
    assert_eq!(
        mapper.to_projected(10),
        ProjectionAnswer::Complete(vec![span(15, 17)]),
        "a rewritten carrier byte answers with the whole rewritten region, \
         which is the only granularity its correspondence carries"
    );
    assert_eq!(
        mapper.to_projected(7),
        ProjectionAnswer::Complete(vec![span(12, 13)]),
        "an identity carrier byte answers with the one projected byte its \
         offset delta names, not the region around it"
    );
}

#[test]
fn a_carrier_position_answers_with_every_projection_in_ascending_projected_order() {
    let allocator = Allocator::default();
    let source = "alpha beta";
    let mut ct = CodeTransform::new(source, &allocator);
    let inserted = ct.alloc_str("X");
    // A mapped insertion at output position 6 whose authored destination is the
    // carrier POINT 2 — the same authored position the surrounding identity run
    // already projects.
    ct.batch_prepend_left_with_source_map(&[(6, Some((2, 0)), inserted)]);
    assert_eq!(ct.build_string(), "alpha Xbeta");
    let mapper = mapper_of("two-emissions.vue", source, MappingProduct::of(&ct));

    assert_eq!(
        mapper.to_projected(2),
        ProjectionAnswer::Complete(vec![span(2, 3), span(6, 7)]),
        "a carrier position derived into two projections answers with both, \
         in projected order — a caller reaching only the first would leave the \
         projected surface internally inconsistent"
    );
    assert_eq!(
        mapper.to_projected(1),
        ProjectionAnswer::Complete(vec![span(1, 2)]),
        "a position before the anchored point produced one projection and \
         answers with one — completeness is the position's own derivation, \
         not the widest list in the file"
    );
    assert_eq!(
        mapper.to_carrier(span(6, 7)),
        CarrierAnswer::Point(2),
        "projected bytes that account for no authored extent answer with the \
         carrier point they stand at, not a fabricated span"
    );
}

#[test]
fn relocated_preimages_with_a_gap_between_them_refuse_instead_of_spanning_it() {
    let allocator = Allocator::default();
    let source = "ABCDEF";
    let mut ct = CodeTransform::new(source, &allocator);
    ct.move_slice(4, 6, 0);
    assert_eq!(ct.build_string(), "EFABCD");
    let mapper = mapper_of("relocated.vue", source, MappingProduct::of(&ct));

    assert_eq!(
        mapper.to_carrier(span(0, 2)),
        CarrierAnswer::Span(span(4, 6)),
        "relocated bytes keep an exact region-to-region correspondence"
    );
    assert_eq!(
        mapper.to_projected(5),
        ProjectionAnswer::Complete(vec![span(1, 2)]),
        "a relocated carrier byte answers with the one projected byte it \
         was copied to"
    );
    // Output 0..3 is carrier 4..6 followed by carrier 0..1. Answering with the
    // hull 0..6 would claim carrier 1..4, which this range does not name.
    assert_eq!(
        mapper.to_carrier(span(0, 3)),
        CarrierAnswer::Refused(Refusal::Discontiguous)
    );
}

#[test]
fn a_declared_zero_width_insertion_resolves_to_its_authored_anchor() {
    let allocator = Allocator::default();
    let source = "<script>const value = 1;</script>";
    let mut ct = CodeTransform::new(source, &allocator);
    let preamble = ct.alloc_str("import { ref } from \"vue\";\n");
    ct.batch_prepend_left_static(&[(8, preamble)]);
    ct.set_helper_preamble_content_at(preamble, 8);
    let mapper = mapper_of("preamble.vue", source, MappingProduct::of(&ct));

    // An engine anchors an auto-import at the start of the generated preamble.
    // Those bytes are synthesized, so the insertion is answerable only through
    // the authored destination the transform declared for it; without that the
    // whole edit is refused and the import silently disappears.
    assert_eq!(mapper.to_carrier(span(8, 8)), CarrierAnswer::Point(8));
    assert_eq!(
        mapper.to_carrier(span(8, 12)),
        CarrierAnswer::Refused(Refusal::NoCarrierPreimage),
        "the anchor does not give the synthesized preamble bytes a preimage"
    );
    assert_eq!(
        mapper.insertion_anchors().len(),
        1,
        "the anchor is carried into the plane, not re-derived from the text"
    );
}

#[test]
fn an_edit_changes_the_map_revision_while_the_carrier_unit_survives_it() {
    let allocator = Allocator::default();
    let before = "const a = 1;";
    let after = "const a = 2;";
    let mut first = CodeTransform::new(before, &allocator);
    first.overwrite(10, 11, "42");
    let mut second = CodeTransform::new(after, &allocator);
    second.overwrite(10, 11, "42");

    let before_map = mapper_of("edited.vue", before, MappingProduct::of(&first));
    let after_map = mapper_of("edited.vue", after, MappingProduct::of(&second));

    assert_eq!(
        before_map.unit(),
        after_map.unit(),
        "lineage is not content: an edit must not mint a new carrier unit, or \
         every association the previous projection held is lost at the first \
         keystroke"
    );
    assert_ne!(
        before_map.revision(),
        after_map.revision(),
        "the map construction is content-bearing and must move with the edit"
    );
    assert_ne!(
        before_map.projection_map_id(),
        after_map.projection_map_id()
    );
}

#[test]
fn identical_projections_compose_identical_identities_and_a_different_one_differs() {
    let allocator = Allocator::default();
    let ct = mixed(&allocator);
    let one = mapper_of("stable.vue", MIXED_SOURCE, MappingProduct::of(&ct));
    let two = mapper_of("stable.vue", MIXED_SOURCE, MappingProduct::of(&ct));
    assert_eq!(one.revision(), two.revision());
    assert_eq!(one.projection_map_id(), two.projection_map_id());
    assert_eq!(one.product(), two.product());

    // Same carrier bytes, different projection of them: the geometry is part of
    // the map construction, so the two are not interchangeable.
    let mut without_rewrite = CodeTransform::new(MIXED_SOURCE, &allocator);
    without_rewrite.prepend("/*p*/");
    without_rewrite.overwrite_unmapped(13, 25, "/*gone*/");
    let other = mapper_of(
        "stable.vue",
        MIXED_SOURCE,
        MappingProduct::of(&without_rewrite),
    );
    assert_eq!(one.unit(), other.unit());
    assert_ne!(
        one.revision(),
        other.revision(),
        "two projections of identical bytes are different map constructions"
    );
}
