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
    /// ```ignore
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
#[path = "source_map_tests.rs"]
mod source_map_tests;
