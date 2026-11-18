use std::borrow::Cow;

use memchr::memchr_iter;
use smallvec::SmallVec;

use super::code_transform::CodeTransform;
use super::fallible::CodeTransformError;
use super::freeze::{chunk_content, synthetic_sequence};
use crate::assembly::map_input::WireSegment;
use crate::emission::correspondence::{classify, wire_tokens, WireCursor, WireSource};
use crate::emission::{MapMode, Piece, Provenance};
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

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
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
    /// let source_map = ct.generate_map(options)?;
    /// ```
    ///
    /// # Errors
    ///
    /// [`CodeTransformError::MapNotCaptured`] for a transform created without
    /// map capture.
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn generate_map(
        &self,
        options: SourceMapOptions,
    ) -> Result<SourceMap<'static>, CodeTransformError> {
        Ok(self.generate_map_product(options)?.0)
    }

    /// Like [`generate_map`](Self::generate_map), but ALSO returns the generated-TSX position
    /// `(line, utf16_column)` immediately AFTER the recorded helper-import preamble insertion (see
    /// [`set_helper_preamble_content`](Self::set_helper_preamble_content)) — the typed
    /// helper-import-preamble end boundary. The preamble insertion is located by pointer identity
    /// during the single generated-order chunk walk (no second pass, no content sniffing). Returns
    /// `None` for the boundary when no preamble was recorded or its chunk was not emitted (e.g.
    /// empty content). This is the SINGLE source of generated-position tracking; `generate_map`
    /// delegates here and drops the boundary.
    ///
    /// # Errors
    ///
    /// [`CodeTransformError::MapNotCaptured`] for a transform created without
    /// map capture.
    #[allow(unused_assignments)] // generated_line/column updated in outro but intentionally not read after
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn generate_map_with_preamble(
        &self,
        options: SourceMapOptions,
    ) -> Result<(SourceMap<'static>, Option<(u32, u32)>), CodeTransformError> {
        let (map, preamble_end, _) = self.generate_map_product(options)?;
        Ok((map, preamble_end))
    }

    #[allow(clippy::type_complexity)]
    fn generate_map_product(
        &self,
        options: SourceMapOptions,
    ) -> Result<
        (
            SourceMap<'static>,
            Option<(u32, u32)>,
            Option<super::mapping_product::InsertionAnchor>,
        ),
        CodeTransformError,
    > {
        self.require_map()?;
        Ok(self.captured_map_product(options))
    }

    /// The map product of a transform that captured map facts.
    pub(super) fn captured_map_product(
        &self,
        options: SourceMapOptions,
    ) -> (
        SourceMap<'static>,
        Option<(u32, u32)>,
        Option<super::mapping_product::InsertionAnchor>,
    ) {
        verter_audit::attribute_n!(SourceMapBuild, self.chunks.len());
        let preamble = self.helper_preamble_content();
        // Generated-TSX position immediately after the helper-import preamble insertion, captured
        // when the walk advances past its chunk. Pointer identity (start + len) is exact: the same
        // bump-allocated `&str` flows from the insertion into its `Inserted`/`InsertedMapped` chunk.
        let mut preamble_end: Option<(u32, u32)> = None;
        let mut insertion_anchor = None;
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

        // Additional authored boundaries never affect generated bytes. Sort
        // them once for both exact reservation accounting and the mapped-chunk
        // emission walk.
        let mut sourcemap_locations = self.sourcemap_locations().to_vec();
        sourcemap_locations.sort_unstable();
        sourcemap_locations.dedup();

        // Reserve an upper bound of the tokens the walk can place, so
        // population does not reallocate.
        let token_capacity =
            self.estimate_sourcemap_token_capacity(source_id.is_some(), &sourcemap_locations);
        let mut wire: Vec<WireSegment> = Vec::with_capacity(token_capacity);
        let mut cursor = WireCursor::default();
        let source = source_id.map(|index| WireSource { index, resolver });

        // The intro is synthetic text.
        for piece in synthetic_sequence(self.intro(), MapMode::Map) {
            wire_tokens(
                classify(&piece),
                self.piece_text(&piece),
                source,
                &sourcemap_locations,
                &mut cursor,
                &mut wire,
            );
        }
        // The helper-import preamble may BE the module intro (the Svelte IDE projection prepends
        // its `@jsxImportSource`-led prelude as the intro so the pragma stays the leading output
        // bytes). When it is, capture the generated position immediately after it as the
        // preamble-end boundary — the same boundary Vue publishes from its `Inserted` preamble
        // chunk. First capture wins (the debug-assert catches accidental double-publication: a
        // preamble can be the intro OR an inserted chunk, never both).
        if self.intro_len() > 0 && self.sole_intro_piece().is_some_and(is_preamble) {
            verter_debug_assert!(preamble_end.is_none());
            preamble_end.get_or_insert((cursor.line, cursor.column));
            insertion_anchor = self.helper_preamble_carrier_anchor().map(|carrier| {
                super::mapping_product::InsertionAnchor {
                    projected: 0,
                    carrier,
                }
            });
        }

        // Every byte through the one interpretation and the one wire policy:
        // the pieces each chunk becomes, the same pieces a frozen transform
        // holds, placed exactly as the emission writer places them.
        let mut generated_byte = self.intro_len() as u32;
        let mut pieces: SmallVec<[Piece<'a>; 4]> = SmallVec::new();
        for (chunk, relocated) in self.leaf_chunks() {
            let chunk_start = generated_byte;
            pieces.clear();
            self.leaf_pieces(chunk, relocated, &mut |piece| pieces.push(piece));
            for piece in &pieces {
                let text = self.piece_text(piece);
                generated_byte = generated_byte.saturating_add(text.len() as u32);
                wire_tokens(
                    classify(piece),
                    text,
                    source,
                    &sourcemap_locations,
                    &mut cursor,
                    &mut wire,
                );
            }
            // The helper-import preamble is an unmapped insertion: capture the
            // generated position immediately after it as the preamble-end
            // boundary. First capture wins (the debug-assert catches
            // accidental double-publication).
            if chunk_content(chunk).is_some_and(is_preamble) {
                verter_debug_assert!(preamble_end.is_none());
                preamble_end.get_or_insert((cursor.line, cursor.column));
                insertion_anchor = self.helper_preamble_carrier_anchor().map(|carrier| {
                    super::mapping_product::InsertionAnchor {
                        projected: chunk_start,
                        carrier,
                    }
                });
            }
        }

        // The outro is synthetic text.
        for piece in synthetic_sequence(self.outro(), MapMode::Map) {
            wire_tokens(
                classify(&piece),
                self.piece_text(&piece),
                source,
                &sourcemap_locations,
                &mut cursor,
                &mut wire,
            );
        }

        let mut tokens: Vec<Token> = Vec::with_capacity(wire.len());
        // Capture the reserved capacity at the reservation point so tests can
        // assert the buffer covers the full emitted token count without
        // reallocating during population.
        #[cfg(test)]
        self.record_reserved_token_capacity(wire.capacity());
        tokens.extend(wire.iter().map(|segment| match segment.payload {
            Some(payload) => Token::new(
                segment.generated_line,
                segment.generated_column,
                payload.source_line,
                payload.source_column,
                Some(payload.source_index),
                None,
            ),
            None => Token::new(
                segment.generated_line,
                segment.generated_column,
                0,
                0,
                None,
                None,
            ),
        }));

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
            insertion_anchor,
        )
    }

    /// Exact number of tokens the emission walk will push for the current
    /// chunks, source availability, and sorted unique authored boundaries.
    /// Keeping this formula variant-for-variant with the emitter avoids both
    /// reallocations and replacement-text newline over-reservation.
    fn estimate_sourcemap_token_capacity(
        &self,
        has_source: bool,
        sourcemap_locations: &[u32],
    ) -> usize {
        let mut capacity = usize::from(self.intro_len() > 0) + usize::from(self.outro_len() > 0);

        let mut pieces: SmallVec<[Piece<'a>; 4]> = SmallVec::new();
        for (chunk, relocated) in self.leaf_chunks() {
            pieces.clear();
            self.leaf_pieces(chunk, relocated, &mut |piece| pieces.push(piece));
            for piece in &pieces {
                let text = self.piece_text(piece);
                capacity += Self::piece_token_count(piece, text, has_source, sourcemap_locations);
            }
        }

        capacity
    }

    /// Count the initial mapped token plus the union of interior line starts
    /// and authored boundaries. A boundary exactly at a line start shares the
    /// line token, matching the identity runs of `wire_tokens`.
    fn mapped_content_token_count(
        content: &str,
        original_start: u32,
        original_end: u32,
        sourcemap_locations: &[u32],
    ) -> usize {
        let interior_line_starts = memchr_iter(b'\n', content.as_bytes())
            .filter(|position| position + 1 < content.len())
            .count();
        let first_location =
            sourcemap_locations.partition_point(|location| *location <= original_start);
        let authored_boundaries = sourcemap_locations[first_location..]
            .iter()
            .take_while(|location| **location < original_end)
            .filter(|location| {
                let relative = (**location - original_start) as usize;
                relative < content.len() && content.as_bytes()[relative - 1] != b'\n'
            })
            .count();

        1 + interior_line_starts + authored_boundaries
    }

    /// Upper bound of the tokens [`wire_tokens`] places for one piece: a
    /// synthetic run needs its sourceless token only while a mapping is
    /// active on its line.
    fn piece_token_count(
        piece: &Piece<'_>,
        text: &str,
        has_source: bool,
        sourcemap_locations: &[u32],
    ) -> usize {
        if text.is_empty() {
            return usize::from(piece.provenance() == Provenance::Unmapped);
        }
        match piece.provenance() {
            Provenance::Exact { source_start } if has_source => Self::mapped_content_token_count(
                text,
                source_start,
                source_start + text.len() as u32,
                sourcemap_locations,
            ),
            Provenance::Exact { .. } => 1,
            Provenance::Rewritten { .. } | Provenance::Anchor { .. } => 1,
            Provenance::Unmapped | Provenance::External(_) => 1,
        }
    }

    #[cfg(test)]
    pub(super) fn estimate_sourcemap_token_capacity_for_test(&self, has_source: bool) -> usize {
        let mut sourcemap_locations = self.sourcemap_locations().to_vec();
        sourcemap_locations.sort_unstable();
        sourcemap_locations.dedup();
        self.estimate_sourcemap_token_capacity(has_source, &sourcemap_locations)
    }

    /// Generate source map and return as JSON string
    ///
    /// # Errors
    ///
    /// [`CodeTransformError::MapNotCaptured`] for a transform created without
    /// map capture.
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn generate_map_json(
        &self,
        options: SourceMapOptions,
    ) -> Result<String, CodeTransformError> {
        Ok(self.generate_map(options)?.to_json_string())
    }

    /// Generate the source map JSON, augmented with the typed helper-import-preamble end boundary
    /// when one was recorded (`x_verter_helper_preamble_end`). This is the producer side of the LSP
    /// auto-import preamble classifier: the boundary is the generated-TSX position immediately after
    /// the last helper import. It rides the source-map JSON (an `x_`-prefixed metadata member, the
    /// source-map extension convention) because the only compiler→LSP transport is the source-map
    /// string; `oxc_sourcemap` ignores the unknown member on parse, and the strict `PositionMapper`
    /// recovers it into a typed field.
    ///
    /// # Errors
    ///
    /// [`CodeTransformError::MapNotCaptured`] for a transform created without
    /// map capture.
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn generate_map_json_with_preamble(
        &self,
        options: SourceMapOptions,
    ) -> Result<String, CodeTransformError> {
        self.require_map()?;
        Ok(self.captured_map_json_with_preamble(options))
    }

    /// [`generate_map_json_with_preamble`](Self::generate_map_json_with_preamble)
    /// of a transform that captured map facts.
    pub(super) fn captured_map_json_with_preamble(&self, options: SourceMapOptions) -> String {
        let (map, preamble_end, insertion_anchor) = self.captured_map_product(options);
        inject_mapping_product(map.to_json_string(), preamble_end, insertion_anchor)
    }
}

/// Inject the `x_verter_helper_preamble_end` member into an `oxc_sourcemap` JSON object string.
///
/// `oxc_sourcemap`'s encoder has a fixed field set, so the boundary is added as a leading object
/// member (the encoder always emits `{"version":3,…`, so splicing after `{` is deterministic and
/// keeps valid JSON). A no-op when there is no boundary. The preamble member shape mirrors the
/// LSP's typed `TsPosition` (`{"line":<u32>,"character":<u32>}`); the compact mapping-product
/// member carries exact `[projected_byte, carrier_byte]` insertion points.
fn inject_mapping_product(
    json: String,
    preamble_end: Option<(u32, u32)>,
    insertion_anchor: Option<super::mapping_product::InsertionAnchor>,
) -> String {
    match (preamble_end, insertion_anchor) {
        // `strip_prefix('{')` both confirms the leading `{` and yields the remainder
        // without an explicit byte slice — safe even if the encoder ever emits an
        // empty / short string.
        (Some((line, character)), anchor) => match json.strip_prefix('{') {
            Some(rest) => format!(
                "{{\"x_verter_helper_preamble_end\":{{\"line\":{line},\"character\":{character}}},{}{rest}",
                anchor.map_or_else(String::new, |anchor| format!(
                    "\"x_verter_mapping_product\":{{\"schema_version\":1,\"insertion_anchors\":[[{},{}]]}},",
                    anchor.projected, anchor.carrier
                ))
            ),
            None => json,
        },
        (None, _) => json,
    }
}

#[cfg(test)]
#[path = "source_map_tests.rs"]
mod source_map_tests;
