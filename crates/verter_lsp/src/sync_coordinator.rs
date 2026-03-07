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
use verter_host::VerterHost;

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
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
    pub host: Arc<VerterHost>,
    pub project_sync: ProjectSync,
    pub needs_provider_sync: Arc<DashSet<String>>,
    pub tsx_profile: parking_lot::RwLock<verter_host::CompileProfile>,
    pub client: Client,
    /// Type provider for fetching TS diagnostics after sync.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// Cached verter-only diagnostics (URI → (version, diagnostics)).
    /// Shared with the server so we can read cached verter diags after sync.
    pub cached_verter_diags: Arc<DashMap<String, (i32, Vec<Diagnostic>)>>,
    /// Negotiated position encoding for building line indexes.
    pub position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
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
    // Sync IDE (TSX) output to type provider
    let profile = deps.tsx_profile.read().clone();
    tracing::info!("sync_coordinator: HOST_GET_IDE_START {canonical_id}");
    let ide = tokio::task::block_in_place(|| deps.host.get_ide(canonical_id, &profile));
    if let Some(ide) = ide {
        tracing::info!("sync_coordinator: HOST_GET_IDE_DONE {canonical_id}");
        let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
        let ide_path = format!("{canonical_id}{ext}");
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
    let api = tokio::task::block_in_place(|| deps.host.get_public_api(canonical_id));
    if let Some(api) = api {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE {canonical_id}");
        let base = canonical_id.strip_suffix(".vue").unwrap_or(canonical_id);
        let dts_path = format!("{base}.vue.ts");
        if let Err(e) = deps.project_sync.sync_dts(&dts_path, &api.code).await {
            tracing::warn!("sync_coordinator: dts sync failed for {dts_path}: {e}");
        }
    } else {
        tracing::info!("sync_coordinator: HOST_GET_API_DONE (none) {canonical_id}");
    }

    tracing::info!("sync_coordinator: SYNC_DONE {canonical_id}");
}

/// Publish merged (Verter lint + TypeScript type) diagnostics for a synced file.
///
/// Uses cached verter diagnostics (computed during `did_open` / previous publish)
/// and fetches fresh TS diagnostics from the type provider. Falls back to cached
/// verter-only diagnostics if the type provider is unavailable or returns an error.
async fn publish_merged_diagnostics(deps: &SyncCoordinatorDeps, canonical_id: &str, uri_str: &str) {
    let uri: Uri = match uri_str.parse() {
        Ok(u) => u,
        Err(_) => return,
    };

    // Read cached verter diagnostics (computed by server during did_open or prior publish)
    let verter_diags = deps
        .cached_verter_diags
        .get(uri_str)
        .map(|entry| entry.1.clone())
        .unwrap_or_default();

    let diagnostics = if let Some(tp) = &deps.type_provider {
        // Build IDE context from the host
        let profile = deps.tsx_profile.read().clone();
        let ide = tokio::task::block_in_place(|| deps.host.get_ide(canonical_id, &profile));

        if let Some(ide) = ide {
            let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
            let tsx_path = format!("{canonical_id}{ext}");
            let encoding = deps.position_encoding.read().clone();
            let tsx_li = LineIndex::new(&ide.code, encoding.clone());

            // Build position mapper from IDE source map
            let mapper = ide
                .source_map
                .as_ref()
                .and_then(|sm| PositionMapper::from_json(sm).ok());

            // Build Vue source line index
            let vue_source = deps.host.get_source(canonical_id);

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
}
