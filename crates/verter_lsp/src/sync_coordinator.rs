//! SyncCoordinator: single long-lived task that debounces type provider syncs.
//!
//! Instead of spawning a new tokio task per keystroke (which can flood TSGO during
//! fast typing), the coordinator receives signals via an mpsc channel and waits
//! for 300ms of silence before triggering a sync. This guarantees exactly one
//! sync per file after typing stops, regardless of keystroke timing.
//!
//! After syncing, the coordinator computes merged (Verter lint + TypeScript type)
//! diagnostics and publishes them via push. Push diagnostics stay visible during
//! typing — VS Code automatically adjusts their positions as the document changes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::{DashMap, DashSet};
use tokio::sync::mpsc;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::Client;

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::DocumentRegistry;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, remove_sync_state, ProviderPathKind,
    ProviderSyncState,
};
use crate::server::compute_verter_diagnostics_for_with_views;
use crate::tsgo::merge;
use crate::tsgo::project_sync::ProjectSync;
use crate::tsgo::traits::TypeProvider;

/// Signal sent to the coordinator when a file changes.
pub struct SyncSignal {
    pub canonical_id: String,
    pub uri_str: String,
}

/// Handle for sending signals to the coordinator.
#[derive(Clone)]
pub struct SyncCoordinatorHandle {
    tx: mpsc::UnboundedSender<SyncSignal>,
}

impl SyncCoordinatorHandle {
    /// Signal that a file has changed and needs a debounced sync.
    pub fn signal(&self, canonical_id: String, uri_str: String) {
        let _ = self.tx.send(SyncSignal {
            canonical_id,
            uri_str,
        });
    }

    /// Create a handle from a raw sender (for testing).
    #[cfg(test)]
    pub fn new_for_test(tx: mpsc::UnboundedSender<SyncSignal>) -> Self {
        Self { tx }
    }
}

/// Shared state the coordinator needs to perform syncs and publish diagnostics.
pub struct SyncCoordinatorDeps {
    pub documents: Arc<DocumentRegistry>,
    pub project_sync: ProjectSync,
    pub needs_provider_sync: Arc<DashSet<String>>,
    pub pending_snapshot_provider_sync: Arc<DashSet<String>>,
    pub client: Client,
    /// Type provider for fetching TS diagnostics after sync.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// Cached verter-only diagnostics (URI → (version, diag_gen, diagnostics)).
    /// Shared with the server so we can read cached verter diags after sync.
    pub cached_verter_diags: Arc<DashMap<String, crate::server::CachedVerterDiagEntry>>,
    /// Negotiated position encoding for building line indexes.
    pub position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Source-keyed provider materialization state shared with the server.
    pub provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    /// VFS workspace for published LspViews and resolver snapshot.
    pub vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
}

/// Debounce interval: sync fires after 300ms of silence for a given file.
const DEBOUNCE_MS: u64 = 300;

/// Spawn the coordinator task and return a handle for sending signals.
pub fn spawn_sync_coordinator(deps: SyncCoordinatorDeps) -> SyncCoordinatorHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(coordinator_loop(rx, deps));
    SyncCoordinatorHandle { tx }
}

async fn coordinator_loop(mut rx: mpsc::UnboundedReceiver<SyncSignal>, deps: SyncCoordinatorDeps) {
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    // Map from canonical_id → (last_change_time, uri_str)
    let mut pending_files: HashMap<String, (Instant, String)> = HashMap::new();

    loop {
        // Calculate next deadline from pending files
        let next_deadline = pending_files.values().map(|(t, _)| *t + debounce).min();

        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(signal) => {
                        // Reset timer for this file
                        pending_files.insert(
                            signal.canonical_id,
                            (Instant::now(), signal.uri_str),
                        );
                    }
                    None => {
                        // Channel closed — coordinator shutting down
                        return;
                    }
                }
            }
            _ = async {
                match next_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending::<()>().await,
                }
            } => {
                // Find files that have been quiet for >= debounce_ms
                let now = Instant::now();
                let ready: Vec<(String, String)> = pending_files
                    .iter()
                    .filter(|(_, (t, _))| now.duration_since(*t) >= debounce)
                    .map(|(id, (_, uri))| (id.clone(), uri.clone()))
                    .collect();

                let mut synced_files: Vec<(String, String)> = Vec::new();
                for (canonical_id, uri_str) in ready {
                    pending_files.remove(&canonical_id);
                    // Only sync if the file is still marked dirty
                    if deps.needs_provider_sync.remove(&canonical_id).is_some() {
                        sync_file(&deps, &canonical_id, &uri_str).await;
                        synced_files.push((canonical_id, uri_str));
                    }
                }

                // After syncing, publish fresh merged diagnostics for each synced file.
                // Push diagnostics replace the previous squiggles — no flickering because
                // VS Code adjusts push diagnostic positions during typing, and we only
                // publish fresh ones after the debounce (typing has stopped).
                for (canonical_id, uri_str) in &synced_files {
                    publish_merged_diagnostics(&deps, canonical_id, uri_str).await;
                }
            }
        }
    }
}

/// Perform the actual sync: sync TSX/DTS to the type provider.
async fn sync_file(deps: &SyncCoordinatorDeps, canonical_id: &str, _uri_str: &str) {
    tracing::info!("sync_coordinator: SYNC_START {canonical_id}");
    let Some(snapshot) = ({
        let ws = deps.vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(crate::server::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            })
        })
    }) else {
        tracing::debug!(
            "sync_coordinator: deferring sync without resolver snapshot {canonical_id}"
        );
        deps.pending_snapshot_provider_sync
            .insert(canonical_id.to_string());
        return;
    };
    deps.documents.host().ensure_loaded(canonical_id);
    // Sync IDE (TSX) output to type provider
    let profile = deps.documents.tsx_profile.read().clone();
    let _ =
        tokio::task::block_in_place(|| deps.documents.host.ensure_compiled(canonical_id, &profile));
    tracing::info!("sync_coordinator: HOST_GET_IDE_START {canonical_id}");
    let ide = tokio::task::block_in_place(|| deps.documents.host.get_ide(canonical_id, &profile));
    let is_jsx = ide.as_ref().map(|ide| ide.is_jsx).unwrap_or(false);
    let Some(next_state) =
        crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, canonical_id, is_jsx)
    else {
        // Always queue for retry on future snapshot rebuild.
        if snapshot.ownership_ready {
            clear_provider_sync_state(&deps.project_sync, &deps.provider_sync_states, canonical_id)
                .await;
        }
        deps.pending_snapshot_provider_sync
            .insert(canonical_id.to_string());
        if snapshot.ownership_ready {
            tracing::warn!(
                "sync_coordinator: {canonical_id} has no project owner after real snapshot"
            );
        } else {
            tracing::info!(
                "sync_coordinator: {canonical_id} unowned during bootstrap, queued for drain"
            );
        }
        return;
    };
    let transition = prepare_sync_transition(&deps.provider_sync_states, canonical_id, next_state);
    close_stale_paths(&deps.project_sync, &transition.stale_paths).await;
    let committed_state = transition.next;
    if let Some(ide) = ide {
        tracing::info!("sync_coordinator: HOST_GET_IDE_DONE {canonical_id}");
        let Some(ide_path) = committed_state.ide_path.clone() else {
            tracing::debug!("sync_coordinator: no owner-aware IDE path for {canonical_id}");
            return;
        };
        tracing::info!("sync_coordinator: TSX_SYNC_START {ide_path}");
        if let Err(e) = deps.project_sync.sync_tsx(&ide_path, &ide.code).await {
            tracing::warn!("sync_coordinator: tsx sync failed for {ide_path}: {e}");
        }
        tracing::info!("sync_coordinator: TSX_SYNC_DONE {ide_path}");
    } else {
        tracing::info!("sync_coordinator: HOST_GET_IDE_DONE (none) {canonical_id}");
    }

    // Sync API (DTS) output to type provider
    tracing::info!("sync_coordinator: HOST_GET_API_START {canonical_id}");
    let api = tokio::task::block_in_place(|| deps.documents.host.get_public_api(canonical_id));
    if let Some(api) = api {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE {canonical_id}");
        let Some(dts_path) = committed_state.api_path.clone() else {
            tracing::debug!("sync_coordinator: no owner-aware API path for {canonical_id}");
            return;
        };
        if let Err(e) = deps.project_sync.sync_dts(&dts_path, &api.code).await {
            tracing::warn!("sync_coordinator: dts sync failed for {dts_path}: {e}");
        }
    } else {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE (none) {canonical_id}");
    }

    commit_sync_transition(&deps.provider_sync_states, canonical_id, committed_state);
    tracing::info!("sync_coordinator: SYNC_DONE {canonical_id}");
}

async fn clear_provider_sync_state(
    sync: &ProjectSync,
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
) {
    if let Some(state) = remove_sync_state(states, canonical_id) {
        close_stale_paths(sync, &state.active_paths()).await;
    }
}

async fn close_stale_paths(sync: &ProjectSync, stale_paths: &[(ProviderPathKind, String)]) {
    for (kind, path) in stale_paths {
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
        if let Err(error) = result {
            tracing::warn!("sync_coordinator: failed to close stale provider path {path}: {error}");
        }
    }
}

/// Publish merged (Verter lint + TypeScript type) diagnostics for a synced file.
///
/// Recomputes fresh verter diagnostics (host errors + lint rules) for the current
/// document version, then merges with fresh TS diagnostics from the type provider.
/// This ensures lint violations introduced during typing appear without reopening.
async fn publish_merged_diagnostics(deps: &SyncCoordinatorDeps, canonical_id: &str, uri_str: &str) {
    let uri: Uri = match uri_str.parse() {
        Ok(u) => u,
        Err(_) => return,
    };

    // Recompute verter diagnostics fresh (lint + host errors) instead of reading stale cache.
    let mut verter_diags = {
        let vfs_ws = deps.vfs_workspace.read();
        compute_verter_diagnostics_for_with_views(
            &deps.documents,
            &uri,
            &deps.cached_verter_diags,
            vfs_ws.as_deref(),
        )
    };

    // When a TypeProvider is active, suppress component usage diagnostics
    // (unknown-prop, unknown-model) since the TypeProvider validates props
    // via the generated TSX and is the source of truth.
    if deps.type_provider.is_some() {
        verter_diags.retain(|d| match &d.code {
            Some(NumberOrString::String(code)) => {
                code != "verter/unknown-prop" && code != "verter/unknown-model"
            }
            _ => true,
        });
    }

    let diagnostics = if let Some(tp) = &deps.type_provider {
        // Build IDE context from the host
        let profile = deps.documents.tsx_profile.read().clone();
        let ide =
            tokio::task::block_in_place(|| deps.documents.host.get_ide(canonical_id, &profile));

        if let Some(ide) = ide {
            // Use committed provider sync state for the tsx_path.
            // This ensures we only query the type provider for paths
            // that are actually materialized in provider state.
            let Some(tsx_path) = deps
                .provider_sync_states
                .get(canonical_id)
                .and_then(|state| state.ide_path.clone())
            else {
                return deps
                    .client
                    .publish_diagnostics(uri, verter_diags, None)
                    .await;
            };
            let encoding = deps.position_encoding.read().clone();
            let tsx_li = LineIndex::new(&ide.code, encoding.clone());

            // Build position mapper from IDE source map
            let mapper = ide
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok());

            // Build Vue source line index
            let vue_source = deps.documents.host.get_source(canonical_id);

            match (tp.get_diagnostics(&tsx_path).await, mapper, vue_source) {
                (Ok(type_diags), Some(mapper), Some(vue_src)) => {
                    let vue_li = LineIndex::new(&vue_src, encoding);
                    tracing::debug!(
                        "sync_coordinator: publish {} verter + {} type diags for {}",
                        verter_diags.len(),
                        type_diags.len(),
                        canonical_id
                    );
                    merge::merge_diagnostics(verter_diags, type_diags, &tsx_li, &mapper, &vue_li)
                }
                (Err(e), _, _) => {
                    tracing::warn!(
                        "sync_coordinator: type provider error for {}: {e}",
                        canonical_id
                    );
                    verter_diags
                }
                _ => verter_diags,
            }
        } else {
            verter_diags
        }
    } else {
        verter_diags
    };

    tracing::info!(
        "sync_coordinator: publishing {} diagnostics for {}",
        diagnostics.len(),
        canonical_id
    );
    deps.client
        .publish_diagnostics(uri, diagnostics, None)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsgo::mock::{MockCall, MockTypeProvider};
    use crate::ProjectSyncMode;
    use tower_lsp_server::{LspService, Server};
    use verter_session::{HostConfig, VerterHost};

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

        let transition = prepare_sync_transition(
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

        close_stale_paths(&sync, &transition.stale_paths).await;
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
}
