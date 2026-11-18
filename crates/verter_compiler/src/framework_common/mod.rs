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
//! framework substrate: the immutable [`CatalogSnapshot`] and the two
//! registries over it ([`CarrierFrontendRegistry`],
//! [`CarrierCompilerBackendRegistry`]) production selectors dispatch
//! through.
//! [`VueCarrierFrontend`] is the Vue carrier identity: it parses, rejects
//! typed, and owns the downcasts the typed Vue backends use, matching
//! [`crate::svelte::SvelteCarrierFrontend`]. Production IDE projection
//! and runtime-bundle emission are the typed capability backends.

pub mod capability;
pub mod carrier_compiler;
pub mod carrier_registry;
pub mod catalog;
pub(crate) mod generated_chunk;
pub mod generated_identifier;
pub mod profile;
pub mod projection_plan;
#[doc(hidden)]
pub mod registered_carrier_projection;
mod registered_geometry_state;
pub mod structure;
pub mod svelte_host_integration;
pub(crate) mod typescript_directives;
pub mod virtual_file_naming;
pub mod vue_bridge;
pub mod vue_bundle;
pub mod vue_carrier_frontend;
pub mod vue_host_integration;
pub mod vue_projection_backend;
pub mod vue_runtime_backend;
pub mod vue_semantic_authority;

#[cfg(test)]
mod carrier_registry_tests;
#[cfg(test)]
mod registered_carrier_projection_tests;

/// Reusable framework IDE sourcemap end-to-end assertion helpers, shared
/// by every carrier vertical's `#[cfg(test)]` sourcemap suite.
#[cfg(test)]
pub mod sourcemap_e2e_helpers;

pub use crate::ide::vue_projection::script_setup::{
    ModuleScopeProjection, ScriptProjectionFacts, SetupProjectionRefusal, SetupStatementKind,
    TsSetupProjection, UniversalSetupBinder,
};
pub use crate::svelte::{svelte_semantic_authority_registration, SvelteSemanticAuthority};
pub use capability::{
    CarrierFrontend, FrameworkEpoch, FrameworkHostIntegrationBackend, FrameworkSemanticAuthority,
    HostEpoch, HostEpochId, NativeHostEpoch, Present, ProductExecutionGrant,
    ProductExecutionGrants, ProjectionBackend, RuntimeCompilerBackend,
};
pub use carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeOutput, QualifiedRuntimeStyle,
    RuntimeBlockContentInput, RuntimeBlockContentInputs, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeDiagnostic, RuntimeDiagnosticSeverity, RuntimeOutputDescriptor,
    RuntimeScriptBlock, RuntimeSurfaceRefusal, RuntimeTemplateBlock, SourceMapFidelity,
    TemplateRenderExport,
};
#[cfg(any(test, feature = "test-support"))]
pub use carrier_registry::{take_registry_dispatches, RegistryDispatches};
pub use carrier_registry::{
    CarrierCompilerBackendRegistry, CarrierCompilerBackendRow, CarrierFrontendRegistry,
    CarrierFrontendRow, HostIntegrationMiss, InstalledCarrierFrontend, ProjectionBackendRow,
    RegisteredBackend, RegisteredCarrierFrontend, SemanticAuthorityRow,
};
pub use catalog::{
    CatalogCapability, CatalogIdentity, CatalogRowProvider, CatalogRows, CatalogSnapshot,
    CatalogSnapshotError, CatalogTable, CatalogTableRow, DuplicateCatalogIdentity, FrontendCap,
    HostCap, ProductCapability, ProjectionCap, RuntimeCap, SemanticCap,
    TypedCapabilityRegistration,
};
pub use generated_identifier::{is_generated_identifier, GENERATED_IDENTIFIER_PREFIX};
pub use projection_plan::origin::{
    compose_source_chain, projected_class_for_emit_op, projected_class_for_emit_op_with,
    CheckingTextRevision, CorrespondenceRevision, EditOrigin, EmissionRefusal, ObservationRole,
    ProjectionEmission, ProjectionOrigin, RoleQualifiedObservation,
};
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
    svelte_catalog_rows, svelte_host_integration_registration, SvelteAdmittedDemand,
    SvelteCompileAdmission, SvelteHostAdmissionRefusal, SvelteHostCompileRefusal,
    SvelteHostCompiledProducts, SvelteHostExecutionInputs, SvelteHostIntegrationBackend,
    SvelteHostMultiProductDemand, SvelteHostRenderedMain, SvelteHostRuntimeRenderDemand,
    SvelteHostUnproducibleDemand, SvelteSuppliedStyle,
};
pub use vue_carrier_frontend::{
    vue_carrier_frontend_registration, vue_grammar_config, VueCarrierFrontend,
    VueGrammarConfigError, VueSfcV3,
};
pub use vue_host_integration::{
    vue_catalog_rows, vue_host_integration_registration, VueAdmittedDemand, VueCompileAdmission,
    VueHostAdmissionRefusal, VueHostCompileRefusal, VueHostCompiledProducts,
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
