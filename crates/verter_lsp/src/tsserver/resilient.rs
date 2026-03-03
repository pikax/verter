//! Resilient wrapper around `TsserverTypeProvider` with crash detection and auto-restart.
//!
//! Same pattern as `tsgo::resilient::ResilientTypeProvider` but wraps a tsserver process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify, OnceCell, RwLock};
use tower_lsp_server::Client;

use crate::tsgo::protocol::*;
use crate::tsgo::traits::{ProviderFuture, TypeProvider};
use crate::tsserver::ipc::TsserverTypeProvider;

struct CrashMonitorState {
    inner: Arc<RwLock<Option<Arc<TsserverTypeProvider>>>>,
    client: Arc<OnceCell<Client>>,
    file_cache: Arc<RwLock<HashMap<String, String>>>,
    restart_count: AtomicU32,
    restart_lock: Mutex<()>,
    node_path: String,
    tsserver_path: String,
    workspace_root: String,
    plugin_path: Option<String>,
    max_restarts: u32,
}

pub struct ResilientTsserverProvider {
    state: Arc<CrashMonitorState>,
}

impl ResilientTsserverProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: TsserverTypeProvider,
        crash_notify: Arc<Notify>,
        node_path: String,
        tsserver_path: String,
        workspace_root: String,
        plugin_path: Option<String>,
        client: Arc<OnceCell<Client>>,
        max_restarts: u32,
    ) -> Self {
        let state = Arc::new(CrashMonitorState {
            inner: Arc::new(RwLock::new(Some(Arc::new(provider)))),
            client,
            file_cache: Arc::new(RwLock::new(HashMap::new())),
            restart_count: AtomicU32::new(0),
            restart_lock: Mutex::new(()),
            node_path,
            tsserver_path,
            workspace_root,
            plugin_path,
            max_restarts,
        });

        spawn_crash_monitor(Arc::clone(&state), crash_notify);

        Self { state }
    }

    async fn get_inner(&self) -> Result<Arc<TsserverTypeProvider>, TypeProviderError> {
        self.state
            .inner
            .read()
            .await
            .clone()
            .ok_or_else(|| TypeProviderError::new("tsserver is restarting"))
    }
}

fn spawn_crash_monitor(state: Arc<CrashMonitorState>, crash_notify: Arc<Notify>) {
    tokio::spawn(async move {
        crash_notify.notified().await;

        tracing::warn!("tsserver crash detected — initiating restart sequence");

        {
            let mut guard = state.inner.write().await;
            *guard = None;
        }

        if let Some(c) = state.client.get() {
            c.show_message(
                tower_lsp_server::ls_types::MessageType::WARNING,
                "TypeScript server (tsserver) crashed. Restarting...",
            )
            .await;
        }

        let attempt = {
            let _lock = state.restart_lock.lock().await;
            state.restart_count.fetch_add(1, Ordering::Relaxed) + 1
        };

        if attempt > state.max_restarts {
            tracing::error!(
                "tsserver restart limit reached ({}) — staying in verter-only mode",
                state.max_restarts
            );
            if let Some(c) = state.client.get() {
                c.show_message(
                    tower_lsp_server::ls_types::MessageType::ERROR,
                    format!(
                        "TypeScript server (tsserver) crashed {} times. \
                         Running in verter-only mode for the rest of this session.",
                        state.max_restarts
                    ),
                )
                .await;
            }
            return;
        }

        let delay_secs = (1u64 << (attempt - 1)).min(4);
        tracing::info!(
            "tsserver restart attempt {attempt}/{} after {delay_secs}s",
            state.max_restarts
        );
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

        let new_crash_notify = Arc::new(Notify::new());
        match TsserverTypeProvider::spawn(
            &state.node_path,
            &state.tsserver_path,
            &state.workspace_root,
            state.plugin_path.as_deref(),
            Some(Arc::clone(&new_crash_notify)),
        )
        .await
        {
            Ok(new_provider) => {
                let new_provider = Arc::new(new_provider);

                let cache_snapshot = state.file_cache.read().await.clone();
                for (path, content) in &cache_snapshot {
                    if let Err(e) = new_provider.open_file(path, content).await {
                        tracing::warn!("Failed to re-open {path} after restart: {e}");
                    }
                }

                {
                    let mut guard = state.inner.write().await;
                    *guard = Some(Arc::clone(&new_provider));
                }

                tracing::info!("tsserver restarted successfully (attempt {attempt})");
                if let Some(c) = state.client.get() {
                    c.show_message(
                        tower_lsp_server::ls_types::MessageType::INFO,
                        "TypeScript server restarted successfully.",
                    )
                    .await;
                }

                state.restart_count.store(0, Ordering::Relaxed);
                spawn_crash_monitor(state, new_crash_notify);
            }
            Err(e) => {
                tracing::error!("Failed to restart tsserver: {e}");
                if let Some(c) = state.client.get() {
                    c.show_message(
                        tower_lsp_server::ls_types::MessageType::ERROR,
                        format!("Failed to restart TypeScript server: {e}"),
                    )
                    .await;
                }
            }
        }
    });
}

impl TypeProvider for ResilientTsserverProvider {
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.state
                .file_cache
                .write()
                .await
                .insert(path_owned.clone(), content_owned.clone());
            let provider = self.get_inner().await?;
            provider.open_file(&path_owned, &content_owned).await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.state
                .file_cache
                .write()
                .await
                .insert(path_owned.clone(), content_owned.clone());
            let provider = self.get_inner().await?;
            provider.load_file(&path_owned, &content_owned).await
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

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async {
            if let Ok(provider) = self.get_inner().await {
                let _ = provider.shutdown().await;
            }
            Ok(())
        })
    }

    fn child_pid(&self) -> Option<u32> {
        self.state
            .inner
            .try_read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|tp| tp.child_pid()))
    }

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let base_url = base_url.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.configure_paths(&base_url, paths).await
        })
    }
}
