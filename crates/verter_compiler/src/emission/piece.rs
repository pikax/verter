use std::sync::atomic::{AtomicU64, Ordering};

use verter_span::Span;

/// A byte range in one of a fragment's owned generated-text buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRef {
    pub buffer: u32,
    pub start: u32,
    pub len: u32,
}

impl TextRef {
    pub(crate) fn range(self) -> std::ops::Range<usize> {
        let start = self.start as usize;
        start..start + self.len as usize
    }
}

/// Identity of a producer-declared position inside generated text. A
/// fragment exposes it as a [`super::FragmentSlot`]; it is never found by
/// searching text. Identities are unique in the process, so fragments
/// nested into one another never share a marker.
#[derive(Debug, Clone, Copy, Eq)]
pub struct MarkerId(pub(crate) u64);

impl PartialEq for MarkerId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl std::hash::Hash for MarkerId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl MarkerId {
    /// A marker no other fragment holds.
    pub fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Where a piece's bytes live. No variant owns text by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceText<'a> {
    /// Bytes of the fragment's own source.
    Source(Span),
    /// Bytes in an arena or a static that outlives the fragment.
    Borrowed(&'a str),
    /// Bytes in a buffer the fragment owns.
    Owned(TextRef),
    Empty,
}

/// What a piece's bytes stand for. The kinds never collapse into each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Byte-for-byte authored text starting at `source_start`.
    Exact { source_start: u32 },
    /// Replacement text standing for `source`. Empty text is a removal.
    Rewritten { source: Span },
    /// Generated text whose first byte stands for `source_offset`.
    Anchor { source_offset: u32 },
    /// Synthetic text with no source.
    Unmapped,
    /// Text whose correspondence is an admitted external decoded map, by
    /// index into the fragment's external-map table.
    External(u32),
}

/// One run of output bytes and what they stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece<'a> {
    pub(crate) text: PieceText<'a>,
    pub(crate) provenance: Provenance,
    /// The piece was moved from its source position.
    pub(crate) relocated: bool,
    pub(crate) marker: Option<MarkerId>,
}

impl<'a> Piece<'a> {
    /// A piece with no marker, at its source position.
    pub(crate) fn new(text: PieceText<'a>, provenance: Provenance) -> Self {
        Self {
            text,
            provenance,
            relocated: false,
            marker: None,
        }
    }

    /// Byte length of the piece's text. A borrowed text longer than
    /// `u32::MAX` reports `u32::MAX`, which no fragment can hold.
    pub fn len(&self) -> u32 {
        match self.text {
            PieceText::Source(span) => span.end.saturating_sub(span.start),
            PieceText::Borrowed(text) => u32::try_from(text.len()).unwrap_or(u32::MAX),
            PieceText::Owned(text) => text.len,
            PieceText::Empty => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn text(&self) -> PieceText<'a> {
        self.text
    }

    pub fn provenance(&self) -> Provenance {
        self.provenance
    }

    pub fn relocated(&self) -> bool {
        self.relocated
    }

    pub fn marker(&self) -> Option<MarkerId> {
        self.marker
    }
}
