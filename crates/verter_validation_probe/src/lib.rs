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
//! * [`disk`] — the crate's ONE disk boundary; nothing else in the crate
//!   touches the filesystem.
//! * [`corpus`] — the corpus adapters: the only place a pinned external
//!   workload is located on disk and read.
//! * [`request`] — the one immutable canonical compile request every Vue
//!   workload case issues, and its digests.
//! * [`runner`] — the table-driven workload runner: one driver process, the
//!   ONE public compile route, structural classification, attributable
//!   termination.
//! * [`lane`] — under `external-corpus` only: manifest in, summary out.
//! * [`summary`] — the lane's one compact machine-readable summary and its
//!   real-exit disposition.
//!
//! Observation never implies acceptance: a manifest entry records evidence
//! about Verter behaviour and never derives expected output from an external
//! corpus. Only a `gate` cell can block, and a gate is admitted only through
//! an implemented authority's atom that lists the gate's exact class.

pub mod authority;
pub mod corpus;
pub mod disk;
pub mod evaluate;
#[cfg(feature = "external-corpus")]
pub mod lane;
pub mod manifest;
pub mod outcome;
pub mod request;
pub mod runner;
pub mod summary;

pub use authority::Authority;
pub use corpus::{CorpusCase, CorpusError};
pub use evaluate::Evaluation;
pub use manifest::{
    Applicability, ApplicabilityFlags, Case, CellId, ComparatorIdentity, Comparison, ExpectedState,
    Framework, ManifestError, ManifestViolation, ProbeEntry, ProbeStateManifest, Sha256, Sha40,
    Stratum, MAX_SMOKE_CASES,
};
pub use outcome::{
    CaseObservation, Dimension, DimensionInput, Evidence, EvidenceSource, InvalidObservation,
    NotApplicableReason, ProbeOutcomeClass, Terminal,
};
pub use runner::{classify_execution, ExecutionEvent, Phase, ProbeRun};
pub use summary::{Counters, Disposition, Lane, ObservedTerminal, Summary, SummaryError};
