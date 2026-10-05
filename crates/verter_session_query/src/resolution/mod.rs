//! Semantic-owned module-resolution kernel.
//!
//! This module owns the batched `AttemptOutcome`/`LoadSet` input-loading
//! protocol, the capability-limited observation interface, and
//! `ModuleResolverCore` itself.
//!
//! `verter_session::resolver_core` keeps the blocking, host-capable
//! lifecycles (`ResolverContext`, `HostResolverContext`,
//! `SessionResolverContext`, `request_store_view.rs`) as its own four
//! carve-out files. This module never names `VerterHost` or any scheduler
//! type.

pub mod ambient_symbol_hit;
pub mod attempt_outcome;
pub mod attempt_output;
pub mod augmentation_key;
pub mod dto;
pub mod env_hashes;
pub mod flow_function_key;
pub mod input_resolution_budgets;
pub mod lowered_decl;
pub mod membership;
pub mod module_augmentation_observation;
mod module_reference_resolution;
pub mod module_resolution_observation;
mod module_resolver_core;
mod node_modules_resolution;
pub mod normalized_glob;
pub mod observation;
mod package_target_resolution;
pub mod path_probe;
pub mod path_utils;
mod preferred_specifier_resolution;
mod priority_frontier;
mod probe_path_resolution;
pub mod project_config;
mod project_ownership_resolution;
mod project_references_resolution;
pub mod project_stable_key;
mod provider_projection_resolution;
pub mod resolution_snapshot;
pub mod resolution_world_identity;
mod resolve_frame;
pub mod resolver_attempt_view;
mod source_id_resolution;
pub mod store_view_identity;
mod top_level_resolution;
mod tsconfig_paths_resolution;

pub use crate::resolution::ambient_symbol_hit::AmbientSymbolHit;
pub use crate::resolution::attempt_outcome::{
    AttemptFailure, AttemptOutcome, CanonicalId, CompletedAttempt, DeclarationSpace, InputKey,
    InputLoadIntegrityReason, KernelAttempt, LoadSet, ResolutionBasis, ResolutionWorldBasis,
    ResolverObservationKind,
};
pub use crate::resolution::attempt_output::{
    AmbientDependency, AttemptOutput, ConsumedResolutionObservationKey,
};
pub use crate::resolution::augmentation_key::{
    AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, ProjectIdentity,
};
pub use crate::resolution::dto::{
    ProjectOwnership, ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase,
    ResolveRequest, ResolveRequestKind, ResolveResult,
};
pub use crate::resolution::env_hashes::EnvHashes;
pub use crate::resolution::flow_function_key::FlowFunctionObservationKey;
pub use crate::resolution::input_resolution_budgets::{
    InputResolutionBudgetError, InputResolutionBudgetExhaustion, InputResolutionBudgetMeter,
    InputResolutionBudgets, InputResolutionRetention,
};
pub use crate::resolution::lowered_decl::{LoweredTypeDecl, LoweredValueDecl, ValueBodyHashFact};
pub use crate::resolution::membership::{
    typescript_default_excludes, ConfiguredMembership, StaticMembershipSpec,
};
pub use crate::resolution::module_augmentation_observation::{
    AugmentationContributorObservation, ModuleAugmentationIndexObservation,
};
pub use crate::resolution::module_reference_resolution::{
    collect_resolvable_module_reference_specifiers, resolve_known_module_reference_dependencies,
};
pub use crate::resolution::module_resolution_observation::ResolutionPackageManifest;
pub use crate::resolution::module_resolver_core::ModuleResolverCore;
pub use crate::resolution::node_modules_resolution::{ancestor_dirs, ancestor_dirs_from_dir};
pub use crate::resolution::normalized_glob::{CompiledGlob, NormalizedGlob};
pub use crate::resolution::observation::ResolverObservation;
pub use crate::resolution::path_probe::PathProbe;
pub use crate::resolution::path_utils::{
    build_known_file_index, carrier_api_provider_path, carrier_ide_provider_path,
    carrier_source_extensions, collapse_path, is_absolute_specifier, is_relative_specifier,
    join_paths, normalize_canonical_id, normalize_known_file_id, parent_dir, path_is_carrier,
    resolve_known_dependency_base, resolve_known_dependency_id, strip_carrier_extension,
    CARRIER_API_MODULE_SPECIFIER_SUFFIX, CARRIER_API_VIRTUAL_SUFFIX,
};
pub use crate::resolution::project_config::{
    canonical_lib_file_name, IdeProjectCompilerOptions, IdeProjectConfig,
    RawSemanticCompilerOptions, ScriptTarget, SemanticCompilerOptions, WorkspaceAlias,
};
pub use crate::resolution::project_stable_key::ProjectStableKey;
pub use crate::resolution::resolution_snapshot::ResolutionObservationSnapshot;
pub use crate::resolution::resolution_world_identity::{
    ResolutionPopulation, ResolutionWorldId, SessionFingerprint, WorkspaceAuthorityId,
};
pub use crate::resolution::resolve_frame::ResolveFrame;
pub use crate::resolution::resolver_attempt_view::ResolverAttemptView;
pub use crate::resolution::store_view_identity::{
    StoreViewOverlayIdentity, StoreViewProjectIdentity, StoreViewValidationToken,
};

#[must_use]
pub fn probe_extensions() -> &'static [&'static str] {
    crate::resolution::probe_path_resolution::probe_extensions_list()
}
