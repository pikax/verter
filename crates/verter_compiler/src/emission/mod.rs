//! Shared immutable emission representation.
//!
//! A producer describes output as ordered pieces. Each piece keeps what its
//! bytes mean: an exact copy of authored text, replacement text standing for
//! a source range, generated text with one source point, synthetic text, or
//! text whose correspondence is an admitted external map. A transform's
//! frozen chunks and the direct [`EmissionBuilder`] are the only ways to make
//! a fragment; the one writer turns placed fragments into final text and,
//! when asked, a decoded map. Nothing here names a framework.
//!
//! The map policy ([`MapMode`]) is chosen when a builder is created, before
//! any producer runs. Under [`MapMode::NoMap`] no map-only fact is captured:
//! anchored and rewritten text is stored as unmapped, and a fragment carries
//! no map hints.

mod builder;
pub(crate) mod correspondence;
mod fragment;
mod piece;
mod source;
pub(crate) mod walk;
pub(crate) mod writer;

pub use builder::EmissionBuilder;
pub use fragment::{EmissionFragment, FragmentSlot, MapHints, PreambleAnchor};
pub use piece::{MarkerId, Piece, PieceText, Provenance};
pub use source::{QualifiedSource, SourceKey};
pub use writer::{DecodedMapSegment, MapSegmentKind};

#[cfg(test)]
pub(crate) use source::test_source;

/// Whether map data is captured. Chosen when a transform, a recorder or a
/// builder is created, before any producer runs. Independent of the
/// observation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapMode {
    NoMap,
    Map,
}

/// A named cut point. The writer reports the output offset of each
/// boundary; it emits no bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArtifactBoundary(pub u32);

/// Typed failure of fragment construction. No partial fragment exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmissionError {
    /// An authored slice is outside its source or splits a UTF-8 sequence,
    /// or a piece names the source of a builder that has none.
    InvalidSourceSlice { span: verter_span::Span },
    /// Output would exceed `u32::MAX` bytes.
    Capacity,
    /// Map data was asked of something created with `MapMode::NoMap`.
    MapNotCaptured,
    /// A marker that the fragment does not hold.
    UnknownMarker(MarkerId),
    /// A nested fragment names a different source than the builder's. A
    /// fragment holds at most one source; several sources meet in a plan.
    ForeignSource,
    /// A nested fragment was built under the other map policy.
    ModeMismatch,
}

#[cfg(test)]
thread_local! {
    static MAP_CAPTURE_EVENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static TEXT_BUFFERS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Count one generated-text buffer a fragment came to own.
#[inline(always)]
fn note_text_buffer() {
    #[cfg(test)]
    TEXT_BUFFERS.with(|buffers| buffers.set(buffers.get() + 1));
}

/// Generated-text buffers fragments came to own on this thread since the
/// last call; resets the count. Bytes held by reference are not counted.
#[cfg(test)]
pub(crate) fn take_text_buffers() -> u64 {
    TEXT_BUFFERS.with(|buffers| buffers.replace(0))
}

/// Count one map-only capture. Every capture point in `emission` and in
/// `code_transform` calls this; under `MapMode::NoMap` none is reached.
#[inline(always)]
pub(crate) fn note_map_capture() {
    #[cfg(test)]
    MAP_CAPTURE_EVENTS.with(|events| events.set(events.get() + 1));
}

/// Map-only captures recorded on this thread since the last reset.
#[cfg(test)]
pub(crate) fn map_capture_events() -> u64 {
    MAP_CAPTURE_EVENTS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_map_capture_events() {
    MAP_CAPTURE_EVENTS.with(|events| events.set(0));
}
