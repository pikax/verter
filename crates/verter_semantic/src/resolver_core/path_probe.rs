//! Typed path-probe result — the resolver kernel's filesystem observation
//! vocabulary.
//!
//! `ResolverObservation::path_probe` returns this type directly; a VFS
//! implementation maps its own filesystem outcomes onto it. Error-tolerant
//! states are not absence: `Inaccessible` and `Unknown` are distinct from
//! `Absent`, since a permission error or a transient failure must not be
//! cached as a stable negative resolution result.

/// Typed path-probe result. Error-tolerant states are not absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PathProbe {
    File,
    Directory,
    Absent,
    Inaccessible,
    Unknown,
}

#[cfg(test)]
#[path = "path_probe_tests.rs"]
mod tests;
