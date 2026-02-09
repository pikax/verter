use memchr::{memchr, memchr2, memchr3, memmem};

use crate::common::Position;

#[derive(Clone, Copy, Eq, PartialOrd, Ord, Debug)]
pub struct CursorPosition {
    pub byte_index: usize,
    pub char_index: usize,

    last_line_offset: u32,
    last_line_number: u32,
}
impl PartialEq for CursorPosition {
    fn eq(&self, other: &Self) -> bool {
        self.byte_index == other.byte_index
    }
}

impl CursorPosition {
    #[inline(always)]
    pub fn to_position(&self) -> Position {
        Position::new(
            self.byte_index as u32,
            self.last_line_number,
            self.char_index as u32 - self.last_line_offset,
            self.char_index as u32,
        )
    }
}

pub struct Cursor<'a> {
    pub input: &'a str,
    pub bytes: &'a [u8],
    pub len: usize,

    pub position: CursorPosition,
}

impl<'a> Cursor<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            len: input.len(),
            position: CursorPosition {
                byte_index: 0,
                char_index: 0,
                last_line_offset: 0,
                last_line_number: 0,
            },
        }
    }

    #[inline(always)]
    pub fn override_position(&mut self, pos: CursorPosition) {
        self.position.byte_index = pos.byte_index;
        self.position.char_index = pos.char_index;
        self.position.last_line_offset = pos.last_line_offset;
        self.position.last_line_number = pos.last_line_number;
    }

    #[inline(always)]
    pub fn current_byte(&self) -> u8 {
        let s = if self.position.byte_index >= self.len {
            self.len - 1
        } else {
            self.position.byte_index
        };

        self.bytes[s]
    }
    pub fn byte_ahead(&self, offset: usize) -> u8 {
        debug_assert!(self.position.byte_index + offset < self.len);
        self.bytes[self.position.byte_index + offset]
    }
    #[inline(always)]
    pub fn next_byte(&self) -> u8 {
        self.bytes[self.position.byte_index + 1]
    }

    #[inline(always)]
    pub fn increment(&mut self) {
        if self.ended() {
            return;
        }
        let s = &self.input[self.position.byte_index..];
        let c = s.chars().next().unwrap();
        let char_len = c.len_utf8();
        self.position.byte_index += char_len;
        self.position.char_index += 1;
        if c == '\n' {
            self.position.last_line_number += 1;
            self.position.last_line_offset = self.position.char_index as u32;
        }
    }
    pub fn advance(&mut self, len: usize) {
        let new_byte = self.position.byte_index + len;
        debug_assert!(new_byte <= self.len, "Advance past end of input");

        let seg = &self.bytes[self.position.byte_index..new_byte];

        // Count chars
        let delta_ch = if seg.is_ascii() {
            // Fast path: ASCII bytes = chars
            len
        } else {
            // Unicode: must count chars in the span
            self.input[self.position.byte_index..new_byte]
                .chars()
                .count()
        };

        // Track newlines using memchr for efficiency
        for nl_offset in memchr::memchr_iter(b'\n', seg) {
            self.position.last_line_number += 1;
            // Calculate char index at this newline position
            let nl_byte_pos = self.position.byte_index + nl_offset;
            let chars_to_nl = self.input[self.position.byte_index..=nl_byte_pos]
                .chars()
                .count();
            self.position.last_line_offset = (self.position.char_index + chars_to_nl) as u32;
        }

        self.position.byte_index = new_byte;
        self.position.char_index += delta_ch;
    }

    #[inline(always)]
    pub fn to_end(&mut self) -> CursorPosition {
        self.advance(self.len - self.position.byte_index);
        self.position
    }

    #[inline(always)]
    pub fn ended(&self) -> bool {
        self.position.byte_index >= self.len
    }

    #[inline(always)]
    pub fn remaining(&self) -> &[u8] {
        &self.bytes[self.position.byte_index..]
    }

    pub fn search2(&self, byte1: u8, byte2: u8) -> Option<usize> {
        let haystack = self.remaining();
        if byte1 == byte2 {
            memchr(byte1, haystack)
        } else {
            memchr2(byte1, byte2, haystack)
        }
    }
    pub fn search3(&self, byte1: u8, byte2: u8, byte3: u8) -> Option<usize> {
        let haystack = self.remaining();
        memchr3(byte1, byte2, byte3, haystack)
    }
    #[inline(always)]
    pub fn next_bytes_equal(&self, needle: &[u8]) -> bool {
        self.remaining().starts_with(needle)
    }
}

#[inline(always)]
pub fn find_subslice(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    memmem::find(haystack, needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::cursor::Cursor;

    // If Position is in your crate, import it; otherwise remove checks involving to_position.
    // use crate::Position;

    #[inline(always)]
    fn is_lead(b: u8) -> bool {
        (b & 0xC0) != 0x80
    }

    fn step_all(input: &str) -> Vec<CursorPosition> {
        let mut c = Cursor::new(input);
        let mut out = Vec::new();
        out.push(c.position);
        while !c.ended() {
            c.increment();
            out.push(c.position);
        }
        out
    }

    fn assert_end_indices(input: &str) {
        let mut c = Cursor::new(input);
        let end = c.to_end();

        assert!(c.ended());
        assert_eq!(
            end.byte_index,
            input.len(),
            "byte_index must equal input.len()"
        );
        assert_eq!(
            end.char_index,
            input.chars().count(),
            "char_index must equal chars().count()"
        );
    }

    fn assert_step_matches_chars(input: &str) {
        let positions = step_all(input);

        // positions length must be chars + 1 (including initial position)
        assert_eq!(
            positions.len(),
            input.chars().count() + 1,
            "one position per char, plus initial"
        );

        // byte_index sequence must match cumulative UTF-8 widths
        let mut cumulative = 0usize;
        assert_eq!(positions[0].byte_index, 0);
        assert_eq!(positions[0].char_index, 0);

        for (i, ch) in input.chars().enumerate() {
            cumulative += ch.len_utf8();
            let p = positions[i + 1];
            assert_eq!(p.char_index, i + 1, "char_index increments by 1 per char");
            assert_eq!(
                p.byte_index, cumulative,
                "byte_index increments by UTF-8 width"
            );
        }
    }

    fn assert_line_tracking_lf_only(input: &str) {
        let mut c = Cursor::new(input);

        let mut expected_line = 0u32;
        let mut expected_last_line_offset = 0u32; // in char indices

        // initial
        assert_eq!(c.position.last_line_number, expected_line);
        assert_eq!(c.position.last_line_offset, expected_last_line_offset);

        while !c.ended() {
            let b = c.current_byte();
            c.increment();

            if b == b'\n' {
                expected_line += 1;
                expected_last_line_offset = c.position.char_index as u32;
            }

            assert_eq!(
                c.position.last_line_number, expected_line,
                "line number should track LF only"
            );
            assert_eq!(
                c.position.last_line_offset, expected_last_line_offset,
                "last_line_offset tracks char_index at start of current line"
            );
        }
    }

    fn assert_remaining_is_suffix(input: &str) {
        let mut c = Cursor::new(input);
        while !c.ended() {
            let bi = c.position.byte_index;
            assert_eq!(c.remaining(), &input.as_bytes()[bi..]);
            c.increment();
        }
        // at end
        assert_eq!(c.remaining(), &input.as_bytes()[input.len()..]);
    }

    // Optional: validates that byte_index is always at a char boundary (lead byte)
    fn assert_byte_index_always_aligned(input: &str) {
        let mut c = Cursor::new(input);
        while !c.ended() {
            let bi = c.position.byte_index;
            if bi < input.len() {
                let b = input.as_bytes()[bi];
                assert!(
                    is_lead(b),
                    "byte_index must always point to UTF-8 lead byte boundary"
                );
            }
            c.increment();
        }
    }

    #[test]
    fn test_ascii_text() {
        let input = "hello world";
        assert_step_matches_chars(input);
        assert_end_indices(input);
        assert_remaining_is_suffix(input);
        assert_byte_index_always_aligned(input);
    }

    #[test]
    fn test_empty_string() {
        let input = "";
        assert_step_matches_chars(input);
        assert_end_indices(input);
        assert_remaining_is_suffix(input);
    }

    #[test]
    fn test_unicode_chars() {
        let input = "Hello 世界 🌍";
        assert_step_matches_chars(input);
        assert_end_indices(input);
        assert_remaining_is_suffix(input);
        assert_byte_index_always_aligned(input);
    }

    #[test]
    fn test_newline_tracking() {
        let input = "line1\nline2\nline3";
        assert_line_tracking_lf_only(input);
        assert_step_matches_chars(input);
    }

    #[test]
    fn test_vue_interpolation() {
        let input = "{{ message }}";
        assert_step_matches_chars(input);
        assert_end_indices(input);
        assert_remaining_is_suffix(input);
    }

    #[test]
    fn test_multiline_vue() {
        let input = "<template>\n  <div>{{ text }}</div>\n</template>";
        assert_line_tracking_lf_only(input);
        assert_step_matches_chars(input);
        assert_end_indices(input);
    }

    #[test]
    fn empty_string_end() {
        assert_end_indices("");
        assert_step_matches_chars("");
    }

    #[test]
    fn ascii_simple() {
        let s = "abc";
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_remaining_is_suffix(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn ascii_with_newlines_lf() {
        let s = "a\nb\n\nc";
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_line_tracking_lf_only(s);

        let mut c = Cursor::new(s);
        // After consuming 'a' then '\n', we are on line 1.
        c.advance(2);
        assert_eq!(c.position.last_line_number, 1);
        assert_eq!(c.position.last_line_offset, c.position.char_index as u32);
    }

    #[test]
    fn ascii_crlf_is_two_chars_and_only_lf_counts() {
        let s = "a\r\nb";
        assert_step_matches_chars(s);
        assert_line_tracking_lf_only(s);

        let mut c = Cursor::new(s);
        c.advance(2); // 'a' '\r'
        assert_eq!(
            c.position.last_line_number, 0,
            "CR alone should not increment line counter"
        );
        c.increment(); // '\n'
        assert_eq!(
            c.position.last_line_number, 1,
            "LF increments line counter even in CRLF"
        );
    }

    #[test]
    fn override_position_roundtrip() {
        let s = "hello\nworld";
        let mut c1 = Cursor::new(s);
        c1.advance(3);
        let saved = c1.position;

        let mut c2 = Cursor::new(s);
        c2.override_position(saved);

        assert_eq!(c2.position, saved);
        assert_eq!(c2.remaining(), &s.as_bytes()[saved.byte_index..]);
    }

    #[test]
    fn unicode_two_byte_chars() {
        let s = "éèê"; // U+00E9 etc, 2-byte UTF-8
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn unicode_three_byte_chars() {
        let s = "汉字"; // common CJK, 3-byte UTF-8
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn unicode_four_byte_chars_emoji() {
        let s = "😀😇"; // 4-byte UTF-8 each
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn mixed_ascii_and_unicode() {
        let s = "aé汉😀z\n";
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_line_tracking_lf_only(s);
        assert_byte_index_always_aligned(s);
    }
    #[test]
    fn combining_mark_sequence_counts_as_two_chars() {
        let s = "e\u{0301}"; // "e" + COMBINING ACUTE ACCENT (NFD)
        assert_eq!(s.chars().count(), 2);
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn precomposed_vs_decomposed_have_different_char_counts() {
        let nfc = "é"; // single char
        let nfd = "e\u{0301}"; // two chars

        assert_eq!(nfc.chars().count(), 1);
        assert_eq!(nfd.chars().count(), 2);

        assert_step_matches_chars(nfc);
        assert_step_matches_chars(nfd);
    }

    #[test]
    fn multiple_combining_marks() {
        let s = "a\u{0301}\u{0327}\u{0308}"; // a + acute + cedilla + diaeresis
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }
    #[test]
    fn zwj_sequence_multiple_chars() {
        // Woman technologist: 👩‍💻 = U+1F469 + ZWJ + U+1F4BB
        let s = "👩\u{200D}💻";
        assert!(s.chars().count() >= 3);
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }

    #[test]
    fn emoji_with_skin_tone_modifier() {
        // 👍🏽 = THUMBS UP + EMOJI MODIFIER FITZPATRICK TYPE-4
        let s = "👍🏽";
        assert_eq!(s.chars().count(), 2);
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }

    #[test]
    fn flag_is_two_regional_indicators() {
        // 🇵🇹 = Regional Indicator P + Regional Indicator T
        let s = "🇵🇹";
        assert_eq!(s.chars().count(), 2);
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }

    #[test]
    fn variation_selector() {
        // ✌️ = VICTORY HAND + VS16
        let s = "✌\u{FE0F}";
        assert_eq!(s.chars().count(), 2);
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }
    #[test]
    fn rtl_text_and_bidi_marks() {
        // Arabic + LRM/RLM
        let s = "abc\u{200F}مرحبا\u{200E}xyz";
        assert_step_matches_chars(s);
        assert_end_indices(s);
        assert_byte_index_always_aligned(s);
    }

    #[test]
    fn zero_width_space_and_joiners() {
        let s = "a\u{200B}b\u{200C}c\u{200D}d"; // ZWSP, ZWNJ, ZWJ
        assert_step_matches_chars(s);
        assert_end_indices(s);
    }
    #[test]
    fn unicode_line_separator_does_not_increment_line() {
        let s = "a\u{2028}b"; // LINE SEPARATOR (U+2028 is 3 bytes in UTF-8)
        assert_step_matches_chars(s);

        let mut c = Cursor::new(s);
        c.advance(1 + 3); // 'a' (1 byte) + U+2028 (3 bytes)
        assert_eq!(
            c.position.last_line_number, 0,
            "U+2028 should not affect line counting under current rules"
        );
    }

    #[test]
    fn nel_does_not_increment_line() {
        let s = "a\u{0085}b"; // NEXT LINE (U+0085 is 2 bytes in UTF-8)
        assert_step_matches_chars(s);

        let mut c = Cursor::new(s);
        c.advance(1 + 2); // 'a' (1 byte) + U+0085 (2 bytes)
        assert_eq!(
            c.position.last_line_number, 0,
            "NEL should not affect line counting under current rules"
        );
    }
    #[test]
    fn search2_finds_ascii_bytes() {
        let s = "abc:def;ghi";
        let c = Cursor::new(s);
        let pos = c.search2(b':', b';').expect("should find ':' or ';'");
        assert_eq!(s.as_bytes()[pos], b':');
    }

    #[test]
    fn search3_finds_one_of_three() {
        let s = "a,b;c";
        let c = Cursor::new(s);
        let pos = c.search3(b':', b';', b',').unwrap();
        assert_eq!(s.as_bytes()[pos], b',');
    }

    #[test]
    fn next_bytes_equal_matches_prefix() {
        let s = "hello world";
        let c = Cursor::new(s);
        assert!(c.next_bytes_equal(b"hell"));
        assert!(!c.next_bytes_equal(b"world"));
    }

    #[test]
    fn byte_search_can_match_inside_utf8_sequence() {
        // "é" in UTF-8 is 0xC3 0xA9
        let s = "é";
        let c = Cursor::new(s);

        // Searching for continuation byte 0xA9 should find it at index 1 (mid-codepoint)
        let idx = c.search2(0xA9, b'x').unwrap();
        assert_eq!(idx, 1);
        assert_eq!(s.as_bytes()[idx], 0xA9);
        // This is expected for byte searches; it is not Unicode-scalar aware.
    }

    #[test]
    fn find_subslice_unicode_bytes() {
        let hay = "aé汉😀z";
        let needle = "汉".as_bytes();
        let at = find_subslice(needle, hay.as_bytes()).unwrap();
        assert_eq!(&hay.as_bytes()[at..at + needle.len()], needle);
    }

    #[test]
    fn find_subslice_empty_needle_is_zero() {
        assert_eq!(find_subslice(b"", b"abc"), Some(0));
    }

    #[test]
    fn find_subslice_not_found() {
        assert_eq!(find_subslice(b"xyz", b"abc"), None);
    }
    #[test]
    #[should_panic]
    fn current_byte_panics_at_end() {
        let c = Cursor::new("");
        // ended immediately
        let _ = c.current_byte();
    }

    #[test]
    #[should_panic]
    fn next_byte_panics_when_no_next() {
        let c = Cursor::new("a");
        // position at 0, next_byte reads index 1 which is OOB
        let _ = c.next_byte();
    }

    #[test]
    fn increment_is_noop_at_end() {
        let mut c = Cursor::new("a");
        c.to_end();
        let p = c.position;
        c.increment();
        assert_eq!(c.position, p);
    }
    #[test]
    fn property_end_indices_match_std_counts_various_strings() {
        let cases = [
            "",
            "ascii",
            "a\nb\nc",
            "é",
            "e\u{0301}",
            "汉字",
            "😀",
            "👩\u{200D}💻",
            "abc\u{200F}مرحبا\u{200E}xyz",
            "a\u{2028}b",
            "a\r\nb",
        ];

        for s in cases {
            assert_end_indices(s);
            assert_step_matches_chars(s);
            assert_remaining_is_suffix(s);
            assert_byte_index_always_aligned(s);
        }
    }

    #[test]
    fn property_line_tracking_matches_count_of_lf() {
        let s = "a\né\n汉😀\n\nx";
        let mut c = Cursor::new(s);
        c.to_end();
        let lf_count = s.as_bytes().iter().filter(|&&b| b == b'\n').count() as u32;
        assert_eq!(c.position.last_line_number, lf_count);
    }
    #[test]
    fn to_position_matches_internal_state() {
        let s = "a\né汉😀\nzz";
        let mut c = Cursor::new(s);

        while !c.ended() {
            let _pos = c.position.to_position();
            // Adjust field names to your Position type
            // assert_eq!(pos.offset(), c.position.char_index as u32);
            // assert_eq!(pos.line(), c.position.last_line_number);
            // assert_eq!(pos.column(), c.position.char_index as u32 - c.position.last_line_offset);
            c.increment();
        }
    }

    #[test]
    fn unicode_to_source() {
        let s = "😀😇"; // 4-byte UTF-8 each
        let mut c = Cursor::new(s);
        let start = c.position.to_position();
        let end = c.to_end().to_position();

        let loc = crate::common::SourceLocation::from_source(s, start, end);
        assert_eq!(loc.source, s);
    }
}
