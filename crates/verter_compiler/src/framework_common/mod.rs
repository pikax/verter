//! Framework adapter plumbing owned by the compiler.
//!
//! Hosts the per-framework carrier bridges between the parser's typed
//! parse results and the framework-neutral
//! [`verter_compiler::framework_common::FrameworkParseArtifact`]. The compiler is the one
//! crate BOTH producers (parse pipelines) and the session (carrier
//! consumers) can name without dependency cycles, so the concrete
//! `CarrierParse` wrappers live here rather than in `verter_parser`
//! (the wrapper is adapter plumbing, not parser data) or
//! `verter_session` (unnameable from compiler-side producers).
//!
//! On top of the carrier wrappers it owns the compiler-side carrier
//! framework substrate: the [`CarrierCompiler`] trait (parse / eval /
//! IDE / template) and the [`CarrierCompilerRegistry`] the host's carrier
//! dispatch looks up. Vue is the reference implementation
//! ([`vue_bridge::VueCarrierCompiler`]), delegating call-for-call to the
//! existing Vue pipeline with ZERO edits to any Vue parser/codegen
//! module.

pub mod capability;
pub mod carrier_compiler;
pub mod catalog;
pub(crate) mod generated_chunk;
pub mod generated_identifier;
#[doc(hidden)]
pub mod registered_carrier_projection;
mod registered_geometry_state;
pub mod registry;
pub mod vue_bridge;
pub mod vue_carrier_frontend;
pub mod vue_semantic_authority;

#[cfg(test)]
mod registered_carrier_projection_tests;

/// Reusable framework IDE sourcemap end-to-end assertion helpers, shared
/// by every carrier vertical's `#[cfg(test)]` sourcemap suite.
#[cfg(test)]
pub mod sourcemap_e2e_helpers;

pub use crate::svelte::{svelte_semantic_authority_registration, SvelteSemanticAuthority};
pub use capability::{
    CarrierFrontend, FrameworkEpoch, FrameworkEpochId, FrameworkHostIntegrationBackend,
    FrameworkSemanticAuthority, HostEpoch, HostEpochId, Present, ProjectionBackend,
    RuntimeCompilerBackend,
};
pub use carrier_compiler::{
    CarrierCompileOutcome, CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput,
    RuntimeBlockContentInput, RuntimeBlockContentInputs, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeCustomBlock, RuntimeDiagnostic, RuntimeDiagnosticSeverity,
    RuntimeMainModule, RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeStyleBlock,
    RuntimeSurfaceRefusal, RuntimeTemplateBlock, SourceMapFidelity, TemplateFacts,
    TemplateRenderExport,
};
pub use catalog::{
    CatalogCapability, CatalogIdentity, CatalogRow, DuplicateCatalogIdentity, FrontendCap, HostCap,
    ImmutableCapabilityCatalog, ProjectionCap, RuntimeCap, SemanticCap,
    TypedCapabilityRegistration,
};
pub use generated_identifier::{is_generated_identifier, GENERATED_IDENTIFIER_PREFIX};
#[doc(hidden)]
pub use registered_carrier_projection::FrameworkParseArtifact;
pub use registered_carrier_projection::RegisteredCarrierPayload;
pub use registry::CarrierCompilerRegistry;
pub use vue_carrier_frontend::{vue_carrier_frontend_registration, VueCarrierFrontend, VueSfcV3};
pub use vue_semantic_authority::{vue_semantic_authority_registration, VueSemanticAuthority};
