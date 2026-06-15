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

pub mod api_projector;
pub mod api_projectors;
pub mod ctx;
pub mod descriptor;
pub mod language_classifier;
pub mod project_capabilities;
pub mod registry;
pub mod rune_module;
pub mod script_facts;
pub mod surface_store;
pub mod svelte_jsx_assets;
pub mod synth;
pub mod virtual_file_naming_ts;

pub use api_projector::{ComponentApiProjector, ComponentApiProjectorCtx};
pub use ctx::FrameworkAdapterCtx;
pub use descriptor::{
    svelte_rune_module_naming, vue_descriptor, FrameworkAdapterDescriptor, VirtualFileNaming,
    VirtualPathPolicy, ALL_FRAMEWORK_SURFACE_KINDS,
};
pub use language_classifier::HostLanguageClassifier;
pub use project_capabilities::ProjectCapabilitySnapshot;
pub use registry::{
    CarrierLeg, FrameworkAdapterRegistry, FrameworkRegistration, SurfaceRegistration,
    TagDisposition,
};
pub use rune_module::{
    rune_module_provider_content, svelte_rune_module_source_type, RuneModuleProviderContent,
};

/// The interned framework-adapter id, re-exported from `verter_language` so
/// session-side registry/descriptor code names it through the `framework`
/// module.
pub use verter_language::FrameworkAdapterId;
