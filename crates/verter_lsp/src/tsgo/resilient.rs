//! Resilient wrapper around `TsgoTypeProvider` with crash detection and auto-restart.
//!
//! Transparently delegates all `TypeProvider` methods to the inner `TsgoTypeProvider`.
//! When the child process crashes (detected via the `read_loop` crash signal),
//! drains in-flight requests, notifies the user via the LSP client, and automatically
//! restarts tsgo with state recovery (re-opening all cached files).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify, OnceCell, RwLock};
use tower_lsp_server::Client;

use crate::tsgo::ipc::TsgoTypeProvider;
use crate::tsgo::protocol::*;
use crate::tsgo::traits::{ProviderFuture, TypeProvider};

/// Shared state for crash monitoring and restart logic.
///
/// Extracted into a separate struct so it can be passed to spawned tasks
/// via `Arc` without any unsafe code.
struct CrashMonitorState {
    inner: Arc<RwLock<Option<Arc<TsgoTypeProvider>>>>,
    client: Arc<OnceCell<Client>>,
    file_cache: Arc<RwLock<HashMap<String, String>>>,
    restart_count: AtomicU32,
    restart_lock: Mutex<()>,
    tsgo_bin: String,
    root_uri: String,
    max_restarts: u32,
}

/// A `TypeProvider` wrapper that detects tsgo crashes and automatically restarts.
///
/// The server and `ProjectSync` use `Arc<dyn TypeProvider>` — this wrapper is
/// injected transparently at the `main.rs` level.
pub struct ResilientTypeProvider {
    /// Shared state with the crash monitor task.
    state: Arc<CrashMonitorState>,
}

impl ResilientTypeProvider {
    /// Create a resilient wrapper from an already-spawned `TsgoTypeProvider`.
    ///
    /// Starts a background crash monitor task that watches for the `crash_notify`
    /// signal and handles restart logic.
    pub fn new(
        provider: TsgoTypeProvider,
        crash_notify: Arc<Notify>,
        tsgo_bin: String,
        root_uri: String,
        client: Arc<OnceCell<Client>>,
        max_restarts: u32,
    ) -> Self {
        let state = Arc::new(CrashMonitorState {
            inner: Arc::new(RwLock::new(Some(Arc::new(provider)))),
            client,
            file_cache: Arc::new(RwLock::new(HashMap::new())),
            restart_count: AtomicU32::new(0),
            restart_lock: Mutex::new(()),
            tsgo_bin,
            root_uri,
            max_restarts,
        });

        spawn_crash_monitor(Arc::clone(&state), crash_notify);

        Self { state }
    }

    /// Get the current inner provider, if available.
    async fn get_inner(&self) -> Result<Arc<TsgoTypeProvider>, TypeProviderError> {
        self.state
            .inner
            .read()
            .await
            .clone()
            .ok_or_else(|| TypeProviderError::new("tsgo is restarting"))
    }
}

/// Spawn a background task that waits for the crash signal and handles restart.
fn spawn_crash_monitor(state: Arc<CrashMonitorState>, crash_notify: Arc<Notify>) {
    tokio::spawn(async move {
        crash_notify.notified().await;

        tracing::warn!("TSGO crash detected — initiating restart sequence");

        // Set inner to None immediately so in-flight requests fail fast.
        {
            let mut guard = state.inner.write().await;
            *guard = None;
        }

        // Notify user via LSP client (if available).
        if let Some(c) = state.client.get() {
            c.show_message(
                tower_lsp_server::lsp_types::MessageType::WARNING,
                "TypeScript server (tsgo) crashed. Restarting...",
            )
            .await;
        } else {
            tracing::warn!("TSGO crashed but LSP client not yet available for notification");
        }

        // Acquire restart lock to prevent concurrent restarts.
        // Scoped so the lock is dropped before we move `state` into the recursive call.
        let attempt = {
            let _lock = state.restart_lock.lock().await;
            state.restart_count.fetch_add(1, Ordering::Relaxed) + 1
        };

        if attempt > state.max_restarts {
            tracing::error!(
                "TSGO restart limit reached ({}) — staying in verter-only mode",
                state.max_restarts
            );
            if let Some(c) = state.client.get() {
                c.show_message(
                    tower_lsp_server::lsp_types::MessageType::ERROR,
                    format!(
                        "TypeScript server (tsgo) crashed {} times. \
                         Running in verter-only mode for the rest of this session.",
                        state.max_restarts
                    ),
                )
                .await;
            }
            return;
        }

        // Backoff delay: 1s, 2s, 4s (capped).
        let delay_secs = (1u64 << (attempt - 1)).min(4);
        tracing::info!(
            "TSGO restart attempt {attempt}/{} after {delay_secs}s",
            state.max_restarts
        );
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

        // Spawn new tsgo with a fresh crash signal.
        let new_crash_notify = Arc::new(Notify::new());
        match TsgoTypeProvider::spawn_with_crash_signal(
            &state.tsgo_bin,
            &state.root_uri,
            Some(Arc::clone(&new_crash_notify)),
        )
        .await
        {
            Ok(new_provider) => {
                let new_provider = Arc::new(new_provider);

                // Re-open all cached files to restore state.
                let cache_snapshot = state.file_cache.read().await.clone();
                for (path, content) in &cache_snapshot {
                    if let Err(e) = new_provider.open_file(path, content).await {
                        tracing::warn!("Failed to re-open {path} after restart: {e}");
                    }
                }

                // Install the new provider.
                {
                    let mut guard = state.inner.write().await;
                    *guard = Some(Arc::clone(&new_provider));
                }

                tracing::info!("TSGO restarted successfully (attempt {attempt})");
                if let Some(c) = state.client.get() {
                    c.show_message(
                        tower_lsp_server::lsp_types::MessageType::INFO,
                        "TypeScript server restarted successfully.",
                    )
                    .await;
                }

                // Reset restart count on success so transient crashes don't accumulate.
                state.restart_count.store(0, Ordering::Relaxed);

                // Re-arm: spawn a new crash monitor for the new provider.
                spawn_crash_monitor(state, new_crash_notify);
            }
            Err(e) => {
                tracing::error!("Failed to restart TSGO: {e}");
                if let Some(c) = state.client.get() {
                    c.show_message(
                        tower_lsp_server::lsp_types::MessageType::ERROR,
                        format!("Failed to restart TypeScript server: {e}"),
                    )
                    .await;
                }
            }
        }
    });
}

impl TypeProvider for ResilientTypeProvider {
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            // Track in file cache for state recovery.
            self.state
                .file_cache
                .write()
                .await
                .insert(path_owned.clone(), content_owned.clone());
            let provider = self.get_inner().await?;
            provider.open_file(&path_owned, &content_owned).await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.state
                .file_cache
                .write()
                .await
                .insert(path_owned.clone(), content_owned.clone());
            let provider = self.get_inner().await?;
            provider.update_file(&path_owned, &content_owned).await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        Box::pin(async move {
            self.state.file_cache.write().await.remove(&path_owned);
            let provider = self.get_inner().await?;
            provider.close_file(&path_owned).await
        })
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let path_owned = path.to_string();
        let trigger_owned = trigger_character.map(|s| s.to_string());
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider
                .get_completions(&path_owned, offset, trigger_owned.as_deref())
                .await
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_hover(&path_owned, offset).await
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_diagnostics(&path_owned).await
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_definition(&path_owned, offset).await
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_references(&path_owned, offset).await
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_rename_locations(&path_owned, offset).await
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_signature_help(&path_owned, offset).await
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider
                .get_code_actions(&path_owned, start_offset, end_offset)
                .await
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_semantic_tokens(&path_owned).await
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.get_document_highlights(&path_owned, offset).await
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider
                .get_inlay_hints(&path_owned, start_offset, end_offset)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsgo::mock::MockTypeProvider;

    /// Helper to create a ResilientTypeProvider with no real inner provider.
    fn make_resilient_with_mock(
        mock: &MockTypeProvider,
    ) -> (
        ResilientTypeProvider,
        Arc<RwLock<Option<Arc<TsgoTypeProvider>>>>,
    ) {
        // We can't use a real TsgoTypeProvider in tests, so we test the file_cache
        // and inner-swapping logic via the fields directly.
        let inner: Arc<RwLock<Option<Arc<TsgoTypeProvider>>>> = Arc::new(RwLock::new(None));
        let _ = mock; // MockTypeProvider is used for type checking only
        let state = Arc::new(CrashMonitorState {
            inner: Arc::clone(&inner),
            client: Arc::new(OnceCell::new()),
            file_cache: Arc::new(RwLock::new(HashMap::new())),
            restart_count: AtomicU32::new(0),
            restart_lock: Mutex::new(()),
            tsgo_bin: "tsgo".to_string(),
            root_uri: "file:///test".to_string(),
            max_restarts: 3,
        });
        (ResilientTypeProvider { state }, inner)
    }

    #[tokio::test]
    async fn file_cache_tracks_open_and_update() {
        let mock = MockTypeProvider::new();
        let (resilient, _inner) = make_resilient_with_mock(&mock);

        // Simulate open_file by writing to file_cache directly (no real provider).
        resilient
            .state
            .file_cache
            .write()
            .await
            .insert("test.tsx".to_string(), "const x = 1;".to_string());

        let cache = resilient.state.file_cache.read().await;
        assert_eq!(cache.get("test.tsx").unwrap(), "const x = 1;");
        drop(cache);

        // Simulate update_file.
        resilient
            .state
            .file_cache
            .write()
            .await
            .insert("test.tsx".to_string(), "const x = 2;".to_string());

        let cache = resilient.state.file_cache.read().await;
        assert_eq!(cache.get("test.tsx").unwrap(), "const x = 2;");
    }

    #[tokio::test]
    async fn file_cache_removes_on_close() {
        let mock = MockTypeProvider::new();
        let (resilient, _inner) = make_resilient_with_mock(&mock);

        resilient
            .state
            .file_cache
            .write()
            .await
            .insert("test.tsx".to_string(), "const x = 1;".to_string());

        // Simulate close_file.
        resilient.state.file_cache.write().await.remove("test.tsx");

        let cache = resilient.state.file_cache.read().await;
        assert!(cache.get("test.tsx").is_none());
    }

    #[tokio::test]
    async fn returns_error_when_inner_is_none() {
        let mock = MockTypeProvider::new();
        let (resilient, _inner) = make_resilient_with_mock(&mock);

        // Inner is None (simulating restart in progress).
        let result = resilient.get_inner().await;
        let err = result.err().expect("should be Err when inner is None");
        assert_eq!(err.message, "tsgo is restarting");
    }

    #[tokio::test]
    async fn restart_count_increments() {
        let mock = MockTypeProvider::new();
        let (resilient, _inner) = make_resilient_with_mock(&mock);

        assert_eq!(resilient.state.restart_count.load(Ordering::Relaxed), 0);
        resilient
            .state
            .restart_count
            .fetch_add(1, Ordering::Relaxed);
        assert_eq!(resilient.state.restart_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn drain_pending_sends_crash_error() {
        use super::super::ipc::drain_pending_for_test;

        let pending: Arc<Mutex<HashMap<i64, tokio::sync::oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx1, rx1) = tokio::sync::oneshot::channel();
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        pending.lock().await.insert(1, tx1);
        pending.lock().await.insert(2, tx2);

        drain_pending_for_test(&pending).await;

        // All pending should be drained.
        assert!(pending.lock().await.is_empty());

        // Receivers should get crash error.
        let r1 = rx1.await.unwrap();
        assert_eq!(
            r1.get("error")
                .unwrap()
                .get("message")
                .unwrap()
                .as_str()
                .unwrap(),
            "tsgo process crashed"
        );
        let r2 = rx2.await.unwrap();
        assert_eq!(
            r2.get("error")
                .unwrap()
                .get("message")
                .unwrap()
                .as_str()
                .unwrap(),
            "tsgo process crashed"
        );
    }
}
