//! `verter_analysis_inputs` — committed analysis-input authority and the
//! local-analysis-input privacy types.
//!
//! This crate is filesystem-free: it never reads a file. Producers hand already-read
//! bytes to [`parse_config`] or [`InputBasis::commit`]. Consumers observe committed
//! rows only — never by a consumer-local filesystem read.
//!
//! - [`InputBasis`] / [`LoadWave`] / [`NegativeFact`] / [`SnapshotFence`] — one
//!   committed input and coherent snapshot authority.
//! - [`ProjectId`] — the opaque `p[0-9]{4}` identity, validated at construction.
//! - [`AnalysisProjects`] / [`AnalysisProject`] — the config schema, with real
//!   paths held PRIVATELY (not `Serialize`, hand-written redacted `Debug`/`Display`).
//! - [`Redactor`] — the single producer-side redactor: real paths → opaque
//!   `analysis://<id>/file-<NNNN>.<ext>` virtual ids.
//! - [`AnalysisInputError`] — a redacted error type whose `Display`/`Debug` never
//!   print a raw path.
//! - [`parse_config`] — parses config CONTENT a caller hands it.
//!
//! Corpus-privacy types remain the analysis-runner / hermetic-guard surface.
//! [`InputBasis`] is the committed-input IR consumed by the session host.

mod config;
mod error;
mod id;
pub mod input_basis;
pub mod loader;
mod redact;

pub use config::{
    parse_config, AnalysisProject, AnalysisProjects, ProjectKind, Workstream,
    ANALYSIS_PROJECTS_SCHEMA,
};
pub use error::AnalysisInputError;
pub use id::{ProjectId, ProjectIdError};
pub use input_basis::{
    CommitError, DirectoryEntry, InputBasis, LoadWave, NegativeFact, NegativeKind, Observation,
    ObservationKind, ObserveError, SnapshotFence, TornSnapshot,
};
pub use loader::ANALYSIS_CORPUS_ENV;
pub use redact::Redactor;

impl AnalysisProjects {
    /// Build a [`Redactor`] keyed on this config's `id → root` map. The redactor is
    /// the only consumer of the private roots — it turns them into opaque tokens.
    pub fn redactor(&self) -> Redactor {
        Redactor::new(self.id_root_pairs())
    }
}
