//! Reserved optimized-basis identities
//! (`docs/arch/compiler-request/compile-request-contract.md`, "Reserved
//! optimized basis").
//!
//! The names describe a two-stage lookup: a stored candidate is looked up
//! by what was known before execution, and each candidate's observation
//! set is then validated against the captured workspace basis, so lookup
//! never needs the future read set. Nothing here traverses a project,
//! runs a proof engine or touches a cache: these types have no
//! constructor at all — inside this crate or out — so no code can
//! construct, key or cache by them until project optimization lands with
//! its owning amendment and declares the parts itself.

/// Known before execution: the request basis up to and including the
/// target-plan stage identity, plus the optimization policy revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptimizationRequestBasis {
    /// Reserved; never read. The parts land with project optimization.
    _reserved: (),
}

/// Discovered during analysis: the ordered observed workspace facts and
/// their digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptimizationObservationSet {
    /// Reserved; never read. The parts land with project optimization.
    _reserved: (),
}

/// What a published optimized artifact was built from: the request
/// basis, the observation digest and the decision digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtifactBasis {
    /// Reserved; never read. The parts land with project optimization.
    _reserved: (),
}
