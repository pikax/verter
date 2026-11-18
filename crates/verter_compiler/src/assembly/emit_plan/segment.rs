pub use crate::emission::{ArtifactBoundary, MapMode};

/// Index of a fragment in the plan that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FragmentRef(pub(super) u32);

/// Index of a reserved slot in the plan that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub(super) u32);

/// One placed item. Meaning lives in fragments, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmissionSegment {
    /// A finished fragment, written in place.
    Fragment(FragmentRef),
    /// A typed slot, filled by its owner before the plan is finished.
    Slot(SlotId),
    /// A cut point between artifacts; it emits no bytes.
    Boundary(ArtifactBoundary),
}
