//! Additive segmented overwrite.
//!
//! `Chunk::Overwritten` emits at most one source-map token for the whole
//! replacement (MagicString: no character-level correspondence). Wrong
//! when generated scaffolding embeds authored lexemes at known offsets
//! (interpolation inside `_toDisplayString(...)`).
//!
//! `try_overwrite_segmented` is a separate crate-private entry; it does
//! not change `overwrite`/`update`. Target range must lie in one live
//! `Original` chunk with no affinity-anchored insertion — otherwise a
//! typed refusal, not a best-effort splice.

use super::chunk::Chunk;
use super::code_transform::CodeTransform;
use super::fallible::CodeTransformError;
use crate::template::code_gen::types::SegmentedOverwriteAuthority;

/// Authored lexeme inside a segmented overwrite:
/// `content[content_offset..content_offset + length]` maps to
/// `[source_pos, source_pos + length)`. Bytes outside anchors are synthetic
/// (no source-map token).
///
/// `pub` to match the `pub` carriers that hold it (`VaporTextPart::Dynamic`,
/// …). Operations that produce a `SegmentAnchor` chunk stay restricted to
/// Vue runtime emitters ([`SegmentedOverwriteAuthority`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentAnchor {
    pub content_offset: u32,
    pub length: u32,
    pub source_pos: u32,
}

impl SegmentAnchor {
    /// Test-only; production builds a struct literal from AST/segment-plan data.
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
    /// Checked segmented overwrite. Fails atomically (no mutation on `Err`)
    /// for a malformed/empty range, out-of-order or overlapping anchors,
    /// out-of-range/`source_pos` mid-character, a target not entirely in one
    /// live `Original` chunk, or any affinity-anchored insertion.
    ///
    /// `_authority` is the static call-site guard ([`SegmentedOverwriteAuthority`]).
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
