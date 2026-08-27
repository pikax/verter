//! Immutable data-only observations for one resolver attempt.
//!
//! The sealed observation implementation owns only a captured resolution
//! snapshot and its basis. It cannot retain callbacks, trait objects, a host,
//! or scheduler state. Observation families outside the module-resolution
//! snapshot return a typed `ObservationUnavailable` terminal until their
//! immutable DTOs are supplied by the owning layer.

use crate::resolver_core::observation::sealed::Sealed;
use crate::resolver_core::{
    AttemptFailure, AttemptOutcome, AugmentationTargetKey, CanonicalId, EnvHashes,
    FlowFunctionObservationKey, LoweredTypeDecl, LoweredValueDecl,
    ModuleAugmentationIndexObservation, ResolutionBasis, ResolutionObservationSnapshot,
    ResolutionPackageManifest, ResolverObservation, ResolverObservationKind,
    StoreViewProjectIdentity,
};
use std::sync::Arc;

/// The one semantic-owned `ResolverObservation` implementor.
#[derive(Debug, Default)]
pub struct ResolverAttemptView {
    resolution_snapshot: Option<(Arc<ResolutionObservationSnapshot>, ResolutionBasis)>,
    input_resolution_budgets: crate::resolver_core::InputResolutionBudgets,
    input_resolution_retention: crate::resolver_core::InputResolutionRetention,
}

impl ResolverAttemptView {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the closure-free workspace observation view used by a retrying
    /// module-resolution driver.
    #[must_use]
    pub fn from_resolution_snapshot(
        snapshot: Arc<ResolutionObservationSnapshot>,
        basis: ResolutionBasis,
    ) -> Self {
        Self::from_resolution_snapshot_with_budgets(
            snapshot,
            basis,
            crate::resolver_core::InputResolutionBudgets::default(),
        )
    }

    /// Builds a workspace attempt view carrying the same semantic-owned whole
    /// budget value as the operation ledger. Kernel frontiers consult this
    /// value before materializing a `LoadSet` too large for that ledger.
    #[must_use]
    pub fn from_resolution_snapshot_with_budgets(
        snapshot: Arc<ResolutionObservationSnapshot>,
        basis: ResolutionBasis,
        input_resolution_budgets: crate::resolver_core::InputResolutionBudgets,
    ) -> Self {
        let input_resolution_retention =
            crate::resolver_core::InputResolutionRetention::new(input_resolution_budgets);
        Self::from_resolution_snapshot_with_operation_retention(
            snapshot,
            basis,
            input_resolution_budgets,
            input_resolution_retention,
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub fn from_resolution_snapshot_with_operation_retention(
        snapshot: Arc<ResolutionObservationSnapshot>,
        basis: ResolutionBasis,
        input_resolution_budgets: crate::resolver_core::InputResolutionBudgets,
        input_resolution_retention: crate::resolver_core::InputResolutionRetention,
    ) -> Self {
        Self {
            resolution_snapshot: Some((snapshot, basis)),
            input_resolution_budgets,
            input_resolution_retention,
        }
    }

    #[must_use]
    pub const fn input_resolution_budgets(&self) -> crate::resolver_core::InputResolutionBudgets {
        self.input_resolution_budgets
    }

    #[doc(hidden)]
    #[must_use]
    pub fn input_resolution_retention(&self) -> &crate::resolver_core::InputResolutionRetention {
        &self.input_resolution_retention
    }
}

impl Sealed for ResolverAttemptView {}

impl ResolverObservation for ResolverAttemptView {
    fn env_hashes(&self, _canonical: Option<&str>) -> AttemptOutcome<EnvHashes> {
        unavailable(ResolverObservationKind::EnvHashes)
    }

    fn project_identity(
        &self,
        _canonical: Option<&str>,
    ) -> AttemptOutcome<StoreViewProjectIdentity> {
        unavailable(ResolverObservationKind::ProjectIdentity)
    }

    fn whole_hash(
        &self,
        _canonical: &str,
    ) -> AttemptOutcome<Option<crate::analysis::types::Hash16>> {
        unavailable(ResolverObservationKind::WholeHash)
    }

    fn workspace_is_package_backed(&self, _canonical: &str) -> AttemptOutcome<bool> {
        unavailable(ResolverObservationKind::WorkspaceIsPackageBacked)
    }

    fn lookup_ambient_symbol(
        &self,
        _consumer_project: crate::resolver_core::ProjectStableKey,
        _symbol: &str,
    ) -> AttemptOutcome<Option<crate::resolver_core::AmbientSymbolHit>> {
        unavailable(ResolverObservationKind::LookupAmbientSymbol)
    }

    fn project_generation(&self) -> AttemptOutcome<u64> {
        unavailable(ResolverObservationKind::ProjectGeneration)
    }

    fn type_decl(
        &self,
        _canonical: &str,
        _owner: verter_type_expr::TopLevelOwnerId,
        _name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredTypeDecl>>> {
        unavailable(ResolverObservationKind::TypeDecl)
    }

    fn value_decl(
        &self,
        _canonical: &str,
        _owner: verter_type_expr::TopLevelOwnerId,
        _name: &str,
    ) -> AttemptOutcome<Option<Arc<LoweredValueDecl>>> {
        unavailable(ResolverObservationKind::ValueDecl)
    }

    fn module_augmentation_index(
        &self,
        _target: &AugmentationTargetKey,
    ) -> AttemptOutcome<ModuleAugmentationIndexObservation> {
        unavailable(ResolverObservationKind::ModuleAugmentationIndex)
    }

    fn function_body_skeleton(
        &self,
        _key: &FlowFunctionObservationKey,
    ) -> AttemptOutcome<Option<Arc<crate::analysis::flow::FunctionBodySkeleton>>> {
        unavailable(ResolverObservationKind::FunctionBodySkeleton)
    }

    fn path_probe(&self, path: &str) -> AttemptOutcome<crate::resolver_core::PathProbe> {
        if let Some((snapshot, basis)) = &self.resolution_snapshot {
            return snapshot.path_probe(path).map_or_else(
                || {
                    AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                        vec![crate::resolver_core::InputKey::PathProbe {
                            path: Arc::from(path),
                        }],
                        *basis,
                    ))
                },
                AttemptOutcome::Complete,
            );
        }
        unavailable(ResolverObservationKind::PathProbe)
    }

    fn real_path(&self, path: &str) -> AttemptOutcome<Option<CanonicalId>> {
        if let Some((snapshot, basis)) = &self.resolution_snapshot {
            return snapshot.real_path(path).map_or_else(
                || {
                    AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                        vec![crate::resolver_core::InputKey::RealPath {
                            path: Arc::from(path),
                        }],
                        *basis,
                    ))
                },
                AttemptOutcome::Complete,
            );
        }
        unavailable(ResolverObservationKind::RealPath)
    }

    fn package_manifest(
        &self,
        directory: &str,
    ) -> AttemptOutcome<Option<Arc<ResolutionPackageManifest>>> {
        if let Some((snapshot, basis)) = &self.resolution_snapshot {
            return snapshot.package_manifest(directory).map_or_else(
                || {
                    AttemptOutcome::NeedInputs(crate::resolver_core::LoadSet::new(
                        vec![crate::resolver_core::InputKey::PackageManifest {
                            directory: Arc::from(directory),
                        }],
                        *basis,
                    ))
                },
                AttemptOutcome::Complete,
            );
        }
        unavailable(ResolverObservationKind::PackageManifest)
    }
}

fn unavailable<T>(observation: ResolverObservationKind) -> AttemptOutcome<T> {
    AttemptOutcome::Terminal(AttemptFailure::ObservationUnavailable { observation })
}

#[cfg(test)]
#[path = "resolver_attempt_view_tests.rs"]
mod resolver_attempt_view_tests;
