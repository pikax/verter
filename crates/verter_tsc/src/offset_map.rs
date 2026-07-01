//! UTF-16 offset → (line, column) conversion for tsgo `--api` diagnostics.
//!
//! The tsgo `--api` diagnostic surface reports each diagnostic as a `pos`/`end`
//! pair measured in **UTF-16 code units** — TypeScript's position unit
//! (`getLineAndCharacterOfPosition` / the TS `LanguageService`), which the Go
//! engine preserves on this surface. The temp-file `tsgo --project` path instead
//! read tsc's TEXT output, which already carried `(line, col)`. To keep the
//! in-memory `--api` typecheck path byte-for-byte parity with that text path,
//! this module converts an `--api` UTF-16 offset into the 1-based `(line, col)`
//! tsc would report:
//!
//! - `line` is 1 + the number of `\n` code units before the offset.
//! - `col` is 1 + the number of UTF-16 code units between the start of that line
//!   and the offset — the same UTF-16 column the inline source map produced by
//!   Verter's compiler is keyed in.
//!
//! Treating the offset as UTF-8 BYTES instead would drift by one line for every
//! multi-byte character earlier in the file (e.g. an em-dash `—`, U+2014: 3 UTF-8
//! bytes but 1 UTF-16 unit — common in the generated carriers' comments), which
//! is exactly the diagnostic-position drift this conversion prevents.

/// Convert a UTF-16 code-unit `offset` into `content` to a 1-based `(line, col)`
/// position, with `col` also measured in UTF-16 code units (TypeScript position
/// semantics). An offset past the end is clamped to the end; an offset landing
/// inside a surrogate pair resolves to a stable in-pair column (never panics).
pub fn offset_to_line_col(content: &str, offset: u32) -> (u32, u32) {
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

    #[test]
    fn start_of_file_is_one_one() {
        assert_eq!(offset_to_line_col("abc", 0), (1, 1));
    }

    #[test]
    fn ascii_same_line_column() {
        // "abc\ndef" (ASCII ⇒ UTF-16 unit == byte): offset 5 is the 'e' on line 2.
        let s = "abc\ndef";
        assert_eq!(offset_to_line_col(s, 5), (2, 2));
        // offset 4 ('d') is column 1 of line 2.
        assert_eq!(offset_to_line_col(s, 4), (2, 1));
        // offset 2 ('c') is column 3 of line 1.
        assert_eq!(offset_to_line_col(s, 2), (1, 3));
    }

    #[test]
    fn newline_reports_end_of_line_column() {
        // The '\n' unit itself reports as the end-of-line position (col after last char).
        let s = "abc\ndef";
        assert_eq!(offset_to_line_col(s, 3), (1, 4));
    }

    #[test]
    fn multiple_lines() {
        let s = "a\nbb\nccc";
        assert_eq!(offset_to_line_col(s, 0), (1, 1)); // 'a'
        assert_eq!(offset_to_line_col(s, 2), (2, 1)); // 'b'
        assert_eq!(offset_to_line_col(s, 5), (3, 1)); // 'c'
        assert_eq!(offset_to_line_col(s, 7), (3, 3)); // last 'c'
    }

    #[test]
    fn em_dash_offset_is_utf16_not_byte() {
        // THE REGRESSION: an em-dash (U+2014) is 3 UTF-8 bytes but 1 UTF-16 unit.
        // A UTF-16 offset landing after it must NOT be treated as a byte offset —
        // the generated carriers carry `—` in comments, which shifted lines by one.
        // "a—b\ncd": UTF-16 units a(0) —(1) b(2) \n(3) c(4) d(5).
        let s = "a\u{2014}b\ncd";
        // offset 2 = 'b' on line 1, col 3 (a, —, then b).
        assert_eq!(offset_to_line_col(s, 2), (1, 3));
        // offset 4 = 'c' — MUST be line 2 col 1 (a byte reading would land on line 1
        // because the em-dash consumes 2 extra bytes but only 1 UTF-16 unit).
        assert_eq!(offset_to_line_col(s, 4), (2, 1));
        // offset 5 = 'd' on line 2, col 2.
        assert_eq!(offset_to_line_col(s, 5), (2, 2));
    }

    #[test]
    fn bmp_multibyte_counts_one_utf16_unit() {
        // "café\nx": é is 1 UTF-16 unit. UTF-16 units: c(0) a(1) f(2) é(3) \n(4) x(5).
        let s = "café\nx";
        // offset 3 = the é position → line 1, col 4.
        assert_eq!(offset_to_line_col(s, 3), (1, 4));
        // offset 4 = the '\n' → line 1, col 5 (é counted once).
        assert_eq!(offset_to_line_col(s, 4), (1, 5));
        // offset 5 = 'x' → line 2, col 1.
        assert_eq!(offset_to_line_col(s, 5), (2, 1));
    }

    #[test]
    fn supplementary_plane_char_is_two_utf16_units() {
        // "𐐷x" (U+10437): 2 UTF-16 units (a surrogate pair), then 'x' at unit 2.
        let s = "𐐷x";
        // offset 2 = after the pair → line 1, col 3.
        assert_eq!(offset_to_line_col(s, 2), (1, 3));
    }

    #[test]
    fn out_of_range_offset_is_clamped_to_end() {
        assert_eq!(offset_to_line_col("abc", 999), (1, 4));
    }

    #[test]
    fn mid_surrogate_offset_does_not_panic() {
        // An offset landing inside a surrogate pair resolves to a stable column.
        let s = "𐐷";
        let (line, _col) = offset_to_line_col(s, 1);
        assert_eq!(line, 1);
    }
}
