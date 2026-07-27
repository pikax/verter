//! Unit tests for [`crate::sync_coordinator`] coordinator behavior.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` in `sync_coordinator.rs` to
//! keep the production source small and readable.
//! Wired back as a `#[cfg(test)] #[path = "sync_coordinator_tests.rs"] mod tests;`
//! child of `sync_coordinator`, so `use super::*` resolves to its items.

use super::*;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::ProjectSyncMode;
use futures_util::{FutureExt, StreamExt};
use tower_lsp_server::{LspService, Server};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

#[derive(Default)]
struct NoopLanguageServer;

impl tower_lsp_server::LanguageServer for NoopLanguageServer {
    async fn initialize(
        &self,
        _: InitializeParams,
    ) -> tower_lsp_server::jsonrpc::Result<InitializeResult> {
        Ok(InitializeResult::default())
    }

    async fn shutdown(&self) -> tower_lsp_server::jsonrpc::Result<()> {
        Ok(())
    }
}

fn make_test_client() -> Client {
    let client_slot = Arc::new(std::sync::Mutex::new(None));
    let client_slot_for_service = Arc::clone(&client_slot);
    let (service, socket) = LspService::new(move |client| {
        *client_slot_for_service.lock().expect("client lock") = Some(client.clone());
        NoopLanguageServer
    });
    tokio::spawn(async move {
        let _ = Server::new(tokio::io::empty(), tokio::io::sink(), socket)
            .serve(service)
            .await;
    });
    let client = client_slot
        .lock()
        .expect("client lock")
        .clone()
        .expect("test client should be captured");
    client
}

#[tokio::test]
async fn sync_coordinator_coalesces_rapid_changes() {
    let (handle, mut wake_rx) = SyncCoordinatorHandle::new_for_test();

    // Ten edits occupy one wake slot and one latest-value document entry.
    for version in 0..10 {
        handle.signal(
            "C:/project/src/App.vue".to_string(),
            format!("file:///C:/project/src/App.vue?v={version}"),
        );
    }

    assert_eq!(wake_rx.try_recv(), Ok(()), "one wake must be queued");
    assert!(
        wake_rx.try_recv().is_err(),
        "the capacity-one wake channel must not queue per-keystroke work"
    );
    let pending = handle.take_pending_for_test();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending.get("C:/project/src/App.vue").map(String::as_str),
        Some("file:///C:/project/src/App.vue?v=9")
    );
}

#[tokio::test]
async fn coordinator_debounces_to_single_sync() {
    // Test the actual debounce logic using an in-process coordinator.
    // We use a mock deps that tracks sync calls via a shared counter.

    let debounce = Duration::from_millis(DEBOUNCE_MS);

    // Simulate: 10 signals at 10ms intervals for the same file
    let mut last_change = Instant::now();
    for _ in 0..10 {
        last_change = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // At this point, the last change was just now.
    // The debounce should NOT have fired yet.
    let elapsed = Instant::now().duration_since(last_change);
    assert!(
        elapsed < debounce,
        "debounce should not fire during rapid changes (elapsed: {:?})",
        elapsed
    );

    // Wait for the debounce interval to pass
    tokio::time::sleep(debounce).await;
    let elapsed = Instant::now().duration_since(last_change);
    assert!(
        elapsed >= debounce,
        "debounce should fire after silence (elapsed: {:?})",
        elapsed
    );
}

#[tokio::test]
async fn sync_coordinator_closes_stale_owner_ids_when_owner_changes() {
    let mock = MockTypeProvider::new();
    let sync = ProjectSync::new(Arc::new(mock.clone()), ProjectSyncMode::FullProject);
    let states = DashMap::new();
    states.insert(
        "/workspace/src/App.vue".to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/workspace/tsconfig.old.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        },
    );

    let transition = crate::provider_sync::prepare_sync_transition(
        &states,
        "/workspace/src/App.vue",
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
                "/workspace/tsconfig.new.json".to_string(),
            ),
            ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
            api_path: Some("/workspace/src/App.vue.ts".to_string()),
            ..Default::default()
        },
    );

    let provider_surfaces = crate::provider_surface_store::ProviderSurfaceStore::new();
    close_stale_paths(
        &sync,
        &provider_surfaces,
        &non_decl_close_targets(&transition.stale_paths),
    )
    .await;
    commit_sync_transition(&states, "/workspace/src/App.vue", transition.next);

    let calls = mock.file_sync_calls();
    assert_eq!(
        calls.len(),
        2,
        "owner change should close both stale provider ids (for rebind)"
    );
    assert!(
        matches!(
            &calls[0],
            MockCall::CloseFile { path }
                if path == "/workspace/src/App.vue.tsx"
        ),
        "first stale close should target the IDE id: {:?}",
        calls[0]
    );
    assert!(
        matches!(
            &calls[1],
            MockCall::CloseFile { path }
                if path == "/workspace/src/App.vue.ts"
        ),
        "second stale close should target the API id: {:?}",
        calls[1]
    );
    assert_eq!(
        states
            .get("/workspace/src/App.vue")
            .expect("new owner state should be committed")
            .owner_binding,
        crate::provider_sync::ProviderOwnerBinding::Owned(
            "/workspace/tsconfig.new.json".to_string()
        ),
        "committed state should have the new owner binding"
    );
}

/// H2 (stale false-vouch): the coordinator's `close_stale_paths` MUST `forget`
/// a closed `Api` surface in the provider-surface store. Otherwise the store
/// keeps vouching the closed `{carrier}.ts` generation as CURRENT, and a later
/// cross-file rename's `current_snapshot()` maps a returned offset through a
/// STALE surface — the fail-closed invariant the store exists to guarantee.
///
/// Discriminating: it records an `Api` `CarrierApi` snapshot (so the path is
/// tracked), drives `close_stale_paths` with that path as a stale `Api` path,
/// and asserts the store NO LONGER tracks it. Against the pre-fix
/// `close_stale_paths` (which only `close_dts`'d and never `forget`'d) the path
/// stays tracked and the assertion FAILS; after the fix it is forgotten.
#[tokio::test]
async fn close_stale_paths_forgets_closed_api_surface_in_store() {
    use crate::provider_surface_store::{ProviderSurfaceStore, RecordSurface};

    let mock = MockTypeProvider::new();
    let sync = ProjectSync::new(Arc::new(mock.clone()), ProjectSyncMode::FullProject);
    let provider_surfaces = ProviderSurfaceStore::new();

    // Record a CarrierApi snapshot under the API path → the store tracks it.
    let api_path = "/workspace/src/Child.vue.ts";
    provider_surfaces.record(RecordSurface::carrier_api_legacy(
        api_path.to_string(),
        "/workspace/src/Child.vue".to_string(),
        Arc::from("declare const Child: { new(props?: { foo: string }): {} }"),
        None,
        Arc::from("<script setup lang=\"ts\">\ndefineProps<{ foo: string }>();\n</script>\n"),
    ));
    assert!(
        provider_surfaces.is_tracked(api_path),
        "precondition: the recorded API surface is tracked before close"
    );

    // Drive the coordinator close path with the API path stale.
    let stale_paths = vec![(NonDeclProviderPathKind::Api, api_path.to_string())];
    close_stale_paths(&sync, &provider_surfaces, &stale_paths).await;

    // The provider close still happened (behavior preserved)...
    let calls = mock.file_sync_calls();
    assert_eq!(calls.len(), 1, "the API path should be closed once");
    assert!(
        matches!(&calls[0], MockCall::CloseFile { path } if path == api_path),
        "the stale close should target the API path: {:?}",
        calls[0]
    );

    // ...AND the store must have forgotten it — no current generation vouches
    // the now-closed surface to a later cross-file rename.
    assert!(
        !provider_surfaces.is_tracked(api_path),
        "close_stale_paths must forget a closed Api surface so it is no longer vouched as \
         current; a still-tracked surface would false-vouch a stale generation to a rename"
    );
}

#[tokio::test]
async fn sync_file_queues_pending_snapshot_sync_when_resolver_snapshot_is_missing() {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let provider = Arc::new(MockTypeProvider::new());
    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(provider, ProjectSyncMode::FullProject)),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(
        &deps,
        "/workspace/src/App.vue",
        "file:///workspace/src/App.vue",
    )
    .await;

    assert!(
        deps.pending_snapshot_provider_sync
            .contains("/workspace/src/App.vue"),
        "sync coordinator should preserve pending IDE/API sync until resolver discovery completes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn preserve_open_unresolved_carrier_no_ide_no_prior_commits_empty_unresolved() {
    // R6-3 (row 1, sync_coordinator caller): with NO prior committed state
    // AND no IDE output (`ide = None`), the coordinator's
    // `preserve_open_unresolved_carrier` commits an EMPTY `Unresolved` state
    // (ide_path=None, binding=Unresolved) — recording the open file's
    // unresolved status. This pins the SAME row-1 behavior the drain and the
    // server `preserve_open_unresolved_carrier` commit (all three unified).
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let provider = Arc::new(MockTypeProvider::new());
    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    // No prior state in the (empty) states map; no IDE output this pass.
    let project_sync = deps.project_sync.clone().expect("test deps carry a sync");
    preserve_open_unresolved_carrier(&deps, &project_sync, "/workspace/src/App.vue", false, None)
        .await;

    let state = deps
        .provider_sync_states
        .get("/workspace/src/App.vue")
        .map(|entry| entry.clone())
        .expect("row 1 must commit an empty Unresolved state");
    assert!(
        state.is_unresolved(),
        "row 1 commits a forced-Unresolved binding, got {:?}",
        state.owner_binding
    );
    assert!(
        state.ide_path.is_none(),
        "row 1 has no live IDE path to advertise, got {:?}",
        state.ide_path
    );
    assert!(state.api_path.is_none(), "row 1 has no API path");
    assert!(!state.ide_background_loaded);

    // No prior + no IDE → nothing to open, update, or close in the provider.
    assert!(
        provider.file_sync_calls().is_empty(),
        "row 1 must not touch any provider file path, calls={:?}",
        provider.file_sync_calls()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn publish_merged_diagnostics_skips_type_provider_without_committed_state() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">const msg = 'hello'</script><template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    publish_merged_diagnostics(&deps, "/workspace/src/App.vue", uri.as_str()).await;

    let calls = provider.calls();
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, MockCall::GetDiagnostics { .. })),
        "diagnostics publishing must not query the type provider without a committed path, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn merged_diagnostics_surface_verter_project_warning_on_unowned_carrier() {
    // The debounced coordinator path (`did_open` / `did_change` route here, NOT
    // through the request-only `compute_full_diagnostics`) must surface the
    // `verter(project)` ownership diagnostic for a genuinely-unowned carrier. This
    // is the wiring fix: pre-fix the diagnostic lived ONLY in
    // `compute_full_diagnostics`, so an orphaned carrier was silently typeless on
    // open AND edit.
    //
    // DISCRIMINATING: without the `project_ownership_diagnostics_for` wiring in
    // `compute_merged_diagnostics`, the returned set carries NO `verter(project)`
    // diagnostic for the unowned carrier and this assertion fails.
    // A ready (authoritative) published root whose only configured project lives
    // at `/other`; the `/workspace` carrier is under no configured project ⇒
    // terminal `NoProject`. `with_ext` publishes `ownership_ready = true`, so the
    // diagnostic path's `ObservePublishedReadiness` resolves authoritatively.
    let vfs = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    );
    let ws = vfs.read().clone().expect("published workspace");
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div/></template>".to_string(),
    });

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    let diagnostics = compute_merged_diagnostics(&deps, "/workspace/src/App.vue", &uri).await;
    assert!(
        diagnostics
            .iter()
            .any(|d| d.source.as_deref() == Some("verter(project)")),
        "an unowned carrier must surface a verter(project) ownership diagnostic on the \
         debounced (did_open/did_change) publish path, got {diagnostics:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn merged_diagnostics_stay_silent_for_resolved_multi_claimant_carrier() {
    // The debounced coordinator path must emit ZERO `verter(project)` diagnostics for a
    // RESOLVED multi-claimant carrier: a carrier claimed by multiple sibling tsconfigs
    // resolves to the single tsgo default owner (`Bound`), and a `Bound` carrier is not
    // the user's problem. Silence is by construction
    // (`project_ownership_diagnostic(Bound) -> None`) but was untested on this path.
    //
    // DISCRIMINATING: a regression that re-terminals a multi-claimant carrier as
    // `Ambiguous(MultipleOwners)` while still serving `Bound` would surface a
    // `verter(project)` warning here and fail this assertion.
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        ),
    ]);
    let vfs = crate::test_utils::make_test_vfs_workspace_with_resolver_and_projects(
        resolver,
        &[
            ("/workspace", "/workspace", Some("/workspace/tsconfig.json")),
            (
                "/workspace",
                "/workspace",
                Some("/workspace/tsconfig.app.json"),
            ),
        ],
    );
    let ws = vfs.read().clone().expect("published workspace");
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div/></template>".to_string(),
    });

    // Meaningfulness gate: the carrier is GENUINELY multi-claimant (the raw resolution
    // is `Ambiguous`) yet resolves to a single default owner — so the silence proves the
    // Bound-serving path, not an accidentally-unique carrier.
    {
        let published = host
            .workspace_read()
            .published_root()
            .expect("published root");
        assert!(
            matches!(
                published
                    .snapshot
                    .configured_owner_resolution_for_file("/workspace/src/App.vue"),
                verter_workspace::workspace_snapshot::ConfiguredOwnerResolution::Ambiguous(_)
            ),
            "the carrier must be genuinely multi-claimant for this test to be meaningful"
        );
        assert!(
            published
                .snapshot
                .default_configured_owner_for_file("/workspace/src/App.vue")
                .is_some(),
            "the multi-claimant carrier must resolve to a single default owner"
        );
    }

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    let diagnostics = compute_merged_diagnostics(&deps, "/workspace/src/App.vue", &uri).await;
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.source.as_deref() == Some("verter(project)")),
        "a RESOLVED multi-claimant carrier must emit NO verter(project) diagnostic on the \
         debounced (did_open/did_change) path, got {diagnostics:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn merged_diagnostics_surface_verter_project_warning_on_carrier_path_conflict() {
    // The debounced coordinator path must ALSO surface the `verter(project)` ownership
    // diagnostic for a terminal DISK-LAYOUT carrier-path conflict (a real user file
    // occupying the generated companion path). Pre-fix the coordinator path only tested
    // `NoProject`; this pins the OTHER terminal cause on the same route.
    //
    // DISCRIMINATING: without the conflict pass downgrading to `Ambiguous`, or without
    // the diagnostic wiring, the returned set carries NO verter(project) diagnostic here.
    let vfs = crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.json"),
    );
    let ws = vfs.read().clone().expect("published workspace");
    // A REAL user file occupies the carrier's generated IDE companion path — Verter must
    // never overlay-shadow it ⇒ terminal `Ambiguous(CarrierPathOccupiedByRealFile)`.
    ws.inject_file(
        "/workspace/src/App.vue.tsx".to_string(),
        std::sync::Arc::from("export const real = 1;\n"),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), ws));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div/></template>".to_string(),
    });

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    let diagnostics = compute_merged_diagnostics(&deps, "/workspace/src/App.vue", &uri).await;
    let conflict = diagnostics
        .iter()
        .find(|d| d.source.as_deref() == Some("verter(project)"))
        .unwrap_or_else(|| {
            panic!(
                "a terminal carrier-path conflict must surface a verter(project) diagnostic on \
                 the debounced path, got {diagnostics:?}"
            )
        });
    // The disk-layout cause (companion-path occupancy) — distinct from the NoProject
    // message — so a mis-classification as NoProject would fail here too.
    assert!(
        conflict.message.contains("companion path"),
        "the conflict diagnostic must explain the disk-layout (companion-path) cause, got {}",
        conflict.message
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_file_preserves_open_vue_state_on_owner_none_ready_snapshot() {
    // AUDIT (sync_coordinator, invariant a): the debounced sync processes
    // OPEN documents (signalled from did_change). When a READY ownership
    // snapshot resolves no owner for an OPEN `.vue`, the coordinator must
    // NOT clear the state nor close its live TSX — it must preserve the
    // open document's Unresolved state. Pre-fix it called
    // `clear_provider_sync_state` (remove + close) on owner-None+ready.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/App.vue";
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    // did_open registers the URI→canonical mapping (so the file reads as
    // OPEN) and feeds the content into the host — no disk I/O.
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<template><div>{{ msg }}</div></template>".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot whose only project lives at `/other` — it does NOT own
    // the open workspace file, so owner resolution returns None.
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    ));
    let provider_sync_states = Arc::new(DashMap::new());
    let ide_path = format!("{canonical_id}.tsx");
    provider_sync_states.insert(
        canonical_id.to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            ide_path: Some(ide_path.clone()),
            api_path: Some(format!("{canonical_id}.ts")),
            decl_path: None,
            ide_background_loaded: true,
            api_background_loaded: true,
            decl_background_loaded: false,
            shadow_path: None,
            shadow_background_loaded: false,
            committed_ide_surface: None,
            commit_stamp: None,
            api_delivered_hash: None,
            api_observed_hash: None,
            shadow_delivered_source_hash: None,
        },
    );

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, uri.as_str()).await;

    // Discriminator: the open file's state must SURVIVE (pre-fix it was
    // removed by clear_provider_sync_state).
    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("open Vue file must keep its provider state across owner-None ready sync");
    assert!(
        state.is_unresolved(),
        "owner-None ready sync must keep the open file Unresolved, got {:?}",
        state.owner_binding
    );
    // Negative: the live TSX must NOT be closed.
    let calls = provider.file_sync_calls();
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == &ide_path
        )),
        "owner-None ready sync must NOT close the open file's live TSX, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rune_module_debounced_diagnostics_map_through_self_file_projection() {
    // P0b (diagnostics half): the debounced coordinator must route a rune
    // module's type diagnostics through the GENERALIZED self-file projection
    // — querying the type provider at the module's OWN canonical path (the
    // Shadow buffer `<rune prelude> + <bytes>`) and mapping each diagnostic
    // back to the user-source position through the rewrite-aware self-file
    // mapper (prelude offset undone). The carrier IDE-source-map path
    // requires an `ide_path` a rune module never has, so without the
    // self-file route the type diagnostic would be dropped entirely.
    use crate::type_provider::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical_id = "/workspace/store.svelte.ts";
    let source = "export const s = $state(0);\nexport const bad: number = 'x';\n";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::<str>::from(source),
        file_language: crate::server::self_file_language_for(canonical_id)
            .expect("the path classifies as a rune module"),
        aliases: Vec::new(),
    });
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/store.svelte.ts"
        .parse()
        .expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: source.to_string(),
    });

    let file_language = crate::server::self_file_language_for(canonical_id).unwrap();
    let provider = Arc::new(MockTypeProvider::new());

    let provider_sync_states = Arc::new(DashMap::new());
    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states,
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    // A REAL shadow sync (the production primitive): commits the Shadow state
    // AND records the Shadow surface the diagnostics path captures.
    assert!(
        crate::server::sync_self_file_shadow_state(
            &deps.documents,
            deps.project_sync.as_ref().expect("test deps carry a sync"),
            &deps.provider_sync_states,
            &|| None,
            &uri,
            canonical_id,
            &file_language,
            deps.type_provider_kind.requires_explicit_source_graph(),
        )
        .await,
        "the shadow sync should succeed against the mock provider"
    );

    // The EXACT provider buffer the coordinator queries against is the recorded
    // Shadow surface. The prelude shifts every user line down; locate the user
    // token `bad` (source line 1) inside the provider buffer and set a type
    // diagnostic over it at provider byte offsets.
    let provider_content = documents
        .provider_surfaces()
        .current_snapshot(canonical_id)
        .expect("the shadow sync records a Shadow surface")
        .provider_content
        .clone();
    let provider_bad = provider_content
        .find("bad")
        .expect("token present in provider buffer");
    provider.set_diagnostics(
        canonical_id,
        vec![TypeDiagnostic {
            message: "Type 'string' is not assignable to type 'number'.".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: provider_bad as u32,
            end: (provider_bad + 3) as u32,
            code: Some("2322".to_string()),
            tags: Vec::new(),
            related_information: Vec::new(),
        }],
    );

    let merged = self_file_diagnostics(&deps, provider.as_ref(), canonical_id, Vec::new()).await;

    // The type provider must have been queried at the module's OWN canonical
    // path (the Shadow buffer), never a derived `.tsx`.
    let calls = provider.calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::GetDiagnostics { path } if path == canonical_id
        )),
        "the rune diagnostics path must query the provider at the OWN canonical path, calls={calls:?}"
    );

    // Discriminator: the type diagnostic survives AND lands on user-source
    // line 1 (the `bad` declaration) — the prelude offset has been undone by
    // the self-file mapper. Without the projection route the diagnostic is
    // dropped (the carrier path has no `ide_path`).
    assert_eq!(
        merged.len(),
        1,
        "the type diagnostic must survive the merge"
    );
    let diag = &merged[0];
    assert_eq!(
        diag.range.start.line, 1,
        "the diagnostic must map back to user-source line 1 (prelude offset undone), got line {}",
        diag.range.start.line
    );
    // And it must NOT be left at a prelude-shifted line (the bug signature).
    let provider_li = LineIndex::new(&provider_content, deps.position_encoding.read().clone());
    let provider_line = provider_li
        .offset_to_position(provider_bad as u32)
        .expect("provider position")
        .line;
    assert_ne!(
        diag.range.start.line, provider_line,
        "the diagnostic must NOT stay at the prelude-shifted provider line {provider_line}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_file_routes_open_rune_module_through_self_file_shadow_not_carrier() {
    // P0b: the debounced coordinator must recognize an OPEN rune module
    // (`.svelte.ts`/`.svelte.js`) as a SELF-FILE buffer and route it through
    // the shared self-file Shadow-sync path — NOT the carrier-miss
    // `preserve_open_unresolved_carrier`, which would CLOBBER the Shadow
    // state with an IDE-path state and break did_close cleanup.
    //
    // The snapshot is OWNER-NONE for the open rune (only `/other` is a
    // project) — pre-fix that drove `carrier_sync_state_for_source` to None
    // and the open file into `preserve_open_unresolved_carrier`, which
    // OVERWRITES `shadow_path` with an `ide_path`. Post-fix the coordinator
    // intercepts the rune module BEFORE the carrier branch and re-syncs its
    // Shadow buffer at its OWN canonical path.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/store.svelte.ts";
    let uri: Uri = "file:///workspace/store.svelte.ts"
        .parse()
        .expect("test uri");
    // did_open registers the URI→canonical mapping (file reads as OPEN) and
    // builds the self-file projection.
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const s = $state(0);\n".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot whose only project lives at `/other` — it does NOT own
    // the open `/workspace` rune module, so owner resolution returns None.
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    ));
    // Pre-seed the rune module's self-file Shadow state at its OWN path.
    let provider_sync_states = Arc::new(DashMap::new());
    provider_sync_states.insert(
        canonical_id.to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            shadow_path: Some(canonical_id.to_string()),
            shadow_background_loaded: true,
            ..Default::default()
        },
    );

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, uri.as_str()).await;

    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the open rune module must keep its self-file provider state across the tick");
    // Discriminator: the Shadow path must SURVIVE as the module's OWN
    // canonical id. Pre-fix `preserve_open_unresolved_carrier` clobbered it
    // (shadow_path → None, ide_path → a `.tsx` path).
    assert_eq!(
        state.shadow_path.as_deref(),
        Some(canonical_id),
        "the coordinator tick must preserve the rune module's Shadow path, got {:?}",
        state.shadow_path
    );
    assert!(
        state.ide_path.is_none(),
        "a rune module has no IDE path — the carrier-miss path must not have committed one, got {:?}",
        state.ide_path
    );
    assert!(
        state.is_unresolved(),
        "the owner-None tick keeps the rune module Unresolved, got {:?}",
        state.owner_binding
    );
    // The coordinator must have re-synced the Shadow buffer at the module's
    // OWN canonical path (a refresh `sync_file` since it was already loaded);
    // it must NEVER open/sync a derived `.tsx` carrier path for it.
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } | MockCall::LoadFile { path, .. }
            if path == canonical_id
        )),
        "the coordinator must re-sync the rune module's OWN-path Shadow buffer, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path.ends_with(".tsx")
        )),
        "the coordinator must NOT sync a derived carrier `.tsx` path for a rune module, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_file_routes_open_plain_script_through_self_file_shadow_not_carrier() {
    // The plain-script half of the shadow-vs-dependency interleave (mirrors
    // `sync_file_routes_open_rune_module_through_self_file_shadow_not_carrier`):
    // an OPEN plain `.ts` is a SELF-FILE buffer — the debounced coordinator must
    // re-sync its own-path Shadow buffer and must NEVER fall through to the
    // carrier-miss `preserve_open_unresolved_carrier`, which would clobber the
    // Shadow state with an IDE-path state (breaking did_close cleanup), nor to
    // the non-open dependency branch, which would clear an OPEN document's
    // provider state from under it.
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/util.ts";
    let uri: Uri = "file:///workspace/src/util.ts".parse().expect("test uri");
    // did_open registers the URI→canonical mapping (file reads as OPEN) and
    // builds the self-file projection (identity mapping for a plain script).
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const utilValue: number = 1;\n".to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot whose only project lives at `/other` — it does NOT own
    // the open `/workspace` plain script, so owner resolution returns None.
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    ));
    // Pre-seed the plain script's self-file Shadow state at its OWN path.
    let provider_sync_states = Arc::new(DashMap::new());
    provider_sync_states.insert(
        canonical_id.to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            shadow_path: Some(canonical_id.to_string()),
            shadow_background_loaded: true,
            ..Default::default()
        },
    );

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, uri.as_str()).await;

    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the open plain script must keep its self-file provider state across the tick");
    // Discriminator: the Shadow path must SURVIVE as the script's OWN
    // canonical id. The carrier-miss path would clobber it (shadow_path →
    // None, ide_path → a `.tsx` path); the non-open dependency branch would
    // have REMOVED the state entirely.
    assert_eq!(
        state.shadow_path.as_deref(),
        Some(canonical_id),
        "the coordinator tick must preserve the plain script's Shadow path, got {:?}",
        state.shadow_path
    );
    assert!(
        state.ide_path.is_none(),
        "a plain script has no IDE path — the carrier-miss path must not have committed one, got {:?}",
        state.ide_path
    );
    assert!(
        state.is_unresolved(),
        "the owner-None tick keeps the plain script Unresolved, got {:?}",
        state.owner_binding
    );
    // The coordinator must have re-synced the Shadow buffer at the script's
    // OWN canonical path (a refresh `UpdateFile` since it was already loaded);
    // it must NEVER open/sync a derived `.tsx` carrier path for it.
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. } | MockCall::LoadFile { path, .. }
            if path == canonical_id
        )),
        "the coordinator must re-sync the plain script's OWN-path Shadow buffer, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path != canonical_id
        )),
        "the coordinator must NOT sync any derived carrier path for a plain script, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_file_clears_non_open_plain_script_dependency_state_once_ready() {
    // The dependency half of the shadow-vs-dependency interleave: a plain
    // `.ts` that is NOT open in the editor reaches the provider only as a
    // background dependency buffer. Once resolver ownership is ready the
    // provider reads the file from disk, so the debounced coordinator must
    // RETIRE the redundant buffer — closing the own-path Shadow provider file
    // and removing the sync state (the rune-module branch documents the same
    // "genuinely non-open … removed once ready" discipline).
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical_id = "/workspace/src/util.ts";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::<str>::from("export const utilValue: number = 1;\n"),
        file_language: crate::server::self_file_language_for(canonical_id)
            .expect("the path classifies as a plain script"),
        aliases: Vec::new(),
    });
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    // No did_open: the plain script is a NON-open (background dependency) file.

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot that OWNS `/workspace` (a configured project).
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.json"),
    ));
    // Pre-seed the dependency shadow state a background sync would have
    // committed while ownership was unresolved.
    let provider_sync_states = Arc::new(DashMap::new());
    provider_sync_states.insert(
        canonical_id.to_string(),
        ProviderSyncState {
            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
            shadow_path: Some(canonical_id.to_string()),
            shadow_background_loaded: true,
            ..Default::default()
        },
    );

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, "file:///workspace/src/util.ts").await;

    // Discriminator: the non-open dependency state is RETIRED once ready …
    assert!(
        provider_sync_states.get(canonical_id).is_none(),
        "a non-open plain script's dependency state must be removed once ownership is ready"
    );
    // … and the redundant provider buffer is CLOSED at the script's OWN path
    // (never a derived carrier path).
    let calls = provider.file_sync_calls();
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::CloseFile { path } if path == canonical_id
        )),
        "the coordinator must close the non-open plain script's own-path buffer, calls={calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_file_retains_stale_paths_when_owner_change_sync_fails() {
    // AUDIT (sync_coordinator, invariant b): on an owner change the
    // coordinator must sync the NEW paths first and close stale paths only
    // AFTER a successful sync. Pre-fix it closed `transition.stale_paths`
    // BEFORE syncing, so a failed sync left the prior live paths closed.
    let canonical_id = "/workspace/src/App.vue";
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::<str>::from(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        ),
        file_language: FileLanguage::vue(),
        aliases: Vec::new(),
    });
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    // Resolver owns the file at `/workspace` → owner-aware state with a
    // `.tsx`/`.ts` path; a prior Owned state from a DIFFERENT owner makes the
    // same paths force-rebind stale.
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.json"),
    ));

    let ide_path = format!("{canonical_id}.tsx");
    let api_path = format!("{canonical_id}.verter.ts");
    let provider = Arc::new(MockTypeProvider::new());
    // Permit dependency-overlay publication, then fail both replacement carriers
    // so the total-failure rollback path is exercised at the intended boundary.
    provider.set_fail_sync_path(&ide_path);
    provider.set_fail_sync_path(&api_path);

    let provider_sync_states = Arc::new(DashMap::new());
    let prior_state = ProviderSyncState {
        owner_binding: crate::provider_sync::ProviderOwnerBinding::Owned(
            "/old/tsconfig.json".to_string(),
        ),
        ide_path: Some(ide_path.clone()),
        api_path: Some(api_path.clone()),
        decl_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_path: None,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
        api_delivered_hash: None,
        api_observed_hash: None,
        shadow_delivered_source_hash: None,
    };
    provider_sync_states.insert(canonical_id.to_string(), prior_state.clone());

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, "file:///workspace/src/App.vue").await;

    let calls = provider.file_sync_calls();
    // Reach (R3-2): the coordinator must have ATTEMPTED to sync the new
    // owner's IDE `.tsx` (the failing mock records the open/update before
    // erroring) before the no-close assertion. A no-op impl that returned
    // before syncing would pass the absence-of-close assertion vacuously.
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &ide_path
        )),
        "failed owner-change sync must REACH the sync and attempt the new `.tsx`, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. }
                | MockCall::UpdateFile { path, .. }
                | MockCall::LoadFile { path, .. }
            if path == &api_path
        )),
        "failed owner-change sync must REACH the sync and attempt the new `.verter.ts`, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateFile { path, .. }
                if path == &format!("{ide_path}.__verter_types.d.ts")
        )),
        "the existing dependency overlay must publish before the carrier failure, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call, MockCall::CloseFile { path } if path == &ide_path
        )),
        "a failed owner-change sync must preserve the prior IDE carrier, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call, MockCall::CloseFile { path } if path == &api_path
        )),
        "a failed owner-change sync must preserve the prior API carrier, calls={calls:?}"
    );
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            MockCall::CloseFile { path }
                if path != &format!("{ide_path}.__verter_types.d.ts")
        )),
        "only the newly-created dependency overlay may be rollback-closed, calls={calls:?}"
    );
    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the prior provider state remains committed");
    assert_eq!(
        state, prior_state,
        "total replacement failure must retain the complete prior state"
    );
}

/// The coordinator's owner-resolved DIRECT-OPEN (tsgo) IDE sync must record the
/// `CarrierIde` surface it delivered: the debounced coordinator is the LIVE
/// re-sync path after an edit, and without the record the interactive
/// request-surface capture misses — every provider-backed feature then silently
/// drops its provider contribution until another producer records.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_direct_ide_sync_records_carrier_ide_surface() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/App.vue";
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n\
               <template><div>{{ msg }}</div></template>\n"
            .to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Owner-resolving snapshot rooted at the workspace: DirectOpen (tsgo).
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.json"),
    ));
    let provider_sync_states = Arc::new(DashMap::new());

    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, uri.as_str()).await;

    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the owner-resolved coordinator sync must commit provider state");
    assert!(
        state.ide_background_loaded,
        "the coordinator's direct IDE sync must mark the IDE kind live"
    );
    let ide_path = state
        .ide_path
        .clone()
        .expect("the owner-resolved sync must commit an IDE path");
    let snapshot = documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("the coordinator's successful direct IDE sync must record a CarrierIde surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the recorded surface must carry the CarrierIde role"
    );
    let delivered = deps
        .project_sync
        .as_ref()
        .and_then(|sync| sync.synced_tsx_content(&ide_path))
        .expect("the coordinator records the projected provider bytes");
    assert_eq!(
        snapshot.provider_content.as_ref(),
        delivered.as_ref(),
        "the recorded surface must pin the EXACT bytes delivered to the provider"
    );
    assert!(delivered.contains("from \"./App.vue.tsx.__verter_types\""));
    assert!(!delivered.contains("from \"@verter/types\""));

    // A FAILED direct IDE sync records NOTHING (fail closed).
    let other_id = "/workspace/src/Other.vue";
    let other_uri: Uri = "file:///workspace/src/Other.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: other_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst other = 1\n</script>\n\
               <template><div>{{ other }}</div></template>\n"
            .to_string(),
    });
    provider.set_fail_sync_path("/workspace/src/Other.vue.tsx");
    sync_file(&deps, other_id, other_uri.as_str()).await;
    assert!(
        documents
            .provider_surfaces()
            .current_snapshot("/workspace/src/Other.vue.tsx")
            .is_none(),
        "a failed coordinator IDE sync must not record a provider surface"
    );
}

/// The coordinator's OPEN-UNRESOLVED preserve (owner-None over a ready
/// snapshot) keeps the open document's IDE TSX live AND queryable — so a
/// successful preserve sync must record the `CarrierIde` surface it delivered,
/// or the interactive capture misses for the unresolved carrier.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_open_unresolved_preserve_records_carrier_ide_surface() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/App.vue";
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n\
               <template><div>{{ msg }}</div></template>\n"
            .to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot rooted ELSEWHERE: owner resolution returns None for the
    // open file, driving the preserve-open-unresolved path.
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/other",
        Some("/other/tsconfig.json"),
    ));
    let provider_sync_states = Arc::new(DashMap::new());

    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    sync_file(&deps, canonical_id, uri.as_str()).await;

    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the open unresolved carrier must commit provider state");
    assert!(
        state.is_unresolved(),
        "owner-None over a ready snapshot must commit an Unresolved binding"
    );
    let ide_path = state
        .ide_path
        .clone()
        .expect("the preserve must keep a live IDE path");
    let snapshot = documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("a successful open-unresolved preserve sync must record a CarrierIde surface");
    assert_eq!(
        snapshot.kind,
        crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
        "the recorded surface must carry the CarrierIde role"
    );
}

/// Shared setup for the background carrier-diagnostics tests: an owner-resolved,
/// coordinator-synced carrier (its CarrierIde surface recorded), plus a provider
/// type diagnostic positioned over the script's `const msg` statement so a
/// successful merge maps it back into the `.vue` source.
async fn make_carrier_diagnostics_fixture() -> (
    Arc<DocumentRegistry>,
    Arc<DashMap<String, ProviderSyncState>>,
    Arc<MockTypeProvider>,
    String,
    String,
    SyncCoordinatorDeps,
) {
    use crate::type_provider::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/App.vue".to_string();
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n\
               <template><div>{{ msg }}</div></template>\n"
            .to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let vfs_workspace = Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
        "/workspace",
        Some("/workspace/tsconfig.json"),
    ));
    let provider_sync_states = Arc::new(DashMap::new());

    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::clone(&provider_sync_states),
        vfs_workspace,
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };
    sync_file(&deps, &canonical_id, uri.as_str()).await;

    let ide_path = provider_sync_states
        .get(&canonical_id)
        .and_then(|state| state.ide_path.clone())
        .expect("the owner-resolved sync must commit an IDE path");
    // A provider reports offsets into the buffer IT holds, not into the raw
    // compiler output: the carrier projection rewrites import specifiers, so the
    // two buffers diverge from the first rewritten import onward. Seed the mock
    // where a real engine would answer — inside the recorded provider surface.
    let recorded = documents
        .provider_surfaces()
        .current_snapshot(&ide_path)
        .expect("a successful sync records the provider surface it delivered");
    let diag_start = recorded
        .provider_content
        .find("const msg")
        .expect("script statement present in the provider buffer")
        + "const ".len();
    provider.set_diagnostics(
        &ide_path,
        vec![TypeDiagnostic {
            message: "PROVIDER_DIAG_SENTINEL".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: diag_start as u32,
            end: (diag_start + 3) as u32,
            code: Some("2322".to_string()),
            tags: Vec::new(),
            related_information: Vec::new(),
        }],
    );
    (
        documents,
        provider_sync_states,
        provider,
        canonical_id,
        ide_path,
        deps,
    )
}

/// STABLE surface: the background carrier-diagnostics merge serves the
/// provider's type diagnostic mapped into the `.vue` source — guards against
/// an over-eager fail-closed gate dropping healthy background diagnostics.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_diagnostics_serve_provider_results_from_stable_recorded_surface() {
    let (documents, provider_sync_states, provider, canonical_id, _ide_path, _deps) =
        make_carrier_diagnostics_fixture().await;

    let merged = carrier_provider_diagnostics(
        &documents,
        &provider_sync_states,
        provider.as_ref(),
        PositionEncodingKind::UTF16,
        &canonical_id,
        Vec::new(),
    )
    .await;
    assert!(
        merged.iter().any(|d| d.message == "PROVIDER_DIAG_SENTINEL"),
        "a stable recorded surface must serve the provider diagnostic, got {merged:?}"
    );
}

/// `needs_provider_sync` is a reconciliation work bit, not a document revision:
/// an interactive IDE sync may reinsert it for deferred API work after the
/// coordinator has synchronized the current carrier. Valid diagnostics for that
/// same LSP version must still run.
#[tokio::test(flavor = "multi_thread")]
async fn diagnostics_are_not_suppressed_by_same_version_deferred_api_work() {
    let (_documents, _states, provider, canonical_id, ide_path, deps) =
        make_carrier_diagnostics_fixture().await;
    provider.clear_calls();
    deps.needs_provider_sync.insert(canonical_id.clone());

    publish_merged_diagnostics(&deps, &canonical_id, "file:///workspace/src/App.vue").await;

    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDiagnostics { path } if path == &ide_path)),
        "same-version deferred API work must not suppress provider diagnostics"
    );
}

/// tsserver's synchronous diagnostic response has no authored LSP version in
/// its payload. The coordinator captures the current version before the pull
/// and must reject the response if that version advances, even when the edit is
/// textually identical and the provider-surface generation therefore remains
/// valid. This isolates the document-version fence from the surface fence.
///
/// @ai-generated - Guards exact-version diagnostics publication after provider I/O.
#[tokio::test(flavor = "multi_thread")]
async fn provider_diagnostics_are_not_published_for_a_superseded_document_version() {
    let (documents, _states, provider, canonical_id, ide_path, mut deps) =
        make_carrier_diagnostics_fixture().await;
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let source = documents
        .get(&uri)
        .expect("fixture document")
        .source
        .to_string();

    let documents_during_query = Arc::clone(&documents);
    let uri_during_query = uri.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            let _ = documents_during_query.did_change(&uri_during_query, 2, &source);
        }),
    );

    let client_slot = Arc::new(std::sync::Mutex::new(None));
    let client_slot_for_service = Arc::clone(&client_slot);
    let (mut service, mut socket) = LspService::new(move |client| {
        *client_slot_for_service.lock().expect("client lock") = Some(client.clone());
        NoopLanguageServer
    });
    deps.client = client_slot
        .lock()
        .expect("client lock")
        .clone()
        .expect("test client should be captured");
    let initialize = tower_lsp_server::jsonrpc::Request::build("initialize")
        .id(1)
        .params(serde_json::json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        }))
        .finish();
    let response = tower_service::Service::call(&mut service, initialize)
        .await
        .expect("initialize service call")
        .expect("initialize response");
    assert!(
        response.is_ok(),
        "test client must initialize: {response:?}"
    );

    let publish_uri = uri.to_string();
    let publish = tokio::spawn(async move {
        publish_merged_diagnostics(&deps, &canonical_id, &publish_uri).await
    });
    let request = socket
        .next()
        .await
        .expect("the staged Verter diagnostics batch must publish");
    assert_eq!(request.method(), "textDocument/publishDiagnostics");
    let params: PublishDiagnosticsParams = serde_json::from_value(
        serde_json::to_value(request.params().expect("publish params"))
            .expect("publish params serialize"),
    )
    .expect("publish params deserialize");
    assert_eq!(params.version, Some(1));
    publish
        .await
        .expect("the diagnostics publication task must not panic");
    assert_eq!(
        documents.get(&uri).map(|document| document.version),
        Some(2),
        "the provider callback must advance only the authored document version"
    );
    assert!(
        socket.next().now_or_never().is_none(),
        "the provider result for version 1 must not publish after version 2 becomes current"
    );
}

/// A provider diagnostic pass can legitimately be unbounded while tsserver
/// constructs a cold configured project. Framework diagnostics are independent
/// snapshot results and must reach the editor before that pull completes.
#[tokio::test(flavor = "multi_thread")]
async fn hanging_provider_diagnostics_do_not_starve_verter_owned_batch() {
    let (documents, _states, provider, canonical_id, ide_path, mut deps) =
        make_carrier_diagnostics_fixture().await;
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let source = "<script setup lang=\"ts\">\n\
                  defineProps<{ deadProp: string }>();\n\
                  </script>\n\
                  <template><div /></template>\n";
    let _ = documents.did_change(&uri, 2, source);
    sync_file(&deps, &canonical_id, uri.as_str()).await;
    provider.clear_calls();
    provider.hang_diagnostics();

    let client_slot = Arc::new(std::sync::Mutex::new(None));
    let client_slot_for_service = Arc::clone(&client_slot);
    let (mut service, mut socket) = LspService::new(move |client| {
        *client_slot_for_service.lock().expect("client lock") = Some(client.clone());
        NoopLanguageServer
    });
    deps.client = client_slot
        .lock()
        .expect("client lock")
        .clone()
        .expect("test client should be captured");
    let initialize = tower_lsp_server::jsonrpc::Request::build("initialize")
        .id(1)
        .params(serde_json::json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {}
        }))
        .finish();
    let response = tower_service::Service::call(&mut service, initialize)
        .await
        .expect("initialize service call")
        .expect("initialize response");
    assert!(
        response.is_ok(),
        "test client must initialize: {response:?}"
    );

    let publish = tokio::spawn(async move {
        publish_merged_diagnostics(&deps, &canonical_id, uri.as_str()).await
    });
    let request = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("Verter diagnostics must publish without waiting for the provider")
        .expect("client socket must remain open");
    assert_eq!(request.method(), "textDocument/publishDiagnostics");
    let params: PublishDiagnosticsParams = serde_json::from_value(
        serde_json::to_value(request.params().expect("publish params"))
            .expect("publish params serialize"),
    )
    .expect("publish params deserialize");
    assert_eq!(params.version, Some(2));
    assert!(
        params.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_ref(),
                Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
            )
        }),
        "the first staged batch must contain the ready Verter hint: {:?}",
        params.diagnostics
    );

    tokio::task::yield_now().await;
    assert!(
        provider
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::GetDiagnostics { path } if path == &ide_path)),
        "the provider pull must still start in the background"
    );
    assert!(
        !publish.is_finished(),
        "the test provider remains wedged after the staged Verter publish"
    );
    publish.abort();
}

/// A provider re-sync landing a FRESH surface generation while the background
/// diagnostics request is awaiting the provider must cause the provider
/// diagnostics to be DROPPED (fail closed): the response was produced against a
/// surface that no longer matches, and mapping it through a torn context would
/// publish wrong positions. The Verter-only diagnostics still publish.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_diagnostics_drop_provider_results_when_surface_regenerates_mid_request() {
    let (documents, provider_sync_states, provider, canonical_id, ide_path, _deps) =
        make_carrier_diagnostics_fixture().await;

    let store = documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    let raced_canonical = canonical_id.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            // A concurrent re-sync lands a NEW generation with drifted content
            // between the capture and the merge of the response.
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::CarrierIde,
                    raced_path,
                    raced_canonical,
                    Arc::from("// drifted ide content"),
                    None,
                    Arc::from("// drifted carrier source"),
                ),
            );
        }),
    );

    let merged = carrier_provider_diagnostics(
        &documents,
        &provider_sync_states,
        provider.as_ref(),
        PositionEncodingKind::UTF16,
        &canonical_id,
        Vec::new(),
    )
    .await;
    assert!(
        !merged.iter().any(|d| d.message == "PROVIDER_DIAG_SENTINEL"),
        "provider diagnostics produced against a superseded surface generation must be \
         DROPPED, got {merged:?}"
    );
}

/// A surface retirement (a racing close) while the background diagnostics
/// request is awaiting the provider must fail closed: the provider diagnostics
/// drop, the Verter-only set survives, no panic.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_diagnostics_drop_provider_results_when_surface_retired_mid_request() {
    let (documents, provider_sync_states, provider, canonical_id, ide_path, _deps) =
        make_carrier_diagnostics_fixture().await;

    let store = documents.provider_surfaces().clone();
    let raced_path = ide_path.clone();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            // The surface is retired mid-request (a racing close began); the
            // close is not yet confirmed, so the token is deliberately kept.
            let _token = store.forget(&raced_path);
        }),
    );

    let merged = carrier_provider_diagnostics(
        &documents,
        &provider_sync_states,
        provider.as_ref(),
        PositionEncodingKind::UTF16,
        &canonical_id,
        Vec::new(),
    )
    .await;
    assert!(
        !merged.iter().any(|d| d.message == "PROVIDER_DIAG_SENTINEL"),
        "provider diagnostics racing a surface retirement must be DROPPED, got {merged:?}"
    );
}

/// A shadow-surface mutation landing while the rune-module diagnostics request
/// is awaiting the provider must cause the provider diagnostics to be DROPPED
/// (fail closed): the response was produced against a Shadow surface that no
/// longer matches, so mapping it through the superseded rewrite-aware mapper
/// would publish wrong positions.
#[tokio::test(flavor = "multi_thread")]
async fn rune_diagnostics_drop_provider_results_when_shadow_surface_regenerates_mid_request() {
    use crate::type_provider::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let canonical_id = "/workspace/store.svelte.ts";
    let source = "export const s = $state(0);\nexport const bad: number = 'x';\n";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::<str>::from(source),
        file_language: crate::server::self_file_language_for(canonical_id)
            .expect("the path classifies as a rune module"),
        aliases: Vec::new(),
    });
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/store.svelte.ts"
        .parse()
        .expect("test uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: source.to_string(),
    });
    let file_language = crate::server::self_file_language_for(canonical_id).unwrap();

    let provider = Arc::new(MockTypeProvider::new());
    let provider_sync_states = Arc::new(DashMap::new());
    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states,
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };
    assert!(
        crate::server::sync_self_file_shadow_state(
            &deps.documents,
            deps.project_sync.as_ref().expect("test deps carry a sync"),
            &deps.provider_sync_states,
            &|| None,
            &uri,
            canonical_id,
            &file_language,
            deps.type_provider_kind.requires_explicit_source_graph(),
        )
        .await,
        "the shadow sync should succeed against the mock provider"
    );

    let provider_content = documents
        .provider_surfaces()
        .current_snapshot(canonical_id)
        .expect("the shadow sync records a Shadow surface")
        .provider_content
        .clone();
    let provider_bad = provider_content
        .find("bad")
        .expect("token present in provider buffer");
    provider.set_diagnostics(
        canonical_id,
        vec![TypeDiagnostic {
            message: "RUNE_PROVIDER_DIAG_SENTINEL".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: provider_bad as u32,
            end: (provider_bad + 3) as u32,
            code: Some("2322".to_string()),
            tags: Vec::new(),
            related_information: Vec::new(),
        }],
    );

    // Mid-request seam: a concurrent shadow re-sync lands a fresh generation
    // with drifted content between the capture and the merge.
    let store = documents.provider_surfaces().clone();
    let raced_canonical = canonical_id.to_string();
    provider.set_on_query(
        canonical_id,
        Box::new(move || {
            store.record(
                crate::provider_surface_store::RecordSurface::carrier_legacy(
                    crate::provider_surface_store::ProviderSurfaceKind::Shadow,
                    raced_canonical.clone(),
                    raced_canonical,
                    Arc::from("// drifted shadow content"),
                    None,
                    Arc::from("// drifted module source"),
                ),
            );
        }),
    );

    let merged = self_file_diagnostics(&deps, provider.as_ref(), canonical_id, Vec::new()).await;
    assert!(
        !merged
            .iter()
            .any(|d| d.message == "RUNE_PROVIDER_DIAG_SENTINEL"),
        "rune provider diagnostics produced against a superseded Shadow surface must be \
         DROPPED, got {merged:?}"
    );
}

/// Provider-less publish half: on a route with NO in-process provider (the
/// editor-owned tsserver plugin, verter-only mode) the coordinator must still
/// publish Verter-owned diagnostics for a signaled open file — the exact
/// end-to-end gap the VS Code E2E exposed (unused-declaration hints never
/// reached the editor because the coordinator did not exist on that route).
///
/// Discriminating: `project_sync: None` was unrepresentable before the fix
/// (the coordinator was only spawned WITH a provider), and the observable —
/// the recomputed verter diagnostics cache entry carrying the
/// `verter/no-unused-props` hint — is written by `publish_merged_diagnostics`,
/// which never ran for this route.
#[tokio::test(flavor = "multi_thread")]
async fn provider_less_coordinator_still_publishes_verter_owned_diagnostics() {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let source = "<script setup lang=\"ts\">\n\
                  defineProps<{ deadProp: string }>();\n\
                  </script>\n\
                  \n\
                  <template>\n\
                  <div />\n\
                  </template>\n";
    let uri: Uri = "file:///workspace/src/UnusedHint.vue".parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    let canonical_id = documents
        .get_canonical_id(&uri)
        .expect("open doc has a canonical id");

    let needs_provider_sync = Arc::new(DashSet::new());
    needs_provider_sync.insert(canonical_id.clone());
    let cached_verter_diags = Arc::new(DashMap::new());
    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync,
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::clone(&cached_verter_diags),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::EditorTsserver,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    let handle = spawn_sync_coordinator(deps);
    handle.signal(canonical_id.clone(), uri.as_str().to_string());

    // Debounce is 300ms; poll for the publish's recomputed verter cache entry.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(entry) = cached_verter_diags.get(uri.as_str()) {
            let has_unused_hint = entry.2.iter().any(|d| {
                matches!(
                    d.code.as_ref(),
                    Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
                )
            });
            if has_unused_hint {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the provider-less coordinator must publish Verter-owned diagnostics \
             (verter/no-unused-props) for a signaled open file; cache: {:?}",
            cached_verter_diags
                .get(uri.as_str())
                .map(|entry| entry.2.clone())
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn semantic_completion_republishes_without_provider_file_sync() {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    documents.set_semantic_analysis_enabled(true);
    let source = "<script setup lang=\"ts\">\n\
                  defineProps<{ deadProp: string }>();\n\
                  </script>\n\
                  <template><div /></template>\n";
    let uri: Uri = "file:///workspace/src/SemanticHint.vue"
        .parse()
        .expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: source.to_string(),
    });
    let canonical_id = documents
        .get_canonical_id(&uri)
        .expect("open doc has a canonical id");
    let provider = Arc::new(MockTypeProvider::new());
    let cached_verter_diags = Arc::new(DashMap::new());
    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new(
            provider.clone(),
            ProjectSyncMode::FullProject,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::clone(&cached_verter_diags),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::Tsserver,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };
    let _handle = spawn_sync_coordinator(deps);
    documents.schedule_semantic_analysis(&uri);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let hint_ready = cached_verter_diags.get(uri.as_str()).is_some_and(|entry| {
            entry.2.iter().any(|diagnostic| {
                matches!(
                    diagnostic.code.as_ref(),
                    Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
                )
            })
        });
        if hint_ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "semantic completion must trigger a diagnostics-only publish"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        provider.calls().is_empty(),
        "optional semantic completion must not open/update provider files or query an uncommitted surface: {:?}",
        provider.calls()
    );
    assert!(
        !documents.semantic_analysis_enabled() || documents.get_analysis(&uri).is_some(),
        "the diagnostic event must follow immutable semantic snapshot commit"
    );
    assert_eq!(
        documents.get_canonical_id(&uri).as_deref(),
        Some(canonical_id.as_str())
    );
}

/// Deps for a verter-only (no in-process provider) coordinator publish.
fn verter_only_deps(documents: Arc<DocumentRegistry>) -> SyncCoordinatorDeps {
    SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::None,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    }
}

/// Install a `svelte` package at `<root>/node_modules/svelte`.
fn install_svelte_at(root: &std::path::Path, manifest: &str) {
    let dir = root.join("node_modules/svelte");
    std::fs::create_dir_all(&dir).expect("svelte package dir");
    std::fs::write(dir.join("package.json"), manifest).expect("manifest");
    std::fs::write(dir.join("index.d.ts"), "export type S = 1;\n").expect("index");
    std::fs::write(dir.join("elements.d.ts"), "export interface E {}\n").expect("elements");
}

const USABLE_SVELTE: &str = r#"{"name":"svelte","version":"5.56.3","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"},"./elements":{"types":"./elements.d.ts"}}}"#;
const UNUSABLE_SVELTE: &str = r#"{"name":"svelte","version":"5.56.3","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"}}}"#;

/// `did_open` and `did_change` both route through this coordinator, and this is
/// the set it hands the client. An unusable `svelte` install must be explained
/// on BOTH — a user who never edits the file still needs to know why every
/// type-aware feature is dark.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_publishes_the_svelte_install_diagnostic_on_open_and_on_change() {
    let tmp = tempfile::tempdir().expect("workspace");
    install_svelte_at(tmp.path(), UNUSABLE_SVELTE);
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir");
    let canonical_id = src_dir
        .join("App.svelte")
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::write(&canonical_id, "<div>hi</div>").expect("component");

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: "<div>hi</div>".to_string(),
    });
    let deps = verter_only_deps(Arc::clone(&documents));

    let on_open = compute_merged_diagnostics(&deps, &canonical_id, &uri).await;
    let has_unusable = |diags: &[Diagnostic]| {
        diags.iter().any(|d| {
            matches!(&d.code, Some(NumberOrString::String(code))
                if code == crate::svelte_assets::SVELTE_PACKAGE_UNUSABLE_CODE)
        })
    };
    assert!(
        has_unusable(&on_open),
        "an unusable svelte install must be explained on did_open, got {on_open:?}"
    );

    // The same document after an edit — the debounced change publish must not
    // drop the category.
    let _ = documents.did_change(&uri, 2, "<div>edited</div>");
    let on_change = compute_merged_diagnostics(&deps, &canonical_id, &uri).await;
    assert!(
        has_unusable(&on_change),
        "the diagnostic must survive did_change, got {on_change:?}"
    );
}

/// The negative control: a healthy install publishes NO install diagnostic on
/// the same path. Without it the test above would pass for a coordinator that
/// warns unconditionally.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_stays_silent_for_a_usable_svelte_install() {
    let tmp = tempfile::tempdir().expect("workspace");
    install_svelte_at(tmp.path(), USABLE_SVELTE);
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir");
    let canonical_id = src_dir
        .join("App.svelte")
        .to_string_lossy()
        .replace('\\', "/");
    std::fs::write(&canonical_id, "<div>hi</div>").expect("component");

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: "<div>hi</div>".to_string(),
    });
    let deps = verter_only_deps(documents);

    let diags = compute_merged_diagnostics(&deps, &canonical_id, &uri).await;
    assert!(
        !diags
            .iter()
            .any(|d| matches!(&d.code, Some(NumberOrString::String(code))
            if code == crate::svelte_assets::SVELTE_PACKAGE_UNUSABLE_CODE
                || code == crate::svelte_assets::SVELTE_PACKAGE_MISSING_CODE)),
        "a usable install must publish no svelte install diagnostic, got {diags:?}"
    );
}

/// The user-visible half of the fail-closed contract: when a carrier's provider
/// surface cannot be prepared at all, the coordinator PUBLISHES the reason.
///
/// This is the reachability the internal ledger alone cannot prove. A first-open
/// preparation failure never commits a provider path for the document, so any
/// lookup keyed on committed provider state finds nothing and the user is left
/// with a silently dark file and a log line.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_publishes_carrier_provider_unavailable_for_an_unpreparable_carrier() {
    let owner = tempfile::tempdir().expect("workspace");
    install_svelte_at(owner.path(), USABLE_SVELTE);
    let src_dir = owner.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir");
    let canonical_id = src_dir
        .join("Component.svelte")
        .to_string_lossy()
        .replace('\\', "/");
    let provider_path = format!("{canonical_id}.tsx");
    std::fs::write(&canonical_id, "<div>hi</div>").expect("component");
    let generated = "/** @jsxImportSource @verter/svelte-jsx */\nconst view = <div />;\n";

    let sync = ProjectSync::new_with_kind(
        Arc::new(MockTypeProvider::new()),
        ProjectSyncMode::FullProject,
        crate::TypeProviderKind::Tsgo,
    );

    // Block the owner-bound asset directory with a FILE so materialization hits
    // a real, unmodellable I/O failure.
    let asset_dir = crate::svelte_assets::owner_asset_dir_for_test(&provider_path, generated)
        .expect("a usable owner has an owner-bound asset directory");
    let _ = std::fs::remove_dir_all(&asset_dir);
    std::fs::create_dir_all(asset_dir.parent().expect("owners dir")).expect("owners dir");
    std::fs::write(&asset_dir, b"not a directory").expect("block the asset directory");
    struct Unblock(std::path::PathBuf);
    impl Drop for Unblock {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let unblock = Unblock(asset_dir);

    // Production ordering: the coordinator syncs the carrier, then publishes.
    // NOTHING is committed to `provider_sync_states` — exactly the first-open
    // shape in which the failure must still be findable.
    assert!(
        sync.carrier_provider_surface(&provider_path, generated)
            .is_none(),
        "an unmodellable buffer must fail closed"
    );

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: "<div>hi</div>".to_string(),
    });
    let mut deps = verter_only_deps(Arc::clone(&documents));
    deps.project_sync = Some(sync.clone());

    let diags = compute_merged_diagnostics(&deps, &canonical_id, &uri).await;
    let unavailable = |diags: &[Diagnostic]| {
        diags.iter().any(|d| {
            matches!(&d.code, Some(NumberOrString::String(code))
                if code == crate::server::CARRIER_PROVIDER_UNAVAILABLE_CODE)
        })
    };
    assert!(
        unavailable(&diags),
        "a carrier whose provider surface cannot be prepared must be EXPLAINED to \
         the user, not only logged; got {diags:?}"
    );

    // Recovery: once preparation succeeds the explanation must disappear, or it
    // becomes a permanent false alarm.
    drop(unblock);
    assert!(
        sync.carrier_provider_surface(&provider_path, generated)
            .is_some(),
        "a repaired environment prepares again"
    );
    let recovered = compute_merged_diagnostics(&deps, &canonical_id, &uri).await;
    assert!(
        !unavailable(&recovered),
        "the explanation must clear once the surface can be prepared, got {recovered:?}"
    );
}
