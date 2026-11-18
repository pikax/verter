use std::sync::atomic::{AtomicU64, Ordering};

use super::piece::{MarkerId, Piece, PieceText};
use super::source::QualifiedSource;
use super::MapMode;
use crate::assembly::map_input::DecodedFragmentMap;

/// Map facts that are not pieces: where an exact run needs an extra token,
/// where a mapping stops inside a piece, and where a helper preamble ends.
/// A fragment holds them exactly under [`MapMode::Map`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapHints {
    /// Source offsets that receive their own token where exact text covers
    /// them. Sorted and unique once the fragment is finished.
    pub(crate) locations: Vec<u32>,
    /// Fragment-relative generated offsets at which the mapping stops inside
    /// a piece. Sorted and unique once the fragment is finished.
    pub(crate) unmapped_boundaries: Vec<u32>,
    pub(crate) preamble: Option<PreambleAnchor>,
}

impl MapHints {
    pub fn locations(&self) -> &[u32] {
        &self.locations
    }

    pub fn unmapped_boundaries(&self) -> &[u32] {
        &self.unmapped_boundaries
    }

    pub fn preamble(&self) -> Option<PreambleAnchor> {
        self.preamble
    }

    pub(super) fn normalise(&mut self) {
        self.locations.sort_unstable();
        self.locations.dedup();
        self.unmapped_boundaries.sort_unstable();
        self.unmapped_boundaries.dedup();
    }
}

/// The end of a leading helper preamble: the fragment-relative generated
/// offset just after it, and the authored point a zero-width insertion at
/// its start stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreambleAnchor {
    pub generated_end: u32,
    pub carrier: Option<u32>,
}

/// Identity of one finished fragment, unique in the process.
#[derive(Debug, Clone, Copy, Eq)]
pub(crate) struct FragmentToken(u64);

impl PartialEq for FragmentToken {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl FragmentToken {
    pub(super) fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// An immutable, sealed run of pieces over at most one source. There is no
/// public constructor, no conversion from a `String` and no mutable access
/// to its text: [`super::EmissionBuilder`] is the only way to make one.
#[derive(Debug)]
pub struct EmissionFragment<'a> {
    pub(super) source: Option<QualifiedSource<'a>>,
    pub(super) pieces: Vec<Piece<'a>>,
    pub(super) owned: Vec<String>,
    pub(super) len: u32,
    pub(super) mode: MapMode,
    /// `None` exactly under [`MapMode::NoMap`].
    pub(super) hints: Option<Box<MapHints>>,
    pub(super) external: Vec<DecodedFragmentMap>,
    pub(super) token: FragmentToken,
}

impl<'a> EmissionFragment<'a> {
    /// Exact output length, cached.
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn mode(&self) -> MapMode {
        self.mode
    }

    pub fn source(&self) -> Option<&QualifiedSource<'a>> {
        self.source.as_ref()
    }

    /// Map facts beside the pieces; `None` exactly under `MapMode::NoMap`.
    pub fn hints(&self) -> Option<&MapHints> {
        self.hints.as_deref()
    }

    /// The declared position `marker`, if this fragment holds it.
    pub fn slot(&self, marker: MarkerId) -> Option<FragmentSlot> {
        let piece = self
            .pieces
            .iter()
            .position(|piece| piece.marker == Some(marker))?;
        Some(FragmentSlot {
            fragment: self.token,
            marker,
            piece: piece as u32,
        })
    }

    pub(crate) fn pieces(&self) -> &[Piece<'a>] {
        &self.pieces
    }

    pub(crate) fn token(&self) -> FragmentToken {
        self.token
    }

    /// The bytes of `piece`, which must be one of this fragment's pieces.
    pub(crate) fn piece_text(&self, piece: &Piece<'a>) -> &str {
        match piece.text {
            PieceText::Source(span) => self.source.as_ref().map_or("", |source| {
                &source.text[span.start as usize..span.end as usize]
            }),
            PieceText::Borrowed(text) => text,
            PieceText::Owned(text) => &self.owned[text.buffer as usize][text.range()],
            PieceText::Empty => "",
        }
    }

    /// The admitted external map an [`super::Provenance::External`] piece
    /// names.
    pub(crate) fn external(&self, index: u32) -> Option<&DecodedFragmentMap> {
        self.external.get(index as usize)
    }
}

/// A producer-declared position inside one fragment that assembly may fill
/// before writing. Neither `Copy` nor `Clone`: a slot is filled once.
#[derive(Debug, PartialEq, Eq)]
pub struct FragmentSlot {
    fragment: FragmentToken,
    marker: MarkerId,
    piece: u32,
}

impl FragmentSlot {
    pub fn marker(&self) -> MarkerId {
        self.marker
    }

    pub(crate) fn fragment(&self) -> FragmentToken {
        self.fragment
    }

    /// Index of the marked piece in its fragment.
    pub(crate) fn piece(&self) -> u32 {
        self.piece
    }
}

#[cfg(test)]
#[path = "fragment_tests.rs"]
mod tests;
