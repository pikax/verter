//! Immutable data-only observations for one resolver attempt.
//!
//! The sealed observation implementation owns only a captured resolution
//! snapshot and its basis. It cannot retain callbacks, trait objects, a host,
//! or scheduler state. Observation families outside the module-resolution
//! snapshot return a typed `ObservationUnavailable` terminal until their
//! immutable DTOs are supplied by the owning layer.

use crate::observation::sealed::Sealed;
use crate::ResolverObservation;
use std::sync::Arc;
use verter_session_query::resolution::{
    AttemptFailure, AttemptOutcome, AugmentationTargetKey, CanonicalId, EnvHashes,
    FlowFunctionObservationKey, LoweredTypeDecl, LoweredValueDecl,
    ModuleAugmentationIndexObservation, ResolutionBasis, ResolutionObservationSnapshot,
    ResolutionPackageManifest, ResolverObservationKind, StoreViewProjectIdentity,
};

/// The immutable `ResolverObservation` implementor owned by this kernel.
#[derive(Debug, Default)]
pub struct ResolverAttemptView {
    resolution_snapshot: Option<(Arc<ResolutionObservationSnapshot>, ResolutionBasis)>,
    input_resolution_budgets: verter_session_query::resolution::InputResolutionBudgets,
    input_resolution_retention: crate::InputResolutionRetention,
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
            verter_session_query::resolution::InputResolutionBudgets::default(),
        )
    }

    /// Builds a workspace attempt view carrying the same semantic-owned whole
    /// budget value as the operation ledger. Kernel frontiers consult this
    /// value before materializing a `LoadSet` too large for that ledger.
    #[must_use]
    pub fn from_resolution_snapshot_with_budgets(
        snapshot: Arc<ResolutionObservationSnapshot>,
        basis: ResolutionBasis,
        input_resolution_budgets: verter_session_query::resolution::InputResolutionBudgets,
    ) -> Self {
        let input_resolution_retention =
            crate::InputResolutionRetention::new(input_resolution_budgets);
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
        input_resolution_budgets: verter_session_query::resolution::InputResolutionBudgets,
        input_resolution_retention: crate::InputResolutionRetention,
    ) -> Self {
        Self {
            resolution_snapshot: Some((snapshot, basis)),
            input_resolution_budgets,
            input_resolution_retention,
        }
    }

    #[must_use]
    pub const fn input_resolution_budgets(
        &self,
    ) -> verter_session_query::resolution::InputResolutionBudgets {
        self.input_resolution_budgets
    }

    #[doc(hidden)]
    #[must_use]
    pub fn input_resolution_retention(&self) -> &crate::InputResolutionRetention {
        &self.input_resolution_retention
    }

    /// [`ResolverObservation::path_probe`] for a path already held as a
    /// shared string: a missing input is reported as its one key, which
    /// shares the string instead of copying it.
    pub(crate) fn path_probe_shared(
        &self,
        path: &Arc<str>,
    ) -> SnapshotRead<'_, verter_session_query::resolution::PathProbe> {
        self.probe_path(path, || Arc::clone(path))
    }

    /// [`ResolverObservation::real_path`] for a path already held as a
    /// shared string.
    pub(crate) fn real_path_shared(
        &self,
        path: &Arc<str>,
    ) -> SnapshotRead<'_, Option<CanonicalId>> {
        self.resolve_real_path(path, || Arc::clone(path))
    }

    fn probe_path(
        &self,
        path: &str,
        key: impl FnOnce() -> Arc<str>,
    ) -> SnapshotRead<'_, verter_session_query::resolution::PathProbe> {
        let Some((snapshot, basis)) = &self.resolution_snapshot else {
            return SnapshotRead::Unavailable(ResolverObservationKind::PathProbe);
        };
        snapshot.path_probe(path).map_or_else(
            || SnapshotRead::Missing(MissingPathInput::PathProbe(key()), basis),
            SnapshotRead::Known,
        )
    }

    fn resolve_real_path(
        &self,
        path: &str,
        key: impl FnOnce() -> Arc<str>,
    ) -> SnapshotRead<'_, Option<CanonicalId>> {
        let Some((snapshot, basis)) = &self.resolution_snapshot else {
            return SnapshotRead::Unavailable(ResolverObservationKind::RealPath);
        };
        snapshot.real_path(path).map_or_else(
            || SnapshotRead::Missing(MissingPathInput::RealPath(key()), basis),
            SnapshotRead::Known,
        )
    }
}

/// One read of the attempt's resolution snapshot.
pub(crate) enum SnapshotRead<'a, T> {
    Known(T),
    /// The snapshot does not hold the input yet: the one input to load,
    /// under the snapshot's basis.
    Missing(MissingPathInput, &'a ResolutionBasis),
    /// The view carries no resolution snapshot.
    Unavailable(ResolverObservationKind),
}

/// A path input the snapshot does not hold, before it is spelled as its key.
pub(crate) enum MissingPathInput {
    PathProbe(Arc<str>),
    RealPath(Arc<str>),
}

impl MissingPathInput {
    pub(crate) fn into_key(self) -> verter_session_query::resolution::InputKey {
        match self {
            Self::PathProbe(path) => verter_session_query::resolution::InputKey::PathProbe { path },
            Self::RealPath(path) => verter_session_query::resolution::InputKey::RealPath { path },
        }
    }
}

impl<T> SnapshotRead<'_, T> {
    /// The observation-protocol answer: a missing input is a one-key load set.
    pub(crate) fn into_outcome(self) -> AttemptOutcome<T> {
        match self {
            Self::Known(value) => AttemptOutcome::Complete(value),
            Self::Missing(input, basis) => AttemptOutcome::NeedInputs(
                verter_session_query::resolution::LoadSet::new(vec![input.into_key()], *basis),
            ),
            Self::Unavailable(observation) => unavailable(observation),
        }
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
    ) -> AttemptOutcome<Option<verter_session_query::analysis::types::Hash16>> {
        unavailable(ResolverObservationKind::WholeHash)
    }

    fn workspace_is_package_backed(&self, _canonical: &str) -> AttemptOutcome<bool> {
        unavailable(ResolverObservationKind::WorkspaceIsPackageBacked)
    }

    fn lookup_ambient_symbol(
        &self,
        _consumer_project: verter_session_query::resolution::ProjectStableKey,
        _symbol: &str,
    ) -> AttemptOutcome<Option<verter_session_query::resolution::AmbientSymbolHit>> {
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
    ) -> AttemptOutcome<Option<verter_session_query::flow::bundle::SkeletonRef>> {
        unavailable(ResolverObservationKind::FunctionBodySkeleton)
    }

    fn path_probe(
        &self,
        path: &str,
    ) -> AttemptOutcome<verter_session_query::resolution::PathProbe> {
        self.probe_path(path, || Arc::from(path)).into_outcome()
    }

    fn real_path(&self, path: &str) -> AttemptOutcome<Option<CanonicalId>> {
        self.resolve_real_path(path, || Arc::from(path))
            .into_outcome()
    }

    fn package_manifest(
        &self,
        directory: &str,
    ) -> AttemptOutcome<Option<Arc<ResolutionPackageManifest>>> {
        if let Some((snapshot, basis)) = &self.resolution_snapshot {
            return snapshot.package_manifest(directory).map_or_else(
                || {
                    AttemptOutcome::NeedInputs(verter_session_query::resolution::LoadSet::new(
                        vec![
                            verter_session_query::resolution::InputKey::PackageManifest {
                                directory: Arc::from(directory),
                            },
                        ],
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
