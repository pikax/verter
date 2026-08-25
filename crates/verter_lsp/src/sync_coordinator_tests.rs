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
use std::time::Duration;
use tokio::time::Instant;
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
            tokio::time::Instant::now(),
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
    preserve_open_unresolved_carrier(
        &deps,
        &project_sync,
        "/workspace/src/App.vue",
        false,
        None,
        None,
    )
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

/// The coordinator's direct-open IDE sync captures `ide.code` and the
/// carrier's live source at TWO DIFFERENT instants: `ide` is read from the
/// host's compile cache before the provider await, while
/// `resolve_carrier_source` (inside the eventual record) re-reads whatever
/// document text is live AT RECORD TIME — with no identity fence pinning the
/// two together, unlike the interactive repair path's
/// `record_carrier_ide_snapshot_if_current` / `retained_ide_response_is_current`.
///
/// A `did_change` landing in the provider-await window is exactly the
/// documented "surface a request-time repair must resync" scenario — but
/// this path's finish then pairs the PRE-EDIT `ide.code` (compiled from
/// revision A) with the POST-EDIT live source (revision B) it re-reads at
/// record time. That pair validates every live-source hash comparison
/// (`request_surface_matches_live_source`) because both sides of that later
/// comparison are revision B, so a subsequent hover/completion/definition
/// request reads revision A's TSX as if it were current — a torn pairing
/// serving a stale type, never a repair.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_direct_ide_sync_must_not_pair_stale_content_with_a_mid_flight_edit() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let canonical_id = "/workspace/src/App.vue";
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'revision-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str =
        "<script setup lang=\"ts\">\nconst msg = 'revision-b-edited'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SOURCE_A.to_string(),
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

    let ide_path = verter_workspace::carrier_ide_provider_path(canonical_id, false);
    // Pause the coordinator's `open_file` call — `ide.code` is already
    // compiled from revision A by this point, and the record has not run yet.
    let (arrived, release) = provider.block_open_file(&ide_path);

    let tick = sync_file(&deps, canonical_id, uri.as_str());
    let edit = async {
        arrived.notified().await;
        let result = documents.did_change(&uri, 2, SOURCE_B);
        assert!(
            result.changed,
            "the interleaved edit must really commit revision B"
        );
        release.notify_one();
    };
    futures_util::future::join(tick, edit).await;

    assert_eq!(
        documents
            .get(&uri)
            .expect("document stays open")
            .source
            .as_ref(),
        SOURCE_B,
        "precondition: the live document is revision B"
    );

    if let Some(recorded) = documents.provider_surfaces().current_snapshot(&ide_path) {
        assert!(
            recorded.provider_content.contains("revision-b-edited"),
            "a CarrierIde surface recorded by this tick must not pair revision \
             A's TSX (compiled before the edit) with revision B's live source \
             — either it reflects B, or nothing should have been recorded, got: \
             {}",
            recorded.provider_content
        );
    }
}

/// The sibling of the test above, reaching the OTHER half of the same bug
/// class: a `did_change` landing between the COMPILE and the pin capture,
/// rather than between the pin capture and the provider sync.
///
/// The test above pauses inside `MockTypeProvider::block_open_file` — a point
/// that sits AFTER the pin capture under EITHER source ordering (the pin is a
/// few synchronous statements before the first `.await` in `sync_file`), so
/// it cannot tell "pin captured before the compile" apart from "pin captured
/// right after `get_ide` returns, still before the provider await": both
/// orderings reach `block_open_file` with the pin already set. It only proves
/// the check-to-record gap (a torn pair surviving the provider await) is
/// closed.
///
/// This test pauses at [`test_hooks::block_after_ide_compile`] — wired
/// immediately after the compile in `sync_file`, the EXACT source position
/// the pin capture sat at before this fix (see `eb08424fe`): compile, then
/// `is_jsx`, then the pin capture. Under the PRE-FIX ordering the pin capture
/// runs AFTER this pause point, so an edit landing during the pause is
/// observed BY the pin capture — the pin ends up `B` while `ide.code` was
/// already compiled from `A`. That pin then MATCHES the still-`B` live
/// identity at record time, so the OLD fenced record would proceed and pair
/// stale `A` content with `B`'s identity/source: a torn pair, not a refusal.
/// (Reverting the pin-capture statements below the hook call reproduces
/// exactly this and makes the final assertion fail — verified by hand while
/// authoring this test.)
///
/// Under the FIX (pin captured BEFORE the compile, unaffected by this pause):
/// the pin is already fixed at revision `A` by the time this pause runs, so
/// the interleaved edit can only ever be observed by the LATER live-identity
/// read at record time, never by the pin itself. Record time then sees pin
/// `A` vs live `B` — a mismatch — and refuses (fail closed): nothing is
/// recorded for this pass, and the edit's own tick records the coherent
/// `B`/`B` pair instead.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_direct_ide_sync_pin_is_captured_before_the_compile_not_after() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    // A canonical id UNIQUE across this file: the pause hook is a global,
    // canonical-id-keyed registry (see `test_hooks`), so reusing the common
    // "/workspace/src/App.vue" fixture literal here would let an unrelated,
    // concurrently-running test's `sync_file` call steal this test's
    // registered pause (or vice versa) — an observed flaky failure.
    let canonical_id = "/workspace/src/PinRaceDirectOpen.vue";
    let uri: Uri = "file:///workspace/src/PinRaceDirectOpen.vue"
        .parse()
        .expect("test uri");
    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'revision-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str =
        "<script setup lang=\"ts\">\nconst msg = 'revision-b-edited'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SOURCE_A.to_string(),
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

    let ide_path = verter_workspace::carrier_ide_provider_path(canonical_id, false);
    // Pause the tick right after the compile — the pre-fix pin-capture spot.
    let (arrived, release) = test_hooks::block_after_ide_compile(canonical_id);

    let tick = sync_file(&deps, canonical_id, uri.as_str());
    let edit = async {
        arrived.notified().await;
        let result = documents.did_change(&uri, 2, SOURCE_B);
        assert!(
            result.changed,
            "the interleaved edit must really commit revision B"
        );
        release.notify_one();
    };
    futures_util::future::join(tick, edit).await;

    assert_eq!(
        documents
            .get(&uri)
            .expect("document stays open")
            .source
            .as_ref(),
        SOURCE_B,
        "precondition: the live document is revision B"
    );

    // The pin was captured BEFORE the compile — before this pause, before the
    // edit — so it is anchored to revision A regardless of what happens
    // during this pause. At record time the live identity is B (the edit
    // already landed), which no longer matches the A-revision pin: the
    // fenced record MUST refuse outright. A recorded surface here — of ANY
    // content — would mean the pin drifted to observe the edit, exactly the
    // pre-fix defect.
    assert!(
        documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .is_none(),
        "a pin captured before the compile must make the record refuse when \
         an edit lands after that capture — a recorded surface here means \
         the pin was captured too late (or not honored), reproducing the \
         pre-fix torn-pairing defect"
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

/// The SAME compile-to-identity race as
/// `coordinator_direct_ide_sync_pin_is_captured_before_the_compile_not_after`,
/// reached through the OTHER `sync_file` arm that records a `CarrierIde`
/// surface: `preserve_open_unresolved_carrier` (owner-None over a ready
/// snapshot). `sync_file` captures ONE pin near its top and threads it
/// through to whichever arm ends up recording — this test proves that thread-
/// through actually reaches the unresolved-preserve arm's record call, not
/// just the owner-resolved `DirectOpen` arm the sibling test covers.
///
/// Same discrimination method: pausing at
/// [`test_hooks::block_after_ide_compile`] (the pre-fix pin-capture spot) and
/// landing an edit there reproduces the pre-fix torn pair if the pin capture
/// is moved back below it (verified by hand while authoring this test, same
/// as the sibling). Against the fix, the already-earlier pin stays anchored
/// to revision A, so the mismatched live identity (B) at record time makes
/// `preserve_open_unresolved_carrier`'s fenced record refuse outright.
#[tokio::test(flavor = "multi_thread")]
async fn coordinator_open_unresolved_preserve_pin_is_captured_before_the_compile_not_after() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    // A canonical id UNIQUE across this file — see the sibling test's comment
    // (`coordinator_direct_ide_sync_pin_is_captured_before_the_compile_not_after`)
    // for why the shared "/workspace/src/App.vue" fixture literal is unsafe here.
    let canonical_id = "/workspace/src/PinRaceUnresolvedPreserve.vue";
    let uri: Uri = "file:///workspace/src/PinRaceUnresolvedPreserve.vue"
        .parse()
        .expect("test uri");
    const SOURCE_A: &str = "<script setup lang=\"ts\">\nconst msg = 'revision-a'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    const SOURCE_B: &str =
        "<script setup lang=\"ts\">\nconst msg = 'revision-b-edited'\n</script>\n\
                             <template><div>{{ msg }}</div></template>\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: SOURCE_A.to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    // Ready snapshot rooted ELSEWHERE: owner resolution returns None for the
    // open file, driving the preserve-open-unresolved arm (never the
    // owner-resolved `DirectOpen` arm the sibling test exercises).
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

    // Pause the tick right after the compile — the pre-fix pin-capture spot,
    // shared by every `sync_file` arm (including this unresolved-preserve one).
    let (arrived, release) = test_hooks::block_after_ide_compile(canonical_id);

    let tick = sync_file(&deps, canonical_id, uri.as_str());
    let edit = async {
        arrived.notified().await;
        let result = documents.did_change(&uri, 2, SOURCE_B);
        assert!(
            result.changed,
            "the interleaved edit must really commit revision B"
        );
        release.notify_one();
    };
    futures_util::future::join(tick, edit).await;

    assert_eq!(
        documents
            .get(&uri)
            .expect("document stays open")
            .source
            .as_ref(),
        SOURCE_B,
        "precondition: the live document is revision B"
    );

    let state = provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone())
        .expect("the open unresolved carrier must still commit provider state");
    assert!(
        state.is_unresolved(),
        "owner-None over a ready snapshot must commit an Unresolved binding"
    );
    let ide_path = state
        .ide_path
        .clone()
        .expect("the preserve must keep a live IDE path");

    // Same fail-closed requirement as the sibling test: the pin was captured
    // before the compile and before the edit, so it stays anchored to A while
    // the live identity moves to B — the fenced record inside
    // `preserve_open_unresolved_carrier` must refuse outright.
    assert!(
        documents
            .provider_surfaces()
            .current_snapshot(&ide_path)
            .is_none(),
        "a pin captured before the compile must make the unresolved-preserve \
         record refuse when an edit lands after that capture — a recorded \
         surface here means the pin either was not threaded through to this \
         arm or was captured too late, reproducing the pre-fix torn-pairing \
         defect"
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

    // Deterministic completion signal for "the background provider pull
    // reached `get_diagnostics`" — a single `yield_now()` only proves the
    // publish task got ONE scheduler turn, which is not the same claim
    // under multi-threaded contention (the spawned task may land on a
    // busy worker and need more than one turn to reach the provider
    // call). `set_on_query` fires synchronously from inside
    // `MockTypeProvider::get_diagnostics`, after the call is recorded and
    // before the hang takes effect, so awaiting this channel (bounded by
    // a timeout, never a sleep/retry loop) is the real event instead of a
    // scheduling guess.
    let (query_tx, query_rx) = tokio::sync::oneshot::channel();
    provider.set_on_query(
        &ide_path,
        Box::new(move || {
            let _ = query_tx.send(());
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

    tokio::time::timeout(Duration::from_secs(1), query_rx)
        .await
        .expect("the provider pull must still start in the background")
        .expect("the query callback channel must not drop before firing");
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
    handle.signal(
        canonical_id.clone(),
        uri.as_str().to_string(),
        tokio::time::Instant::now(),
    );

    handle
        .await_until(
            || {
                cached_verter_diags.get(uri.as_str()).is_some_and(|entry| {
                    entry.2.iter().any(|d| {
                        matches!(
                            d.code.as_ref(),
                            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
                        )
                    })
                })
            },
            || {
                panic!(
                    "the provider-less coordinator must publish Verter-owned diagnostics \
                     (verter/no-unused-props) for a signaled open file; cache: {:?}",
                    cached_verter_diags
                        .get(uri.as_str())
                        .map(|entry| entry.2.clone())
                )
            },
        )
        .await;
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
    let handle = spawn_sync_coordinator(deps);
    documents.schedule_semantic_analysis(&uri);

    handle
        .await_until(
            || {
                cached_verter_diags.get(uri.as_str()).is_some_and(|entry| {
                    entry.2.iter().any(|diagnostic| {
                        matches!(
                            diagnostic.code.as_ref(),
                            Some(NumberOrString::String(code)) if code == "verter/no-unused-props"
                        )
                    })
                })
            },
            || panic!("semantic completion must trigger a diagnostics-only publish"),
        )
        .await;
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

const USABLE_SVELTE: &str = r#"{"name":"svelte","version":"5.56.10","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"},"./elements":{"types":"./elements.d.ts"}}}"#;
const UNUSABLE_SVELTE: &str = r#"{"name":"svelte","version":"5.56.10","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"}}}"#;

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

// ---------------------------------------------------------------------------
// Reproduction: https://github.com/pikax/verter/issues/96
//
// `coordinator_loop` stamps the start of a signal's debounce quiet window with
// `Instant::now()` taken at the moment it DRAINS the inbox (the
// `wake = wake_rx.recv()` arm of the select), not at the moment
// `SyncCoordinatorHandle::signal` deposited that signal. Every millisecond a
// signal spends waiting in the inbox — because `did_change` handlers are
// serialized on `did_change_mutex` and each one commits its document before it
// signals, and because the coordinator's own `sync_file(..).await` is inline in
// the loop and blocks the drain — is therefore charged to the user as fresh
// quiet time, and the full 300ms window restarts from zero no matter how long
// the file has actually been quiet.
//
// These tests are deliberately structured so that NO assertion depends on how
// fast this machine is:
//
//   * The stall is produced by `std::thread::sleep` on the single-threaded test
//     runtime. That blocks the one worker thread, so the spawned coordinator
//     task provably cannot be polled during it — the drain is late by
//     construction, not by scheduling luck.
//   * The failing assertion is "a file quiet for 4x the debounce interval was
//     NOT dispatched on the coordinator's first look". On `main` this cannot
//     pass: the drain schedules `sleep_until(drain_instant + 300ms)`, which is
//     300ms in the future. Machine load can only push that later.
//   * The passing direction is equally robust: once the window is measured from
//     signal receipt, the file's window has demonstrably elapsed (real time
//     only ever grows), so it dispatches on the first look.

/// Build the minimum deps for driving a real `coordinator_loop` with no
/// provider I/O at all. `project_sync: None` makes `sync_file` return at its
/// first line — so nothing calls `tokio::task::block_in_place`, which would
/// panic on the current-thread runtime these tests need — and
/// `type_provider: None` keeps the publish half out of the picture.
///
/// The observable is `needs_provider_sync`: the coordinator's deadline arm
/// removes the canonical id from that set exactly when the debounce fires for
/// it, so membership is a precise "has this file been dispatched yet" probe.
fn debounce_probe_deps() -> (SyncCoordinatorDeps, Arc<DashSet<String>>) {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let needs_provider_sync = Arc::new(DashSet::new());
    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: None,
        needs_provider_sync: Arc::clone(&needs_provider_sync),
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
    (deps, needs_provider_sync)
}

/// Wait for one coordinator loop tick. The caller must have already made
/// a select arm ready (a wake in the inbox, or a timer that has already
/// been advanced to). Awaiting a tick that is not going to fire would
/// auto-advance the paused clock to the next quiet-window deadline.
async fn pump_tick(handle: &crate::sync_coordinator::SyncCoordinatorHandle) {
    handle.await_loop_tick().await;
}

/// REPRODUCES https://github.com/pikax/verter/issues/96.
///
/// EXPECTED TO FAIL on `main`. It passes once the quiet window is measured
/// from the instant `signal()` deposited the signal instead of the instant the
/// coordinator got around to draining it.
///
/// `App.vue` is signalled and then sits in the inbox for 4x the debounce
/// interval while the runtime thread is blocked — exactly the shape of the
/// `did_change` backlog in the issue, where 50 serialized handlers push the
/// coordinator's first look seconds past the last keystroke. By the time the
/// coordinator looks, the file has been quiet for far longer than the quiet
/// window, so the sync is already overdue and must fire on that first look.
#[tokio::test]
async fn debounce_window_restarts_at_inbox_drain_instead_of_signal_receipt() {
    let (deps, needs_provider_sync) = debounce_probe_deps();
    let quiet_id = "/workspace/src/App.vue".to_string();
    let just_typed_id = "/workspace/src/Sidebar.vue".to_string();
    needs_provider_sync.insert(quiet_id.clone());
    needs_provider_sync.insert(just_typed_id.clone());

    let handle = spawn_sync_coordinator(deps);

    // The keystroke for App.vue lands here. Nothing has awaited yet, so the
    // spawned coordinator task has not been polled and the signal is sitting
    // in the inbox, unseen.
    let signalled_at = Instant::now();
    handle.signal(
        quiet_id.clone(),
        "file:///workspace/src/App.vue".to_string(),
        signalled_at,
    );

    // The issue #96 shape: `std::thread::sleep` on the current-thread
    // runtime blocks the ONE worker, so the coordinator cannot drain
    // while time passes. `tokio::time::advance` cannot model this — it
    // yields and the coordinator runs. This sleep is test setup, not a
    // correctness assertion.
    let backlog = Duration::from_millis(DEBOUNCE_MS * 4);
    std::thread::sleep(backlog);

    // A second file is signalled immediately before the drain. It has NOT been
    // quiet and must still be debounced — it is what keeps the fix honest.
    handle.signal(
        just_typed_id.clone(),
        "file:///workspace/src/Sidebar.vue".to_string(),
        Instant::now(),
    );

    let quiet = Arc::clone(&needs_provider_sync);
    let quiet_id_for_wait = quiet_id.clone();
    handle
        .await_until(|| !quiet.contains(&quiet_id_for_wait), || {})
        .await;

    assert!(
        needs_provider_sync.contains(&just_typed_id),
        "{just_typed_id} was signalled immediately before the drain and has not \
         been quiet for {DEBOUNCE_MS}ms — measuring the window from signal \
         receipt must not turn the debounce into an unconditional immediate \
         dispatch"
    );
}

/// Positive control for the fix to #96: the debounce must still debounce.
/// Passes on `main` and must keep passing after the fix — it is what fails if
/// the fix degenerates into "dispatch on every drain".
#[tokio::test(start_paused = true)]
async fn debounce_still_waits_for_quiet_before_dispatching() {
    let (deps, needs_provider_sync) = debounce_probe_deps();
    let canonical_id = "/workspace/src/App.vue".to_string();
    needs_provider_sync.insert(canonical_id.clone());

    let handle = spawn_sync_coordinator(deps);
    handle.signal(
        canonical_id.clone(),
        "file:///workspace/src/App.vue".to_string(),
        Instant::now(),
    );

    pump_tick(&handle).await;
    assert!(
        needs_provider_sync.contains(&canonical_id),
        "a file quiet for 0ms must not have synced yet — the \
         {DEBOUNCE_MS}ms debounce is what stops rapid typing from flooding the \
         type provider"
    );

    tokio::time::advance(Duration::from_millis(DEBOUNCE_MS)).await;
    pump_tick(&handle).await;
    assert!(
        !needs_provider_sync.contains(&canonical_id),
        "a file quiet for longer than {DEBOUNCE_MS}ms must have synced"
    );
}

/// The quiet-window policy is semantic time: prove the boundary under a paused
/// Tokio clock. Nothing dispatches at `window - ε`; a later edit resets the
/// window; advancing to the new boundary dispatches exactly once.
///
/// What this pins is that the COORDINATOR's window is the shared
/// [`crate::edit_quiet_window::EDIT_QUIET_WINDOW`] policy value, not that the
/// policy is 300ms. Both sides read the same constant, so changing the policy
/// moves the test with it — deliberately: the number is policy, the identity
/// is the invariant ("a second 300 ms constant is a bug"). The discriminating
/// mutation is therefore in `coordinator_loop`'s `debounce` binding, not in
/// the constant: replacing it with any other duration turns the `window - ε`
/// assertion (shorter) or the final dispatch assertion (longer) red. Verified
/// by planting `Duration::from_millis(150)` there.
#[tokio::test(start_paused = true)]
async fn quiet_window_boundary_under_paused_time() {
    use crate::edit_quiet_window::EDIT_QUIET_WINDOW;

    let (deps, needs_provider_sync) = debounce_probe_deps();
    let canonical_id = "/workspace/src/App.vue".to_string();
    needs_provider_sync.insert(canonical_id.clone());

    let handle = spawn_sync_coordinator(deps);
    handle.signal(
        canonical_id.clone(),
        "file:///workspace/src/App.vue".to_string(),
        Instant::now(),
    );
    pump_tick(&handle).await;

    let epsilon = Duration::from_millis(1);
    tokio::time::advance(EDIT_QUIET_WINDOW - epsilon).await;
    tokio::task::yield_now().await;
    assert!(
        needs_provider_sync.contains(&canonical_id),
        "nothing must dispatch at quiet_window - ε"
    );

    // A later edit resets the window: advancing another (window - ε) from the
    // first stamp would have crossed the original boundary, but must not fire.
    handle.signal(
        canonical_id.clone(),
        "file:///workspace/src/App.vue".to_string(),
        Instant::now(),
    );
    pump_tick(&handle).await;
    tokio::time::advance(EDIT_QUIET_WINDOW - epsilon).await;
    tokio::task::yield_now().await;
    assert!(
        needs_provider_sync.contains(&canonical_id),
        "a later edit must reset the quiet window so the original boundary does not fire"
    );

    tokio::time::advance(epsilon).await;
    pump_tick(&handle).await;
    assert!(
        !needs_provider_sync.contains(&canonical_id),
        "advancing to the reset quiet-window boundary must dispatch exactly once"
    );
}

// ---------------------------------------------------------------------------
// The other half of the fix for https://github.com/pikax/verter/issues/96.
//
// Measuring the quiet window from RECEIPT is what makes an overdue sync fire
// promptly, but on its own it converts the stall into a flood: with 50
// `did_change` handlers serialized behind the global commit mutex, each one
// deposits a signal whose receipt instant is ALREADY older than the debounce,
// so the coordinator would dispatch once per handler — one provider sync per
// keystroke, exactly what `sync_coordinator`'s module documentation says the
// debounce exists to prevent.
//
// A change is therefore not merely "signalled at time T": it is IN FLIGHT from
// the moment its handler is entered until that handler is done. A document with
// a change in flight is not quiet, whatever its last receipt says.

/// The coordinator PARKS on a gated canonical id — it arms no timer for it at
/// all — so the ticket release is the only thing that can wake it. A handler
/// that returns WITHOUT signalling (a virtual document, a style-only edit) must
/// therefore still wake it, or an already-overdue sync waits forever.
#[tokio::test(start_paused = true)]
async fn releasing_a_ticket_without_signalling_wakes_the_gated_coordinator() {
    let (deps, needs_provider_sync) = debounce_probe_deps();
    let canonical_id = "/workspace/src/App.vue".to_string();
    needs_provider_sync.insert(canonical_id.clone());

    let handle = spawn_sync_coordinator(deps);
    let signalled_at = Instant::now();
    handle.signal(
        canonical_id.clone(),
        "file:///workspace/src/App.vue".to_string(),
        signalled_at,
    );

    tokio::time::advance(Duration::from_millis(DEBOUNCE_MS * 4)).await;
    assert_eq!(
        Instant::now(),
        signalled_at + Duration::from_millis(DEBOUNCE_MS * 4)
    );

    // A later change arrives and takes a ticket, then its handler returns
    // without ever signalling.
    let ticket = handle.change_received(canonical_id.clone());
    pump_tick(&handle).await;
    assert!(
        needs_provider_sync.contains(&canonical_id),
        "a document with a change in flight is not quiet, however overdue its \
         last receipt is — dispatching here is the per-keystroke sync flood"
    );

    drop(ticket);
    pump_tick(&handle).await;
    assert!(
        !needs_provider_sync.contains(&canonical_id),
        "releasing the last in-flight ticket must wake the coordinator: it holds \
         no armed timer for a gated canonical id, so without that wake the \
         overdue sync never fires at all"
    );
}

/// Count the provider file-sync calls a dispatched `sync_file` delivered for
/// the carrier `canonical_id`. Derived from the mock's recorded call log, never
/// assumed.
///
/// Matched by prefix because a carrier's provider companions live in the
/// source's own namespace (`{canonical}.tsx`, `{carrier}.ts`), so the prefix
/// names exactly this carrier's surfaces and nothing else.
fn provider_syncs_for(provider: &MockTypeProvider, canonical_id: &str) -> usize {
    provider
        .calls()
        .iter()
        .filter(|call| match call {
            MockCall::OpenFile { path, .. }
            | MockCall::OpenFileBackground { path, .. }
            | MockCall::LoadFile { path, .. }
            | MockCall::UpdateFile { path, .. } => path.starts_with(canonical_id),
            _ => false,
        })
        .count()
}

/// PROPERTY (b) for https://github.com/pikax/verter/issues/96: a backlog of
/// received changes still collapses to ONE provider sync.
///
/// This is what fails if the receipt-time window ships without the in-flight
/// gate. Each handler in a backlog deposits a signal whose receipt is already
/// older than the debounce, so an ungated coordinator dispatches on its very
/// next look — once per handler, for the whole backlog, each dispatch
/// compiling and pushing a fresh TSX buffer.
///
/// No assertion is a wall-clock threshold, and none is a hardcoded count:
///
///   * The per-dispatch cost is MEASURED first, from a single change, as a
///     pre/post delta on the mock's recorded call log.
///   * The burst assertion is `== that measured unit`. A storm shows up as
///     dispatches DURING the burst, which are already counted before the final
///     wait begins — so waiting longer can only make a storm more visible,
///     never less.
///   * `Sidebar.vue` is an UNGATED document with an equally overdue receipt. It
///     must sync during the burst, which is what proves the coordinator was
///     awake and dispatching the whole time — so `App.vue` staying at zero is
///     the gate working, not the coordinator merely never being scheduled.
#[tokio::test(flavor = "multi_thread")]
async fn a_backlog_of_received_changes_still_collapses_to_one_provider_sync() {
    let (documents, _states, provider, canonical_id, _ide_path, deps) =
        make_carrier_diagnostics_fixture().await;
    let uri: Uri = "file:///workspace/src/App.vue".parse().expect("test uri");
    let revision = |marker: &str| {
        format!(
            "<script setup lang=\"ts\">\nconst msg = '{marker}'\n</script>\n\
             <template><div>{{{{ msg }}}}</div></template>\n"
        )
    };

    // The ungated liveness control: a second open carrier under the same
    // workspace root, so it resolves the same owner and takes the same
    // `sync_file` path.
    let control_uri: Uri = "file:///workspace/src/Sidebar.vue"
        .parse()
        .expect("control uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: control_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: revision("sidebar"),
    });
    let control_id = documents
        .get_canonical_id(&control_uri)
        .expect("the control document must be open");

    let needs_provider_sync = Arc::clone(&deps.needs_provider_sync);
    let handle = spawn_sync_coordinator(deps);

    // Taken here so the control receipt ages across calibration. The
    // calibration wait plus one quiet window makes it overdue before the burst.
    let control_received_at = Instant::now();

    // ---- Calibrate: what does ONE dispatched sync of this document cost?
    let baseline = provider_syncs_for(&provider, &canonical_id);
    let published_before_calibration = handle.diags_published_count();
    needs_provider_sync.insert(canonical_id.clone());
    {
        let change = handle.change_received(canonical_id.clone());
        let _ = documents.did_change(&uri, 2, &revision("v2"));
        change.signal(uri.as_str().to_string());
    }
    // Ticket Drop already woke the coordinator. The production quiet-window
    // timer fires on the real clock; settlement is the provider-sync receipt.
    let synced = wait_for_provider_syncs(&provider, &canonical_id, baseline + 1).await;
    assert!(
        synced > baseline,
        "calibration must observe a real dispatch, otherwise the burst assertion \
         below compares zero against zero and cannot fail"
    );

    // A dispatch is not finished at its provider-sync receipt: the tick
    // SPAWNS the diagnostics publish after `sync_file` returns, and that
    // publish makes its own provider calls. Settling on `diag_tasks_live
    // == 0` alone is vacuous here (the task has not been spawned yet), and
    // a fixed sleep is exactly the mechanism these fences replace.
    // The monotonic publication count makes the tick's completion exact, so
    // `unit` is the FULL per-dispatch cost and the burst baseline below is
    // taken after every call this dispatch will ever make.
    handle
        .await_until(
            || handle.diags_published_count() > published_before_calibration,
            || {},
        )
        .await;
    handle
        .await_until(|| handle.diag_tasks_live() == 0, || {})
        .await;
    let unit = provider_syncs_for(&provider, &canonical_id) - baseline;

    // ---- The backlog. Every handler is entered (ticket taken) before any of
    // them finishes, which is the shape a typing burst produces: the tickets
    // are the changes the server has RECEIVED and not yet processed.
    const BURST: usize = 12;
    let burst_baseline = provider_syncs_for(&provider, &canonical_id);
    let control_baseline = provider_syncs_for(&provider, &control_id);
    let mut tickets: Vec<ChangeInFlight> = (0..BURST)
        .map(|_| handle.change_received(canonical_id.clone()))
        .collect();

    // Each handler commits and signals in turn. Tickets stay live, so App.vue
    // is gated; the coordinator drains each signal without dispatching it.
    //
    // The receipts are stamped ALREADY OVERDUE — the shape the issue
    // describes, where serialized handlers push the coordinator's first look
    // seconds past the last keystroke. Stamping them "now" would leave the
    // DEBOUNCE, not the gate, deferring App for the whole burst window, and
    // the zero below would then hold with the gate ripped out entirely. It
    // did: disabling `quiescent` outright still passed 34 runs in 35.
    let backlog_received_at = control_received_at;
    for (index, _ticket) in tickets.iter().enumerate() {
        let version = 3 + index as i32;
        needs_provider_sync.insert(canonical_id.clone());
        let _ = documents.did_change(&uri, version, &revision(&format!("v{version}")));
        handle.signal(
            canonical_id.clone(),
            uri.as_str().to_string(),
            backlog_received_at,
        );
    }

    // FENCE, not a hope: the control's dispatch only proves App did not
    // dispatch if the coordinator had already TAKEN App's signals. Wait for
    // the inbox to drain them, and only then signal the control — so the
    // tick that dispatches the control is a tick that looked at App and
    // declined. Signalling the control first would let it dispatch while
    // App's signals were still sitting in the inbox, and the zero below
    // would say nothing about the gate.
    handle
        .await_until(|| !handle.inbox_contains(&canonical_id), || {})
        .await;

    // Read the dispatch-arm count BEFORE the control is signalled, so the arm
    // that serves the control is necessarily past it — and if such an arm has
    // already run, the predicate below is already true and nothing waits.
    let dispatches_before_control = handle.dispatch_ticks();
    needs_provider_sync.insert(control_id.clone());
    handle.signal(
        control_id.clone(),
        control_uri.as_str().to_string(),
        control_received_at,
    );

    // The control's provider-sync receipt is the reproduced liveness fence: it
    // proves the coordinator processed an overdue, ungated document while App
    // remained held. The dispatch-tick half is retained as conservative extra
    // progress evidence, but is not independently established as necessary for
    // this discriminator. The reproduced load-bearing setup is the already-
    // overdue App receipt above; stamping the burst at ticket time lets the
    // debounce, rather than the in-flight gate, defer App throughout the sample.
    handle
        .await_until(
            || {
                provider_syncs_for(&provider, &control_id) > control_baseline
                    && handle.dispatch_ticks() > dispatches_before_control
            },
            || {},
        )
        .await;

    let during_burst = provider_syncs_for(&provider, &canonical_id) - burst_baseline;
    assert_eq!(
        during_burst, 0,
        "no sync may dispatch for a document whose changes are still in flight. \
         {during_burst} dispatched: measuring the quiet window from receipt \
         without the in-flight gate makes every backlogged handler's already-\
         expired receipt fire its own sync — one provider sync per keystroke"
    );
    let control_during_burst = provider_syncs_for(&provider, &control_id) - control_baseline;
    assert!(
        control_during_burst > 0,
        "the ungated control must sync while the backlog is in flight — without \
         it, App.vue's zero above would prove nothing about the gate"
    );

    // ---- The backlog drains. Exactly one sync, for the newest revision.
    let published_before_drain = handle.diags_published_count();
    tickets.clear();
    let _ = wait_for_provider_syncs(&provider, &canonical_id, burst_baseline + unit).await;
    // Same completion fence as the calibration: count the drain tick's full
    // cost, including the provider calls its diagnostics publish makes, so a
    // storm cannot hide behind a half-observed tick.
    handle
        .await_until(
            || handle.diags_published_count() > published_before_drain,
            || {},
        )
        .await;
    handle
        .await_until(|| handle.diag_tasks_live() == 0, || {})
        .await;
    let after_burst = provider_syncs_for(&provider, &canonical_id) - burst_baseline;
    assert_eq!(
        after_burst, unit,
        "a backlog of {BURST} received changes must cost exactly what ONE change \
         costs ({unit} provider file-sync call(s)), not {BURST}x it"
    );
    assert!(
        documents
            .host()
            .get_ide(&canonical_id, &documents.tsx_profile.read())
            .is_some_and(|ide| ide.code.contains(&format!("'v{}'", 2 + BURST))),
        "the one sync that does run must carry the NEWEST revision — coalescing \
         that publishes a superseded buffer is worse than the flood it replaced"
    );
}

/// Wait until `provider` has recorded at least `want` file-sync calls for
/// `canonical_id`. The mock's call-recorded Notify is the receipt; the
/// timeout is only a hang watchdog.
async fn wait_for_provider_syncs(
    provider: &MockTypeProvider,
    canonical_id: &str,
    want: usize,
) -> usize {
    tokio::time::timeout(
        Duration::from_secs(20),
        provider.wait_until_calls(|calls| {
            calls
                .iter()
                .filter(|call| match call {
                    MockCall::OpenFile { path, .. }
                    | MockCall::OpenFileBackground { path, .. }
                    | MockCall::LoadFile { path, .. }
                    | MockCall::UpdateFile { path, .. } => path.starts_with(canonical_id),
                    _ => false,
                })
                .count()
                >= want
        }),
    )
    .await
    .expect("provider file-sync calls never reached the expected count");
    provider_syncs_for(provider, canonical_id)
}

/// LSP notification handlers are dispatched concurrently, so an OLDER change
/// can deposit its signal after a newer one has already deposited its own. The
/// quiet window must take the LATEST receipt, never simply the last deposit —
/// otherwise a straggler walks the window backwards into the past and fires a
/// sync while the user is still typing, which is the flood the debounce exists
/// to prevent.
///
/// Both coalescing points are covered: the inbox (`signal`, when the two
/// deposits land between the same pair of drains) and the coordinator's pending
/// map (the drain arm, when they land in different drains).
#[tokio::test(start_paused = true)]
async fn a_late_arriving_older_receipt_does_not_walk_the_quiet_window_backwards() {
    let (deps, needs_provider_sync) = debounce_probe_deps();
    let same_drain = "/workspace/src/SameDrain.vue".to_string();
    let later_drain = "/workspace/src/LaterDrain.vue".to_string();
    needs_provider_sync.insert(same_drain.clone());
    needs_provider_sync.insert(later_drain.clone());

    let stale = Instant::now();
    tokio::time::advance(Duration::from_millis(DEBOUNCE_MS * 4)).await;
    assert_eq!(
        Instant::now(),
        stale + Duration::from_millis(DEBOUNCE_MS * 4)
    );

    let handle = spawn_sync_coordinator(deps);

    // Case A — both deposits land before the coordinator's first drain, so the
    // INBOX coalesces them.
    handle.signal(
        same_drain.clone(),
        "file:///workspace/src/SameDrain.vue".to_string(),
        Instant::now(),
    );
    handle.signal(
        same_drain.clone(),
        "file:///workspace/src/SameDrain.vue".to_string(),
        stale,
    );

    // Case B — the newer deposit is drained first, so the coordinator's PENDING
    // map is what has to reject the straggler.
    handle.signal(
        later_drain.clone(),
        "file:///workspace/src/LaterDrain.vue".to_string(),
        Instant::now(),
    );
    pump_tick(&handle).await;
    handle.signal(
        later_drain.clone(),
        "file:///workspace/src/LaterDrain.vue".to_string(),
        stale,
    );
    pump_tick(&handle).await;

    assert!(
        needs_provider_sync.contains(&same_drain),
        "the inbox must keep the LATEST receipt: a straggler carrying a receipt \
         {}x the {DEBOUNCE_MS}ms window old must not restart the window in the \
         past and dispatch a sync the user's newest keystroke has not earned",
        (DEBOUNCE_MS * 4) / DEBOUNCE_MS
    );
    assert!(
        needs_provider_sync.contains(&later_drain),
        "the coordinator's pending map must keep the LATEST receipt for the same \
         reason — the straggler arrived in a later drain, but it is still older"
    );
}

/// A carrier whose open-time compile FAILED has no provider projection, and a
/// document with no projection fails closed on every capture
/// (`capture_provider_request_surface` returns `None`) until a repair path
/// compiles one.
///
/// The document commit deliberately does not compile — doing so per keystroke is
/// https://github.com/pikax/verter/issues/96, and a compile that FAILS installs
/// no projection, so "compile until one exists" never terminates on a file being
/// typed. The interactive repair heals a projection-less carrier on the next
/// provider-backed request (attempt-bounded), but only when a request arrives;
/// the debounced coordinator is the request-INDEPENDENT recovery, compiling the
/// IDE surface once per quiet window.
///
/// Drives the real `sync_file`, so it fails if the install is not wired into it —
/// not merely if the method is wrong.
#[tokio::test(flavor = "multi_thread")]
async fn the_coordinator_installs_a_projection_the_failed_open_compile_never_built() {
    let (documents, _states, _provider, _canonical_id, _ide_path, deps) =
        make_carrier_diagnostics_fixture().await;
    let uri: Uri = "file:///workspace/src/Recovered.vue".parse().expect("uri");

    // Open malformed: no IDE surface, so no projection.
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: "<script setup lang=\"ts\">\nconst broken = (((\n".to_string(),
    });
    let recovered_id = documents
        .get_canonical_id(&uri)
        .expect("the open document has a canonical id");
    assert!(
        documents.get_projection(&uri).is_none(),
        "precondition: the malformed open must leave the carrier projection-less, or \
         this test exercises nothing"
    );

    // The user fixes it. The commit stores the text and compiles nothing, so the
    // document is STILL projection-less — the state the coordinator must recover.
    let _ = documents.did_change(
        &uri,
        2,
        "<script setup lang=\"ts\">\nconst fixed = 1\n</script>\n\
         <template><div>{{ fixed }}</div></template>\n",
    );
    assert!(
        documents.get_projection(&uri).is_none(),
        "the commit must not have compiled — if it did, this asserts nothing about \
         the coordinator and the per-keystroke compile is back"
    );

    let needs_provider_sync = Arc::clone(&deps.needs_provider_sync);
    await_projection_via_coordinator(
        deps,
        &needs_provider_sync,
        &documents,
        &recovered_id,
        &uri,
        "failed open compile",
    )
    .await;
}

/// Recovery must not depend on the provider-sync arm being reachable.
///
/// `sync_file` returns early on a provider-less route (`project_sync: None`) and
/// again before a resolver snapshot is published — the ordinary state of a
/// workspace that is still settling, and of every editor-owned-tsserver /
/// verter-only session. A carrier whose open-time compile failed has no provider
/// projection and the commit never compiles one. The interactive repair heals it
/// only when a provider-backed request arrives (and a provider-less route has no
/// provider to ask), so if the debounced tick also skips it the document is
/// stranded with NO IDE features at all — worse than the latency bug this
/// change is about.
///
/// Both early-return paths are covered here because they are different gates:
/// the previous fixture always supplied a provider AND a published VFS, so it
/// could not reach either.
#[tokio::test(flavor = "multi_thread")]
async fn a_projectionless_carrier_recovers_without_a_provider_or_a_snapshot() {
    let fixed = "<script setup lang=\"ts\">\nconst fixed = 1\n</script>\n\
                 <template><div>{{ fixed }}</div></template>\n";

    for (case, deps_for) in [
        (
            "no provider (project_sync: None)",
            0usize, // provider-less: deps built below with project_sync None
        ),
        ("no published resolver snapshot", 1usize),
    ] {
        let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
            HostConfig::default(),
        ))));
        let uri: Uri = "file:///workspace/src/Stranded.vue".parse().expect("uri");
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<script setup lang=\"ts\">\nconst broken = (((\n".to_string(),
        });
        let canonical_id = documents
            .get_canonical_id(&uri)
            .expect("the open document has a canonical id");
        assert!(
            documents.get_projection(&uri).is_none(),
            "{case} precondition: the malformed open must leave the carrier \
             projection-less"
        );

        // The user fixes the file. The commit stores text and compiles nothing.
        let _ = documents.did_change(&uri, 2, fixed);
        assert!(
            documents.get_projection(&uri).is_none(),
            "{case}: the commit must not compile — if it did, the per-keystroke \
             compile is back and this asserts nothing about recovery"
        );

        let provider = Arc::new(MockTypeProvider::new());
        let mut deps = SyncCoordinatorDeps {
            documents: Arc::clone(&documents),
            project_sync: None,
            needs_provider_sync: Arc::new(DashSet::new()),
            pending_snapshot_provider_sync: Arc::new(DashSet::new()),
            client: make_test_client(),
            type_provider: None,
            cached_verter_diags: Arc::new(DashMap::new()),
            position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
            provider_sync_states: Arc::new(DashMap::new()),
            // No published resolver snapshot for EITHER case; the second case
            // additionally has a provider, so it reaches the snapshot gate.
            vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
            type_provider_kind: crate::TypeProviderKind::Tsgo,
            carrier_publish_coordinator: None,
            carrier_transaction_coordinator: std::sync::Arc::new(
                crate::external_ts::CarrierTransactionCoordinator::new(),
            ),
        };
        if deps_for == 1 {
            deps.project_sync = Some(ProjectSync::new(
                provider.clone(),
                ProjectSyncMode::FullProject,
            ));
        }

        let needs_provider_sync = Arc::clone(&deps.needs_provider_sync);
        await_projection_via_coordinator(
            deps,
            &needs_provider_sync,
            &documents,
            &canonical_id,
            &uri,
            case,
        )
        .await;
    }
}

/// Drive the REAL coordinator for one settled revision and wait for the
/// document's provider projection to appear.
///
/// The refresh belongs to the tick, not to `sync_file`, so a recovery test has
/// to go through the tick to exercise it — which is also the wiring that would
/// break if a future change stopped arming it.
async fn await_projection_via_coordinator(
    deps: SyncCoordinatorDeps,
    needs_provider_sync: &Arc<DashSet<String>>,
    documents: &Arc<DocumentRegistry>,
    canonical_id: &str,
    uri: &Uri,
    case: &str,
) {
    let handle = spawn_sync_coordinator(deps);
    needs_provider_sync.insert(canonical_id.to_string());
    handle.signal(
        canonical_id.to_string(),
        uri.as_str().to_string(),
        Instant::now(),
    );
    handle
        .await_until(
            || documents.get_projection(uri).is_some(),
            || {
                panic!(
                    "{case}: the debounced tick must install the projection the failed open \
                     never built; without it the document fails closed downstream forever"
                )
            },
        )
        .await;
}

/// Build a provider-less coordinator (`project_sync: None`, `type_provider:
/// None`) — the shipping default: `--type-provider=editor-tsserver` installs no
/// LOCAL provider (`editor_tsserver_topology` returns `provider: None`), and
/// `--type-provider=off` is the same code path.
fn make_provider_less_deps(documents: &Arc<DocumentRegistry>) -> SyncCoordinatorDeps {
    SyncCoordinatorDeps {
        documents: Arc::clone(documents),
        project_sync: None,
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: None,
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(parking_lot::RwLock::new(None)),
        type_provider_kind: crate::TypeProviderKind::EditorTsserver,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    }
}

/// A Vue SFC whose template is either well-formed or carries a close tag that
/// closed no open element — a template parse error Verter reports on its OWN,
/// with no type provider involved.
fn vue_carrier_source(broken: bool) -> String {
    let rows: String = (0..8)
        .map(|i| format!("    <div>{{{{ {i} }}}}</div>\n"))
        .collect();
    let bad = if broken {
        "    <div><span></div>\n"
    } else {
        ""
    };
    format!(
        "<script setup lang=\"ts\">\nconst a = 1\n</script>\n\n\
         <template>\n  <section>\n{rows}{bad}  </section>\n</template>\n"
    )
}

/// A Svelte component whose markup is either well-formed or carries a close tag
/// that closed no open element (Verter's `element_invalid_closing_tag`, the
/// Svelte analogue of Vue's "Invalid end tag.").
///
/// Svelte is first-class on this path: the debounced refresh is carrier-
/// agnostic, so it must recover Svelte exactly as it recovers Vue.
fn svelte_carrier_source(broken: bool) -> String {
    let rows: String = (0..8).map(|i| format!("  <div>{i}</div>\n")).collect();
    let bad = if broken { "</span>\n" } else { "" };
    format!("<script lang=\"ts\">\n  const a = 1;\n</script>\n\n{rows}{bad}")
}

/// The diagnostic codes each carrier's BROKEN revision must produce — the
/// Verter-owned parse errors for a close tag that closed no open element.
///
/// Named codes, not counts: a count assertion passes for any unrelated set of
/// the same size, and it was exactly a count-shaped observation ("diagnostics
/// are there") that let this regression ship.
const VUE_BROKEN_CODE: &str = "XInvalidEndTag";
const SVELTE_BROKEN_CODE: &str = "svelte-official-reject-element-invalid-closing-tag";

/// Every diagnostic in `diagnostics` as `(code, range)`, sorted — a stable,
/// order-independent identity for a published set.
///
/// Codes AND ranges, never a count: a count passes for any unrelated set of the
/// same size, and it was exactly a count-shaped observation ("diagnostics are
/// there") that let this regression ship. The range half is compared against
/// what the SAME revision produces when opened, so it pins that the debounced
/// refresh maps positions exactly as the open path does — without asserting a
/// property a carrier does not have today (Svelte's structural-reject
/// diagnostics carry a collapsed 0:0 span at the producer, on the open path
/// too; Vue's carry real spans).
fn diagnostic_identities(diagnostics: &[Diagnostic]) -> Vec<(String, u32, u32, u32, u32)> {
    let mut identities: Vec<(String, u32, u32, u32, u32)> = diagnostics
        .iter()
        .map(|diagnostic| {
            let code = match diagnostic.code.as_ref() {
                Some(NumberOrString::String(code)) => code.clone(),
                Some(NumberOrString::Number(code)) => code.to_string(),
                None => String::new(),
            };
            (
                code,
                diagnostic.range.start.line,
                diagnostic.range.start.character,
                diagnostic.range.end.line,
                diagnostic.range.end.character,
            )
        })
        .collect();
    identities.sort();
    identities
}

/// Just the codes, for the preconditions.
fn codes_of(identities: &[(String, u32, u32, u32, u32)]) -> Vec<String> {
    let mut codes: Vec<String> = identities.iter().map(|(code, ..)| code.clone()).collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Verter's OWN diagnostics must keep tracking the document after an edit on a
/// provider-less route — the shipping `--type-provider=editor-tsserver` default,
/// where `project_sync` is `None`.
///
/// The regression (a live one, on `main`): the document commit deliberately
/// stopped compiling (issue #96), and the host's `upsert` clears
/// `latest_diagnostics`, so after an edit `get_diagnostics` — documented as NOT
/// triggering compilation — answers an empty snapshot. The one debounced path
/// that recompiles, `sync_file`, reached its `ensure_ide_compiled` only AFTER
/// `let Some(project_sync) = … else { return }`. So on a provider-less route
/// nothing ever recompiled and the diagnostics went EMPTY and never came back:
/// the identical broken text yields errors when OPENED and none when the same
/// breakage arrives as an edit.
///
/// This drives the REAL coordinator: a spawned `spawn_sync_coordinator` loop,
/// fed exactly what `handle_did_change` feeds it — a `needs_provider_sync`
/// insert plus a `signal` — and observed through the publish path's own diagnostic
/// cache. Calling `sync_file` directly would leave production's
/// `requires_sync && needs_provider_sync` dispatch gate uncovered, and a future
/// "skip `sync_file` when `project_sync` is None" optimisation would then
/// re-break the shipping VS Code route with this test still green.
///
/// It asserts the diagnostic CODES for each revision, against the codes that
/// same revision produces when OPENED — the control that showed the diagnostics
/// went empty rather than stale. The repair legs are what make it discriminating
/// in both directions: a "fix" that merely stopped clearing diagnostics would
/// pass the broken legs and fail the repaired ones.
#[tokio::test(flavor = "multi_thread")]
async fn verter_diagnostics_track_edits_on_a_provider_less_route() {
    for (carrier, uri_str, language_id, source, broken_code) in [
        (
            "vue",
            "file:///workspace/src/Cycle.vue",
            "vue",
            vue_carrier_source as fn(bool) -> String,
            VUE_BROKEN_CODE,
        ),
        (
            "svelte",
            "file:///workspace/src/Cycle.svelte",
            "svelte",
            svelte_carrier_source as fn(bool) -> String,
            SVELTE_BROKEN_CODE,
        ),
    ] {
        let when_opened_valid = identities_when_opened(uri_str, language_id, &source(false)).await;
        let when_opened_broken = identities_when_opened(uri_str, language_id, &source(true)).await;
        assert!(
            !codes_of(&when_opened_valid).contains(&broken_code.to_string()),
            "{carrier} precondition: the valid revision must NOT report {broken_code} \
             when opened (got {:?})",
            codes_of(&when_opened_valid)
        );
        assert!(
            codes_of(&when_opened_broken).contains(&broken_code.to_string()),
            "{carrier} precondition: the broken revision must report {broken_code} \
             when opened (got {:?}) — if it does not, the rest of this test proves \
             nothing",
            codes_of(&when_opened_broken)
        );

        let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
            HostConfig::default(),
        ))));
        let uri: Uri = uri_str.parse().expect("uri");
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: language_id.to_string(),
            version: 1,
            text: source(true),
        });
        let canonical_id = documents
            .get_canonical_id(&uri)
            .expect("the open document has a canonical id");

        // The real coordinator, wired exactly as the server wires it on a
        // provider-less route.
        let needs_provider_sync = Arc::new(DashSet::new());
        let cached_verter_diags = Arc::new(DashMap::new());
        let mut deps = make_provider_less_deps(&documents);
        deps.needs_provider_sync = Arc::clone(&needs_provider_sync);
        deps.cached_verter_diags = Arc::clone(&cached_verter_diags);
        // A read-only twin of the coordinator's own dependencies, so the
        // assertion reads the COMPLETE Verter-owned set through the same
        // function the open-side control uses. The coordinator's diagnostic
        // cache holds only the version-cached DOCUMENT half; the state-derived
        // categories (`verter(project)` ownership, the Svelte install check)
        // are recomputed on every publish and never enter it.
        let observer = deps.clone();
        let handle = spawn_sync_coordinator(deps);

        // Repair, break, repair, break. Each leg is one edit announced to the
        // coordinator the way `handle_did_change` announces it, then a wait for
        // the publish path's own recomputation for THAT document version.
        for (version, broken) in [(2, false), (3, true), (4, false), (5, true)] {
            let _ = documents.did_change(&uri, version, &source(broken));
            needs_provider_sync.insert(canonical_id.clone());
            handle.signal(
                canonical_id.clone(),
                uri.as_str().to_string(),
                Instant::now(),
            );

            await_publish_for_version(
                &handle,
                &cached_verter_diags,
                uri.as_str(),
                version,
                carrier,
            )
            .await;
            let published =
                diagnostic_identities(&compute_verter_diagnostics(&observer, &canonical_id, &uri));
            let expected = if broken {
                &when_opened_broken
            } else {
                &when_opened_valid
            };
            assert_eq!(
                &published, expected,
                "{carrier} v{version} (broken={broken}): the debounced coordinator must \
                 publish exactly what OPENING this same revision publishes. Anything \
                 else is the #96 regression: after the first edit the diagnostics went \
                 empty and never returned, because the only debounced recompile sat \
                 behind the `project_sync` gate on a route that has no provider"
            );
        }
    }
}

/// The Verter-owned diagnostics a revision produces when it is OPENED, as
/// `(code, range)`, in a fresh registry. `did_open` compiles; the document
/// commit deliberately does not, which is the whole asymmetry under test.
async fn identities_when_opened(
    uri_str: &str,
    language_id: &str,
    text: &str,
) -> Vec<(String, u32, u32, u32, u32)> {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let uri: Uri = uri_str.parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: language_id.to_string(),
        version: 1,
        text: text.to_string(),
    });
    let canonical_id = documents
        .get_canonical_id(&uri)
        .expect("the open document has a canonical id");
    let deps = make_provider_less_deps(&documents);
    diagnostic_identities(&compute_verter_diagnostics(&deps, &canonical_id, &uri))
}

/// Wait for the coordinator's publish path to recompute THIS document version.
///
/// The cache entry is written by the publish path itself and is stamped with the
/// document version it was computed for, so waiting on `version` observes the
/// real debounced publish rather than sleeping for one. The caller then compares
/// the complete Verter-owned set, ranges included, against the open-path control.
async fn await_publish_for_version(
    handle: &crate::sync_coordinator::SyncCoordinatorHandle,
    cached_verter_diags: &DashMap<String, crate::server::CachedVerterDiagEntry>,
    uri_str: &str,
    version: i32,
    carrier: &str,
) {
    handle
        .await_until(
            || {
                cached_verter_diags
                    .get(uri_str)
                    .is_some_and(|entry| entry.0 == version)
            },
            || {
                panic!(
                    "{carrier}: the coordinator never published diagnostics for v{version}; \
                     cached entry: {:?}",
                    cached_verter_diags.get(uri_str).map(|entry| entry.0)
                )
            },
        )
        .await;
}

/// An edit followed by a CLOSE before the quiet window elapses must not make
/// the debounced tick reach into the host for the file at all.
///
/// `did_change` leaves a pending coordinator signal behind and `did_close` does
/// not cancel it, so the tick still runs for a canonical id whose document is
/// gone and whose host source the close EVICTED. An ungated refresh then calls
/// `ensure_loaded`, which for an evicted canonical is no longer a cache lookup:
/// it submits a load that RESURRECTS the file from disk and pulls its
/// dependency closure in, to compile a buffer nobody is looking at — and the
/// publication that follows finds no document and drops the result anyway.
/// Pure waste, on the shared coordinator actor, ahead of every other file's
/// tick.
///
/// Driven through the real spawned coordinator, so it covers the wiring and not
/// just the helper. Measured on `ensure_loaded_calls` rather than on the compile
/// rail: the resurrect IS the load, and asserting the load never happens holds
/// whether or not the reloaded file would go on to compile (in a fixture with no
/// file on disk it would not, which makes a compile-count assertion vacuous
/// here).
#[tokio::test(start_paused = true)]
async fn a_closed_documents_pending_tick_never_reaches_into_the_host() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let uri: Uri = "file:///workspace/src/Closed.vue".parse().expect("uri");
    let needs_provider_sync = Arc::new(DashSet::new());
    let mut deps = make_provider_less_deps(&documents);
    deps.needs_provider_sync = Arc::clone(&needs_provider_sync);
    let handle = spawn_sync_coordinator(deps);

    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: vue_carrier_source(false),
    });
    let canonical_id = documents
        .get_canonical_id(&uri)
        .expect("the open document has a canonical id");

    // The user edits, then closes the tab before the debounce fires. The close
    // evicts the host source exactly as `handle_did_close` does — and, exactly
    // as `handle_did_close` does, it leaves the signal in place.
    let _ = documents.did_change(&uri, 2, &vue_carrier_source(true));
    needs_provider_sync.insert(canonical_id.clone());
    handle.signal(
        canonical_id.clone(),
        uri.as_str().to_string(),
        Instant::now(),
    );
    documents.did_close(&uri);
    host.evict(&canonical_id);

    let before = host.provenance_snapshot().ensure_loaded_calls;
    // Drive the quiet window under paused time: the wake, then the
    // exact debounce Instant. The tick must run and still not load.
    pump_tick(&handle).await;
    tokio::time::advance(Duration::from_millis(DEBOUNCE_MS)).await;
    pump_tick(&handle).await;
    let loads = host.provenance_snapshot().ensure_loaded_calls - before;

    assert_eq!(
        loads, 0,
        "the tick for a CLOSED document asked the host to load it {loads} time(s): \
         for an evicted canonical that is a disk reload plus dependency prefetch, \
         done for a buffer that no longer exists"
    );

    // Positive control: the identical tick for an OPEN document DOES load and
    // DOES compile. Without this the zero above passes for a refresh that never
    // runs at all — which is the regression this branch exists to fix.
    let open_uri: Uri = "file:///workspace/src/Open.vue".parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: open_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: vue_carrier_source(false),
    });
    let open_id = documents
        .get_canonical_id(&open_uri)
        .expect("the open document has a canonical id");
    let _ = documents.did_change(&open_uri, 2, &vue_carrier_source(true));
    let before_open = host.provenance_snapshot();
    needs_provider_sync.insert(open_id.clone());
    handle.signal(
        open_id.clone(),
        open_uri.as_str().to_string(),
        Instant::now(),
    );

    pump_tick(&handle).await;
    tokio::time::advance(Duration::from_millis(DEBOUNCE_MS)).await;
    pump_tick(&handle).await;
    let now = host.provenance_snapshot();
    assert!(
        now.ensure_loaded_calls > before_open.ensure_loaded_calls
            && now.compile_cold_runs > before_open.compile_cold_runs,
        "the tick for an OPEN document must still reach the host AND compile — \
         otherwise the closed-file zero above is vacuous and Verter's own \
         diagnostics never refresh"
    );
}

// ─── Cross-file republish: a child's settled edit re-arms its open parents ───

/// A child whose single declared prop is `prop`.
fn child_declaring_prop(prop: &str) -> String {
    format!(
        "<script setup lang=\"ts\">\ndefineProps<{{ {prop}: string }}>()\n</script>\n\
         <template><div>{{{{ {prop} }}}}</div></template>\n"
    )
}

/// A parent that passes `label` to the imported child. Valid while the child
/// declares `label`; a `verter/unknown-prop` the moment the child renames it.
const PARENT_PASSING_LABEL: &str = "<script setup lang=\"ts\">\n\
     import Child from './Child.vue'\n\
     </script>\n\
     <template><Child label=\"x\" /></template>\n";

/// The Verter-owned diagnostics `PARENT_PASSING_LABEL` produces when it is
/// OPENED against `child_source` — the fixture control. A fresh registry per
/// call, so nothing leaks between the control and the coordinator run.
async fn parent_diagnostics_when_opened_against(child_source: &str) -> Vec<Diagnostic> {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let child_uri: Uri = "file:///workspace/src/Child.vue".parse().expect("uri");
    let parent_uri: Uri = "file:///workspace/src/Parent.vue".parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri,
        language_id: "vue".to_string(),
        version: 1,
        text: child_source.to_string(),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: PARENT_PASSING_LABEL.to_string(),
    });
    let deps = verter_only_deps(Arc::clone(&documents));
    compute_verter_diagnostics(&deps, "/workspace/src/Parent.vue", &parent_uri)
}

/// Wait until the coordinator's publish path has recomputed the PARENT's
/// diagnostics into a set satisfying `satisfied`, and return that set.
///
/// The cache entry is written only by the publish path itself (nothing else in
/// these tests computes the parent), so observing it observes a real debounced
/// republish of the parent — the same observation rail
/// `await_publish_for_version` uses, keyed on content rather than version
/// because a cross-file republish does not move the parent's version.
async fn await_parent_republish_with(
    handle: &crate::sync_coordinator::SyncCoordinatorHandle,
    cached_verter_diags: &DashMap<String, crate::server::CachedVerterDiagEntry>,
    parent_uri: &str,
    what: &str,
    satisfied: impl Fn(&[Diagnostic]) -> bool,
) -> Vec<Diagnostic> {
    handle
        .await_until(
            || {
                cached_verter_diags
                    .get(parent_uri)
                    .is_some_and(|entry| satisfied(&entry.2))
            },
            || {
                panic!(
                    "the coordinator never republished the parent's diagnostics: {what}; \
                     the parent's cached entry is now {:?}",
                    cached_verter_diags
                        .get(parent_uri)
                        .map(|entry| (entry.0, entry.2.clone()))
                )
            },
        )
        .await;
    cached_verter_diags
        .get(parent_uri)
        .map(|entry| entry.2.clone())
        .expect("the awaited parent republish satisfied the predicate")
}

/// Editing a CHILD component must make its OPEN parents re-report diagnostics.
///
/// The regression (a live one, on `main`): a child edit clears only the
/// child's own `latest_diagnostics` (host upsert), signals only the child's
/// canonical (the `did_change` handler), and the debounced tick compiles,
/// syncs, and publishes ONLY pending keys — so a parent whose usage of the
/// child just became wrong keeps whatever the editor last showed, forever.
/// The reverse import graph the workspace maintains for exactly this
/// ("LSP affected-files reporting + diagnostics", R22) had zero production
/// call sites in `verter_lsp`.
///
/// Drives the REAL spawned coordinator, fed exactly what `handle_did_change`
/// feeds it — a `needs_provider_sync` insert plus a `signal` for the CHILD
/// only; the parent is never signalled, never edited, never touched. Asserts
/// the parent's published CONTENT (`verter/unknown-prop`, the prop and
/// component names, the range over the parent's own template), not merely that
/// a publish occurred.
///
/// Three legs make it discriminating in both directions:
/// - controls: the parent OPENED against each child revision proves the
///   fixture itself discriminates (no unknown-prop against v1, unknown-prop
///   against the rename) — without this a green means nothing;
/// - appear: after the child's prop rename settles, the parent republishes
///   WITH the diagnostic;
/// - clear: after the child restores the prop, the parent republishes WITHOUT
///   it — a fix that arms once but never clears would pass the appear leg and
///   fail here.
#[tokio::test(flavor = "multi_thread")]
async fn a_childs_settled_edit_republishes_each_open_parents_diagnostics() {
    fn unknown_label_prop(diags: &[Diagnostic]) -> Option<&Diagnostic> {
        diags.iter().find(|d| {
            matches!(&d.code, Some(NumberOrString::String(code)) if code == "verter/unknown-prop")
                && d.message.contains("'label'")
                && d.message.contains("<Child>")
        })
    }

    // ── Controls: the fixture must discriminate on its own. ──
    let against_original =
        parent_diagnostics_when_opened_against(&child_declaring_prop("label")).await;
    assert!(
        unknown_label_prop(&against_original).is_none(),
        "control: the parent opened against the ORIGINAL child must not report \
         an unknown `label` prop, got {against_original:?}"
    );
    let against_renamed =
        parent_diagnostics_when_opened_against(&child_declaring_prop("title")).await;
    let control_diag = unknown_label_prop(&against_renamed)
        .unwrap_or_else(|| {
            panic!(
                "control: the parent opened against the RENAMED child must report \
                 `verter/unknown-prop` for `label` — if it does not, the rest of \
                 this test proves nothing (got {against_renamed:?})"
            )
        })
        .clone();

    // ── The scenario under test: both files open, the CHILD is edited. ──
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let child_uri: Uri = "file:///workspace/src/Child.vue".parse().expect("uri");
    let parent_uri: Uri = "file:///workspace/src/Parent.vue".parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: child_declaring_prop("label"),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: PARENT_PASSING_LABEL.to_string(),
    });
    let child_id = documents
        .get_canonical_id(&child_uri)
        .expect("open child has a canonical id");
    let parent_id = documents
        .get_canonical_id(&parent_uri)
        .expect("open parent has a canonical id");

    // Fixture precondition: the upsert-recorded parsed edges alone must make
    // the parent reachable from the child on the workspace's reverse import
    // graph — the graph production populates on every open/edit.
    let importers = documents
        .host()
        .workspace_read()
        .affected_canonicals(&child_id);
    assert!(
        importers.contains(&parent_id),
        "fixture precondition: `affected_canonicals({child_id})` must contain the \
         open parent {parent_id}, got {importers:?}"
    );

    let deps = verter_only_deps(Arc::clone(&documents));
    let observer = deps.clone();
    let handle = spawn_sync_coordinator(deps);

    // ── Appear: rename the child's prop; announce the CHILD only, exactly as
    // `handle_did_change` announces it. ──
    let _ = documents.did_change(&child_uri, 2, &child_declaring_prop("title"));
    observer.needs_provider_sync.insert(child_id.clone());
    handle.signal(
        child_id.clone(),
        child_uri.as_str().to_string(),
        Instant::now(),
    );

    let republished = await_parent_republish_with(
        &handle,
        &observer.cached_verter_diags,
        parent_uri.as_str(),
        "the child renamed its prop, so the parent's `label` usage became an \
         unknown prop — the parent must re-report it",
        |diags| unknown_label_prop(diags).is_some(),
    )
    .await;
    let republished_diag = unknown_label_prop(&republished)
        .expect("the awaited set satisfied the predicate")
        .clone();
    assert_eq!(
        (republished_diag.range, republished_diag.severity),
        (control_diag.range, control_diag.severity),
        "the republished parent diagnostic must carry the same range and severity \
         as the one the open-path control produces for the identical state"
    );

    // ── Clear: restore the child's prop; the parent must republish WITHOUT the
    // diagnostic. ──
    let _ = documents.did_change(&child_uri, 3, &child_declaring_prop("label"));
    observer.needs_provider_sync.insert(child_id.clone());
    handle.signal(
        child_id.clone(),
        child_uri.as_str().to_string(),
        Instant::now(),
    );

    await_parent_republish_with(
        &handle,
        &observer.cached_verter_diags,
        parent_uri.as_str(),
        "the child restored its prop, so the parent's stale `verter/unknown-prop` \
         must clear",
        |diags| unknown_label_prop(diags).is_none(),
    )
    .await;
}

/// Svelte is first-class on the cross-file republish path, and the fan-out is
/// bounded by the debounce, never per keystroke.
///
/// Provider-backed (mock tsgo): the parent's committed provider surface is
/// seeded with a sentinel type diagnostic, and a burst of CHILD keystrokes is
/// announced exactly as `handle_did_change` announces them. The parent's
/// republish is observed on the provider's own rail — a fresh
/// `get_diagnostics` pull for the PARENT's committed surface, which can only
/// happen through the parent's publish path — and the pull COUNT is the storm
/// bound: five keystrokes coalesce into one settled child window, so the
/// parent republishes once (two at most under pathological scheduling), never
/// once per keystroke. The content control pins that the pulled sentinel maps
/// back into the parent's own source through the exact merge the debounced
/// publish runs (`compute_merged_diagnostics`, the sanctioned compute/publish
/// split — the coordinator otherwise pushes to a socket a test cannot read).
#[tokio::test(flavor = "multi_thread")]
async fn a_svelte_childs_settled_edit_republishes_the_open_parent_bounded_by_the_debounce() {
    use crate::type_provider::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

    let tmp = tempfile::tempdir().expect("workspace");
    install_svelte_at(tmp.path(), USABLE_SVELTE);
    let src_dir = tmp.path().join("src");
    std::fs::create_dir_all(&src_dir).expect("src dir");
    let root = verter_span::path::canonicalize_path(&tmp.path().to_string_lossy());

    let child_canonical =
        verter_span::path::canonicalize_path(&src_dir.join("Child.svelte").to_string_lossy());
    let parent_canonical =
        verter_span::path::canonicalize_path(&src_dir.join("Parent.svelte").to_string_lossy());
    let child_source = |n: usize| {
        format!(
            "<script lang=\"ts\">\n  export let label{n}: string;\n</script>\n\
             <div>{{label{n}}}</div>\n"
        )
    };
    let parent_source = "<script lang=\"ts\">\n  import Child from './Child.svelte';\n\
         </script>\n<Child label0=\"x\" />\n";
    std::fs::write(&child_canonical, child_source(0)).expect("child on disk");
    std::fs::write(&parent_canonical, parent_source).expect("parent on disk");

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));
    let child_uri = crate::uri::path_to_file_uri(&child_canonical).expect("child uri");
    let parent_uri = crate::uri::path_to_file_uri(&parent_canonical).expect("parent uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: child_source(0),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "svelte".to_string(),
        version: 1,
        text: parent_source.to_string(),
    });

    let provider = Arc::new(MockTypeProvider::new());
    let deps = SyncCoordinatorDeps {
        documents: Arc::clone(&documents),
        project_sync: Some(ProjectSync::new_with_kind(
            provider.clone(),
            ProjectSyncMode::FullProject,
            crate::TypeProviderKind::Tsgo,
        )),
        needs_provider_sync: Arc::new(DashSet::new()),
        pending_snapshot_provider_sync: Arc::new(DashSet::new()),
        client: make_test_client(),
        type_provider: Some(provider.clone()),
        cached_verter_diags: Arc::new(DashMap::new()),
        position_encoding: Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16)),
        provider_sync_states: Arc::new(DashMap::new()),
        vfs_workspace: Arc::new(crate::test_utils::make_test_vfs_workspace_with_resolver(
            &root,
            Some(&format!("{root}/tsconfig.json")),
        )),
        type_provider_kind: crate::TypeProviderKind::Tsgo,
        carrier_publish_coordinator: None,
        carrier_transaction_coordinator: std::sync::Arc::new(
            crate::external_ts::CarrierTransactionCoordinator::new(),
        ),
    };

    // Commit the PARENT's provider surface once — the open-time sync the
    // server performs — so the debounced republish has a committed surface to
    // pull provider diagnostics for.
    sync_file(&deps, &parent_canonical, parent_uri.as_str()).await;
    let parent_ide_path = deps
        .provider_sync_states
        .get(&parent_canonical)
        .and_then(|state| state.ide_path.clone())
        .expect("the owner-resolved parent sync must commit an IDE path");

    let recorded = documents
        .provider_surfaces()
        .current_snapshot(&parent_ide_path)
        .expect("a successful sync records the provider surface it delivered");
    let at = recorded
        .provider_content
        .find("label0")
        .expect("the child usage's attribute is present in the parent's provider buffer");
    provider.set_diagnostics(
        &parent_ide_path,
        vec![TypeDiagnostic {
            message: "SVELTE_PARENT_SENTINEL".to_string(),
            severity: TypeDiagnosticSeverity::Error,
            start: at as u32,
            end: (at + 5) as u32,
            code: Some("2322".to_string()),
            tags: Vec::new(),
            related_information: Vec::new(),
        }],
    );

    // Content control: the parent's publish-path merge maps the sentinel back
    // into the parent's `.svelte` source.
    let merged = compute_merged_diagnostics(&deps, &parent_canonical, &parent_uri).await;
    assert!(
        merged.iter().any(|d| d.message == "SVELTE_PARENT_SENTINEL"),
        "content control: the parent's merge must serve the provider sentinel \
         mapped into the parent source, got {merged:?}"
    );

    // Fixture precondition: the reverse import graph reaches the parent from
    // the child for `.svelte` carriers too.
    let importers = host.workspace_read().affected_canonicals(&child_canonical);
    assert!(
        importers.contains(&parent_canonical),
        "fixture precondition: `affected_canonicals({child_canonical})` must \
         contain the open Svelte parent {parent_canonical}, got {importers:?}"
    );

    provider.clear_calls();
    let observer = deps.clone();
    let handle = spawn_sync_coordinator(deps);

    // A burst of child keystrokes inside one quiet window, each announced
    // exactly as `handle_did_change` announces it.
    const KEYSTROKES: usize = 5;
    for i in 1..=KEYSTROKES {
        let _ = documents.did_change(&child_uri, 1 + i as i32, &child_source(i));
        observer.needs_provider_sync.insert(child_canonical.clone());
        handle.signal(
            child_canonical.clone(),
            child_uri.as_str().to_string(),
            Instant::now(),
        );
    }

    // The parent must republish: a fresh provider pull for the PARENT's own
    // committed surface, reachable only through the parent's publish path.
    let parent_pulls = |provider: &MockTypeProvider| {
        provider
            .calls()
            .iter()
            .filter(
                |call| matches!(call, MockCall::GetDiagnostics { path } if path == &parent_ide_path),
            )
            .count()
    };
    handle
        .await_until(
            || parent_pulls(&provider) > 0 && handle.diag_tasks_live() == 0,
            || {
                panic!(
                    "the coordinator never republished the Svelte parent after the \
                     child's settled edit — no fresh provider pull for {parent_ide_path}; \
                     provider calls: {:?}",
                    provider.calls()
                )
            },
        )
        .await;
    let pulls = parent_pulls(&provider);
    assert!(
        (1..=2).contains(&pulls),
        "the parent republished {pulls} times for {KEYSTROKES} child keystrokes — \
         the re-arm must ride the debounced settle (once per quiet window, twice \
         at most under pathological scheduling), never the per-keystroke path"
    );
}

/// Does the set contain the parent's `verter/unknown-prop` for `label`?
fn has_unknown_label_prop(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| {
        matches!(&d.code, Some(NumberOrString::String(code)) if code == "verter/unknown-prop")
            && d.message.contains("'label'")
    })
}

/// The diagnostics-epoch value a fresh read of `uri` would validate against.
fn live_epoch(documents: &DocumentRegistry, canonical_id: &str) -> Option<u64> {
    documents.host().get_diagnostics_generation(canonical_id)
}

/// The cache entry a computation that ENTERED before the arm eventually
/// writes: the document version and epoch it snapshotted at entry, and the
/// diagnostics it derived from the pre-edit world.
fn snapshot_entry(
    cached_verter_diags: &DashMap<String, crate::server::CachedVerterDiagEntry>,
    uri: &Uri,
    what: &str,
) -> crate::server::CachedVerterDiagEntry {
    let entry = cached_verter_diags
        .get(uri.as_str())
        .unwrap_or_else(|| panic!("{what}: the compute must warm the cache entry"));
    (entry.0, entry.1, entry.2.clone())
}

/// A parent diagnostic computation already IN FLIGHT when the child's settled
/// edit arms the parent must never have its result served after the arm.
///
/// The race (found in review; worse than the bug the arming fixed, because it
/// republishes WRONG diagnostics rather than none): the arm's cache-entry
/// removal fences nothing that is already running —
/// 1. a parent computation begins and captures PRE-edit child state;
/// 2. the child's settled edit arms the parent and drops its cache entry;
/// 3. the old computation finishes and lands its write AFTER the drop (the
///    unconditional insert at the end of the document-half compute in
///    `server_utils.rs`);
/// 4. the armed pass reads the cache: the parent's document version and host
///    diagnostics epoch both still match — warm hit;
/// 5. the parent republishes pre-edit diagnostics as fresh, and nothing ever
///    invalidates them again.
///
/// The fence under test: arming ADVANCES the parent's host diagnostics epoch —
/// the value every computation snapshots at entry, stamps into its write, and
/// every read re-validates against. A computation that began before the arm
/// carries a pre-arm stamp, so no post-arm read can ever be satisfied by it —
/// read-side authoritative, no writer coordination.
///
/// Why the removal alone was not enough, measured on this very fixture: the
/// parent's epoch next moves at the ARMED TICK, one full debounce window after
/// the arm, when `refresh_carrier_ide_surface`'s cold recompile stores
/// diagnostics — and only IF that recompile is cold, which is an accident of
/// compile-fact granularity, not a designed fence. Until then the slot
/// validates a pre-arm stamp: for ~one debounce window every read — the pull
/// `textDocument/diagnostic` path shares this exact cache — serves the
/// in-flight computation's pre-edit result as fresh.
///
/// Two legs, each unconditional and each discriminating a different mechanism:
///
/// - **the fence** runs the arm DIRECTLY, with no coordinator spawned and
///   nothing else able to touch the parent's slot. That ordering is the
///   finding's exact interleaving, produced by construction rather than by
///   winning a race against a debounce window: the plant lands after the arm
///   because the test puts it there. It is also the only form in which the
///   assertion is about the ARM: driven through the coordinator, an accidental
///   cold recompile advances the epoch too, so a green would not distinguish
///   the fence from a compile that happened to bump the same counter;
/// - **the armed republish** runs the REAL coordinator over a parent whose
///   slot already holds a stale entry stamped with the LIVE epoch, so the only
///   thing that can dislodge it is the arm the child's settle owes. Nothing is
///   spawned until the plant is in place, so the ordering needs no polling.
#[tokio::test(flavor = "multi_thread")]
async fn an_inflight_parent_computation_never_satisfies_a_read_after_the_arm() {
    // ── Control: fresh content is reachable at all. ──
    let against_renamed =
        parent_diagnostics_when_opened_against(&child_declaring_prop("title")).await;
    assert!(
        has_unknown_label_prop(&against_renamed),
        "control: the parent opened against the RENAMED child must report \
         `verter/unknown-prop` for `label`, got {against_renamed:?}"
    );

    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let child_uri: Uri = "file:///workspace/src/Child.vue".parse().expect("uri");
    let parent_uri: Uri = "file:///workspace/src/Parent.vue".parse().expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: child_declaring_prop("label"),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: PARENT_PASSING_LABEL.to_string(),
    });
    let child_id = documents
        .get_canonical_id(&child_uri)
        .expect("open child has a canonical id");
    let parent_id = documents
        .get_canonical_id(&parent_uri)
        .expect("open parent has a canonical id");

    let deps = verter_only_deps(Arc::clone(&documents));
    let observer = deps.clone();

    // The pre-arm computation, run to completion through the REAL compute
    // path. Its cache write is byte-identical to what an in-flight computation
    // that began before the arm eventually lands: the parent's version, the
    // parent's PRE-arm host diagnostics epoch (both snapshotted at compute
    // entry), and diagnostics derived from the pre-edit child.
    let _ = compute_verter_diagnostics(&observer, &parent_id, &parent_uri);
    let stale_entry = snapshot_entry(
        &observer.cached_verter_diags,
        &parent_uri,
        "the pre-edit parent compute",
    );
    assert!(
        !has_unknown_label_prop(&stale_entry.2),
        "control: the pre-edit parent set must NOT contain the unknown-prop — \
         otherwise stale and fresh are indistinguishable and this test proves \
         nothing, got {:?}",
        stale_entry.2
    );

    // ── The child's edit, landed in the host exactly as `handle_did_change`
    // lands it. It moves the CHILD's epoch and the CHILD's document version;
    // it moves neither of the parent's, which is precisely why the parent's
    // slot still validates and why the arm owes the fence. ──
    let _ = documents.did_change(&child_uri, 2, &child_declaring_prop("title"));
    assert_eq!(
        live_epoch(&documents, &parent_id),
        stale_entry.1,
        "fixture: a child edit must leave the PARENT's epoch where the \
         in-flight computation sampled it — if the edit moved it on its own \
         the arm's advance would not be what this test observes"
    );

    // ── Leg 1: the arm, run directly, with nothing else in the process able
    // to touch the parent's slot or its epoch. ──
    let mut pending: HashMap<String, (Instant, PendingSignal)> = HashMap::new();
    arm_open_importer_republish(&observer, std::slice::from_ref(&child_id), &mut pending);
    assert!(
        pending.contains_key(&parent_id),
        "fixture: the child's settled edit must arm the open parent — \
         armed: {:?}",
        pending.keys().collect::<Vec<_>>()
    );

    // Land the in-flight computation's late write AFTER the arm: the same key,
    // the same value shape, the same shared map as the racing writer.
    observer
        .cached_verter_diags
        .insert(parent_uri.as_str().to_string(), stale_entry.clone());

    // The invariant, on the same validating read every publisher and the pull
    // `textDocument/diagnostic` path use: a computation that began before the
    // arm can never satisfy a read that happens after it. Un-fenced, this read
    // warm-hits the late write — the parent's document version and its host
    // diagnostics epoch both still match — and serves the pre-edit set as
    // fresh.
    let served = compute_verter_diagnostics(&observer, &parent_id, &parent_uri);
    assert!(
        has_unknown_label_prop(&served),
        "a post-arm read was satisfied by the in-flight pre-edit computation's \
         late write — the parent would serve stale diagnostics as fresh. The \
         plant was stamped {:?} and the live epoch is {:?}; served: {served:?}",
        stale_entry.1,
        live_epoch(&documents, &parent_id)
    );

    // ── Leg 2: the finding's step 4→5 end to end. The parent's slot is
    // primed with a stale entry stamped with the LIVE epoch — an entry that
    // validates, so only the arm the child's settle owes can dislodge it —
    // and the coordinator is spawned only after the plant is in place, so no
    // polling is needed to establish the ordering. The child then RESTORES
    // `label`, making post-edit content lack the unknown-prop the plant
    // carries: discriminable in the opposite direction. ──
    let primed: crate::server::CachedVerterDiagEntry = (
        stale_entry.0,
        live_epoch(&documents, &parent_id),
        served.clone(),
    );
    assert!(
        has_unknown_label_prop(&primed.2),
        "fixture: the primed entry must carry the title-world unknown-prop so \
         the post-edit republish is distinguishable from it"
    );
    observer
        .cached_verter_diags
        .insert(parent_uri.as_str().to_string(), primed);

    let handle = spawn_sync_coordinator(deps);
    let _ = documents.did_change(&child_uri, 3, &child_declaring_prop("label"));
    observer.needs_provider_sync.insert(child_id.clone());
    handle.signal(
        child_id.clone(),
        child_uri.as_str().to_string(),
        Instant::now(),
    );

    await_parent_republish_with(
        &handle,
        &observer.cached_verter_diags,
        parent_uri.as_str(),
        "the armed republish must replace a stale-but-validating entry with \
         post-edit content (the restored `label` clears the unknown-prop)",
        |diags| !has_unknown_label_prop(diags),
    )
    .await;
}

/// Grandparent that only knows the middle parent — never imports the child.
/// Its own diagnostics need not move on a child prop rename; arming is
/// observed via epoch advance / cache drop, not content.
const GRANDPARENT_USING_PARENT: &str = "<script setup lang=\"ts\">\n\
     import Parent from './Parent.vue'\n\
     </script>\n\
     <template><Parent /></template>\n";

/// Depth fixture that the shallow one-parent tests cannot pin:
/// `Grandparent → Parent → Child`, plus a closed direct importer of Child.
///
/// The prior tests each use one direct open importer. They pass even if the
/// implementation armed only direct reverse deps, armed closed documents, or
/// marked armed parents `requires_sync: true` (which would cascade). There is
/// no grandparent, so no-cascade is never discriminated.
///
/// Every arming property is pinned on ONE direct call to the arm, with no
/// coordinator spawned — so the armed SET, the epoch advances, and the shape
/// of each armed signal are exact values, not a race against a debounce
/// window:
/// 1. **Transitive open**: the armed set is EXACTLY the two open importers.
///    The grandparent is reached through the `affected_canonicals` closure
///    even though it does not import the child directly; direct-only arming
///    would leave it out.
/// 2. **Open-only**: the closed direct importer sits in the reverse closure
///    and is absent from the armed set — its pre-warmed cache entry and host
///    epoch both survive byte-identical.
/// 3. **No cascade**: every armed signal carries `requires_sync: false`, and
///    the tick appends to `settled_edits` — the list this arm consumes — only
///    for a signal whose `requires_sync` is set (`sync_coordinator.rs`, the
///    `if signal.requires_sync` push). An armed republish therefore cannot
///    re-enter the arm as an edit, so it cannot arm the armed file's own
///    importers. This is the mechanism itself, asserted as a value; the
///    superseded form counted epoch advances after a wall-clock quiescence
///    wait, which cannot distinguish "no cascade" from "the cascade's advance
///    lands after the test stopped looking".
/// 4. **Exactly one advance per armed file**, counted with nothing else
///    running.
///
/// A final leg drives the REAL coordinator to pin that the same arm publishes
/// the direct parent's post-edit content, observed on content rather than on
/// a timer.
#[tokio::test(flavor = "multi_thread")]
async fn open_importer_arming_reaches_transitive_open_only_without_cascade() {
    let documents = Arc::new(DocumentRegistry::new(Arc::new(VerterHost::new_standalone(
        HostConfig::default(),
    ))));
    let child_uri: Uri = "file:///workspace/src/Child.vue".parse().expect("uri");
    let parent_uri: Uri = "file:///workspace/src/Parent.vue".parse().expect("uri");
    let grandparent_uri: Uri = "file:///workspace/src/Grandparent.vue"
        .parse()
        .expect("uri");
    let closed_uri: Uri = "file:///workspace/src/ClosedImporter.vue"
        .parse()
        .expect("uri");
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: child_declaring_prop("label"),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: PARENT_PASSING_LABEL.to_string(),
    });
    let _ = documents.did_open(&TextDocumentItem {
        uri: grandparent_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: GRANDPARENT_USING_PARENT.to_string(),
    });
    // Closed direct importer of the child: open just long enough to record the
    // reverse edge, pre-warm its verter-diag cache, then close. `notify_close`
    // clears the overlay only — reverse edges survive (see workspace
    // `notify_close` vs `notify_delete`), so the closed file remains in the
    // child's affected closure and would be armed by an open-filter miss.
    let _ = documents.did_open(&TextDocumentItem {
        uri: closed_uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: PARENT_PASSING_LABEL.to_string(),
    });

    let child_id = documents
        .get_canonical_id(&child_uri)
        .expect("open child has a canonical id");
    let parent_id = documents
        .get_canonical_id(&parent_uri)
        .expect("open parent has a canonical id");
    let grandparent_id = documents
        .get_canonical_id(&grandparent_uri)
        .expect("open grandparent has a canonical id");
    let closed_id = documents
        .get_canonical_id(&closed_uri)
        .expect("open closed-importer has a canonical id");

    let deps = verter_only_deps(Arc::clone(&documents));
    let observer = deps.clone();

    // Pre-warm every importer's cache so the arm's drop is observable.
    let _ = compute_verter_diagnostics(&observer, &parent_id, &parent_uri);
    let _ = compute_verter_diagnostics(&observer, &grandparent_id, &grandparent_uri);
    let _ = compute_verter_diagnostics(&observer, &closed_id, &closed_uri);
    let closed_entry_before = snapshot_entry(
        &observer.cached_verter_diags,
        &closed_uri,
        "the closed importer's pre-warm",
    );
    assert!(
        observer
            .cached_verter_diags
            .contains_key(grandparent_uri.as_str()),
        "pre-warm must leave a grandparent cache entry so a direct-only arm \
         (which never touches the grandparent) is distinguishable from a \
         transitive arm (which drops it)"
    );

    documents.did_close(&closed_uri);
    assert!(
        !documents
            .open_uris()
            .iter()
            .any(|u| u == closed_uri.as_str()),
        "fixture: ClosedImporter must be closed before the child's edit"
    );

    // Fixture precondition: reverse closure is transitive AND includes the
    // closed direct importer. Without this the rest of the test is vacuous.
    let importers = documents
        .host()
        .workspace_read()
        .affected_canonicals(&child_id);
    assert!(
        importers.contains(&parent_id)
            && importers.contains(&grandparent_id)
            && importers.contains(&closed_id),
        "fixture precondition: `affected_canonicals({child_id})` must contain \
         the direct parent {parent_id}, the transitive grandparent \
         {grandparent_id}, and the closed direct importer {closed_id}, got \
         {importers:?}"
    );

    let parent_epoch_before = live_epoch(&documents, &parent_id);
    let grandparent_epoch_before = live_epoch(&documents, &grandparent_id);
    let closed_epoch_before = live_epoch(&documents, &closed_id);

    // ── One arm for one settled child edit, run directly: nothing else in
    // this process can advance an epoch or touch a slot while it runs. ──
    let mut pending: HashMap<String, (Instant, PendingSignal)> = HashMap::new();
    arm_open_importer_republish(&observer, std::slice::from_ref(&child_id), &mut pending);

    // (1) + (2): the armed set is EXACTLY the open transitive closure.
    let mut armed: Vec<String> = pending.keys().cloned().collect();
    armed.sort();
    let mut expected = vec![grandparent_id.clone(), parent_id.clone()];
    expected.sort();
    assert_eq!(
        armed, expected,
        "one settled child edit must arm EXACTLY the open importers in the \
         reverse closure: the direct parent, the transitive grandparent, and \
         nothing else. The closed direct importer {closed_id} is in the \
         closure and must NOT appear; a direct-only arm would omit the \
         grandparent {grandparent_id}"
    );

    // (3) No cascade, as the mechanism itself: the tick appends to
    // `settled_edits` only for a signal carrying `requires_sync`, so an armed
    // republish can never re-enter this arm as an edit.
    for (armed_id, (_, signal)) in pending.iter() {
        assert!(
            !signal.requires_sync,
            "armed importer {armed_id} must carry `requires_sync: false` — a \
             `true` here puts the armed file into the NEXT tick's \
             `settled_edits`, which re-arms ITS importers: the republish \
             cascade this contract forbids"
        );
        assert!(
            signal.force_diagnostics,
            "armed importer {armed_id} must carry `force_diagnostics: true` — \
             the arm exists to make the importer republish"
        );
    }

    // (4) Exactly one epoch advance per armed file, and the pre-warmed entry
    // dropped so no stale value survives the arm.
    for (armed_id, before) in [
        (&parent_id, parent_epoch_before),
        (&grandparent_id, grandparent_epoch_before),
    ] {
        let after = live_epoch(&documents, armed_id);
        assert_eq!(
            after,
            before.map(|epoch| epoch + 1).or(Some(1)),
            "arming {armed_id} must advance its diagnostics epoch EXACTLY \
             once (was {before:?}); a second advance means it was armed twice \
             from a single settle"
        );
    }
    assert!(
        !observer
            .cached_verter_diags
            .contains_key(parent_uri.as_str())
            && !observer
                .cached_verter_diags
                .contains_key(grandparent_uri.as_str()),
        "arming must drop each armed importer's cache entry"
    );

    // (2, continued) The closed importer is frozen in both dimensions.
    assert_eq!(
        live_epoch(&documents, &closed_id),
        closed_epoch_before,
        "a closed importer must not be armed: its host diagnostics epoch must \
         stay at the pre-edit baseline {closed_epoch_before:?}"
    );
    assert_eq!(
        observer
            .cached_verter_diags
            .get(closed_uri.as_str())
            .map(|entry| (entry.0, entry.1, entry.2.clone())),
        Some(closed_entry_before.clone()),
        "a closed importer must not be armed: its pre-warmed cache entry must \
         survive the arm byte-identical (arming drops the entry)"
    );

    // ── The same arm, through the REAL coordinator: the open direct parent
    // must republish the post-edit content. Observed on content, not a timer.
    let handle = spawn_sync_coordinator(deps);
    let _ = documents.did_change(&child_uri, 2, &child_declaring_prop("title"));
    observer.needs_provider_sync.insert(child_id.clone());
    handle.signal(
        child_id.clone(),
        child_uri.as_str().to_string(),
        Instant::now(),
    );

    await_parent_republish_with(
        &handle,
        &observer.cached_verter_diags,
        parent_uri.as_str(),
        "the open direct parent must republish `verter/unknown-prop` after the \
         child renames its prop",
        has_unknown_label_prop,
    )
    .await;
}

/// The stale write a pre-arm computation lands, distinguishable from anything
/// a real recompute can produce.
fn inflight_sentinel() -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: 0,
                character: 1,
            },
        },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(
            "test/pre-arm-inflight-write".to_string(),
        )),
        message: "planted by a computation that entered before the arm".to_string(),
        ..Default::default()
    }
}

/// The arm's fence is carrier-agnostic: an OPEN plain `.ts` importer gets the
/// same guarantee a `.vue` parent does.
///
/// The failure this pins is not hypothetical, and it is not about `.ts`
/// syntax — it is about which files carry a compile row. The epoch the fence
/// rides lived behind two gates that a plain script reaches routinely and a
/// freshly-compiled carrier does not:
///
/// - `get_diagnostics_generation` reported `None` for any canonical flagged
///   evicted, while `evict` left the stored counter untouched and
///   `bump_diagnostics_generation` kept advancing it — so the arm's advance
///   was invisible;
/// - reopening a document with byte-identical content takes the `upsert`
///   quintuple-unchanged fast path, which does not clear that evicted flag.
///
/// `did_close` → `did_open` is therefore enough: the reopened document is
/// OPEN, is in the child's reverse closure, is armed — and its epoch reads the
/// same value before the arm and after it. With the reader collapsing that
/// `None` onto `0`, an in-flight computation's pre-arm write validates against
/// a post-arm read and the parent republishes pre-edit diagnostics as fresh.
///
/// Deterministic by construction: the arm is called directly, so the plant
/// lands after it because the test puts it there, and no compile can advance
/// the epoch behind the assertion's back. The planted set carries a sentinel
/// no recompute can produce, so "the read refused the plant" is observable
/// even for a file whose real diagnostics are empty in both worlds.
#[tokio::test(flavor = "multi_thread")]
async fn an_open_plain_ts_importer_is_fenced_after_an_evict_and_reopen() {
    let tmp = tempfile::tempdir().expect("workspace");
    let ws = tmp.path();
    std::fs::create_dir_all(ws.join("src")).expect("src dir");
    std::fs::write(ws.join("tsconfig.json"), "{\"include\":[\"src\"]}").expect("tsconfig");
    let child_source = "export const label = 'x';\n";
    let parent_source = "import { label } from './child';\nexport const echo = label;\n";
    std::fs::write(ws.join("src/child.ts"), child_source).expect("child on disk");
    std::fs::write(ws.join("src/parent.ts"), parent_source).expect("parent on disk");

    let workspace_id = crate::test_utils::canonical_test_path(ws);
    let child_id = format!("{workspace_id}/src/child.ts");
    let parent_id = format!("{workspace_id}/src/parent.ts");

    let host = crate::test_utils::make_filesystem_test_host(ws);
    host.configure_projects(vec![crate::project_resolver::IdeProjectConfig::new(
        workspace_id.clone(),
        workspace_id.clone(),
        Some(format!("{workspace_id}/tsconfig.json")),
    )]);
    let documents = Arc::new(DocumentRegistry::new(Arc::clone(&host)));

    let child_uri = crate::uri::path_to_file_uri(&child_id).expect("child uri");
    let parent_uri = crate::uri::path_to_file_uri(&parent_id).expect("parent uri");
    let open_parent = TextDocumentItem {
        uri: parent_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: parent_source.to_string(),
    };
    let _ = documents.did_open(&open_parent);
    let _ = documents.did_open(&TextDocumentItem {
        uri: child_uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: child_source.to_string(),
    });

    // The server's `did_close` handler: drop the cached entry, evict the file
    // from the host (`server/lifecycle.rs`). Then reopen it byte-identical —
    // the `upsert` quintuple-unchanged fast path, which leaves the evicted
    // flag set.
    documents.did_close(&parent_uri);
    host.evict(&parent_id);
    let _ = documents.did_open(&open_parent);
    assert!(
        documents
            .open_uris()
            .iter()
            .any(|u| u == parent_uri.as_str()),
        "fixture: the plain-script parent must be OPEN again after the reopen"
    );

    // Fixture precondition: the arm can reach it at all.
    let importers = documents
        .host()
        .workspace_read()
        .affected_canonicals(&child_id);
    assert!(
        importers.contains(&parent_id),
        "fixture precondition: `affected_canonicals({child_id})` must contain \
         the open plain-script parent {parent_id}, got {importers:?}"
    );

    let deps = verter_only_deps(Arc::clone(&documents));

    // The pre-arm computation, through the real compute path: its write
    // carries the parent's version and the epoch it sampled at entry.
    let _ = compute_verter_diagnostics(&deps, &parent_id, &parent_uri);
    let warmed = snapshot_entry(
        &deps.cached_verter_diags,
        &parent_uri,
        "the plain-script parent's pre-arm compute",
    );
    let stale_entry: crate::server::CachedVerterDiagEntry =
        (warmed.0, warmed.1, vec![inflight_sentinel()]);
    let epoch_before_arm = live_epoch(&documents, &parent_id);
    assert_eq!(
        warmed.1, epoch_before_arm,
        "fixture: the compute must stamp the epoch it read, or the plant does \
         not model an in-flight write"
    );

    // ── The arm. ──
    let mut pending: HashMap<String, (Instant, PendingSignal)> = HashMap::new();
    arm_open_importer_republish(&deps, std::slice::from_ref(&child_id), &mut pending);
    assert!(
        pending.contains_key(&parent_id),
        "the open plain-script parent must be armed by its child's settled \
         edit — armed: {:?}",
        pending.keys().collect::<Vec<_>>()
    );

    // The advance the fence rides must be OBSERVABLE. This is the assertion
    // the evicted-reopened plain script failed: the arm bumped a counter no
    // reader could see, so before and after were the same value.
    let epoch_after_arm = live_epoch(&documents, &parent_id);
    assert_ne!(
        epoch_after_arm, epoch_before_arm,
        "arming an open plain-script importer must move the epoch a reader \
         validates against: it read {epoch_before_arm:?} before the arm and \
         {epoch_after_arm:?} after, so a computation that entered before the \
         arm still validates against every read that follows it"
    );

    // The in-flight computation's late write lands after the arm.
    deps.cached_verter_diags
        .insert(parent_uri.as_str().to_string(), stale_entry.clone());

    let served = compute_verter_diagnostics(&deps, &parent_id, &parent_uri);
    assert!(
        !served.iter().any(|d| d.code == stale_entry.2[0].code),
        "a post-arm read on the open plain-script parent was satisfied by the \
         pre-arm computation's late write: it served the planted sentinel as \
         fresh. Plant stamped {:?}, live epoch {:?}; served: {served:?}",
        stale_entry.1,
        live_epoch(&documents, &parent_id)
    );
}
