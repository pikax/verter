//! Placement of emission fragments.
//!
//! A plan orders fragments, fills typed slots, registers artifact
//! boundaries and binds source identities. It resolves no source edit and
//! never reinterprets a fragment's provenance. [`EmitPlan::write`] is the
//! one finalisation of an artifact: one buffer, one generated-order pass,
//! and in [`MapMode::Map`] the decoded map in that same pass. Written text is
//! immutable.

mod artifact;
mod plan;
mod segment;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use artifact::EmittedWithMarkers;
pub use artifact::{DecodedMap, EmittedArtifact, MapSource};
pub use plan::{EmitPlan, EmitPlanBuilder, EmitPlanError, SlotHandle};
pub use segment::{ArtifactBoundary, EmissionSegment, FragmentRef, MapMode, SlotId};

pub use crate::emission::{
    DecodedMapSegment, EmissionBuilder, EmissionError, EmissionFragment, FragmentSlot, MapHints,
    MapSegmentKind, MarkerId, Piece, PieceText, PreambleAnchor, Provenance, QualifiedSource,
    SourceKey,
};
