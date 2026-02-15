use memchr::memchr_iter;
use std::str;

use crate::common::SourceLocation;

// ============================================================================
// UTF-16 LENGTH CALCULATION
// ============================================================================

/// Compute UTF-16 code unit count for a valid UTF-8 string.
///
/// Uses an ASCII fast-path: if all bytes are ASCII, returns byte length directly.
/// For strings containing non-ASCII characters, falls back to byte-stepping.
#[inline]
pub fn utf16_len(slice: &str) -> usize {
    let bytes: &[u8] = slice.as_bytes();

    // Fast path: if all ASCII, length equals byte length
    if bytes.iter().all(|&b| b < 0x80) {
        return bytes.len();
    }

    // Slow path for non-ASCII: walk bytes with variable stride
    utf16_len_non_ascii(bytes)
}

/// Compute UTF-16 length for strings known to contain non-ASCII.
/// Walks byte-by-byte using UTF-8 lead byte patterns.
#[inline]
fn utf16_len_non_ascii(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            // ASCII: 1 byte -> 1 UTF-16 unit
            count += 1;
            i += 1;
        } else if b < 0xC0 {
            // Continuation byte (shouldn't be at sequence start in valid UTF-8)
            i += 1;
        } else if b < 0xE0 {
            // 2-byte sequence (U+0080 to U+07FF): 1 UTF-16 unit
            count += 1;
            i += 2;
        } else if b < 0xF0 {
            // 3-byte sequence (U+0800 to U+FFFF): 1 UTF-16 unit
            count += 1;
            i += 3;
        } else {
            // 4-byte sequence (U+10000+): 2 UTF-16 units (surrogate pair)
            count += 2;
            i += 4;
        }
    }

    count
}

/// Find all newline offsets using memchr with Bump allocator.
pub fn find_lines_memchr_bump_vec(input: &[u8]) -> Vec<usize> {
    let estimated_lines = input.len() / 40;
    let mut offsets = Vec::with_capacity(estimated_lines);
    offsets.extend(memchr_iter(b'\n', input));
    offsets
}

/// Compute UTF-16 cumulative offsets at the start of each line.
///
/// Returns a vector where `utf16_cache[i]` is the UTF-16 offset at the start of line `i`.
/// This enables efficient conversion from byte offsets to UTF-16 offsets.
///
/// The returned vector has `line_offsets.len() + 1` elements (one per line, since
/// N newlines = N+1 lines).
pub fn find_utf16_offset_from_vec(input: &str, line_offsets: &[usize]) -> Vec<usize> {
    let bytes: &[u8] = input.as_bytes();
    // N newlines = N+1 lines, so we need N+1 entries
    let num_lines = line_offsets.len() + 1;
    let mut utf16_cache = Vec::with_capacity(num_lines);
    // Line 0 always starts at UTF-16 offset 0
    utf16_cache.push(0);

    // Fast path: if all ASCII, UTF-16 offsets equal byte offsets
    if bytes.iter().all(|&b| b < 0x80) {
        for &newline_pos in line_offsets.iter() {
            // Next line starts at byte (newline_pos + 1), which equals UTF-16 offset for ASCII
            utf16_cache.push(newline_pos + 1);
        }
        return utf16_cache;
    }

    // Slow path: compute cumulative UTF-16 lengths
    let mut running_utf16: usize = 0;
    let mut prev_byte_pos: usize = 0;

    for &newline_pos in line_offsets.iter() {
        // Add UTF-16 length of segment from prev_byte_pos to newline_pos (inclusive)
        running_utf16 += utf16_len(&input[prev_byte_pos..=newline_pos]);
        utf16_cache.push(running_utf16);
        prev_byte_pos = newline_pos + 1;
    }

    utf16_cache
}

/// Resolves byte offsets to (line, column) positions with UTF-16 column counting.
pub struct PositionResolver<'a> {
    input: &'a str,
    line_offsets: Vec<usize>,
    utf16_cache: Vec<usize>,
}

impl<'a> PositionResolver<'a> {
    pub fn new(input: &'a str) -> Self {
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        // calculate utf16 cache if needed in future
        let utf16_cache = find_utf16_offset_from_vec(input, &line_offsets);

        Self {
            input,
            line_offsets,
            utf16_cache,
        }
    }

    /// Returns the 1-based line and UTF-16 column for the given byte offset.
    #[inline]
    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize, usize) {
        debug_assert!(offset <= self.input.len(), "offset out of bounds");
        debug_assert!(
            self.input.is_char_boundary(offset),
            "offset must land on a char boundary"
        );

        // Binary search returns the line index (0-based)
        // Ok = offset is exactly at a newline, Err = offset is within a line
        let line = self
            .line_offsets
            .binary_search(&offset)
            .unwrap_or_else(|i| i);

        // Line start is either 0 (first line) or byte after previous newline
        let line_start = if line == 0 {
            0
        } else {
            self.line_offsets[line - 1] + 1
        };

        // Calculate UTF-16 column for the slice from line start to offset
        let col_utf16 = utf16_len(&self.input[line_start..offset]);
        let offset_utf16 = self.utf16_cache.get(line).copied().unwrap_or(0) + col_utf16;

        (line + 1, col_utf16 + 1, offset_utf16)
    }

    pub fn to_source_location(&self, start: u32, end: u32) -> SourceLocation<'a> {
        let (start_line, start_col, start_offset_utf16) = self.offset_to_line_col(start as usize);
        let (end_line, end_col, end_offset_utf16) = self.offset_to_line_col(end as usize);

        SourceLocation {
            start: crate::common::Position {
                // TODO offset is not utf16 aware
                offset: start,
                offset_utf16: start_offset_utf16 as u32,
                line: start_line as u32,
                column: start_col as u32,
            },
            end: crate::common::Position {
                // TODO offset is not utf16 aware
                offset: end,
                offset_utf16: end_offset_utf16 as u32,
                line: end_line as u32,
                column: end_col as u32,
            },
            source: &self.input[start as usize..end as usize],
        }
    }
    pub fn to_position(&self, offset: u32) -> crate::common::Position {
        let (line, column, offset_utf16) = self.offset_to_line_col(offset as usize);
        crate::common::Position {
            offset,

            offset_utf16: offset_utf16 as u32,
            line: line as u32,
            column: column as u32,
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.input[start..end]
    }

    pub fn slice_bytes(&self, start: usize, end: usize) -> &'a [u8] {
        &self.input.as_bytes()[start..end]
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bumpalo::Bump;

    /// Reference implementation using std library
    fn utf16_len_reference(s: &str) -> usize {
        s.encode_utf16().count()
    }

    /// Test against reference implementation
    fn assert_utf16_len(input: &str) {
        let expected = utf16_len_reference(input);
        let actual = utf16_len(input);
        assert_eq!(
            actual,
            expected,
            "utf16_len mismatch for {:?} (len={}): got {}, expected {}",
            if input.len() > 50 {
                format!("{}...", &input[..50])
            } else {
                input.to_string()
            },
            input.len(),
            actual,
            expected
        );
    }

    // === Empty and basic ===

    #[test]
    fn test_empty() {
        assert_utf16_len("");
    }

    #[test]
    fn test_single_chars() {
        assert_utf16_len("a");
        assert_utf16_len("z");
        assert_utf16_len("0");
        assert_utf16_len(" ");
        assert_utf16_len("\n");
        assert_utf16_len("\t");
    }

    // === ASCII ===

    #[test]
    fn test_ascii_strings() {
        assert_utf16_len("hello");
        assert_utf16_len("Hello, World!");
        assert_utf16_len("0123456789");
        assert_utf16_len("function foo() { return 42; }");
        assert_utf16_len("const x = 'hello';");
    }

    #[test]
    fn test_ascii_various_lengths() {
        for len in [1, 2, 7, 8, 15, 16, 31, 32, 63, 64, 100, 500, 1000, 3000] {
            let s = "x".repeat(len);
            assert_utf16_len(&s);
        }
    }

    #[test]
    fn test_ascii_whitespace() {
        assert_utf16_len("   ");
        assert_utf16_len("\t\t\t");
        assert_utf16_len("\n\n\n");
        assert_utf16_len("  \t  \n  ");
    }

    // === 2-byte UTF-8 (U+0080 to U+07FF) ===

    #[test]
    fn test_latin_extended() {
        assert_utf16_len("é");
        assert_utf16_len("café");
        assert_utf16_len("über");
        assert_utf16_len("naïve");
        assert_utf16_len("résumé");
    }

    #[test]
    fn test_symbols_2byte() {
        assert_utf16_len("©");
        assert_utf16_len("®");
        assert_utf16_len("™");
        assert_utf16_len("©®™");
        assert_utf16_len("±×÷");
    }

    #[test]
    fn test_greek() {
        assert_utf16_len("α");
        assert_utf16_len("αβγδ");
        assert_utf16_len("Ωπ");
    }

    #[test]
    fn test_cyrillic() {
        assert_utf16_len("я");
        assert_utf16_len("привет");
        assert_utf16_len("Москва");
    }

    // === 3-byte UTF-8 (U+0800 to U+FFFF - BMP) ===

    #[test]
    fn test_cjk_chinese() {
        assert_utf16_len("中");
        assert_utf16_len("中文");
        assert_utf16_len("你好世界");
    }

    #[test]
    fn test_cjk_japanese() {
        assert_utf16_len("日");
        assert_utf16_len("日本語");
        assert_utf16_len("こんにちは");
        assert_utf16_len("カタカナ");
    }

    #[test]
    fn test_cjk_korean() {
        assert_utf16_len("한");
        assert_utf16_len("한글");
        assert_utf16_len("안녕하세요");
    }

    #[test]
    fn test_arabic() {
        assert_utf16_len("م");
        assert_utf16_len("العربية");
        assert_utf16_len("مرحبا");
    }

    #[test]
    fn test_hebrew() {
        assert_utf16_len("א");
        assert_utf16_len("עברית");
        assert_utf16_len("שלום");
    }

    #[test]
    fn test_thai() {
        assert_utf16_len("ก");
        assert_utf16_len("สวัสดี");
    }

    // === 4-byte UTF-8 (U+10000+ - surrogate pairs in UTF-16) ===

    #[test]
    fn test_emoji_simple() {
        assert_utf16_len("😀");
        assert_utf16_len("🎉");
        assert_utf16_len("🚀");
        assert_utf16_len("🔥");
        assert_utf16_len("💻");
    }

    #[test]
    fn test_emoji_multiple() {
        assert_utf16_len("😀😃😄😁😆");
        assert_utf16_len("🎉🎊🎁🎈");
    }

    #[test]
    fn test_emoji_zwj_sequences() {
        // Family: man + ZWJ + woman + ZWJ + girl + ZWJ + boy
        assert_utf16_len("👨‍👩‍👧‍👦");
        // Couple with heart
        assert_utf16_len("👩‍❤️‍👨");
    }

    #[test]
    fn test_emoji_flags() {
        assert_utf16_len("🇺🇸");
        assert_utf16_len("🇯🇵");
        assert_utf16_len("🇩🇪");
        assert_utf16_len("🇫🇷");
    }

    #[test]
    fn test_emoji_skin_tones() {
        assert_utf16_len("👋🏻");
        assert_utf16_len("👋🏿");
    }

    #[test]
    fn test_musical_symbols() {
        assert_utf16_len("𝄞"); // G clef U+1D11E
        assert_utf16_len("𝄢"); // F clef
        assert_utf16_len("𝄞𝄢");
    }

    #[test]
    fn test_mathematical_symbols() {
        assert_utf16_len("𝕒"); // Double-struck a
        assert_utf16_len("𝔸"); // Double-struck A
    }

    // === Mixed content ===

    #[test]
    fn test_mixed_ascii_and_2byte() {
        assert_utf16_len("cafe");
        assert_utf16_len("café");
        assert_utf16_len("hello café world");
    }

    #[test]
    fn test_mixed_ascii_and_3byte() {
        assert_utf16_len("hello 世界");
        assert_utf16_len("const 变量 = '值';");
        assert_utf16_len("// コメント");
    }

    #[test]
    fn test_mixed_ascii_and_4byte() {
        assert_utf16_len("a😀b");
        assert_utf16_len("hello 🎉 world");
        assert_utf16_len("console.log('🎉');");
    }

    #[test]
    fn test_mixed_all_types() {
        assert_utf16_len("café☕🍰");
        assert_utf16_len("Hello, 世界! 🎉");
        assert_utf16_len("über日本語😀");
    }

    // === Combining marks ===

    #[test]
    fn test_combining_marks() {
        // Decomposed é = e + combining acute accent
        assert_utf16_len("e\u{0301}");
        // Multiple combining marks
        assert_utf16_len("a\u{0300}\u{0301}\u{0302}");
        // Zalgo-style
        assert_utf16_len("h\u{0300}\u{0301}e\u{0302}l\u{0303}l\u{0304}o");
    }

    // === Edge cases for UTF-8 encoding boundaries ===

    #[test]
    fn test_encoding_boundaries() {
        assert_utf16_len("\x7F"); // Last ASCII (DEL)
        assert_utf16_len("\u{0080}"); // First 2-byte
        assert_utf16_len("\u{07FF}"); // Last 2-byte
        assert_utf16_len("\u{0800}"); // First 3-byte
        assert_utf16_len("\u{FFFF}"); // Last 3-byte (before surrogates needed)
        assert_utf16_len("\u{FFFD}"); // Replacement char
        assert_utf16_len("\u{10000}"); // First 4-byte (first surrogate pair)
        assert_utf16_len("\u{10FFFF}"); // Last valid Unicode
    }

    // === Realistic code patterns ===

    #[test]
    fn test_code_lines_typical() {
        assert_utf16_len("const result = await fetchData();");
        assert_utf16_len("    const result = await fetchData();");
        assert_utf16_len("        <div class=\"container\" v-if=\"visible\">");
        assert_utf16_len("export default defineComponent({");
        assert_utf16_len("    return { count, increment, decrement };");
    }

    #[test]
    fn test_code_lines_with_comments() {
        assert_utf16_len("// This is a comment");
        assert_utf16_len("    // This is a comment with émojis 🎉");
        assert_utf16_len("    /* 多语言注释 */");
        assert_utf16_len("/** @param {string} name - 名前 */");
    }

    #[test]
    fn test_code_lines_strings() {
        assert_utf16_len("const msg = \"Hello, 世界!\";");
        assert_utf16_len("const emoji = '🎉';");
        assert_utf16_len("const template = `Hello ${name} 👋`;");
    }

    #[test]
    fn test_long_lines() {
        // Long ASCII line
        let long_ascii = "x".repeat(3000);
        assert_utf16_len(&long_ascii);

        // Long mixed line
        let long_mixed = "const data = ".to_string() + &"日本語".repeat(500);
        assert_utf16_len(&long_mixed);

        // Long emoji line
        let long_emoji = "🎉".repeat(500);
        assert_utf16_len(&long_emoji);
    }

    // === UTF-16 count verification ===

    #[test]
    fn test_utf16_counts_explicit() {
        // ASCII: 1 byte = 1 UTF-16 unit
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len("abc"), 3);

        // 2-byte UTF-8: 1 UTF-16 unit
        assert_eq!(utf16_len("é"), 1);
        assert_eq!(utf16_len("éé"), 2);

        // 3-byte UTF-8: 1 UTF-16 unit
        assert_eq!(utf16_len("中"), 1);
        assert_eq!(utf16_len("中文"), 2);

        // 4-byte UTF-8: 2 UTF-16 units (surrogate pair)
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("😀😀"), 4);

        // Mixed
        assert_eq!(utf16_len("a😀"), 3); // 1 + 2
        assert_eq!(utf16_len("中😀"), 3); // 1 + 2
        assert_eq!(utf16_len("aé中😀"), 5); // 1 + 1 + 1 + 2
    }

    // === find_utf16_offset_from_vec tests ===

    #[test]
    fn test_utf16_offset_ascii_only() {
        let _bump = Bump::new();
        let input = "line1\nline2\nline3";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [5, 11] (newline positions)
        // utf16_offsets[i] = UTF-16 offset at start of line i
        // Line 0 starts at byte 0, line 1 at byte 6, line 2 at byte 12
        assert_eq!(utf16_offsets.len(), line_offsets.len() + 1);
        assert_eq!(utf16_offsets.as_slice(), &[0, 6, 12]);
    }

    #[test]
    fn test_utf16_offset_with_emoji() {
        let _bump = Bump::new();
        // "a😀b\ncd\ne"
        // Bytes: a(0), 😀(1-4), b(5), \n(6), c(7), d(8), \n(9), e(10)
        // UTF-16: a(1), 😀(2), b(1), \n(1), c(1), d(1), \n(1), e(1)
        let input = "a😀b\ncd\ne";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [6, 9]
        // utf16_offsets[0] = 0 (line 0 starts at UTF-16 offset 0)
        // utf16_offsets[1] = utf16_len("a😀b\n") = 1+2+1+1 = 5
        // utf16_offsets[2] = utf16_len("a😀b\ncd\n") = 5+3 = 8
        assert_eq!(utf16_offsets.len(), 3);
        assert_eq!(utf16_offsets.as_slice(), &[0, 5, 8]);
    }

    #[test]
    fn test_utf16_offset_with_cjk() {
        let _bump = Bump::new();
        // "你好\n世界"
        // Bytes: 你(0-2), 好(3-5), \n(6), 世(7-9), 界(10-12)
        // UTF-16: 你(1), 好(1), \n(1), 世(1), 界(1)
        let input = "你好\n世界";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [6]
        // utf16_offsets[0] = 0 (line 0 starts at UTF-16 offset 0)
        // utf16_offsets[1] = utf16_len("你好\n") = 3
        assert_eq!(utf16_offsets.len(), 2);
        assert_eq!(utf16_offsets.as_slice(), &[0, 3]);
    }

    #[test]
    fn test_utf16_offset_mixed_content() {
        let _bump = Bump::new();
        // "café\n🎉party"
        // Bytes: c(0), a(1), f(2), é(3-4), \n(5), 🎉(6-9), p(10)...
        // UTF-16: c(1), a(1), f(1), é(1), \n(1), 🎉(2), p(1)...
        let input = "café\n🎉party";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [5]
        // utf16_offsets[0] = 0
        // utf16_offsets[1] = utf16_len("café\n") = 5
        assert_eq!(utf16_offsets.len(), 2);
        assert_eq!(utf16_offsets.as_slice(), &[0, 5]);
    }

    #[test]
    fn test_utf16_offset_empty_lines() {
        let _bump = Bump::new();
        let input = "a\n\nb";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [1, 2]
        // utf16_offsets[0] = 0 (line 0 starts at UTF-16 offset 0)
        // utf16_offsets[1] = utf16_len("a\n") = 2
        // utf16_offsets[2] = utf16_len("a\n\n") = 3
        assert_eq!(utf16_offsets.len(), 3);
        assert_eq!(utf16_offsets.as_slice(), &[0, 2, 3]);
    }

    #[test]
    fn test_utf16_offset_no_newlines() {
        let _bump = Bump::new();
        let input = "hello world";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // No newlines = 1 line, so we need 1 entry (for line 0)
        assert_eq!(utf16_offsets.len(), 1);
        assert_eq!(utf16_offsets[0], 0);
    }

    #[test]
    fn test_utf16_offset_single_emoji_line() {
        let _bump = Bump::new();
        let input = "😀😀😀\n";
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_offsets = find_utf16_offset_from_vec(input, &line_offsets);

        // line_offsets = [12] (newline at byte 12)
        // utf16_offsets[0] = 0
        // utf16_offsets[1] = utf16_len("😀😀😀\n") = 2+2+2+1 = 7
        assert_eq!(utf16_offsets.len(), 2);
        assert_eq!(utf16_offsets.as_slice(), &[0, 7]);
    }

    // === PositionResolver tests ===

    #[test]
    fn test_position_resolver_ascii() {
        let _bump = Bump::new();
        let input = "line1\nline2\nline3";
        let resolver = PositionResolver::new(input);

        // offset_to_line_col returns (line, column, utf16_offset)
        assert_eq!(resolver.offset_to_line_col(0), (1, 1, 0)); // 'l' in line1
        assert_eq!(resolver.offset_to_line_col(4), (1, 5, 4)); // '1' in line1
        assert_eq!(resolver.offset_to_line_col(5), (1, 6, 5)); // '\n' after line1
        assert_eq!(resolver.offset_to_line_col(6), (2, 1, 6)); // 'l' in line2
        assert_eq!(resolver.offset_to_line_col(11), (2, 6, 11)); // '\n' after line2
        assert_eq!(resolver.offset_to_line_col(12), (3, 1, 12)); // 'l' in line3
        assert_eq!(resolver.offset_to_line_col(16), (3, 5, 16)); // '3' in line3
    }

    #[test]
    fn test_position_resolver_utf16() {
        let _bump = Bump::new();
        let input = "a😊b\nc🧪d";
        let resolver = PositionResolver::new(input);

        // Line 1: a(1 byte, 1 utf16) + 😊(4 bytes, 2 utf16) + b(1 byte, 1 utf16) + \n(1 byte, 1 utf16)
        // Byte offsets: a=0, 😊=1-4, b=5, \n=6
        // UTF16 offsets: a=0, 😊=1, b=3, \n=4
        let (line, col, utf16_off) = resolver.offset_to_line_col(0);
        assert_eq!((line, col), (1, 1)); // 'a' col 1
        assert_eq!(utf16_off, 0);

        let (line, col, utf16_off) = resolver.offset_to_line_col(1);
        assert_eq!((line, col), (1, 2)); // '😊' col 2
        assert_eq!(utf16_off, 1);

        let (line, col, utf16_off) = resolver.offset_to_line_col(5);
        assert_eq!((line, col), (1, 4)); // 'b' col 4 (after surrogate pair)
        assert_eq!(utf16_off, 3);

        // Line 2: c(1) + 🧪(4 bytes, 2 utf16) + d(1)
        // Byte offsets: c=7, 🧪=8-11, d=12
        // UTF16 line start offset = 5 (from "a😊b\n")
        let (line, col, utf16_off) = resolver.offset_to_line_col(7);
        assert_eq!((line, col), (2, 1)); // 'c' col 1
        assert_eq!(utf16_off, 5);

        let (line, col, utf16_off) = resolver.offset_to_line_col(8);
        assert_eq!((line, col), (2, 2)); // '🧪' col 2
        assert_eq!(utf16_off, 6);

        let (line, col, utf16_off) = resolver.offset_to_line_col(12);
        assert_eq!((line, col), (2, 4)); // 'd' col 4
        assert_eq!(utf16_off, 8);
    }

    #[test]
    fn test_position_resolver_cjk() {
        let _bump = Bump::new();
        let input = "你好\n世界";
        let resolver = PositionResolver::new(input);

        // Line 1: 你(3 bytes, 1 utf16) + 好(3 bytes, 1 utf16) + \n(1 byte, 1 utf16)
        let (line, col, utf16_off) = resolver.offset_to_line_col(0);
        assert_eq!((line, col), (1, 1)); // '你'
        assert_eq!(utf16_off, 0);

        let (line, col, utf16_off) = resolver.offset_to_line_col(3);
        assert_eq!((line, col), (1, 2)); // '好'
        assert_eq!(utf16_off, 1);

        let (line, col, utf16_off) = resolver.offset_to_line_col(6);
        assert_eq!((line, col), (1, 3)); // '\n'
        assert_eq!(utf16_off, 2);

        // Line 2: 世(3 bytes) + 界(3 bytes)
        // UTF16 line start offset = 3 (from "你好\n")
        let (line, col, utf16_off) = resolver.offset_to_line_col(7);
        assert_eq!((line, col), (2, 1)); // '世'
        assert_eq!(utf16_off, 3);

        let (line, col, utf16_off) = resolver.offset_to_line_col(10);
        assert_eq!((line, col), (2, 2)); // '界'
        assert_eq!(utf16_off, 4);
    }

    #[test]
    fn test_position_resolver_empty_lines() {
        let _bump = Bump::new();
        let input = "a\n\nb";
        let resolver = PositionResolver::new(input);

        let (line, col, utf16_off) = resolver.offset_to_line_col(0);
        assert_eq!((line, col), (1, 1)); // 'a'
        assert_eq!(utf16_off, 0);

        let (line, col, utf16_off) = resolver.offset_to_line_col(1);
        assert_eq!((line, col), (1, 2)); // first '\n'
        assert_eq!(utf16_off, 1);

        let (line, col, utf16_off) = resolver.offset_to_line_col(2);
        assert_eq!((line, col), (2, 1)); // second '\n' (empty line)
        assert_eq!(utf16_off, 2);

        let (line, col, utf16_off) = resolver.offset_to_line_col(3);
        assert_eq!((line, col), (3, 1)); // 'b'
        assert_eq!(utf16_off, 3);
    }
}

// ============================================================================
// PROPERTY-BASED TESTS
// ============================================================================

#[cfg(test)]
mod proptests {
    use super::*;

    /// Reference implementation
    fn reference(s: &str) -> usize {
        s.encode_utf16().count()
    }

    #[test]
    fn proptest_random_strings() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let char_pools: &[&[char]] = &[
            &['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j'], // ASCII
            &['é', 'è', 'ê', 'ë', 'ñ', 'ü', 'ö', 'ä'],           // 2-byte
            &['中', '文', '日', '本', '語', '한', '글'],         // 3-byte
            &['😀', '😃', '🎉', '🚀', '🔥', '💻', '🧪'],         // 4-byte
        ];

        for seed in 0..1000 {
            let mut hasher = DefaultHasher::new();
            seed.hash(&mut hasher);
            let mut state = hasher.finish();

            let len = (state % 501) as usize;
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);

            let mut s = String::new();
            for _ in 0..len {
                let pool_idx = (state % 4) as usize;
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);

                let pool = char_pools[pool_idx];
                let char_idx = (state % pool.len() as u64) as usize;
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);

                s.push(pool[char_idx]);
            }

            let expected = reference(&s);
            let actual = utf16_len(&s);
            assert_eq!(
                actual,
                expected,
                "Mismatch for seed {}: len={}, expected={}, got={}",
                seed,
                s.len(),
                expected,
                actual
            );
        }
    }

    #[test]
    fn proptest_ascii_only() {
        for len in 0..200 {
            let s: String = (0..len).map(|i| ((i % 95) as u8 + 32) as char).collect();
            assert_eq!(utf16_len(&s), reference(&s));
        }
    }

    #[test]
    fn proptest_all_emoji() {
        let emojis = ['😀', '😃', '😄', '😁', '🎉', '🚀', '🔥', '💻', '🧪', '🌍'];

        for len in 0..100 {
            let s: String = (0..len).map(|i| emojis[i % emojis.len()]).collect();
            assert_eq!(utf16_len(&s), reference(&s));
        }
    }

    #[test]
    fn proptest_all_cjk() {
        let chars = ['中', '文', '日', '本', '語', '한', '글', '你', '好', '世'];

        for len in 0..100 {
            let s: String = (0..len).map(|i| chars[i % chars.len()]).collect();
            assert_eq!(utf16_len(&s), reference(&s));
        }
    }

    #[test]
    fn proptest_alternating_ascii_emoji() {
        for len in 0..100 {
            let s: String = (0..len)
                .map(|i| if i % 2 == 0 { 'a' } else { '😀' })
                .collect();
            assert_eq!(utf16_len(&s), reference(&s));
        }
    }

    #[test]
    fn proptest_alternating_ascii_cjk() {
        for len in 0..100 {
            let s: String = (0..len)
                .map(|i| if i % 2 == 0 { 'x' } else { '中' })
                .collect();
            assert_eq!(utf16_len(&s), reference(&s));
        }
    }

    #[test]
    fn proptest_repeating_patterns() {
        let patterns = ["a", "ab", "abc", "é", "中", "😀", "a😀", "中😀é"];

        for pattern in patterns {
            for repeat in 1..50 {
                let s = pattern.repeat(repeat);
                assert_eq!(
                    utf16_len(&s),
                    reference(&s),
                    "pattern={:?} repeat={}",
                    pattern,
                    repeat
                );
            }
        }
    }
}
