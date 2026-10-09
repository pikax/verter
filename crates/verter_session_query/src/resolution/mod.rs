//! Owned module-resolution inputs, answers, identities and observation records.
//! Routing and candidate search are implemented by `verter_resolution`.

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
pub mod module_resolution_observation;
pub mod normalized_glob;
pub mod path_probe;
pub mod path_utils;
pub mod project_config;
pub mod project_id;
pub mod project_stable_key;
pub mod resolution_snapshot;
pub mod resolution_world_identity;
pub mod store_view_identity;
pub mod unresolved;

pub use crate::resolution::ambient_symbol_hit::AmbientSymbolHit;
pub use crate::resolution::attempt_outcome::{
    AttemptFailure, AttemptOutcome, CanonicalId, DeclarationSpace, InputKey,
    InputLoadIntegrityReason, LoadSet, ResolutionBasis, ResolutionWorldBasis,
    ResolverObservationKind,
};
pub use crate::resolution::attempt_output::{AmbientDependency, ConsumedResolutionObservationKey};
pub use crate::resolution::augmentation_key::{
    AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, AugmenterEntry,
    AugmenterSet, ProjectIdentity,
};
pub use crate::resolution::dto::{
    ProjectOwnership, ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase,
    ResolveRequest, ResolveRequestKind, ResolveResult,
};
pub use crate::resolution::env_hashes::EnvHashes;
pub use crate::resolution::flow_function_key::FlowFunctionObservationKey;
pub use crate::resolution::input_resolution_budgets::{
    InputResolutionBudgetError, InputResolutionBudgetExhaustion, InputResolutionBudgetMeter,
    InputResolutionBudgets,
};
pub use crate::resolution::lowered_decl::{LoweredTypeDecl, LoweredValueDecl, ValueBodyHashFact};
pub use crate::resolution::membership::{
    typescript_default_excludes, ConfiguredMembership, StaticMembershipSpec,
};
pub use crate::resolution::module_augmentation_observation::{
    AugmentationContributorObservation, ModuleAugmentationIndexObservation,
};
pub use crate::resolution::module_resolution_observation::ResolutionPackageManifest;
pub use crate::resolution::normalized_glob::{CompiledGlob, NormalizedGlob};
pub use crate::resolution::path_probe::PathProbe;
pub use crate::resolution::path_utils::{
    build_known_file_index, carrier_api_provider_path, carrier_ide_provider_path,
    carrier_source_extensions, collapse_path, is_absolute_specifier, is_relative_specifier,
    join_paths, normalize_canonical_id, normalize_known_file_id, parent_dir, path_is_carrier,
    resolve_known_dependency_base, strip_carrier_extension, CARRIER_API_MODULE_SPECIFIER_SUFFIX,
    CARRIER_API_VIRTUAL_SUFFIX,
};
pub use crate::resolution::project_config::{
    canonical_lib_file_name, IdeProjectCompilerOptions, IdeProjectConfig,
    RawSemanticCompilerOptions, ScriptTarget, SemanticCompilerOptions, WorkspaceAlias,
};
pub use crate::resolution::project_id::ProjectId;
pub use crate::resolution::project_stable_key::ProjectStableKey;
pub use crate::resolution::resolution_snapshot::ResolutionObservationSnapshot;
pub use crate::resolution::resolution_world_identity::{
    ResolutionPopulation, ResolutionWorldId, SessionFingerprint, WorkspaceAuthorityId,
};
pub use crate::resolution::store_view_identity::{
    StoreViewOverlayIdentity, StoreViewProjectIdentity, StoreViewValidationToken,
};
