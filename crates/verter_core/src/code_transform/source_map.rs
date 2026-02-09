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

        let mut generated_line = 0u32;
        let mut generated_column = 0u32;

        // Add intro (no source mapping for inserted content)
        if !self.intro().is_empty() {
            // Add unmapped token to break any potential mapping chain
            builder.add_token(generated_line, generated_column, 0, 0, None, None);

            for ch in self.intro().chars() {
                if ch == '\n' {
                    generated_line += 1;
                    generated_column = 0;
                } else {
                    generated_column += 1;
                }
            }
        }

        // Process chunks
        for chunk in self.chunks() {
            use super::chunk::Chunk;

            match chunk {
                Chunk::Original { start, end } => {
                    // Original content - create mappings at each line boundary
                    if let Some(source_id) = source_id {
                        let source_slice = self.slice(*start, *end);
                        let (mut source_line, mut source_column) =
                            self.calculate_line_column(*start);

                        // Add mapping for the start of this chunk
                        builder.add_token(
                            generated_line,
                            generated_column,
                            source_line,
                            source_column,
                            Some(source_id),
                            None,
                        );

                        // Process each character, adding mappings at line boundaries
                        // but NOT after the final newline (to avoid mapping unmapped content)
                        let chars: Vec<char> = source_slice.chars().collect();
                        let len = chars.len();

                        for (i, ch) in chars.into_iter().enumerate() {
                            if ch == '\n' {
                                generated_line += 1;
                                generated_column = 0;
                                source_line += 1;
                                source_column = 0;

                                // Only add mapping if this is NOT the last character
                                // (don't map the line after final newline)
                                if i + 1 < len {
                                    builder.add_token(
                                        generated_line,
                                        generated_column,
                                        source_line,
                                        source_column,
                                        Some(source_id),
                                        None,
                                    );
                                }
                            } else {
                                generated_column += 1;
                                source_column += 1;
                            }
                        }
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

                    // Check if this has original position info
                    let has_original = original_start.is_some() && original_end.is_some();

                    if has_original {
                        let orig_start = original_start.unwrap();
                        let orig_end = original_end.unwrap();
                        let original_slice = self.slice(orig_start, orig_end);

                        // Check if this is a move (content matches original) or overwrite (content differs)
                        let is_move = *content == original_slice;

                        if is_move {
                            // Moved content - create line-by-line mappings like Original chunks
                            if let Some(source_id) = source_id {
                                let (mut source_line, mut source_column) =
                                    self.calculate_line_column(orig_start);

                                // Add mapping for the start of this chunk
                                builder.add_token(
                                    generated_line,
                                    generated_column,
                                    source_line,
                                    source_column,
                                    Some(source_id),
                                    None,
                                );

                                // Process each character, adding mappings at line boundaries
                                let chars: Vec<char> = content.chars().collect();
                                let len = chars.len();

                                for (i, ch) in chars.into_iter().enumerate() {
                                    if ch == '\n' {
                                        generated_line += 1;
                                        generated_column = 0;
                                        source_line += 1;
                                        source_column = 0;

                                        // Only add mapping if this is NOT the last character
                                        if i + 1 < len {
                                            builder.add_token(
                                                generated_line,
                                                generated_column,
                                                source_line,
                                                source_column,
                                                Some(source_id),
                                                None,
                                            );
                                        }
                                    } else {
                                        generated_column += 1;
                                        source_column += 1;
                                    }
                                }
                            }
                        } else {
                            // Overwritten content - only map start position
                            if let Some(source_id) = source_id {
                                let (source_line, source_column) =
                                    self.calculate_line_column(orig_start);

                                builder.add_token(
                                    generated_line,
                                    generated_column,
                                    source_line,
                                    source_column,
                                    Some(source_id),
                                    None,
                                );
                            }

                            // Update generated position
                            for ch in content.chars() {
                                if ch == '\n' {
                                    generated_line += 1;
                                    generated_column = 0;
                                } else {
                                    generated_column += 1;
                                }
                            }
                        }
                    } else {
                        // Pure insertion - add unmapped token to break mapping chain
                        builder.add_token(generated_line, generated_column, 0, 0, None, None);

                        // Update generated position
                        for ch in content.chars() {
                            if ch == '\n' {
                                generated_line += 1;
                                generated_column = 0;
                            } else {
                                generated_column += 1;
                            }
                        }
                    }
                }
            }
        }

        // Add outro (no source mapping for inserted content)
        if !self.outro().is_empty() {
            // Add unmapped token to break any mapping chain from previous content
            builder.add_token(generated_line, generated_column, 0, 0, None, None);

            for ch in self.outro().chars() {
                if ch == '\n' {
                    generated_line += 1;
                    generated_column = 0;
                } else {
                    generated_column += 1;
                }
            }
        }

        builder.into_sourcemap()
    }

    /// Calculate line and column from byte offset
    fn calculate_line_column(&self, offset: u32) -> (u32, u32) {
        let mut line = 0u32;
        let mut column = 0u32;
        let mut current = 0u32;

        for ch in self.original().chars() {
            if current >= offset {
                break;
            }

            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }

            current += ch.len_utf8() as u32;
        }

        (line, column)
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
        let allocator = Allocator::default();
        let ct = CodeTransform::new("Hello\nWorld\nTest", &allocator);

        assert_eq!(ct.calculate_line_column(0), (0, 0)); // H
        assert_eq!(ct.calculate_line_column(5), (0, 5)); // \n
        assert_eq!(ct.calculate_line_column(6), (1, 0)); // W
        assert_eq!(ct.calculate_line_column(12), (2, 0)); // T
    }
}
