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
use crate::type_provider::merge;
use crate::type_provider::project_sync::ProjectSync;
use crate::type_provider::traits::TypeProvider;

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
            clear_provider_sync_state(
                &deps.project_sync,
                deps.documents.provider_surfaces(),
                &deps.provider_sync_states,
                canonical_id,
            )
            .await;
        }
        return;
    }

    // Sync IDE (TSX) output to type provider. IDE-sync: drive the IDE/TSX
    // surface (not the runtime `Main`) so a Main-less carrier (Svelte)
    // populates its `CachedTsx` before the `get_ide` read below.
    let profile = deps.documents.tsx_profile.read().clone();
    let _ = tokio::task::block_in_place(|| {
        deps.documents
            .host
            .ensure_ide_compiled(canonical_id, &profile)
    });
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
            clear_provider_sync_state(
                &deps.project_sync,
                deps.documents.provider_surfaces(),
                &deps.provider_sync_states,
                canonical_id,
            )
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
                    // Record a fresh generation pinning the EXACT content just
                    // synced under this virtual path (the single choke point).
                    crate::provider_surface_store::record_carrier_api_surface(
                        deps.documents.provider_surfaces(),
                        Some(&deps.documents),
                        deps.documents.host(),
                        canonical_id,
                        &dts_path,
                        &api.code,
                        api.source_map.as_deref(),
                    );
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
        // `close_stale_paths` retires any closed `Api` surface's active generation
        // in the provider-surface store (forget), so a closed `{carrier}.ts` is
        // never later vouched as current by a cross-file rename.
        close_stale_paths(
            &deps.project_sync,
            deps.documents.provider_surfaces(),
            &genuinely_stale,
        )
        .await;
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
        close_stale_paths(
            &deps.project_sync,
            deps.documents.provider_surfaces(),
            std::slice::from_ref(&dropped),
        )
        .await;
    }
    if let Some(stale) = commit.stale_ide_after_success {
        close_stale_paths(
            &deps.project_sync,
            deps.documents.provider_surfaces(),
            std::slice::from_ref(&stale),
        )
        .await;
    }
}

async fn clear_provider_sync_state(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
) {
    if let Some(state) = remove_sync_state(states, canonical_id) {
        close_stale_paths(sync, provider_surfaces, &state.active_paths()).await;
    }
}

/// Close stale provider paths AND retire any closed `Api` surface's active
/// generation in the provider-surface store.
///
/// A closed `{carrier}.ts` API path is no longer the active synced virtual
/// surface: the store must `forget` it so a later cross-file rename's
/// `current_snapshot()` does not VOUCH the now-closed generation as current
/// (historical snapshots stay valid for any in-flight rename that already
/// captured them — `forget` only retires the active generation). This mirrors the
/// sibling [`crate::background_drain::close_stale_provider_paths`]; the
/// coordinator MUST forget too, or a coordinator-driven close leaves the store
/// vouching a stale surface (the fail-closed invariant relies on this).
async fn close_stale_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    stale_paths: &[(ProviderPathKind, String)],
) {
    for (kind, path) in stale_paths {
        // Retire the closing API surface under a fresh close EPOCH (see the sibling
        // `background_drain::close_stale_provider_paths`): the `Closing` state keeps
        // the path classifying VirtualDrop until the provider close is CONFIRMED, so
        // a failed close cannot let it degrade to NotVirtual and corrupt a real
        // file. Capture the epoch-stamped token so the finalize is scoped to THIS
        // close.
        let close_token = if *kind == ProviderPathKind::Api {
            Some(provider_surfaces.forget(path))
        } else {
            None
        };
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
        match result {
            // Only a CONFIRMED API close finalizes, and only via THIS close's token —
            // a reopen (or newer close) during the await makes the epoch mismatch and
            // the finalize a no-op (the fresh snapshot survives). An error drops the
            // token, leaving the `Closing` state (fail closed). Ide/Shadow have no
            // token.
            Ok(()) => {
                if let Some(token) = close_token {
                    provider_surfaces.finalize_close(token);
                }
            }
            Err(error) => {
                tracing::warn!(
                    "sync_coordinator: failed to close stale provider path {path}: {error}"
                );
            }
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
    let source_li = LineIndex::new(&source, encoding.clone());
    let encoding_for_related = encoding;

    match tp.get_diagnostics(canonical_id).await {
        Ok(type_diags) => {
            // Related-span map-back: a same-file related span maps through the
            // in-context mapper; a real `.ts` related span reads its own source via
            // the VFS reader. A FOREIGN carrier `.tsx` related span needs the
            // server-side external resolver (unavailable on this background path)
            // and drops fail-closed (`external_resolver: None`).
            let carrier_source_exists = |p: &str| deps.documents.host().get_source(p).is_some();
            merge::merge_diagnostics(
                verter_diags,
                type_diags,
                canonical_id,
                &provider_li,
                &mapper,
                &source_li,
                None,
                &carrier_source_exists,
                encoding_for_related,
                &|p: &str| {
                    crate::server::block_in_place_guarded(|| {
                        deps.documents.host().workspace_read().read_file(p)
                    })
                },
            )
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

    // A `.svelte` file whose owner workspace has NO `svelte`
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
                    let carrier_li = LineIndex::new(&carrier_src, encoding.clone());
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
                    // Related-span map-back: same-file related spans map through the
                    // in-context mapper; real `.ts` related spans read their own
                    // source via the VFS reader. A FOREIGN carrier `.tsx` related
                    // span needs the server-side external resolver (unavailable on
                    // this background path) → drops fail-closed (`None`).
                    let carrier_source_exists =
                        |p: &str| deps.documents.host().get_source(p).is_some();
                    merge::merge_diagnostics(
                        verter_diags,
                        type_diags,
                        &tsx_path,
                        &tsx_li,
                        &mapper,
                        &carrier_li,
                        None,
                        &carrier_source_exists,
                        encoding,
                        &|p: &str| {
                            crate::server::block_in_place_guarded(|| {
                                deps.documents.host().workspace_read().read_file(p)
                            })
                        },
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
#[path = "sync_coordinator_tests.rs"]
mod tests;
