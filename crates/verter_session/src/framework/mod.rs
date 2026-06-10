//! Framework adapter substrate: host-level language classification.
//!
//! `verter_language` owns the PURE static classification (ids, static
//! extension rows, gated-candidate descriptors). This module owns the
//! HOST level: [`HostLanguageClassifier`] composes the static registry
//! with the project capability snapshot to resolve gated candidate rows.
//!
//! Crates below the session (`verter_scheduler`, `verter_workspace`) see
//! only `LanguageRegistry::classify_static` directly; host-gated
//! classification reaches them exclusively through session-implemented
//! trait objects (the scheduler `SourceLoader` impl).

pub mod language_classifier;
pub mod project_capabilities;

pub use language_classifier::HostLanguageClassifier;
pub use project_capabilities::ProjectCapabilitySnapshot;
