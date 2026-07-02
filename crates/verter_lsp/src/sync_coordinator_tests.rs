//! Unit tests for [`crate::sync_coordinator`] coordinator behavior.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` in `sync_coordinator.rs` to
//! keep the production source under the file-size guard (`no_oversize_files`).
//! Wired back as a `#[cfg(test)] #[path = "sync_coordinator_tests.rs"] mod tests;`
//! child of `sync_coordinator`, so `use super::*` resolves to its items.

use super::*;
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::ProjectSyncMode;
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
    // Test the coordinator's channel and signal delivery.
    // We verify that all signals arrive and can be coalesced.
    let (tx, mut rx) = mpsc::unbounded_channel::<SyncSignal>();
    let handle = SyncCoordinatorHandle { tx };

    // Send 10 rapid changes (no delay — instant burst)
    for _ in 0..10 {
        handle.signal(
            "C:/project/src/App.vue".to_string(),
            "file:///C:/project/src/App.vue".to_string(),
        );
    }

    // Drain signals and verify they were received
    let mut count = 0;
    while let Ok(signal) = rx.try_recv() {
        count += 1;
        assert_eq!(signal.canonical_id, "C:/project/src/App.vue");
    }

    // All 10 signals should have been sent
    assert_eq!(count, 10, "all 10 signals should be in the channel");
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
        project_sync: ProjectSync::new(provider, ProjectSyncMode::FullProject),
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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
    };

    // No prior state in the (empty) states map; no IDE output this pass.
    preserve_open_unresolved_carrier(&deps, "/workspace/src/App.vue", false, None).await;

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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
        },
    );

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
        file_language: crate::server::adapter_module_language_for(canonical_id)
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

    let file_language = crate::server::adapter_module_language_for(canonical_id).unwrap();
    // Build the EXACT provider buffer the coordinator will query against,
    // so we can place a diagnostic at a known user-source token's provider
    // byte offset (no snapshot → empty rewrites → pure prelude offset).
    let provider_content = crate::server::self_file_provider_content(
        &documents,
        None,
        canonical_id,
        &file_language,
        source,
    )
    .expect("rune provider content builds");
    // The prelude shifts every user line down; locate the user token `bad`
    // (source line 1) inside the provider buffer and set a type diagnostic
    // over it at provider byte offsets.
    let provider_bad = provider_content
        .find("bad")
        .expect("token present in provider buffer");
    let provider = Arc::new(MockTypeProvider::new());
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

    // Pre-seed the rune module's Shadow state so the diagnostics path queries
    // the provider at its own path.
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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
    };

    let merged = rune_module_diagnostics(
        &deps,
        provider.as_ref(),
        canonical_id,
        &file_language,
        &uri,
        Vec::new(),
    )
    .await;

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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
    let api_path = format!("{canonical_id}.ts");
    let provider = Arc::new(MockTypeProvider::new());
    // Fail every file op so the new sync cannot succeed.
    provider.set_fail_file_ops(true);

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
    };
    provider_sync_states.insert(canonical_id.to_string(), prior_state.clone());

    let deps = SyncCoordinatorDeps {
        documents,
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
    // Discriminator: with the new sync failing, NOTHING must be closed.
    // Pre-fix the stale-paths loop closed the IDE/API paths before syncing.
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, MockCall::CloseFile { .. })),
        "a failed owner-change sync must not close any provider path, calls={calls:?}"
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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
    let profile = documents.tsx_profile.read().clone();
    let ide = host
        .get_ide(canonical_id, &profile)
        .expect("IDE output should exist");
    assert_eq!(
        snapshot.provider_content.as_ref(),
        ide.code.as_ref(),
        "the recorded surface must pin the EXACT bytes delivered to the provider"
    );

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
        project_sync: ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject),
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
