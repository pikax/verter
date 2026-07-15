//! Characterization tests for the `magic-string` edit model on
//! [`CodeTransform`]: the content-only [`CodeTransform::update`] /
//! [`CodeTransform::try_update`] vs [`CodeTransform::try_overwrite`]
//! boundary-insertion A/B, the four-affinity stacking order of the checked
//! insertion ops, `try_remove`'s interior-insertion clearing, and the typed
//! [`CodeTransformError`] refusals for malformed offsets.
//!
//! The load-bearing semantics under pin (composed edit sequences must render
//! byte-identically to `magic-string`, which official framework compilers use
//! for their emitted CSS/JS carriers):
//!
//! - `update` is CONTENT-ONLY: boundary insertions attached to the replaced
//!   range's first chunk survive; `overwrite` clears them — the A/B that
//!   distinguishes the two range replacements.
//! - `try_append_left` / `try_prepend_right` / `try_append_right` affinity
//!   and per-affinity stacking order at the SAME offset.
//! - `try_remove` clears the insertions attached to every chunk starting
//!   inside the range, but never the PRIOR chunk's left-affinity content.
//! - Splitting a removed (empty-replacement) range transfers its end-boundary
//!   left insertions to the right half and clears them; a split inside a
//!   content-bearing replacement refuses with a typed error.
//! - Malformed offsets (out-of-range, mid-UTF-8-char, reversed, zero-length
//!   where illegal) return a typed [`CodeTransformError`] instead of
//!   panicking, and a refused operation mutates nothing (fail-atomic).

use super::*;
use oxc_allocator::Allocator;

// ─── update vs overwrite: the content-only A/B ──────────────────────────────

#[test]
fn update_preserves_start_boundary_insertion_where_overwrite_clears_it() {
    // The scoped-CSS render shape: a closing `*/` appended with RIGHT
    // affinity at the byte where a `:global` token is then updated away.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(2, "*/").unwrap();
    ct.try_update(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "ab*/ef");

    // The SAME ops through `try_overwrite`: the insertion is cleared with the
    // content — the two operations are semantically distinct.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(2, "*/").unwrap();
    ct.try_overwrite(2, 4, "").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abef");
    // Should-NOT: the range-start right insertion never survives an overwrite.
    assert!(!out.contains("*/"));

    // Same A/B with a `try_prepend_right` insertion at the range start.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_prepend_right(2, "P").unwrap();
    ct.try_update(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "abPef");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_prepend_right(2, "P").unwrap();
    ct.try_overwrite(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "abef");
}

#[test]
fn update_preserves_range_end_left_insertion_where_overwrite_clears_it() {
    // `try_append_left` at the range END lands on the left-affinity boundary
    // that belongs to the replaced range's single chunk — content-only keeps
    // it.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(4, "L").unwrap();
    ct.try_update(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "abLef");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(4, "L").unwrap();
    ct.try_overwrite(2, 4, "").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abef");
    assert!(!out.contains('L'));
}

#[test]
fn prior_chunk_outro_survives_both_update_and_overwrite() {
    // A LEFT-affinity insertion at the range start belongs to the PREVIOUS
    // chunk — outside the replaced range under both operations.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(2, "K").unwrap();
    ct.try_update(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "abKef");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(2, "K").unwrap();
    ct.try_overwrite(2, 4, "").unwrap();
    assert_eq!(ct.build_string(), "abKef");
}

#[test]
fn interior_boundary_insertions_clear_under_update_and_overwrite() {
    // Only the FIRST chunk of the range honors content-only; insertions at
    // interior boundaries are cleared under BOTH operations.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(3, "X").unwrap();
    ct.try_update(2, 5, "").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abf");
    assert!(!out.contains('X'));

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(3, "X").unwrap();
    ct.try_overwrite(2, 5, "").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abf");
    assert!(!out.contains('X'));
}

#[test]
fn update_replaces_content_and_keeps_surrounding_chunks() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_update(2, 4, "XY").unwrap();
    assert_eq!(ct.build_string(), "abXYef");
    // Should-NOT: the replacement never duplicates or drops neighbors.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_overwrite(2, 4, "XY").unwrap();
    assert_eq!(ct.build_string(), "abXYef");
}

#[test]
fn multi_chunk_update_keeps_the_first_chunks_left_outro_and_clears_the_rest() {
    // `update` is content-only on the FIRST chunk of the range only: a
    // left-affinity insertion at the first chunk's end boundary (created by
    // the interior split) survives, rendering right after the replacement
    // content.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(3, "W").unwrap();
    ct.try_update(2, 5, "XY").unwrap();
    assert_eq!(ct.build_string(), "abXYWf");

    // The same insertion clears under a non-content-only overwrite.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(3, "W").unwrap();
    ct.try_overwrite(2, 5, "XY").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abXYf");
    assert!(!out.contains('W'));
}

#[test]
fn multi_chunk_update_clears_the_range_end_left_insertion() {
    // With an interior boundary, the range-end left insertion belongs to an
    // interior (non-first) chunk — content-only no longer protects it.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(3, "G").unwrap(); // interior boundary at 3
    ct.try_append_left(4, "L").unwrap(); // end-boundary left insertion
    ct.try_update(2, 4, "").unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abef");
    assert!(!out.contains('L'));
    assert!(!out.contains('G'));
}

// ─── insertion affinity + stacking at the same offset ───────────────────────

#[test]
fn same_offset_affinity_and_stacking_order() {
    // LEFT content precedes RIGHT content at the same position;
    // `try_append_left` stacks in call order, `try_prepend_right` stacks in
    // REVERSE call order, and `try_append_right` appends after the existing
    // right-affinity content.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcd", &allocator);
    ct.try_append_left(2, "L1").unwrap();
    ct.try_prepend_right(2, "R1").unwrap();
    ct.try_append_left(2, "L2").unwrap();
    ct.try_append_right(2, "R2").unwrap();
    ct.try_prepend_right(2, "R0").unwrap();
    assert_eq!(ct.build_string(), "abL1L2R0R1R2cd");
}

// ─── remove semantics ────────────────────────────────────────────────────────

#[test]
fn remove_clears_interior_insertions_but_not_prior_chunk_outro() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(2, "KEEP").unwrap(); // prior chunk's outro — outside the range
    ct.try_prepend_right(3, "GONE").unwrap(); // interior insertion — cleared
    ct.try_append_right(2, "GONE2").unwrap(); // right insertion at the removed range start — cleared
    ct.try_remove(2, 5).unwrap();
    let out = ct.build_string();
    assert_eq!(out, "abKEEPf");
    // Should-NOT: no interior insertion survives a remove.
    assert!(!out.contains("GONE"));
}

#[test]
fn remove_zero_length_range_is_a_no_op() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_remove(2, 2).unwrap();
    assert_eq!(ct.build_string(), "abcdef");
}

// ─── splitting replaced (removed) ranges ─────────────────────────────────────

#[test]
fn edited_chunk_split_transfers_then_clears_the_outro() {
    // Splitting a REMOVED (empty-replacement) range hands its end-boundary
    // left insertions to the right half and immediately clears them — the
    // `magic-string` `Chunk.split` + `edit('', false)` behavior.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_remove(2, 5).unwrap();
    ct.try_append_left(5, "TAIL").unwrap(); // left insertion at the removed range's end
    ct.try_append_left(3, "MID").unwrap(); // splits the removed range at 3
    let out = ct.build_string();
    assert_eq!(out, "abMIDf");
    // Should-NOT: the transferred end insertion never survives the split.
    assert!(!out.contains("TAIL"));
}

// ─── the buffer edge insertion runs ──────────────────────────────────────────

#[test]
fn append_left_zero_and_prepend_right_len_land_at_the_buffer_edges() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    ct.try_append_left(0, "A").unwrap(); // no chunk ends at 0 → leading edge
    ct.try_prepend_right(0, "B").unwrap(); // right-affinity content of the first chunk
    ct.try_append_left(3, "C").unwrap(); // left-affinity content of the last chunk
    ct.try_prepend_right(3, "D").unwrap(); // no chunk starts at len → trailing edge
    assert_eq!(ct.build_string(), "ABabcCD");
}

#[test]
fn buffer_edge_insertions_stack_in_affinity_order() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    ct.try_append_left(0, "A1").unwrap();
    ct.try_append_left(0, "A2").unwrap(); // append_left stacks in call order
    ct.try_prepend_right(3, "D1").unwrap();
    ct.try_prepend_right(3, "D2").unwrap(); // prepend_right stacks in reverse call order
    assert_eq!(ct.build_string(), "A1A2abcD2D1");
}

// ─── typed refusals: malformed offsets never panic ───────────────────────────

#[test]
fn split_inside_content_bearing_replacement_errors_instead_of_panicking() {
    // A boundary strictly inside content a previous edit already replaced
    // cannot be expressed — the operation refuses with a typed error.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_update(2, 5, "XY").unwrap();
    let err = ct.try_append_left(3, "Z").err().unwrap();
    assert_eq!(err, CodeTransformError::ReplacedContentSplit { offset: 3 });
    // The refused op mutated nothing.
    assert_eq!(ct.build_string(), "abXYf");

    // The empty-replacement split (the remove-then-insert sequence) stays a
    // VALID operation — no error.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_remove(2, 5).unwrap();
    ct.try_append_left(3, "Z").unwrap();
    assert_eq!(ct.build_string(), "abZf");
}

#[test]
fn reversed_and_zero_length_replace_ranges_error_instead_of_panicking() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    let err = ct.try_update(4, 2, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::ReversedRange { start: 4, end: 2 });
    assert_eq!(ct.build_string(), "abcdef");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    let err = ct.try_overwrite(3, 3, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::ZeroLengthRange { offset: 3 });
    assert_eq!(ct.build_string(), "abcdef");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    let err = ct.try_remove(5, 1).err().unwrap();
    assert_eq!(err, CodeTransformError::ReversedRange { start: 5, end: 1 });
    assert_eq!(ct.build_string(), "abcdef");
}

#[test]
fn out_of_range_offsets_error_instead_of_panicking() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abc", &allocator);
    let err = ct.try_append_left(99, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 3 });

    let mut ct = CodeTransform::new("abc", &allocator);
    let err = ct.try_remove(0, 99).err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 3 });

    let mut ct = CodeTransform::new("abc", &allocator);
    let err = ct.try_update(1, 99, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 3 });

    let mut ct = CodeTransform::new("abc", &allocator);
    let err = ct.try_prepend_right(99, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 3 });

    let mut ct = CodeTransform::new("abc", &allocator);
    let err = ct.try_append_right(99, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 3 });
    // Should-NOT: the refused ops never mutated the transform.
    assert_eq!(ct.build_string(), "abc");
}

#[test]
fn mid_character_offsets_error_instead_of_panicking() {
    // `é` spans bytes 1..3 — an edit offset inside it cannot form a chunk
    // boundary; the op refuses and `build_string` stays safe.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("aébc", &allocator);
    let err = ct.try_append_left(2, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::MidChar { offset: 2 });
    assert_eq!(ct.build_string(), "aébc");

    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("aébc", &allocator);
    let err = ct.try_update(2, 4, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::MidChar { offset: 2 });
    assert_eq!(ct.build_string(), "aébc");
}

#[test]
fn erroring_operation_is_a_no_op_not_a_torn_half_edit() {
    // The `Result` model's fail-closed contract: an operation that refuses
    // its offsets mutates NOTHING (the refused result is never a torn
    // half-edit), surfaces the typed error, and leaves the transform fully
    // usable.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(2, "K").unwrap();
    let err = ct.try_remove(0, 99).err().unwrap();
    assert_eq!(err, CodeTransformError::OutOfRange { offset: 99, len: 6 });
    assert_eq!(ct.build_string(), "abKcdef");

    // A second malformed op refuses independently, still without mutation.
    let err = ct.try_update(4, 2, "x").err().unwrap();
    assert_eq!(err, CodeTransformError::ReversedRange { start: 4, end: 2 });
    assert_eq!(ct.build_string(), "abKcdef");

    // A subsequent VALID op still applies normally: the prior-chunk left
    // insertion `K` survives a content-only update of [2, 4).
    ct.try_update(2, 4, "XY").unwrap();
    assert_eq!(ct.build_string(), "abKXYef");
}

// ─── the unchecked convenience twin ──────────────────────────────────────────

#[test]
fn convenience_update_is_the_content_only_unchecked_twin() {
    // Valid ranges: `update` behaves exactly like `try_update`, including the
    // boundary-insertion preservation that distinguishes it from `overwrite`.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_right(2, "*/").unwrap();
    ct.update(2, 4, "");
    assert_eq!(ct.build_string(), "ab*/ef");

    // Zero-length range: silent no-op, mirroring `overwrite`'s convenience
    // contract.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.update(3, 3, "X");
    assert_eq!(ct.build_string(), "abcdef");

    // Plain replacement without boundary insertions matches `overwrite`.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.update(2, 4, "XY");
    assert_eq!(ct.build_string(), "abXYef");
}

// ─── source maps stay valid across the checked ops ───────────────────────────

#[test]
fn affinity_insertions_are_unmapped_in_the_generated_sourcemap() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_append_left(2, "L").unwrap();
    ct.try_prepend_right(2, "R").unwrap();
    ct.try_update(3, 5, "XY").unwrap();
    assert_eq!(ct.build_string(), "abLRcXYf");

    let map = ct.generate_map(
        SourceMapOptions::new()
            .with_source("test.css")
            .include_content(true),
    );
    let sources: Vec<_> = map.get_sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(map.get_source_content(0).unwrap(), "abcdef");
}

// ─── overlapping removals account exactly (never a wrapped capacity) ─────────

#[test]
fn overlapping_removals_do_not_double_count_the_output_delta() {
    // A removal INSIDE an already/later-removed range must not be charged
    // twice: the tracked delta is the NET output-length change, and
    // `build_string`'s capacity derives from it — a double-count drives
    // `original.len() + delta` negative and the `as usize` cast wraps into a
    // capacity-overflow abort. This is the scoped-css minify shape: the
    // whitespace run before a rule's closing brace is removed first, then
    // the WHOLE unused rule (covering that run), then the outside trims.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("p { color: red; }", &allocator);
    ct.try_remove(15, 16).unwrap(); // the space before `}`
    ct.try_remove(0, 17).unwrap(); // the whole rule (covers the space)
    assert_eq!(ct.output_delta(), -17, "net delta is the WHOLE source once");
    assert_eq!(ct.build_string(), "", "everything removed exactly once");

    // The same overlap through the unchecked positional path (shared splice).
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("p { color: red; }", &allocator);
    ct.remove(15, 16);
    ct.remove(0, 17);
    assert_eq!(ct.output_delta(), -17);
    assert_eq!(ct.build_string(), "");

    // Overlap with a CONTENT-bearing replacement: the second overwrite
    // subsumes the first replacement's content — charged as the content's
    // length, not the original range's.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_overwrite(2, 4, "XYZW").unwrap(); // +2
    ct.try_remove(1, 5).unwrap(); // removes b, XYZW, e
    assert_eq!(ct.build_string(), "af");
    assert_eq!(ct.output_delta(), -4);

    // A partially-overlapping removal never over-charges the shared bytes.
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    ct.try_remove(0, 3).unwrap();
    ct.try_remove(2, 6).unwrap(); // [2,3) already removed
    assert_eq!(ct.build_string(), "");
    assert_eq!(ct.output_delta(), -6);
}
