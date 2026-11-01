//! Checked-coordinate tests for the successor coordinate domains.
//!
//! The rows follow the coordinate constitution's case table
//! (`docs/arch/kernel/coordinates.md`, "Case table"). Domain mixing
//! (rows K01–K03) is pinned by the `compile_fail` doctests on the module;
//! the table-driven rows live here and each one pins a refusal the
//! constitution's law names: checked arithmetic never clamps or wraps
//! (K04–K08), a position that is not a boundary of its domain is a typed
//! error (K09–K12), line terminators and cross-encoding conversion are
//! exact-or-refused (K13–K25), and the newtypes cost nothing over the
//! existing width (K26, K28). Rows the constitution marks as the boundary
//! owners' own corpora (the lone-`\r` and U+2028/U+2029 wire forms, wire
//! ingress, lone surrogates arriving from a JavaScript string, the
//! UTF-8-session conversion buffer) belong to the boundary owners, not
//! here.

use super::*;

// ======================== K04–K08: checked arithmetic ========================

#[test]
fn checked_add_refuses_overflow_k04() {
    assert_eq!(
        SourceByteOffset(u32::MAX).checked_add(1),
        Err(CoordError::Overflow)
    );
    assert_eq!(
        GeneratedByteOffset(u32::MAX).checked_add(1),
        Err(CoordError::Overflow)
    );
    assert_eq!(
        EmbeddedByteOffset(u32::MAX).checked_add(1),
        Err(CoordError::Overflow)
    );
    assert_eq!(
        SourceByteOffset(u32::MAX - 1).checked_add(1),
        Ok(SourceByteOffset(u32::MAX))
    );
    assert_eq!(
        SourceByteOffset(0).checked_add(u32::MAX),
        Ok(SourceByteOffset(u32::MAX))
    );
}

#[test]
fn checked_sub_refuses_underflow_and_answers_a_length_k05() {
    assert_eq!(
        SourceByteOffset(5).checked_sub(SourceByteOffset(6)),
        Err(CoordError::Overflow)
    );
    assert_eq!(
        GeneratedByteOffset(5).checked_sub(GeneratedByteOffset(6)),
        Err(CoordError::Overflow)
    );
    assert_eq!(
        EmbeddedByteOffset(5).checked_sub(EmbeddedByteOffset(6)),
        Err(CoordError::Overflow)
    );
    // The answer is the same base-relative length unit `checked_add`
    // takes, never an absolute offset of the domain: a rebase result must
    // not type-check as a position.
    let length = SourceByteOffset(6)
        .checked_sub(SourceByteOffset(5))
        .unwrap();
    assert_eq!(length, 1);
    assert_eq!(SourceByteOffset(5).checked_sub(SourceByteOffset(5)), Ok(0));
    let end = SourceByteOffset(2).checked_add(length).unwrap();
    assert_eq!(end, SourceByteOffset(3));
    // The unit composes: add takes what sub answered, and only add mints
    // an offset.
    let _position: SourceByteOffset = SourceByteOffset(2).checked_add(length).unwrap();
}

#[test]
fn inverted_range_is_refused_not_swapped_k06() {
    assert_eq!(
        SourceByteRange::checked_new(SourceByteOffset(5), SourceByteOffset(3)),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        GeneratedByteRange::checked_new(GeneratedByteOffset(5), GeneratedByteOffset(3)),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        EmbeddedByteRange::checked_new(EmbeddedByteOffset(5), EmbeddedByteOffset(3)),
        Err(CoordError::InvalidBoundary)
    );
    // The ordered form is fine and keeps its endpoints as given.
    let range = SourceByteRange::checked_new(SourceByteOffset(3), SourceByteOffset(5)).unwrap();
    assert_eq!((range.start.0, range.end.0), (3, 5));
}

/// The infallible constructor refuses inversion loudly too: an inverted
/// range is never silently normalised to an empty one whose `len()` clamps
/// to 0.
#[test]
#[should_panic(expected = "a byte range is ordered")]
fn inverted_range_is_refused_by_the_infallible_constructor_k06() {
    let _ = SourceByteRange::new(SourceByteOffset(5), SourceByteOffset(3));
}

/// Every length the range answers is a plain subtraction: an ordered range
/// of length `n` answers exactly `n`, and the empty range answers 0 without
/// any saturating fallback.
#[test]
fn range_len_is_plain_subtraction_on_ordered_ranges() {
    assert_eq!(
        SourceByteRange::new(SourceByteOffset(3), SourceByteOffset(5)).len(),
        2
    );
    assert_eq!(
        GeneratedByteRange::new(GeneratedByteOffset(0), GeneratedByteOffset(u32::MAX)).len(),
        u32::MAX
    );
}

#[test]
fn empty_range_is_valid_k07() {
    for (start, end) in [(0, 0), (5, 5)] {
        let range = SourceByteRange::checked_new(SourceByteOffset(start), SourceByteOffset(end))
            .expect("start == end is a valid empty range");
        assert_eq!(range.len(), 0);
        assert!(range.is_empty());
    }
    // Empty at end-of-file, validated against the content snapshot.
    let content = "ab";
    let at_eof = SourceByteOffset(content.len() as u32);
    let range = SourceByteRange::checked_new(at_eof, at_eof)
        .unwrap()
        .validate(content)
        .expect("an empty range at end-of-file is valid against the content it indexes");
    assert_eq!(range.len(), 0);
    assert!(range.is_empty());
    // And at offset 0 of empty content.
    let zero = SourceByteRange::checked_new(SourceByteOffset(0), SourceByteOffset(0))
        .unwrap()
        .validate("")
        .expect("an empty range at offset 0 of empty content is valid");
    assert_eq!(zero.len(), 0);
}

#[test]
fn rebase_past_u32_width_refuses_k08() {
    // Relative-to-absolute rebase: base + relative offset must fit u32.
    let base = SourceByteOffset(u32::MAX - 3);
    assert_eq!(base.checked_add(4), Err(CoordError::Overflow));
    assert_eq!(base.checked_add(3), Ok(SourceByteOffset(u32::MAX)));
}

// ======================== coordinate-width narrowing ========================

/// The converters mint no coordinate they cannot name: narrowing a byte
/// position or counted extent to the coordinate width is exact-or-refused,
/// never a truncation. Pinned at the narrowing boundary itself, which no
/// realizable content fixture could reach.
#[test]
fn width_narrowing_refuses_instead_of_truncating() {
    assert_eq!(in_u32(u32::MAX as u64), Ok(u32::MAX));
    assert_eq!(in_u32(0), Ok(0));
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        in_u32(u32::MAX as u64 + 1),
        Err(CoordError::Overflow),
        "2^32 byte positions do not truncate to offset 0"
    );
    // The wire line/column form narrows through the same refusal. Its col
    // is 1-based, so u32::MAX units into the line is the last column that
    // still fits; one more unit into the line overflows the wire form.
    assert_eq!(wire_line_col(1, u32::MAX as u64 - 1).unwrap().col, u32::MAX);
    assert_eq!(wire_line_col(1, u32::MAX as u64), Err(CoordError::Overflow));
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        wire_line_col(u32::MAX as u64 + 1, 0),
        Err(CoordError::Overflow)
    );
    assert_eq!(wire_line_col(2, 3).unwrap(), LineCol { line: 2, col: 4 });
}

/// Even a range assembled by literal inside this crate (the one route left,
/// since the struct refuses literals from other crates) meets the typed
/// refusal at the checking boundary: it is rejected, never emptied.
#[test]
fn a_literal_assembled_inverted_range_still_meets_the_typed_refusal() {
    let inverted = SourceByteRange {
        start: SourceByteOffset(5),
        end: SourceByteOffset(3),
    };
    assert_eq!(
        inverted.validate("0123456789"),
        Err(CoordError::InvalidBoundary)
    );
    assert!(
        !inverted.is_empty(),
        "an inverted range is not a valid empty range"
    );
}

// ======================== K09–K12: boundaries ========================

/// `a` + U+00E9 (2 bytes) + U+2014 (3 bytes) + U+1F600 (4 bytes) + `z` = 11
/// bytes, 6 UTF-16 units, 5 UTF-32 scalars.
const MIXED_CONTENT: &str = "a\u{00E9}\u{2014}\u{1F600}z";

#[test]
fn end_of_file_offset_is_valid_k09() {
    let eof = MIXED_CONTENT.len() as u32;
    assert_eq!(
        SourceByteOffset(eof).validate(MIXED_CONTENT),
        Ok(SourceByteOffset(eof))
    );
    assert_eq!(
        GeneratedByteOffset(eof).validate(MIXED_CONTENT),
        Ok(GeneratedByteOffset(eof))
    );
    assert_eq!(
        EmbeddedByteOffset(eof).validate(MIXED_CONTENT),
        Ok(EmbeddedByteOffset(eof))
    );
    // EOF also converts to the final line/column position (K17 covers lines).
    assert_eq!(
        SourceByteOffset(eof).line_col(MIXED_CONTENT),
        Ok(LineCol { line: 1, col: 7 })
    );
}

#[test]
fn past_end_offset_is_refused_k10() {
    let past = MIXED_CONTENT.len() as u32 + 1;
    assert_eq!(
        SourceByteOffset(past).validate(MIXED_CONTENT),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        GeneratedByteOffset(past).validate(MIXED_CONTENT),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        SourceByteOffset(past).to_utf16(MIXED_CONTENT),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        SourceByteOffset(past).to_utf32(MIXED_CONTENT),
        Err(CoordError::InvalidBoundary)
    );
}

#[test]
fn interior_sequence_bytes_refused_k11() {
    // Byte layout: a=0, é=1..=2, —=3..=5, 😀=6..=9, z=10.
    // Interior byte of the 2-byte sequence (U+00E9).
    assert_eq!(
        SourceByteOffset(2).validate(MIXED_CONTENT),
        Err(CoordError::NotCharBoundary)
    );
    // Interior bytes of the 3-byte sequence (U+2014).
    for raw in [4, 5] {
        assert_eq!(
            SourceByteOffset(raw).validate(MIXED_CONTENT),
            Err(CoordError::NotCharBoundary),
            "interior byte {raw} of U+2014"
        );
    }
    // Interior bytes of the 4-byte sequence (U+1F600).
    for raw in [7, 8, 9] {
        assert_eq!(
            SourceByteOffset(raw).validate(MIXED_CONTENT),
            Err(CoordError::NotCharBoundary),
            "interior byte {raw} of U+1F600"
        );
    }
    // Not snapped to the sequence start either: the scalar starts are valid.
    for raw in [1, 3, 6, 10] {
        assert!(SourceByteOffset(raw).validate(MIXED_CONTENT).is_ok());
    }
}

#[test]
fn empty_content_boundaries_k12() {
    assert_eq!(SourceByteOffset(0).validate(""), Ok(SourceByteOffset(0)));
    assert_eq!(
        SourceByteOffset(1).validate(""),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        SourceByteOffset(0).line_col(""),
        Ok(LineCol { line: 1, col: 1 })
    );
}

// ======================== K13–K18: line terminators ========================

#[test]
fn crlf_is_one_terminator_k13() {
    // "a\r\nb": b is on the SECOND line (0-based line 1), column 0 — under the
    // TypeScript rule and the LSP 3.17 rule alike, because \r\n is ONE
    // terminator. The fact pinned here: b is NOT on a third line.
    let content = "a\r\nb";
    let b = SourceByteOffset(3);
    assert_eq!(b.line_col(content), Ok(LineCol { line: 2, col: 1 }));
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 2, col: 1 }),
        Ok(b)
    );
    // Only two lines exist.
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 3, col: 1 }),
        Err(CoordError::InvalidBoundary)
    );
}

#[test]
fn between_cr_and_lf_has_no_line_col_k14() {
    let content = "a\r\nb";
    // Byte 2 sits between the \r and the \n: a valid source-domain byte offset
    // (it is a UTF-8 scalar boundary)...
    assert_eq!(
        SourceByteOffset(2).validate(content),
        Ok(SourceByteOffset(2))
    );
    // ...but it lies inside a two-byte terminator, so it has NO line/column
    // position: refused, never a past-end column or a next-line position.
    assert_eq!(
        SourceByteOffset(2).line_col(content),
        Err(CoordError::InvalidBoundary)
    );
}

#[test]
fn terminator_at_eof_k17() {
    // line count = terminators + 1; (last line, 0-based column 0) == EOF.
    let content = "a\n";
    let eof = SourceByteOffset(2);
    assert_eq!(eof.line_col(content), Ok(LineCol { line: 2, col: 1 }));
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 2, col: 1 }),
        Ok(eof)
    );
    // Column one past the line's units is the line's end boundary.
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 1, col: 2 }),
        Ok(SourceByteOffset(1))
    );
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 3, col: 1 }),
        Err(CoordError::InvalidBoundary)
    );
    // Three terminators → four lines.
    let content = "a\nb\nc\n";
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 4, col: 1 }),
        Ok(SourceByteOffset(6))
    );
    assert_eq!(
        SourceByteOffset::from_line_col(content, LineCol { line: 5, col: 1 }),
        Err(CoordError::InvalidBoundary)
    );
}

#[test]
fn mixed_endings_round_trip_exactly_k18() {
    // One file mixing \n, \r\n and lone \r. Each terminator counts once by the
    // TypeScript rule; byte -> line/column -> byte is exact for every scalar
    // boundary that has a line/column position.
    let content = "a\nb\r\nc\rd";
    // a=0 \n=1 b=2 \r=3 \n=4 c=5 \r=6 d=7 (len 8).
    let line_starts = [
        (SourceByteOffset(0), 1),
        (SourceByteOffset(2), 2),
        (SourceByteOffset(5), 3),
        (SourceByteOffset(7), 4),
    ];
    for (offset, line) in line_starts {
        assert_eq!(
            offset.line_col(content),
            Ok(LineCol { line, col: 1 }),
            "line start {offset:?}"
        );
    }
    for raw in 0..=(content.len() as u32) {
        let offset = SourceByteOffset(raw);
        match offset.line_col(content) {
            Ok(pos) => {
                assert_eq!(
                    SourceByteOffset::from_line_col(content, pos),
                    Ok(offset),
                    "round trip drift at byte {raw}"
                );
            }
            // The only scalar boundary without a line/column position is byte 4,
            // between the \r and the \n of the CRLF pair (K14).
            Err(CoordError::InvalidBoundary) => assert_eq!(raw, 4),
            other => panic!("unexpected line_col at byte {raw}: {other:?}"),
        }
    }
}

// ======================== K19–K24: Unicode ========================

#[test]
fn astral_scalar_widths_round_trip_k19() {
    let content = "\u{1F600}x";
    let after = SourceByteOffset(4);
    // UTF-8 width 4, UTF-16 width 2, UTF-32 width 1.
    assert_eq!(after.to_utf16(content), Ok(2));
    assert_eq!(after.to_utf32(content), Ok(1));
    assert_eq!(SourceByteOffset::from_utf16(content, 2), Ok(after));
    assert_eq!(SourceByteOffset::from_utf32(content, 1), Ok(after));
    // Round trips exact in all three encodings for every scalar boundary.
    for raw in [0, 4, 5] {
        let offset = SourceByteOffset(raw);
        let u16_units = offset.to_utf16(content).unwrap();
        let u32_units = offset.to_utf32(content).unwrap();
        assert_eq!(SourceByteOffset::from_utf16(content, u16_units), Ok(offset));
        assert_eq!(SourceByteOffset::from_utf32(content, u32_units), Ok(offset));
    }
}

#[test]
fn utf16_in_pair_offset_refused() {
    // The checked internal form (K20's wire-ingress row itself is the boundary
    // owners'): a UTF-16 unit offset inside a surrogate pair never becomes a
    // byte offset — refused, not snapped to the pair start.
    let content = "\u{1F600}x";
    assert_eq!(
        SourceByteOffset::from_utf16(content, 1),
        Err(CoordError::NotCharBoundary)
    );
    // Past the content's unit length is out of extent.
    assert_eq!(
        SourceByteOffset::from_utf16(content, 4),
        Err(CoordError::InvalidBoundary)
    );
    assert_eq!(
        SourceByteOffset::from_utf32(content, 3),
        Err(CoordError::InvalidBoundary)
    );
}

#[test]
fn combining_mark_counts_code_units_k21() {
    let content = "e\u{0301}x";
    // The offset between the base and the mark is a valid scalar boundary.
    let between = SourceByteOffset(1);
    assert_eq!(between.validate(content), Ok(between));
    // Columns count UTF-16 code units, not graphemes: e is column 1, the
    // combining mark column 2, x column 3.
    assert_eq!(
        SourceByteOffset(1).line_col(content),
        Ok(LineCol { line: 1, col: 2 })
    );
    assert_eq!(
        SourceByteOffset(3).line_col(content),
        Ok(LineCol { line: 1, col: 3 })
    );
}

#[test]
fn zwj_sequence_scalar_boundaries_k22() {
    let content = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    // 4+3+4+3+4 = 18 bytes; UTF-16 2+1+2+1+2 = 8 units; UTF-32 5 scalars.
    assert_eq!(content.len(), 18);
    for raw in [0, 4, 7, 11, 14, 18] {
        let offset = SourceByteOffset(raw);
        assert!(offset.validate(content).is_ok(), "boundary {raw}");
        assert_eq!(
            SourceByteOffset::from_utf16(content, offset.to_utf16(content).unwrap()),
            Ok(offset)
        );
        assert_eq!(
            SourceByteOffset::from_utf32(content, offset.to_utf32(content).unwrap()),
            Ok(offset)
        );
    }
    let end = SourceByteOffset(18);
    assert_eq!(end.to_utf16(content), Ok(8));
    assert_eq!(end.to_utf32(content), Ok(5));
}

#[test]
fn bom_is_counted_as_content_k23() {
    let content = "\u{FEFF}a";
    assert_eq!(content.len(), 4);
    let after = SourceByteOffset(3);
    assert_eq!(after.validate(content), Ok(after));
    // 3 UTF-8 bytes, 1 UTF-16 unit, 1 UTF-32 unit — content in every domain.
    assert_eq!(after.to_utf16(content), Ok(1));
    assert_eq!(after.to_utf32(content), Ok(1));
    assert_eq!(SourceByteOffset::from_utf16(content, 1), Ok(after));
    assert_eq!(SourceByteOffset::from_utf32(content, 1), Ok(after));
}

#[test]
fn generated_non_ascii_converts_exactly_k24() {
    // Generated surfaces carry authored non-ASCII identifiers and literals
    // (R07): byte and UTF-16 positions differ and both convert exactly.
    // Layout: `const `=0..5, `caf`=6..8, `é`=9..10, ` = `=11..13, `"`=14,
    // 日=15..17, 本=18..20, 語=21..23, `"`=24, `;`=25.
    let content = "const caf\u{00E9} = \"\u{65E5}\u{672C}\u{8A9E}\";";
    let after_identifier = GeneratedByteOffset(11);
    let at_cjk_start = GeneratedByteOffset(15);
    let before_last_cjk = GeneratedByteOffset(21);
    for (offset, expect_u16) in [
        (after_identifier, 10),
        (at_cjk_start, 14),
        (before_last_cjk, 16),
    ] {
        let got = offset.to_utf16(content).unwrap();
        assert_ne!(
            offset.0, got,
            "byte and UTF-16 positions must differ past non-ASCII"
        );
        assert_eq!(got, expect_u16);
        assert_eq!(
            GeneratedByteOffset::from_utf16(content, got),
            Ok(offset),
            "exact conversion at byte {}",
            offset.0
        );
        let scalars = offset.to_utf32(content).unwrap();
        assert_eq!(
            GeneratedByteOffset::from_utf32(content, scalars),
            Ok(offset)
        );
    }
    // The embedded domain converts against its cooked text the same way.
    // `color: red `=0..10, →=11..13, ` #f00`=14..18.
    let cooked = "color: red \u{2192} #f00";
    let after_arrow = EmbeddedByteOffset(14);
    assert_eq!(after_arrow.to_utf16(cooked), Ok(12));
    assert_eq!(EmbeddedByteOffset::from_utf16(cooked, 12), Ok(after_arrow));
    assert_eq!(after_arrow.validate(cooked), Ok(after_arrow));
}

// ======================== K26, K28: baseline cost ========================

#[test]
fn layout_is_transparent_over_the_existing_width_k26() {
    assert_eq!(
        std::mem::size_of::<SourceByteOffset>(),
        std::mem::size_of::<u32>()
    );
    assert_eq!(
        std::mem::align_of::<SourceByteOffset>(),
        std::mem::align_of::<u32>()
    );
    assert_eq!(
        std::mem::size_of::<GeneratedByteOffset>(),
        std::mem::size_of::<u32>()
    );
    assert_eq!(
        std::mem::align_of::<GeneratedByteOffset>(),
        std::mem::align_of::<u32>()
    );
    assert_eq!(
        std::mem::size_of::<EmbeddedByteOffset>(),
        std::mem::size_of::<u32>()
    );
    assert_eq!(
        std::mem::align_of::<EmbeddedByteOffset>(),
        std::mem::align_of::<u32>()
    );
    assert_eq!(
        std::mem::size_of::<GeneratedByteLen>(),
        std::mem::size_of::<u32>()
    );
    assert_eq!(
        std::mem::align_of::<GeneratedByteLen>(),
        std::mem::align_of::<u32>()
    );
    for range in [
        std::mem::size_of::<SourceByteRange>(),
        std::mem::size_of::<GeneratedByteRange>(),
        std::mem::size_of::<EmbeddedByteRange>(),
    ] {
        assert_eq!(range, std::mem::size_of::<[u32; 2]>());
    }
    for range in [
        std::mem::align_of::<SourceByteRange>(),
        std::mem::align_of::<GeneratedByteRange>(),
        std::mem::align_of::<EmbeddedByteRange>(),
    ] {
        assert_eq!(range, std::mem::align_of::<[u32; 2]>());
    }
}

/// Spec-derived oracle (the definition of the conversion, not a copy of the
/// implementation): UTF-16 units are the `len_utf16` of every scalar strictly
/// before the byte offset.
fn oracle_to_utf16(content: &str, byte: u32) -> u32 {
    content
        .char_indices()
        .take_while(|(b, _)| (*b as u32) < byte)
        .map(|(_, ch)| ch.len_utf16() as u32)
        .sum()
}

fn oracle_to_utf32(content: &str, byte: u32) -> u32 {
    content
        .char_indices()
        .take_while(|(b, _)| (*b as u32) < byte)
        .count() as u32
}

#[test]
fn ascii_fast_path_equals_general_path_k28() {
    let corpora = [
        "fn ascii_name() {\n  let value = \"literal\";\n}\n",
        "const caf\u{00E9} = \"\u{65E5}\u{672C}\"; // \u{1F600} \u{2014}\n",
    ];
    for content in corpora {
        // Offsets the conversion exists for (`R06`: an interior byte of a
        // multi-byte sequence is refused — pinned by K11). ASCII corpora make
        // every byte offset a boundary, so the fast path is exercised end to
        // end; the non-ASCII corpus exercises the general path.
        let boundaries: Vec<u32> = (0..=(content.len() as u32))
            .filter(|offset| content.is_char_boundary(*offset as usize))
            .collect();
        for offset in boundaries {
            let typed = SourceByteOffset(offset);
            // The fast path (ASCII corpora) must agree with the general path
            // (the char-wise oracle) on every offset of both corpora.
            assert_eq!(
                typed.to_utf16(content).unwrap(),
                oracle_to_utf16(content, offset),
                "utf16 drift at byte {offset} of {content:?}"
            );
            assert_eq!(
                typed.to_utf32(content).unwrap(),
                oracle_to_utf32(content, offset),
                "utf32 drift at byte {offset} of {content:?}"
            );
            // Every unit count in extent is a char start or end-of-file, so the
            // ingress direction is exact too.
            let u16_units = typed.to_utf16(content).unwrap();
            assert_eq!(
                SourceByteOffset::from_utf16(content, u16_units).unwrap().0,
                offset
            );
            let u32_units = typed.to_utf32(content).unwrap();
            assert_eq!(
                SourceByteOffset::from_utf32(content, u32_units).unwrap().0,
                offset
            );
        }
    }
}
