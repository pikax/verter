use std::ops::Range;

use verter_span::Span;

use super::fragment::{EmissionFragment, FragmentToken, MapHints, PreambleAnchor};
use super::piece::{MarkerId, Piece, PieceText, Provenance, TextRef};
use super::source::QualifiedSource;
use super::{note_map_capture, EmissionError, MapMode};
use crate::assembly::map_input::DecodedFragmentMap;

/// The one way to construct a fragment directly. Append-only: nothing
/// pushed can be changed or removed afterwards.
///
/// Under [`MapMode::NoMap`] the anchored and rewritten pushes store
/// unmapped pieces and read neither their anchor nor their span, external
/// maps are dropped unread, and no map hint is kept.
pub struct EmissionBuilder<'a> {
    mode: MapMode,
    source: Option<QualifiedSource<'a>>,
    pieces: Vec<Piece<'a>>,
    owned: Vec<String>,
    /// Index in `owned` of the buffer small generated texts are copied into.
    append_buffer: Option<u32>,
    len: u32,
    /// The first refusal; `finish` reports it.
    failure: Option<EmissionError>,
    hints: Option<Box<MapHints>>,
    external: Vec<DecodedFragmentMap>,
}

impl<'a> EmissionBuilder<'a> {
    pub fn new(mode: MapMode) -> Self {
        Self {
            mode,
            source: None,
            pieces: Vec::new(),
            owned: Vec::new(),
            append_buffer: None,
            len: 0,
            failure: None,
            hints: match mode {
                MapMode::NoMap => None,
                MapMode::Map => Some(Box::default()),
            },
            external: Vec::new(),
        }
    }

    /// The source whose bytes [`Self::push_source`] copies and whose
    /// offsets the anchored and rewritten pushes name. A builder binds one
    /// source: rebinding it to another refuses the fragment
    /// ([`EmissionError::ForeignSource`]), since pieces already admitted
    /// were checked against the first.
    pub fn with_source(mut self, source: QualifiedSource<'a>) -> Self {
        match &self.source {
            Some(bound) if !bound.key.same(&source.key) => {
                self.fail(EmissionError::ForeignSource);
            }
            Some(_) => {}
            None => self.source = Some(source),
        }
        self
    }

    pub fn mode(&self) -> MapMode {
        self.mode
    }

    /// The source this builder's authored pieces name, once one is set.
    pub fn source(&self) -> Option<&QualifiedSource<'a>> {
        self.source.as_ref()
    }

    /// Bytes pushed so far.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Authored bytes carried through unchanged (`Exact`). The span must lie
    /// inside the builder's source on UTF-8 boundaries.
    pub fn push_source(&mut self, span: Span) -> Result<(), EmissionError> {
        self.check_source_span(span)?;
        if span.start < span.end {
            self.push_piece(Piece::new(
                PieceText::Source(span),
                Provenance::Exact {
                    source_start: span.start,
                },
            ));
        }
        Ok(())
    }

    /// Replacement text standing for `source`. Empty text records a
    /// removal of `source` under `MapMode::Map` and nothing otherwise.
    pub fn push_rewritten(&mut self, text: &str, source: Span) {
        match self.mode {
            MapMode::NoMap => self.push_copied(text, Provenance::Unmapped),
            MapMode::Map => {
                note_map_capture();
                if text.is_empty() {
                    self.push_piece(Piece::new(
                        PieceText::Empty,
                        Provenance::Rewritten { source },
                    ));
                } else {
                    self.push_copied(text, Provenance::Rewritten { source });
                }
            }
        }
    }

    /// Generated text whose first byte stands for `source_offset`.
    pub fn push_anchored(&mut self, text: &str, source_offset: u32) {
        let provenance = match self.mode {
            MapMode::NoMap => Provenance::Unmapped,
            MapMode::Map => {
                note_map_capture();
                Provenance::Anchor { source_offset }
            }
        };
        self.push_copied(text, provenance);
    }

    /// Synthetic text with no source.
    pub fn push_unmapped(&mut self, text: &str) {
        self.push_copied(text, Provenance::Unmapped);
    }

    /// Synthetic text that outlives the fragment, held by reference.
    pub fn push_borrowed(&mut self, text: &'a str) {
        if !text.is_empty() {
            self.push_piece(Piece::new(PieceText::Borrowed(text), Provenance::Unmapped));
        }
    }

    /// Synthetic text written straight into the builder's own buffer.
    pub fn push_unmapped_fmt(&mut self, args: std::fmt::Arguments<'_>) {
        use std::fmt::Write as _;
        let buffer = self.append_buffer_index();
        let target = &mut self.owned[buffer as usize];
        let start = target.len();
        // Writing into a `String` fails only when a `Display` implementation
        // inside `args` reports an error, as `format!` would.
        target
            .write_fmt(args)
            .expect("a Display implementation returned an error");
        let end = target.len();
        self.push_buffer_range(buffer, start, end, Provenance::Unmapped);
    }

    /// A declared map boundary: the bytes after it inherit no earlier
    /// mapping, even on a new line. Recorded only under `MapMode::Map`.
    pub fn push_map_stop(&mut self) {
        if self.mode == MapMode::Map {
            self.pieces
                .push(Piece::new(PieceText::Empty, Provenance::Unmapped));
        }
    }

    /// Take ownership of a generated buffer; its bytes are not copied. An
    /// `Exact` claim must hold byte for byte against the builder's source;
    /// `External` provenance enters only through [`Self::push_external`].
    pub fn push_owned(&mut self, text: String, provenance: Provenance) {
        let provenance = match (self.mode, provenance) {
            (_, Provenance::External(_)) => {
                verter_debug_assert!(false, "an external map enters through push_external");
                Provenance::Unmapped
            }
            (_, Provenance::Exact { source_start }) => {
                if !self.source_holds(source_start, &text) {
                    self.fail(EmissionError::InvalidSourceSlice {
                        span: span_after(source_start, text.len()),
                    });
                    return;
                }
                provenance
            }
            (MapMode::NoMap, _) => Provenance::Unmapped,
            (MapMode::Map, Provenance::Unmapped) => Provenance::Unmapped,
            (MapMode::Map, mapped) => {
                note_map_capture();
                mapped
            }
        };
        let end = text.len();
        let buffer = self.adopt_buffer(text);
        self.push_buffer_range(buffer, 0, end, provenance);
    }

    /// One owned buffer cut into pieces: each `(range, source_offset)` is a
    /// generated range anchored at `source_offset`; the bytes between ranges
    /// are unmapped. Ranges must be ordered, disjoint, inside `text` and on
    /// UTF-8 boundaries. Under `MapMode::NoMap` the ranges are not read and
    /// the buffer is one unmapped piece.
    pub fn push_owned_ranges(
        &mut self,
        text: String,
        anchored: &[(Range<u32>, u32)],
    ) -> Result<(), EmissionError> {
        if self.mode == MapMode::NoMap {
            let end = text.len();
            let buffer = self.adopt_buffer(text);
            self.push_buffer_range(buffer, 0, end, Provenance::Unmapped);
            return Ok(());
        }
        let mut cursor = 0usize;
        for (range, _) in anchored {
            let (start, end) = (range.start as usize, range.end as usize);
            if start < cursor
                || start > end
                || end > text.len()
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(end)
            {
                return Err(EmissionError::InvalidSourceSlice {
                    span: Span::new(range.start, range.end),
                });
            }
            cursor = end;
        }
        let len = text.len();
        let buffer = self.adopt_buffer(text);
        let mut cursor = 0usize;
        for (range, source_offset) in anchored {
            let (start, end) = (range.start as usize, range.end as usize);
            self.push_buffer_range(buffer, cursor, start, Provenance::Unmapped);
            note_map_capture();
            self.push_buffer_range(
                buffer,
                start,
                end,
                Provenance::Anchor {
                    source_offset: *source_offset,
                },
            );
            cursor = end;
        }
        self.push_buffer_range(buffer, cursor, len, Provenance::Unmapped);
        Ok(())
    }

    /// Text whose correspondence is an admitted external map, decoded
    /// against exactly this text. Under `MapMode::NoMap` the map is dropped
    /// unread and the text is unmapped.
    pub fn push_external(&mut self, text: &'a str, map: DecodedFragmentMap) {
        // An empty external fragment can still carry an end-position token
        // and source/name tables. Only text-only execution can discard it.
        if text.is_empty() && self.mode == MapMode::NoMap {
            return;
        }
        let provenance = match self.mode {
            MapMode::NoMap => Provenance::Unmapped,
            MapMode::Map => {
                note_map_capture();
                let index = self.external.len() as u32;
                self.external.push(map);
                Provenance::External(index)
            }
        };
        self.push_piece(Piece::new(PieceText::Borrowed(text), provenance));
    }

    /// Push `piece` as the declared position `marker`. The piece's text must
    /// be a source span of the builder's source or borrowed bytes.
    pub fn push_marked(&mut self, piece: Piece<'a>, marker: MarkerId) -> Result<(), EmissionError> {
        let piece = self.admit_piece(piece)?;
        self.push_piece(Piece {
            marker: Some(marker),
            ..piece
        });
        Ok(())
    }

    /// Nest a finished fragment: its pieces, buffers and external maps move
    /// in and no text is copied. Its source becomes the builder's source;
    /// a builder that already names a different source refuses it.
    pub fn append_fragment(&mut self, fragment: EmissionFragment<'a>) -> Result<(), EmissionError> {
        if fragment.mode != self.mode {
            return Err(EmissionError::ModeMismatch);
        }
        match (&self.source, &fragment.source) {
            (Some(own), Some(nested)) if !own.key.same(&nested.key) => {
                return Err(EmissionError::ForeignSource);
            }
            _ => {}
        }
        let at = self.len;
        let Some(len) = at.checked_add(fragment.len) else {
            return Err(EmissionError::Capacity);
        };
        let EmissionFragment {
            source,
            pieces,
            owned,
            hints,
            external,
            ..
        } = fragment;
        if self.source.is_none() {
            self.source = source;
        }
        let buffer_base = self.owned.len() as u32;
        let external_base = self.external.len() as u32;
        self.owned.extend(owned);
        self.external.extend(external);
        self.pieces.extend(pieces.into_iter().map(|piece| Piece {
            text: match piece.text {
                PieceText::Owned(text) => PieceText::Owned(TextRef {
                    buffer: text.buffer + buffer_base,
                    ..text
                }),
                text => text,
            },
            provenance: match piece.provenance {
                Provenance::External(index) => Provenance::External(index + external_base),
                provenance => provenance,
            },
            ..piece
        }));
        if let (Some(own), Some(nested)) = (self.hints.as_deref_mut(), hints) {
            let nested = *nested;
            own.locations.extend(nested.locations);
            own.unmapped_boundaries
                .extend(nested.unmapped_boundaries.iter().map(|offset| offset + at));
            if own.preamble.is_none() {
                own.preamble = nested.preamble.map(|preamble| PreambleAnchor {
                    generated_end: preamble.generated_end + at,
                    ..preamble
                });
            }
        }
        self.len = len;
        Ok(())
    }

    /// Append pieces whose text is a span of the builder's source or bytes
    /// borrowed for `'a`. A transform's freeze hands its chunks over this
    /// way; owned and external pieces enter only through their own pushes.
    pub fn append_pieces(
        &mut self,
        pieces: impl IntoIterator<Item = Piece<'a>>,
    ) -> Result<(), EmissionError> {
        for piece in pieces {
            let piece = self.admit_piece(piece)?;
            self.push_piece(piece);
        }
        Ok(())
    }

    /// An authored offset that receives its own map token where exact text
    /// covers it. Recorded only under `MapMode::Map`.
    pub fn hint_location(&mut self, source_offset: u32) {
        if let Some(hints) = self.hints.as_deref_mut() {
            note_map_capture();
            hints.locations.push(source_offset);
        }
    }

    /// An ownership transition at the builder's current length plus
    /// `offset` (inside the piece pushed next), reported to range consumers.
    /// It is not a map stop: the V3 product places tokens by piece, as the
    /// transform's own map does. Recorded only under `MapMode::Map`.
    pub fn hint_unmapped_boundary(&mut self, offset: u32) {
        let at = self.len.saturating_add(offset);
        if let Some(hints) = self.hints.as_deref_mut() {
            note_map_capture();
            hints.unmapped_boundaries.push(at);
        }
    }

    /// A leading helper preamble ends at the builder's current length.
    /// Recorded only under `MapMode::Map`; the first record wins.
    pub fn hint_preamble_end(&mut self, carrier: Option<u32>) {
        let generated_end = self.len;
        if let Some(hints) = self.hints.as_deref_mut() {
            note_map_capture();
            hints.preamble.get_or_insert(PreambleAnchor {
                generated_end,
                carrier,
            });
        }
    }

    pub fn finish(self) -> Result<EmissionFragment<'a>, EmissionError> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let mut hints = self.hints;
        if let Some(hints) = hints.as_deref_mut() {
            hints.normalise();
        }
        Ok(EmissionFragment {
            source: self.source,
            pieces: self.pieces,
            owned: self.owned,
            len: self.len,
            mode: self.mode,
            hints,
            external: self.external,
            token: FragmentToken::fresh(),
        })
    }

    fn check_source_span(&self, span: Span) -> Result<(), EmissionError> {
        let (start, end) = (span.start as usize, span.end as usize);
        let valid = self.source.as_ref().is_some_and(|source| {
            start <= end
                && end <= source.text.len()
                && source.text.is_char_boundary(start)
                && source.text.is_char_boundary(end)
        });
        if valid {
            Ok(())
        } else {
            Err(EmissionError::InvalidSourceSlice { span })
        }
    }

    fn source_holds(&self, source_start: u32, text: &str) -> bool {
        let start = source_start as usize;
        self.source.as_ref().is_some_and(|source| {
            start
                .checked_add(text.len())
                .and_then(|end| source.text.get(start..end))
                == Some(text)
        })
    }

    /// Check a piece handed over whole and settle its provenance for the
    /// builder's map policy.
    fn admit_piece(&self, piece: Piece<'a>) -> Result<Piece<'a>, EmissionError> {
        match piece.text {
            PieceText::Source(span) => self.check_source_span(span)?,
            PieceText::Borrowed(_) | PieceText::Empty => {}
            PieceText::Owned(_) => {
                verter_debug_assert!(false, "an owned buffer enters through push_owned");
                return Err(EmissionError::InvalidSourceSlice {
                    span: Span::new(0, 0),
                });
            }
        }
        let provenance = match (self.mode, piece.provenance) {
            (_, Provenance::External(_)) => {
                verter_debug_assert!(false, "an external map enters through push_external");
                return Err(EmissionError::MapNotCaptured);
            }
            (MapMode::NoMap, Provenance::Rewritten { .. } | Provenance::Anchor { .. }) => {
                Provenance::Unmapped
            }
            (_, provenance) => provenance,
        };
        Ok(Piece {
            provenance,
            ..piece
        })
    }

    fn push_copied(&mut self, text: &str, provenance: Provenance) {
        if text.is_empty() {
            return;
        }
        let buffer = self.append_buffer_index();
        let target = &mut self.owned[buffer as usize];
        let start = target.len();
        target.push_str(text);
        let end = target.len();
        self.push_buffer_range(buffer, start, end, provenance);
    }

    fn append_buffer_index(&mut self) -> u32 {
        match self.append_buffer {
            Some(buffer) => buffer,
            None => {
                let buffer = self.adopt_buffer(String::new());
                self.append_buffer = Some(buffer);
                buffer
            }
        }
    }

    fn adopt_buffer(&mut self, text: String) -> u32 {
        super::note_text_buffer();
        self.owned.push(text);
        (self.owned.len() - 1) as u32
    }

    fn push_buffer_range(&mut self, buffer: u32, start: usize, end: usize, provenance: Provenance) {
        if start >= end {
            return;
        }
        match (u32::try_from(start), u32::try_from(end - start)) {
            (Ok(start), Ok(len)) => self.push_piece(Piece::new(
                PieceText::Owned(TextRef { buffer, start, len }),
                provenance,
            )),
            _ => self.fail(EmissionError::Capacity),
        }
    }

    fn push_piece(&mut self, piece: Piece<'a>) {
        let fits = match piece.text {
            PieceText::Borrowed(text) => u32::try_from(text.len()).is_ok(),
            _ => true,
        };
        match self.len.checked_add(piece.len()) {
            Some(len) if fits => {
                self.len = len;
                self.pieces.push(piece);
            }
            _ => self.fail(EmissionError::Capacity),
        }
    }

    fn fail(&mut self, failure: EmissionError) {
        self.failure.get_or_insert(failure);
    }
}

fn span_after(start: u32, len: usize) -> Span {
    Span::new(
        start,
        u32::try_from(len)
            .ok()
            .and_then(|len| start.checked_add(len))
            .unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
