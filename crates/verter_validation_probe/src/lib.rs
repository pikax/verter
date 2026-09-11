//! The validation-probe contract shared by every comparison and workload tool
//! that exercises Verter against external inputs.
//!
//! * [`outcome`] — the closed [`ProbeOutcomeClass`] taxonomy, the causal
//!   precedence fold [`ProbeOutcomeClass::terminal`], the per-dimension
//!   [`Terminal`], and the total per-case [`CaseObservation`].
//! * [`manifest`] — the machine-readable [`ProbeStateManifest`]: one TOML file
//!   per framework under `manifest/<framework>.toml`, validated by
//!   [`ProbeStateManifest::validate`].
//! * [`authority`] — the closed [`Authority`] citation ids. Which framework,
//!   dimensions and outcomes an authority covers is validation-authority
//!   catalog data, never encoded here.
//! * [`evaluate`] — [`ProbeEntry::evaluate`], the only place an expectation
//!   meets an observation.
//!
//! Observation never implies acceptance: a manifest entry records evidence
//! about Verter behaviour and never derives expected output from an external
//! corpus. Only a `gate` cell can block, and a gate is admitted only through
//! an implemented authority's atom that lists the gate's exact class.

pub mod authority;
pub mod evaluate;
pub mod manifest;
pub mod outcome;

pub use authority::Authority;
pub use evaluate::Evaluation;
pub use manifest::{
    Applicability, ApplicabilityFlags, Case, CellId, ComparatorIdentity, Comparison, ExpectedState,
    Framework, ManifestError, ManifestViolation, ProbeEntry, ProbeStateManifest, Sha256, Sha40,
    Stratum,
};
pub use outcome::{
    CaseObservation, Dimension, DimensionInput, Evidence, EvidenceSource, InvalidObservation,
    NotApplicableReason, ProbeOutcomeClass, Terminal,
};
