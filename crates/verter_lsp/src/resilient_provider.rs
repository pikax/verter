use std::collections::{hash_map::Entry, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify, OnceCell, RwLock};
use tower_lsp_server::Client;

use crate::tsgo::protocol::*;
use crate::tsgo::traits::{ProviderFuture, TypeProvider};

type SpawnFuture<'a, P> = Pin<Box<dyn Future<Output = Result<P, TypeProviderError>> + Send + 'a>>;

pub(crate) trait ResilientBackend<P>: Send + Sync + 'static
where
    P: TypeProvider + Send + Sync + 'static,
{
    fn log_name(&self) -> &'static str;

    fn user_label(&self) -> &'static str;

    fn restarting_error(&self) -> &'static str;

    fn spawn<'a>(&'a self, crash_notify: Arc<Notify>) -> SpawnFuture<'a, P>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedFileMode {
    Load,
    Open,
}

#[derive(Debug, Clone)]
struct CachedFile {
    content: String,
    mode: CachedFileMode,
}

#[derive(Debug, Clone)]
struct CachedPathConfig {
    base_url: String,
    paths: serde_json::Value,
}

struct ResilientState<P, B>
where
    P: TypeProvider + Send + Sync + 'static,
    B: ResilientBackend<P>,
{
    inner: Arc<RwLock<Option<Arc<P>>>>,
    client: Arc<OnceCell<Client>>,
    file_cache: Arc<RwLock<HashMap<String, CachedFile>>>,
    workspace_folders: Arc<RwLock<Vec<serde_json::Value>>>,
    path_config: Arc<RwLock<Option<CachedPathConfig>>>,
    restart_count: AtomicU32,
    restart_lock: Mutex<()>,
    backend: B,
    max_restarts: u32,
}

pub(crate) struct ResilientProvider<P, B>
where
    P: TypeProvider + Send + Sync + 'static,
    B: ResilientBackend<P>,
{
    state: Arc<ResilientState<P, B>>,
}

impl<P, B> ResilientProvider<P, B>
where
    P: TypeProvider + Send + Sync + 'static,
    B: ResilientBackend<P>,
{
    pub(crate) fn new(
        provider: P,
        crash_notify: Arc<Notify>,
        backend: B,
        client: Arc<OnceCell<Client>>,
        max_restarts: u32,
    ) -> Self {
        let state = Arc::new(ResilientState {
            inner: Arc::new(RwLock::new(Some(Arc::new(provider)))),
            client,
            file_cache: Arc::new(RwLock::new(HashMap::new())),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
            path_config: Arc::new(RwLock::new(None)),
            restart_count: AtomicU32::new(0),
            restart_lock: Mutex::new(()),
            backend,
            max_restarts,
        });

        spawn_crash_monitor(Arc::clone(&state), crash_notify);

        Self { state }
    }

    async fn get_inner(&self) -> Result<Arc<P>, TypeProviderError> {
        self.state
            .inner
            .read()
            .await
            .clone()
            .ok_or_else(|| TypeProviderError::new(self.state.backend.restarting_error()))
    }

    async fn cache_file(&self, path: &str, content: &str, mode: Option<CachedFileMode>) {
        let mut cache = self.state.file_cache.write().await;
        match cache.entry(path.to_string()) {
            Entry::Occupied(mut entry) => {
                let cached = entry.get_mut();
                cached.content = content.to_string();
                if let Some(next_mode) = mode {
                    match next_mode {
                        CachedFileMode::Open => cached.mode = CachedFileMode::Open,
                        CachedFileMode::Load if cached.mode != CachedFileMode::Open => {
                            cached.mode = CachedFileMode::Load;
                        }
                        CachedFileMode::Load => {}
                    }
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(CachedFile {
                    content: content.to_string(),
                    mode: mode.unwrap_or(CachedFileMode::Open),
                });
            }
        }
    }

    async fn remove_cached_file(&self, path: &str) {
        self.state.file_cache.write().await.remove(path);
    }

    async fn cache_workspace_folders(
        &self,
        added: &[serde_json::Value],
        removed: &[serde_json::Value],
    ) {
        let mut folders = self.state.workspace_folders.write().await;
        for folder in removed {
            if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                folders
                    .retain(|existing| existing.get("uri").and_then(|v| v.as_str()) != Some(uri));
            }
        }
        for folder in added {
            if let Some(uri) = folder.get("uri").and_then(|v| v.as_str()) {
                folders
                    .retain(|existing| existing.get("uri").and_then(|v| v.as_str()) != Some(uri));
            }
            folders.push(folder.clone());
        }
    }

    async fn cache_path_config(&self, base_url: &str, paths: serde_json::Value) {
        *self.state.path_config.write().await = Some(CachedPathConfig {
            base_url: base_url.to_string(),
            paths,
        });
    }
}

fn spawn_crash_monitor<P, B>(state: Arc<ResilientState<P, B>>, crash_notify: Arc<Notify>)
where
    P: TypeProvider + Send + Sync + 'static,
    B: ResilientBackend<P>,
{
    tokio::spawn(async move {
        crash_notify.notified().await;

        tracing::warn!(
            "{} crash detected - initiating restart sequence",
            state.backend.log_name()
        );

        {
            let mut guard = state.inner.write().await;
            *guard = None;
        }

        if let Some(client) = state.client.get() {
            client
                .show_message(
                    tower_lsp_server::ls_types::MessageType::WARNING,
                    format!(
                        "TypeScript server ({}) crashed. Restarting...",
                        state.backend.user_label()
                    ),
                )
                .await;
        } else {
            tracing::warn!(
                "{} crashed before the LSP client was available",
                state.backend.log_name()
            );
        }

        let attempt = {
            let _lock = state.restart_lock.lock().await;
            state.restart_count.fetch_add(1, Ordering::Relaxed) + 1
        };

        if attempt > state.max_restarts {
            tracing::error!(
                "{} restart limit reached ({}) - staying in verter-only mode",
                state.backend.log_name(),
                state.max_restarts
            );
            if let Some(client) = state.client.get() {
                client
                    .show_message(
                        tower_lsp_server::ls_types::MessageType::ERROR,
                        format!(
                            "TypeScript server ({}) crashed {} times. Running in verter-only mode for the rest of this session.",
                            state.backend.user_label(),
                            state.max_restarts
                        ),
                    )
                    .await;
            }
            return;
        }

        let delay_secs = (1u64 << (attempt - 1)).min(4);
        tracing::info!(
            "{} restart attempt {attempt}/{} after {delay_secs}s",
            state.backend.log_name(),
            state.max_restarts
        );
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

        let new_crash_notify = Arc::new(Notify::new());
        match state.backend.spawn(Arc::clone(&new_crash_notify)).await {
            Ok(provider) => {
                let provider = Arc::new(provider);

                let folders = state.workspace_folders.read().await.clone();
                if !folders.is_empty() {
                    let _ = provider.update_workspace_folders(folders, vec![]).await;
                }

                if let Some(path_config) = state.path_config.read().await.clone() {
                    let _ = provider
                        .configure_paths(&path_config.base_url, path_config.paths)
                        .await;
                }

                let cache_snapshot = state.file_cache.read().await.clone();
                for (path, file) in &cache_snapshot {
                    let result = match file.mode {
                        CachedFileMode::Load => provider.load_file(path, &file.content).await,
                        CachedFileMode::Open => provider.open_file(path, &file.content).await,
                    };

                    if let Err(err) = result {
                        tracing::warn!(
                            "Failed to replay {} into {} after restart: {}",
                            path,
                            state.backend.log_name(),
                            err
                        );
                    }
                }

                {
                    let mut guard = state.inner.write().await;
                    *guard = Some(Arc::clone(&provider));
                }

                tracing::info!(
                    "{} restarted successfully (attempt {attempt})",
                    state.backend.log_name()
                );
                if let Some(client) = state.client.get() {
                    client
                        .show_message(
                            tower_lsp_server::ls_types::MessageType::INFO,
                            "TypeScript server restarted successfully.",
                        )
                        .await;
                }

                state.restart_count.store(0, Ordering::Relaxed);
                spawn_crash_monitor(state, new_crash_notify);
            }
            Err(err) => {
                tracing::error!("Failed to restart {}: {}", state.backend.log_name(), err);
                if let Some(client) = state.client.get() {
                    client
                        .show_message(
                            tower_lsp_server::ls_types::MessageType::ERROR,
                            format!("Failed to restart TypeScript server: {err}"),
                        )
                        .await;
                }
            }
        }
    });
}

impl<P, B> TypeProvider for ResilientProvider<P, B>
where
    P: TypeProvider + Send + Sync + 'static,
    B: ResilientBackend<P>,
{
    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.cache_file(&path_owned, &content_owned, Some(CachedFileMode::Open))
                .await;
            let provider = self.get_inner().await?;
            provider.open_file(&path_owned, &content_owned).await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.cache_file(&path_owned, &content_owned, Some(CachedFileMode::Load))
                .await;
            let provider = self.get_inner().await?;
            provider.load_file(&path_owned, &content_owned).await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        let content_owned = content.to_string();
        Box::pin(async move {
            self.cache_file(&path_owned, &content_owned, None).await;
            let provider = self.get_inner().await?;
            provider.update_file(&path_owned, &content_owned).await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path_owned = path.to_string();
        Box::pin(async move {
            self.remove_cached_file(&path_owned).await;
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

    fn resolve_completion(
        &self,
        path: &str,
        data: serde_json::Value,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let path_owned = path.to_string();
        Box::pin(async move {
            let provider = self.get_inner().await?;
            provider.resolve_completion(&path_owned, data).await
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

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        let base_url = base_url.to_string();
        let cached_paths = paths.clone();
        Box::pin(async move {
            self.cache_path_config(&base_url, cached_paths).await;
            match self.get_inner().await {
                Ok(provider) => provider.configure_paths(&base_url, paths).await,
                Err(_) => Ok(()),
            }
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let added_clone = added.clone();
        let removed_clone = removed.clone();
        Box::pin(async move {
            self.cache_workspace_folders(&added_clone, &removed_clone)
                .await;
            match self.get_inner().await {
                Ok(provider) => provider.update_workspace_folders(added, removed).await,
                Err(_) => Ok(()),
            }
        })
    }

    fn child_pid(&self) -> Option<u32> {
        self.state
            .inner
            .try_read()
            .ok()
            .and_then(|guard| guard.as_ref().and_then(|provider| provider.child_pid()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsgo::mock::{MockCall, MockTypeProvider};

    struct TestBackend {
        replacement: MockTypeProvider,
    }

    impl ResilientBackend<MockTypeProvider> for TestBackend {
        fn log_name(&self) -> &'static str {
            "test-provider"
        }

        fn user_label(&self) -> &'static str {
            "test"
        }

        fn restarting_error(&self) -> &'static str {
            "test provider is restarting"
        }

        fn spawn<'a>(&'a self, _crash_notify: Arc<Notify>) -> SpawnFuture<'a, MockTypeProvider> {
            let provider = self.replacement.clone();
            Box::pin(async move { Ok(provider) })
        }
    }

    fn make_resilient(
        initial: MockTypeProvider,
        replacement: MockTypeProvider,
    ) -> (
        ResilientProvider<MockTypeProvider, TestBackend>,
        Arc<Notify>,
    ) {
        let crash_notify = Arc::new(Notify::new());
        let provider = ResilientProvider::new(
            initial,
            Arc::clone(&crash_notify),
            TestBackend { replacement },
            Arc::new(OnceCell::new()),
            3,
        );
        (provider, crash_notify)
    }

    #[tokio::test]
    async fn update_workspace_folders_is_cached_while_restarting() {
        let initial = MockTypeProvider::new();
        let replacement = MockTypeProvider::new();
        let (provider, _crash_notify) = make_resilient(initial, replacement);

        {
            let mut guard = provider.state.inner.write().await;
            *guard = None;
        }

        let added = vec![serde_json::json!({ "uri": "file:///workspace-a" })];
        let result = provider
            .update_workspace_folders(added.clone(), vec![])
            .await;

        assert!(
            result.is_ok(),
            "workspace folder cache update should succeed"
        );
        assert_eq!(
            provider.state.workspace_folders.read().await.clone(),
            added,
            "workspace folders should still be cached for replay"
        );
    }

    #[tokio::test]
    async fn restart_replays_cached_state_without_downgrading_loaded_files() {
        let initial = MockTypeProvider::new();
        let replacement = MockTypeProvider::new();
        let replacement_clone = replacement.clone();
        let (provider, crash_notify) = make_resilient(initial, replacement);

        provider
            .load_file("/project/src/loaded.vue.tsx", "const loaded = true;")
            .await
            .unwrap();
        provider
            .update_file("/project/src/loaded.vue.tsx", "const loaded = 2;")
            .await
            .unwrap();
        provider
            .open_file("/project/src/open.vue.tsx", "const open = true;")
            .await
            .unwrap();
        provider
            .configure_paths("/project/src", serde_json::json!({ "@/*": ["./*"] }))
            .await
            .unwrap();
        provider
            .update_workspace_folders(
                vec![serde_json::json!({ "uri": "file:///project" })],
                vec![],
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        crash_notify.notify_waiters();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while replacement_clone.calls().len() < 4 && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let calls = replacement_clone.calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::LoadFile { path, .. } if path == "/project/src/loaded.vue.tsx"
            )),
            "loaded files should replay via load_file"
        );
        assert!(
            !calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/project/src/loaded.vue.tsx"
            )),
            "loaded files must not be replayed as open files"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::OpenFile { path, .. } if path == "/project/src/open.vue.tsx"
            )),
            "open files should replay via open_file"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::ConfigurePaths { base_url, .. } if base_url == "/project/src"
            )),
            "path configuration should be replayed after restart"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::UpdateWorkspaceFolders { added, .. }
                    if added.iter().any(|folder| folder.get("uri").and_then(|value| value.as_str()) == Some("file:///project"))
            )),
            "workspace folders should be replayed after restart"
        );
    }

    #[tokio::test]
    async fn resolve_completion_delegates_to_the_inner_provider() {
        let initial = MockTypeProvider::new();
        let replacement = MockTypeProvider::new();
        initial.set_resolve_completion(
            "/project/src/App.vue.tsx",
            serde_json::json!({ "kind": "import" }),
            Some(CompletionResolveResult {
                additional_text_edits: vec![ResolvedTextEdit {
                    start: 0,
                    end: 0,
                    new_text: "import Foo from './Foo';".to_string(),
                }],
            }),
        );
        let (provider, _crash_notify) = make_resilient(initial.clone(), replacement);

        let result = provider
            .resolve_completion(
                "/project/src/App.vue.tsx",
                serde_json::json!({ "kind": "import" }),
            )
            .await
            .unwrap();

        assert_eq!(
            result.map(|resolved| resolved.additional_text_edits.len()),
            Some(1)
        );
        assert!(
            initial.calls().iter().any(|call| matches!(
                call,
                MockCall::ResolveCompletion { path, .. } if path == "/project/src/App.vue.tsx"
            )),
            "resolve_completion should be forwarded"
        );
    }
}
