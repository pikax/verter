use std::sync::Arc;

use super::ResolverAttemptView;
use crate::resolver_core::{
    AttemptFailure, AttemptOutcome, InputKey, PathProbe, ResolutionBasis,
    ResolutionObservationSnapshot, ResolutionPackageManifest, ResolutionWorldBasis,
    ResolverObservation, ResolverObservationKind,
};

fn basis() -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    )
}

fn assert_unavailable<T>(outcome: AttemptOutcome<T>, observation: ResolverObservationKind) {
    assert!(matches!(
        outcome,
        AttemptOutcome::Terminal(AttemptFailure::ObservationUnavailable {
            observation: actual
        }) if actual == observation
    ));
}

#[test]
fn empty_view_has_only_typed_unavailable_observations() {
    let view = ResolverAttemptView::new();
    assert_unavailable(view.env_hashes(None), ResolverObservationKind::EnvHashes);
    assert_unavailable(
        view.project_identity(None),
        ResolverObservationKind::ProjectIdentity,
    );
    assert_unavailable(
        view.whole_hash("/p/a.ts"),
        ResolverObservationKind::WholeHash,
    );
    assert_unavailable(
        view.workspace_is_package_backed("/p/a.ts"),
        ResolverObservationKind::WorkspaceIsPackageBacked,
    );
    assert_unavailable(
        view.project_generation(),
        ResolverObservationKind::ProjectGeneration,
    );
    assert_unavailable(
        view.path_probe("/p/a.ts"),
        ResolverObservationKind::PathProbe,
    );
    assert_unavailable(view.real_path("/p/a.ts"), ResolverObservationKind::RealPath);
    assert_unavailable(
        view.package_manifest("/p"),
        ResolverObservationKind::PackageManifest,
    );
}

#[test]
fn snapshot_returns_owned_data_and_requests_only_missing_keys() {
    let manifest = Arc::new(ResolutionPackageManifest {
        main: Some("index.js".to_string()),
        module: None,
        types: None,
        typings: None,
        exports: None,
        imports: None,
    });
    let mut snapshot = ResolutionObservationSnapshot::default();
    snapshot.insert_path_probe("/p/a.ts".to_string(), PathProbe::File);
    snapshot.insert_real_path("/p/a.ts".to_string(), Some(Arc::from("/real/a.ts")));
    snapshot.insert_package_manifest("/p".to_string(), Some(Arc::clone(&manifest)));
    let view = ResolverAttemptView::from_resolution_snapshot(Arc::new(snapshot), basis());

    assert_eq!(
        view.path_probe("/p/a.ts"),
        AttemptOutcome::Complete(PathProbe::File)
    );
    assert_eq!(
        view.real_path("/p/a.ts"),
        AttemptOutcome::Complete(Some(Arc::from("/real/a.ts")))
    );
    assert_eq!(
        view.package_manifest("/p"),
        AttemptOutcome::Complete(Some(manifest))
    );
    assert!(matches!(
        view.path_probe("/p/missing.ts"),
        AttemptOutcome::NeedInputs(load)
            if load.basis() == basis()
                && load.keys() == [InputKey::PathProbe { path: Arc::from("/p/missing.ts") }]
    ));
    assert_unavailable(
        view.project_generation(),
        ResolverObservationKind::ProjectGeneration,
    );
}

#[test]
fn snapshot_view_is_send_sync_data() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ResolverAttemptView>();
}
