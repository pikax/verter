//! File-language routing authority for Verter.
//!
//! This leaf crate owns the single open language descriptor
//! ([`FileLanguage`]) and the pure static classification registry
//! ([`LanguageRegistry`]) every Verter crate routes files through. It is
//! the ONE definition of "what kind of file is this" in the workspace —
//! per-crate file-kind enums and ad-hoc `ends_with(".vue")` checks are
//! retired in its favour.
//!
//! Two authority levels split classification across the workspace:
//!
//! * **Pure static classification** (this crate):
//!   [`LanguageRegistry::classify_static`] resolves a path against static
//!   extension rows and gated-candidate descriptors. It never reads
//!   project configuration, package graphs, or host state.
//! * **Host-gated classification** (`verter_session`): the host-level
//!   classifier composes [`LanguageRegistry::classify_static`] with the
//!   project capability snapshot to resolve
//!   [`StaticClassification::Gated`] candidates. Crates below the session
//!   (`verter_scheduler`, `verter_workspace`) see only the pure static
//!   entry directly; host-gated classification reaches them exclusively
//!   through session-implemented trait objects.
//!
//! Built-in rows cover the TS/JS script family plus the `.vue` and
//! `.svelte` framework carriers. A registry row whose adapter has no
//! registered carrier implementation behind it (`.svelte`) is the
//! STRUCTURAL source of the typed unsupported-language state: dispatch
//! finds the row, finds no carrier, and returns a typed error — never a
//! silent empty result, never a panic.

mod ids;
mod language;
mod parse_artifact;
mod registry;

pub use ids::{CapabilityId, FrameworkAdapterId, LanguageId};
pub use language::{FileLanguage, JsModuleKind, ScriptFlavor, ScriptSourceType};
pub use parse_artifact::{
    CarrierAccessToken, CarrierParse, ExternalLink, ExternalLinkKind, FrameworkParseArtifact,
    FrameworkParseCommon, LanguageDiagnostic, LanguageDiagnosticSeverity, ScriptRegion,
    ScriptRegionKind, StyleRegion, TemplateRegion, __carrier_downcast_arc, __carrier_downcast_ref,
};
pub use registry::{
    GatedCandidate, LanguageRegistry, LanguageRow, RowClassification, StaticClassification,
    SVELTE_RUNE_MODULE_LANGUAGE_ID,
};
