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
    carrier_store_dir_for, default_carrier_store_host_version, CanonicalSource,
    CarrierPublishStore, Manifest, ProjectUri, ReconcileErr, ReconcileOutcome, ReconcileReason,
};
use crate::type_provider::mock::{MockCall, MockTypeProvider};

/// A test IDE companion for `provider`.
fn ide_companion(provider: &str) -> CarrierCompanion {
    CarrierCompanion::carrier_ide_from_generated(
        Arc::from(provider),
        "/workspace/src/App.vue",
        "export default {} as any;\n",
        None,
        verter_session::external_ts::ScriptKind::Tsx,
        1,
    )
}

/// Build a coordinator over a fresh backend + a mock provider, returning both so
/// the test can inspect the provider's recorded calls.
fn coordinator() -> (CarrierPublishCoordinator, MockTypeProvider) {
    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord = CarrierPublishCoordinator::new(backend, Arc::new(mock.clone()), "5.9.0");
    (coord, mock)
}

#[tokio::test]
async fn workspace_publication_refresh_is_one_provider_batch() {
    let (coord, mock) = coordinator();
    let paths = vec![
        "/proj/src/A.vue.verter.ts".to_string(),
        "/proj/src/A.vue.tsx".to_string(),
        "/proj/src/B.vue.verter.ts".to_string(),
        "/proj/src/B.vue.tsx".to_string(),
    ];

    coord
        .refresh_published_companions(&paths)
        .await
        .expect("one workspace refresh succeeds");

    let calls = mock.calls();
    assert_eq!(calls.len(), 1, "one provider call for the whole batch");
    assert!(matches!(
        &calls[0],
        MockCall::NotifyCarriersChanged { companion_paths } if companion_paths == &paths
    ));
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

/// The synthetic workspace root for a store-isolating test, built from the three
/// disambiguators a concurrent test run varies over.
///
/// `pid` is the load-bearing one. This suite runs one test per PROCESS, so the
/// per-process counter in [`unique_ws_root`] reads 0 in every process and
/// disambiguates nothing across them, and `SystemTime::now()` is only
/// MICROSECOND-resolution on macOS. Without the process identity the root collides
/// whenever two test processes reach it inside the same microsecond, which aliases
/// both onto ONE on-disk carrier store and one `manifest.json`.
fn ws_root_for(pid: u32, nanos: u128, n: u64) -> String {
    format!("d:/verter_publish_retract_{pid}_{nanos}_{n}/ws")
}

/// A unique, already-canonical (lowercase drive, forward slashes) workspace root —
/// so the on-disk carrier store dir (`temp/verter-carrier-store/<host>/<hash(ws)>`)
/// is isolated per run and `CarrierPublishStore::open` keys the same dir the
/// coordinator's backend wrote. Unique across concurrent PROCESSES, not merely
/// within one — see [`ws_root_for`].
fn unique_ws_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ws_root_for(std::process::id(), nanos, n)
}

/// Read the manifest of the carrier store the coordinator's backend writes for
/// `ws_root`, STRICTLY: `Ok(None)` ONLY when the manifest genuinely does not exist,
/// and `Err` for every other failure.
///
/// The oracle must NOT read through [`CarrierPublishStore::current_manifest`]. That
/// reader deliberately reports a fresh EMPTY manifest for an unreadable or unparseable
/// one — correct for a read-only diagnostics view, but underneath a presence assertion
/// it launders "the store could not be read" into "the carrier is not advertised", so
/// a store failure surfaces as a clean carrier ABSENCE and the test blames the wrong
/// thing.
fn read_store_manifest_strict(ws_root: &str) -> Result<Option<Manifest>, String> {
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), ws_root);
    let path = store.manifest_path();
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Manifest>(&bytes)
            .map(Some)
            .map_err(|e| {
                format!(
                    "carrier manifest at {} is present but unparseable: {e}",
                    path.display()
                )
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!(
            "carrier manifest at {} is unreadable: {e} (kind={:?}, errno={:?})",
            path.display(),
            e.kind(),
            e.raw_os_error()
        )),
    }
}

/// The synthetic workspace root a store-isolating test derives must be unique across
/// concurrent PROCESSES, not merely within one process.
///
/// This suite runs one test per PROCESS, so the per-process `COUNTER` in
/// [`unique_ws_root`] reads 0 in every process and disambiguates nothing across them,
/// and `SystemTime::now()` is only MICROSECOND-resolution on macOS (repeated
/// tight-loop calls return the identical value). A root built from the clock and that
/// counter alone therefore ALIASES two test processes that reach it inside the same
/// microsecond onto ONE on-disk carrier store — and the tests in this file then
/// trample each other there: `durable_retract_failure_propagates_not_silent_success`
/// writes a deliberately corrupt manifest into it, and
/// `owner_loss_retracts_previously_published_carrier` retracts the very source that
/// `owner_change_a_to_b_retracts_from_old_project` asserts is present.
#[test]
fn synthetic_ws_root_is_unique_across_processes_not_only_within_one() {
    // Same microsecond AND same per-process counter — exactly what two concurrent
    // one-test-per-process runs observe. Only the process identity differs.
    const SAME_MICROSECOND: u128 = 1_785_068_278_682_867_000;
    let a = ws_root_for(4242, SAME_MICROSECOND, 0);
    let b = ws_root_for(4243, SAME_MICROSECOND, 0);
    assert_ne!(
        a, b,
        "two test PROCESSES deriving a root in the same microsecond must not get the \
         SAME workspace root (the per-process counter reads 0 in both)"
    );

    // The consequence that actually bites: the derived on-disk store dirs must differ,
    // or both processes read-modify-write ONE manifest.json.
    let host = default_carrier_store_host_version();
    assert_ne!(
        carrier_store_dir_for(host, &a),
        carrier_store_dir_for(host, &b),
        "distinct test processes must resolve DISTINCT carrier-store dirs; an aliased \
         dir means two processes share one manifest"
    );

    // The within-process disambiguator must still work.
    assert_ne!(
        ws_root_for(4242, SAME_MICROSECOND, 0),
        ws_root_for(4242, SAME_MICROSECOND, 1),
        "two roots taken inside one microsecond by ONE process must still differ"
    );

    // And the live derivation must actually vary the process identity in — otherwise
    // its uniqueness rests on a microsecond clock alone.
    let live = unique_ws_root();
    assert!(
        live.starts_with(&format!(
            "d:/verter_publish_retract_{}_",
            std::process::id()
        )),
        "unique_ws_root must carry this process's identity; got {live}"
    );
}

/// The store oracle must surface an unreadable / unparseable manifest as a FAILURE,
/// never launder it into "the carrier is absent".
///
/// [`CarrierPublishStore::current_manifest`] deliberately reports a fresh EMPTY
/// manifest for a corrupt one — correct for a read-only diagnostics view (it never
/// writes, so there is nothing to clobber), but WRONG underneath a test oracle:
/// reading a presence assertion through it turns "the store could not be read" into
/// "the carrier is not advertised". That laundering is why an aliased/corrupted store
/// surfaced as the misleading "after the A publish the carrier must be in A's
/// owned+ready set" instead of naming the real cause.
#[test]
fn store_oracle_reports_a_corrupt_manifest_as_a_failure_not_as_absence() {
    let corrupt_root = unique_ws_root();
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), &corrupt_root);
    std::fs::create_dir_all(store.workspace_dir()).expect("create the store dir");
    std::fs::write(store.manifest_path(), b"{ this manifest is truncated")
        .expect("write a corrupt manifest");

    // Pin the fail-open behaviour of the diagnostics reader the oracle must NOT inherit.
    assert!(
        store.current_manifest().projects.is_empty(),
        "the diagnostics reader is fail-open by design; this pins what the oracle must \
         not inherit"
    );

    let detail = read_store_manifest_strict(&corrupt_root).expect_err(
        "a present-but-corrupt manifest must be an ERROR from the oracle's reader, never \
         an empty manifest that reads as carrier ABSENCE",
    );
    assert!(
        detail.contains("unparseable"),
        "the error must name the actual cause; got {detail:?}"
    );

    // A genuinely absent manifest stays distinguishable from a corrupt one.
    let absent_root = unique_ws_root();
    assert!(
        matches!(read_store_manifest_strict(&absent_root), Ok(None)),
        "a store that was never published must read as Ok(None), not as an error"
    );
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
    let manifest = match read_store_manifest_strict(ws_root) {
        Ok(Some(manifest)) => manifest,
        // No manifest at all ⇒ genuinely nothing published for this workspace.
        Ok(None) => return (false, false),
        // A store FAILURE is not a carrier absence — surface the real cause instead of
        // letting a presence assertion report a misleading "not owned/ready".
        Err(detail) => panic!(
            "the carrier-store oracle must surface a store failure rather than report \
             the carrier absent: {detail}"
        ),
    };
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

    let companion = CarrierCompanion::carrier_ide_from_generated(
        Arc::from(provider.as_str()),
        "/workspace/src/App.vue",
        "export default {} as any;\n",
        None,
        verter_session::external_ts::ScriptKind::Tsx,
        1,
    );

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

    let companion = CarrierCompanion::carrier_ide_from_generated(
        Arc::from(provider.as_str()),
        "/workspace/src/App.vue",
        "export default {} as any;\n",
        None,
        verter_session::external_ts::ScriptKind::Tsx,
        1,
    );

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

    let companion = CarrierCompanion::carrier_ide_from_generated(
        Arc::from(provider.as_str()),
        "/workspace/src/App.vue",
        "export default {} as any;\n",
        None,
        verter_session::external_ts::ScriptKind::Tsx,
        1,
    );

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
