//! Positional project index within one published workspace snapshot.

/// Index of a project in one published workspace snapshot's ordered
/// project list.
///
/// The index is positional: it is meaningful only against the snapshot
/// that assigned it. A rebuilt snapshot may assign the same index to a
/// different project, so this is NOT a stable cross-snapshot cache
/// identity — cross-snapshot identity is [`super::ProjectStableKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(pub u32);
