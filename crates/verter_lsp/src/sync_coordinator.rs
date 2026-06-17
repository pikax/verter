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
    commit_sync_transition, genuinely_stale_after_sync, open_unresolved_carrier_commit,
    open_unresolved_carrier_state, prepare_sync_transition, remove_sync_state,
    revert_unsynced_kinds, ProviderPathKind, ProviderSyncState,
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

    // A self-file rune module (`.svelte.ts` / `.svelte.js`) is NOT a carrier —
    // it serves its OWN-path provider buffer (`<rune prelude> + <rewritten
    // module bytes>`), has no IDE TSX, and its provider state lives in the
    // Shadow slot keyed at its own canonical path. Route it through the SHARED
    // self-file shadow-sync path (the SAME one the editor ingress uses) so the
    // debounced tick (a) uses the generalized projection for diagnostics and (b)
    // never clobbers the Shadow state via the carrier-miss
    // `preserve_open_unresolved_carrier`, which would overwrite it with an
    // IDE-path state and break did_close cleanup.
    if let Some(file_language) = crate::server::adapter_module_language_for(canonical_id) {
        if let Some(uri) = deps.documents.canonical_id_to_uri(canonical_id) {
            crate::server::sync_self_file_shadow_state(
                &deps.documents,
                &deps.project_sync,
                &deps.provider_sync_states,
                Some(&snapshot),
                &uri,
                canonical_id,
                &file_language,
            )
            .await;
        } else if snapshot.ownership_ready {
            // A genuinely non-open rune module is removed once ready.
            clear_provider_sync_state(&deps.project_sync, &deps.provider_sync_states, canonical_id)
                .await;
        }
        return;
    }

    // Sync IDE (TSX) output to type provider
    let profile = deps.documents.tsx_profile.read().clone();
    let _ =
        tokio::task::block_in_place(|| deps.documents.host.ensure_compiled(canonical_id, &profile));
    tracing::info!("sync_coordinator: HOST_GET_IDE_START {canonical_id}");
    let ide = tokio::task::block_in_place(|| deps.documents.host.get_ide(canonical_id, &profile));
    let is_jsx = ide.as_ref().map(|ide| ide.is_jsx).unwrap_or(false);
    let Some(next_state) = crate::provider_sync::carrier_sync_state_for_source(
        &snapshot.resolver,
        canonical_id,
        is_jsx,
    ) else {
        // No owner resolved. Editor-liveness invariant: the coordinator syncs
        // OPEN documents (signalled from did_change). An OPEN `.vue` must keep
        // its TSX live as Unresolved open-document state — NEVER clear+close.
        // Only a genuinely non-open file is removed (and only once ready).
        if deps.documents.canonical_id_to_uri(canonical_id).is_some() {
            preserve_open_unresolved_carrier(deps, canonical_id, is_jsx, ide.as_ref()).await;
        } else if snapshot.ownership_ready {
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
    // Close-AFTER-successful-sync (per-kind, skip-active): capture stale + prior
    // state, sync each kind, then commit and close only genuinely-stale paths.
    // The coordinator can touch an OPEN file, so a failed replacement sync must
    // never close the live path nor commit an unsynced path.
    let previous_state = deps
        .provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone());
    let transition = prepare_sync_transition(&deps.provider_sync_states, canonical_id, next_state);
    let stale_paths = transition.stale_paths;
    let mut committed_state = transition.next;
    let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

    if let Some(ide) = ide {
        tracing::info!("sync_coordinator: HOST_GET_IDE_DONE {canonical_id}");
        if let Some(ide_path) = committed_state.ide_path.clone() {
            tracing::info!("sync_coordinator: TSX_SYNC_START {ide_path}");
            let result = if committed_state.ide_background_loaded {
                deps.project_sync.sync_tsx(&ide_path, &ide.code).await
            } else {
                deps.project_sync.open_tsx(&ide_path, &ide.code).await
            };
            match result {
                Ok(()) => {
                    committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                    synced_kinds.push(ProviderPathKind::Ide);
                }
                Err(e) => tracing::warn!("sync_coordinator: tsx sync failed for {ide_path}: {e}"),
            }
            tracing::info!("sync_coordinator: TSX_SYNC_DONE {ide_path}");
        } else {
            tracing::debug!("sync_coordinator: no owner-aware IDE path for {canonical_id}");
        }
    } else {
        tracing::info!("sync_coordinator: HOST_GET_IDE_DONE (none) {canonical_id}");
    }

    // Sync API (DTS) output to type provider
    tracing::info!("sync_coordinator: HOST_GET_API_START {canonical_id}");
    let api = tokio::task::block_in_place(|| deps.documents.host.get_public_api(canonical_id));
    if let Some(api) = api {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE {canonical_id}");
        if let Some(dts_path) = committed_state.api_path.clone() {
            let result = if committed_state.api_background_loaded {
                deps.project_sync.sync_dts(&dts_path, &api.code).await
            } else {
                deps.project_sync.open_dts(&dts_path, &api.code).await
            };
            match result {
                Ok(()) => {
                    committed_state.set_background_loaded(ProviderPathKind::Api, true);
                    synced_kinds.push(ProviderPathKind::Api);
                }
                Err(e) => tracing::warn!("sync_coordinator: dts sync failed for {dts_path}: {e}"),
            }
        } else {
            tracing::debug!("sync_coordinator: no owner-aware API path for {canonical_id}");
        }
    } else {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE (none) {canonical_id}");
    }

    if !synced_kinds.is_empty() {
        revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
        let genuinely_stale =
            genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
        commit_sync_transition(&deps.provider_sync_states, canonical_id, committed_state);
        close_stale_paths(&deps.project_sync, &genuinely_stale).await;
    }
    // On total failure nothing is committed and nothing is closed: the previous
    // state + provider paths are retained intact.
    tracing::info!("sync_coordinator: SYNC_DONE {canonical_id}");
}

/// Preserve (or create) an OPEN Vue document's unresolved provider state when
/// the coordinator's ready snapshot resolves no owner, keeping its IDE TSX live.
///
/// Editor-liveness invariant: builds the commit state through the shared
/// [`open_unresolved_carrier_state`] primitive (forces `Unresolved`, preserves the
/// owner-independent live IDE path, drops the owner-derived API path), syncs the
/// IDE TSX when fresh code is available, and commits. It NEVER removes the state
/// or closes the TSX.
async fn preserve_open_unresolved_carrier(
    deps: &SyncCoordinatorDeps,
    canonical_id: &str,
    is_jsx: bool,
    ide: Option<&verter_session::IdeResponse>,
) {
    let previous = deps
        .provider_sync_states
        .get(canonical_id)
        .map(|entry| entry.clone());
    // The DESIRED Unresolved target: owner-independent desired-extension IDE
    // path + the open-vs-update syncability hint. Binding forced `Unresolved`,
    // owner-derived API dropped.
    let target = open_unresolved_carrier_state(previous.as_ref(), canonical_id, is_jsx);

    // Attempt the desired IDE sync when fresh code is available (update-in-place
    // when the desired path is already live, else first-open).
    let mut ide_synced = false;
    if let (Some(ide), Some(ide_path)) = (ide, target.ide_path.clone()) {
        let result = if target.ide_background_loaded {
            deps.project_sync.sync_tsx(&ide_path, &ide.code).await
        } else {
            deps.project_sync.open_tsx(&ide_path, &ide.code).await
        };
        match result {
            Ok(()) => ide_synced = true,
            Err(error) => tracing::warn!(
                "sync_coordinator: failed to sync open unresolved IDE path {ide_path}: {error}"
            ),
        }
    }

    // Build the committed state + close targets through the SAME per-kind
    // discipline the owner-resolved path uses: a non-synced IDE kind RETAINS the
    // prior LIVE path (never dropped to a dead/None path while the prior is still
    // open — rows 7 & 9), the owner-derived API is dropped+closed unconditionally,
    // and the orphaned prior IDE path is closed ONLY after a successful flip.
    let commit = open_unresolved_carrier_commit(previous.as_ref(), target, ide_synced);
    commit_sync_transition(&deps.provider_sync_states, canonical_id, commit.committed);
    if let Some(dropped) = commit.dropped_api {
        close_stale_paths(&deps.project_sync, std::slice::from_ref(&dropped)).await;
    }
    if let Some(stale) = commit.stale_ide_after_success {
        close_stale_paths(&deps.project_sync, std::slice::from_ref(&stale)).await;
    }
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

/// Merge a rune module's debounced diagnostics through the generalized
/// SELF-FILE projection: query the type provider at the module's OWN canonical
/// path (the Shadow provider buffer `<rune prelude> + <rewritten module
/// bytes>`), then map each type diagnostic back to the user-source position
/// through the document's rewrite-aware self-file mapper (prelude offset +
/// per-line rewrite delta). Falls back to the verter diagnostics alone when the
/// provider has no committed Shadow path, the mapper/content is unavailable, or
/// the provider errors.
async fn rune_module_diagnostics(
    deps: &SyncCoordinatorDeps,
    tp: &dyn TypeProvider,
    canonical_id: &str,
    file_language: &verter_session::FileLanguage,
    uri: &Uri,
    verter_diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    // Only query the provider when the rune module's Shadow buffer is actually
    // committed at its own path (avoids querying an unmaterialized path).
    let has_shadow = deps
        .provider_sync_states
        .get(canonical_id)
        .is_some_and(|state| state.shadow_path.as_deref() == Some(canonical_id));
    if !has_shadow {
        return verter_diags;
    }

    let Some(source) = deps.documents.get(uri).map(|d| d.source.clone()) else {
        return verter_diags;
    };
    let Some(mapper) = deps.documents.get_position_mapper(uri) else {
        return verter_diags;
    };
    let snapshot = {
        let ws = deps.vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(crate::server::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            })
        })
    };
    let Some(provider_content) = crate::server::self_file_provider_content(
        &deps.documents,
        snapshot.as_ref(),
        canonical_id,
        file_language,
        &source,
    ) else {
        return verter_diags;
    };

    let encoding = deps.position_encoding.read().clone();
    let provider_li = LineIndex::new(&provider_content, encoding.clone());
    let source_li = LineIndex::new(&source, encoding);

    match tp.get_diagnostics(canonical_id).await {
        Ok(type_diags) => {
            merge::merge_diagnostics(verter_diags, type_diags, &provider_li, &mapper, &source_li)
        }
        Err(error) => {
            tracing::warn!(
                "sync_coordinator: type provider error for rune module {canonical_id}: {error}"
            );
            verter_diags
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

    // D-ae(d) / D-ay: a `.svelte` file whose owner workspace has NO `svelte`
    // install fails CLOSED (module-not-found on the shim's `svelte` import) —
    // surface the typed `svelte-package-missing` diagnostic on the source file
    // so the failure is explained, not just a raw TS module-not-found. The
    // owner root resolves through the published resolver snapshot.
    if crate::server::carrier_language_for(canonical_id).is_some_and(|l| l.is_svelte()) {
        let owner_root = {
            let ws = deps.vfs_workspace.read();
            ws.as_ref()
                .and_then(|ws| ws.load_published())
                .and_then(|p| {
                    p.snapshot
                        .resolver
                        .owner_for_file(canonical_id)
                        .map(|o| o.root.clone())
                })
        };
        if let Some(owner_root) = owner_root {
            let source = deps
                .documents
                .host
                .get_source(canonical_id)
                .unwrap_or_default();
            if let Some(diag) = crate::svelte_assets::svelte_package_missing_diagnostic(
                canonical_id,
                &owner_root,
                &source,
            ) {
                verter_diags.push(diag);
            }
        }
    }

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

    // A self-file rune module (`.svelte.ts` / `.svelte.js`) has NO IDE TSX —
    // its provider buffer is served from its OWN canonical path (`<rune prelude>
    // + <rewritten module bytes>`). Route its debounced diagnostics through the
    // generalized self-file projection (the document's rewrite-aware mapper +
    // own-path provider buffer), so type diagnostics land at the correctly
    // offset source position — NOT through the carrier IDE-source-map path
    // below (which requires an `ide_path` the rune module never has).
    if let Some(tp) = &deps.type_provider {
        if let Some(file_language) = crate::server::adapter_module_language_for(canonical_id) {
            let diagnostics = rune_module_diagnostics(
                deps,
                tp.as_ref(),
                canonical_id,
                &file_language,
                &uri,
                verter_diags,
            )
            .await;
            return deps
                .client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
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
            let carrier_source = deps.documents.host.get_source(canonical_id);

            match (tp.get_diagnostics(&tsx_path).await, mapper, carrier_source) {
                (Ok(type_diags), Some(mapper), Some(carrier_src)) => {
                    let carrier_li = LineIndex::new(&carrier_src, encoding);
                    let mapper =
                        crate::documents::provider_projection::ProviderPositionMapper::source_map(
                            mapper,
                        );
                    tracing::debug!(
                        "sync_coordinator: publish {} verter + {} type diags for {}",
                        verter_diags.len(),
                        type_diags.len(),
                        canonical_id
                    );
                    merge::merge_diagnostics(
                        verter_diags,
                        type_diags,
                        &tsx_li,
                        &mapper,
                        &carrier_li,
                    )
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
                ide_background_loaded: true,
                api_background_loaded: true,
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
        use crate::tsgo::protocol::{TypeDiagnostic, TypeDiagnosticSeverity};

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
            ide_background_loaded: true,
            api_background_loaded: true,
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
}
