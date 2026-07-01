//! UTF-16 code-unit offset conversions.
//!
//! TypeScript positions (`getLineAndCharacterOfPosition`, the `LanguageService`,
//! and the tsgo `--api` diagnostic `pos`/`end` pair) are measured in **UTF-16
//! code units**, not UTF-8 bytes. This module is the single owner of the
//! conversion FROM a UTF-16 code-unit offset into the two coordinate forms
//! Verter consumes:
//!
//! - a **byte offset** into the same content (the [`Span`](crate::Span) /
//!   `TypeDiagnostic` byte contract), via [`utf16_offset_to_byte`];
//! - a 1-based **(line, column)** with the column also in UTF-16 code units (the
//!   coordinate the inline source maps Verter's compiler emits are keyed in), via
//!   [`utf16_offset_to_line_col`].
//!
//! Treating a UTF-16 offset as a UTF-8 byte offset drifts on any multi-byte
//! character earlier in the content — e.g. an em-dash `—` (U+2014) is 3 UTF-8
//! bytes but 1 UTF-16 unit — which shifts a byte reading by 2 for that one
//! character. These converters count UTF-16 units so the result is exact.
//!
//! `verter_span` is the leaf coordinate owner, so both the `verter_tsgo_api`
//! wire boundary and `verter-tsc` share this ONE implementation rather than each
//! carrying its own offset walk.

/// Convert a UTF-16 code-unit `offset` into `content` to a **byte offset** into
/// the same content.
///
/// An `offset` past the content's UTF-16 length is clamped to `content.len()`
/// (the end of the byte buffer). An `offset` landing INSIDE a surrogate pair
/// resolves to the byte offset of the pair's start (never a mid-`char` byte
/// index, never a panic).
#[must_use]
pub fn utf16_offset_to_byte(content: &str, offset: u32) -> u32 {
    let target = offset as usize; // UTF-16 code units
    let mut u16_idx = 0usize; // UTF-16 units consumed so far

    for (byte_idx, ch) in content.char_indices() {
        let ch_units = ch.len_utf16();
        // `target` falls at this char's start, OR strictly inside it (a mid-surrogate
        // offset for a 2-unit char): resolve to this char's START byte — never a
        // mid-`char` byte index.
        if target < u16_idx + ch_units {
            return byte_idx as u32;
        }
        u16_idx += ch_units;
    }

    // `offset` is at or past the content's total UTF-16 length: the byte offset is
    // the end of the buffer.
    content.len() as u32
}

/// Convert a UTF-16 code-unit `offset` into `content` to a 1-based `(line, col)`
/// position, with `col` also measured in UTF-16 code units (TypeScript position
/// semantics).
///
/// - `line` is 1 + the number of `\n` code units before the offset (a `\r\n` is a
///   single terminator whose line starts after the `\n`; the `\r` and `\n`
///   themselves report on the preceding line).
/// - `col` is 1 + the number of UTF-16 code units between the start of that line
///   and the offset.
///
/// An offset past the end is clamped to the end; an offset landing inside a
/// surrogate pair resolves to a stable in-pair column (never panics).
#[must_use]
pub fn utf16_offset_to_line_col(content: &str, offset: u32) -> (u32, u32) {
    let target = offset as usize; // UTF-16 code units

    let mut u16_idx = 0usize; // UTF-16 units consumed so far
    let mut line = 1u32;
    let mut line_start = 0usize; // UTF-16 index of the current line's first unit

    for ch in content.chars() {
        if u16_idx >= target {
            break;
        }
        u16_idx += ch.len_utf16();
        if ch == '\n' {
            line += 1;
            line_start = u16_idx; // first unit after the '\n'
        }
    }

    // `u16_idx` is the offset just after the last consumed unit (>= target, or the
    // content's total UTF-16 length when the offset runs past the end). Clamp so a
    // past-the-end offset reports the final column and never underflows.
    let eff = target.min(u16_idx);
    (line, (eff.saturating_sub(line_start)) as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── utf16_offset_to_byte ────────────────────────────────────────────────

    #[test]
    fn byte_ascii_is_identity() {
        // ASCII: 1 UTF-16 unit == 1 byte, so byte offset == UTF-16 offset.
        let s = "abc\ndef";
        assert_eq!(utf16_offset_to_byte(s, 0), 0);
        assert_eq!(utf16_offset_to_byte(s, 4), 4); // 'd'
        assert_eq!(utf16_offset_to_byte(s, 5), 5); // 'e'
    }

    #[test]
    fn byte_em_dash_offset_is_utf16_not_byte() {
        // THE REGRESSION the whole block turns on: an em-dash (U+2014) is 3 UTF-8
        // bytes but 1 UTF-16 unit. A UTF-16 offset AFTER it must resolve to the
        // correct BYTE offset (which is 2 larger than the UTF-16 offset), not be
        // copied straight through as if UTF-16 units were bytes.
        // "a—b": bytes a(0) —(1,2,3) b(4); UTF-16 units a(0) —(1) b(2).
        let s = "a\u{2014}b";
        assert_eq!(utf16_offset_to_byte(s, 0), 0, "'a' at byte 0");
        assert_eq!(utf16_offset_to_byte(s, 1), 1, "'—' starts at byte 1");
        // UTF-16 offset 2 is 'b'. Its BYTE offset is 4 (the em-dash ate 3 bytes).
        // A straight passthrough (byte == utf16 offset) would wrongly give 2.
        assert_eq!(
            utf16_offset_to_byte(s, 2),
            4,
            "'b' is at byte 4, NOT byte 2 (the em-dash is 3 bytes / 1 UTF-16 unit)"
        );
    }

    #[test]
    fn byte_bmp_multibyte_counts_one_utf16_unit() {
        // "café": c(0) a(1) f(2) é(3,4 bytes) — é is 2 UTF-8 bytes / 1 UTF-16 unit.
        let s = "café!";
        // UTF-16 offset 3 = é start = byte 3.
        assert_eq!(utf16_offset_to_byte(s, 3), 3);
        // UTF-16 offset 4 = '!' = byte 5 (é ate 2 bytes).
        assert_eq!(utf16_offset_to_byte(s, 4), 5);
    }

    #[test]
    fn byte_supplementary_pair_is_two_utf16_units() {
        // "x𐐷y": 𐐷 (U+10437) is 4 UTF-8 bytes / 2 UTF-16 units.
        // bytes x(0) 𐐷(1..5) y(5); UTF-16 x(0) hi(1) lo(2) y(3).
        let s = "x\u{10437}y";
        assert_eq!(utf16_offset_to_byte(s, 0), 0); // 'x'
        assert_eq!(utf16_offset_to_byte(s, 1), 1); // start of the pair
        assert_eq!(utf16_offset_to_byte(s, 3), 5); // 'y' — after the 4-byte / 2-unit pair
    }

    #[test]
    fn byte_mid_surrogate_resolves_to_pair_start() {
        // Offset 1 lands INSIDE the surrogate pair of "𐐷" (units 0,1). It resolves
        // to the pair's START byte (0), never a mid-char byte index or a panic.
        let s = "\u{10437}";
        assert_eq!(utf16_offset_to_byte(s, 1), 0);
    }

    #[test]
    fn byte_past_end_clamps_to_len() {
        let s = "abc";
        assert_eq!(utf16_offset_to_byte(s, 999), 3);
        // Non-ASCII: "é" is 2 bytes / 1 unit; past-end clamps to byte len 2.
        assert_eq!(utf16_offset_to_byte("é", 999), 2);
    }

    // ── utf16_offset_to_line_col ────────────────────────────────────────────

    #[test]
    fn line_col_start_of_file_is_one_one() {
        assert_eq!(utf16_offset_to_line_col("abc", 0), (1, 1));
    }

    #[test]
    fn line_col_ascii_same_line_column() {
        let s = "abc\ndef";
        assert_eq!(utf16_offset_to_line_col(s, 5), (2, 2)); // 'e'
        assert_eq!(utf16_offset_to_line_col(s, 4), (2, 1)); // 'd'
        assert_eq!(utf16_offset_to_line_col(s, 2), (1, 3)); // 'c'
    }

    #[test]
    fn line_col_em_dash_offset_is_utf16_not_byte() {
        // "a—b\ncd": UTF-16 units a(0) —(1) b(2) \n(3) c(4) d(5).
        let s = "a\u{2014}b\ncd";
        assert_eq!(utf16_offset_to_line_col(s, 2), (1, 3)); // 'b'
                                                            // offset 4 = 'c' — MUST be line 2 col 1 (a byte reading would land on line 1
                                                            // because the em-dash consumes 2 extra bytes but only 1 UTF-16 unit).
        assert_eq!(utf16_offset_to_line_col(s, 4), (2, 1));
        assert_eq!(utf16_offset_to_line_col(s, 5), (2, 2)); // 'd'
    }

    #[test]
    fn line_col_supplementary_plane_char_is_two_utf16_units() {
        // "𐐷x": 2 UTF-16 units (surrogate pair), then 'x' at unit 2.
        let s = "\u{10437}x";
        assert_eq!(utf16_offset_to_line_col(s, 2), (1, 3));
    }

    #[test]
    fn line_col_crlf_is_single_terminator() {
        // "ab\r\ncd": units a(0) b(1) \r(2) \n(3) c(4) d(5). `\r\n` is ONE terminator.
        let s = "ab\r\ncd";
        assert_eq!(utf16_offset_to_line_col(s, 2), (1, 3)); // at the `\r`, still line 1
        assert_eq!(utf16_offset_to_line_col(s, 3), (1, 4)); // at the `\n`, still line 1
        assert_eq!(utf16_offset_to_line_col(s, 4), (2, 1)); // after `\r\n`, line 2
    }

    #[test]
    fn line_col_out_of_range_offset_is_clamped_to_end() {
        assert_eq!(utf16_offset_to_line_col("abc", 999), (1, 4));
    }

    #[test]
    fn line_col_mid_surrogate_resolves_to_stable_in_pair_column() {
        // Offset 1 is mid-pair in "𐐷" and clamps to the pair's start column → (1, 2).
        assert_eq!(utf16_offset_to_line_col("\u{10437}", 1), (1, 2));
    }
}
