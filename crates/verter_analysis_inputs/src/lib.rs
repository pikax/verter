//! `verter_analysis_inputs` — the neutral foundation for local-analysis-input
//! handling: opaque project ids, the config schema, path-privacy types, the
//! producer-side redactor, and the redacted error type.
//!
//! Real-world projects are run through Verter as LOCAL ANALYSIS INPUTS to find
//! deviations against reference tooling. Their on-disk paths are private bytes: no
//! project name, path, relative filename, or basename may appear in any committed
//! or emitted artifact. This crate is the cross-cutting home for the types and the
//! single redactor that enforce that — consumed by the analysis runners and the
//! workspace hermetic guard, never by the compiler/LSP/session layers.
//!
//! - [`ProjectId`] — the opaque `p[0-9]{4}` identity, validated at construction.
//! - [`AnalysisProjects`] / [`AnalysisProject`] — the config schema, with real
//!   paths held PRIVATELY (not `Serialize`, hand-written redacted `Debug`/`Display`).
//! - [`Redactor`] — the single producer-side redactor: real paths → opaque
//!   `analysis://<id>/file-<NNNN>.<ext>` virtual ids.
//! - [`AnalysisInputError`] — a redacted error type whose `Display`/`Debug` never
//!   print a raw path.
//! - [`parse_config`] — parses config CONTENT a caller hands it. This crate is
//!   filesystem-free: it never reads a file. The consumer that owns an allow-listed
//!   disk boundary (the TS dx-harness, or the future Rust analysis runner) reads the
//!   file and feeds the bytes here. The shared env-var name lives in [`loader`].

mod config;
mod error;
mod id;
pub mod loader;
mod redact;

pub use config::{
    parse_config, AnalysisProject, AnalysisProjects, ProjectKind, Workstream,
    ANALYSIS_PROJECTS_SCHEMA,
};
pub use error::AnalysisInputError;
pub use id::{ProjectId, ProjectIdError};
pub use loader::ANALYSIS_CORPUS_ENV;
pub use redact::Redactor;

impl AnalysisProjects {
    /// Build a [`Redactor`] keyed on this config's `id → root` map. The redactor is
    /// the only consumer of the private roots — it turns them into opaque tokens.
    pub fn redactor(&self) -> Redactor {
        Redactor::new(self.id_root_pairs())
    }
}
