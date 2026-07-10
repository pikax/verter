use std::borrow::Cow;

use memchr::memchr_iter;

use super::chunk::Chunk;
use super::code_transform::CodeTransform;
use crate::cursor::position::{utf16_len, PositionResolver};
use oxc_sourcemap::{SourceMap, Token};

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
    /// use verter_compiler::code_transform::{CodeTransform, SourceMapOptions};
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
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn generate_map(&self, options: SourceMapOptions) -> SourceMap<'static> {
        self.generate_map_with_preamble(options).0
    }

    /// Like [`generate_map`](Self::generate_map), but ALSO returns the generated-TSX position
    /// `(line, utf16_column)` immediately AFTER the recorded helper-import preamble insertion (see
    /// [`set_helper_preamble_content`](Self::set_helper_preamble_content)) — the typed
    /// helper-import-preamble end boundary. The preamble insertion is located by pointer identity
    /// during the single generated-order chunk walk (no second pass, no content sniffing). Returns
    /// `None` for the boundary when no preamble was recorded or its chunk was not emitted (e.g.
    /// empty content). This is the SINGLE source of generated-position tracking; `generate_map`
    /// delegates here and drops the boundary.
    #[must_use]
    #[allow(unused_assignments)] // generated_line/column updated in outro but intentionally not read after
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn generate_map_with_preamble(
        &self,
        options: SourceMapOptions,
    ) -> (SourceMap<'static>, Option<(u32, u32)>) {
        let preamble = self.helper_preamble_content();
        // Generated-TSX position immediately after the helper-import preamble insertion, captured
        // when the walk advances past its chunk. Pointer identity (start + len) is exact: the same
        // bump-allocated `&str` flows from the insertion into its `Inserted`/`InsertedMapped` chunk.
        let mut preamble_end: Option<(u32, u32)> = None;
        let is_preamble = |content: &str| {
            preamble.is_some_and(|p| {
                std::ptr::eq(p.as_ptr(), content.as_ptr()) && p.len() == content.len()
            })
        };

        // Set up source file. Our usage never duplicates sources or adds names,
        // so the tokens accumulate into a single locally-owned, pre-reserved Vec
        // that is handed directly to `SourceMap::new` — avoiding both the
        // builder's incremental token-Vec regrowth and a dedup hash map.
        let (sources, source_contents, source_id) = if let Some(source) = options.source {
            let content = if options.include_content {
                self.original()
            } else {
                ""
            };
            (
                vec![Cow::Owned(source.to_owned())],
                vec![Some(Cow::Owned(content.to_owned()))],
                Some(0u32),
            )
        } else {
            (Vec::new(), Vec::new(), None)
        };

        let file = options.file.map(|f| Cow::Owned(f.to_owned()));

        // One resolver per original source, built once and reused across maps.
        let resolver = self.sourcemap_resolver();

        // Reserve the token vector up front from a chunk/newline upper bound so
        // it never reallocates during population.
        let mut tokens: Vec<Token> = Vec::with_capacity(self.estimate_sourcemap_token_capacity());

        // Capture the reserved capacity at the reservation point so tests can
        // assert the buffer covers the full emitted token count — proving the
        // reservation exists (a missing one collapses this to 0) without
        // reallocating during population.
        #[cfg(test)]
        self.record_reserved_token_capacity(tokens.capacity());

        let mut generated_line = 0u32;
        let mut generated_column = 0u32;

        // Add intro (no source mapping for inserted content)
        if !self.intro().is_empty() {
            tokens.push(Token::new(
                generated_line,
                generated_column,
                0,
                0,
                None,
                None,
            ));
            Self::advance_generated_position(
                self.intro(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        let is_ascii = self.is_ascii();

        // Process chunks
        for chunk in self.chunks() {
            match chunk {
                Chunk::Original { start, end } => {
                    if let Some(source_id) = source_id {
                        let slice = &self.original()[*start as usize..*end as usize];

                        Self::emit_mapped_content(
                            &mut tokens,
                            slice,
                            source_id,
                            resolver,
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
                            &mut tokens,
                            content,
                            source_id,
                            resolver,
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

                        tokens.push(Token::new(
                            generated_line,
                            generated_column,
                            source_line,
                            source_column,
                            Some(source_id),
                            None,
                        ));
                    }

                    Self::advance_generated_position(
                        content,
                        &mut generated_line,
                        &mut generated_column,
                    );
                }
                Chunk::Inserted { content } | Chunk::InsertedAnchored { content, .. } => {
                    if content.is_empty() {
                        continue;
                    }
                    // Pure insertion — unmapped (an anchored insertion's
                    // affinity only affects edit semantics, not mapping)
                    tokens.push(Token::new(
                        generated_line,
                        generated_column,
                        0,
                        0,
                        None,
                        None,
                    ));

                    Self::advance_generated_position(
                        content,
                        &mut generated_line,
                        &mut generated_column,
                    );
                    // The helper-import preamble is an unmapped insertion: capture the
                    // generated position immediately after it as the preamble-end boundary.
                    if is_preamble(content) {
                        preamble_end = Some((generated_line, generated_column));
                    }
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
                        tokens.push(Token::new(
                            generated_line,
                            generated_column,
                            0,
                            0,
                            None,
                            None,
                        ));
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
                            tokens.push(Token::new(
                                generated_line,
                                generated_column,
                                (sl - 1) as u32,
                                (sc - 1) as u32,
                                Some(source_id),
                                None,
                            ));
                        }
                        Self::advance_generated_position(
                            rest,
                            &mut generated_line,
                            &mut generated_column,
                        );
                    }
                    // Defensive: the helper-import preamble is emitted unmapped (`Inserted`), but
                    // match here too so the boundary survives if a preamble is ever routed through
                    // a mapped insertion.
                    if is_preamble(content) {
                        preamble_end = Some((generated_line, generated_column));
                    }
                }
            }
        }

        // Add outro (no source mapping)
        if !self.outro().is_empty() {
            tokens.push(Token::new(
                generated_line,
                generated_column,
                0,
                0,
                None,
                None,
            ));
            Self::advance_generated_position(
                self.outro(),
                &mut generated_line,
                &mut generated_column,
            );
        }

        (
            SourceMap::new(
                file,
                Vec::new(),
                None,
                sources,
                source_contents,
                tokens.into_boxed_slice(),
                None,
            ),
            preamble_end,
        )
    }

    /// Upper bound on the number of source-map tokens `generate_map` will emit,
    /// used to reserve the token vector before population so it never
    /// reallocates while being populated.
    ///
    /// Each term covers a distinct source of emitted tokens:
    /// - Every chunk emits at least one token at its start — `self.chunks().len()`.
    /// - An `Original` chunk emits one extra token after each interior newline.
    ///   Original chunks are disjoint slices of the source, so the original's
    ///   total newline count bounds them collectively — `original_newlines`.
    /// - A `Moved` chunk likewise emits one extra token per interior newline, but
    ///   a moved overwrite carries *replacement* text whose newlines are absent
    ///   from the original source; those are counted per chunk —
    ///   `moved_content_newlines`.
    /// - An `InsertedMapped` chunk can emit a second token for its unmapped
    ///   prefix — `inserted_mapped`.
    /// - The optional intro and outro each emit at most one token — the `+ 2`.
    ///
    /// Over-counting (a `Moved` slice of the original is counted in both
    /// `original_newlines` and `moved_content_newlines`) only enlarges the
    /// reserve; the result stays a true upper bound, never an under-estimate.
    pub(super) fn estimate_sourcemap_token_capacity(&self) -> usize {
        let original_newlines = memchr_iter(b'\n', self.original().as_bytes()).count();

        let mut moved_content_newlines = 0usize;
        let mut inserted_mapped = 0usize;
        for chunk in self.chunks() {
            match chunk {
                Chunk::Moved { content, .. } => {
                    moved_content_newlines += memchr_iter(b'\n', content.as_bytes()).count();
                }
                Chunk::InsertedMapped { .. } => inserted_mapped += 1,
                _ => {}
            }
        }

        self.chunks().len() + original_newlines + moved_content_newlines + inserted_mapped + 2
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
        tokens: &mut Vec<Token>,
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

        tokens.push(Token::new(
            *generated_line,
            *generated_column,
            source_line,
            (sc - 1) as u32,
            Some(source_id),
            None,
        ));

        // Scan for newlines — O(1) manual tracking per newline (no binary search)
        let mut prev = 0usize;

        for nl_pos in memchr_iter(b'\n', content_bytes) {
            *generated_line += 1;
            *generated_column = 0;
            source_line += 1;
            prev = nl_pos + 1;

            // After a newline, source column is always 0
            if prev < content_len {
                tokens.push(Token::new(
                    *generated_line,
                    *generated_column,
                    source_line,
                    0,
                    Some(source_id),
                    None,
                ));
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

    /// Generate the source map JSON, augmented with the typed helper-import-preamble end boundary
    /// when one was recorded (`x_verter_helper_preamble_end`). This is the producer side of the LSP
    /// auto-import preamble classifier: the boundary is the generated-TSX position immediately after
    /// the last helper import. It rides the source-map JSON (an `x_`-prefixed metadata member, the
    /// source-map extension convention) because the only compiler→LSP transport is the source-map
    /// string; `oxc_sourcemap` ignores the unknown member on parse, and the strict `PositionMapper`
    /// recovers it into a typed field.
    #[must_use]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn generate_map_json_with_preamble(&self, options: SourceMapOptions) -> String {
        let (map, preamble_end) = self.generate_map_with_preamble(options);
        inject_helper_preamble_end(map.to_json_string(), preamble_end)
    }
}

/// Inject the `x_verter_helper_preamble_end` member into an `oxc_sourcemap` JSON object string.
///
/// `oxc_sourcemap`'s encoder has a fixed field set, so the boundary is added as a leading object
/// member (the encoder always emits `{"version":3,…`, so splicing after `{` is deterministic and
/// keeps valid JSON). A no-op when there is no boundary. The member shape mirrors the LSP's typed
/// `TsPosition` (`{"line":<u32>,"character":<u32>}`).
fn inject_helper_preamble_end(json: String, preamble_end: Option<(u32, u32)>) -> String {
    match preamble_end {
        // `strip_prefix('{')` both confirms the leading `{` and yields the remainder
        // without an explicit byte slice — safe even if the encoder ever emits an
        // empty / short string.
        Some((line, character)) => match json.strip_prefix('{') {
            Some(rest) => format!(
                "{{\"x_verter_helper_preamble_end\":{{\"line\":{line},\"character\":{character}}},{rest}"
            ),
            None => json,
        },
        None => json,
    }
}

#[cfg(test)]
#[path = "source_map_tests.rs"]
mod source_map_tests;
