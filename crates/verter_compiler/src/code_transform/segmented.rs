//! Additive segmented overwrite.
//!
//! `Chunk::Overwritten` emits at most one source-map token for the whole
//! replacement (MagicString: no character-level correspondence). Wrong
//! when generated scaffolding embeds authored lexemes at known offsets
//! (interpolation inside `_toDisplayString(...)`).
//!
//! The correspondence between the replacement and the source is a claim the
//! producer makes and this owner checks: [`CodeTransform::check_correspondence`]
//! is the only way to obtain a [`SegmentedCorrespondence`], and it refuses
//! anchors that are out of order, out of bounds, off a character boundary,
//! or that claim exact bytes the source does not hold. The target range of
//! [`CodeTransform::try_overwrite_segmented`] must lie in one live,
//! unedited `Original` chunk with no affinity-anchored insertion — otherwise
//! a typed refusal, not a best-effort splice.

use verter_span::Span;

use super::chunk::Chunk;
use super::code_transform::CodeTransform;
use super::fallible::CodeTransformError;

/// What an anchor claims about its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// The anchored bytes are the authored bytes at `source_pos`, copied
    /// unchanged: `content[anchor] == source[source_pos..][..length]`.
    Exact,
    /// The anchored bytes stand for the authored position `source_pos`
    /// without being a copy of it (rewritten text, a delimiter that marks
    /// where an authored construct starts).
    Point,
}

/// Authored correspondence inside a segmented overwrite:
/// `content[content_offset..content_offset + length]` maps to `source_pos`,
/// byte for byte when `kind` is [`AnchorKind::Exact`]. Bytes outside anchors
/// are synthetic (no source-map token).
///
/// `pub` to match the `pub` carriers that hold it (`VaporTextPart::Dynamic`,
/// …). A `SegmentAnchor` reaches a chunk only through
/// [`CodeTransform::check_correspondence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentAnchor {
    pub content_offset: u32,
    pub length: u32,
    pub source_pos: u32,
    pub kind: AnchorKind,
}

impl SegmentAnchor {
    /// Test-only; production builds a struct literal from AST/segment-plan data.
    #[cfg(test)]
    pub(crate) fn new(content_offset: u32, length: u32, source_pos: u32, kind: AnchorKind) -> Self {
        Self {
            content_offset,
            length,
            source_pos,
            kind,
        }
    }
}

/// A replacement and its anchors, checked against one transform's source by
/// [`CodeTransform::check_correspondence`]. The fields are private: the
/// checked constructor is the only way to build one.
#[derive(Debug, Clone, Copy)]
pub struct SegmentedCorrespondence<'c> {
    content: &'c str,
    anchors: &'c [SegmentAnchor],
    checked_against: &'c str,
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// Check a producer's correspondence claim: anchors in content order and
    /// non-overlapping, inside `content` on character boundaries, with
    /// arithmetic that stays in `u32`; every source position inside this
    /// transform's source on a character boundary; and every
    /// [`AnchorKind::Exact`] anchor naming source bytes equal to its own.
    pub(crate) fn check_correspondence<'c>(
        &self,
        content: &'c str,
        anchors: &'c [SegmentAnchor],
    ) -> Result<SegmentedCorrespondence<'c>, CodeTransformError>
    where
        'a: 'c,
    {
        let content_len =
            u32::try_from(content.len()).map_err(|_| CodeTransformError::OutOfRange {
                offset: u32::MAX,
                len: u32::MAX,
            })?;
        let mut prev_end = 0u32;
        for anchor in anchors {
            if anchor.content_offset < prev_end {
                return Err(CodeTransformError::ReversedRange {
                    start: anchor.content_offset,
                    end: prev_end,
                });
            }
            let anchor_end = anchor
                .content_offset
                .checked_add(anchor.length)
                .filter(|end| *end <= content_len)
                .ok_or(CodeTransformError::OutOfRange {
                    offset: anchor.content_offset,
                    len: content_len,
                })?;
            if !content.is_char_boundary(anchor.content_offset as usize) {
                return Err(CodeTransformError::MidChar {
                    offset: anchor.content_offset,
                });
            }
            if !content.is_char_boundary(anchor_end as usize) {
                return Err(CodeTransformError::MidChar { offset: anchor_end });
            }
            self.check_offset(anchor.source_pos)?;
            if anchor.kind == AnchorKind::Exact {
                let not_exact = CodeTransformError::CorrespondenceNotExact {
                    content_offset: anchor.content_offset,
                    source_pos: anchor.source_pos,
                };
                let source_end = anchor
                    .source_pos
                    .checked_add(anchor.length)
                    .ok_or(not_exact)?;
                self.check_offset(source_end).map_err(|_| not_exact)?;
                if content[anchor.content_offset as usize..anchor_end as usize]
                    != self.original[anchor.source_pos as usize..source_end as usize]
                {
                    return Err(not_exact);
                }
            }
            prev_end = anchor_end;
        }
        Ok(SegmentedCorrespondence {
            content,
            anchors,
            checked_against: self.original,
        })
    }

    /// Checked segmented overwrite of `[start, end)` with a checked
    /// correspondence. Fails atomically (no mutation on `Err`) for a
    /// malformed/empty range, a target not entirely in one live unedited
    /// `Original` chunk, or any affinity-anchored insertion. A
    /// correspondence checked against another source is checked again here.
    pub(crate) fn try_overwrite_segmented(
        &mut self,
        start: u32,
        end: u32,
        correspondence: SegmentedCorrespondence<'_>,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.check_range(start, end)?;
        if start == end {
            return Err(CodeTransformError::ZeroLengthRange { offset: start });
        }
        let SegmentedCorrespondence {
            content,
            anchors,
            checked_against,
        } = correspondence;
        if !std::ptr::eq(checked_against, self.original) {
            self.check_correspondence(content, anchors)?;
        }
        let not_live = CodeTransformError::SegmentedTargetNotLive {
            range: Span::new(start, end),
        };
        if self.anchored_present {
            return Err(not_live);
        }

        // Narrow precondition: the range must fall entirely inside ONE
        // live `Original` chunk (the exact shape `try_fast_overwrite`
        // already isolates for the unchecked splice — reused here as a
        // read-only classification, never mutating on a `false` result).
        let Some(chunk_index) = self.find_sole_containing_original(start, end) else {
            return Err(not_live);
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
                | Chunk::Relocated { .. }
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
