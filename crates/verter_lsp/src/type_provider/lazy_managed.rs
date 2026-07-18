//! Managed TypeScript provider activated only after the preferred editor-owned route fails.
//!
//! File/configuration lifecycle is recorded without starting a process. The first real
//! fallback query creates one managed provider, replays the latest desired state under an
//! activation gate, and only then serves the query. Concurrent lifecycle and query calls
//! therefore cannot enter the child between its snapshot and replay.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::{Mutex as AsyncMutex, OnceCell};
use verter_type_runtime::protocol::*;
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

type FactoryFuture =
    Pin<Box<dyn Future<Output = Result<Arc<dyn TypeProvider>, TypeProviderError>> + Send>>;
type Factory = dyn Fn() -> FactoryFuture + Send + Sync;

#[derive(Debug, Clone, Copy)]
enum FileAccess {
    Open,
    Loaded,
}

#[derive(Debug, Clone, Copy)]
enum PriorityLane {
    Interactive,
    Normal,
    Background,
}

#[derive(Debug, Clone)]
struct DesiredFile {
    content: String,
    access: FileAccess,
    lane: PriorityLane,
}

#[derive(Debug, Clone)]
struct RegisteredCarrier {
    source_path: String,
    content: String,
    project_file_name: String,
}

#[derive(Debug, Clone)]
struct WorkspaceChange {
    added: bool,
    value: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
struct DesiredState {
    files: BTreeMap<String, DesiredFile>,
    paths: Option<(String, serde_json::Value)>,
    workspace_changes: BTreeMap<String, WorkspaceChange>,
    carriers: BTreeMap<String, RegisteredCarrier>,
    carrier_notifications: BTreeSet<String>,
    resync_open_files: bool,
}

/// A managed tsgo provider whose process is not created until a fallback query.
///
/// A successful activation is memoized for the session. A FAILED activation is
/// NOT latched: it is retried after [`ACTIVATION_RETRY_COOLDOWN`] so a transient
/// spawn/replay failure recovers (the next fallback query answers) instead of
/// leaving the provider dead for the session — while the cooldown still prevents
/// a hot respawn storm. A new LSP session constructs a fresh instance regardless.
/// Lifecycle calls made before activation update only [`DesiredState`].
pub struct LazyManagedTypeProvider {
    factory: Arc<Factory>,
    provider: OnceCell<Arc<dyn TypeProvider>>,
    activation: AsyncMutex<()>,
    desired: Mutex<DesiredState>,
    /// The last failed activation (when + error message). Consulted under the
    /// activation mutex: a failure newer than [`ACTIVATION_RETRY_COOLDOWN`] is
    /// returned WITHOUT re-running the factory (storm protection); an older one
    /// is retried. Cleared on a successful activation.
    last_activation_failure: Mutex<Option<(std::time::Instant, String)>>,
}

/// Minimum interval between managed-fallback activation attempts after a failure.
/// Bounds the respawn rate so a persistently failing backend cannot hot-loop,
/// while still letting a transient failure recover on a later query.
pub(crate) const ACTIVATION_RETRY_COOLDOWN: std::time::Duration =
    std::time::Duration::from_millis(250);

impl LazyManagedTypeProvider {
    #[must_use]
    pub fn new<F, Fut>(factory: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<dyn TypeProvider>, TypeProviderError>> + Send + 'static,
    {
        Self {
            factory: Arc::new(move || Box::pin(factory())),
            provider: OnceCell::new(),
            activation: AsyncMutex::new(()),
            desired: Mutex::new(DesiredState::default()),
            last_activation_failure: Mutex::new(None),
        }
    }

    fn current(&self) -> Option<Arc<dyn TypeProvider>> {
        self.provider.get().cloned()
    }

    async fn activate(&self) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let _activation = self.activation.lock().await;
        if let Some(provider) = self.provider.get() {
            return Ok(provider.clone());
        }
        // Storm protection: a failure within the cooldown returns the cached error
        // WITHOUT re-running the factory; an older failure is retried below.
        {
            let last = self.last_activation_failure.lock().unwrap();
            if let Some((at, message)) = &*last {
                if at.elapsed() < ACTIVATION_RETRY_COOLDOWN {
                    return Err(TypeProviderError::new(format!(
                        "managed fallback activation failed (retry pending): {message}"
                    )));
                }
            }
        }

        let result = match (self.factory)().await {
            Ok(provider) => match self.replay(&provider).await {
                Ok(()) => Ok(provider),
                Err(error) => {
                    // A provider that did not accept the complete replay must never be
                    // exposed as warm. Best-effort teardown also prevents an orphaned
                    // fallback process when activation fails part-way through.
                    let _ = provider.shutdown().await;
                    Err(TypeProviderError::new(format!(
                        "managed fallback state replay failed: {error}"
                    )))
                }
            },
            Err(error) => Err(error),
        };
        match result {
            Ok(provider) => {
                *self.last_activation_failure.lock().unwrap() = None;
                let _ = self.provider.set(provider.clone());
                Ok(provider)
            }
            Err(error) => {
                *self.last_activation_failure.lock().unwrap() =
                    Some((std::time::Instant::now(), error.message.clone()));
                Err(error)
            }
        }
    }

    async fn replay(&self, provider: &Arc<dyn TypeProvider>) -> Result<(), TypeProviderError> {
        let desired = self.desired.lock().unwrap().clone();

        if let Some((base_url, paths)) = desired.paths {
            provider.configure_paths(&base_url, paths).await?;
        }

        if !desired.workspace_changes.is_empty() {
            let mut added = Vec::new();
            let mut removed = Vec::new();
            for change in desired.workspace_changes.into_values() {
                if change.added {
                    added.push(change.value);
                } else {
                    removed.push(change.value);
                }
            }
            provider.update_workspace_folders(added, removed).await?;
        }

        for (path, carrier) in desired.carriers {
            provider
                .register_carrier_member(
                    &carrier.source_path,
                    &path,
                    &carrier.content,
                    &carrier.project_file_name,
                )
                .await?;
        }

        for (path, file) in desired.files {
            match (file.access, file.lane) {
                (FileAccess::Open, PriorityLane::Interactive) => {
                    provider.open_file(&path, &file.content).await?
                }
                (FileAccess::Open, PriorityLane::Normal) => {
                    provider.open_file_normal(&path, &file.content).await?
                }
                (FileAccess::Open, PriorityLane::Background) => {
                    provider.open_file_background(&path, &file.content).await?
                }
                (FileAccess::Loaded, PriorityLane::Interactive) => {
                    provider.load_file(&path, &file.content).await?
                }
                (FileAccess::Loaded, PriorityLane::Normal) => {
                    provider.load_file_normal(&path, &file.content).await?
                }
                (FileAccess::Loaded, PriorityLane::Background) => {
                    provider.load_file_background(&path, &file.content).await?
                }
            }
        }

        for path in desired.carrier_notifications {
            provider.notify_carrier_changed(&path).await?;
        }
        if desired.resync_open_files {
            provider.resync_open_files().await?;
        }
        Ok(())
    }

    async fn record_file(
        &self,
        path: String,
        content: String,
        access: Option<FileAccess>,
        lane: PriorityLane,
        update: bool,
    ) -> Result<(), TypeProviderError> {
        let _activation = self.activation.lock().await;
        {
            let mut desired = self.desired.lock().unwrap();
            let preserved_access = desired
                .files
                .get(&path)
                .map_or(FileAccess::Open, |file| file.access);
            desired.files.insert(
                path.clone(),
                DesiredFile {
                    content: content.clone(),
                    access: access.unwrap_or(preserved_access),
                    lane,
                },
            );
        }
        let Some(provider) = self.current() else {
            return Ok(());
        };
        match (update, lane) {
            (true, PriorityLane::Interactive) => provider.update_file(&path, &content).await,
            (true, PriorityLane::Normal) => provider.update_file_normal(&path, &content).await,
            (true, PriorityLane::Background) => {
                provider.update_file_background(&path, &content).await
            }
            (false, PriorityLane::Interactive) => match access.unwrap_or(FileAccess::Open) {
                FileAccess::Open => provider.open_file(&path, &content).await,
                FileAccess::Loaded => provider.load_file(&path, &content).await,
            },
            (false, PriorityLane::Normal) => match access.unwrap_or(FileAccess::Open) {
                FileAccess::Open => provider.open_file_normal(&path, &content).await,
                FileAccess::Loaded => provider.load_file_normal(&path, &content).await,
            },
            (false, PriorityLane::Background) => match access.unwrap_or(FileAccess::Open) {
                FileAccess::Open => provider.open_file_background(&path, &content).await,
                FileAccess::Loaded => provider.load_file_background(&path, &content).await,
            },
        }
    }

    async fn record_close(
        &self,
        path: String,
        lane: PriorityLane,
    ) -> Result<(), TypeProviderError> {
        let _activation = self.activation.lock().await;
        self.desired.lock().unwrap().files.remove(&path);
        let Some(provider) = self.current() else {
            return Ok(());
        };
        match lane {
            PriorityLane::Interactive => provider.close_file(&path).await,
            PriorityLane::Normal => provider.close_file_normal(&path).await,
            PriorityLane::Background => provider.close_file_background(&path).await,
        }
    }

    async fn record_paths(
        &self,
        base_url: String,
        paths: serde_json::Value,
        background: bool,
    ) -> Result<(), TypeProviderError> {
        let _activation = self.activation.lock().await;
        self.desired.lock().unwrap().paths = Some((base_url.clone(), paths.clone()));
        let Some(provider) = self.current() else {
            return Ok(());
        };
        if background {
            provider.configure_paths_background(&base_url, paths).await
        } else {
            provider.configure_paths(&base_url, paths).await
        }
    }

    async fn record_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
        background: bool,
    ) -> Result<(), TypeProviderError> {
        let _activation = self.activation.lock().await;
        {
            let mut desired = self.desired.lock().unwrap();
            for value in &added {
                desired.workspace_changes.insert(
                    workspace_value_key(value),
                    WorkspaceChange {
                        added: true,
                        value: value.clone(),
                    },
                );
            }
            for value in &removed {
                desired.workspace_changes.insert(
                    workspace_value_key(value),
                    WorkspaceChange {
                        added: false,
                        value: value.clone(),
                    },
                );
            }
        }
        let Some(provider) = self.current() else {
            return Ok(());
        };
        if background {
            provider
                .update_workspace_folders_background(added, removed)
                .await
        } else {
            provider.update_workspace_folders(added, removed).await
        }
    }
}

fn workspace_value_key(value: &serde_json::Value) -> String {
    value
        .get("uri")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

impl TypeProvider for LazyManagedTypeProvider {
    fn provider_id(&self) -> &'static str {
        "tsgo"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Open),
            PriorityLane::Interactive,
            false,
        ))
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Loaded),
            PriorityLane::Interactive,
            false,
        ))
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            None,
            PriorityLane::Interactive,
            true,
        ))
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_close(path.to_string(), PriorityLane::Interactive))
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let path = path.to_string();
        let trigger_character = trigger_character.map(str::to_string);
        Box::pin(async move {
            self.activate()
                .await?
                .get_completions(&path, offset, trigger_character.as_deref())
                .await
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        Box::pin(async move {
            self.activate()
                .await?
                .get_completion_details(path, offset, items)
                .await
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.resolve_completion(&path, data).await })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.get_hover(&path, offset).await })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.get_diagnostics(&path).await })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.get_definition(&path, offset).await })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_type_definition(&path, offset)
                .await
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.get_references(&path, offset).await })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_rename_locations(&path, offset)
                .await
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_signature_help(&path, offset)
                .await
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        let path = path.to_string();
        let diagnostics = diagnostics.to_vec();
        Box::pin(async move {
            self.activate()
                .await?
                .get_code_actions(&path, start_offset, end_offset, &diagnostics)
                .await
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path = path.to_string();
        Box::pin(async move { self.activate().await?.get_semantic_tokens(&path).await })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_document_highlights(&path, offset)
                .await
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_inlay_hints(&path, start_offset, end_offset)
                .await
        })
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Open),
            PriorityLane::Background,
            false,
        ))
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Loaded),
            PriorityLane::Background,
            false,
        ))
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            None,
            PriorityLane::Background,
            true,
        ))
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_close(path.to_string(), PriorityLane::Background))
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            self.activate()
                .await?
                .get_diagnostics_background(&path)
                .await
        })
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Open),
            PriorityLane::Normal,
            false,
        ))
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            Some(FileAccess::Loaded),
            PriorityLane::Normal,
            false,
        ))
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_file(
            path.to_string(),
            content.to_string(),
            None,
            PriorityLane::Normal,
            true,
        ))
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_close(path.to_string(), PriorityLane::Normal))
    }

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_paths(base_url.to_string(), paths, false))
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_paths(base_url.to_string(), paths, true))
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        let companion_path = companion_path.to_string();
        Box::pin(async move {
            let _activation = self.activation.lock().await;
            self.desired
                .lock()
                .unwrap()
                .carrier_notifications
                .insert(companion_path.clone());
            let Some(provider) = self.current() else {
                return Ok(());
            };
            provider.notify_carrier_changed(&companion_path).await
        })
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let content = content.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            let _activation = self.activation.lock().await;
            self.desired.lock().unwrap().carriers.insert(
                companion_path.clone(),
                RegisteredCarrier {
                    source_path: source_path.clone(),
                    content: content.clone(),
                    project_file_name: project_file_name.clone(),
                },
            );
            let Some(provider) = self.current() else {
                return Ok(());
            };
            provider
                .register_carrier_member(
                    &source_path,
                    &companion_path,
                    &content,
                    &project_file_name,
                )
                .await
        })
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            let _activation = self.activation.lock().await;
            self.desired.lock().unwrap().resync_open_files = true;
            let Some(provider) = self.current() else {
                return Ok(());
            };
            provider.resync_open_files().await
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_workspace_folders(added, removed, false))
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        Box::pin(self.record_workspace_folders(added, removed, true))
    }

    fn child_pid(&self) -> Option<u32> {
        self.current().and_then(|provider| provider.child_pid())
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            let _activation = self.activation.lock().await;
            let Some(provider) = self.current() else {
                return Ok(());
            };
            provider.shutdown().await
        })
    }
}
