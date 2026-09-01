//! File-language routing authority for Verter.
//!
//! This leaf crate owns the single open language descriptor
//! ([`FileLanguage`]) and the pure static classification registry
//! ([`LanguageRegistry`]) every Verter crate routes files through. It is
//! the ONE definition of "what kind of file is this" in the workspace: no
//! crate carries its own file-kind enum or ad-hoc `ends_with(".vue")`
//! check.
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
pub mod parse_artifact;
pub mod parse_identity;
mod registry;

pub mod carrier_grammar;
pub mod carrier_versions;
pub mod registered_source_authority;

pub use ids::{CapabilityId, FrameworkAdapterId, LanguageId};
pub use language::{FileLanguage, JsModuleKind, ScriptFlavor, ScriptSourceType};
pub use parse_artifact::carrier_inventory::*;
pub use parse_artifact::carrier_structure_hash::{
    compute_carrier_structure_hash, CarrierStructureHash,
};
pub use parse_artifact::{
    __carrier_downcast_arc, __carrier_downcast_ref, compare_language_diagnostic_fields,
    compare_language_diagnostics, sort_language_diagnostics, CarrierParse, DiagnosticArg,
    DiagnosticSpanRejectReason, ExternalLink, ExternalLinkKind, FrameworkParseCommon,
    LanguageDiagnostic, LanguageDiagnosticOrderKey, LanguageDiagnosticSeverity, ScriptRegion,
    ScriptRegionKind, StyleRegion, SyntaxReject, TemplateRegion,
    UnregisteredFrameworkParseArtifact, UnsupportedSyntaxProfileReason,
};
pub use parse_identity::{
    default_parse_identity_for, parse_identity_for, parse_key_for, syntax_profile_id_for, ParseKey,
    ParseOptions, SyntaxProfileId, FRAMEWORK_SYNTAX_COMPATIBILITY_DOMAIN,
    FRAMEWORK_SYNTAX_COMPATIBILITY_EPOCH, SCRIPT_SYNTAX_COMPATIBILITY_DOMAIN,
    SCRIPT_SYNTAX_COMPATIBILITY_EPOCH, SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
    SVELTE_SYNTAX_COMPATIBILITY_EPOCH, VUE_SYNTAX_COMPATIBILITY_DOMAIN,
    VUE_SYNTAX_COMPATIBILITY_EPOCH,
};
pub use registry::{
    GatedCandidate, LanguageRegistry, LanguageRow, RowClassification, StaticClassification,
    SVELTE_RUNE_MODULE_LANGUAGE_ID,
};
