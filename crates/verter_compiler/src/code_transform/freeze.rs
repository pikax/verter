//! Hand a finished transform over as an immutable emission fragment.
//!
//! One pass over the intro, the chunks (relocated runs expanded) and the
//! outro turns every chunk into pieces that keep what its bytes mean:
//! authored bytes stay exact, a replacement stays rewritten unless it was
//! recorded as synthetic, a mapped insertion becomes synthetic text plus an
//! anchored run, a segmented overwrite splits at its anchors. No text is
//! copied: pieces name the source or borrow the transform's arena, which the
//! fragment's lifetime already binds.

use std::ops::Range;

use smallvec::SmallVec;
use verter_span::Span;

use super::chunk::Chunk;
use super::code_transform::{CodeTransform, GeneratedContentMarker};
use super::fallible::CodeTransformError;
use super::segmented::AnchorKind;
use crate::emission::{
    EmissionBuilder, EmissionError, EmissionFragment, MapMode, MarkerId, Piece, PieceText,
    Provenance, QualifiedSource,
};

/// Why a transform refused to freeze with declared positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MarkedFreezeError {
    /// The transform itself refused to freeze.
    Transform(CodeTransformError),
    /// No chunk of the transform holds the marker's content, or more than
    /// one does.
    UnresolvedMarker(MarkerId),
    /// The marker's range covers authored or anchored bytes, or more than
    /// one run of its chunk, so no part of one generated run stands for it.
    MarkerOutsideGeneratedRun(MarkerId),
    /// The marker's range partly overlaps another marker's.
    OverlappingMarkers(MarkerId),
}

impl From<CodeTransformError> for MarkedFreezeError {
    fn from(error: CodeTransformError) -> Self {
        Self::Transform(error)
    }
}

/// A declared position resolved to its output bytes.
type ResolvedMark = (Range<u32>, MarkerId);

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// Consume the transform into a fragment over `source`, which must be
    /// this transform's own source text. The fragment keeps the transform's
    /// map policy; under [`MapMode::NoMap`] it carries no map facts.
    pub fn freeze(
        self,
        source: QualifiedSource<'a>,
    ) -> Result<EmissionFragment<'a>, CodeTransformError> {
        self.freeze_with(source, |transform, builder, pieces| {
            builder
                .append_pieces(pieces.drain(..))
                .map_err(|error| transform.emission_refusal(error))
        })
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// [`Self::freeze`], with each marker a producer registered on this
    /// transform exposed as the fragment slot `id`
    /// ([`EmissionFragment::slot`]). A marker resolves to its chunk by
    /// content-pointer identity, exactly as
    /// [`Self::generated_content_range`] resolves it; the run it covers
    /// becomes a piece of its own. The marker lies in synthetic text or in a
    /// replacement, and each part of a split replacement keeps standing for
    /// the replaced span. A marker whose range lies inside another
    /// marker's has no slot of its own: the enclosing slot's fill replaces
    /// its bytes too.
    ///
    /// # Errors
    ///
    /// [`MarkedFreezeError`]: a marker no chunk (or more than one) holds, a
    /// marker over text that keeps a source correspondence, two markers
    /// that partly overlap, or the transform's own freeze refusal.
    pub(crate) fn freeze_marked(
        self,
        source: QualifiedSource<'a>,
        marks: &[(GeneratedContentMarker<'a>, MarkerId)],
    ) -> Result<EmissionFragment<'a>, MarkedFreezeError> {
        let mut pending = self.resolve_marks(marks)?.into_iter().peekable();
        let fragment = self.freeze_with(source, |transform, builder, pieces| {
            transform.place_marked(builder, pieces, &mut pending)
        })?;
        match pending.next() {
            Some((_, id)) => Err(MarkedFreezeError::UnresolvedMarker(id)),
            None => Ok(fragment),
        }
    }

    /// Every marker's output range, in output order, without the markers
    /// another marker's range already contains.
    fn resolve_marks(
        &self,
        marks: &[(GeneratedContentMarker<'a>, MarkerId)],
    ) -> Result<SmallVec<[ResolvedMark; 4]>, MarkedFreezeError> {
        let mut resolved: SmallVec<[ResolvedMark; 4]> = SmallVec::with_capacity(marks.len());
        for &(marker, id) in marks {
            let range = self
                .generated_content_range(marker)
                .ok_or(MarkedFreezeError::UnresolvedMarker(id))?;
            resolved.push((range, id));
        }
        resolved.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.end)));
        let mut kept: SmallVec<[ResolvedMark; 4]> = SmallVec::with_capacity(resolved.len());
        for (range, id) in resolved {
            if let Some((outer, _)) = kept.last() {
                let inside =
                    range.end <= outer.end && (range.start < outer.end || outer.is_empty());
                if inside {
                    continue;
                }
                if range.start < outer.end {
                    return Err(MarkedFreezeError::OverlappingMarkers(id));
                }
            }
            kept.push((range, id));
        }
        Ok(kept)
    }

    /// Append one chunk's pieces, splitting a generated piece around each
    /// pending marker it holds and pushing the marked run as the slot.
    fn place_marked(
        &self,
        builder: &mut EmissionBuilder<'a>,
        pieces: &mut SmallVec<[Piece<'a>; 4]>,
        pending: &mut std::iter::Peekable<smallvec::IntoIter<[ResolvedMark; 4]>>,
    ) -> Result<(), MarkedFreezeError> {
        for piece in pieces.drain(..) {
            let start = builder.len();
            let end = start.saturating_add(piece.len());
            // Synthetic or replacement text: its parts keep its
            // provenance. Authored and anchored runs are not split.
            let splittable = matches!(piece.text, PieceText::Borrowed(_) | PieceText::Empty)
                && matches!(
                    piece.provenance,
                    Provenance::Unmapped | Provenance::Rewritten { .. }
                );
            let text = self.piece_text(&piece);
            let local = |at: u32| (at - start) as usize;
            let split = |text: &'a str| Piece {
                text: borrowed(text),
                marker: None,
                ..piece
            };
            let mut rest = start;
            while let Some((range, id)) = pending.next_if(|(range, _)| {
                range.start < end || (splittable && range.is_empty() && range.start == end)
            }) {
                if !splittable || range.end > end {
                    return Err(MarkedFreezeError::MarkerOutsideGeneratedRun(id));
                }
                let (Some(before), Some(marked)) = (
                    text.get(local(rest)..local(range.start)),
                    text.get(local(range.start)..local(range.end)),
                ) else {
                    return Err(MarkedFreezeError::MarkerOutsideGeneratedRun(id));
                };
                if !before.is_empty() {
                    builder
                        .append_pieces([split(before)])
                        .map_err(|error| self.emission_refusal(error))?;
                }
                builder
                    .push_marked(split(marked), id)
                    .map_err(|error| self.emission_refusal(error))?;
                rest = range.end;
            }
            let remainder = if rest == start {
                Some(piece)
            } else {
                text.get(local(rest)..)
                    .filter(|after| !after.is_empty())
                    .map(split)
            };
            if let Some(remainder) = remainder {
                builder
                    .append_pieces([remainder])
                    .map_err(|error| self.emission_refusal(error))?;
            }
        }
        Ok(())
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// The one freeze walk: intro, each chunk's pieces handed to `place`,
    /// outro.
    fn freeze_with<E: From<CodeTransformError>>(
        self,
        source: QualifiedSource<'a>,
        mut place: impl FnMut(
            &Self,
            &mut EmissionBuilder<'a>,
            &mut SmallVec<[Piece<'a>; 4]>,
        ) -> Result<(), E>,
    ) -> Result<EmissionFragment<'a>, E> {
        if !std::ptr::eq(source.text, self.original) {
            return Err(CodeTransformError::ForeignSource.into());
        }
        let mapped = self.map_mode() == MapMode::Map;
        let mut builder = EmissionBuilder::new(self.map_mode()).with_source(source);
        if mapped {
            let mut locations = self.sourcemap_locations().to_vec();
            locations.sort_unstable();
            locations.dedup();
            for location in locations {
                builder.hint_location(location);
            }
        }

        let preamble = self.helper_preamble_content().filter(|_| mapped);
        let is_preamble = |content: &str| {
            preamble.is_some_and(|preamble| {
                std::ptr::eq(preamble.as_ptr(), content.as_ptr()) && preamble.len() == content.len()
            })
        };

        let intro_is_preamble = self.sole_intro_piece().is_some_and(is_preamble);
        builder
            .append_pieces(synthetic_sequence(self.intro(), self.map_mode()))
            .map_err(|error| E::from(self.emission_refusal(error)))?;
        if intro_is_preamble {
            builder.hint_preamble_end(self.helper_preamble_carrier_anchor());
        }

        let mut pieces: SmallVec<[Piece<'a>; 4]> = SmallVec::new();
        for (chunk, relocated) in self.leaf_chunks() {
            let content = chunk_content(chunk);
            if mapped {
                for &offset in
                    content.map_or(&[][..], |content| self.unmapped_boundaries_in(content))
                {
                    builder.hint_unmapped_boundary(offset);
                }
            }
            pieces.clear();
            self.leaf_pieces(chunk, relocated, &mut |piece| pieces.push(piece));
            place(&self, &mut builder, &mut pieces)?;
            if content.is_some_and(is_preamble) {
                builder.hint_preamble_end(self.helper_preamble_carrier_anchor());
            }
        }

        builder
            .append_pieces(synthetic_sequence(self.outro(), self.map_mode()))
            .map_err(|error| E::from(self.emission_refusal(error)))?;
        builder
            .finish()
            .map_err(|error| E::from(self.emission_refusal(error)))
    }

    /// The bytes of a piece this transform made.
    pub(super) fn piece_text(&self, piece: &Piece<'a>) -> &'a str {
        match piece.text() {
            PieceText::Source(span) => &self.original[span.start as usize..span.end as usize],
            PieceText::Borrowed(text) => text,
            PieceText::Empty => "",
            PieceText::Owned(_) => unreachable!("a transform's pieces own no buffer"),
        }
    }

    /// The pieces one leaf chunk (never a relocated run) becomes, in output
    /// order, each marked relocated when the chunk sits in a relocated run.
    pub(super) fn leaf_pieces(
        &self,
        chunk: &Chunk<'a>,
        relocated: bool,
        out: &mut impl FnMut(Piece<'a>),
    ) {
        let mut emit = |text: PieceText<'a>, provenance: Provenance| {
            let mut piece = Piece::new(text, provenance);
            piece.relocated = relocated;
            out(piece);
        };
        match *chunk {
            Chunk::Original { start, end } => emit(
                PieceText::Source(Span::new(start, end)),
                Provenance::Exact {
                    source_start: start,
                },
            ),
            Chunk::Overwritten {
                start,
                end,
                content,
            } => {
                let provenance = if self.is_unmapped_overwrite(content) {
                    Provenance::Unmapped
                } else {
                    Provenance::Rewritten {
                        source: Span::new(start, end),
                    }
                };
                emit(borrowed(content), provenance);
            }
            Chunk::InsertedMapped {
                content,
                source_start,
                content_offset,
            } => {
                let offset = (content_offset as usize).min(content.len());
                if offset > 0 {
                    for piece in synthetic_pieces(&content[..offset], self.map_mode()) {
                        emit(piece.text, piece.provenance);
                    }
                }
                if offset < content.len() {
                    emit(
                        borrowed(&content[offset..]),
                        Provenance::Anchor {
                            source_offset: source_start,
                        },
                    );
                }
            }
            Chunk::OverwrittenSegmented {
                content, anchors, ..
            } => {
                let mut cursor = 0usize;
                for anchor in anchors {
                    let start = anchor.content_offset as usize;
                    let end = start + anchor.length as usize;
                    if start > cursor {
                        emit(borrowed(&content[cursor..start]), Provenance::Unmapped);
                    }
                    if end > start {
                        match anchor.kind {
                            // Checked equal to the source when recorded, so the
                            // piece names the source bytes themselves.
                            AnchorKind::Exact => emit(
                                PieceText::Source(Span::new(
                                    anchor.source_pos,
                                    anchor.source_pos + anchor.length,
                                )),
                                Provenance::Exact {
                                    source_start: anchor.source_pos,
                                },
                            ),
                            AnchorKind::Point => emit(
                                borrowed(&content[start..end]),
                                Provenance::Anchor {
                                    source_offset: anchor.source_pos,
                                },
                            ),
                        }
                    }
                    cursor = cursor.max(end);
                }
                if cursor < content.len() {
                    emit(borrowed(&content[cursor..]), Provenance::Unmapped);
                }
            }
            Chunk::Inserted { content } | Chunk::InsertedAnchored { content, .. } => {
                for piece in synthetic_pieces(content, self.map_mode()) {
                    emit(piece.text, piece.provenance);
                }
            }
            Chunk::Relocated { .. } => unreachable!("the leaf walk expands relocated runs"),
        }
    }

    /// A fragment refusal of this transform's own pieces, in the transform's
    /// error type.
    fn emission_refusal(&self, error: EmissionError) -> CodeTransformError {
        match error {
            EmissionError::InvalidSourceSlice { span } => CodeTransformError::OutOfRange {
                offset: span.start,
                len: self.original.len() as u32,
            },
            EmissionError::Capacity => CodeTransformError::Capacity {
                len: self.original.len(),
            },
            EmissionError::MapNotCaptured => CodeTransformError::MapNotCaptured,
            EmissionError::UnknownMarker(_)
            | EmissionError::ForeignSource
            | EmissionError::ModeMismatch => CodeTransformError::ForeignSource,
        }
    }
}

/// Synthetic insertions declare their own boundary, even at a line end or
/// when no authored mapping is active. Keep that zero-width event alongside
/// the text so direct maps and frozen fragments read the same wire policy.
pub(super) fn synthetic_pieces(text: &str, mode: MapMode) -> impl Iterator<Item = Piece<'_>> {
    synthetic_sequence(std::iter::once(text), mode)
}

/// Intro/outro buffers form one synthetic insertion, regardless of how many
/// borrowed pieces hold their bytes. Buffer splits introduce no map events.
pub(super) fn synthetic_sequence<'a>(
    texts: impl Iterator<Item = &'a str>,
    mode: MapMode,
) -> impl Iterator<Item = Piece<'a>> {
    texts
        .filter(|text| !text.is_empty())
        .enumerate()
        .flat_map(move |(index, text)| {
            [
                (index == 0 && mode == MapMode::Map)
                    .then(|| Piece::new(PieceText::Empty, Provenance::Unmapped)),
                Some(Piece::new(borrowed(text), Provenance::Unmapped)),
            ]
            .into_iter()
            .flatten()
        })
}

/// The generated text a chunk holds, or `None` for authored source.
pub(super) fn chunk_content<'a>(chunk: &Chunk<'a>) -> Option<&'a str> {
    match *chunk {
        Chunk::Original { .. } | Chunk::Relocated { .. } => None,
        Chunk::Inserted { content }
        | Chunk::Overwritten { content, .. }
        | Chunk::OverwrittenSegmented { content, .. }
        | Chunk::InsertedMapped { content, .. }
        | Chunk::InsertedAnchored { content, .. } => Some(content),
    }
}

fn borrowed(text: &str) -> PieceText<'_> {
    if text.is_empty() {
        PieceText::Empty
    } else {
        PieceText::Borrowed(text)
    }
}

#[cfg(test)]
#[path = "freeze_tests.rs"]
mod tests;
