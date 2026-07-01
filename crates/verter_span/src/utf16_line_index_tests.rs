//! Offset-parity tests: [`Utf16LineIndex`] must be BYTE-IDENTICAL to the scalar
//! converters [`utf16_offset_to_byte`] / [`utf16_offset_to_line_col`] over a battery
//! of contents and offsets (ASCII, em-dash, astral/supplementary, CRLF, lone CR,
//! U+2028/U+2029, boundary + past-end). The parity assertions are the discriminator:
//! any divergence between the build-once index and the scalars fails RED.

use super::*;
use crate::tsgo_offset::{utf16_offset_to_byte, utf16_offset_to_line_col};

/// Assert the index agrees with BOTH scalar converters for every offset in
/// `0..=text_u16_len + 3` (covers every in-range offset, the boundary, and several
/// past-end offsets).
fn assert_parity_all_offsets(s: &str) {
    let index = Utf16LineIndex::new(s);
    let u16_len: u32 = s.chars().map(|c| c.len_utf16() as u32).sum();
    for offset in 0..=(u16_len + 3) {
        let (want_line, want_col) = utf16_offset_to_line_col(s, offset);
        let got = index
            .line_col_for_utf16(offset)
            .expect("index built by new() always has line starts");
        assert_eq!(
            (got.line, got.col),
            (want_line, want_col),
            "line_col parity drift at offset {offset} in {s:?}: index={got:?}, scalar=({want_line},{want_col})"
        );

        let want_byte = utf16_offset_to_byte(s, offset) as usize;
        let got_byte = index
            .byte_for_utf16(offset)
            .expect("index built by new() always has line starts");
        assert_eq!(
            got_byte, want_byte,
            "byte parity drift at offset {offset} in {s:?}: index={got_byte}, scalar={want_byte}"
        );
    }
}

#[test]
fn parity_ascii_multiline() {
    assert_parity_all_offsets("abc\ndef");
    assert_parity_all_offsets("");
    assert_parity_all_offsets("single line no newline");
    assert_parity_all_offsets("trailing newline\n");
}

#[test]
fn parity_em_dash_bmp_multibyte() {
    // em-dash U+2014: 3 UTF-8 bytes / 1 UTF-16 unit.
    assert_parity_all_offsets("a\u{2014}b");
    assert_parity_all_offsets("a\u{2014}b\ncd");
    // é U+00E9: 2 UTF-8 bytes / 1 UTF-16 unit.
    assert_parity_all_offsets("café!");
    assert_parity_all_offsets("café\nqux");
}

#[test]
fn parity_supplementary_astral_pairs() {
    // U+10437: 4 UTF-8 bytes / 2 UTF-16 units (a surrogate pair) — exercises the
    // mid-surrogate clamp on both queries.
    assert_parity_all_offsets("x\u{10437}y");
    assert_parity_all_offsets("\u{10437}");
    assert_parity_all_offsets("\u{10437}x");
    assert_parity_all_offsets("a\u{10437}\nb\u{10437}c");
}

#[test]
fn parity_crlf_single_terminator() {
    assert_parity_all_offsets("ab\r\ncd");
    assert_parity_all_offsets("\r\n");
    assert_parity_all_offsets("line1\r\nline2\r\nline3");
}

#[test]
fn parity_lone_cr_terminator() {
    assert_parity_all_offsets("ab\rcd");
    assert_parity_all_offsets("a\rb\r\nc"); // mixed lone-CR + CRLF (each counts once)
    assert_parity_all_offsets("\r");
}

#[test]
fn parity_unicode_line_and_paragraph_separators() {
    assert_parity_all_offsets("a\u{2028}b"); // U+2028 LINE SEPARATOR
    assert_parity_all_offsets("a\u{2029}b"); // U+2029 PARAGRAPH SEPARATOR
    assert_parity_all_offsets("a\u{2028}\u{2029}b");
    // A separator adjacent to astral content stresses both the col clamp and the
    // byte walk across the multi-byte terminator.
    assert_parity_all_offsets("\u{10437}\u{2028}\u{2014}");
}

#[test]
fn parity_boundary_and_past_end_explicit() {
    // Explicit pinned boundary values (matching the scalar's own tests).
    let index = Utf16LineIndex::new("abc");
    assert_eq!(
        index.line_col_for_utf16(0).unwrap(),
        LineCol { line: 1, col: 1 }
    );
    assert_eq!(
        index.line_col_for_utf16(999).unwrap(),
        LineCol { line: 1, col: 4 },
        "past-end clamps to the final column (parity with the scalar)"
    );
    assert_eq!(
        index.byte_for_utf16(999).unwrap(),
        3,
        "past-end byte clamps to len"
    );

    // Mid-surrogate resolves to the pair start on BOTH queries.
    let astral = Utf16LineIndex::new("\u{10437}");
    assert_eq!(
        astral.line_col_for_utf16(1).unwrap(),
        LineCol { line: 1, col: 2 },
        "mid-surrogate column clamps to the pair start"
    );
    assert_eq!(
        astral.byte_for_utf16(1).unwrap(),
        0,
        "mid-surrogate byte clamps to the pair start byte"
    );
}

#[test]
fn line_count_matches_terminators() {
    assert_eq!(Utf16LineIndex::new("").line_count(), 1);
    assert_eq!(Utf16LineIndex::new("abc").line_count(), 1);
    assert_eq!(Utf16LineIndex::new("a\nb").line_count(), 2);
    assert_eq!(Utf16LineIndex::new("a\nb\n").line_count(), 3); // trailing NL opens a 3rd (empty) line
    assert_eq!(Utf16LineIndex::new("a\r\nb").line_count(), 2); // CRLF is ONE terminator
    assert_eq!(Utf16LineIndex::new("a\rb\r\nc").line_count(), 3); // lone CR + CRLF
}

#[test]
fn empty_index_error_is_unreachable_via_new_but_typed() {
    // `new()` always records line 1 at (0,0), so a query can never hit EmptyIndex.
    // This pins that the happy path returns Ok for a degenerate empty file.
    let index = Utf16LineIndex::new("");
    assert!(index.line_col_for_utf16(0).is_ok());
    assert!(index.byte_for_utf16(0).is_ok());
    // Display coverage for the reserved error variant.
    assert!(!OffsetError::EmptyIndex.to_string().is_empty());
}
