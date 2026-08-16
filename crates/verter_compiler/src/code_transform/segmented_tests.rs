//! Direct primitive tests for `CodeTransform::try_overwrite_segmented`.
//! See `segmented.rs`'s module doc for the contract under test.

use super::*;
use crate::code_transform::code_transform::CodeTransform;
use crate::code_transform::fallible::CodeTransformError;
use crate::code_transform::source_map::SourceMapOptions;
use oxc_allocator::Allocator;

/// `(generated_line, generated_col, Some((source_line, source_col)) | None)`.
type MappedToken = (u32, u32, Option<(u32, u32)>);

fn mapped_tokens(map: &oxc_sourcemap::SourceMap<'_>) -> Vec<MappedToken> {
    map.get_tokens()
        .map(|t| {
            let src = t
                .get_source_id()
                .map(|_| (t.get_src_line(), t.get_src_col()));
            (t.get_dst_line(), t.get_dst_col(), src)
        })
        .collect()
}

/// A single anchor at the very BEGINNING of the replacement content maps
/// exactly, and the trailing synthetic bytes stay unmapped (no bleed).
#[test]
fn anchor_at_beginning_of_content_maps_exactly_and_trailing_bytes_stay_unmapped() {
    let allocator = Allocator::default();
    // "count" lives at byte 0 in the source.
    let source = "count";
    let mut ct = CodeTransform::new(source, &allocator);
    let anchors = [SegmentAnchor::new(0, 5, 0)];
    ct.try_overwrite_segmented(
        0,
        5,
        "count.value",
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("anchor fits inside the sole Original chunk");

    assert_eq!(ct.build_string(), "count.value");

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    // Token 0: mapped anchor at generated col 0 -> source (0,0).
    assert_eq!(tokens[0], (0, 0, Some((0, 0))));
    // Token 1: unmapped tail (".value") at generated col 5.
    assert_eq!(tokens[1], (0, 5, None));
    assert_eq!(tokens.len(), 2);
}

/// A single anchor in the MIDDLE of the replacement content: unmapped prefix,
/// mapped anchor, unmapped suffix — three tokens, none bleeding into another.
#[test]
fn anchor_in_middle_of_content_produces_prefix_anchor_suffix_tokens() {
    let allocator = Allocator::default();
    let source = "msg";
    let mut ct = CodeTransform::new(source, &allocator);
    // Replacement: `_toDisplayString(msg)` — "msg" anchor sits at offset 17..20.
    let replacement = "_toDisplayString(msg)";
    let anchor_offset = replacement.find("msg").unwrap() as u32;
    let anchors = [SegmentAnchor::new(anchor_offset, 3, 0)];
    ct.try_overwrite_segmented(
        0,
        3,
        replacement,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("anchor fits inside the sole Original chunk");

    assert_eq!(ct.build_string(), replacement);

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    assert_eq!(tokens[0], (0, 0, None)); // unmapped prefix "_toDisplayString("
    assert_eq!(tokens[1], (0, anchor_offset, Some((0, 0)))); // mapped "msg"
    assert_eq!(tokens[2], (0, anchor_offset + 3, None)); // unmapped suffix ")"
    assert_eq!(tokens.len(), 3);
}

/// A single anchor at the very END of the replacement content: unmapped
/// prefix then the mapped anchor, with no trailing token at all (matches
/// `InsertedMapped`'s own trailing-anchor shape).
#[test]
fn anchor_at_end_of_content_has_no_trailing_unmapped_token() {
    let allocator = Allocator::default();
    let source = "x";
    let mut ct = CodeTransform::new(source, &allocator);
    let replacement = "__props.x";
    let anchor_offset = replacement.find('x').unwrap() as u32;
    let anchors = [SegmentAnchor::new(anchor_offset, 1, 0)];
    ct.try_overwrite_segmented(
        0,
        1,
        replacement,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("anchor fits inside the sole Original chunk");

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    assert_eq!(tokens[0], (0, 0, None));
    assert_eq!(tokens[1], (0, anchor_offset, Some((0, 0))));
    assert_eq!(tokens.len(), 2);
}

/// Multiple, non-adjacent anchors inside one replacement — each gets its own
/// exact token at its own generated + source position.
#[test]
fn multiple_anchors_each_map_to_their_own_source_position() {
    let allocator = Allocator::default();
    let source = "a + b";
    let mut ct = CodeTransform::new(source, &allocator);
    // "a" at source 0, "b" at source 4. Replacement embeds both, prefixed.
    let replacement = "(ctx.a + ctx.b)";
    let a_off = replacement.find('a').unwrap() as u32;
    let b_off = replacement.rfind('b').unwrap() as u32;
    let anchors = [
        SegmentAnchor::new(a_off, 1, 0),
        SegmentAnchor::new(b_off, 1, 4),
    ];
    ct.try_overwrite_segmented(
        0,
        5,
        replacement,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("both anchors fit inside the sole Original chunk");

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    assert!(tokens.contains(&(0, a_off, Some((0, 0)))));
    assert!(tokens.contains(&(0, b_off, Some((0, 4)))));
}

/// Two anchors at the SAME content offset are rejected — anchors must be
/// supplied in ascending, non-overlapping order (a zero-width second anchor
/// at the same offset as the first is a caller bug, not a valid ordering).
#[test]
fn anchors_at_same_content_position_are_rejected() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("ab", &allocator);
    let anchors = [
        SegmentAnchor::new(0, 1, 0),
        SegmentAnchor::new(0, 1, 1), // same content_offset as the first — reversed/overlap
    ];
    let Err(err) = ct.try_overwrite_segmented(
        0,
        2,
        "xy",
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    ) else {
        panic!("overlapping/duplicate content_offset must be refused");
    };
    assert!(matches!(err, CodeTransformError::ReversedRange { .. }));
}

/// Multiline replacement content: anchors after an embedded newline map to
/// the correct generated LINE, not just column.
#[test]
fn multiline_content_anchors_advance_generated_line() {
    let allocator = Allocator::default();
    let source = "val";
    let mut ct = CodeTransform::new(source, &allocator);
    let replacement = "line1\nval";
    let anchor_offset = replacement.find("val").unwrap() as u32;
    let anchors = [SegmentAnchor::new(anchor_offset, 3, 0)];
    ct.try_overwrite_segmented(
        0,
        3,
        replacement,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("anchor fits inside the sole Original chunk");

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    // The mapped anchor token must be on generated line 1 (after "line1\n"), column 0.
    assert!(tokens.contains(&(1, 0, Some((0, 0)))));
}

/// Non-ASCII authored source: the anchor's source position resolves through
/// UTF-16 columns exactly like every other mapped chunk kind. The whole
/// source is a single overwrite (no preceding Original chunk), isolating the
/// SOURCE-side UTF-16 resolution from generated-column arithmetic.
#[test]
fn non_ascii_source_anchor_resolves_utf16_column() {
    let allocator = Allocator::default();
    // "\u{3b1}" (Greek alpha, 1 UTF-16 unit, 2 UTF-8 bytes) precedes "val".
    let source = "\u{3b1}val";
    let mut ct = CodeTransform::new(source, &allocator);
    let val_byte_offset = source.find("val").unwrap() as u32;
    let replacement = "_ctx.val";
    let anchor_offset = replacement.find("val").unwrap() as u32;
    let anchors = [SegmentAnchor::new(anchor_offset, 3, val_byte_offset)];
    ct.try_overwrite_segmented(
        0,
        source.len() as u32,
        replacement,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("anchor fits inside the sole Original chunk");

    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    // Source column is UTF-16: alpha is 1 unit, so "val" starts at src col 1.
    // Generated column: no preceding chunk, so it's exactly `anchor_offset`.
    assert!(tokens.contains(&(0, anchor_offset, Some((0, 1)))));
}

/// An anchor whose `content_offset`/length is out of bounds against `content`
/// is a typed refusal, and the transform is left untouched (fail-atomic).
#[test]
fn out_of_bounds_content_anchor_is_rejected_without_mutation() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    let anchors = [SegmentAnchor::new(5, 3, 0)]; // "xy" is only 2 bytes long
    let Err(err) = ct.try_overwrite_segmented(
        0,
        3,
        "xy",
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    ) else {
        panic!("anchor content span exceeds the replacement text");
    };
    assert!(matches!(err, CodeTransformError::OutOfRange { .. }));
    assert_eq!(ct.build_string(), "abc", "a rejected op must not mutate");
}

/// An anchor whose `source_pos` falls outside the original source is a typed
/// refusal.
#[test]
fn out_of_bounds_source_anchor_is_rejected_without_mutation() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    let anchors = [SegmentAnchor::new(0, 2, 100)]; // source has only 3 bytes
    let Err(err) = ct.try_overwrite_segmented(
        0,
        2,
        "xy",
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    ) else {
        panic!("anchor source position exceeds the original source");
    };
    assert!(matches!(err, CodeTransformError::OutOfRange { .. }));
    assert_eq!(ct.build_string(), "abc");
}

/// An anchor content span that splits a UTF-8 character boundary is rejected.
#[test]
fn mid_char_content_anchor_is_rejected() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("x", &allocator);
    // "\u{e9}" is a 2-byte UTF-8 char; anchor [0,1) splits it.
    let content = "\u{e9}yz";
    let anchors = [SegmentAnchor::new(0, 1, 0)];
    let Err(err) = ct.try_overwrite_segmented(
        0,
        1,
        content,
        &anchors,
        SegmentedOverwriteAuthority::new_for_test(),
    ) else {
        panic!("mid-codepoint content anchor must be refused");
    };
    assert!(matches!(err, CodeTransformError::MidChar { .. }));
}

/// A zero-length `[start, end)` replacement range is rejected.
#[test]
fn zero_length_range_is_rejected() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    let Err(err) =
        ct.try_overwrite_segmented(1, 1, "x", &[], SegmentedOverwriteAuthority::new_for_test())
    else {
        panic!("empty range must be refused");
    };
    assert!(matches!(err, CodeTransformError::ZeroLengthRange { .. }));
}

/// A range spanning MORE than one live chunk (already split by a prior edit)
/// is refused rather than silently mis-splicing.
#[test]
fn range_spanning_multiple_chunks_is_rejected() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.overwrite(2, 3, "X"); // splits the Original chunk at [2,3)
    let Err(err) = ct.try_overwrite_segmented(
        0,
        6,
        "whole",
        &[],
        SegmentedOverwriteAuthority::new_for_test(),
    ) else {
        panic!("range crossing an already-edited chunk boundary must be refused");
    };
    assert!(matches!(
        err,
        CodeTransformError::ReplacedContentSplit { .. }
    ));
}

/// Once any affinity-anchored insertion has been used anywhere on this
/// transform, `try_overwrite_segmented` fails closed — the narrow-shape
/// precondition this primitive relies on (see the module doc).
#[test]
fn fails_closed_when_an_anchored_insertion_is_present() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    ct.try_append_left(1, "!").expect("anchored insertion");
    let Err(err) =
        ct.try_overwrite_segmented(1, 2, "x", &[], SegmentedOverwriteAuthority::new_for_test())
    else {
        panic!("anchored insertion anywhere on the transform must refuse this op");
    };
    assert!(matches!(
        err,
        CodeTransformError::ReplacedContentSplit { .. }
    ));
}

/// Zero anchors — the whole replacement is unmapped scaffolding, matching a
/// plain `overwrite_unmapped` call's single "no correspondence" shape but
/// through the segmented path.
#[test]
fn zero_anchors_produces_no_mapped_tokens() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    ct.try_overwrite_segmented(
        0,
        3,
        "xyz",
        &[],
        SegmentedOverwriteAuthority::new_for_test(),
    )
    .expect("empty anchor list is valid");
    let map = ct.generate_map(SourceMapOptions::new().with_source("a.ts"));
    let tokens = mapped_tokens(&map);
    assert_eq!(tokens, vec![(0, 0, None)]);
}
