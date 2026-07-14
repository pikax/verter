//! Tests for the live carrier-publish coordinator.
//!
//! These exercise the fail-closed resolution gate and the publish→store→eviction
//! chain with a mock provider (no real tsserver). The real-provider end-to-end
//! membership is covered by `real_provider_tests::external_ts_baseline`.

use std::sync::Arc;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

use super::*;
use crate::external_ts::tsserver_backend::TsserverEngineBackend;
use crate::external_ts::{
    default_carrier_store_host_version, CanonicalSource, CarrierPublishStore, ProjectUri,
    ReconcileErr, ReconcileOutcome, ReconcileReason,
};
use crate::type_provider::mock::{MockCall, MockTypeProvider};

/// A test IDE companion for `provider`.
fn ide_companion(provider: &str) -> CarrierCompanion {
    CarrierCompanion {
        provider_uri: Arc::from(provider),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    }
}

/// Build a coordinator over a fresh backend + a mock provider, returning both so
/// the test can inspect the provider's recorded calls.
fn coordinator() -> (CarrierPublishCoordinator, MockTypeProvider) {
    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord = CarrierPublishCoordinator::new(backend, Arc::new(mock.clone()), "5.9.0");
    (coord, mock)
}

/// A host + filesystem workspace with NO published snapshot — the no-owner case.
fn host_without_snapshot() -> (Arc<VerterHost>, verter_workspace::FilesystemWorkspace) {
    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs));
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    (host, fs)
}

/// An empty companion set under a COLD (not-yet-authoritative) snapshot is a
/// bootstrap: the reconciler DEFERS (not advertised, not a clean publish) and never
/// touches the provider or the store. The reconciler is the single transition entry.
#[tokio::test]
async fn cold_bootstrap_defers_empty_companions() {
    let (coord, mock) = coordinator();
    let (host, fs) = host_without_snapshot();
    let outcome = coord
        .reconcile_membership(
            &host,
            &fs,
            "/proj/src/Comp.vue",
            vec![],
            // ownership_ready = false ⇒ cold bootstrap (defer without thrash).
            false,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("a cold reconcile defers (it is not an error)");
    assert!(
        matches!(outcome, ReconcileOutcome::Deferred { .. }),
        "a cold snapshot ⇒ bootstrap defer, got {outcome:?}"
    );
    assert!(
        mock.file_sync_calls().is_empty(),
        "a cold defer must not touch the provider"
    );
}

/// A COLD snapshot defers without thrash (no provider eviction, no store mutation)
/// rather than retracting a possibly-still-valid carrier — the cold-start-vs-owner-
/// loss discriminant.
#[tokio::test]
async fn cold_bootstrap_defers_without_thrash() {
    let (coord, mock) = coordinator();
    let (host, fs) = host_without_snapshot();
    let companion = ide_companion("/proj/src/Comp.vue.tsx");
    let outcome = coord
        .reconcile_membership(
            &host,
            &fs,
            "/proj/src/Comp.vue",
            vec![companion],
            false,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("a cold reconcile defers");
    assert!(
        matches!(outcome, ReconcileOutcome::Deferred { .. }),
        "a cold snapshot ⇒ defer, got {outcome:?}"
    );
    assert!(
        !mock
            .file_sync_calls()
            .iter()
            .any(|c| matches!(c, MockCall::NotifyCarrierChanged { .. })),
        "a cold defer must not fire a carrier-changed eviction"
    );
}

/// A unique, already-canonical (lowercase drive, forward slashes) workspace root —
/// so the on-disk carrier store dir (`temp/verter-carrier-store/<host>/<hash(ws)>`)
/// is isolated per run and `CarrierPublishStore::open` keys the same dir the
/// coordinator's backend wrote.
fn unique_ws_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("d:/verter_publish_retract_{nanos}_{n}/ws")
}

/// Build a real `WorkspaceSnapshot` with ONE configured project owning `src/**/*`
/// (so a `.vue` source under `src/` resolves to a `ProjectBinding`), driving the
/// SAME production membership parse/expansion chain as the resolver's own tests
/// (`load_project_membership` + `membership_to_spec`), hermetically over an
/// in-memory workspace.
fn project_binding_snapshot(ws_root: &str, tsconfig: &str) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        tsconfig.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    ws.inject_file(
        format!("{ws_root}/src/Comp.vue"),
        Arc::<str>::from("<template></template>"),
    );

    let root = CanonicalPath::new(ws_root);
    let raw_membership = load_project_membership(&ws, tsconfig);
    let compiler_options = load_compiler_options(&ws, tsconfig);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, tsconfig)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(ws_root),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// Read the published carrier store the coordinator's `with_default_host_version`
/// backend writes for `ws_root`, returning whether `source`/`provider` are still
/// in the project's owned/ready set.
fn carrier_is_in_store(
    ws_root: &str,
    tsconfig: &str,
    source: &str,
    provider: &str,
) -> (bool, bool) {
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), ws_root);
    let manifest = store.current_manifest();
    let Some(project) = manifest.projects.get(tsconfig) else {
        return (false, false);
    };
    let owned = project.owned_sources.iter().any(|o| o.source_uri == source);
    let ready = project.ready_files.contains_key(provider);
    (owned, ready)
}

/// A carrier published under a resolved configured project that LATER loses its
/// owner (re-resolves to `NoProject`) must be RETRACTED from the store, not left
/// stale. The bug: `publish_carrier`'s no-owner arms returned `Ok(false)` WITHOUT
/// retracting, so the prior carrier persisted in `getExternalFiles` (a fail-closed
/// resolution must mean RETRACTED, not stale-retained). The live per-edit publish
/// uses `OwnedSetScope::SourceDelta`, whose store reconciliation never prunes, so
/// only an explicit retract on owner-loss removes the stale ready file.
#[tokio::test]
async fn owner_loss_retracts_previously_published_carrier() {
    let (coord, _mock) = coordinator();
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs));
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());

    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    };

    // 1. Publish under a resolved configured owner (ProjectBinding) through the
    //    reconciler — the production transition entry (durable store + ledger).
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));
    let outcome = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion.clone()],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("publish under a configured owner succeeds");
    assert!(
        matches!(outcome, ReconcileOutcome::Advertised { .. }),
        "the source resolves to a ProjectBinding ⇒ advertised, got {outcome:?}"
    );

    let (owned_before, ready_before) = carrier_is_in_store(&ws_root, &tsconfig, &source, &provider);
    assert!(
        owned_before && ready_before,
        "after a ProjectBinding publish the carrier must be in the store's owned+ready set \
         (owned={owned_before}, ready={ready_before})"
    );
    assert!(
        coord
            .backend()
            .membership_ledger()
            .is_advertised(&CanonicalSource::from(source.as_str())),
        "after a publish the source must be advertised in the ledger"
    );

    // 2. The owner disappears: the same source now resolves to NoProject (empty
    //    ownership). Re-reconciling must RETRACT the prior carrier (tombstone).
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        build_workspace_snapshot_simple(Vec::new(), SnapshotGeneration(2)),
    )));
    let outcome = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion.clone()],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("a no-owner resolution reconciles to a tombstone");
    assert!(
        matches!(outcome, ReconcileOutcome::Tombstoned { .. }),
        "NoProject resolution ⇒ tombstoned (retracted), got {outcome:?}"
    );

    // 3. The carrier must be GONE from the store AND the ledger — owner loss retracts.
    let (owned_after, ready_after) = carrier_is_in_store(&ws_root, &tsconfig, &source, &provider);
    assert!(
        !owned_after && !ready_after,
        "owner loss must RETRACT the carrier from the store's owned+ready set (so \
         getExternalFiles stops serving it); still present: owned={owned_after}, \
         ready={ready_after}"
    );
    assert!(
        !coord
            .backend()
            .membership_ledger()
            .is_advertised(&CanonicalSource::from(source.as_str())),
        "owner loss must leave the source NOT advertised in the ledger-backed getExternalFiles"
    );
}

/// Owner A→B: a source that MOVES to a new owning project must not stay
/// advertised in its OLD project. The per-edit publish uses
/// `OwnedSetScope::SourceDelta` (union into the target, never prune), so without
/// the cross-project prune the carrier persists in project A's `ready_files` (and
/// thus A's `getExternalFiles`) while ALSO appearing in B. RED before the fix: the
/// carrier remains owned+ready in A after the B publish.
#[tokio::test]
async fn owner_change_a_to_b_retracts_from_old_project() {
    let (coord, _mock) = coordinator();
    let ws_root = unique_ws_root();
    // Two DISTINCT configured projects at the ws_root dir (so each `include:
    // ["src/**/*"]`, which is relative to the tsconfig's own dir, owns the SAME
    // `{ws_root}/src/Comp.vue`). The store keys projects by tsconfig URI, so A and B
    // are separate manifest entries owning the one source — the A→B move shape.
    let tsconfig_a = format!("{ws_root}/tsconfig.json");
    let tsconfig_b = format!("{ws_root}/tsconfig.app.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs));
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());

    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    };

    // 1. Publish under owner A through the reconciler.
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig_a),
    )));
    assert!(
        matches!(
            coord
                .reconcile_membership(
                    &host,
                    &fs,
                    &source,
                    vec![companion.clone()],
                    true,
                    ReconcileReason::SourceSynced,
                )
                .await
                .expect("publish under owner A succeeds"),
            ReconcileOutcome::Advertised { .. }
        ),
        "the source resolves to project A ⇒ advertised"
    );
    let (owned_a1, ready_a1) = carrier_is_in_store(&ws_root, &tsconfig_a, &source, &provider);
    assert!(
        owned_a1 && ready_a1,
        "after the A publish the carrier must be in A's owned+ready set"
    );

    // 2. The owner CHANGES to B (same source, new configured project). Re-reconcile.
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig_b),
    )));
    let outcome = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion.clone()],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("publish under owner B succeeds");
    assert!(
        matches!(
            outcome,
            ReconcileOutcome::Advertised {
                replaced: Some(_),
                ..
            }
        ),
        "the A→B move advertises under B and records the replaced project A, got {outcome:?}"
    );

    // 3. The carrier must be PRESENT in B and GONE from A (no stale cross-project
    //    membership) — in BOTH the on-disk store and the ledger-backed getExternalFiles.
    let (owned_b, ready_b) = carrier_is_in_store(&ws_root, &tsconfig_b, &source, &provider);
    assert!(
        owned_b && ready_b,
        "after the A→B move the carrier must be a member of the NEW project B \
         (owned={owned_b}, ready={ready_b})"
    );
    let (owned_a2, ready_a2) = carrier_is_in_store(&ws_root, &tsconfig_a, &source, &provider);
    assert!(
        !owned_a2 && !ready_a2,
        "the A→B move MUST retract the carrier from the OLD project A's owned+ready \
         set (so A's getExternalFiles stops serving it); still present in A: \
         owned={owned_a2}, ready={ready_a2}"
    );
    let ledger = coord.backend().membership_ledger();
    assert_eq!(
        ledger.advertised_under(&ProjectUri::from(tsconfig_b.as_str())),
        vec![CanonicalSource::from(source.as_str())],
        "the ledger must advertise the source under the new project B"
    );
    assert!(
        ledger
            .advertised_under(&ProjectUri::from(tsconfig_a.as_str()))
            .is_empty(),
        "the ledger must leave NOTHING advertised under the old project A"
    );
}

/// Owner A→B with a FAILING stale-owner prune must leave the store with NEITHER a
/// stale old-owner (A) row NOR a duplicate (the source under both A and B). The
/// owner-move publishes into B (a `SourceDelta` union) and then prunes the source
/// from every OTHER project; those are SEPARATE store writes. If the prune fails
/// after the publish committed, the source would be advertised under BOTH A and B —
/// a duplicated/stale `ready_files` set the plugin serves cross-process. The
/// compensation rolls the publish back (retract everywhere), leaving the source fully
/// unadvertised and surfacing the failure fail-closed. RED before the fix: the prune
/// `?`-propagated WITHOUT rolling back, so the source stayed in BOTH A (stale) and B
/// (the duplicate).
#[tokio::test]
async fn owner_move_with_failing_prune_leaves_no_stale_or_duplicate_ready_file() {
    let (coord, _mock) = coordinator();
    let ws_root = unique_ws_root();
    // Two configured projects at the ws_root dir, each owning the SAME source — the
    // A→B owner-move shape (the store keys projects by tsconfig URI).
    let tsconfig_a = format!("{ws_root}/tsconfig.json");
    let tsconfig_b = format!("{ws_root}/tsconfig.app.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs));
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());

    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    };

    // 1. Publish under owner A.
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig_a),
    )));
    assert!(matches!(
        coord
            .reconcile_membership(
                &host,
                &fs,
                &source,
                vec![companion.clone()],
                true,
                ReconcileReason::SourceSynced,
            )
            .await
            .expect("publish under owner A succeeds"),
        ReconcileOutcome::Advertised { .. }
    ));
    let (_, ready_a1) = carrier_is_in_store(&ws_root, &tsconfig_a, &source, &provider);
    assert!(
        ready_a1,
        "after the A publish the carrier must be ready in A"
    );

    // 2. The owner CHANGES to B, but ARM the stale-owner prune to FAIL after the
    //    publish into B commits.
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig_b),
    )));
    coord.backend().arm_prune_except_failure();
    let result = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion.clone()],
            true,
            ReconcileReason::SourceSynced,
        )
        .await;
    assert!(
        matches!(result, Err(ReconcileErr::MembershipCommit { .. })),
        "a failing owner-move prune must surface fail-closed as Err(MembershipCommit) (the \
         publish was rolled back), got {result:?}"
    );

    // 3. The store must carry NEITHER a stale A row NOR a B duplicate — the rollback
    //    left the source fully unadvertised.
    let (_, ready_a2) = carrier_is_in_store(&ws_root, &tsconfig_a, &source, &provider);
    let (_, ready_b) = carrier_is_in_store(&ws_root, &tsconfig_b, &source, &provider);
    assert!(
        !ready_a2 && !ready_b,
        "a partial owner-move (prune failed) must leave NEITHER a stale old-owner row in \
         A (ready_a={ready_a2}) NOR a duplicate in B (ready_b={ready_b}); the carrier \
         must not be advertised under multiple projects"
    );
}

/// A durable-store retract FAILURE on owner loss must not be swallowed into a silent
/// success. A corrupt on-disk manifest makes the strict `read_manifest` the retract
/// commit performs fail, so the owner-loss reconcile's durable retract fails — and
/// the reconciler must surface that as `Err(MembershipCommit)`, never report a
/// fail-closed "not advertised" success while the carrier may still be advertised.
/// The ledger tombstone is NOT committed when the durable store cannot be retracted.
#[tokio::test]
async fn durable_retract_failure_propagates_not_silent_success() {
    let (coord, _mock) = coordinator();
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), vfs));
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());

    let companion = ide_companion(&provider);

    // 1. Publish under a resolved owner (creates a valid manifest + registers the
    //    backend's per-workspace store).
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));
    assert!(matches!(
        coord
            .reconcile_membership(
                &host,
                &fs,
                &source,
                vec![companion.clone()],
                true,
                ReconcileReason::SourceSynced,
            )
            .await
            .expect("initial publish succeeds"),
        ReconcileOutcome::Advertised { .. }
    ));

    // 2. CORRUPT the on-disk manifest so the strict `read_manifest` the retract
    //    commit performs fails (the fail-closed "present-but-corrupt manifest
    //    propagates" path).
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), &ws_root);
    std::fs::write(store.manifest_path(), b"{ this is not valid json :: ")
        .expect("corrupt the manifest on disk");

    // 3. Owner-loss reconcile (NoProject) → the reconciler's durable retract fails
    //    on the corrupt manifest → MUST propagate as `Err(MembershipCommit)`.
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        build_workspace_snapshot_simple(Vec::new(), SnapshotGeneration(2)),
    )));
    let result = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion.clone()],
            true,
            ReconcileReason::SourceSynced,
        )
        .await;
    assert!(
        matches!(result, Err(ReconcileErr::MembershipCommit { .. })),
        "a durable retract failure on an owner-loss reconcile MUST surface as \
         Err(MembershipCommit), never a silent success that reports not-advertised while \
         the carrier may still be advertised; got {result:?}"
    );
}
