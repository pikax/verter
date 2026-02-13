use memchr::memchr_iter;

use super::code_transform::CodeTransform;
use oxc_sourcemap::{SourceMap, SourceMapBuilder};

/// Options for source map generation
#[derive(Debug, Clone)]
pub struct SourceMapOptions {
    /// The filename of the source file
    pub source: Option<String>,
    /// The filename of the generated file
    pub file: Option<String>,
    /// Whether to include the source content in the map
    pub include_content: bool,
}

impl Default for SourceMapOptions {
    fn default() -> Self {
        Self {
            source: None,
            file: None,
            include_content: true,
        }
    }
}

impl SourceMapOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
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
    #[allow(unused_assignments)]
    pub fn generate_map(&self, options: SourceMapOptions) -> SourceMap {
        let mut builder = SourceMapBuilder::default();

        // Set up source file
        let source_id = if let Some(source) = &options.source {
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
            builder.set_file(&file);
        }

        // Build line-starts table once: line_starts[i] = byte offset of line i.
        // Line 0 always starts at offset 0.
        let original_bytes = self.original().as_bytes();
        let line_starts = {
            let mut starts = Vec::with_capacity(original_bytes.len() / 40 + 1);
            starts.push(0u32);
            for pos in memchr_iter(b'\n', original_bytes) {
                starts.push((pos + 1) as u32);
            }
            starts
        };

        let mut generated_line = 0u32;
        let mut generated_column = 0u32;

        // Add intro (no source mapping for inserted content)
        if !self.intro().is_empty() {
            builder.add_token(generated_line, generated_column, 0, 0, None, None);
            Self::advance_generated_position(
                self.intro().as_bytes(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        // Process chunks
        for chunk in self.chunks() {
            use super::chunk::Chunk;

            match chunk {
                Chunk::Original { start, end } => {
                    if let Some(source_id) = source_id {
                        let slice_bytes = &original_bytes[*start as usize..*end as usize];
                        let (mut source_line, mut source_column) =
                            Self::offset_to_line_column(&line_starts, *start);

                        // Map start of chunk
                        builder.add_token(
                            generated_line,
                            generated_column,
                            source_line,
                            source_column,
                            Some(source_id),
                            None,
                        );

                        // Scan for newlines using memchr — only newlines matter for mappings
                        let mut prev = 0usize;
                        let slice_len = slice_bytes.len();
                        for nl_pos in memchr_iter(b'\n', slice_bytes) {
                            // Characters before this newline advance column
                            let chars_before = nl_pos - prev;
                            generated_column += chars_before as u32;
                            source_column += chars_before as u32;

                            // Newline
                            generated_line += 1;
                            generated_column = 0;
                            source_line += 1;
                            source_column = 0;
                            prev = nl_pos + 1;

                            // Only add mapping if this is NOT the last byte
                            if prev < slice_len {
                                builder.add_token(
                                    generated_line,
                                    generated_column,
                                    source_line,
                                    source_column,
                                    Some(source_id),
                                    None,
                                );
                            }
                        }

                        // Remaining chars after last newline (or all chars if no newlines)
                        let remaining = slice_len - prev;
                        generated_column += remaining as u32;
                    }
                }
                Chunk::Edited {
                    content,
                    original_start,
                    original_end,
                    ..
                } => {
                    if content.is_empty() {
                        continue;
                    }

                    let has_original = original_start.is_some() && original_end.is_some();

                    if has_original {
                        let orig_start = original_start.unwrap();
                        let orig_end = original_end.unwrap();
                        let original_slice = self.slice(orig_start, orig_end);

                        let is_move = *content == original_slice;

                        if is_move {
                            // Moved content — line-by-line mappings like Original chunks
                            if let Some(source_id) = source_id {
                                let content_bytes = content.as_bytes();
                                let (mut source_line, mut source_column) =
                                    Self::offset_to_line_column(&line_starts, orig_start);

                                builder.add_token(
                                    generated_line,
                                    generated_column,
                                    source_line,
                                    source_column,
                                    Some(source_id),
                                    None,
                                );

                                let mut prev = 0usize;
                                let content_len = content_bytes.len();
                                for nl_pos in memchr_iter(b'\n', content_bytes) {
                                    let chars_before = nl_pos - prev;
                                    generated_column += chars_before as u32;
                                    source_column += chars_before as u32;

                                    generated_line += 1;
                                    generated_column = 0;
                                    source_line += 1;
                                    source_column = 0;
                                    prev = nl_pos + 1;

                                    if prev < content_len {
                                        builder.add_token(
                                            generated_line,
                                            generated_column,
                                            source_line,
                                            source_column,
                                            Some(source_id),
                                            None,
                                        );
                                    }
                                }

                                let remaining = content_len - prev;
                                generated_column += remaining as u32;
                            }
                        } else {
                            // Overwritten content — only map start position
                            if let Some(source_id) = source_id {
                                let (source_line, source_column) =
                                    Self::offset_to_line_column(&line_starts, orig_start);

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
                                content.as_bytes(),
                                &mut generated_line,
                                &mut generated_column,
                            );
                        }
                    } else {
                        // Pure insertion — unmapped
                        builder.add_token(generated_line, generated_column, 0, 0, None, None);

                        Self::advance_generated_position(
                            content.as_bytes(),
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
                self.outro().as_bytes(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        builder.into_sourcemap()
    }

    /// Binary-search the line-starts table to convert a byte offset
    /// into (line, column) in O(log N) time.
    #[inline]
    fn offset_to_line_column(line_starts: &[u32], offset: u32) -> (u32, u32) {
        // partition_point returns the first index where line_starts[i] > offset,
        // so line = that index - 1.
        let line = line_starts.partition_point(|&s| s <= offset);
        let line = if line > 0 { line - 1 } else { 0 };
        let column = offset - line_starts[line];
        (line as u32, column)
    }

    /// Advance generated line/column position through a byte slice using memchr.
    #[inline]
    fn advance_generated_position(bytes: &[u8], line: &mut u32, column: &mut u32) {
        let mut prev = 0usize;
        for nl_pos in memchr_iter(b'\n', bytes) {
            *line += 1;
            prev = nl_pos + 1;
        }
        if prev == 0 {
            // No newlines at all — just advance column by byte count
            *column += bytes.len() as u32;
        } else {
            // Column is distance from last newline to end
            *column = (bytes.len() - prev) as u32;
        }
    }

    /// Generate source map and return as JSON string
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

    #[test]
    fn test_line_column_calculation() {
        let source = "Hello\nWorld\nTest";
        let bytes = source.as_bytes();
        let mut line_starts = vec![0u32];
        for pos in memchr_iter(b'\n', bytes) {
            line_starts.push((pos + 1) as u32);
        }

        assert_eq!(
            CodeTransform::offset_to_line_column(&line_starts, 0),
            (0, 0)
        ); // H
        assert_eq!(
            CodeTransform::offset_to_line_column(&line_starts, 5),
            (0, 5)
        ); // \n
        assert_eq!(
            CodeTransform::offset_to_line_column(&line_starts, 6),
            (1, 0)
        ); // W
        assert_eq!(
            CodeTransform::offset_to_line_column(&line_starts, 12),
            (2, 0)
        ); // T
    }
}
