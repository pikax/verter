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
//! framework substrate: the immutable per-capability catalogs
//! (`registered_carrier_projection::built_in_frontend_catalog` and
//! friends) production selectors dispatch through.
//! [`vue_bridge::VueCarrierCompiler`] is the Vue parse/identity/downcast
//! surface the typed Vue backends and frontend row build on, matching
//! [`crate::svelte::SvelteCarrierCompiler`]. Production IDE projection
//! and runtime-bundle emission are the typed capability backends.

pub mod capability;
pub mod carrier_compiler;
pub mod catalog;
pub(crate) mod generated_chunk;
pub mod generated_identifier;
pub mod projection_plan;
#[doc(hidden)]
pub mod registered_carrier_projection;
mod registered_geometry_state;
pub mod svelte_host_integration;
pub(crate) mod typescript_directives;
pub mod vue_bridge;
pub mod vue_carrier_frontend;
pub mod vue_host_integration;
pub mod vue_projection_backend;
pub mod vue_runtime_backend;
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
    FrameworkSemanticAuthority, HostEpoch, HostEpochId, NativeHostEpoch, Present,
    ProductExecutionGrant, ProductExecutionGrants, ProjectionBackend, RuntimeCompilerBackend,
};
pub use carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeCompileOptions, IdeOutput, QualifiedRuntimeStyle,
    RuntimeBlockContentInput, RuntimeBlockContentInputs, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeDiagnostic, RuntimeDiagnosticSeverity, RuntimeOutputDescriptor,
    RuntimeScriptBlock, RuntimeSurfaceRefusal, RuntimeTemplateBlock, SourceMapFidelity,
    TemplateRenderExport,
};
pub use catalog::{
    CatalogCapability, CatalogIdentity, CatalogRow, DuplicateCatalogIdentity, FrontendCap, HostCap,
    ImmutableCapabilityCatalog, ProjectionCap, RuntimeCap, SemanticCap,
    TypedCapabilityRegistration,
};
pub use generated_identifier::{is_generated_identifier, GENERATED_IDENTIFIER_PREFIX};
pub use projection_plan::{
    build_projection_plan, build_projection_plan_incremental, incomplete_missing_parse,
    incomplete_parse_snapshot_mismatch, plan_from_source, AdmittedExpressionId, BindingOrigin,
    BindingOriginId, BranchEdge, BranchOutcome, CompleteCacheRefusal, CompletePlanCache,
    ComponentUse, ComponentUseId, ExpressionOccurrence, GenericBinderRef, Incompleteness,
    LexicalScopeId, ObligationKind, OrderedAttributeOp, PlanCompleteness, PlanInput,
    PlanSnapshotId, ProjectionPlan, SyntaxObligation,
};
#[doc(hidden)]
pub use registered_carrier_projection::FrameworkParseArtifact;
pub use registered_carrier_projection::RegisteredCarrierPayload;
pub use svelte_host_integration::{
    svelte_host_integration_registration, SvelteAdmittedDemand, SvelteCompileAdmission,
    SvelteHostAdmissionRefusal, SvelteHostCompileRefusal, SvelteHostCompiledProducts,
    SvelteHostExecutionInputs, SvelteHostIntegrationBackend, SvelteHostMultiProductDemand,
    SvelteHostRenderedMain, SvelteHostRuntimeRenderDemand, SvelteHostUnproducibleDemand,
    SvelteSuppliedStyle,
};
pub use vue_carrier_frontend::{vue_carrier_frontend_registration, VueCarrierFrontend, VueSfcV3};
pub use vue_host_integration::{
    built_in_host_integration_catalog, registered_host_integration_for,
    vue_host_integration_registration, InstalledHostIntegration, VueAdmittedDemand,
    VueCompileAdmission, VueHostAdmissionRefusal, VueHostCompileRefusal, VueHostCompiledProducts,
    VueHostExecutionInputs, VueHostIntegrationBackend, VueHostMultiProductDemand,
    VueHostRenderedMain, VueHostRuntimeRenderDemand, VueHostUnproducibleDemand,
};
pub use vue_projection_backend::{
    vue_projection_backend_registration, VueIdeCompanion, VueProjectionBackend,
    VueProjectionDiagnostic, VueProjectionError, VueProjectionInputs,
};
pub use vue_runtime_backend::{
    vue_runtime_backend_registration, VueRuntimeBackend, VueRuntimeError, VueRuntimeExecutionFacts,
    VueRuntimeInputs,
};
pub use vue_semantic_authority::{vue_semantic_authority_registration, VueSemanticAuthority};
