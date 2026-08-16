//! The additive, opt-in segmented-overwrite primitive.
//!
//! `Chunk::Overwritten` (the existing, unconditional `overwrite`/
//! `overwrite_unmapped`/`update` family) emits AT MOST one source-map token
//! for its entire replacement — `generate_map` maps the whole replaced span
//! to the ORIGINAL range's own start (MagicString convention: "no
//! character-level correspondence"). That is correct for the overwhelming
//! majority of overwrites, but wrong for a specific, narrow shape this
//! primitive exists to serve: generated code that is MOSTLY synthetic
//! scaffolding but embeds one or more AUTHORED lexemes VERBATIM at known
//! byte offsets (an interpolation identifier inside `_toDisplayString(...)`,
//! a static attribute name inside a hoisted template string) — each such
//! embedded lexeme has a REAL, checkable authored correspondence that a
//! single whole-block token cannot express.
//!
//! `try_overwrite_segmented` is a SEPARATE, crate-private entry point — it
//! does not modify `overwrite`, `try_overwrite`, `update`, or any existing
//! splice path in any way, and produces a distinct `Chunk::OverwrittenSegmented`
//! variant that only `generate_map`'s own dedicated arm interprets. Existing
//! callers of every other `CodeTransform` operation are provably unaffected
//! (see `code_transform/tests.rs`'s byte-identity suite).
//!
//! Deliberately narrow: the target range must fall entirely within ONE
//! live `Original` chunk (the fast-overwrite precondition `try_fast_overwrite`
//! already uses) and no affinity-anchored insertion may be active anywhere
//! in this transform. Both are true for every intended caller (VDOM/Vapor/
//! SSR template emitters build fresh, non-overlapping per-node overwrites
//! and never use the anchored insertion API) — a caller outside that shape
//! gets a typed refusal instead of a best-effort/wrong splice.

use super::chunk::Chunk;
use super::code_transform::CodeTransform;
use super::fallible::CodeTransformError;
use crate::template::code_gen::types::SegmentedOverwriteAuthority;

/// One embedded authored anchor inside a segmented-overwrite's replacement
/// text: `content[content_offset..content_offset + length]` is the AUTHORED
/// lexeme, copied verbatim, and maps to the authored source byte range
/// `[source_pos, source_pos + length)`. Bytes outside every anchor —
/// including before the first and after the last — are synthetic
/// scaffolding and carry no source-map token.
///
/// `pub`, not `pub(crate)`: it rides inside otherwise-`pub` carriers
/// (`VaporTextPart::Dynamic`, `VaporRootElement::statements`, …) that
/// themselves cross module-visibility boundaries within
/// `template::code_gen`, so the plain data shape must match their own
/// visibility. This does NOT relax the actual authorization boundary — the
/// OPERATIONS that produce a `SegmentAnchor`-bearing chunk
/// (`CodeTransform::try_overwrite_segmented`,
/// `CodeGenOutput::overwrite_segmented`) stay restricted to the authorized
/// Vue runtime emitters; see the static call-site guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentAnchor {
    pub content_offset: u32,
    pub length: u32,
    pub source_pos: u32,
}

impl SegmentAnchor {
    /// Test-only constructor — production call sites always build a
    /// `SegmentAnchor { .. }` struct literal directly (they compute all
    /// three fields inline from AST/segment-plan data), so this convenience
    /// constructor exists only for the direct primitive tests.
    #[cfg(test)]
    pub(crate) fn new(content_offset: u32, length: u32, source_pos: u32) -> Self {
        Self {
            content_offset,
            length,
            source_pos,
        }
    }

    fn content_end(&self) -> u32 {
        self.content_offset + self.length
    }
}

impl<'a> CodeTransform<'a> {
    /// Checked, crate-private segmented overwrite. See the module doc for
    /// the exact shape this serves and its narrow preconditions.
    ///
    /// Fails atomically (no mutation on `Err`) for: a malformed `[start,
    /// end)` range (see [`CodeTransformError`]); an empty range; any anchor
    /// whose `content` span is out of range, not a UTF-8 boundary, or
    /// overlaps/precedes the previous anchor (anchors MUST be supplied in
    /// ascending, non-overlapping `content_offset` order — the caller
    /// already builds them in source order); any anchor's `source_pos` out
    /// of range or mid-character; the target range not fitting entirely
    /// inside one live, unedited `Original` chunk; or any affinity-anchored
    /// insertion active anywhere in this transform (the narrow-shape
    /// precondition above).
    ///
    /// `_authority` proves the caller is `template::code_gen` — the sole
    /// authorized caller (see [`SegmentedOverwriteAuthority`]'s own doc) —
    /// and is otherwise unused; its mere presence in the signature is the
    /// static call-site guard.
    pub(crate) fn try_overwrite_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
        _authority: SegmentedOverwriteAuthority,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.check_range(start, end)?;
        if start == end {
            return Err(CodeTransformError::ZeroLengthRange { offset: start });
        }
        if self.anchored_present {
            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
        }

        let content_len = content.len() as u32;
        let mut prev_end = 0u32;
        for anchor in anchors {
            if anchor.content_offset < prev_end {
                return Err(CodeTransformError::ReversedRange {
                    start: anchor.content_offset,
                    end: prev_end,
                });
            }
            let anchor_end = anchor.content_end();
            if anchor_end > content_len {
                return Err(CodeTransformError::OutOfRange {
                    offset: anchor_end,
                    len: content_len,
                });
            }
            if !content.is_char_boundary(anchor.content_offset as usize)
                || !content.is_char_boundary(anchor_end as usize)
            {
                return Err(CodeTransformError::MidChar {
                    offset: anchor.content_offset,
                });
            }
            self.check_offset(anchor.source_pos)?;
            let source_end = anchor.source_pos + anchor.length;
            self.check_offset(source_end)?;
            prev_end = anchor_end;
        }

        // Narrow precondition: the range must fall entirely inside ONE
        // live `Original` chunk (the exact shape `try_fast_overwrite`
        // already isolates for the unchecked splice — reused here as a
        // read-only classification, never mutating on a `false` result).
        let Some(chunk_index) = self.find_sole_containing_original(start, end) else {
            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
        };

        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };
        let anchors_ref: &'a [SegmentAnchor] = if anchors.is_empty() {
            &[]
        } else {
            self.allocator.alloc_slice_copy(anchors)
        };

        self.splice_segmented_fast(chunk_index, start, end, content_ref, anchors_ref);
        self.output_delta += content_ref.len() as i64 - (end - start) as i64;
        Ok(self)
    }

    /// Read-only classification mirroring `try_fast_overwrite`'s own
    /// precondition: `[start, end)` falls entirely inside exactly one live
    /// `Original` chunk. Returns that chunk's index, never mutating.
    fn find_sole_containing_original(&self, start: u32, end: u32) -> Option<usize> {
        let search_start = self.search_start_for(start);
        for i in search_start..self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    if ce <= start {
                        continue;
                    }
                    if cs > start {
                        return None;
                    }
                    if ce < end {
                        return None;
                    }
                    return Some(i);
                }
                Chunk::Overwritten { .. }
                | Chunk::OverwrittenSegmented { .. }
                | Chunk::Moved { .. }
                | Chunk::Inserted { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    if let Some(cs) = chunk_start(&self.chunks[i]) {
                        if cs > start {
                            return None;
                        }
                    }
                }
            }
        }
        None
    }

    /// Split the single containing `Original` chunk at `chunk_index` into
    /// up to 3 pieces — `[cs, start)`, the new `OverwrittenSegmented`, and
    /// `[end, ce)` — mirroring `try_fast_overwrite`'s exact 4-way split,
    /// specialized to always know it is splitting an `Original` chunk (the
    /// only case `find_sole_containing_original` returns `Some` for).
    fn splice_segmented_fast(
        &mut self,
        chunk_index: usize,
        start: u32,
        end: u32,
        content_ref: &'a str,
        anchors_ref: &'a [SegmentAnchor],
    ) {
        let Chunk::Original { start: cs, end: ce } = self.chunks[chunk_index] else {
            unreachable!("find_sole_containing_original only returns Original chunk indices")
        };
        let new_chunk = Chunk::overwritten_segmented(start, end, content_ref, anchors_ref);
        match (cs < start, ce > end) {
            (true, true) => {
                self.chunks[chunk_index] = Chunk::from_source(cs, start);
                self.chunks.insert(chunk_index + 1, new_chunk);
                self.chunks
                    .insert(chunk_index + 2, Chunk::from_source(end, ce));
                self.cursor_hint = chunk_index + 1;
            }
            (false, true) => {
                self.chunks[chunk_index] = new_chunk;
                self.chunks
                    .insert(chunk_index + 1, Chunk::from_source(end, ce));
                self.cursor_hint = chunk_index;
            }
            (true, false) => {
                self.chunks[chunk_index] = Chunk::from_source(cs, start);
                self.chunks.insert(chunk_index + 1, new_chunk);
                self.cursor_hint = chunk_index + 1;
            }
            (false, false) => {
                self.chunks[chunk_index] = new_chunk;
                self.cursor_hint = chunk_index;
            }
        }
    }
}

/// The chunk's own start position, for chunk kinds that carry one — used
/// only by `find_sole_containing_original`'s conservative "did we already
/// pass a positioned chunk that starts before `start`" check on non-Original
/// chunks it walks over on the way to finding the target (mirrors
/// `try_fast_overwrite`'s own `Overwritten`-chunk handling, generalized to
/// the one additional variant this module introduces).
fn chunk_start(chunk: &Chunk<'_>) -> Option<u32> {
    match *chunk {
        Chunk::Overwritten { start, .. } | Chunk::OverwrittenSegmented { start, .. } => Some(start),
        _ => None,
    }
}

#[cfg(test)]
#[path = "segmented_tests.rs"]
mod tests;
