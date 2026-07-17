//! Unit tests for the authoritative source-membership reconciler.
//!
//! Each test drives the REAL [`MembershipReconciler`] API and asserts against the
//! [`MembershipLedger`] state directly (the `getExternalFiles` end-to-end read path
//! is a later step). Every test names the implementation defect that makes it RED.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use verter_session::external_ts::{
    AmbiguityCause, CarrierOwnershipResolution, EnvDims, ProjectBinding, ScriptKind, SnapshotRole,
};
use verter_session::file_artifact_store::ProjectIdentity;

use super::{
    CarrierMembershipCommitter, CommitFuture, MembershipReconciler, ReconcileErr, ReconcileOutcome,
    ReconcileReason,
};
use crate::external_ts::membership_ledger::{
    AbsentReason, CanonicalSource, LedgerCompanion, MembershipLedger, MembershipRecord, ProjectUri,
};
use crate::external_ts::publish_coordinator::CarrierPublishError;
use crate::external_ts::CarrierCompanion;
use crate::type_provider::mock::{FailingTypeProvider, MockCall, MockTypeProvider};
use crate::type_provider::traits::TypeProvider;

/// A recording membership-committer mock — records each `commit_owned` / `retract`
/// and can be armed to fail, so the reconciler tests assert the membership is
/// committed as part of an authoritative transition without standing up a real
/// engine backend.
#[derive(Default)]
struct RecordingMembershipCommitter {
    committed: parking_lot::Mutex<Vec<String>>,
    retracted: parking_lot::Mutex<Vec<String>>,
    fail: AtomicBool,
}

impl RecordingMembershipCommitter {
    fn arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn arm_failure(&self) {
        self.fail.store(true, Ordering::SeqCst);
    }
}

impl CarrierMembershipCommitter for RecordingMembershipCommitter {
    fn commit_owned<'a>(
        &'a self,
        _binding: &'a ProjectBinding,
        source_canonical: &'a str,
        _companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(CarrierPublishError::Publish("armed commit failure".into()));
            }
            self.committed.lock().push(source_canonical.to_string());
            Ok(())
        })
    }

    fn retract<'a>(&'a self, source_canonical: &'a str) -> CommitFuture<'a> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(CarrierPublishError::Retract("armed commit failure".into()));
            }
            self.retracted.lock().push(source_canonical.to_string());
            Ok(())
        })
    }
}

/// A committer that BLOCKS inside `commit_owned` until the test releases a permit,
/// signalling on entry — used to prove per-source serialization deterministically. A
/// transition that reaches the committer sends one `entered` event and then parks on
/// the release semaphore WHILE holding the reconciler's per-source gate, so the test
/// can observe whether a concurrent same-source transition also reaches the committer.
struct SerializingCommitter {
    entered_tx: tokio::sync::mpsc::UnboundedSender<()>,
    release: Arc<tokio::sync::Semaphore>,
}

impl CarrierMembershipCommitter for SerializingCommitter {
    fn commit_owned<'a>(
        &'a self,
        _binding: &'a ProjectBinding,
        _source_canonical: &'a str,
        _companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        Box::pin(async move {
            // Reached the committer — inside the per-source gate.
            let _ = self.entered_tx.send(());
            // Park (holding the gate) until the test hands out a permit.
            self.release
                .acquire()
                .await
                .expect("release semaphore is never closed")
                .forget();
            Ok(())
        })
    }

    fn retract<'a>(&'a self, _source_canonical: &'a str) -> CommitFuture<'a> {
        Box::pin(async move { Ok(()) })
    }
}

/// A resolved `ProjectBinding` for `project` (test-only seam). The env dims are
/// inert (the mock committer ignores them); only `tsconfig_uri == project`
/// matters, since the reconciler derives the ledger's project from it.
fn test_binding(project: &str) -> ProjectBinding {
    let env_dims = EnvDims {
        parse_env_hash: [0u8; 16],
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        project_identity: ProjectIdentity([0u8; 16]),
    };
    ProjectBinding::new_for_test(
        "/proj",
        project,
        "5.9.0",
        env_dims,
        Vec::new(),
        verter_workspace::ProjectId(0),
        verter_workspace::SnapshotGeneration(1),
    )
}

/// A `Bound` ownership resolution for `project` (the single state in which a carrier
/// is advertised). Companions are passed separately to `reconcile_source_membership`.
fn bound(project: &str) -> CarrierOwnershipResolution {
    CarrierOwnershipResolution::Bound(test_binding(project))
}

// ── test fixtures ───────────────────────────────────────────────────────────

/// A carrier companion with the given path / role / script kind.
fn companion(provider_uri: &str, role: SnapshotRole, script_kind: ScriptKind) -> CarrierCompanion {
    CarrierCompanion {
        provider_uri: Arc::from(provider_uri),
        content: Arc::from("/* carrier content */"),
        map_json: None,
        role,
        script_kind,
        version: 1,
    }
}

/// The IDE `.tsx` carrier companion for a path (the common single-companion case).
fn ide(provider_uri: &str) -> CarrierCompanion {
    companion(provider_uri, SnapshotRole::CarrierIde, ScriptKind::Tsx)
}

/// A fresh, empty per-source membership-gate map for a standalone test reconciler.
/// Production shares ONE map via the coordinator; a test reconciler used once (or
/// concurrently as a single shared instance) is correctly served by its own map.
fn fresh_source_gates() -> Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>> {
    Arc::new(dashmap::DashMap::new())
}

/// Build a reconciler over a fresh ledger and the given provider, returning the
/// shared ledger handle for assertions.
fn reconciler_with(
    provider: Arc<dyn TypeProvider>,
) -> (MembershipReconciler, Arc<MembershipLedger>) {
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let reconciler = MembershipReconciler::new(
        Arc::clone(&ledger),
        provider,
        RecordingMembershipCommitter::arc(),
        fresh_source_gates(),
    );
    (reconciler, ledger)
}

/// Like [`reconciler_with`] but also returns the membership-committer handle for
/// tests that assert the membership was committed (or arm a commit failure).
fn reconciler_with_committer(
    provider: Arc<dyn TypeProvider>,
) -> (
    MembershipReconciler,
    Arc<MembershipLedger>,
    Arc<RecordingMembershipCommitter>,
) {
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let committer = RecordingMembershipCommitter::arc();
    let reconciler = MembershipReconciler::new(
        Arc::clone(&ledger),
        provider,
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        fresh_source_gates(),
    );
    (reconciler, ledger, committer)
}

/// Advertise `source` under `project` with `companions` through the real reconciler.
async fn advertise(
    reconciler: &MembershipReconciler,
    source: &CanonicalSource,
    project: &str,
    companions: Vec<CarrierCompanion>,
) {
    // The `#[must_use]` outcome is intentionally discarded by this test helper.
    let _ = reconciler
        .reconcile_source_membership(
            source,
            bound(project),
            companions,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("advertise should succeed");
}

// ── Owned ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn extension_flip_closes_stale_companion_before_registering_the_new_one() {
    // DISCRIMINATION: a `.tsx` → `.jsx` companion EXTENSION FLIP for the SAME
    // carrier (not an owner change) must close the stale `.tsx` BEFORE
    // registering the new `.jsx`. tsserver's wildcard membership check
    // excludes a newly opened `.jsx` while a same-stem `.tsx` is still a
    // program member ("Detected output file" — the `.jsx` looks like the
    // `.tsx`'s emit), so register-first strands the `.jsx` in an inferred
    // project and every projectFileName-targeted query fails closed. An
    // owner CHANGE (different stem) keeps the register-first ordering so the
    // new project's buffer is live before the old one drops.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, _ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;
    mock.clear_calls();

    // The carrier's script kind is corrected to JS: the IDE companion flips
    // `.tsx` → `.jsx` under the SAME project.
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![companion(
            "/proj/src/Comp.vue.jsx",
            SnapshotRole::CarrierIde,
            ScriptKind::Jsx,
        )],
    )
    .await;

    let calls = mock.calls();
    let close_pos = calls
        .iter()
        .position(
            |call| matches!(call, MockCall::CloseFile { path } if path == "/proj/src/Comp.vue.tsx"),
        )
        .expect("the stale .tsx must be closed");
    let register_pos = calls
        .iter()
        .position(|call| matches!(call, MockCall::RegisterCarrierMember { companion_path, .. } if companion_path == "/proj/src/Comp.vue.jsx"))
        .expect("the new .jsx must be registered");
    assert!(
        close_pos < register_pos,
        "the stale .tsx must close BEFORE the .jsx registers (tsserver output-file exclusion): {calls:?}"
    );
}

#[tokio::test]
async fn owner_change_registers_new_companion_before_closing_the_old_one() {
    // The counter-ordering guard: an owner CHANGE (same companion identity,
    // different owning project) keeps the register-first ordering — the new
    // project's buffer goes live before the old one drops, so a query landing
    // mid-transition never finds a dead window.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, _ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;
    mock.clear_calls();

    advertise(
        &reconciler,
        &source,
        "/other/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;

    let calls = mock.calls();
    let register_pos = calls
        .iter()
        .position(|call| matches!(call, MockCall::RegisterCarrierMember { companion_path, project_file_name, .. } if companion_path == "/proj/src/Comp.vue.tsx" && project_file_name == "/other/tsconfig.json"))
        .expect("the companion must re-register under the new project");
    // No close of the companion may precede that re-registration.
    assert!(
        !calls[..register_pos].iter().any(
            |call| matches!(call, MockCall::CloseFile { path } if path == "/proj/src/Comp.vue.tsx")
        ),
        "an owner change must register the new buffer before closing the old one: {calls:?}"
    );
}

#[tokio::test]
async fn owned_advertises_exactly_its_companions_under_project() {
    // DISCRIMINATION: an impl that skips the ledger commit, registers under the wrong
    // project, or drops a companion leaves the source un-advertised / mis-keyed —
    // the asserts below go RED.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    let project = "/proj/tsconfig.json";
    let companions = vec![
        ide("/proj/src/Comp.vue.tsx"),
        companion(
            "/proj/src/Comp.vue.verter.ts",
            SnapshotRole::CarrierApi,
            ScriptKind::Ts,
        ),
    ];

    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            bound(project),
            companions,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("owned reconcile should succeed");

    match outcome {
        ReconcileOutcome::Advertised {
            project: p,
            companions,
            replaced,
            ..
        } => {
            assert_eq!(p.as_str(), project);
            assert_eq!(companions, 2);
            assert!(replaced.is_none(), "first advertisement replaces nothing");
        }
        other => panic!("expected Advertised, got {other:?}"),
    }

    assert!(ledger.is_advertised(&source));
    assert_eq!(
        ledger.advertised_under(&ProjectUri::from(project)),
        vec![source.clone()]
    );

    // The provider-buffer transition went through the actor command API:
    // `register_carrier_member` for each companion, with the owning project as the
    // `project_file_name`.
    let registered: Vec<(String, String)> = mock
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            MockCall::RegisterCarrierMember {
                companion_path,
                project_file_name,
                ..
            } => Some((companion_path, project_file_name)),
            _ => None,
        })
        .collect();
    assert_eq!(registered.len(), 2, "both companions registered");
    assert!(registered.iter().all(|(_, proj)| proj == project));
    assert!(registered
        .iter()
        .any(|(p, _)| p == "/proj/src/Comp.vue.tsx"));
    assert!(registered
        .iter()
        .any(|(p, _)| p == "/proj/src/Comp.vue.verter.ts"));
}

#[tokio::test]
async fn store_only_membership_commits_without_a_local_provider_actor() {
    // The editor-owned tsserver reads membership directly through its plugin. This
    // route must complete the durable transaction and mint readiness without a fake
    // local TypeProvider merely to satisfy the reconciler's shape.
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let committer = RecordingMembershipCommitter::arc();
    let reconciler = MembershipReconciler::new_store_only(
        Arc::clone(&ledger),
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        fresh_source_gates(),
    );
    let source = CanonicalSource::from("/proj/src/EditorOwned.vue");
    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            bound("/proj/tsconfig.json"),
            vec![ide("/proj/src/EditorOwned.vue.tsx")],
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("store-only membership must not require a local provider");

    assert!(matches!(outcome, ReconcileOutcome::Advertised { .. }));
    assert!(ledger.is_advertised(&source));
    assert_eq!(
        committer.committed.lock().as_slice(),
        ["/proj/src/EditorOwned.vue"]
    );

    let removed = reconciler
        .remove_source_membership(&source, AbsentReason::Deleted)
        .await
        .expect("store-only retraction must not require a local provider");
    assert!(matches!(removed, ReconcileOutcome::Tombstoned { .. }));
    assert_eq!(
        committer.retracted.lock().as_slice(),
        ["/proj/src/EditorOwned.vue"]
    );
}

#[tokio::test]
async fn owner_change_a_to_b_atomically_replaces_leaving_nothing_under_a() {
    // DISCRIMINATION: a publish-then-prune impl that ADDS B without removing the A
    // entry (e.g. a (source, project)-keyed store) leaves the source advertised under
    // A — `advertised_under(A)` would be non-empty. The source-indexed single-entry
    // swap keeps it empty.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    let project_a = "/proj/a/tsconfig.json";
    let project_b = "/proj/b/tsconfig.json";
    let companions = vec![ide("/proj/src/Comp.vue.tsx")];

    advertise(&reconciler, &source, project_a, companions.clone()).await;
    assert_eq!(
        ledger.advertised_under(&ProjectUri::from(project_a)),
        vec![source.clone()]
    );

    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            bound(project_b),
            companions.clone(),
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("advertise under B should succeed");

    match outcome {
        ReconcileOutcome::Advertised {
            ref project,
            ref replaced,
            ..
        } => {
            assert_eq!(project.as_str(), project_b);
            assert_eq!(
                replaced.as_ref().map(ProjectUri::as_str),
                Some(project_a),
                "an A→B change records the replaced project"
            );
        }
        other => panic!("expected Advertised, got {other:?}"),
    }

    assert!(
        ledger
            .advertised_under(&ProjectUri::from(project_a))
            .is_empty(),
        "nothing must remain advertised under the old project A"
    );
    assert_eq!(
        ledger.advertised_under(&ProjectUri::from(project_b)),
        vec![source.clone()]
    );
    match ledger.record_snapshot(&source) {
        Some(MembershipRecord::Advertised { project, .. }) => {
            assert_eq!(project.as_str(), project_b);
        }
        other => panic!("expected a single Advertised record under B, got {other:?}"),
    }
}

// ── Absent (each typed reason) ───────────────────────────────────────────────

/// Advertise a source, then transition it to `reason` (via ownership resolution for
/// the ownership-derived reasons, or via `remove_source_membership` for the terminal
/// reasons) and assert it is fully retracted.
///
/// DISCRIMINATION: the original owner-loss bug — a per-source publish that never
/// retracts on owner loss — leaves the source advertised. A forgotten tombstone
/// leaves no `Tombstone` record. A missing provider close leaves the buffer open.
/// All three are caught here.
async fn assert_absent_retracts(reason: AbsentReason, via_remove: bool) {
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;
    assert!(ledger.is_advertised(&source));
    mock.clear_calls();

    let outcome = if via_remove {
        reconciler
            .remove_source_membership(&source, reason)
            .await
            .expect("remove should succeed")
    } else {
        // The ownership-resolved absent reasons map onto their `CarrierOwnershipResolution`.
        let resolution = match reason {
            AbsentReason::NoProject => CarrierOwnershipResolution::NoProject,
            AbsentReason::Ambiguous => CarrierOwnershipResolution::Ambiguous {
                candidates: Vec::new(),
                cause: AmbiguityCause::MultipleOwners,
            },
            other => panic!("{other:?} is not an ownership-resolved absent reason"),
        };
        reconciler
            .reconcile_source_membership(
                &source,
                resolution,
                Vec::new(),
                ReconcileReason::SourceSynced,
            )
            .await
            .expect("absent reconcile should succeed")
    };

    match outcome {
        ReconcileOutcome::Tombstoned { reason: got, .. } => assert_eq!(got, reason),
        other => panic!("expected Tombstoned({reason:?}), got {other:?}"),
    }
    assert!(
        !ledger.is_advertised(&source),
        "an absent outcome must retract the advertisement"
    );
    assert!(
        matches!(
            ledger.record_snapshot(&source),
            Some(MembershipRecord::Tombstone { reason: got, .. }) if got == reason
        ),
        "an absent outcome must leave a typed tombstone"
    );
    assert!(
        mock.calls().iter().any(
            |call| matches!(call, MockCall::CloseFile { path } if path == "/proj/src/Comp.vue.tsx")
        ),
        "the advertised companion buffer must be closed through the actor API"
    );
}

#[tokio::test]
async fn absent_no_project_retracts() {
    assert_absent_retracts(AbsentReason::NoProject, false).await;
}

#[tokio::test]
async fn absent_ambiguous_retracts() {
    assert_absent_retracts(AbsentReason::Ambiguous, false).await;
}

#[tokio::test]
async fn absent_synthetic_scratch_retracts() {
    // SyntheticScratch is not an ownership-resolution outcome — it is an explicit
    // scratch-lane removal, so it flows through `remove_source_membership`.
    assert_absent_retracts(AbsentReason::SyntheticScratch, true).await;
}

#[tokio::test]
async fn absent_deleted_removes() {
    assert_absent_retracts(AbsentReason::Deleted, true).await;
}

#[tokio::test]
async fn absent_compile_failed_removes() {
    assert_absent_retracts(AbsentReason::CompileFailed, true).await;
}

#[tokio::test]
async fn absent_conflict_removed_removes() {
    assert_absent_retracts(AbsentReason::ConflictRemoved, true).await;
}

#[tokio::test]
async fn terminal_reason_short_circuits_without_resolving_ownership() {
    // DISCRIMINATION: an impl that routes on the passed resolution BEFORE checking the
    // caller-authoritative terminal reason would advertise under the (wrong) bound
    // project instead of tombstoning. Passing a `Bound(/wrong)` resolution with a
    // terminal `Deleted` reason and asserting a `Deleted` tombstone catches that
    // ordering bug.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;

    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            bound("/wrong/tsconfig.json"),
            vec![ide("/wrong/Comp.vue.tsx")],
            ReconcileReason::Deleted,
        )
        .await
        .expect("terminal delete should succeed");

    assert!(matches!(
        outcome,
        ReconcileOutcome::Tombstoned {
            reason: AbsentReason::Deleted,
            ..
        }
    ));
    assert!(!ledger.is_advertised(&source));
}

// ── Bootstrap (cold ownership) ───────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_unknown_defers_without_advertising_or_clean_success() {
    // DISCRIMINATION: an impl that maps cold ownership to NoProject (tombstone) or to
    // a clean Advertised would write a record or return Advertised — both caught here
    // (Deferred outcome + no ledger record + no provider transition).
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");

    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            CarrierOwnershipResolution::NotReady,
            Vec::new(),
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("bootstrap defers (it is not an error)");

    assert!(
        matches!(outcome, ReconcileOutcome::Deferred { .. }),
        "cold ownership must DEFER, not report clean success"
    );
    assert!(!ledger.is_advertised(&source));
    assert!(
        ledger.record_snapshot(&source).is_none(),
        "bootstrap must not mutate the ledger"
    );
    assert!(
        mock.calls().is_empty(),
        "bootstrap must not transition the provider buffer"
    );
}

#[tokio::test]
async fn bootstrap_does_not_thrash_an_existing_advertisement() {
    // DISCRIMINATION: the cold-start-vs-owner-loss conflation bug — treating a cold
    // snapshot as owner loss retracts a still-valid advertisement. Asserting the
    // source STAYS advertised after a bootstrap reconcile catches it.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;
    assert!(ledger.is_advertised(&source));
    mock.clear_calls();

    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            CarrierOwnershipResolution::NotReady,
            Vec::new(),
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("bootstrap defers");

    assert!(matches!(outcome, ReconcileOutcome::Deferred { .. }));
    assert!(
        ledger.is_advertised(&source),
        "a cold snapshot must NOT retract a still-valid advertisement"
    );
    assert!(
        mock.calls().is_empty(),
        "bootstrap must not touch the provider"
    );
}

// ── Fail-closed ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn ledger_commit_failure_returns_err_not_ok() {
    // DISCRIMINATION: an impl that returns Ok WITHOUT verifying the post-commit state
    // reports success even though the ledger did not reach desired. Arming the commit
    // fault + asserting Err + not-advertised + no-record catches the missing
    // post-commit verification.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    ledger.arm_commit_failure();

    let result = reconciler
        .reconcile_source_membership(
            &source,
            bound("/proj/tsconfig.json"),
            vec![ide("/proj/src/Comp.vue.tsx")],
            ReconcileReason::SourceSynced,
        )
        .await;

    assert!(
        matches!(result, Err(ReconcileErr::LedgerCommit { .. })),
        "a failed ledger commit must return Err, not Ok"
    );
    assert!(
        !ledger.is_advertised(&source),
        "a failed commit must not leave the source advertised"
    );
    assert!(
        ledger.record_snapshot(&source).is_none(),
        "a failed commit must not write a record"
    );
}

#[tokio::test]
async fn provider_transition_failure_does_not_commit_the_tombstone() {
    // DISCRIMINATION: an impl that commits the ledger regardless of (or before) the
    // provider transition would tombstone the source even though the buffer close
    // failed. A failing provider + a seeded advertisement + asserting Err +
    // still-advertised catches that ordering bug.
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    // Seed a prior advertisement directly through the ledger so the absent path has a
    // companion buffer it must close (the failing provider rejects that close).
    ledger
        .commit(
            &source,
            MembershipRecord::Advertised {
                project: ProjectUri::from("/proj/tsconfig.json"),
                companions: vec![LedgerCompanion {
                    provider_uri: Arc::from("/proj/src/Comp.vue.tsx"),
                    role: SnapshotRole::CarrierIde,
                    script_kind: ScriptKind::Tsx,
                }],
                lease: ledger.current_session(),
            },
        )
        .expect("seed advertisement");
    assert!(ledger.is_advertised(&source));

    let provider: Arc<dyn TypeProvider> = Arc::new(FailingTypeProvider::new("provider down"));
    let reconciler = MembershipReconciler::new(
        Arc::clone(&ledger),
        provider,
        RecordingMembershipCommitter::arc(),
        fresh_source_gates(),
    );

    let result = reconciler
        .remove_source_membership(&source, AbsentReason::Deleted)
        .await;

    assert!(
        matches!(result, Err(ReconcileErr::ProviderTransition { .. })),
        "a failed provider-buffer transition must return Err"
    );
    assert!(
        ledger.is_advertised(&source),
        "a failed provider close must NOT commit the tombstone"
    );
}

#[tokio::test]
async fn membership_commit_failure_returns_err_and_does_not_advertise() {
    // DISCRIMINATION: the reconciler folds in the membership commit (the engine's
    // advertised-membership surface — the tsserver plugin's cross-process content +
    // advertised set, or a future in-memory engine's overlay). If that commit fails,
    // the transition must fail closed — Err(MembershipCommit), no ledger
    // advertisement, no false success. An impl that committed the ledger regardless
    // of the membership commit would advertise a carrier the engine cannot serve.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger, committer) = reconciler_with_committer(mock.clone());
    committer.arm_failure();
    let source = CanonicalSource::from("/proj/src/Comp.vue");

    let result = reconciler
        .reconcile_source_membership(
            &source,
            bound("/proj/tsconfig.json"),
            vec![ide("/proj/src/Comp.vue.tsx")],
            ReconcileReason::SourceSynced,
        )
        .await;

    assert!(
        matches!(result, Err(ReconcileErr::MembershipCommit { .. })),
        "a failed membership commit must return Err(MembershipCommit), got {result:?}"
    );
    assert!(
        !ledger.is_advertised(&source),
        "a failed membership commit must NOT advertise in the ledger"
    );
    assert!(
        ledger.record_snapshot(&source).is_none(),
        "a failed membership commit must not write a ledger record"
    );
}

#[tokio::test]
async fn owned_commits_membership_before_advertising() {
    // DISCRIMINATION: an owned advertisement must also commit the carrier's
    // membership (so the engine can serve it), not only the ledger. An impl that
    // skipped the commit would leave `committed` empty.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger, committer) = reconciler_with_committer(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    let _ = reconciler
        .reconcile_source_membership(
            &source,
            bound("/proj/tsconfig.json"),
            vec![ide("/proj/src/Comp.vue.tsx")],
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("owned reconcile should succeed");

    assert_eq!(
        committer.committed.lock().as_slice(),
        ["/proj/src/Comp.vue".to_string()],
        "an owned advertisement must commit the source's membership"
    );
    assert!(ledger.is_advertised(&source));
}

/// A membership committer whose commit/retract futures cross a real `.await`
/// suspension point (`yield_now`) and, AFTER resuming, observe the shared ledger's
/// advertised state at the moment the commit runs. The reconciler drives the
/// transition in order — membership commit, THEN ledger commit — so a committer
/// future that is actually AWAITED to completion before the ledger commit must
/// observe the source as NOT-yet-advertised. This proves the seam is genuinely
/// async (the future is polled to completion across the await), not fire-and-forget.
struct AwaitOrderingCommitter {
    ledger: Arc<MembershipLedger>,
    /// The ledger's `is_advertised` value observed (post-yield) at commit time, for
    /// the source the commit named. `true` would mean the ledger committed BEFORE the
    /// membership commit future completed (a non-awaited / fire-and-forget seam).
    advertised_at_commit_time: parking_lot::Mutex<Vec<bool>>,
    /// Set once the commit future has fully run (post-yield) — absent if the future
    /// was dropped without being polled to completion.
    commit_completed: AtomicBool,
}

impl AwaitOrderingCommitter {
    fn arc(ledger: Arc<MembershipLedger>) -> Arc<Self> {
        Arc::new(Self {
            ledger,
            advertised_at_commit_time: parking_lot::Mutex::new(Vec::new()),
            commit_completed: AtomicBool::new(false),
        })
    }
}

impl CarrierMembershipCommitter for AwaitOrderingCommitter {
    fn commit_owned<'a>(
        &'a self,
        _binding: &'a ProjectBinding,
        source_canonical: &'a str,
        _companions: &'a [CarrierCompanion],
    ) -> CommitFuture<'a> {
        Box::pin(async move {
            // A genuine suspension point: only a reconciler that AWAITS this future to
            // completion observes the post-yield effects below before it commits the
            // ledger.
            tokio::task::yield_now().await;
            let observed = self
                .ledger
                .is_advertised(&CanonicalSource::from(source_canonical));
            self.advertised_at_commit_time.lock().push(observed);
            self.commit_completed.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    fn retract<'a>(&'a self, _source_canonical: &'a str) -> CommitFuture<'a> {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn membership_commit_future_is_awaited_before_ledger_commit() {
    // DISCRIMINATION: the commit seam is ASYNC. A reconciler that drives the
    // committer future to completion across its `.await` (the correct behavior)
    // observes the ledger as NOT-yet-advertised at commit time and records the
    // commit as completed. A sync-in-async-clothing / fire-and-forget seam that did
    // NOT await the future would either (a) leave `commit_completed` false (the
    // future was dropped unpolled) or (b) let the ledger commit race ahead of the
    // post-yield observation (`advertised_at_commit_time == true`). Both bugs go RED.
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let committer = AwaitOrderingCommitter::arc(Arc::clone(&ledger));
    let mock: Arc<dyn TypeProvider> = Arc::new(MockTypeProvider::new());
    let reconciler = MembershipReconciler::new(
        Arc::clone(&ledger),
        mock,
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        fresh_source_gates(),
    );
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    let outcome = reconciler
        .reconcile_source_membership(
            &source,
            bound("/proj/tsconfig.json"),
            vec![ide("/proj/src/Comp.vue.tsx")],
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("owned reconcile should succeed");
    assert!(matches!(outcome, ReconcileOutcome::Advertised { .. }));

    assert!(
        committer.commit_completed.load(Ordering::SeqCst),
        "the commit future must be AWAITED to completion (its post-yield body ran), \
         not dropped unpolled"
    );
    assert_eq!(
        committer.advertised_at_commit_time.lock().as_slice(),
        [false],
        "the membership commit future must complete BEFORE the ledger commit — at \
         commit time the source must NOT yet be advertised (a fire-and-forget seam \
         would let the ledger commit race ahead and observe `true`)"
    );
    // And after the awaited transition the ledger IS advertised (the commit and the
    // ledger commit both happened, in order).
    assert!(ledger.is_advertised(&source));
}

// ── Session epoch / lease ────────────────────────────────────────────────────

#[tokio::test]
async fn stale_session_lease_is_not_advertised() {
    // DISCRIMINATION: an impl that ignores the lease (treats any Advertised record as
    // advertised) reports the source advertised after a session advance. Asserting
    // is_advertised becomes false WHILE the record still exists catches that — the
    // lease, not deletion, gates advertisement.
    let mock = Arc::new(MockTypeProvider::new());
    let (reconciler, ledger) = reconciler_with(mock.clone());
    let source = CanonicalSource::from("/proj/src/Comp.vue");
    advertise(
        &reconciler,
        &source,
        "/proj/tsconfig.json",
        vec![ide("/proj/src/Comp.vue.tsx")],
    )
    .await;
    assert!(ledger.is_advertised(&source));

    let prior = ledger.current_session();
    let advanced = ledger.advance_session();
    assert_ne!(advanced, prior);

    assert!(
        !ledger.is_advertised(&source),
        "a stale old-session advertisement must not be treated as advertised"
    );
    assert!(
        matches!(
            ledger.record_snapshot(&source),
            Some(MembershipRecord::Advertised { .. })
        ),
        "the record still exists (stale, not deleted) — the lease gates advertisement"
    );
}

// ── Transition matrix (randomized sequences + failure injection) ──────────────
//
// Drives the REAL reconciler over deterministic pseudo-random transition
// SEQUENCES (a seeded LCG — the workspace's hand-rolled property-test idiom, no
// `proptest` crate) across a small set of sources and projects, maintaining a
// REFERENCE MODEL of the expected per-source advertisement and asserting the
// ledger matches the model after EVERY transition. Failure injection asserts
// every transition is fail-closed: an armed fault returns `Err` and leaves the
// model (the ledger's advertisement authority) untorn.

/// A deterministic LCG (same multiplier as the `verter_parser` proptests).
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state
}

/// Seed the LCG from a seed index (stable across platforms).
fn lcg_seed(seed: u64) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    hasher.finish()
}

/// The reference model of a source's expected ledger advertisement.
#[derive(Clone, PartialEq, Debug)]
enum Model {
    /// Not advertised (a tombstone or never advertised).
    NotAdv,
    /// Advertised under `projects[idx]` at the current session.
    AdvUnder(usize),
}

/// Apply transition `op` to `source` through a production reconciler entry point.
/// `op` encodes: 0 = owned under `proj`; 1/2 = an ownership-resolved absent
/// (NoProject / Ambiguous); 3 = SyntheticScratch (an explicit scratch-lane remove);
/// 4/5/6 = a caller-authoritative terminal (Deleted / CompileFailed /
/// ConflictRemoved); 7 = bootstrap (NotReady).
async fn apply_matrix_op(
    reconciler: &MembershipReconciler,
    source: &CanonicalSource,
    op: u64,
    proj: &str,
    companion: &str,
) -> Result<ReconcileOutcome, ReconcileErr> {
    let synced = ReconcileReason::SourceSynced;
    match op {
        0 => {
            reconciler
                .reconcile_source_membership(source, bound(proj), vec![ide(companion)], synced)
                .await
        }
        1..=2 => {
            let resolution = if op == 1 {
                CarrierOwnershipResolution::NoProject
            } else {
                CarrierOwnershipResolution::Ambiguous {
                    candidates: Vec::new(),
                    cause: AmbiguityCause::MultipleOwners,
                }
            };
            reconciler
                .reconcile_source_membership(source, resolution, Vec::new(), synced)
                .await
        }
        3 => {
            // SyntheticScratch is not an ownership-resolution outcome — it is an
            // explicit scratch-lane removal.
            reconciler
                .remove_source_membership(source, AbsentReason::SyntheticScratch)
                .await
        }
        4..=6 => {
            let reason = match op {
                4 => ReconcileReason::Deleted,
                5 => ReconcileReason::CompileFailed,
                _ => ReconcileReason::ConflictRemoved,
            };
            // The resolution is intentionally a wrong bound decision: a terminal
            // reason MUST short-circuit to a tombstone without routing on it.
            reconciler
                .reconcile_source_membership(
                    source,
                    bound("/never/tsconfig.json"),
                    Vec::new(),
                    reason,
                )
                .await
        }
        7 => {
            reconciler
                .reconcile_source_membership(
                    source,
                    CarrierOwnershipResolution::NotReady,
                    Vec::new(),
                    synced,
                )
                .await
        }
        _ => unreachable!("op is bounded by % 8"),
    }
}

/// Assert the whole ledger matches the reference model: each source is advertised
/// iff the model says so, under exactly the modelled project (nothing under any
/// other), and its companion provider path is in that project's advertised set.
fn assert_matches_model(
    ledger: &MembershipLedger,
    model: &[Model],
    sources: &[CanonicalSource],
    projects: &[&str],
    companions: &[String],
    seed: u64,
) {
    for (i, entry) in model.iter().enumerate() {
        match entry {
            Model::NotAdv => assert!(
                !ledger.is_advertised(&sources[i]),
                "seed {seed}: source {i} must NOT be advertised"
            ),
            Model::AdvUnder(p) => {
                assert!(
                    ledger.is_advertised(&sources[i]),
                    "seed {seed}: source {i} must be advertised under project {p}"
                );
                assert!(
                    ledger
                        .advertised_under(&ProjectUri::from(projects[*p]))
                        .contains(&sources[i]),
                    "seed {seed}: source {i} must be advertised under project {p}"
                );
                for (q, proj) in projects.iter().enumerate() {
                    if q != *p {
                        assert!(
                            !ledger
                                .advertised_under(&ProjectUri::from(*proj))
                                .contains(&sources[i]),
                            "seed {seed}: source {i} must NOT remain under other project {q} \
                             (a publish-then-prune bug leaks it under the old project)"
                        );
                    }
                }
                assert!(
                    ledger
                        .advertised_provider_paths_under(&ProjectUri::from(projects[*p]))
                        .iter()
                        .any(|path| path.as_ref() == companions[i]),
                    "seed {seed}: companion of source {i} must be in project {p}'s advertised set"
                );
            }
        }
    }
}

#[tokio::test]
async fn transition_matrix_random_sequences_preserve_model() {
    // DISCRIMINATION across the whole matrix: an A→B publish-then-prune bug leaves
    // S under A (caught by the "not under other project" assertion); a dropped
    // tombstone leaves S advertised after an absent/terminal op; a cold-as-owner-loss
    // bug retracts on bootstrap; a committing op that silently succeeds under an
    // armed commit fault would change the model (caught by the fail-closed branch).
    let sources: Vec<CanonicalSource> = (0..3)
        .map(|i| CanonicalSource::new(format!("/proj/src/Comp{i}.vue")))
        .collect();
    let projects = [
        "/proj/a/tsconfig.json",
        "/proj/b/tsconfig.json",
        "/proj/c/tsconfig.json",
    ];
    let companions: Vec<String> = (0..3)
        .map(|i| format!("/proj/src/Comp{i}.vue.tsx"))
        .collect();

    for seed in 0..200u64 {
        let mock = Arc::new(MockTypeProvider::new());
        let (reconciler, ledger) = reconciler_with(mock.clone());
        let mut model = vec![Model::NotAdv; 3];
        let mut state = lcg_seed(seed);

        for step in 0..14 {
            let s_idx = (lcg_next(&mut state) % 3) as usize;
            let op = lcg_next(&mut state) % 8;
            let p_idx = (lcg_next(&mut state) % 3) as usize;
            let inject_commit_fail = lcg_next(&mut state).is_multiple_of(5);
            let source = &sources[s_idx];

            // The ledger's post-commit verification is END-STATE based: an idempotent
            // commit (desired record already equals current) succeeds even with the
            // apply skipped. So inject the commit fault ONLY on a transition the model
            // proves changes the record — an owned op to a different/new state, or a
            // tombstone op on a currently-advertised source — where the fault is
            // GUARANTEED to leave the ledger short of desired.
            let changes_record = match (op, &model[s_idx]) {
                (0, Model::AdvUnder(q)) => *q != p_idx,
                (0, Model::NotAdv) => true,
                (1..=6, Model::AdvUnder(_)) => true,
                _ => false,
            };

            // Fail-closed: a guaranteed-changing committing op under an armed one-shot
            // commit fault MUST return Err and leave the model untouched.
            if inject_commit_fail && changes_record {
                let before = model[s_idx].clone();
                ledger.arm_commit_failure();
                let result =
                    apply_matrix_op(&reconciler, source, op, projects[p_idx], &companions[s_idx])
                        .await;
                assert!(
                    matches!(result, Err(ReconcileErr::LedgerCommit { .. })),
                    "seed {seed} step {step}: an armed commit fault on op {op} must fail closed, \
                     got {result:?}"
                );
                assert_eq!(
                    model[s_idx], before,
                    "seed {seed} step {step}: a fail-closed op must not change the model"
                );
                assert_matches_model(&ledger, &model, &sources, &projects, &companions, seed);
                continue;
            }

            let outcome =
                apply_matrix_op(&reconciler, source, op, projects[p_idx], &companions[s_idx])
                    .await
                    .unwrap_or_else(|e| {
                        panic!("seed {seed} step {step}: op {op} must succeed: {e}")
                    });

            match op {
                0 => {
                    assert!(
                        matches!(outcome, ReconcileOutcome::Advertised { .. }),
                        "seed {seed}: owned op must advertise, got {outcome:?}"
                    );
                    model[s_idx] = Model::AdvUnder(p_idx);
                }
                1..=6 => {
                    assert!(
                        matches!(outcome, ReconcileOutcome::Tombstoned { .. }),
                        "seed {seed}: absent/terminal op must tombstone, got {outcome:?}"
                    );
                    model[s_idx] = Model::NotAdv;
                }
                7 => {
                    // Bootstrap defers WITHOUT thrash: the outcome is Deferred and the
                    // model (incl. a prior current-session advertisement) is unchanged.
                    assert!(
                        matches!(outcome, ReconcileOutcome::Deferred { .. }),
                        "seed {seed}: bootstrap must defer, got {outcome:?}"
                    );
                }
                _ => unreachable!(),
            }
            assert_matches_model(&ledger, &model, &sources, &projects, &companions, seed);
        }
    }
}

#[tokio::test]
async fn transition_matrix_membership_commit_and_provider_failures_are_fail_closed() {
    // DISCRIMINATION: an impl that commits the ledger regardless of a failing
    // membership commit (owner change) or a failing provider close (remove) would
    // tear the advertisement; both branches assert Err + the prior advertisement
    // survives untouched.

    // Membership-commit failure on an owner change A→B: nothing moves.
    for seed in 0..40u64 {
        let mut state = lcg_seed(seed);
        let mock = Arc::new(MockTypeProvider::new());
        let (reconciler, ledger, committer) = reconciler_with_committer(mock.clone());
        let source =
            CanonicalSource::new(format!("/proj/src/Comp{}.vue", lcg_next(&mut state) % 5));
        let project_a = "/proj/a/tsconfig.json";
        let project_b = "/proj/b/tsconfig.json";
        let companion = format!("{}.tsx", source.as_str());

        advertise(&reconciler, &source, project_a, vec![ide(&companion)]).await;
        assert!(ledger.is_advertised(&source));

        committer.arm_failure();
        let result = reconciler
            .reconcile_source_membership(
                &source,
                bound(project_b),
                vec![ide(&companion)],
                ReconcileReason::SourceSynced,
            )
            .await;

        assert!(
            matches!(result, Err(ReconcileErr::MembershipCommit { .. })),
            "seed {seed}: a failed membership commit must fail closed, got {result:?}"
        );
        assert_eq!(
            ledger.advertised_under(&ProjectUri::from(project_a)),
            vec![source.clone()],
            "seed {seed}: the prior advertisement under A must survive a failed move to B"
        );
        assert!(
            ledger
                .advertised_under(&ProjectUri::from(project_b))
                .is_empty(),
            "seed {seed}: the failed target project B must advertise nothing"
        );
    }

    // Provider-buffer failure on a terminal remove: the tombstone is not committed.
    for seed in 0..40u64 {
        let mut state = lcg_seed(seed.wrapping_add(10_000));
        let ledger = Arc::new(MembershipLedger::with_initial_session());
        let source =
            CanonicalSource::new(format!("/proj/src/Comp{}.vue", lcg_next(&mut state) % 5));
        let companion = format!("{}.tsx", source.as_str());
        ledger
            .commit(
                &source,
                MembershipRecord::Advertised {
                    project: ProjectUri::from("/proj/tsconfig.json"),
                    companions: vec![LedgerCompanion {
                        provider_uri: Arc::from(companion.as_str()),
                        role: SnapshotRole::CarrierIde,
                        script_kind: ScriptKind::Tsx,
                    }],
                    lease: ledger.current_session(),
                },
            )
            .expect("seed advertisement");
        assert!(ledger.is_advertised(&source));

        let provider: Arc<dyn TypeProvider> = Arc::new(FailingTypeProvider::new("provider down"));
        let reconciler = MembershipReconciler::new(
            Arc::clone(&ledger),
            provider,
            RecordingMembershipCommitter::arc(),
            fresh_source_gates(),
        );
        let reason = match lcg_next(&mut state) % 3 {
            0 => AbsentReason::Deleted,
            1 => AbsentReason::CompileFailed,
            _ => AbsentReason::ConflictRemoved,
        };

        let result = reconciler.remove_source_membership(&source, reason).await;
        assert!(
            matches!(result, Err(ReconcileErr::ProviderTransition { .. })),
            "seed {seed}: a failed provider close must fail closed, got {result:?}"
        );
        assert!(
            ledger.is_advertised(&source),
            "seed {seed}: a failed provider close must NOT commit the tombstone"
        );
    }
}

// ── Per-source serialization ──────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_source_membership_transitions_serialize_per_source() {
    // Two concurrent reconciles for the SAME source must NOT both be in-flight in the
    // committer at once: the reconciler's per-source gate serializes them (the transition
    // reads the prior ledger record and swaps it across `.await`s without holding the
    // ledger lock, so an unserialized peer could interleave into a torn advertisement).
    //
    // DISCRIMINATION: a committer that blocks on entry lets us observe whether a second
    // same-source transition reaches the committer WHILE the first is in-flight. Post-fix
    // the gate provably blocks it (the second `entered` never arrives → timeout). Pre-fix
    // (no gate) the second entered concurrently → the timeout resolves → RED.
    use std::time::Duration;

    let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let committer = Arc::new(SerializingCommitter {
        entered_tx,
        release: Arc::clone(&release),
    });
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let provider = Arc::new(MockTypeProvider::new());
    let reconciler = Arc::new(MembershipReconciler::new(
        ledger,
        provider,
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        fresh_source_gates(),
    ));
    let source = "/proj/src/Same.vue";
    let spawn_reconcile = |reconciler: Arc<MembershipReconciler>| {
        tokio::spawn(async move {
            reconciler
                .reconcile_source_membership(
                    &CanonicalSource::from(source),
                    bound("/proj/tsconfig.json"),
                    vec![ide("/proj/src/Same.vue.tsx")],
                    ReconcileReason::SourceSynced,
                )
                .await
        })
    };

    // Task 1 enters the committer and parks there, holding the per-source gate.
    let t1 = spawn_reconcile(Arc::clone(&reconciler));
    entered_rx
        .recv()
        .await
        .expect("the first transition must reach the committer");

    // Task 2 (SAME source) must block on the gate — it must NOT reach the committer
    // while task 1 holds it.
    let t2 = spawn_reconcile(Arc::clone(&reconciler));
    let second_entered = tokio::time::timeout(Duration::from_millis(300), entered_rx.recv()).await;
    assert!(
        second_entered.is_err(),
        "a second concurrent transition for the SAME source reached the committer while the \
         first was still in-flight — the per-source membership gate is not serializing"
    );

    // Release both: task 1 completes and drops the gate, then task 2 proceeds.
    release.add_permits(2);
    let _ = tokio::time::timeout(Duration::from_millis(300), entered_rx.recv()).await;
    let _ = t1
        .await
        .expect("join task 1")
        .expect("task 1 reconcile succeeds");
    let _ = t2
        .await
        .expect("join task 2")
        .expect("task 2 reconcile succeeds");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn distinct_source_membership_transitions_run_concurrently() {
    // The gate is PER SOURCE, not global: two transitions for DIFFERENT sources must be
    // able to be in-flight in the committer at the same time. This guards against a
    // regression that over-serializes (e.g. a single global lock) and is the negative
    // companion to `same_source_membership_transitions_serialize_per_source`.
    use std::time::Duration;

    let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let committer = Arc::new(SerializingCommitter {
        entered_tx,
        release: Arc::clone(&release),
    });
    let ledger = Arc::new(MembershipLedger::with_initial_session());
    let provider = Arc::new(MockTypeProvider::new());
    let reconciler = Arc::new(MembershipReconciler::new(
        ledger,
        provider,
        Arc::clone(&committer) as Arc<dyn CarrierMembershipCommitter>,
        fresh_source_gates(),
    ));
    let spawn_reconcile = |reconciler: Arc<MembershipReconciler>, source: &'static str| {
        tokio::spawn(async move {
            reconciler
                .reconcile_source_membership(
                    &CanonicalSource::from(source),
                    bound("/proj/tsconfig.json"),
                    vec![ide(&format!("{source}.tsx"))],
                    ReconcileReason::SourceSynced,
                )
                .await
        })
    };

    let t1 = spawn_reconcile(Arc::clone(&reconciler), "/proj/src/A.vue");
    let t2 = spawn_reconcile(Arc::clone(&reconciler), "/proj/src/B.vue");

    // BOTH distinct-source transitions must reach the committer concurrently: two
    // `entered` events arrive before either is released.
    let first = tokio::time::timeout(Duration::from_millis(500), entered_rx.recv()).await;
    let second = tokio::time::timeout(Duration::from_millis(500), entered_rx.recv()).await;
    assert!(
        first.is_ok() && second.is_ok(),
        "two transitions for DIFFERENT sources must run concurrently in the committer, but \
         the second did not enter while the first was in-flight (the gate over-serializes)"
    );

    release.add_permits(2);
    let _ = t1
        .await
        .expect("join task 1")
        .expect("task 1 reconcile succeeds");
    let _ = t2
        .await
        .expect("join task 2")
        .expect("task 2 reconcile succeeds");
}
