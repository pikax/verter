use memchr::memchr_iter;

use super::code_transform::CodeTransform;
use crate::cursor::position::{utf16_len, PositionResolver};
use oxc_sourcemap::{SourceMap, SourceMapBuilder};

/// Options for source map generation
#[derive(Debug, Clone)]
pub struct SourceMapOptions<'a> {
    /// The filename of the source file
    pub source: Option<&'a str>,
    /// The filename of the generated file
    pub file: Option<&'a str>,
    /// Whether to include the source content in the map
    pub include_content: bool,
}

impl Default for SourceMapOptions<'_> {
    fn default() -> Self {
        Self {
            source: None,
            file: None,
            include_content: true,
        }
    }
}

#[allow(dead_code)] // Builder API used in tests
impl<'a> SourceMapOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_source(mut self, source: &'a str) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_file(mut self, file: &'a str) -> Self {
        self.file = Some(file);
        self
    }

    pub fn include_content(mut self, include: bool) -> Self {
        self.include_content = include;
        self
    }
}

impl<'a> CodeTransform<'a> {
    /// Generate a source map for the transformations
    ///
    /// # Example
    /// ```
    /// use verter_core::code_transform::{CodeTransform, SourceMapOptions};
    /// use oxc_allocator::Allocator;
    ///
    /// let allocator = Allocator::default();
    /// let mut ct = CodeTransform::new("Hello World", &allocator);
    /// ct.overwrite(6, 11, "Rust");
    ///
    /// let options = SourceMapOptions::new()
    ///     .with_source("input.js")
    ///     .with_file("output.js");
    ///
    /// let source_map = ct.generate_map(options);
    /// ```
    #[must_use]
    #[allow(unused_assignments)] // generated_line/column updated in outro but intentionally not read after
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn generate_map(&self, options: SourceMapOptions) -> SourceMap {
        let mut builder = SourceMapBuilder::default();

        // Set up source file
        let source_id = if let Some(source) = options.source {
            let id = if options.include_content {
                builder.set_source_and_content(source, self.original())
            } else {
                builder.set_source_and_content(source, "")
            };
            Some(id)
        } else {
            None
        };

        // Set output file name
        if let Some(file) = options.file {
            builder.set_file(file);
        }

        // Build position resolver for O(log N) byte-offset → (line, UTF-16 column) lookups.
        // Uses the sourcemap-optimized constructor that skips the UTF-16 cumulative offset
        // cache (not needed here — we only use line and column).
        let resolver = PositionResolver::new_for_sourcemap(self.original());

        let mut generated_line = 0u32;
        let mut generated_column = 0u32;

        // Add intro (no source mapping for inserted content)
        if !self.intro().is_empty() {
            builder.add_token(generated_line, generated_column, 0, 0, None, None);
            Self::advance_generated_position(
                self.intro(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        let is_ascii = self.is_ascii();

        // Process chunks
        for chunk in self.chunks() {
            use super::chunk::Chunk;

            match chunk {
                Chunk::Original { start, end } => {
                    if let Some(source_id) = source_id {
                        let slice = &self.original()[*start as usize..*end as usize];

                        Self::emit_mapped_content(
                            &mut builder,
                            slice,
                            source_id,
                            &resolver,
                            *start,
                            &mut generated_line,
                            &mut generated_column,
                            is_ascii,
                        );
                    }
                }
                Chunk::Moved {
                    start: orig_start,
                    content,
                    ..
                } => {
                    if content.is_empty() {
                        continue;
                    }
                    // Moved content — line-by-line mappings like Original chunks
                    if let Some(source_id) = source_id {
                        Self::emit_mapped_content(
                            &mut builder,
                            content,
                            source_id,
                            &resolver,
                            *orig_start,
                            &mut generated_line,
                            &mut generated_column,
                            is_ascii,
                        );
                    }
                }
                Chunk::Overwritten {
                    start: orig_start,
                    content,
                    ..
                } => {
                    if content.is_empty() {
                        continue;
                    }
                    // Overwritten content — emit a single token at the original start
                    // position. Unlike Original/Moved chunks, there is no character-level
                    // correspondence between replacement content and the source, so
                    // per-line tokens would be misleading. This matches MagicString behavior.
                    if let Some(source_id) = source_id {
                        let (src_line_1, src_col_1) =
                            resolver.offset_to_line_and_col(*orig_start as usize);
                        let source_line = (src_line_1 - 1) as u32;
                        let source_column = (src_col_1 - 1) as u32;

                        builder.add_token(
                            generated_line,
                            generated_column,
                            source_line,
                            source_column,
                            Some(source_id),
                            None,
                        );
                    }

                    Self::advance_generated_position(
                        content,
                        &mut generated_line,
                        &mut generated_column,
                    );
                }
                Chunk::Inserted { content } => {
                    if content.is_empty() {
                        continue;
                    }
                    // Pure insertion — unmapped
                    builder.add_token(generated_line, generated_column, 0, 0, None, None);

                    Self::advance_generated_position(
                        content,
                        &mut generated_line,
                        &mut generated_column,
                    );
                }
                Chunk::InsertedMapped {
                    content,
                    source_start,
                    content_offset,
                } => {
                    if content.is_empty() {
                        continue;
                    }
                    // Inserted content mapped to a specific source position.
                    // `content_offset` shifts the source map token within the content:
                    // characters before `content_offset` are unmapped (e.g., `(__props.`),
                    // then the token is emitted at `content_offset` pointing to `source_start`.
                    let offset = (*content_offset as usize).min(content.len());
                    if offset > 0 {
                        // Unmapped prefix (e.g., the `(` and binding prefix)
                        let prefix = &content[..offset];
                        builder.add_token(generated_line, generated_column, 0, 0, None, None);
                        Self::advance_generated_position(
                            prefix,
                            &mut generated_line,
                            &mut generated_column,
                        );
                    }
                    // Mapped token at content_offset → source_start
                    let rest = &content[offset..];
                    if !rest.is_empty() {
                        if let Some(source_id) = source_id {
                            let (sl, sc) = resolver.offset_to_line_and_col(*source_start as usize);
                            builder.add_token(
                                generated_line,
                                generated_column,
                                (sl - 1) as u32,
                                (sc - 1) as u32,
                                Some(source_id),
                                None,
                            );
                        }
                        Self::advance_generated_position(
                            rest,
                            &mut generated_line,
                            &mut generated_column,
                        );
                    }
                }
            }
        }

        // Add outro (no source mapping)
        if !self.outro().is_empty() {
            builder.add_token(generated_line, generated_column, 0, 0, None, None);
            Self::advance_generated_position(
                self.outro(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        builder.into_sourcemap()
    }

    /// Emit line-by-line source map tokens for content that maps back to original source.
    /// Used for both Original chunks and moved Edited chunks.
    ///
    /// Scans `content` for newlines, emitting a token at the start and after each newline.
    /// Source positions are resolved via `PositionResolver` (UTF-16 aware).
    /// Generated column advances use the resolver's column difference since the content
    /// is always from the original source (either in-place or moved).
    #[allow(clippy::too_many_arguments)]
    fn emit_mapped_content(
        builder: &mut SourceMapBuilder,
        content: &str,
        source_id: u32,
        resolver: &PositionResolver,
        original_start: u32,
        generated_line: &mut u32,
        generated_column: &mut u32,
        is_ascii: bool,
    ) {
        let content_bytes = content.as_bytes();
        let content_len = content_bytes.len();

        // Single resolver lookup for the initial source position (O(log N) once per chunk)
        let (sl, sc) = resolver.offset_to_line_and_col(original_start as usize);
        let mut source_line = (sl - 1) as u32;

        builder.add_token(
            *generated_line,
            *generated_column,
            source_line,
            (sc - 1) as u32,
            Some(source_id),
            None,
        );

        // Scan for newlines — O(1) manual tracking per newline (no binary search)
        let mut prev = 0usize;

        for nl_pos in memchr_iter(b'\n', content_bytes) {
            *generated_line += 1;
            *generated_column = 0;
            source_line += 1;
            prev = nl_pos + 1;

            // After a newline, source column is always 0
            if prev < content_len {
                builder.add_token(
                    *generated_line,
                    *generated_column,
                    source_line,
                    0,
                    Some(source_id),
                    None,
                );
            }
        }

        // Advance generated_column for remaining content after last newline.
        // For ASCII sources, byte length == UTF-16 length, so skip utf16_len().
        let remaining = &content[prev..];
        *generated_column += if is_ascii {
            remaining.len() as u32
        } else {
            utf16_len(remaining) as u32
        };
    }

    /// Advance generated line/column position through a string using memchr.
    /// Counts columns in UTF-16 code units for correct source map positions.
    #[inline]
    fn advance_generated_position(content: &str, line: &mut u32, column: &mut u32) {
        let bytes = content.as_bytes();
        let mut prev = 0usize;
        for nl_pos in memchr_iter(b'\n', bytes) {
            *line += 1;
            prev = nl_pos + 1;
        }
        if prev == 0 {
            // No newlines at all — advance column by UTF-16 length
            *column += utf16_len(content) as u32;
        } else {
            // Column is UTF-16 length from last newline to end
            *column = utf16_len(&content[prev..]) as u32;
        }
    }

    /// Generate source map and return as JSON string
    #[must_use]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn generate_map_json(&self, options: SourceMapOptions) -> String {
        let map = self.generate_map(options);
        map.to_json_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    #[test]
    fn test_source_map_generation() {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("Hello World", &allocator);
        ct.overwrite(6, 11, "Rust");

        let options = SourceMapOptions::new()
            .with_source("input.js")
            .with_file("output.js");

        let map = ct.generate_map(options);

        // The source map should be valid
        let sources: Vec<_> = map.get_sources().collect();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].as_ref(), "input.js");
    }

    #[test]
    fn test_source_map_with_content() {
        let allocator = Allocator::default();
        let source = "const x = 1;\nconst y = 2;";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(6, 7, "foo");

        let options = SourceMapOptions::new()
            .with_source("test.js")
            .include_content(true);

        let map = ct.generate_map(options);

        // Should include source content
        let content = map.get_source_content(0);
        assert!(content.is_some());
        assert_eq!(content.unwrap().as_ref(), source);
    }

    /// Verify PositionResolver-based line/column calculation (0-indexed for source maps)
    #[test]
    fn test_line_column_calculation_via_resolver() {
        let source = "Hello\nWorld\nTest";
        let resolver = PositionResolver::new(source);

        // PositionResolver returns 1-indexed; source maps use 0-indexed
        let to_0 = |offset: usize| {
            let (line, col, _) = resolver.offset_to_line_col(offset);
            ((line - 1) as u32, (col - 1) as u32)
        };

        assert_eq!(to_0(0), (0, 0)); // H
        assert_eq!(to_0(5), (0, 5)); // \n
        assert_eq!(to_0(6), (1, 0)); // W
        assert_eq!(to_0(12), (2, 0)); // T
    }

    /// @ai-generated — Verify PositionResolver edge cases
    #[test]
    fn test_line_column_edge_cases_via_resolver() {
        // Single line
        let resolver = PositionResolver::new("Hello");
        let (line, col, _) = resolver.offset_to_line_col(0);
        assert_eq!((line - 1, col - 1), (0, 0));
        let (line, col, _) = resolver.offset_to_line_col(4);
        assert_eq!((line - 1, col - 1), (0, 4));

        // Two lines: "abc\ndef"
        let resolver = PositionResolver::new("abc\ndef");
        let (line, col, _) = resolver.offset_to_line_col(0);
        assert_eq!((line - 1, col - 1), (0, 0));
        let (line, col, _) = resolver.offset_to_line_col(3);
        assert_eq!((line - 1, col - 1), (0, 3));
        let (line, col, _) = resolver.offset_to_line_col(4);
        assert_eq!((line - 1, col - 1), (1, 0));
        let (line, col, _) = resolver.offset_to_line_col(6);
        assert_eq!((line - 1, col - 1), (1, 2));

        // "a\nb\nc"
        let resolver = PositionResolver::new("a\nb\nc");
        let (line, col, _) = resolver.offset_to_line_col(2);
        assert_eq!((line - 1, col - 1), (1, 0));
        let (line, col, _) = resolver.offset_to_line_col(4);
        assert_eq!((line - 1, col - 1), (2, 0));
    }

    // ========================================================================
    // TDD: Mapping accuracy tests — verify actual token positions
    // ========================================================================

    /// @ai-generated — Verify exact token positions for a simple overwrite
    #[test]
    fn test_source_map_token_positions_simple_overwrite() {
        let allocator = Allocator::default();
        // Source: "abc\ndef\nghi"
        //          012 3 456 7 890
        // Overwrite "def" (bytes 4-7) with "XYZ"
        // Output: "abc\nXYZ\nghi"
        let source = "abc\ndef\nghi";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(4, 7, "XYZ");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Token 0: start of "abc" — gen(0,0) → src(0,0)
        assert_eq!(tokens[0].get_dst_line(), 0);
        assert_eq!(tokens[0].get_dst_col(), 0);
        assert_eq!(tokens[0].get_src_line(), 0);
        assert_eq!(tokens[0].get_src_col(), 0);

        // Find token mapping "XYZ" — should be gen(1,0) → src(1,0)
        let xyz_token = tokens
            .iter()
            .find(|t| t.get_dst_line() == 1 && t.get_dst_col() == 0 && t.get_source_id().is_some())
            .expect("should have a token at generated line 1, col 0");
        assert_eq!(xyz_token.get_src_line(), 1);
        assert_eq!(xyz_token.get_src_col(), 0);

        // Find token mapping "ghi" — should be gen(2,0) → src(2,0)
        let ghi_token = tokens
            .iter()
            .find(|t| t.get_dst_line() == 2 && t.get_dst_col() == 0 && t.get_source_id().is_some())
            .expect("should have a token at generated line 2, col 0");
        assert_eq!(ghi_token.get_src_line(), 2);
        assert_eq!(ghi_token.get_src_col(), 0);
    }

    /// @ai-generated — Verify prepend shifts generated line positions
    #[test]
    fn test_source_map_token_positions_with_prepend() {
        let allocator = Allocator::default();
        let source = "abc\ndef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.prepend("// header\n");

        // Output: "// header\nabc\ndef"
        // The "abc" chunk maps to src(0,0) but should be at gen(1,0)
        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Find the token for "abc" — generated line should be 1 (after "// header\n")
        let abc_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have a token mapping to src(0,0)");
        assert_eq!(
            abc_token.get_dst_line(),
            1,
            "abc should be on generated line 1 after header"
        );
        assert_eq!(abc_token.get_dst_col(), 0);
    }

    /// @ai-generated — Verify moved multiline content maps back to original positions
    #[test]
    fn test_source_map_token_positions_moved_multiline() {
        let allocator = Allocator::default();
        // Source: "line1\nline2\nline3\nline4"
        //          01234 5 67890 1 23456 7 89012
        // Move "line2\nline3\n" (6-18) to beginning with wrapping
        let source = "line1\nline2\nline3\nline4";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.move_wrapped(6, 18, 0, "/*s*/", "/*e*/");
        // Output: "/*s*/line2\nline3\n/*e*/line1\nline4"

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The moved "line2" should map back to original src line 1
        // It's at generated position after "/*s*/" (5 chars), so gen(0, 5)
        let line2_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 1 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have a token mapping to src(1,0) for moved line2");
        assert_eq!(
            line2_token.get_dst_line(),
            0,
            "moved line2 should be on generated line 0"
        );
        assert_eq!(
            line2_token.get_dst_col(),
            5,
            "moved line2 should start at column 5 after /*s*/"
        );

        // The moved "line3" should map back to original src line 2
        let line3_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 2 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have a token mapping to src(2,0) for moved line3");
        assert_eq!(
            line3_token.get_dst_line(),
            1,
            "moved line3 should be on generated line 1"
        );
    }

    // ========================================================================
    // TDD: UTF-16 column accuracy tests — these should FAIL before the fix
    // ========================================================================

    /// @ai-generated — CJK characters: 3 bytes each, 1 UTF-16 unit each
    /// Source: "abc中文\ndef"
    ///   bytes: a(0) b(1) c(2) 中(3-5) 文(6-8) \n(9) d(10) e(11) f(12)
    ///   UTF-16 cols on line 0: a=0, b=1, c=2, 中=3, 文=4, \n=5
    /// After overwrite of "中" (bytes 3-6) with "X":
    ///   Output: "abcX文\ndef"
    ///   The "文" source col should be 4 (UTF-16), not 6 (bytes)
    #[test]
    fn test_source_map_utf16_column_for_cjk() {
        let allocator = Allocator::default();
        let source = "abc中文\ndef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(3, 6, "X"); // Replace "中" with "X"

        // Output: "abcX文\ndef"
        assert_eq!(ct.build_string(), "abcX文\ndef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Token for "abc" — src(0, 0)
        let abc_token = tokens
            .iter()
            .find(|t| t.get_dst_line() == 0 && t.get_dst_col() == 0 && t.get_source_id().is_some())
            .expect("should have token at gen(0,0)");
        assert_eq!(abc_token.get_src_col(), 0);

        // Token for "X" overwrite — should map to src(0, 3) (UTF-16 column of "中")
        // "中" starts at byte 3, which in UTF-16 columns is column 3 (a=0, b=1, c=2, 中=3)
        let x_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 3 && t.get_source_id().is_some())
            .expect("overwrite of 中 should map to src col 3 (UTF-16)");
        assert_eq!(x_token.get_dst_col(), 3, "generated col for X should be 3");

        // Token for "文" (remaining original) — should map to src(0, 4) in UTF-16
        // "文" starts at byte 6, but UTF-16 column should be 4 (after a, b, c, 中)
        let wen_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some());
        assert!(
            wen_token.is_some(),
            "文 should map to src col 4 (UTF-16), not 6 (bytes). Tokens: {:?}",
            tokens
                .iter()
                .filter(|t| t.get_src_line() == 0)
                .map(|t| (t.get_src_col(), t.get_dst_col()))
                .collect::<Vec<_>>()
        );
    }

    /// @ai-generated — Emoji: 4 bytes each, 2 UTF-16 units each
    /// Source: "a😀b\ncd"
    ///   bytes: a(0) 😀(1-4) b(5) \n(6) c(7) d(8)
    ///   UTF-16 cols on line 0: a=0, 😀=1(+2 units), b=3, \n=4
    /// The source column of 'b' should be 3 (UTF-16), not 5 (bytes)
    #[test]
    fn test_source_map_utf16_column_for_emoji() {
        let allocator = Allocator::default();
        let source = "a😀b\ncd";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(5, 6, "B"); // Replace 'b' with 'B'

        // Output: "a😀B\ncd"
        assert_eq!(ct.build_string(), "a😀B\ncd");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Token for overwrite of 'b' → should map to src(0, 3) in UTF-16
        // Byte offset 5 = after a(1 byte) + 😀(4 bytes) = col 3 in UTF-16 (a=0, 😀=1-2, b=3)
        let b_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 3 && t.get_source_id().is_some());
        assert!(
            b_token.is_some(),
            "b should map to src col 3 (UTF-16), not 5 (bytes). Tokens on line 0: {:?}",
            tokens
                .iter()
                .filter(|t| t.get_src_line() == 0)
                .map(|t| (t.get_src_col(), t.get_dst_col()))
                .collect::<Vec<_>>()
        );
    }

    /// @ai-generated — Verify generated columns use UTF-16 counting too
    /// Overwrite ASCII with CJK content, check generated column of next chunk
    #[test]
    fn test_source_map_generated_column_utf16() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(2, 4, "中文"); // Replace "cd" with "中文" (2 chars, 6 bytes)

        // Output: "ab中文ef"
        assert_eq!(ct.build_string(), "ab中文ef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Token for "ef" — should be at generated col 4 (UTF-16: a=0, b=1, 中=2, 文=3, e=4)
        // NOT at generated col 8 (bytes: a=0, b=1, 中=2-4, 文=5-7, e=8)
        let ef_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some())
            .expect("should have token for original 'ef' at src col 4");
        assert_eq!(
            ef_token.get_dst_col(),
            4,
            "generated col for 'ef' should be 4 (UTF-16), not 8 (bytes)"
        );
    }

    // ========================================================================
    // TDD: Coverage gap tests — verify existing behavior for untested paths
    // ========================================================================

    /// @ai-generated — include_content(false) should store empty string
    #[test]
    fn test_source_map_include_content_false() {
        let allocator = Allocator::default();
        let source = "const x = 1;";
        let ct = CodeTransform::new(source, &allocator);

        let map = ct.generate_map(
            SourceMapOptions::new()
                .with_source("test.js")
                .include_content(false),
        );

        let content = map.get_source_content(0);
        assert!(content.is_some(), "source content entry should exist");
        assert_eq!(
            content.unwrap().as_ref(),
            "",
            "content should be empty string when include_content is false"
        );
    }

    /// @ai-generated — source: None should produce tokens with no source_id
    #[test]
    fn test_source_map_no_source_option() {
        let allocator = Allocator::default();
        let source = "abc\ndef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(0, 3, "XYZ");

        let map = ct.generate_map(SourceMapOptions::new()); // No source set

        let tokens: Vec<_> = map.get_tokens().collect();
        // All tokens should have no source_id
        for token in &tokens {
            assert!(
                token.get_source_id().is_none(),
                "token should have no source_id when source option is None"
            );
        }
    }

    /// @ai-generated — Overwrite content that introduces newlines
    #[test]
    fn test_source_map_multiline_overwrite_content() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(2, 4, "X\nY"); // Replace "cd" with multiline content

        // Output: "abX\nYef"
        assert_eq!(ct.build_string(), "abX\nYef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Token for "ef" should be on generated line 1 (after the newline in overwrite)
        let ef_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 4 && t.get_source_id().is_some())
            .expect("should have token for 'ef' at src(0,4)");
        assert_eq!(
            ef_token.get_dst_line(),
            1,
            "ef should be on generated line 1 after multiline overwrite"
        );
        assert_eq!(
            ef_token.get_dst_col(),
            1,
            "ef should be at generated col 1 after 'Y'"
        );
    }

    /// @ai-generated — Outro should produce unmapped token
    #[test]
    fn test_source_map_with_outro() {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new("abc", &allocator);
        ct.append("\n// footer");

        // Output: "abc\n// footer"
        assert_eq!(ct.build_string(), "abc\n// footer");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Should have at least 2 tokens: one for "abc" and one for the outro
        assert!(
            tokens.len() >= 2,
            "should have tokens for content and outro"
        );

        // Find the outro token — it should be unmapped (no source_id)
        // The outro starts at the same position as end of content, so look for
        // a token with no source_id
        let has_unmapped = tokens.iter().any(|t| t.get_source_id().is_none());
        assert!(
            has_unmapped,
            "outro should produce an unmapped token (no source_id)"
        );
    }

    /// @ai-generated — Empty source string should not panic
    #[test]
    fn test_source_map_empty_source() {
        let allocator = Allocator::default();
        let ct = CodeTransform::new("", &allocator);

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        // Should produce a valid (empty) source map
        let json = map.to_json_string();
        assert!(json.contains("\"mappings\""));
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    /// @ai-generated — Moved CJK content preserves correct UTF-16 source columns
    #[test]
    fn test_source_map_moved_content_utf16() {
        let allocator = Allocator::default();
        // Source: "abc\n中文def"
        //   Line 0: a(0) b(1) c(2) \n(3)
        //   Line 1: 中(4-6) 文(7-9) d(10) e(11) f(12)
        //   UTF-16 cols on line 1: 中=0, 文=1, d=2, e=3, f=4
        let source = "abc\n中文def";
        let mut ct = CodeTransform::new(source, &allocator);
        // Move "中文def" (bytes 4-13) to the beginning
        ct.move_slice(4, 13, 0);
        // Output: "中文defabc\n"
        assert_eq!(ct.build_string(), "中文defabc\n");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The moved "中文def" should map back to src line 1, col 0
        let moved_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 1 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("moved content should map to src(1, 0)");
        // It's at the start of generated output
        assert_eq!(moved_token.get_dst_line(), 0);
        assert_eq!(moved_token.get_dst_col(), 0);
    }

    /// @ai-generated — Consecutive newlines (empty lines) track source_line correctly
    #[test]
    fn test_source_map_consecutive_newlines() {
        let allocator = Allocator::default();
        // Source: "a\n\nb\n\nc"  (lines: "a", "", "b", "", "c")
        let source = "a\n\nb\n\nc";
        let ct = CodeTransform::new(source, &allocator);

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // "a" at src(0,0) → gen(0,0)
        let a_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have token for 'a'");
        assert_eq!(a_token.get_dst_line(), 0);

        // "b" at src(2,0) → gen(2,0)
        let b_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 2 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have token for 'b'");
        assert_eq!(b_token.get_dst_line(), 2);

        // "c" at src(4,0) → gen(4,0)
        let c_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 4 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have token for 'c'");
        assert_eq!(c_token.get_dst_line(), 4);
    }

    /// @ai-generated — Content ending with newline: no token emitted after trailing newline
    #[test]
    fn test_source_map_trailing_newline() {
        let allocator = Allocator::default();
        // Source: "abc\ndef\n" — last byte is \n
        let source = "abc\ndef\n";
        let ct = CodeTransform::new(source, &allocator);

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Should have tokens for "abc" (line 0) and "def" (line 1)
        // but NOT for line 2 (nothing after trailing newline)
        let line2_tokens: Vec<_> = tokens.iter().filter(|t| t.get_dst_line() == 2).collect();
        assert!(
            line2_tokens.is_empty(),
            "should have no tokens on line 2 after trailing newline"
        );
    }

    /// @ai-generated — Remove (empty overwrite) followed by content: positions chain correctly
    #[test]
    fn test_source_map_remove_then_content() {
        let allocator = Allocator::default();
        // Source: "abcdef"
        // Remove "cd" (bytes 2-4), leaving "abef"
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(2, 4, "");

        assert_eq!(ct.build_string(), "abef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // "ab" at src(0,0) → gen(0,0)
        let ab_token = &tokens[0];
        assert_eq!(ab_token.get_src_col(), 0);
        assert_eq!(ab_token.get_dst_col(), 0);

        // "ef" at src(0,4) → gen(0,2) (generated col 2 after "ab")
        let ef_token = tokens
            .iter()
            .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some())
            .expect("should have token for 'ef' at src col 4");
        assert_eq!(
            ef_token.get_dst_col(),
            2,
            "ef should be at generated col 2 after removal of 'cd'"
        );
    }

    /// @ai-generated — UTF-16 on a non-first line: verify resolver handles deeper offsets
    #[test]
    fn test_source_map_utf16_on_later_line() {
        let allocator = Allocator::default();
        // Source: "line1\na😀b"
        //   Line 0: l(0) i(1) n(2) e(3) 1(4) \n(5)
        //   Line 1: a(6) 😀(7-10) b(11)
        //   UTF-16 cols on line 1: a=0, 😀=1(+2 units), b=3
        let source = "line1\na😀b";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(11, 12, "B"); // Replace 'b' with 'B'

        assert_eq!(ct.build_string(), "line1\na😀B");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Overwrite of 'b' should map to src(1, 3) in UTF-16
        let b_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 1 && t.get_src_col() == 3 && t.get_source_id().is_some());
        assert!(
            b_token.is_some(),
            "b on line 1 should map to src col 3 (UTF-16). Tokens on line 1: {:?}",
            tokens
                .iter()
                .filter(|t| t.get_src_line() == 1)
                .map(|t| (t.get_src_col(), t.get_dst_col()))
                .collect::<Vec<_>>()
        );

        // Generated col should also be 3 (UTF-16: a=0, 😀=1-2, B=3)
        let b_token = b_token.unwrap();
        assert_eq!(b_token.get_dst_col(), 3);
    }

    /// @ai-generated — Overwrite with emoji content: next chunk's generated column uses UTF-16
    #[test]
    fn test_source_map_overwrite_with_emoji_content() {
        let allocator = Allocator::default();
        // Source: "abcdef"
        // Overwrite "cd" with "😀" (4 bytes, 2 UTF-16 units)
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.overwrite(2, 4, "😀");

        // Output: "ab😀ef"
        assert_eq!(ct.build_string(), "ab😀ef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // "ef" at src(0,4) → gen col should be 4 (UTF-16: a=0, b=1, 😀=2-3, e=4)
        // NOT 6 (bytes: a=0, b=1, 😀=2-5, e=6)
        let ef_token = tokens
            .iter()
            .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some())
            .expect("should have token for 'ef' at src col 4");
        assert_eq!(
            ef_token.get_dst_col(),
            4,
            "generated col for 'ef' should be 4 (UTF-16), not 6 (bytes)"
        );
    }

    /// @ai-generated — After moved UTF-16 content, next chunk's generated column is correct
    #[test]
    fn test_source_map_after_moved_utf16_content() {
        let allocator = Allocator::default();
        // Source: "abc中文def"
        //   中(3-5) 文(6-8) — each 3 bytes, 1 UTF-16 unit
        // Move "中文" (bytes 3-9) to beginning
        let source = "abc中文def";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.move_slice(3, 9, 0);

        // Output: "中文abcdef"
        assert_eq!(ct.build_string(), "中文abcdef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.js"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // "abc" (original bytes 0-3) should be at generated col 2
        // (after moved "中文" = 2 UTF-16 units), NOT col 6 (bytes)
        let abc_token = tokens
            .iter()
            .find(|t| t.get_src_line() == 0 && t.get_src_col() == 0 && t.get_source_id().is_some())
            .expect("should have token for 'abc' at src(0,0)");
        assert_eq!(
            abc_token.get_dst_col(),
            2,
            "abc should be at generated col 2 after moved 中文 (2 UTF-16 units), not 6 (bytes)"
        );
    }

    // ========================================================================
    // InsertedMapped tests — source-mapped insertions
    // ========================================================================

    /// @ai-generated — InsertedMapped produces a source map token at the given source position
    #[test]
    fn test_inserted_mapped_produces_source_map_token() {
        let allocator = Allocator::default();
        // Source: "abc def ghi"
        //          012345678901
        // Use batch_prepend_left_with_source_map to insert "(def) ? " before "ghi"
        // mapped to source position 4 (where "def" starts).
        let source = "abc def ghi";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.batch_prepend_left_with_source_map(&[(8, Some((4, 0)), "(def) ? ")]);

        // Output: "abc def (def) ? ghi"
        assert_eq!(ct.build_string(), "abc def (def) ? ghi");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Find the InsertedMapped token — it should map to src(0, 4)
        let mapped_token = tokens
            .iter()
            .find(|t| t.get_src_col() == 4 && t.get_source_id().is_some());
        assert!(
            mapped_token.is_some(),
            "InsertedMapped should produce a token at src col 4. Tokens: {:?}",
            tokens
                .iter()
                .map(|t| (
                    t.get_dst_line(),
                    t.get_dst_col(),
                    t.get_src_line(),
                    t.get_src_col(),
                    t.get_source_id()
                ))
                .collect::<Vec<_>>()
        );
        // The token should be at generated col 8 (after "abc def ")
        let mapped_token = mapped_token.unwrap();
        assert_eq!(mapped_token.get_dst_col(), 8);
        assert_eq!(mapped_token.get_src_line(), 0);
    }

    /// @ai-generated — None source_pos produces unmapped token (like regular Inserted)
    #[test]
    fn test_inserted_mapped_none_produces_unmapped_token() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.batch_prepend_left_with_source_map(&[(3, None, "XY")]);

        assert_eq!(ct.build_string(), "abcXYdef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The XY insertion should be unmapped (source_id = None)
        let xy_token = tokens.iter().find(|t| t.get_dst_col() == 3);
        assert!(xy_token.is_some(), "should have token at gen col 3");
        assert!(
            xy_token.unwrap().get_source_id().is_none(),
            "None source_pos should produce unmapped token"
        );
    }

    /// @ai-generated — InsertedMapped with multiline content advances generated position correctly
    #[test]
    fn test_inserted_mapped_multiline_content() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.batch_prepend_left_with_source_map(&[(3, Some((0, 0)), "X\nY")]);

        // Output: "abcX\nYdef"
        assert_eq!(ct.build_string(), "abcX\nYdef");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // "def" at src(0,3) should be on generated line 1 after the newline
        let def_token = tokens
            .iter()
            .find(|t| t.get_src_col() == 3 && t.get_source_id().is_some())
            .expect("should have token for 'def'");
        assert_eq!(def_token.get_dst_line(), 1, "def should be on line 1");
        assert_eq!(
            def_token.get_dst_col(),
            1,
            "def should be at col 1 after 'Y'"
        );
    }

    /// @ai-generated — Mixed mapped and unmapped prepends at the same position
    #[test]
    fn test_inserted_mapped_mixed_with_regular() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        // Two prepends at position 3: one unmapped, one mapped to source pos 0
        ct.batch_prepend_left_with_source_map(&[(3, None, ", "), (3, Some((0, 0)), "(show) ? ")]);

        assert_eq!(ct.build_string(), "abc, (show) ? def");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The unmapped ", " should have no source_id
        let comma_token = tokens.iter().find(|t| t.get_dst_col() == 3);
        assert!(comma_token.is_some());
        assert!(comma_token.unwrap().get_source_id().is_none());

        // The mapped "(show) ? " should map to src(0, 0)
        let mapped_token = tokens
            .iter()
            .find(|t| t.get_dst_col() == 5 && t.get_source_id().is_some());
        assert!(
            mapped_token.is_some(),
            "mapped prepend should produce source-mapped token"
        );
        assert_eq!(mapped_token.unwrap().get_src_col(), 0);
    }

    // ── content_offset tests ────────────────────────────────────

    /// @ai-generated — content_offset shifts the source map token within InsertedMapped content
    #[test]
    fn test_content_offset_shifts_token_within_content() {
        let allocator = Allocator::default();
        // Source: "abc show def"
        //          01234567890
        // Insert "(__props.show) ? " before "def" (position 9), mapped to source 4 (where "show" is)
        // content_offset = 9 (length of "(__props." = 9)
        let source = "abc show def";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.batch_prepend_left_with_source_map(&[(9, Some((4, 9)), "(__props.show) ? ")]);

        assert_eq!(ct.build_string(), "abc show (__props.show) ? def");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The unmapped prefix "(__props." should have NO source_id
        let prefix_token = tokens
            .iter()
            .find(|t| t.get_dst_col() == 9 && t.get_source_id().is_none());
        assert!(
            prefix_token.is_some(),
            "Unmapped prefix '(__props.' should produce unmapped token. Tokens: {:?}",
            tokens
                .iter()
                .map(|t| (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some()
                ))
                .collect::<Vec<_>>()
        );

        // The mapped token should be at dst_col 18 (9 + 9) pointing to src_col 4
        let mapped_token = tokens
            .iter()
            .find(|t| t.get_dst_col() == 18 && t.get_source_id().is_some());
        assert!(
            mapped_token.is_some(),
            "Mapped token should be at dst_col 18 (after prefix). Tokens: {:?}",
            tokens
                .iter()
                .map(|t| (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            mapped_token.unwrap().get_src_col(),
            4,
            "Mapped token should point to src_col 4 (position of 'show')"
        );
    }

    /// @ai-generated — content_offset = 0 behaves like original InsertedMapped (token at start)
    #[test]
    fn test_content_offset_zero_is_original_behavior() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        ct.batch_prepend_left_with_source_map(&[(3, Some((0, 0)), "(show) ? ")]);

        assert_eq!(ct.build_string(), "abc(show) ? def");

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // With content_offset = 0, token should be at dst_col 3 (start of content)
        let mapped = tokens
            .iter()
            .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_some());
        assert!(
            mapped.is_some(),
            "With offset 0, token should be at content start. Tokens: {:?}",
            tokens
                .iter()
                .map(|t| (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(mapped.unwrap().get_src_col(), 0);

        // Negative: there should be NO unmapped token at dst_col 3
        // (since offset is 0, the very first token should be mapped)
        let unmapped_at_start = tokens
            .iter()
            .find(|t| t.get_dst_col() == 3 && t.get_source_id().is_none());
        assert!(
            unmapped_at_start.is_none(),
            "With offset 0, there should be no unmapped prefix token at content start"
        );
    }

    /// @ai-generated — content_offset with binding prefix: hover on identifier maps correctly
    #[test]
    fn test_content_offset_binding_prefix_hover_maps_to_identifier() {
        let allocator = Allocator::default();
        // Simulates: v-if="leftArrow" → condition prefix "(__props.leftArrow) ? "
        // Source: `<div v-if="leftArrow">` where "leftArrow" starts at byte 11
        // content_offset = 10 (length of "(__props." = 1 + 8 = 9... wait:
        //   "(" = 1, "__props." = 8, total = 9)
        let source = "<div v-if=\"leftArrow\">content</div>";
        let mut ct = CodeTransform::new(source, &allocator);
        // Insert condition prefix before "<div" (pos 0), mapped to source 11 (where "leftArrow" starts)
        // content_offset = 9: skip "(__props."
        ct.batch_prepend_left_with_source_map(&[(0, Some((11, 9)), "(__props.leftArrow) ? ")]);

        let output = ct.build_string();
        assert!(
            output.starts_with("(__props.leftArrow) ? <div"),
            "got: {}",
            output
        );

        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // The mapped token should be at dst_col 9 (after "(__props.") pointing to src_col 11
        let mapped = tokens
            .iter()
            .find(|t| t.get_dst_col() == 9 && t.get_source_id().is_some() && t.get_src_col() == 11);
        assert!(
            mapped.is_some(),
            "Mapped token at 'leftArrow' should point to src col 11. Tokens: {:?}",
            tokens
                .iter()
                .map(|t| (
                    t.get_dst_col(),
                    t.get_src_col(),
                    t.get_source_id().is_some()
                ))
                .collect::<Vec<_>>()
        );

        // Negative: no mapped token should exist at dst_col 0 or 1
        // (the "(" and "__props." are unmapped)
        let mapped_at_prefix = tokens
            .iter()
            .find(|t| t.get_dst_col() < 9 && t.get_source_id().is_some());
        assert!(
            mapped_at_prefix.is_none(),
            "No mapped token should exist in the unmapped prefix region (dst_col < 9)"
        );
    }

    /// @ai-generated — content_offset clamped to content length (safety)
    #[test]
    fn test_content_offset_clamped_if_exceeds_length() {
        let allocator = Allocator::default();
        let source = "abcdef";
        let mut ct = CodeTransform::new(source, &allocator);
        // content_offset = 100, but content is only 3 bytes
        ct.batch_prepend_left_with_source_map(&[(3, Some((0, 100)), "XYZ")]);

        assert_eq!(ct.build_string(), "abcXYZdef");

        // Should not panic; the entire content becomes unmapped prefix
        let map = ct.generate_map(SourceMapOptions::new().with_source("test.vue"));
        let _tokens: Vec<_> = map.get_tokens().collect();
    }
}
