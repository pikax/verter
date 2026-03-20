use std::{collections::HashSet, sync::Arc};

use dashmap::{DashMap, DashSet};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::sfc_scanner::scan_sfc_blocks;
use crate::documents::{uri_to_canonical_id, DocumentRegistry};
use crate::features::action_utils::fix_placeholder_uris;
use crate::features::call_hierarchy;
use crate::features::code_lens::code_lenses;
use crate::features::color_info;
use crate::features::completion::completions_at_position;
use crate::features::cursor_context::{
    classify_cursor_context, classify_expression_context_with_trigger, CursorContext,
    ExpressionContext, TemplateCursorContext,
};
use crate::features::definition::definition_at_position;
use crate::features::diagnostics::map_diagnostics;
use crate::features::document_highlight::highlights_at_position;
use crate::features::document_link::build_document_links;
use crate::features::document_symbol::build_document_symbols;
use crate::features::folding_range::build_folding_ranges;
use crate::features::formatting::format_document;
use crate::features::hover;
use crate::features::hover::hover_at_position;
use crate::features::linked_editing::linked_editing_ranges;
use crate::features::organize_imports::organize_imports_actions;
use crate::features::references::references_at_position;
use crate::features::rename::{prepare_rename, rename_at_position};
use crate::features::workspace_symbol::workspace_symbols;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, remove_sync_state, ProviderPathKind,
    ProviderSyncState,
};
use crate::statistics::Statistics;
use crate::tsgo::merge;
use crate::tsgo::project_sync::ProjectSync;
use crate::tsgo::traits::TypeProvider;
use crate::LspConfig;
use verter_vfs::WorkspaceAccess;

// ── Handler tracking for freeze diagnosis ──────────────────────────────

/// Global counter of in-flight LSP request handlers. When this reaches the tokio
/// worker thread count, the runtime is saturated and timers/heartbeats can't fire.
static ACTIVE_HANDLERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// RAII guard that tracks handler lifecycle. Logs entry (with thread ID and active
/// handler count) on creation, logs exit (with duration) on drop.
struct HandlerGuard {
    name: &'static str,
    start: std::time::Instant,
    thread_id: std::thread::ThreadId,
}

impl HandlerGuard {
    fn new(name: &'static str) -> Self {
        let prev = ACTIVE_HANDLERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let thread_id = std::thread::current().id();
        tracing::info!(
            "HANDLER_ENTER {name} active={} thread={thread_id:?}",
            prev + 1
        );
        Self {
            name,
            start: std::time::Instant::now(),
            thread_id,
        }
    }
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        let remaining = ACTIVE_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) - 1;
        let elapsed = self.start.elapsed();
        tracing::info!(
            "HANDLER_EXIT {} active={remaining} elapsed={elapsed:?} thread={:?}",
            self.name,
            self.thread_id,
        );
    }
}

#[path = "protocol_types.rs"]
pub(crate) mod protocol_types;
pub use self::protocol_types::*;

#[path = "server_utils.rs"]
mod server_utils;
use self::server_utils::*;
pub(crate) use self::server_utils::{
    compute_verter_diagnostics_for_with_views, prepare_non_vue_provider_sync, resolve_component_for,
};

#[path = "background_init.rs"]
mod background_init;
use self::background_init::*;
pub(crate) use self::background_init::{
    configure_provider_paths_for_source, is_generated_verter_types_event,
};

fn block_in_place_if_available<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

/// Lightweight snapshot of the published resolver, replacing the old `ResolverSnapshot`.
///
/// Preserves the `.resolver` field access pattern so callers don't need deep changes.
#[derive(Debug, Clone)]
pub(crate) struct PublishedResolverSnapshot {
    pub(crate) resolver: crate::project_resolver::NativeProjectResolver,
}

/// Pre-extracted data for type provider calls.
/// All DashMap guards are dropped before this is constructed, so it is safe
/// to hold across `.await` points without risking deadlock.
struct TypeProviderContext {
    tsx_path: String,
    tsx_content: Arc<str>,
    mapper: PositionMapper,
    tsx_line_index: LineIndex,
    vue_line_index: LineIndex,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedNonVueProviderSync {
    pub(crate) provider_path: String,
    pub(crate) rewritten: String,
    pub(crate) resolved_dependencies: Vec<crate::project_resolver::ResolveResult>,
}

struct ResolvedComponentDocument {
    uri: Uri,
    analysis: verter_host::FileAnalysisSnapshot,
    line_index: LineIndex,
}

/// The Verter language server implementation.
///
/// Wraps `verter_host` for SFC analysis and optionally a `TypeProvider`
/// (e.g., TSGO) for richer type information.
///
pub struct VerterLanguageServer {
    client: Client,
    documents: Arc<DocumentRegistry>,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_sync: Option<ProjectSync>,
    workspace_roots: tokio::sync::Mutex<Vec<String>>,
    statistics: Arc<Statistics>,
    /// Negotiated position encoding (LSP 3.17). Set during `initialize()`.
    /// Shared with SyncCoordinator so it can compute diagnostics with the correct encoding.
    position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Action engine — produces quick fixes and refactoring code actions.
    action_engine: verter_actions::ActionEngine,
    /// Lint options from initializationOptions, stored during initialize() for use in initialized().
    init_lint_options: tokio::sync::Mutex<Option<serde_json::Value>>,
    /// Vite config options (enabled, trusted files, node path).
    vite_config_options: tokio::sync::Mutex<crate::vite_config::ViteConfigOptions>,
    /// Whether type provider inlay hints are enabled (from initializationOptions).
    inlay_hints_enabled: std::sync::atomic::AtomicBool,
    /// Cached verter diagnostics per document:
    /// URI → (document_version, diagnostics_generation, diagnostics).
    /// Avoids re-running host + lint + component diagnostics when both push and
    /// pull paths request diagnostics for the same document version and host
    /// diagnostics generation. Arc-wrapped so the SyncCoordinator can read
    /// cached verter diagnostics when publishing merged diagnostics after sync.
    cached_verter_diags: Arc<DashMap<String, CachedVerterDiagEntry>>,
    /// Source-keyed provider materialization state shared across background/live sync.
    provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    /// Which type provider backend is active (TSGO, tsserver, or none).
    type_provider_kind: crate::TypeProviderKind,
    /// When `true`, show a recommendation to switch to TSGO in VS Code settings.
    suggest_tsgo: bool,
    /// Generation counter for completion coalescing. During rapid typing, each keystroke
    /// triggers a completion request. By incrementing this counter, stale requests can
    /// detect they've been superseded and skip the expensive type provider call.
    completion_generation: std::sync::atomic::AtomicU64,
    /// Canonical IDs needing **interactive IDE sync** (set by did_change, cleared by
    /// `ensure_current_file_synced`). Only the IDE TSX path is flushed on hover/completion.
    needs_ide_sync: Arc<DashSet<String>>,
    /// Canonical IDs needing **deferred API/.vue.ts sync** + owner-aware reconciliation.
    /// Set by did_change and by the interactive path (when API is deferred).
    /// Cleared by the coordinator's debounced sync after a resolver snapshot exists.
    needs_deferred_sync: Arc<DashSet<String>>,
    /// Source IDs whose provider sync depends on a resolver snapshot that is not ready yet.
    /// Drained after background initialization commits a new snapshot.
    pending_snapshot_provider_sync: Arc<DashSet<String>>,
    /// Handle for the SyncCoordinator — replaces the spawn-per-keystroke debounce.
    /// Signals are sent per keystroke; the coordinator coalesces them and syncs
    /// after 300ms of silence. `None` when no type provider is connected.
    sync_coordinator: Option<crate::sync_coordinator::SyncCoordinatorHandle>,
    /// Epoch millis of the last `did_change` call.  Used to skip non-critical TSGO requests
    /// (diagnostics, semantic tokens, inlay hints) during typing.  The debounced sync needs
    /// time to fire + TSGO needs time to process the update, so we suppress these requests
    /// for a short cooldown window after the last edit.
    last_change_ms: std::sync::atomic::AtomicU64,
    /// Serializes `did_change` handlers so only one runs at a time.
    ///
    /// The host's `upsert()` and `ensure_compiled()` use `std::sync::RwLock` (blocking),
    /// which blocks the calling tokio worker thread. When 5+ concurrent `did_change`
    /// handlers all contend on the write lock, they can block ALL worker threads →
    /// complete runtime starvation (no timers, no heartbeat, no responses).
    ///
    /// By serializing through a `tokio::sync::Mutex`, only one handler holds the blocking
    /// lock at a time. Others `.await` this mutex, YIELDING their worker thread back to
    /// the runtime so timers, completions, and heartbeats can still run.
    did_change_mutex: tokio::sync::Mutex<()>,
    /// Handle for the background workspace scanner. Receives priority signals
    /// from `did_open` to reorder the scan queue. `None` until `initialized()`.
    /// Arc-wrapped so background init can install the scanner without &self.
    workspace_scanner:
        Arc<tokio::sync::Mutex<Option<crate::workspace_scanner::WorkspaceScannerHandle>>>,
    /// Generation counter for background initialization. Incremented each time
    /// `initialized()` or `did_change_workspace_folders` spawns a new background
    /// init task. Background tasks check this before committing results to discard
    /// stale work when a newer init supersedes them.
    init_generation: Arc<std::sync::atomic::AtomicU64>,
    /// Actual MCP HTTP port (already bound). Sent to the extension during `initialized()`.
    mcp_port: Option<u16>,
    /// Why no type provider could be started. Sent via `$/verter/typeProviderStatus`.
    type_provider_none_reason: Option<String>,
    /// Most-recently-used canonical IDs. Updated on did_open, did_change, and
    /// interactive reads (hover, completion, definition). Used for MRU-ordered
    /// snapshot drain — most recently interacted files reconcile first.
    mru_canonical_ids: parking_lot::Mutex<Vec<String>>,
    /// Shared hydration cache: prevents re-hydrating compile blockers when
    /// the file's semantic hash hasn't changed since the last hydration.
    /// VFS filesystem workspace, built during background_init() after workspace
    /// roots and project configuration are known. `None` until initialization
    /// completes. Provides disk-backed file reads, project ownership, and import
    /// resolution through the [`WorkspaceAccess`] trait.
    vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_vfs::FilesystemWorkspace>>>>,
}

impl VerterLanguageServer {
    pub fn new(client: Client, config: LspConfig) -> Self {
        let project_sync = config
            .type_provider
            .as_ref()
            .map(|tp| ProjectSync::new(Arc::clone(tp), config.project_sync_mode));

        let needs_ide_sync = Arc::new(DashSet::new());
        let needs_deferred_sync = Arc::new(DashSet::new());
        let documents = Arc::new(DocumentRegistry::new(config.host));
        let position_encoding = Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16));
        let cached_verter_diags = Arc::new(DashMap::new());
        let provider_sync_states = Arc::new(DashMap::new());
        let pending_snapshot_provider_sync = Arc::new(DashSet::new());
        let vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_vfs::FilesystemWorkspace>>>> =
            Arc::new(parking_lot::RwLock::new(None));

        // Create SyncCoordinator if a type provider is connected.
        // The coordinator's debounced loop replaces the old spawn-per-keystroke pattern.
        let sync_coordinator = project_sync.as_ref().map(|ps| {
            crate::sync_coordinator::spawn_sync_coordinator(
                crate::sync_coordinator::SyncCoordinatorDeps {
                    documents: Arc::clone(&documents),
                    project_sync: ps.clone(),
                    needs_provider_sync: Arc::clone(&needs_deferred_sync),
                    pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
                    client: client.clone(),
                    type_provider: config.type_provider.clone(),
                    cached_verter_diags: Arc::clone(&cached_verter_diags),
                    position_encoding: Arc::clone(&position_encoding),
                    provider_sync_states: Arc::clone(&provider_sync_states),
                    vfs_workspace: Arc::clone(&vfs_workspace),
                },
            )
        });

        Self {
            client,
            documents,
            type_provider: config.type_provider,
            project_sync,
            workspace_roots: tokio::sync::Mutex::new(Vec::new()),
            statistics: Arc::new(Statistics::new(500)),
            position_encoding,
            action_engine: verter_actions::ActionEngine::default(),
            init_lint_options: tokio::sync::Mutex::new(None),
            vite_config_options: tokio::sync::Mutex::new(
                crate::vite_config::ViteConfigOptions::default(),
            ),
            inlay_hints_enabled: std::sync::atomic::AtomicBool::new(true),
            cached_verter_diags,
            provider_sync_states,
            type_provider_kind: config.type_provider_kind,
            suggest_tsgo: config.suggest_tsgo,
            completion_generation: std::sync::atomic::AtomicU64::new(0),
            needs_ide_sync,
            needs_deferred_sync,
            pending_snapshot_provider_sync,
            sync_coordinator,
            last_change_ms: std::sync::atomic::AtomicU64::new(0),
            did_change_mutex: tokio::sync::Mutex::new(()),
            workspace_scanner: Arc::new(tokio::sync::Mutex::new(None)),
            init_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mcp_port: config.mcp_port,
            type_provider_none_reason: config.type_provider_none_reason,
            mru_canonical_ids: parking_lot::Mutex::new(Vec::new()),
            vfs_workspace,
        }
    }

    /// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
    /// Caches results per document version to avoid redundant re-computation when both
    /// push (didChange) and pull (textDocument/diagnostic) paths request diagnostics.
    fn compute_verter_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        let vfs_ws = self.vfs_workspace.read();
        compute_verter_diagnostics_for_with_views(
            &self.documents,
            uri,
            &self.cached_verter_diags,
            vfs_ws.as_deref(),
        )
    }

    /// Compute and push **merged** (Verter lint + TypeScript type) diagnostics.
    ///
    /// This is the primary diagnostic path. Push diagnostics stay visible during
    /// typing — VS Code automatically adjusts their positions as the document changes.
    /// Fresh diagnostics are published after the SyncCoordinator's 300ms debounce fires.
    async fn publish_full_diagnostics(&self, uri: &Uri) {
        let verter_diags = self.compute_verter_diagnostics(uri);

        let diagnostics = if let Some(tp) = &self.type_provider {
            match self.ide_context(uri) {
                Some((tsx_path, tsx_content, mapper)) => {
                    let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                    let vue_li = self.documents.get(uri).map(|d| d.line_index.clone());
                    match (tp.get_diagnostics(&tsx_path).await, vue_li) {
                        (Ok(type_diags), Some(vue_li)) => {
                            tracing::debug!(
                                "publish_full_diagnostics: type provider returned {} for {}",
                                type_diags.len(),
                                uri.as_str()
                            );
                            merge::merge_diagnostics(
                                verter_diags,
                                type_diags,
                                &tsx_li,
                                &mapper,
                                &vue_li,
                            )
                        }
                        (Err(e), _) => {
                            tracing::warn!(
                                "publish_full_diagnostics: type provider error for {}: {e}",
                                uri.as_str()
                            );
                            verter_diags
                        }
                        _ => verter_diags,
                    }
                }
                None => verter_diags,
            }
        } else {
            verter_diags
        };

        self.publish_diagnostics_raw(uri, diagnostics).await;
    }

    /// Low-level: push pre-computed diagnostics to the client.
    async fn publish_diagnostics_raw(&self, uri: &Uri, diagnostics: Vec<Diagnostic>) {
        let _timer = self
            .statistics
            .timer("diagnostics", Some(uri.as_str().to_string()));

        tracing::info!(
            "publish_diagnostics ENTER {} ({} diags)",
            uri.as_str(),
            diagnostics.len()
        );

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;

        tracing::info!("publish_diagnostics EXIT {}", uri.as_str());
    }

    /// Build a TextEdit for inserting an import statement into the script block.
    fn build_auto_import_edit(
        &self,
        doc_uri_str: &str,
        component_name: &str,
        import_path: &str,
    ) -> Option<TextEdit> {
        let uri: Uri = doc_uri_str.parse().ok()?;
        let doc = self.documents.get(&uri)?;
        let blocks = scan_sfc_blocks(&doc.source);

        // Find the script setup block
        let script_block = blocks
            .iter()
            .find(|b| b.tag_name == "script" && b.attrs_raw.contains("setup"))?;

        let (content_start, _content_end) = script_block.content_range();

        // Check if the component is already imported
        if let Some(analysis) = self.documents.get_analysis(&uri) {
            for import in &analysis.imports {
                if import.bindings.iter().any(|b| b.name == component_name) {
                    return None; // Already imported
                }
            }

            // Find the position after the last import statement
            let last_import_end = analysis.imports.iter().map(|imp| imp.span.end).max();

            let insert_offset = if let Some(end) = last_import_end {
                // Insert after the last import — the span_end is relative to script content
                let abs_offset = content_start + end;
                // Skip past the newline after the import
                let rest = &doc.source[abs_offset as usize..];
                let newline_skip = rest
                    .bytes()
                    .take_while(|&b| b == b'\n' || b == b'\r')
                    .count();
                abs_offset + newline_skip as u32
            } else {
                // No existing imports — insert at the beginning of the script block
                content_start
            };

            let import_stmt = format!("import {} from '{}'\n", component_name, import_path);
            let pos = doc.line_index.offset_to_position(insert_offset)?;

            Some(TextEdit {
                range: Range::new(pos, pos),
                new_text: import_stmt,
            })
        } else {
            None
        }
    }

    #[allow(dead_code)] // Used by sync_coordinator, may be useful for future callers
    async fn sync_ide_to_provider(&self, uri: &Uri) {
        let _timer = self
            .statistics
            .timer("ide_sync", Some(uri.as_str().to_string()));
        if let Some(sync) = &self.project_sync {
            if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
                self.documents.host().ensure_loaded(&canonical_id);
            }
            if let Some(ide) = self.documents.get_ide(uri) {
                let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
                    return;
                };
                let Some(transition) =
                    self.prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                else {
                    self.pending_snapshot_provider_sync.insert(canonical_id);
                    tracing::debug!(
                        "sync_ide: resolver snapshot unavailable for {}",
                        uri.as_str()
                    );
                    return;
                };
                self.close_provider_paths(&transition.stale_paths).await;
                let committed_state = transition.next;
                let Some(ide_path) = committed_state.ide_path.clone() else {
                    return;
                };
                tracing::info!("sync_ide: {} ({} bytes)", ide_path, ide.code.len());
                if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                    tracing::warn!("sync_ide: failed for {ide_path}: {e}");
                } else {
                    self.commit_provider_sync_state(&canonical_id, committed_state.clone());
                    tracing::info!("sync_ide: ok for {}", ide_path);
                }
            } else {
                tracing::debug!("sync_ide: no IDE output available for {}", uri.as_str());
            }
        }
    }

    /// Sync the public API (.vue.ts) to the type provider for cross-file component resolution.
    async fn sync_api_to_provider(&self, uri: &Uri) {
        if let Some(sync) = &self.project_sync {
            let canonical_id = match self.documents.get_canonical_id(uri) {
                Some(id) => id,
                None => return,
            };
            self.documents.host().ensure_loaded(&canonical_id);
            if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                if let Some(snapshot) = self.published_resolver() {
                    configure_provider_paths_for_source(sync, &snapshot, &canonical_id, false)
                        .await;
                }
            }
            let Some(transition) = self
                .documents
                .get_ide(uri)
                .and_then(|ide| {
                    self.prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                })
                .or_else(|| {
                    self.prepare_vue_provider_sync_transition(
                        &canonical_id,
                        self.documents.is_jsx(uri),
                    )
                })
            else {
                self.pending_snapshot_provider_sync.insert(canonical_id);
                return;
            };
            self.close_provider_paths(&transition.stale_paths).await;
            let mut committed_state = transition.next;
            if let Some(dts_path) = committed_state.api_path.clone() {
                if let Some(api) = self.documents.host.get_public_api(&canonical_id) {
                    let result = if committed_state.api_background_loaded {
                        sync.sync_dts(&dts_path, &api.code).await
                    } else {
                        sync.open_dts(&dts_path, &api.code).await
                    };
                    if let Err(e) = result {
                        tracing::warn!("sync_api: failed for {dts_path}: {e}");
                    } else {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                        self.commit_provider_sync_state(&canonical_id, committed_state);
                    }
                }
            }
        }
    }

    async fn sync_vue_public_api_by_canonical_id(&self, canonical_id: &str) {
        if let Some(uri) = self.documents.canonical_id_to_uri(canonical_id) {
            self.sync_api_to_provider(&uri).await;
        } else {
            self.resync_background_vue_file(canonical_id).await;
        }
    }

    fn refresh_vue_dependency_tracking(&self, canonical_id: &str) {
        let Some(snapshot) = self.published_resolver() else {
            return;
        };
        let Some(analysis) = self.documents.host().get_analysis(canonical_id) else {
            return;
        };

        let reader = LspProjectResolverReader::new(&self.documents);
        let resolved_dependencies = collect_resolved_provider_dependencies_from_analyzed_refs(
            &snapshot.resolver,
            &reader,
            canonical_id,
            &analysis.module_references,
        );

        self.documents.host.set_import_dependencies(
            canonical_id,
            resolved_dependencies
                .iter()
                .map(|entry| verter_host::DependencyResolution {
                    specifier: entry.provider_specifier.clone(),
                    resolved_canonical_id: Some(entry.source_id.clone()),
                    possible_canonical_ids: Vec::new(),
                })
                .collect(),
        );
    }

    async fn sync_non_vue_file_to_provider(
        &self,
        snapshot: &PublishedResolverSnapshot,
        canonical_id: &str,
        source: Arc<str>,
        module_references: &[verter_host::ScriptModuleReference],
    ) {
        let reader = LspProjectResolverReader::new(&self.documents);
        let Some(prepared) = prepare_non_vue_provider_sync(
            Some(snapshot),
            &reader,
            canonical_id,
            &source,
            module_references,
        ) else {
            return;
        };

        if let Some(sync) = &self.project_sync {
            if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                configure_provider_paths_for_source(sync, snapshot, canonical_id, false).await;
            }
            if let Some(transition) = self.prepare_non_vue_provider_sync_transition(canonical_id) {
                self.close_provider_paths(&transition.stale_paths).await;
                if let Err(error) = sync
                    .sync_file(&prepared.provider_path, &prepared.rewritten)
                    .await
                {
                    tracing::warn!(
                        "failed to sync provider shadow file {}: {error}",
                        prepared.provider_path
                    );
                } else {
                    self.commit_provider_sync_state(canonical_id, transition.next);
                }
            } else if let Err(error) = sync
                .sync_file(&prepared.provider_path, &prepared.rewritten)
                .await
            {
                tracing::warn!(
                    "failed to sync provider shadow file {}: {error}",
                    prepared.provider_path
                );
            }
        }

        if !prepared.resolved_dependencies.is_empty() {
            self.documents.host.set_import_dependencies(
                canonical_id,
                prepared
                    .resolved_dependencies
                    .iter()
                    .map(|entry| verter_host::DependencyResolution {
                        specifier: entry.provider_specifier.clone(),
                        resolved_canonical_id: Some(entry.source_id.clone()),
                        possible_canonical_ids: Vec::new(),
                    })
                    .collect(),
            );
        }

        let vue_targets = prepared
            .resolved_dependencies
            .iter()
            .filter(|dependency| {
                dependency.provider_target == crate::project_resolver::ProviderTarget::VuePublicApi
            })
            .map(|dependency| dependency.source_id.clone())
            .collect::<Vec<_>>();
        for vue_target in vue_targets {
            self.sync_vue_public_api_by_canonical_id(&vue_target).await;
        }

        let non_vue_targets = prepared
            .resolved_dependencies
            .iter()
            .filter(|dependency| {
                dependency.provider_target
                    == crate::project_resolver::ProviderTarget::ShadowSourceFile
                    || (dependency.provider_target
                        == crate::project_resolver::ProviderTarget::SourceFile
                        && dependency.source_id.contains("node_modules"))
            })
            .map(|dependency| dependency.source_id.clone())
            .collect::<Vec<_>>();
        self.sync_non_vue_provider_graph(&snapshot.resolver, non_vue_targets)
            .await;
    }

    async fn sync_non_vue_provider_graph(
        &self,
        resolver: &crate::project_resolver::NativeProjectResolver,
        initial_ids: Vec<String>,
    ) {
        let Some(sync) = &self.project_sync else {
            return;
        };

        let reader = LspProjectResolverReader::new(&self.documents);
        let mut pending = initial_ids;
        let mut seen = HashSet::new();

        while let Some(canonical_id) = pending.pop() {
            if !seen.insert(canonical_id.clone()) || canonical_id.ends_with(".vue") {
                continue;
            }

            let Some(source) = reader.read_file(&canonical_id) else {
                continue;
            };

            let module_references = self
                .documents
                .host
                .upsert(verter_host::UpsertRequest {
                    canonical_id: Some(canonical_id.clone()),
                    input_id: canonical_id.clone(),
                    source: Arc::clone(&source),
                    file_kind: verter_host::FileKind::NonSfc,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default();

            let Some(prepared) = prepare_non_vue_provider_sync(
                Some(&PublishedResolverSnapshot {
                    resolver: resolver.clone(),
                }),
                &reader,
                &canonical_id,
                &source,
                &module_references,
            ) else {
                continue;
            };

            if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                let snapshot = PublishedResolverSnapshot {
                    resolver: resolver.clone(),
                };
                configure_provider_paths_for_source(sync, &snapshot, &canonical_id, true).await;
            }
            if let Some(transition) = self.prepare_non_vue_provider_sync_transition(&canonical_id) {
                self.close_provider_paths(&transition.stale_paths).await;
                if let Err(error) = sync
                    .sync_file(&prepared.provider_path, &prepared.rewritten)
                    .await
                {
                    tracing::warn!(
                        "failed to sync provider shadow file {}: {error}",
                        prepared.provider_path
                    );
                } else {
                    self.commit_provider_sync_state(&canonical_id, transition.next);
                }
            } else if let Err(error) = sync
                .sync_file(&prepared.provider_path, &prepared.rewritten)
                .await
            {
                tracing::warn!(
                    "failed to sync provider shadow file {}: {error}",
                    prepared.provider_path
                );
            }

            let resolved_dependencies = prepared.resolved_dependencies;
            if !resolved_dependencies.is_empty() {
                self.documents.host.set_import_dependencies(
                    &canonical_id,
                    resolved_dependencies
                        .iter()
                        .map(|entry| verter_host::DependencyResolution {
                            specifier: entry.provider_specifier.clone(),
                            resolved_canonical_id: Some(entry.source_id.clone()),
                            possible_canonical_ids: Vec::new(),
                        })
                        .collect(),
                );
            }

            for dependency in resolved_dependencies {
                if dependency.provider_target
                    == crate::project_resolver::ProviderTarget::VuePublicApi
                {
                    self.sync_vue_public_api_by_canonical_id(&dependency.source_id)
                        .await;
                } else if dependency.provider_target
                    == crate::project_resolver::ProviderTarget::ShadowSourceFile
                {
                    pending.push(dependency.source_id.clone());
                } else if dependency.provider_target
                    == crate::project_resolver::ProviderTarget::SourceFile
                    && dependency.source_id.contains("node_modules")
                {
                    // Follow node_modules dependencies transitively
                    pending.push(dependency.source_id.clone());
                }
            }
        }
    }

    fn sync_api_to_provider_in_background(&self, uri: Uri) {
        let Some(sync) = self.project_sync.clone() else {
            return;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(&uri) else {
            return;
        };
        let Some(snapshot) = self.published_resolver() else {
            self.pending_snapshot_provider_sync.insert(canonical_id);
            return;
        };
        let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
        let Some(transition) =
            self.prepare_vue_provider_sync_transition(&canonical_id, self.documents.is_jsx(&uri))
        else {
            self.pending_snapshot_provider_sync.insert(canonical_id);
            return;
        };
        let dts_path = match transition.next.api_path.clone() {
            Some(path) => path,
            None => return,
        };
        let host = self.documents.host_arc();
        let provider_sync_states = Arc::clone(&self.provider_sync_states);
        tokio::spawn(async move {
            if is_tsgo {
                configure_provider_paths_for_source(&sync, &snapshot, &canonical_id, true).await;
            }
            for (kind, path) in &transition.stale_paths {
                let result = match kind {
                    ProviderPathKind::Ide => sync.close_tsx(path).await,
                    ProviderPathKind::Api => sync.close_dts(path).await,
                    ProviderPathKind::Shadow => sync.close_file(path).await,
                };
                if let Err(error) = result {
                    tracing::warn!(
                        "sync_api(background): failed to close stale provider path {path}: {error}"
                    );
                }
            }
            let api = block_in_place_if_available(|| host.get_public_api(&canonical_id));
            if let Some(api) = api {
                let mut committed_state = transition.next;
                let result = if committed_state.api_background_loaded {
                    sync.sync_dts(&dts_path, &api.code).await
                } else {
                    sync.open_dts(&dts_path, &api.code).await
                };
                if let Err(e) = result {
                    tracing::warn!("sync_api(background): failed for {dts_path}: {e}");
                } else {
                    committed_state.set_background_loaded(ProviderPathKind::Api, true);
                    commit_sync_transition(&provider_sync_states, &canonical_id, committed_state);
                }
            }
        });
    }

    /// Flush the active file's IDE TSX to the type provider for interactive queries.
    ///
    /// Called by hover, completion, goto_definition, type_definition BEFORE making
    /// a type provider query. Only syncs the IDE path (TSX) — API (.vue.ts) sync
    /// is deferred to the coordinator.
    ///
    /// Runs when:
    /// - File is in `needs_ide_sync`, OR
    /// - No committed provider sync state exists (first open, timeout retry, failure recovery)
    ///
    /// **With resolver snapshot**: owner-aware IDE sync.
    /// **Without snapshot**: pre-snapshot blocker hydration + provisional IDE sync.
    async fn ensure_current_file_synced(&self, uri: &Uri) {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };

        // Touch MRU for snapshot drain ordering
        self.touch_mru(&canonical_id);

        let has_committed_state = self.provider_sync_states.contains_key(&canonical_id);
        let ide_already_synced = self
            .provider_sync_states
            .get(&canonical_id)
            .map(|s| s.ide_background_loaded)
            .unwrap_or(false);
        let needs_sync = self.needs_ide_sync.remove(&canonical_id).is_some();

        if !needs_sync && has_committed_state && ide_already_synced {
            return; // IDE is fresh
        }

        tracing::info!(
            "ensure_current_file_synced: flushing IDE sync for {} (needs_sync={}, has_state={})",
            uri.as_str(),
            needs_sync,
            has_committed_state,
        );

        let Some(sync) = &self.project_sync else {
            return;
        };

        // Ensure file and its deps are loaded. The scheduler's extract_deps
        // + auto-ingress handles recursive dependency walking.
        self.documents.host().ensure_loaded(&canonical_id);

        // Recompile + refresh mapper (in case blocker hydration changed TSX)
        self.documents.recompile_and_refresh_mapper(uri);

        let ide = self.documents.get_ide(uri);
        let is_jsx = ide.as_ref().map(|r| r.is_jsx).unwrap_or(false);

        // Determine IDE path — owner-aware or provisional
        let (ide_path, provisional) = if let Some(snapshot) = self.published_resolver() {
            if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                configure_provider_paths_for_source(sync, &snapshot, &canonical_id, false).await;
            }
            match provider_ide_path_for_source(&snapshot.resolver, &canonical_id, is_jsx) {
                Some(path) => (path, false),
                None => {
                    self.pending_snapshot_provider_sync
                        .insert(canonical_id.clone());
                    return;
                }
            }
        } else {
            // Provisional: no resolver
            let ext = if is_jsx { ".jsx" } else { ".tsx" };
            (format!("{canonical_id}{ext}"), true)
        };

        let Some(ide) = ide else {
            return;
        };

        // Choose open_file vs update_file based on existing state
        let result = if has_committed_state {
            // Already known to provider — update
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                sync.sync_tsx(&ide_path, &ide.code),
            )
            .await
        } else {
            // First time — open
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                sync.open_tsx(&ide_path, &ide.code),
            )
            .await
        };

        match result {
            Ok(Ok(())) => {
                // Commit state
                let mut state = if provisional {
                    crate::provider_sync::ProviderSyncState {
                        owner_key: "__provisional__".to_string(),
                        ide_path: Some(ide_path),
                        api_path: None,
                        ..Default::default()
                    }
                } else if let Some(snapshot) = self.published_resolver() {
                    crate::provider_sync::vue_sync_state_for_source(
                        &snapshot.resolver,
                        &canonical_id,
                        is_jsx,
                    )
                    .unwrap_or_else(|| {
                        crate::provider_sync::ProviderSyncState {
                            owner_key: "__provisional__".to_string(),
                            ide_path: Some(ide_path.clone()),
                            api_path: None,
                            ..Default::default()
                        }
                    })
                } else {
                    // Snapshot vanished between check and use (rare race) — provisional fallback
                    crate::provider_sync::ProviderSyncState {
                        owner_key: "__provisional__".to_string(),
                        ide_path: Some(ide_path),
                        api_path: None,
                        ..Default::default()
                    }
                };
                state.set_background_loaded(ProviderPathKind::Ide, true);
                self.commit_provider_sync_state(&canonical_id, state);
                if provisional {
                    self.pending_snapshot_provider_sync
                        .insert(canonical_id.clone());
                }
                // Queue deferred API sync
                self.needs_deferred_sync.insert(canonical_id);
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    "ensure_current_file_synced: IDE sync failed for {}: {e}",
                    uri.as_str()
                );
                self.needs_ide_sync.insert(canonical_id);
            }
            Err(_) => {
                tracing::warn!(
                    "ensure_current_file_synced: IDE sync timed out for {}",
                    uri.as_str()
                );
                self.needs_ide_sync.insert(canonical_id);
            }
        }
    }

    async fn force_reopen_current_file_in_type_provider(&self, uri: &Uri) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };

        self.documents.recompile_and_refresh_mapper(uri);

        let Some(ide) = self.documents.get_ide(uri) else {
            return;
        };
        let Some(ide_path) = self.ide_path_for_uri(uri) else {
            return;
        };

        if let Err(error) = sync.close_tsx(&ide_path).await {
            tracing::warn!(
                "force_reopen_current_file_in_type_provider: failed to close {}: {error}",
                ide_path
            );
        }

        match sync.open_tsx(&ide_path, &ide.code).await {
            Ok(()) => {
                if let Some(mut state) = self.provider_sync_state_for_source(&canonical_id) {
                    state.ide_path = Some(ide_path);
                    self.commit_provider_sync_state(&canonical_id, state);
                }
            }
            Err(error) => {
                tracing::warn!(
                    "force_reopen_current_file_in_type_provider: failed to reopen {}: {error}",
                    uri.as_str()
                );
                self.needs_ide_sync.insert(canonical_id);
            }
        }
    }

    /// Legacy wrapper for backward compat — calls `ensure_current_file_synced`.
    async fn ensure_provider_synced(&self, uri: &Uri) {
        self.ensure_current_file_synced(uri).await;
        self.ensure_imported_vue_apis_synced(uri).await;
        self.ensure_barrel_imports_synced_for_tsgo(uri).await;
    }

    async fn ensure_imported_vue_apis_synced(&self, uri: &Uri) {
        if matches!(self.type_provider_kind, crate::TypeProviderKind::None) {
            return;
        }

        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return;
        };

        let mut import_ids = collect_imported_vue_priority_ids_from_imports_with_fallback(
            &analysis.imports,
            Some(&canonical_id),
            |parent, specifier| self.resolve_import_specifier(parent, specifier),
        );

        let snapshot = self.published_resolver();
        let reader = LspProjectResolverReader::new(&self.documents);
        let dynamic_ids = collect_priority_vue_targets_from_module_references(
            snapshot.as_ref(),
            &reader,
            &canonical_id,
            &analysis.module_references,
        );
        let mut seen: HashSet<String> = import_ids.iter().cloned().collect();
        for import_id in dynamic_ids {
            if seen.insert(import_id.clone()) {
                import_ids.push(import_id);
            }
        }

        for import_id in import_ids {
            self.sync_imported_vue_api_lightweight(&import_id).await;
        }
    }

    /// Sync barrel (non-Vue re-export) imports and their Vue dependencies to TSGO.
    ///
    /// When a Vue file imports components through a barrel (`import { Comp } from './components'`),
    /// `ensure_imported_vue_apis_synced` misses both the barrel and its Vue re-export targets
    /// because the barrel is a `.ts` file. This method discovers barrels from template component
    /// usages, syncs their Vue dependencies first, then syncs the barrel itself.
    async fn ensure_barrel_imports_synced_for_tsgo(&self, uri: &Uri) {
        if !matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
            return;
        }
        let Some(sync) = &self.project_sync else {
            return;
        };
        let Some(snapshot) = self.published_resolver() else {
            return;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return;
        };
        let Some(template) = analysis.template.as_ref() else {
            return;
        };

        let host = self.documents.host();
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_vue_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_vue = HashSet::new();

        for component in &template.components {
            let Some(import_source) = component.import_source.as_deref() else {
                continue;
            };
            let Some(resolved) = self.resolve_import_specifier(&canonical_id, import_source) else {
                continue;
            };
            if resolved.ends_with(".vue") {
                continue; // already handled by Vue sync
            }
            if !seen_barrels.insert(resolved.clone()) {
                continue;
            }

            // Load barrel into host and scan its module references for Vue specifiers
            host.ensure_loaded(&resolved);

            if let Some(barrel_analysis) = host.get_analysis(&resolved) {
                for module_ref in barrel_analysis.module_references.iter() {
                    if let Some(specifier) = &module_ref.literal_specifier {
                        if specifier.ends_with(".vue") {
                            if let Some(vue_id) =
                                self.resolve_import_specifier(&resolved, specifier)
                            {
                                if vue_id.ends_with(".vue")
                                    && seen_barrel_vue.insert(vue_id.clone())
                                {
                                    barrel_vue_deps.push(vue_id);
                                }
                            }
                        }
                    }
                }
            }

            barrel_ids.push(resolved);
        }

        // Phase 1: Sync Vue dependencies first (so TSGO has .vue.ts targets)
        for vue_id in &barrel_vue_deps {
            self.sync_imported_vue_api_lightweight(vue_id).await;
        }

        // Phase 2: Sync barrel files (TSGO's rewrite_vue_imports_for_tsgo handles .vue → .vue.ts)
        for barrel_id in &barrel_ids {
            // Skip if already synced
            if let Some(state) = self.provider_sync_state_for_source(barrel_id) {
                if state.shadow_background_loaded {
                    continue;
                }
            }

            let Some(source) = host.get_source(barrel_id) else {
                continue;
            };
            let module_references = block_in_place_if_available(|| {
                host.upsert(verter_host::UpsertRequest {
                    canonical_id: Some(barrel_id.clone()),
                    input_id: barrel_id.clone(),
                    source: source.clone(),
                    file_kind: verter_host::FileKind::NonSfc,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default()
            });
            let reader = LspProjectResolverReader::new(&self.documents);
            let Some(prepared) = prepare_non_vue_provider_sync(
                Some(&snapshot),
                &reader,
                barrel_id,
                &source,
                &module_references,
            ) else {
                continue;
            };

            configure_provider_paths_for_source(sync, &snapshot, barrel_id, false).await;

            if let Some(transition) = self.prepare_non_vue_provider_sync_transition(barrel_id) {
                self.close_provider_paths(&transition.stale_paths).await;
                if let Err(error) = sync
                    .sync_file(&prepared.provider_path, &prepared.rewritten)
                    .await
                {
                    tracing::warn!(
                        "barrel sync: failed to sync {}: {error}",
                        prepared.provider_path
                    );
                } else {
                    self.commit_provider_sync_state(barrel_id, transition.next);
                }
            } else if let Err(error) = sync
                .sync_file(&prepared.provider_path, &prepared.rewritten)
                .await
            {
                tracing::warn!(
                    "barrel sync: failed to sync {}: {error}",
                    prepared.provider_path
                );
            }
        }
    }

    fn current_file_needs_inline_type_provider_sync(&self, uri: &Uri) -> bool {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return false;
        };

        if self.needs_ide_sync.contains(&canonical_id) {
            return true;
        }

        let Some(state) = self.provider_sync_state_for_source(&canonical_id) else {
            return true;
        };

        if !state.ide_background_loaded {
            return true;
        }

        let Some(ide_path) = self.ide_path_for_uri(uri) else {
            return false;
        };

        state.ide_path.as_deref() != Some(ide_path.as_str())
    }

    /// Returns true if the user is actively typing (last change was within the cooldown window).
    /// Used to suppress non-critical TSGO requests (diagnostics, semantic tokens, inlay hints)
    /// during rapid typing.  TSGO processes requests serially, so queuing these during typing
    /// blocks interactive requests like completions.
    fn is_typing_cooldown(&self) -> bool {
        let last = self
            .last_change_ms
            .load(std::sync::atomic::Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now.saturating_sub(last) < 300
    }

    /// Get IDE context for TypeProvider queries: (ide_path, ide_code, position_mapper).
    fn ide_context(&self, uri: &Uri) -> Option<(String, Arc<str>, PositionMapper)> {
        let canonical_id = self.documents.get_canonical_id(uri);
        if canonical_id.is_none() {
            tracing::info!("ide_context: no canonical_id for {}", uri.as_str());
            return None;
        }
        self.documents
            .host()
            .ensure_loaded(canonical_id.as_deref()?);
        let Some(ide) = self.documents.get_ide(uri) else {
            tracing::info!(
                "ide_context: no IDE output for {} (canonical={})",
                uri.as_str(),
                canonical_id.as_deref().unwrap_or("?")
            );
            return None;
        };

        let Some(mapper) = self.documents.get_position_mapper(uri) else {
            tracing::info!("ide_context: no position mapper for {}", uri.as_str());
            return None;
        };
        let ide_path = self.ide_path_for_uri(uri)?;
        Some((ide_path, ide.code, mapper))
    }

    /// Load the current workspace snapshot's resolver, if a published snapshot exists.
    ///
    /// Returns a `ResolverSnapshot`-like wrapper with a `.resolver` field for
    /// compatibility with existing access patterns (`snapshot.resolver.method()`).
    fn published_resolver(&self) -> Option<PublishedResolverSnapshot> {
        let ws = self.vfs_workspace.read();
        let ws = ws.as_ref()?;
        let published = ws.load_published()?;
        Some(PublishedResolverSnapshot {
            resolver: published.snapshot.resolver.clone(),
        })
    }

    /// Check if a file is in an SSR context using the published LspViews.
    fn is_ssr_context(&self, canonical_id: &str) -> bool {
        let ws = self.vfs_workspace.read();
        ws.as_ref()
            .and_then(|ws| ws.load_published())
            .and_then(|published| {
                let views = published.ext::<crate::workspace_state::LspViews>()?;
                Some(views.is_ssr_context(&published.snapshot, canonical_id))
            })
            .unwrap_or(false)
    }

    /// Find the project root for a file using the published LspViews.
    #[allow(dead_code)]
    fn find_project_root(&self, canonical_id: &str) -> Option<String> {
        let ws = self.vfs_workspace.read();
        let ws = ws.as_ref()?;
        let published = ws.load_published()?;
        let views = published.ext::<crate::workspace_state::LspViews>()?;
        views
            .find_project_root(&published.snapshot, canonical_id)
            .map(|s| s.to_string())
    }

    /// Get a linter for a file from the published LspViews. Returns a default linter
    /// if no published snapshot exists or the file has no owner.
    fn linter_for_file(&self, canonical_id: &str) -> verter_diagnostics::Linter {
        let ws = self.vfs_workspace.read();
        if let Some(ws) = ws.as_ref() {
            if let Some(published) = ws.load_published() {
                if let Some(views) = published.ext::<crate::workspace_state::LspViews>() {
                    if let Some(view) =
                        views.linter_view_for_file(&published.snapshot, canonical_id)
                    {
                        return verter_diagnostics::Linter::new(view.lint_config.config.clone());
                    }
                }
            }
        }
        verter_diagnostics::Linter::default()
    }

    /// Generate a provisional IDE file path (.tsx or .jsx) without resolver.
    ///
    /// Mirrors `provider_ide_id_for_source()` but skips `owner_for_file()` — used
    /// before `background_init()` finishes building the resolver snapshot.
    fn provisional_ide_path(&self, uri: &Uri) -> Option<String> {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        if !canonical.ends_with(".vue") {
            return None;
        }
        let ext = if self.documents.is_jsx(uri) {
            ".jsx"
        } else {
            ".tsx"
        };
        Some(format!("{canonical}{ext}"))
    }

    /// Generate a provisional public API path (.vue.ts) without resolver ownership.
    ///
    /// Mirrors `provider_id_for_source()` for Vue files and is used during cold
    /// start before `background_init()` has built the resolver snapshot.
    fn provisional_api_path_for_canonical_id(&self, canonical_id: &str) -> Option<String> {
        canonical_id
            .ends_with(".vue")
            .then(|| format!("{canonical_id}.ts"))
    }

    async fn sync_vue_ide_provisionally(
        &self,
        canonical_id: &str,
        ide_code: &str,
        is_jsx: bool,
    ) -> bool {
        let Some(sync) = &self.project_sync else {
            return false;
        };
        let ext = if is_jsx { ".jsx" } else { ".tsx" };
        let ide_path = format!("{canonical_id}{ext}");

        let mut state = self
            .provider_sync_state_for_source(canonical_id)
            .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
                owner_key: "__provisional__".to_string(),
                ..Default::default()
            });

        if state.owner_key.is_empty() {
            state.owner_key = "__provisional__".to_string();
        }

        let needs_open =
            state.ide_path.as_deref() != Some(ide_path.as_str()) || !state.ide_background_loaded;
        let result = if needs_open {
            sync.open_tsx(&ide_path, ide_code).await
        } else {
            sync.sync_tsx(&ide_path, ide_code).await
        };

        match result {
            Ok(()) => {
                state.ide_path = Some(ide_path);
                state.ide_background_loaded = true;
                self.commit_provider_sync_state(canonical_id, state);
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                true
            }
            Err(error) => {
                tracing::warn!("sync_vue_ide_provisionally: failed for {canonical_id}: {error}");
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                false
            }
        }
    }

    async fn sync_vue_api_provisionally(&self, canonical_id: &str, api_code: &str) -> bool {
        let Some(sync) = &self.project_sync else {
            return false;
        };
        let Some(dts_path) = self.provisional_api_path_for_canonical_id(canonical_id) else {
            return false;
        };

        let mut state = self
            .provider_sync_state_for_source(canonical_id)
            .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
                owner_key: "__provisional__".to_string(),
                ..Default::default()
            });

        if state.owner_key.is_empty() {
            state.owner_key = "__provisional__".to_string();
        }

        let needs_open =
            state.api_path.as_deref() != Some(dts_path.as_str()) && !state.api_background_loaded;
        let result = if needs_open {
            sync.open_dts(&dts_path, api_code).await
        } else {
            sync.sync_dts(&dts_path, api_code).await
        };

        match result {
            Ok(()) => {
                state.api_path = Some(dts_path);
                state.api_background_loaded = true;
                self.commit_provider_sync_state(canonical_id, state);
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                true
            }
            Err(error) => {
                tracing::warn!("sync_vue_api_provisionally: failed for {canonical_id}: {error}");
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                false
            }
        }
    }

    /// Generate the IDE file path (.tsx or .jsx) for a given Vue file URI.
    /// Falls back to `provisional_ide_path` when no resolver snapshot is available.
    fn ide_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        if let Some(snapshot) = self.published_resolver() {
            return provider_ide_path_for_source(
                &snapshot.resolver,
                &canonical,
                self.documents.is_jsx(uri),
            );
        }
        // Fallback: provisional path without resolver
        self.provisional_ide_path(uri)
    }

    /// Get IDE content and mapper by IDE path (reverse lookup).
    fn ide_context_by_path(&self, ide_path: &str) -> Option<(String, Arc<str>, PositionMapper)> {
        let snapshot = self.published_resolver()?;
        let canonical_id =
            source_id_from_provider_vue_path(&snapshot.resolver, self.documents.host(), ide_path)?;
        let uri = self.documents.canonical_id_to_uri(&canonical_id)?;
        self.ide_context(&uri)
    }

    fn resolve_import_specifier(
        &self,
        parent_canonical_id: &str,
        specifier: &str,
    ) -> Option<String> {
        self.documents
            .host()
            .resolve_import_via_workspace(parent_canonical_id, specifier)
    }

    fn component_import_binding_name(
        &self,
        analysis: &verter_host::FileAnalysisSnapshot,
        component: &verter_analysis::template::TemplateComponentUsage,
    ) -> Option<String> {
        let import_source = component.import_source.as_ref()?;
        let import = analysis
            .imports
            .iter()
            .find(|import| import.source == *import_source)?;

        import
            .bindings
            .iter()
            .find(|binding| {
                binding.name == component.name || to_pascal_case(&binding.name) == component.name
            })
            .map(|binding| binding.name.clone())
            .or_else(|| import.bindings.first().map(|binding| binding.name.clone()))
            .or_else(|| Some("default".to_string()))
    }

    fn resolve_component_document_for_usage(
        &self,
        parent_uri: &Uri,
        parent_analysis: &verter_host::FileAnalysisSnapshot,
        component: &verter_analysis::template::TemplateComponentUsage,
    ) -> Option<ResolvedComponentDocument> {
        let import_source = component.import_source.as_ref()?;
        let parent_canonical_id = uri_to_canonical_id(parent_uri);
        let binding_name = self.component_import_binding_name(parent_analysis, component);
        let import = parent_analysis
            .imports
            .iter()
            .find(|import| import.source == *import_source);
        let mut resolved_targets = Vec::new();
        if let Some(resolved) = import.and_then(|entry| entry.resolved_canonical_id.clone()) {
            resolved_targets.push(resolved);
        }
        if let Some(resolved) = self.resolve_import_specifier(&parent_canonical_id, import_source) {
            if !resolved_targets
                .iter()
                .any(|candidate| candidate == &resolved)
            {
                resolved_targets.push(resolved);
            }
        }

        let child_canonical_id = resolved_targets.into_iter().find_map(|resolved_target| {
            if resolved_target.ends_with(".vue") {
                return Some(resolved_target);
            }

            binding_name.as_deref().and_then(|binding| {
                self.documents
                    .host()
                    .get_export_span_follow_reexports(&resolved_target, binding)
                    .map(|(resolved_id, _, _)| resolved_id)
                    .filter(|resolved_id| resolved_id.ends_with(".vue"))
            })
        })?;

        let child_analysis = self.documents.host().get_analysis(&child_canonical_id)?;
        let child_source = self.documents.host().get_source(&child_canonical_id)?;
        let child_line_index = LineIndex::new(&child_source, self.documents.encoding());
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;

        Some(ResolvedComponentDocument {
            uri: child_uri,
            analysis: child_analysis,
            line_index: child_line_index,
        })
    }

    fn resolve_component_document_for_import_binding(
        &self,
        parent_uri: &Uri,
        parent_analysis: &verter_host::FileAnalysisSnapshot,
        import_source: &str,
        binding_name: &str,
    ) -> Option<ResolvedComponentDocument> {
        let parent_canonical_id = uri_to_canonical_id(parent_uri);
        let import = parent_analysis
            .imports
            .iter()
            .find(|import| import.source == import_source);
        let mut resolved_targets = Vec::new();
        if let Some(resolved) = import.and_then(|entry| entry.resolved_canonical_id.clone()) {
            resolved_targets.push(resolved);
        }
        if let Some(resolved) = self.resolve_import_specifier(&parent_canonical_id, import_source) {
            if !resolved_targets
                .iter()
                .any(|candidate| candidate == &resolved)
            {
                resolved_targets.push(resolved);
            }
        }

        let child_canonical_id = resolved_targets.into_iter().find_map(|resolved_target| {
            if resolved_target.ends_with(".vue") {
                return Some(resolved_target);
            }

            self.documents
                .host()
                .get_export_span_follow_reexports(&resolved_target, binding_name)
                .map(|(resolved_id, _, _)| resolved_id)
                .filter(|resolved_id| resolved_id.ends_with(".vue"))
        })?;

        let child_analysis = self.documents.host().get_analysis(&child_canonical_id)?;
        let child_source = self.documents.host().get_source(&child_canonical_id)?;
        let child_line_index = LineIndex::new(&child_source, self.documents.encoding());
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;

        Some(ResolvedComponentDocument {
            uri: child_uri,
            analysis: child_analysis,
            line_index: child_line_index,
        })
    }

    fn collect_component_event_definition_locations(
        &self,
        child: &ResolvedComponentDocument,
        event_name: &str,
    ) -> Vec<Location> {
        let mut locations = Vec::new();
        let mut seen = HashSet::new();

        let mut emit_locations = Vec::new();
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_analysis::AnalyzedMacroKind::DefineEmits {
                continue;
            }
            for emit_field in &mac.emit_fields {
                if let Some(rank) = event_name_match_rank(event_name, &emit_field.name) {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, emit_field.span)
                    {
                        emit_locations.push((rank, location));
                    }
                }
            }
        }
        if let Some(template) = child.analysis.template.as_ref() {
            for emit in &template.emit_definitions {
                if !emit.is_declared {
                    continue;
                }
                if let Some(rank) = event_name_match_rank(event_name, &emit.event_name) {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, emit.span)
                    {
                        emit_locations.push((rank, location));
                    }
                }
            }
        }
        emit_locations.sort_by_key(|(rank, location)| {
            (
                *rank,
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            )
        });
        for (_, location) in emit_locations {
            push_unique_location(&mut locations, &mut seen, location);
        }

        let prop_candidates = listener_prop_candidates(event_name);
        let mut prop_locations = Vec::new();
        for mac in child.analysis.macros.iter() {
            for prop_field in &mac.prop_fields {
                if let Some(rank) = prop_candidates
                    .iter()
                    .position(|candidate| candidate == &prop_field.name)
                {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, prop_field.span)
                    {
                        prop_locations.push((rank, location));
                    }
                }
            }
        }
        if let Some(template) = child.analysis.template.as_ref() {
            for prop_definition in &template.prop_definitions {
                if let Some(rank) = prop_candidates
                    .iter()
                    .position(|candidate| candidate == &prop_definition.name)
                {
                    if let Some(location) =
                        location_from_span(&child.uri, &child.line_index, prop_definition.span)
                    {
                        prop_locations.push((rank, location));
                    }
                }
            }
        }
        prop_locations.sort_by_key(|(rank, location)| {
            (
                *rank,
                location.range.start.line,
                location.range.start.character,
                location.range.end.line,
                location.range.end.character,
            )
        });
        for (_, location) in prop_locations {
            push_unique_location(&mut locations, &mut seen, location);
        }

        locations
    }

    fn resolve_definition_path(&self, canonical_id: &str, specifier: &str) -> Option<String> {
        self.resolve_import_specifier(canonical_id, specifier)
    }

    fn resolve_precise_export_location(
        &self,
        target_canonical_id: &str,
        binding_name: &str,
    ) -> Option<Location> {
        let host = &self.documents.host;
        let (resolved_id, start, end) = host
            .get_export_span_follow_reexports(target_canonical_id, binding_name)
            .or_else(|| {
                let (s, e) = host.get_export_span(target_canonical_id, binding_name)?;
                Some((target_canonical_id.to_string(), s, e))
            })?;
        let target_source = host.get_source(&resolved_id)?;
        let target_li = LineIndex::new(&target_source, self.position_encoding.read().clone());
        let start_pos = target_li.offset_to_position(start)?;
        let end_pos = target_li.offset_to_position(end)?;
        Some(Location {
            uri: merge::file_path_to_uri(&resolved_id)?,
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        })
    }

    fn resolve_template_identifier(
        &self,
        uri: &Uri,
        analysis: &verter_host::FileAnalysisSnapshot,
        line_index: &LineIndex,
        word: &str,
    ) -> Option<GotoDefinitionResponse> {
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name != word {
                    continue;
                }

                if let Some(canonical_id) = import.resolved_canonical_id.as_deref() {
                    if let Some(location) =
                        self.resolve_precise_export_location(canonical_id, &binding.name)
                    {
                        return Some(GotoDefinitionResponse::Scalar(location));
                    }
                    if canonical_id.ends_with(".vue") {
                        if let Some(location) =
                            self.resolve_precise_export_location(canonical_id, "default")
                        {
                            return Some(GotoDefinitionResponse::Scalar(location));
                        }
                    }
                }

                if let Some(resolved) =
                    self.resolve_definition_path(&uri_to_canonical_id(uri), &import.source)
                {
                    if let Some(location) =
                        self.resolve_precise_export_location(&resolved, &binding.name)
                    {
                        return Some(GotoDefinitionResponse::Scalar(location));
                    }
                    if resolved.ends_with(".vue") {
                        if let Some(location) =
                            self.resolve_precise_export_location(&resolved, "default")
                        {
                            return Some(GotoDefinitionResponse::Scalar(location));
                        }
                    }
                }

                if let Some(location) = location_from_span(uri, line_index, binding.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }

        if let Some(binding) = analysis
            .bindings
            .iter()
            .find(|binding| binding.name == word)
        {
            if let Some(location) = location_from_span(uri, line_index, binding.span) {
                return Some(GotoDefinitionResponse::Scalar(location));
            }
        }

        for mac in analysis.macros.iter() {
            if let Some(prop_field) = mac.prop_fields.iter().find(|field| field.name == word) {
                if let Some(location) = location_from_span(uri, line_index, prop_field.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }

            if mac.binding_name.as_deref() == Some(word) {
                if let Some(location) = location_from_span(uri, line_index, mac.span) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
        }

        None
    }

    /// Unified component contract resolution: props, events, v-model, slots.
    /// Runs BEFORE `definition_at_position` and returns `Some` if any contract
    /// surface was hit, or `None` to fall through to normal definition logic.
    fn try_component_contract_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let template = analysis.template.as_ref()?;
        let offset = doc.line_index.position_to_offset(position)?;

        for element in &template.elements {
            if !element.is_component && element.tag != "template" {
                continue;
            }

            // For <template #slot> elements, find the parent component
            let (component, child) = if element.tag == "template" {
                // Walk up to the parent element to find the component
                let parent_idx = match element.parent_index {
                    Some(idx) => idx as usize,
                    None => continue,
                };
                let parent_element = match template.elements.get(parent_idx) {
                    Some(element) => element,
                    None => continue,
                };
                if !parent_element.is_component {
                    continue;
                }
                let comp = match template.components.iter().find(|c| {
                    offset >= c.span.start
                        && offset < c.span.end
                        && (c.name == parent_element.tag
                            || c.name == to_pascal_case(&parent_element.tag))
                }) {
                    Some(component) => component,
                    None => continue,
                };
                let child = match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(child) => child,
                    None => continue,
                };
                (comp, child)
            } else {
                let comp = template.components.iter().find(|c| {
                    offset >= c.span.start
                        && offset < c.span.end
                        && (c.name == element.tag || c.name == to_pascal_case(&element.tag))
                });
                let comp = match comp {
                    Some(c) => c,
                    None => continue,
                };
                let child = match self.resolve_component_document_for_usage(uri, &analysis, comp) {
                    Some(c) => c,
                    None => continue,
                };
                (comp, child)
            };

            // ── Props ───────────────────────────────────────────────
            for prop in &component.props {
                if offset >= prop.name_span.start && offset < prop.name_span.end {
                    let mut locations = Vec::new();

                    // For shorthand props, also resolve the parent binding
                    if prop.is_shorthand {
                        if let Some(parent_def) = self.resolve_template_identifier(
                            uri,
                            &analysis,
                            &doc.line_index,
                            &prop.name,
                        ) {
                            match parent_def {
                                GotoDefinitionResponse::Scalar(loc) => locations.push(loc),
                                GotoDefinitionResponse::Array(locs) => locations.extend(locs),
                                GotoDefinitionResponse::Link(links) => {
                                    locations.extend(links.into_iter().map(|link| Location {
                                        uri: link.target_uri,
                                        range: link.target_selection_range,
                                    }));
                                }
                            }
                        }
                    }

                    // Find matching prop field in child's defineProps
                    let mut child_found = false;
                    for mac in child.analysis.macros.iter() {
                        if let Some(prop_field) =
                            mac.prop_fields.iter().find(|f| f.name == prop.name)
                        {
                            if let Some(loc) =
                                location_from_span(&child.uri, &child.line_index, prop_field.span)
                            {
                                locations.push(loc);
                                child_found = true;
                            }
                        }
                    }
                    // Fallback: template-level prop definitions
                    if !child_found {
                        if let Some(child_template) = child.analysis.template.as_ref() {
                            if let Some(prop_def) = child_template
                                .prop_definitions
                                .iter()
                                .find(|d| d.name == prop.name)
                            {
                                if let Some(loc) =
                                    location_from_span(&child.uri, &child.line_index, prop_def.span)
                                {
                                    locations.push(loc);
                                    child_found = true;
                                }
                            }
                        }
                    }
                    // Final fallback: navigate to child file
                    if !child_found && !prop.is_shorthand {
                        locations.push(Location {
                            uri: child.uri.clone(),
                            range: Range::default(),
                        });
                    }

                    if !locations.is_empty() {
                        return Some(goto_response_from_locations(locations));
                    }
                }
            }

            // ── Events (v-on) ───────────────────────────────────────
            for directive in &element.directives {
                if directive.name == "on" {
                    if let Some(arg_span) = directive.arg_span {
                        if offset >= arg_span.start && offset < arg_span.end {
                            let event_name = directive.argument.as_deref()?;
                            let locations = self
                                .collect_component_event_definition_locations(&child, event_name);
                            return if locations.is_empty() {
                                None
                            } else {
                                Some(goto_response_from_locations(locations))
                            };
                        }
                    }
                }
            }

            // ── V-model ─────────────────────────────────────────────
            for directive in &element.directives {
                if directive.name != "model" {
                    continue;
                }

                // Named v-model: `v-model:title="t"` — cursor on "title" (the arg)
                if let Some(arg_span) = directive.arg_span {
                    if offset >= arg_span.start && offset < arg_span.end {
                        let model_name = directive.argument.as_deref().unwrap_or("modelValue");
                        return self.resolve_vmodel_definition(&child, model_name);
                    }
                }

                // Plain v-model: `v-model="val"` — cursor on the directive name ("v-model")
                // The name area spans from directive.span.start up to name_end
                if directive.argument.is_none()
                    && offset >= directive.span.start
                    && offset < directive.name_end
                {
                    return self.resolve_vmodel_definition(&child, "modelValue");
                }
            }

            // ── Slot name (v-slot / #) ──────────────────────────────
            for directive in &element.directives {
                if directive.name != "slot" {
                    continue;
                }

                // Slot name: cursor on arg_span (#header → "header")
                if let Some(arg_span) = directive.arg_span {
                    if offset >= arg_span.start && offset < arg_span.end {
                        let slot_name = directive.argument.as_deref().unwrap_or("default");
                        return self.resolve_slot_name_definition(&child, slot_name);
                    }
                }

                // Slot-prop binding: cursor inside expression_span (#default="{ item }")
                if let Some(expr_span) = directive.expression_span {
                    if offset >= expr_span.start && offset < expr_span.end {
                        let slot_name = directive.argument.as_deref().unwrap_or("default");
                        // Find the word under cursor
                        let source_bytes = doc.source.as_bytes();
                        let word = extract_word_at_offset(source_bytes, offset, expr_span);
                        if let Some(word) = word {
                            return self.resolve_slot_binding_definition(&child, slot_name, &word);
                        }
                    }
                }
            }
        }

        None
    }

    /// Resolve barrel-file export clicks to terminal target.
    ///
    /// When the cursor is on an `ExportSignature` that is a re-export
    /// (has `reexport_source`), follow the chain to the terminal declaration.
    fn try_barrel_export_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let offset = doc.line_index.position_to_offset(position)?;

        let encoding = self.position_encoding.read().clone();
        let host = &self.documents.host;
        let canonical_id = uri_to_canonical_id(uri);

        for sig in analysis.export_signatures.iter() {
            // Only handle re-exports (has a source module)
            if sig.reexport_source.is_none() {
                continue;
            }

            // Check if cursor is on the exported name span
            let on_exported = offset >= sig.span.start && offset < sig.span.end;

            // Check if cursor is on the local name span (for aliased re-exports)
            let on_local = sig
                .local_span
                .as_ref()
                .is_some_and(|ls| offset >= ls.start && offset < ls.end);

            if !on_exported && !on_local {
                continue;
            }

            // Determine the binding name to follow in the target module
            let binding_to_follow = if on_local {
                // Clicking on local side (e.g., `default` in `export { default as Popup }`)
                // Follow this local name in the target
                sig.reexport_local.as_deref().unwrap_or(sig.name.as_str())
            } else {
                // Clicking on exported side (e.g., `Overlay` in `export { default as Overlay }`)
                // The name exported from this file; follow via get_export_span_follow_reexports
                sig.name.as_str()
            };

            // Follow the re-export chain to the terminal
            let terminal = if on_local {
                // For local side, resolve the source module first, then follow
                let resolved = host.resolve_import(&canonical_id, sig.reexport_source.as_ref()?)?;
                let local_name = sig.reexport_local.as_deref().unwrap_or(sig.name.as_str());
                host.get_export_span_follow_reexports(&resolved, local_name)
            } else {
                host.get_export_span_follow_reexports(&canonical_id, binding_to_follow)
            };

            if let Some((resolved_id, start, end)) = terminal {
                let target_source = host.get_source(&resolved_id)?;
                let target_li = LineIndex::new(&target_source, encoding);
                let start_pos = target_li.offset_to_position(start)?;
                let end_pos = target_li.offset_to_position(end)?;
                let target_uri = merge::file_path_to_uri(&resolved_id)?;
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                }));
            }
        }

        None
    }

    fn canonicalize_provider_path(path: &str) -> String {
        let normalized = path.trim().replace('\\', "/");
        if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
            let mut chars = normalized.chars();
            let first = chars.next().unwrap_or_default().to_ascii_lowercase();
            format!("{first}{}", chars.as_str())
        } else {
            normalized
        }
    }

    /// Resolve a raw type-provider location that lands on a barrel file to the terminal target.
    fn resolve_barrel_type_provider_location(
        &self,
        path: &str,
        start: u32,
        end: u32,
    ) -> Option<Location> {
        let canonical = Self::canonicalize_provider_path(path);
        let host = &self.documents.host;
        let analysis = host.get_analysis(&canonical)?;
        let (sig, matched_local) = analysis.export_signatures.iter().find_map(|sig| {
            sig.reexport_source.as_ref()?;
            if sig.span.start <= start && end <= sig.span.end {
                return Some((sig, false));
            }
            if let Some(local_span) = sig.local_span.as_ref() {
                if local_span.start <= start && end <= local_span.end {
                    return Some((sig, true));
                }
            }
            None
        })?;

        let (terminal_id, terminal_start, terminal_end) = if matched_local {
            let target = host.resolve_import(&canonical, sig.reexport_source.as_ref()?)?;
            let binding = sig.reexport_local.as_deref().unwrap_or(sig.name.as_str());
            host.get_export_span_follow_reexports(&target, binding)?
        } else {
            host.get_export_span_follow_reexports(&canonical, &sig.name)?
        };

        let source = host.get_source(&terminal_id)?;
        let line_index = LineIndex::new(&source, self.position_encoding.read().clone());
        let start_pos = line_index.offset_to_position(terminal_start)?;
        let end_pos = line_index.offset_to_position(terminal_end)?;
        let uri = merge::file_path_to_uri(&terminal_id)?;
        Some(Location {
            uri,
            range: Range {
                start: start_pos,
                end: end_pos,
            },
        })
    }

    /// Post-process type provider definition results to follow barrel re-exports.
    ///
    /// When the type provider returns a location in a barrel file (`.ts`/`.js` with
    /// re-exports), resolve each location to the terminal declaration so the user
    /// doesn't land in the barrel file.
    fn resolve_barrel_locations(
        &self,
        response: Option<GotoDefinitionResponse>,
    ) -> Option<GotoDefinitionResponse> {
        let response = response?;
        let encoding = self.position_encoding.read().clone();
        let host = &self.documents.host;

        let resolve_location = |loc: Location| -> Location {
            let canonical = uri_to_canonical_id(&loc.uri);
            // Check if this file has re-export signatures at the target position
            if let Some(analysis) = host.get_analysis(&canonical) {
                // Find which export signature the target position falls within
                if let Some(source) = host.get_source(&canonical) {
                    let target_li = LineIndex::new(&source, encoding.clone());
                    if let Some(offset) = target_li.position_to_offset(&loc.range.start) {
                        for sig in analysis.export_signatures.iter() {
                            if sig.reexport_source.is_none() {
                                continue;
                            }
                            let on_sig = offset >= sig.span.start && offset < sig.span.end;
                            let on_local = sig
                                .local_span
                                .as_ref()
                                .is_some_and(|ls| offset >= ls.start && offset < ls.end);
                            if !on_sig && !on_local {
                                continue;
                            }
                            // Follow to terminal
                            if let Some(end_offset) = target_li.position_to_offset(&loc.range.end) {
                                if let Some(resolved) = self.resolve_barrel_type_provider_location(
                                    &canonical, offset, end_offset,
                                ) {
                                    return resolved;
                                }
                            }
                            break;
                        }
                    }
                }
            }
            loc
        };

        Some(match response {
            GotoDefinitionResponse::Scalar(loc) => {
                GotoDefinitionResponse::Scalar(resolve_location(loc))
            }
            GotoDefinitionResponse::Array(locs) => {
                GotoDefinitionResponse::Array(locs.into_iter().map(resolve_location).collect())
            }
            other => other,
        })
    }

    /// Resolve v-model to child defineModel (Tier 1), then classic prop+emit (Tier 2),
    /// then template-level definitions (Tier 3).
    fn resolve_vmodel_definition(
        &self,
        child: &ResolvedComponentDocument,
        model_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        let mut locations = Vec::new();

        // Tier 1: defineModel macro
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_analysis::AnalyzedMacroKind::DefineModel {
                continue;
            }
            let macro_model_name = mac.model_name.as_deref().unwrap_or("modelValue");
            if macro_model_name == model_name {
                if let Some(loc) = location_from_span(&child.uri, &child.line_index, mac.span) {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        // Tier 2: classic prop + emit pattern
        // The prop is the model_name itself (e.g., "modelValue" or "title")
        for mac in child.analysis.macros.iter() {
            if let Some(prop_field) = mac.prop_fields.iter().find(|f| f.name == model_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, prop_field.span)
                {
                    locations.push(loc);
                }
            }
            // The emit is `update:modelName`
            let emit_name = format!("update:{model_name}");
            if let Some(emit_field) = mac.emit_fields.iter().find(|f| f.name == emit_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, emit_field.span)
                {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        // Tier 3: template-level definitions
        if let Some(child_template) = child.analysis.template.as_ref() {
            if let Some(prop_def) = child_template
                .prop_definitions
                .iter()
                .find(|d| d.name == model_name)
            {
                if let Some(loc) = location_from_span(&child.uri, &child.line_index, prop_def.span)
                {
                    locations.push(loc);
                }
            }
        }

        if !locations.is_empty() {
            return Some(goto_response_from_locations(locations));
        }

        None
    }

    /// Resolve a slot name (#header) to the child's defineSlots field or template DefinedSlot.
    fn resolve_slot_name_definition(
        &self,
        child: &ResolvedComponentDocument,
        slot_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        // Check defineSlots macro first
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            if let Some(slot_field) = mac.slot_fields.iter().find(|f| f.name == slot_name) {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, slot_field.span)
                {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }

        // Fallback: template-level DefinedSlot
        if let Some(child_template) = child.analysis.template.as_ref() {
            if let Some(defined_slot) = child_template
                .defined_slots
                .iter()
                .find(|s| s.name == slot_name)
            {
                if let Some(loc) =
                    location_from_span(&child.uri, &child.line_index, defined_slot.span)
                {
                    return Some(GotoDefinitionResponse::Scalar(loc));
                }
            }
        }

        None
    }

    /// Resolve a slot-prop binding (e.g., "item" in `#default="{ item }"`) to
    /// the child's defineSlots binding span.
    fn resolve_slot_binding_definition(
        &self,
        child: &ResolvedComponentDocument,
        slot_name: &str,
        binding_name: &str,
    ) -> Option<GotoDefinitionResponse> {
        // Check defineSlots macro
        for mac in child.analysis.macros.iter() {
            if mac.kind != verter_analysis::AnalyzedMacroKind::DefineSlots {
                continue;
            }
            if let Some(slot_field) = mac.slot_fields.iter().find(|f| f.name == slot_name) {
                if let Some(binding) = slot_field.bindings.iter().find(|b| b.name == binding_name) {
                    if binding.span.start != 0 || binding.span.end != 0 {
                        if let Some(loc) =
                            location_from_span(&child.uri, &child.line_index, binding.span)
                        {
                            return Some(GotoDefinitionResponse::Scalar(loc));
                        }
                    }
                }
            }
        }

        None
    }

    fn external_ide_context(&self, ide_path: &str) -> Option<merge::ExternalIdeContext> {
        let (_tsx_path, tsx_content, mapper) = self.ide_context_by_path(ide_path)?;
        let tsx_line_index = LineIndex::new(&tsx_content, self.documents.encoding());
        // Get the Vue file's line index
        let snapshot = self.published_resolver()?;
        let canonical_id =
            source_id_from_provider_vue_path(&snapshot.resolver, self.documents.host(), ide_path)?;
        let uri = self.documents.canonical_id_to_uri(&canonical_id)?;
        let doc = self.documents.get(&uri)?;
        Some(merge::ExternalIdeContext {
            tsx_line_index,
            mapper,
            vue_line_index: doc.line_index.clone(),
        })
    }

    /// Pre-extracted data for type provider calls.
    /// All DashMap guards are dropped before this is returned, so it is safe
    /// to hold this across `.await` points without risking deadlock.
    fn type_provider_context(&self, uri: &Uri) -> Option<TypeProviderContext> {
        let (tsx_path, tsx_content, mapper) = self.ide_context(uri)?;
        let tsx_line_index = LineIndex::new(&tsx_content, self.documents.encoding());
        let vue_line_index = self.documents.get(uri)?.line_index.clone();
        // DashMap Ref dropped here at end of `?` chain
        Some(TypeProviderContext {
            tsx_path,
            tsx_content,
            mapper,
            tsx_line_index,
            vue_line_index,
        })
    }

    /// Find the Vue URI corresponding to an IDE path.
    fn vue_uri_from_ide_path(&self, ide_path: &str) -> Option<Uri> {
        let snapshot = self.published_resolver()?;
        let canonical_id =
            source_id_from_provider_vue_path(&snapshot.resolver, self.documents.host(), ide_path)?;
        self.documents.canonical_id_to_uri(&canonical_id)
    }

    /// Touch a canonical ID in the MRU list (push to front, dedup).
    fn touch_mru(&self, canonical_id: &str) {
        let mut mru = self.mru_canonical_ids.lock();
        mru.retain(|id| id != canonical_id);
        mru.insert(0, canonical_id.to_string());
        // Cap at a reasonable size
        mru.truncate(64);
    }

    fn queue_snapshot_provider_sync(&self, canonical_id: impl Into<String>) {
        self.pending_snapshot_provider_sync
            .insert(canonical_id.into());
    }

    fn provider_sync_state_for_source(&self, canonical_id: &str) -> Option<ProviderSyncState> {
        self.provider_sync_states
            .get(canonical_id)
            .map(|entry| entry.clone())
    }

    fn prepare_vue_provider_sync_transition(
        &self,
        canonical_id: &str,
        is_jsx: bool,
    ) -> Option<crate::provider_sync::ProviderSyncTransition> {
        let snapshot = self.published_resolver()?;
        let next_state = crate::provider_sync::vue_sync_state_for_source(
            &snapshot.resolver,
            canonical_id,
            is_jsx,
        )?;
        Some(prepare_sync_transition(
            &self.provider_sync_states,
            canonical_id,
            next_state,
        ))
    }

    fn prepare_non_vue_provider_sync_transition(
        &self,
        canonical_id: &str,
    ) -> Option<crate::provider_sync::ProviderSyncTransition> {
        let snapshot = self.published_resolver()?;
        let next_state =
            crate::provider_sync::non_vue_sync_state_for_source(&snapshot.resolver, canonical_id)?;
        Some(prepare_sync_transition(
            &self.provider_sync_states,
            canonical_id,
            next_state,
        ))
    }

    fn commit_provider_sync_state(&self, canonical_id: &str, state: ProviderSyncState) {
        commit_sync_transition(&self.provider_sync_states, canonical_id, state);
    }

    fn remove_provider_sync_state(&self, canonical_id: &str) -> Option<ProviderSyncState> {
        remove_sync_state(&self.provider_sync_states, canonical_id)
    }

    fn is_background_loaded_for_source_kind(
        &self,
        canonical_id: &str,
        kind: ProviderPathKind,
    ) -> bool {
        self.provider_sync_state_for_source(canonical_id)
            .map(|state| state.background_loaded_for_kind(kind))
            .unwrap_or(false)
    }

    async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)]) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        for (kind, path) in paths {
            let result = match kind {
                ProviderPathKind::Ide => sync.close_tsx(path).await,
                ProviderPathKind::Api => sync.close_dts(path).await,
                ProviderPathKind::Shadow => sync.close_file(path).await,
            };
            if let Err(error) = result {
                tracing::warn!("failed to close provider path {path}: {error}");
            }
        }
    }

    async fn close_provider_state(&self, state: &ProviderSyncState) {
        let paths = state.active_paths();
        self.close_provider_paths(&paths).await;
    }

    /// Resolve a child component's analysis from an import source path.
    ///
    /// Tries three strategies:
    /// 1. Relative imports → resolve against the parent's directory
    /// 2. Path alias resolution via tsconfig.json
    /// 3. Direct lookup (bare specifiers)
    fn resolve_component(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<verter_host::FileAnalysisSnapshot> {
        let canonical_id = uri_to_canonical_id(parent_uri);
        resolve_component_for(self.documents.host(), &canonical_id, import_source)
    }

    /// Resolve a child component with full context for cross-file editing.
    ///
    /// When `component_name` is provided and the import resolves to a non-`.vue`
    /// file (e.g. a barrel `index.ts`), follows re-export chains via
    /// `get_export_span_follow_reexports` to reach the terminal `.vue` file.
    fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
        component_name: Option<&str>,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Resolve the child's canonical ID
        let mut child_canonical_id = self
            .resolve_import_specifier(&canonical_id, import_source)
            .unwrap_or_else(|| {
                if import_source.starts_with('.') {
                    let parts: Vec<&str> = canonical_id.split('/').collect();
                    let dir = parts[..parts.len().saturating_sub(1)].join("/");
                    resolve_import_path(&dir, import_source)
                } else {
                    import_source.to_string()
                }
            });

        // Follow barrel re-export chains: if the resolved file is not a .vue file
        // and we know the component name, look up the re-export chain to find the
        // terminal .vue file (e.g. ./components/index.ts → ./components/Button.vue).
        if !child_canonical_id.ends_with(".vue") {
            if let Some(name) = component_name {
                // Ensure the barrel file is loaded so we can inspect its exports
                if self
                    .documents
                    .host()
                    .get_analysis(&child_canonical_id)
                    .is_none()
                {
                    self.documents.host().ensure_loaded(&child_canonical_id);
                }
                if let Some((resolved_id, _, _)) = self
                    .documents
                    .host()
                    .get_export_span_follow_reexports(&child_canonical_id, name)
                {
                    if resolved_id.ends_with(".vue") {
                        child_canonical_id = resolved_id;
                    }
                }
            }
        }

        if self
            .documents
            .host()
            .get_source(&child_canonical_id)
            .is_none()
            || self
                .documents
                .host()
                .get_analysis(&child_canonical_id)
                .is_none()
        {
            if !self.documents.host().ensure_loaded(&child_canonical_id) {
                return None;
            }
            let profile = self.documents.tsx_profile.read().clone();
            let _ = self
                .documents
                .host
                .ensure_compiled(&child_canonical_id, &profile);
        }

        let analysis = self
            .resolve_component(parent_uri, import_source)
            .or_else(|| self.documents.host().get_analysis(&child_canonical_id))?;

        // If the analysis came from the barrel file but we resolved to a .vue file,
        // prefer the .vue file's analysis for accurate prop/emit information.
        let analysis = if child_canonical_id.ends_with(".vue") {
            self.documents
                .host()
                .get_analysis(&child_canonical_id)
                .unwrap_or(analysis)
        } else {
            analysis
        };

        // Get the child's source
        let child_source_arc = self.documents.host().get_source(&child_canonical_id)?;
        let child_source = child_source_arc.to_string();
        let child_uri = crate::uri::path_to_file_uri(&child_canonical_id)?;
        let blocks = scan_sfc_blocks(&child_source);
        let line_index = LineIndex::new(&child_source, self.documents.encoding());

        Some(crate::features::cross_file::ChildComponentContext {
            canonical_id: child_canonical_id,
            uri: child_uri,
            source: child_source,
            analysis,
            blocks,
            line_index,
        })
    }

    fn child_hover_for_target(
        &self,
        parent_uri: &Uri,
        target: &hover::ChildHoverTarget,
    ) -> Option<Hover> {
        match target {
            hover::ChildHoverTarget::ComponentTag(target) => {
                let child = self.resolve_component_context(
                    parent_uri,
                    &target.import_source,
                    Some(&target.component_name),
                )?;
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&child.canonical_id)
                    .map(|api| api.code.to_string());
                Some(hover::build_child_component_hover(
                    &target.component_name,
                    &target.import_source,
                    &child.analysis,
                    public_api.as_deref(),
                    &target.usage_props,
                ))
            }
            hover::ChildHoverTarget::ImportBinding(target) => {
                let parent_analysis = self.documents.get_analysis(parent_uri)?;
                let child = self.resolve_component_document_for_import_binding(
                    parent_uri,
                    &parent_analysis,
                    &target.import_source,
                    &target.binding_name,
                )?;
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&crate::documents::uri_to_canonical_id(&child.uri))
                    .map(|api| api.code.to_string());
                Some(hover::build_child_component_hover(
                    &target.binding_name,
                    &target.import_source,
                    &child.analysis,
                    public_api.as_deref(),
                    &[],
                ))
            }
            hover::ChildHoverTarget::EventAttribute(target) => {
                let child =
                    self.resolve_component_context(parent_uri, &target.import_source, None)?;
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&child.canonical_id)
                    .map(|api| api.code.to_string());
                hover::build_child_event_hover(
                    &target.vue_attr,
                    &child.analysis,
                    public_api.as_deref(),
                )
            }
        }
    }

    /// Check if a URI is a virtual file and return its TSGO routing context.
    ///
    /// For virtual files (verter-virtual://), the content IS the TSX already.
    /// The cursor position is in TSX coordinates, so we can query TSGO directly
    /// without position mapping.
    ///
    /// Returns `Some((tsx_path, virtual_doc_line_index))` if this is a virtual file
    /// that should be routed through the source .vue file's TSX.
    fn virtual_file_context(&self, uri: &Uri) -> Option<(String, LineIndex)> {
        let source_uri_str = self.documents.get_virtual_source_uri(uri)?;
        let source_uri: Uri = source_uri_str.parse().ok()?;

        // Get the TSX path from the source .vue file
        let tsx_path = self.ide_path_for_uri(&source_uri)?;

        // Build LineIndex from the virtual file's content (for offset conversion)
        let doc = self.documents.get(uri)?;
        let line_index = doc.line_index.clone();

        Some((tsx_path, line_index))
    }

    // ── Custom protocol handlers ──────────────────────────────────────

    /// Handle `$/onDidChangeTsOrJsFile` notification.
    ///
    /// Called when the client edits a `.ts`, `.js`, or `.vue` file.
    /// Invalidates host caches and re-syncs to the TypeProvider.
    pub async fn on_did_change_ts_or_js_file(&self, params: OnDidChangeTsOrJsFileParams) {
        tracing::info!("onDidChangeTsOrJsFile ENTER {}", params.uri);

        // Skip .vue files — they are synced to the type provider via TSX compilation
        // in sync_ide_to_provider(). Sending raw Vue SFC source to TSGO (which
        // expects TypeScript) corrupts its internal state.
        if params.uri.ends_with(".vue") {
            return;
        }

        // For non-Vue files tracked by the extension (TS/JS), keep the host and
        // provider in sync. Exact `.vue` imports are rewritten to `.vue.ts`
        // before syncing so the provider resolves through Verter-managed files.
        if let Some(last) = params.changes.last() {
            // Convert file:// URI to filesystem path — update_file() calls
            // path_to_uri() internally, so passing a URI would double-wrap it
            // (e.g., file:///file:///...).
            let path = if let Ok(uri) = params.uri.parse::<Uri>() {
                uri_to_canonical_id(&uri)
            } else {
                params.uri.clone()
            };

            let module_references = self
                .documents
                .host
                .upsert(verter_host::UpsertRequest {
                    canonical_id: Some(path.clone()),
                    input_id: path.clone(),
                    source: Arc::from(last.text.as_str()),
                    file_kind: verter_host::FileKind::NonSfc,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default();

            if let Some(snapshot) = self.published_resolver() {
                self.sync_non_vue_file_to_provider(
                    &snapshot,
                    &path,
                    Arc::from(last.text.as_str()),
                    &module_references,
                )
                .await;
            } else {
                self.queue_snapshot_provider_sync(path);
            }
        }
    }

    /// Handle `$/onFileChanged` notification.
    ///
    /// Called when `node_modules` files are created, updated, or deleted.
    pub async fn on_file_changed(&self, params: OnFileChangedParams) {
        tracing::debug!("$/onFileChanged: {} ({})", params.uri, params.change_type);

        let canonical_id = if let Ok(uri) = params.uri.parse::<Uri>() {
            uri_to_canonical_id(&uri)
        } else {
            crate::documents::uri_to_canonical_id_from_str(&params.uri)
        };

        // Skip watcher events for Verter-generated @verter/types stubs.
        // Real installed @verter/types packages (no marker) pass through normally.
        if is_generated_verter_types_event(&canonical_id) {
            return;
        }

        // Handle .vue file changes from the file watcher.
        // These are files not open in the editor — re-sync to type provider.
        if params.uri.ends_with(".vue") {
            match params.change_type.as_str() {
                "create" | "update" => {
                    self.resync_background_vue_file(&canonical_id).await;
                }
                "delete" => {
                    // Close TSX/DTS in the type provider and clean up.
                    if let Some(state) =
                        self.remove_provider_sync_state(&canonical_id).or_else(|| {
                            let profile = self.documents.tsx_profile.read().clone();
                            self.documents
                                .host
                                .get_ide(&canonical_id, &profile)
                                .and_then(|ide| {
                                    self.prepare_vue_provider_sync_transition(
                                        &canonical_id,
                                        ide.is_jsx,
                                    )
                                    .map(|transition| transition.next)
                                })
                        })
                    {
                        self.close_provider_state(&state).await;
                    }
                    self.documents.host.remove(&canonical_id);
                }
                _ => {}
            }
        }

        // Check if the changed file is a known vite config or its dependency.
        // If so, trigger a full registry rebuild to re-analyze aliases.
        let is_vite_dep = {
            let ws = self.vfs_workspace.read();
            ws.as_ref()
                .and_then(|ws| ws.load_published())
                .and_then(|published| {
                    published
                        .ext::<crate::workspace_state::LspViews>()
                        .map(|views| {
                            views
                                .project_views
                                .iter()
                                .any(|v| v.vite_config_deps.iter().any(|dep| dep == &canonical_id))
                        })
                })
                .unwrap_or(false)
        };

        if is_vite_dep {
            tracing::debug!(
                "vite config dependency changed: {} — triggering registry rebuild",
                canonical_id
            );
            self.trigger_registry_rebuild().await;
        }
    }

    /// Build `BackgroundInitArgs` from the current server state and spawn
    /// `background_init` as a fire-and-forget tokio task.
    ///
    /// Used by `initialized()`, `trigger_registry_rebuild()`, and
    /// `did_change_workspace_folders()` — the three sites that need a full
    /// project-registry rebuild.
    async fn spawn_background_init(
        &self,
        init_lint_opts: Option<serde_json::Value>,
        context: &str,
    ) {
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }
        let my_gen = self
            .init_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;

        let mut vite_opts = self.vite_config_options.lock().await.clone();
        vite_opts.node_path = crate::tsserver::find_node();
        let args = BackgroundInitArgs {
            roots,
            vite_opts,
            init_lint_opts,
            my_gen,
            client: self.client.clone(),
            type_provider: self.type_provider.clone(),
            workspace_scanner: Arc::clone(&self.workspace_scanner),
            init_generation: Arc::clone(&self.init_generation),
            project_sync: self.project_sync.clone(),
            documents: Arc::clone(&self.documents),
            provider_sync_states: Arc::clone(&self.provider_sync_states),
            pending_snapshot_provider_sync: Arc::clone(&self.pending_snapshot_provider_sync),
            is_tsgo: matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo),
            cached_verter_diags: Arc::clone(&self.cached_verter_diags),
            position_encoding: Arc::clone(&self.position_encoding),
            mru_canonical_ids: {
                // Snapshot the MRU list at spawn time — background_init uses it for drain ordering
                Arc::new(parking_lot::Mutex::new(
                    self.mru_canonical_ids.lock().clone(),
                ))
            },
            vfs_workspace: Arc::clone(&self.vfs_workspace),
        };

        let ctx = context.to_owned();
        tokio::spawn(async move {
            if let Err(e) = background_init(args).await {
                tracing::error!("background {ctx} failed: {e}");
            }
        });
    }

    /// Trigger a full registry rebuild (same as did_change_workspace_folders).
    /// Used when vite config files change on disk.
    async fn trigger_registry_rebuild(&self) {
        self.spawn_background_init(None, "vite config rebuild")
            .await;
    }

    /// Re-read a non-open .vue file from disk, upsert, compile, and sync it to the provider.
    /// Lightweight imported-Vue sync for `did_open`.
    ///
    /// Tries to generate and sync the required Vue artifacts without disk I/O:
    /// if the host already has the file in memory, `get_public_api` avoids
    /// re-reading from disk. Falls back to `resync_background_vue_file` when
    /// the file hasn't been upserted yet.
    async fn sync_imported_vue_api_lightweight(&self, canonical_id: &str) {
        let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
        let profile = self.documents.tsx_profile.read().clone();

        // Fast path: host already has the file — sync directly from cached artifacts.
        if let Some(api) = self.documents.host.get_public_api(canonical_id) {
            let ide = if is_tsgo {
                self.documents.host.get_ide(canonical_id, &profile)
            } else {
                None
            };

            if self.published_resolver().is_none() {
                if let Some(ide) = ide.as_ref() {
                    let _ = self
                        .sync_vue_ide_provisionally(canonical_id, &ide.code, ide.is_jsx)
                        .await;
                }
                let _ = self
                    .sync_vue_api_provisionally(canonical_id, &api.code)
                    .await;
                return;
            }

            if let Some(sync) = &self.project_sync {
                let Some(transition) = self.prepare_vue_provider_sync_transition(
                    canonical_id,
                    ide.as_ref().map(|output| output.is_jsx).unwrap_or(false),
                ) else {
                    let _ = self
                        .sync_vue_api_provisionally(canonical_id, &api.code)
                        .await;
                    return;
                };
                self.close_provider_paths(&transition.stale_paths).await;
                let mut committed_state = transition.next;
                let mut synced_any = false;

                if let Some(ide) = ide.as_ref() {
                    if let Some(ide_path) = committed_state.ide_path.clone() {
                        let result = if committed_state.ide_background_loaded {
                            sync.sync_tsx(&ide_path, &ide.code).await
                        } else {
                            sync.open_tsx(&ide_path, &ide.code).await
                        };
                        if result.is_ok() {
                            committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                            synced_any = true;
                        } else if let Err(error) = result {
                            tracing::warn!(
                                "sync_imported_vue_api_lightweight: failed for {ide_path}: {error}"
                            );
                            self.queue_snapshot_provider_sync(canonical_id.to_string());
                        }
                    }
                }

                if let Some(dts_path) = committed_state.api_path.clone() {
                    let result = if committed_state.api_background_loaded {
                        sync.sync_dts(&dts_path, &api.code).await
                    } else {
                        sync.open_dts(&dts_path, &api.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                        synced_any = true;
                    } else if let Err(e) = result {
                        tracing::warn!(
                            "sync_imported_vue_api_lightweight: failed for {dts_path}: {e}"
                        );
                        self.queue_snapshot_provider_sync(canonical_id.to_string());
                    }
                }

                if synced_any {
                    self.commit_provider_sync_state(canonical_id, committed_state);
                }
            }
            return;
        }

        if self.published_resolver().is_none() {
            let compiled = block_in_place_if_available(|| {
                self.documents.host.remove(canonical_id);
                if !self.documents.host.ensure_loaded(canonical_id) {
                    return false;
                }

                self.documents.host().ensure_loaded(canonical_id);

                let profile = self.documents.tsx_profile.read().clone();
                self.documents
                    .host
                    .ensure_compiled(canonical_id, &profile)
                    .is_ok()
            });

            if compiled {
                if is_tsgo {
                    if let Some(ide) = self.documents.host.get_ide(canonical_id, &profile) {
                        let _ = self
                            .sync_vue_ide_provisionally(canonical_id, &ide.code, ide.is_jsx)
                            .await;
                    }
                }
                if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                    let _ = self
                        .sync_vue_api_provisionally(canonical_id, &api.code)
                        .await;
                    return;
                }
            }

            self.queue_snapshot_provider_sync(canonical_id.to_string());
            return;
        }

        // Slow path: file not in host yet — full disk read + upsert + compile + sync.
        self.resync_background_vue_file(canonical_id).await;
    }

    async fn resync_background_vue_file(&self, canonical_id: &str) {
        tracing::info!(
            "resync_background: START {canonical_id} thread={:?}",
            std::thread::current().id()
        );
        // Load from disk + upsert + compile (all blocking) — wrapped in block_in_place
        // to prevent tokio worker thread exhaustion during background sync.
        let compile_result = block_in_place_if_available(|| {
            self.documents.host.remove(canonical_id);
            if !self.documents.host.ensure_loaded(canonical_id) {
                tracing::debug!("resync_background: can't read {canonical_id}");
                return None;
            }

            self.documents.host().ensure_loaded(canonical_id);

            // Compile
            let profile = self.documents.tsx_profile.read().clone();
            if self
                .documents
                .host
                .ensure_compiled(canonical_id, &profile)
                .is_err()
            {
                return None;
            }
            Some(profile)
        });
        tracing::info!("resync_background: COMPILED {canonical_id}");

        let Some(profile) = compile_result else {
            return;
        };

        self.refresh_vue_dependency_tracking(canonical_id);

        // Sync to type provider
        if let Some(sync) = &self.project_sync {
            let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
            let Some(ide) = self.documents.host.get_ide(canonical_id, &profile) else {
                return;
            };
            if is_tsgo {
                if let Some(snapshot) = self.published_resolver() {
                    configure_provider_paths_for_source(sync, &snapshot, canonical_id, true).await;
                }
            }
            let Some(transition) =
                self.prepare_vue_provider_sync_transition(canonical_id, ide.is_jsx)
            else {
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                return;
            };
            self.close_provider_paths(&transition.stale_paths).await;
            let mut committed_state = transition.next;

            if let Some(tsx_path) = committed_state.ide_path.clone() {
                let is_bg =
                    self.is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Ide);
                let result = if is_bg {
                    sync.sync_tsx(&tsx_path, &ide.code).await
                } else {
                    sync.open_tsx(&tsx_path, &ide.code).await
                };
                if result.is_ok() {
                    committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                } else if let Err(e) = result {
                    tracing::warn!("resync_background: failed to sync {canonical_id}: {e}");
                }
            }

            // Sync .vue.ts as secondary provider support output.
            if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                let Some(dts_path) = committed_state.api_path.clone() else {
                    return;
                };
                let is_bg =
                    self.is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Api);
                let result = if is_tsgo {
                    // TSGO: open/update DTS so it's in TSGO's virtual FS alongside the IDE file.
                    if is_bg {
                        sync.sync_dts(&dts_path, &api.code).await
                    } else {
                        sync.open_dts(&dts_path, &api.code).await
                    }
                } else if is_bg {
                    sync.sync_dts(&dts_path, &api.code).await
                } else {
                    sync.load_dts(&dts_path, &api.code).await
                };
                if result.is_ok() {
                    committed_state.set_background_loaded(ProviderPathKind::Api, true);
                    self.commit_provider_sync_state(canonical_id, committed_state);
                }
            }
        }
    }

    /// Handle `$/getCompiledCode` request.
    ///
    /// Returns the compiled TSX output for a Vue file URI.
    pub async fn get_compiled_code(
        &self,
        params: GetCompiledCodeParams,
    ) -> Result<Option<CompiledCodeResponse>> {
        let uri = params.uri;
        tracing::debug!("$/getCompiledCode: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        if let Some(canonical_id) = self.documents.get_canonical_id(&parsed_uri) {
            self.documents.host().ensure_loaded(&canonical_id);
        }
        let tsx = self.documents.get_ide(&parsed_uri);

        Ok(tsx.map(|tsx| CompiledCodeResponse {
            js: CompiledBlock {
                code: tsx.code.to_string(),
                map: tsx.source_map.map(|m| m.to_string()),
            },
            css: CompiledBlock {
                code: String::new(),
                map: None,
            },
            wasm: CompiledBlock {
                code: String::new(),
                map: None,
            },
        }))
    }

    /// Handle `$/verter/documentDropEdit` request.
    ///
    /// When a `.vue` file is dropped into a template, inserts a component tag
    /// and an import statement.
    pub async fn document_drop_edit(
        &self,
        params: DocumentDropEditParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document.uri;
        tracing::debug!(
            "$/verter/documentDropEdit: {} -> {}",
            params.dropped_uri,
            uri.as_str()
        );

        let doc = match self.documents.get(uri) {
            Some(d) => d,
            None => return Ok(None),
        };

        let blocks = scan_sfc_blocks(&doc.source);
        // Compute preferred import path (alias-based if available)
        let canonical_target = crate::documents::uri_to_canonical_id(uri);
        let canonical_dropped = crate::documents::uri_to_canonical_id_from_str(&params.dropped_uri);
        let preferred_import_path = self
            .documents
            .host()
            .preferred_specifier(&canonical_target, &canonical_dropped);

        let edit = crate::features::document_drop_edit::document_drop_edit(
            &params.dropped_uri,
            &params.position,
            &doc.source,
            &blocks,
            &doc.line_index,
            uri,
            preferred_import_path.as_deref(),
        );

        Ok(edit)
    }

    /// Handle `$/verter/getVirtualFiles` request.
    ///
    /// Returns all virtual files for a Vue document URI.
    pub async fn get_virtual_files(
        &self,
        params: GetVirtualFilesParams,
    ) -> Result<Option<VirtualFilesResponse>> {
        let uri = params.uri;
        tracing::info!("getVirtualFiles ENTER {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        if let Some(canonical_id) = self.documents.get_canonical_id(&parsed_uri) {
            self.documents.host().ensure_loaded(&canonical_id);
        }
        let response = self.documents.get_virtual_files(&parsed_uri);
        tracing::info!("getVirtualFiles EXIT {uri}");
        Ok(response)
    }

    /// Handle `$/verter/applyStyleOverrides` request.
    ///
    /// Applies preprocessor-compiled CSS overrides to style blocks, updating the host's
    /// analysis cache. Used by the VS Code extension after transpiling Sass/Stylus.
    pub async fn apply_style_overrides(
        &self,
        params: ApplyStyleOverridesParams,
    ) -> Result<ApplyStyleOverridesResponse> {
        let uri = &params.uri;
        tracing::debug!("$/verter/applyStyleOverrides: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(ApplyStyleOverridesResponse { success: false }),
        };

        let canonical_id = uri_to_canonical_id(&parsed_uri);
        let overrides = params
            .overrides
            .into_iter()
            .map(|o| verter_host::StyleOverrideEntry {
                index: o.index as usize,
                code: Arc::from(o.code),
                source_map: o.source_map.map(Arc::from),
            })
            .collect();

        let result = self
            .documents
            .apply_style_overrides(&canonical_id, overrides);

        if result {
            // Re-publish diagnostics since analysis has changed
            self.publish_full_diagnostics(&parsed_uri).await;
        }

        Ok(ApplyStyleOverridesResponse { success: result })
    }

    /// Handle `$/verter/getAnalysis` request.
    ///
    /// Returns the full analysis snapshot as JSON for a Vue document URI.
    pub async fn get_analysis(
        &self,
        params: GetAnalysisParams,
    ) -> Result<Option<serde_json::Value>> {
        let uri = params.uri;
        tracing::debug!("$/verter/getAnalysis: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };

        Ok(self.documents.get_analysis_json(&parsed_uri))
    }

    /// Handle `$/verter/getStatistics` request.
    ///
    /// Returns basic statistics about the LSP session.
    pub async fn get_statistics(
        &self,
        _params: Option<StatisticsRequestParams>,
    ) -> Result<StatisticsSnapshot> {
        tracing::debug!("$/verter/getStatistics");

        let mut by_type = serde_json::Map::new();
        let mut by_file = serde_json::Map::new();

        // Collect LSP handler statistics
        for (event_type, summary) in self.statistics.summary_by_type() {
            by_type.insert(
                event_type,
                serde_json::json!({
                    "count": summary.count,
                    "totalMs": summary.total_ms,
                    "minMs": summary.min_ms,
                    "maxMs": summary.max_ms,
                    "averageMs": summary.average_ms(),
                }),
            );
        }
        for (file, summary) in self.statistics.summary_by_file() {
            by_file.insert(
                file,
                serde_json::json!({
                    "count": summary.count,
                    "totalMs": summary.total_ms,
                    "minMs": summary.min_ms,
                    "maxMs": summary.max_ms,
                    "averageMs": summary.average_ms(),
                }),
            );
        }

        // Merge host metrics (compile/upsert counters)
        let host_metrics = self.documents.host.metrics_snapshot();
        by_type.insert(
            "host:upsert".into(),
            serde_json::json!({
                "count": host_metrics.upserts,
                "totalMs": host_metrics.slice_hash_time_us_total as f64 / 1000.0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": host_metrics.avg_slice_hash_time_us / 1000.0,
            }),
        );
        by_type.insert(
            "host:compile".into(),
            serde_json::json!({
                "count": host_metrics.compile_requests,
                "totalMs": host_metrics.compile_time_us_total as f64 / 1000.0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": if host_metrics.compile_requests > 0 {
                    (host_metrics.compile_time_us_total as f64 / host_metrics.compile_requests as f64) / 1000.0
                } else {
                    0.0
                },
            }),
        );
        by_type.insert(
            "host:cache_hits".into(),
            serde_json::json!({
                "count": host_metrics.compile_cache_hits,
                "totalMs": 0,
                "minMs": 0,
                "maxMs": 0,
                "averageMs": 0,
            }),
        );

        Ok(StatisticsSnapshot {
            enabled: self.statistics.is_enabled(),
            session: StatisticsSession { by_type, by_file },
        })
    }

    /// Handle `$/verter/getProjectOverview` request.
    ///
    /// Returns a global project overview: all known files, component usage graph,
    /// and aggregate statistics.
    pub async fn get_project_overview(
        &self,
        _params: serde_json::Value,
    ) -> Result<ProjectOverviewResponse> {
        tracing::debug!("$/verter/getProjectOverview");

        let file_list = self.documents.host.list_files();

        let mut files = Vec::new();
        let mut component_graph = Vec::new();
        let mut total_vue_files = 0usize;
        let mut total_components = 0usize;
        let mut files_with_scoped_styles = 0usize;

        for (canonical_id, file_kind) in &file_list {
            let kind = match file_kind {
                verter_host::FileKind::VueSfc => "vue",
                verter_host::FileKind::NonSfc => {
                    if canonical_id.ends_with(".ts") || canonical_id.ends_with(".tsx") {
                        "ts"
                    } else {
                        "js"
                    }
                }
            };

            files.push(ProjectOverviewFile {
                path: canonical_id.clone(),
                kind,
            });

            if *file_kind == verter_host::FileKind::VueSfc {
                total_vue_files += 1;

                // Get analysis for component graph
                if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                    // Component usage
                    if let Some(template) = &analysis.template {
                        let used: Vec<String> =
                            template.components.iter().map(|c| c.name.clone()).collect();
                        total_components += used.len();
                        if !used.is_empty() {
                            component_graph.push(ProjectOverviewComponentEdge {
                                file: canonical_id.clone(),
                                uses_components: used,
                            });
                        }
                    }

                    // Scoped styles check
                    if analysis.styles.iter().any(|s| s.scoped) {
                        files_with_scoped_styles += 1;
                    }
                }
            }
        }

        Ok(ProjectOverviewResponse {
            files,
            component_graph,
            stats: ProjectOverviewStats {
                total_vue_files,
                total_components,
                total_provide_keys: 0,
                total_inject_keys: 0,
                files_with_scoped_styles,
            },
        })
    }

    /// Handle `$/verter/getRouteTree` request.
    ///
    /// Returns a complete route analysis snapshot for the first workspace root.
    pub async fn get_route_tree(&self, _params: serde_json::Value) -> Result<serde_json::Value> {
        tracing::debug!("$/verter/getRouteTree");

        let roots = self.workspace_roots.lock().await.clone();
        let Some(root) = roots.first() else {
            return Ok(serde_json::to_value(
                verter_analysis::routes::RouteAnalysisSnapshot::default(),
            )
            .unwrap_or_default());
        };

        // Collect template components from all Vue SFC analyses
        let file_list = self.documents.host.list_files();
        let mut template_components = Vec::new();
        for (canonical_id, file_kind) in &file_list {
            if *file_kind == verter_host::FileKind::VueSfc {
                if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                    if let Some(template) = &analysis.template {
                        template_components
                            .push((canonical_id.clone(), template.components.clone()));
                    }
                }
            }
        }

        let project_root = std::path::Path::new(root);
        let snapshot =
            verter_analysis::routes::build_route_analysis(project_root, &template_components);

        Ok(serde_json::to_value(snapshot).unwrap_or_default())
    }

    /// Handle `$/verter/getBindingTypes` request.
    ///
    /// For each binding in the file's analysis, queries TSGO for its TypeScript type.
    /// Returns a map of binding name → type string (or null if unavailable).
    pub async fn get_binding_types(&self, params: GetAnalysisParams) -> Result<serde_json::Value> {
        let uri = params.uri;
        tracing::debug!("$/verter/getBindingTypes: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => return Ok(serde_json::Value::Object(serde_json::Map::new())),
        };

        let mut result = serde_json::Map::new();

        // Get analysis for the file's bindings
        let analysis = self.documents.get_analysis(&parsed_uri);
        let Some(analysis) = analysis else {
            return Ok(serde_json::Value::Object(result));
        };

        // Need type provider and TSX context for type queries
        let Some(tp) = &self.type_provider else {
            return Ok(serde_json::Value::Object(result));
        };
        let Some((tsx_path, tsx_content, mapper)) = self.ide_context(&parsed_uri) else {
            return Ok(serde_json::Value::Object(result));
        };

        let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
        let Some(doc) = self.documents.get(&parsed_uri) else {
            return Ok(serde_json::Value::Object(result));
        };

        for binding in &analysis.bindings {
            // Convert Vue byte offset → Vue Position → TSX offset
            let vue_pos = doc.line_index.offset_to_position(binding.span.start);
            let Some(vue_pos) = vue_pos else { continue };

            let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                &vue_pos,
                &doc.line_index,
                &mapper,
                &tsx_li,
            );
            let Some(tsx_offset) = tsx_offset else {
                continue;
            };

            // Query TSGO for the type at this position
            if let Ok(Some(hover)) = tp.get_hover(&tsx_path, tsx_offset).await {
                // Extract the type from the hover contents
                // Typical format: "```typescript\nconst x: number\n```" or "(property) x: string"
                let type_str = extract_type_from_hover(&hover.contents, &binding.name);
                result.insert(
                    binding.name.clone(),
                    type_str
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                );
            } else {
                result.insert(binding.name.clone(), serde_json::Value::Null);
            }
        }

        Ok(serde_json::Value::Object(result))
    }

    /// Handle `$/verter/getComponentParents` request.
    ///
    /// Returns all files that use the component defined in the given URI,
    /// along with the props and slots they pass to it.
    pub async fn get_component_parents(
        &self,
        params: GetComponentParentsParams,
    ) -> Result<ComponentParentsResponse> {
        let uri = params.uri;
        tracing::debug!("$/verter/getComponentParents: {uri}");

        let parsed_uri: Uri = match uri.parse() {
            Ok(u) => u,
            Err(_) => {
                return Ok(ComponentParentsResponse {
                    component_path: uri,
                    parents: Vec::new(),
                });
            }
        };

        let target_canonical = self
            .documents
            .get_canonical_id(&parsed_uri)
            .unwrap_or_else(|| uri_to_canonical_id(&parsed_uri));

        // Normalize the target path for comparison
        let target_normalized = target_canonical.replace('\\', "/");

        let file_list = self.documents.host.list_files();
        let mut parents = Vec::new();
        let vue_count = file_list
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .count();
        tracing::info!(
            "getComponentParents: target='{}' scanning {} vue files",
            target_normalized,
            vue_count
        );

        for (canonical_id, file_kind) in &file_list {
            if *file_kind != verter_host::FileKind::VueSfc {
                continue;
            }
            // Skip the target file itself
            let normalized_id = canonical_id.replace('\\', "/");
            if normalized_id == target_normalized {
                continue;
            }

            if let Some(analysis) = self.documents.host.get_analysis(canonical_id) {
                if let Some(template) = &analysis.template {
                    for comp in &template.components {
                        if let Some(src) = &comp.import_source {
                            // Resolve the import source to an absolute path via VFS
                            let resolved = self
                                .documents
                                .host()
                                .resolve_import_via_workspace(&normalized_id, src)
                                .unwrap_or_else(|| {
                                    if src.starts_with('.') {
                                        let importer_dir = normalized_id
                                            .rfind('/')
                                            .map(|i| &normalized_id[..i])
                                            .unwrap_or("");
                                        resolve_import_path(importer_dir, src)
                                    } else {
                                        src.to_string()
                                    }
                                });
                            tracing::info!(
                                "  [{}] component '{}' import='{}' → resolved='{}'",
                                normalized_id.rsplit('/').next().unwrap_or("?"),
                                comp.name,
                                src,
                                resolved
                            );
                            let resolved_normalized = resolved.replace('\\', "/");
                            let matches = import_resolved_matches_target(
                                &resolved_normalized,
                                &target_normalized,
                            );
                            if matches {
                                tracing::info!(
                                    "  MATCH! resolved='{}' == target='{}'",
                                    resolved_normalized,
                                    target_normalized
                                );
                                let props_json = comp
                                    .props
                                    .iter()
                                    .filter_map(|p| serde_json::to_value(p).ok())
                                    .collect();
                                parents.push(ComponentParentInfo {
                                    file_path: canonical_id.clone(),
                                    component_name: comp.name.clone(),
                                    props: props_json,
                                    slots_used: comp.slots_used.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(ComponentParentsResponse {
            component_path: target_canonical,
            parents,
        })
    }
}

// Test-only accessors for the cross-module test harness (`test_harness.rs`).
#[cfg(test)]
impl VerterLanguageServer {
    /// Access the document registry (test harness access).
    pub(crate) fn test_documents(&self) -> &std::sync::Arc<crate::documents::DocumentRegistry> {
        &self.documents
    }

    /// Trigger interactive file sync to the type provider (test harness access).
    pub(crate) async fn test_ensure_synced(&self, uri: &tower_lsp_server::ls_types::Uri) {
        self.ensure_current_file_synced(uri).await;
    }

    /// Access the VFS workspace (test harness access).
    pub(crate) fn test_vfs_workspace(&self) -> Option<Arc<verter_vfs::FilesystemWorkspace>> {
        self.vfs_workspace.read().clone()
    }

    /// Install a VFS workspace (test harness access).
    pub(crate) fn install_vfs_workspace(&self, workspace: Arc<verter_vfs::FilesystemWorkspace>) {
        *self.vfs_workspace.write() = Some(workspace);
    }
}

impl LanguageServer for VerterLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("verter-lsp initializing");
        tracing::info!(
            "type provider: {} ({})",
            if self.type_provider.is_some() {
                "connected"
            } else {
                "NONE — no TypeScript intellisense"
            },
            self.type_provider_kind,
        );

        // ── Position encoding negotiation (LSP 3.17) ────────────────────
        // Prefer UTF-8 (native Rust encoding — no conversion needed),
        // then UTF-32, then UTF-16. Default to UTF-16 per LSP spec.
        let encoding = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_ref())
            .and_then(|encodings| {
                if encodings.contains(&PositionEncodingKind::UTF8) {
                    Some(PositionEncodingKind::UTF8)
                } else if encodings.contains(&PositionEncodingKind::UTF32) {
                    Some(PositionEncodingKind::UTF32)
                } else if encodings.contains(&PositionEncodingKind::UTF16) {
                    Some(PositionEncodingKind::UTF16)
                } else {
                    None
                }
            })
            .unwrap_or(PositionEncodingKind::UTF16);
        tracing::info!("negotiated position encoding: {}", encoding.as_str());
        *self.position_encoding.write() = encoding.clone();
        self.documents.set_encoding(encoding.clone());

        // Extract and store all workspace roots
        if let Some(folders) = &params.workspace_folders {
            let mut roots = Vec::new();
            for folder in folders {
                tracing::info!("workspace folder: {}", folder.uri.as_str());
                roots.push(folder.uri.as_str().to_string());
            }
            *self.workspace_roots.lock().await = roots.clone();

            // Create VFS workspace early so relative imports resolve before
            // background_init completes. The project graph starts empty —
            // Step 1's ownership fix makes relative/node_modules/hash imports
            // work without project ownership. background_init later calls
            // set_project_graph() to populate alias resolution.
            let canonical_roots: Vec<String> = roots
                .iter()
                .map(|r| crate::documents::uri_to_canonical_id_from_str(r))
                .collect();
            let ws = std::sync::Arc::new(verter_vfs::FilesystemWorkspace::new(
                verter_vfs::FilesystemOptions {
                    roots: canonical_roots,
                    eager_preload: false,
                },
            ));
            ws.set_project_graph(verter_vfs::ProjectGraph::new());
            let ws_dyn: std::sync::Arc<dyn verter_vfs::WorkspaceAccess> = ws.clone();
            self.documents.host().set_workspace(ws_dyn);
            *self.vfs_workspace.write() = Some(ws);
            tracing::info!(
                "VFS workspace created early in initialize() with {} roots",
                roots.len()
            );
        }

        // Parse initialization options (statistics config, lint config, etc.)
        if let Some(opts) = &params.initialization_options {
            tracing::debug!("initialization options: {opts}");
            if let Some(stats_enabled) = opts
                .get("statistics")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool())
            {
                self.statistics.set_enabled(stats_enabled);
                tracing::info!(
                    "statistics: {}",
                    if stats_enabled { "enabled" } else { "disabled" }
                );
            }
            // Store lint options for use in initialized()
            if opts.get("lint").is_some() {
                *self.init_lint_options.lock().await = Some(opts.clone());
            }
            // Read viteConfig settings
            {
                let mut vite_opts = self.vite_config_options.lock().await;
                if let Some(vite_config) = opts.get("viteConfig") {
                    if let Some(enabled) = vite_config.get("enabled").and_then(|v| v.as_bool()) {
                        vite_opts.enabled = enabled;
                    }
                    if let Some(trusted) =
                        vite_config.get("trustedFiles").and_then(|v| v.as_array())
                    {
                        vite_opts.trusted_files = trusted
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.replace('\\', "/")))
                            .collect();
                    }
                }
                tracing::info!(
                    "vite config: enabled={}, trusted_files={}",
                    vite_opts.enabled,
                    vite_opts.trusted_files.len()
                );
            }
            // Read inlayHints.enabled setting (default: true)
            if let Some(enabled) = opts
                .get("inlayHints")
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool())
            {
                self.inlay_hints_enabled
                    .store(enabled, std::sync::atomic::Ordering::Relaxed);
                tracing::info!(
                    "type provider inlay hints: {}",
                    if enabled { "enabled" } else { "disabled" }
                );
            }
            // Read experimental.conditionalRootNarrowing setting (default: false)
            if let Some(enabled) = opts
                .get("experimental")
                .and_then(|v| v.get("conditionalRootNarrowing"))
                .and_then(|v| v.as_bool())
            {
                self.documents
                    .tsx_profile
                    .write()
                    .conditional_root_narrowing = enabled;
                tracing::info!(
                    "conditional root narrowing: {}",
                    if enabled { "enabled" } else { "disabled" }
                );
            }
            // Read experimental.strictSlots setting (default: false)
            if let Some(enabled) = opts
                .get("experimental")
                .and_then(|v| v.get("strictSlots"))
                .and_then(|v| v.as_bool())
            {
                self.documents.tsx_profile.write().strict_slots = enabled;
                tracing::info!(
                    "strict slots: {}",
                    if enabled { "enabled" } else { "disabled" }
                );
            }
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(&encoding),
            server_info: Some(ServerInfo {
                name: "verter-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            offset_encoding: Some(encoding.as_str().to_owned()),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("verter-lsp initialized");

        // A. Spawn heartbeat FIRST — ensures the extension sees heartbeats
        // even while background initialization is running.
        spawn_heartbeat(self.client.clone());

        // B. Send immediate non-blocking notifications
        let tp_label = self.type_provider_kind.to_string();
        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "verter-lsp {} initialized (type provider: {tp_label})",
                    env!("CARGO_PKG_VERSION")
                ),
            )
            .await;

        // Notify the extension of the type provider child PID for orphan cleanup.
        if let Some(tp) = &self.type_provider {
            if let Some(pid) = tp.child_pid() {
                let kind = self.type_provider_kind.to_string().to_lowercase();
                self.client
                    .send_notification::<TypeProviderStarted>(TypeProviderStartedParams {
                        pid,
                        kind: kind.clone(),
                    })
                    .await;
                // Legacy TsgoStarted notification — only send when TSGO is actually active
                if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                    self.client
                        .send_notification::<TsgoStarted>(TsgoStartedParams { pid })
                        .await;
                }
            }
        }

        // Send type provider status notification — tells the extension which
        // provider is active (or why none could be started) for the status bar.
        {
            let kind = self.type_provider_kind.to_string().to_lowercase();
            let reason = if matches!(self.type_provider_kind, crate::TypeProviderKind::None) {
                self.type_provider_none_reason.clone()
            } else {
                None
            };
            self.client
                .send_notification::<TypeProviderStatus>(TypeProviderStatusParams {
                    kind,
                    reason: reason.clone(),
                })
                .await;
            // When no type provider is available, also show a warning message
            if matches!(self.type_provider_kind, crate::TypeProviderKind::None) {
                let msg = if let Some(ref r) = reason {
                    format!(
                        "Verter: No TypeScript type provider available ({r}). \
                         Hover, completions, and go-to-definition will be limited to \
                         Verter's built-in analysis."
                    )
                } else {
                    "Verter: No TypeScript type provider available. \
                     Hover, completions, and go-to-definition will be limited to \
                     Verter's built-in analysis."
                        .into()
                };
                self.client.show_message(MessageType::WARNING, msg).await;
            }
        }

        // Suggest switching to TSGO if auto mode chose tsserver
        if self.suggest_tsgo {
            self.client
                .show_message(
                    MessageType::INFO,
                    "Verter: Using workspace TypeScript (tsserver) for type checking. \
                     For faster performance, install TSGO and set verter.typeProvider to \"tsgo\" in VS Code settings.",
                )
                .await;
        }

        // Warn about TSGO limitations
        if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Verter: TSGO has known limitations — (1) re-exported .vue components \
                     (e.g. barrel files) may lose their typing; (2) path aliases from \
                     composite/referenced tsconfig files (e.g. tsconfig.app.json) are not \
                     resolved. If you experience issues, switch to tsserver: set \
                     verter.typeProvider to \"tsserver\".",
                )
                .await;
        }

        // Notify extension of MCP HTTP port (dynamic, OS-assigned).
        if let Some(port) = self.mcp_port {
            self.client
                .send_notification::<McpReady>(McpReadyParams { port })
                .await;
            tracing::info!("Sent $/verter/mcpReady with port {port}");
        }

        // C0. Eagerly populate type provider workspace roots so that
        // did_open (which can fire before background_init completes) sends
        // a reasonable projectRootPath to tsserver.
        if let Some(tp) = &self.type_provider {
            let roots = self.workspace_roots.lock().await;
            if !roots.is_empty() {
                let added: Vec<serde_json::Value> = roots
                    .iter()
                    .map(|uri| {
                        serde_json::json!({
                            "uri": uri,
                            "name": uri.rsplit('/').next().unwrap_or(uri)
                        })
                    })
                    .collect();
                drop(roots);
                let _ = tp.update_workspace_folders(added, vec![]).await;
            }
        }

        // C. Spawn background init (fire-and-forget)
        let init_lint_opts = self.init_lint_options.lock().await.take();
        self.spawn_background_init(init_lint_opts, "initialization")
            .await;

        // D. Register file system watchers for external file changes.
        // This enables did_change_watched_files notifications for source files,
        // Vue SFCs, and config files changed outside the editor (e.g., git checkout,
        // build tools, other editors). Enables non-VS Code clients (Neovim, etc.)
        // to get full external change detection via the standard LSP mechanism.
        let watch_kind = Some(WatchKind::Change | WatchKind::Create | WatchKind::Delete);
        let _ = self
            .client
            .register_capability(vec![Registration {
                id: "verter-file-watcher".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/*.vue".to_string()),
                            kind: watch_kind,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String(
                                "**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}".to_string(),
                            ),
                            kind: watch_kind,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/tsconfig*.json".to_string()),
                            kind: watch_kind,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/.verterrc.json".to_string()),
                            kind: watch_kind,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String(
                                "**/vite.config.{ts,js,mjs,cjs,mts,cts}".to_string(),
                            ),
                            kind: watch_kind,
                        },
                        FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/package.json".to_string()),
                            kind: watch_kind,
                        },
                    ],
                })
                .ok(),
            }])
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!("verter-lsp shutting down");
        // Gracefully shut down the type provider (sends LSP shutdown+exit to TSGO).
        if let Some(tp) = &self.type_provider {
            let _ = tp.shutdown().await;
        }
        self.client
            .log_message(MessageType::INFO, "verter-lsp shutting down")
            .await;
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let _hg = HandlerGuard::new("did_open");
        let uri = &params.text_document.uri;
        let _timer = self
            .statistics
            .timer("did_open", Some(uri.as_str().to_string()));
        tracing::info!("did_open: {}", uri.as_str());
        let result = self.documents.did_open(&params.text_document);
        let current_canonical_id = self.documents.get_canonical_id(uri);
        // Touch MRU for snapshot drain ordering (after did_open registers the canonical ID)
        if let Some(canonical_id) = current_canonical_id.as_ref() {
            self.touch_mru(canonical_id);
            if canonical_id.ends_with(".vue") {
                self.refresh_vue_dependency_tracking(canonical_id);
            }
        }
        if result.diagnostics.has_errors {
            tracing::debug!(
                "did_open: {} errors for {}",
                result.diagnostics.diagnostics.len(),
                uri.as_str(),
            );
        }
        let startup_policy = did_open_startup_policy(self.type_provider_kind);
        let prewarm_imported_vue_apis = startup_policy.sync_imported_vue_files
            && matches!(self.type_provider_kind, crate::TypeProviderKind::Tsserver);
        let imported_vue_priority_ids = self
            .documents
            .get_analysis(uri)
            .map(|analysis| {
                // Primary: analysis.imports already has resolved_canonical_id from host
                // (works even before background_init builds the resolver snapshot)
                let mut ids = collect_imported_vue_priority_ids_from_imports_with_fallback(
                    &analysis.imports,
                    current_canonical_id.as_deref(),
                    |parent, specifier| self.resolve_import_specifier(parent, specifier),
                );

                // Supplement: module_references for dynamic import()/require() cases
                // that aren't in analysis.imports (needs resolver, may return empty pre-init)
                if let Some(canonical_id) = current_canonical_id.as_ref() {
                    let snapshot = self.published_resolver();
                    let reader = LspProjectResolverReader::new(&self.documents);
                    let dynamic_ids = collect_priority_vue_targets_from_module_references(
                        snapshot.as_ref(),
                        &reader,
                        canonical_id,
                        &analysis.module_references,
                    );
                    // Dedup: add only IDs not already in the primary set
                    let seen: HashSet<String> = ids.iter().cloned().collect();
                    for id in dynamic_ids {
                        if !seen.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                ids
            })
            .unwrap_or_default();
        // Signal the background scanner to prioritize this file's directory
        if let Some(scanner) = self.workspace_scanner.lock().await.as_ref() {
            if let Some(canonical_id) = current_canonical_id.as_ref() {
                scanner.signal_priority(canonical_id.clone());
            }
            for import_id in &imported_vue_priority_ids {
                scanner.signal_priority(import_id.clone());
            }
        }

        if prewarm_imported_vue_apis {
            for import_id in &imported_vue_priority_ids {
                self.sync_imported_vue_api_lightweight(import_id).await;
            }
        }

        // Active file IDE sync FIRST (Interactive priority) — enables typed hover immediately.
        // tsserver is the exception: imported Vue public APIs are warmed above so the initial
        // open does not snapshot missing `.vue.ts` modules into the configured project.
        let provider_sync_policy = did_open_provider_sync_policy(self.type_provider_kind);
        if provider_sync_policy.await_ide_sync {
            // Use ensure_current_file_synced for immediate IDE-only sync
            self.ensure_current_file_synced(uri).await;
        }

        // Imported Vue API warmup SECOND (Normal priority, never blocks active file)
        if startup_policy.sync_imported_vue_files && !prewarm_imported_vue_apis {
            for import_id in &imported_vue_priority_ids {
                let should_sync =
                    !self.is_background_loaded_for_source_kind(import_id, ProviderPathKind::Api);
                if should_sync {
                    self.sync_imported_vue_api_lightweight(import_id).await;
                }
            }
        }

        // API sync (deferred — queued for coordinator)
        if provider_sync_policy.await_api_sync {
            self.sync_api_to_provider(uri).await;
        } else if provider_sync_policy.background_api_sync {
            self.sync_api_to_provider_in_background(uri.clone());
        }
        // Signal coordinator for fresh diagnostics on open (not just on change).
        // This ensures re-opening a file after external modifications publishes
        // up-to-date merged diagnostics (Verter lint + type provider).
        if let Some(coordinator) = &self.sync_coordinator {
            if let Some(canonical_id) = current_canonical_id.as_ref() {
                self.needs_ide_sync.insert(canonical_id.clone());
                self.needs_deferred_sync.insert(canonical_id.clone());
                coordinator.signal(canonical_id.clone(), uri.as_str().to_string());
            }
        }

        if startup_policy.publish_diagnostics {
            self.publish_full_diagnostics(uri).await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let _hg = HandlerGuard::new("did_change");
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;
        tracing::info!(
            "did_change ENTER v{version} {} thread={:?}",
            uri.as_str(),
            std::thread::current().id()
        );

        // Record change timestamp for typing cooldown (suppresses non-critical TSGO requests)
        self.last_change_ms.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            std::sync::atomic::Ordering::Relaxed,
        );

        // CRITICAL: Serialize did_change handlers via a tokio::sync::Mutex.
        //
        // tower-lsp dispatches did_change notifications CONCURRENTLY. Each handler calls
        // host.upsert() + host.ensure_compiled() which acquire std::sync::RwLock (blocking).
        // With N concurrent handlers on M worker threads, if N >= M all threads are blocked
        // on the RwLock, starving the runtime (no timers, heartbeats, or responses fire).
        //
        // By serializing through a tokio::sync::Mutex, waiting handlers YIELD their worker
        // thread instead of blocking it. Only one handler holds the blocking lock at a time.
        tracing::info!(
            "did_change MUTEX_WAIT v{version} active={} thread={:?}",
            ACTIVE_HANDLERS.load(std::sync::atomic::Ordering::Relaxed),
            std::thread::current().id()
        );
        let mutex_wait_start = std::time::Instant::now();
        let _guard = self.did_change_mutex.lock().await;
        tracing::info!(
            "did_change MUTEX_ACQUIRED v{version} wait={:?} thread={:?}",
            mutex_wait_start.elapsed(),
            std::thread::current().id()
        );
        tracing::info!("did_change MUTEX_ACQUIRED v{version}");

        let _timer = self
            .statistics
            .timer("did_change", Some(uri.as_str().to_string()));
        let is_virtual = self.documents.get_virtual_source_uri(&uri).is_some();

        tracing::info!(
            "did_change UPSERT_START v{version} thread={:?}",
            std::thread::current().id()
        );
        let upsert_start = std::time::Instant::now();
        let update_result = block_in_place_if_available(|| {
            self.documents
                .did_change_incremental(&uri, version, params.content_changes)
        });
        tracing::info!(
            "did_change UPSERT_DONE v{version} elapsed={:?} thread={:?}",
            upsert_start.elapsed(),
            std::thread::current().id()
        );

        // Virtual files don't need TSX sync or diagnostics.
        if is_virtual {
            tracing::info!("did_change EXIT (virtual) v{version}");
            return;
        }

        let style_only = update_result.changed && update_result.slice_changes.is_style_only();

        // Debounced type provider sync via SyncCoordinator.
        // All keystrokes reset the coordinator's timer → exactly 1 sync fires
        // after 300ms of silence. No concurrent spawned tasks.
        if !style_only {
            if let Some(canonical_id) = self.documents.get_canonical_id(&uri) {
                if canonical_id.ends_with(".vue") {
                    self.refresh_vue_dependency_tracking(&canonical_id);
                }
                self.needs_ide_sync.insert(canonical_id.clone());
                self.needs_deferred_sync.insert(canonical_id.clone());
                if let Some(coordinator) = &self.sync_coordinator {
                    coordinator.signal(canonical_id, uri.as_str().to_string());
                }

                // Eager TSX sync — send fresh TSX to type provider immediately.
                // sync_tsx is fire-and-forget (~1ms), so this adds negligible latency.
                // This ensures ALL subsequent requests (completion, hover, definition)
                // see fresh content without needing per-handler inline sync.
                if let Some(sync) = &self.project_sync {
                    if let Some(ide) = self.documents.get_ide(&uri) {
                        if let Some(ide_path) = self.ide_path_for_uri(&uri) {
                            if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                                tracing::warn!("did_change: eager tsx sync failed: {e}");
                            }
                        }
                    }
                }
            }
        }

        tracing::info!("did_change EXIT v{version}");
        // No diagnostics published during typing — old push diagnostics stay visible
        // and VS Code adjusts their positions as the document changes (line insertions etc.).
        // The SyncCoordinator publishes fresh merged diagnostics after 300ms of silence.
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let _hg = HandlerGuard::new("did_close");
        let uri = &params.text_document.uri;
        tracing::info!("did_close: {}", uri.as_str());
        // Virtual files don't have TSX in the provider
        if self.documents.get_virtual_source_uri(uri).is_none()
            && self.project_sync.is_some()
            && self.documents.get_ide(uri).is_some()
        {
            let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
                self.documents.did_close(uri);
                self.cached_verter_diags.remove(uri.as_str());
                return;
            };
            let state = self
                .provider_sync_state_for_source(&canonical_id)
                .or_else(|| {
                    self.documents.get_ide(uri).and_then(|ide| {
                        self.prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                            .map(|transition| transition.next)
                    })
                });
            let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);

            if let Some(state) = state {
                if is_tsgo {
                    // TSGO: always close IDE (.vue.tsx) — it was only opened for
                    // internal type checking of this file. DTS stays alive for imports.
                    if let Some(path) = state.ide_path.as_ref() {
                        self.close_provider_paths(&[(ProviderPathKind::Ide, path.clone())])
                            .await;
                    }
                } else if state.ide_background_loaded {
                    // tsserver: keep background-synced TSX alive for cross-file resolution.
                    tracing::debug!(
                        "did_close: keeping background-synced file in provider: {}",
                        state.ide_path.as_deref().unwrap_or("<missing>")
                    );
                } else {
                    // tsserver: close TSX and DTS for non-background files.
                    self.close_provider_state(&state).await;
                    self.remove_provider_sync_state(&canonical_id);
                }
            }
        }
        // Capture canonical_id before did_close clears document state.
        let canonical_id = self.documents.get_canonical_id(uri);

        // Clear the VFS overlay FIRST so the workspace falls back to disk.
        // This must happen before scheduler.close_file() because close_file
        // enqueues a background Source reload that reads via WorkspaceSourceLoader.
        self.documents.did_close(uri);
        self.cached_verter_diags.remove(uri.as_str());

        // Evict the host's FileEntry so ensure_loaded / get_source don't
        // serve stale editor-buffer content. Then tell the scheduler to reload
        // from disk.
        if let Some(ref canonical_id) = canonical_id {
            self.documents.host().evict(canonical_id);
            self.documents.host().scheduler().close_file(canonical_id);
        }
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // No-op; document content is already tracked via did_change
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        let _hg = HandlerGuard::new("did_change_workspace_folders");
        let event = &params.event;

        // Update workspace_roots (quick, non-blocking)
        {
            let mut roots = self.workspace_roots.lock().await;
            for removed in &event.removed {
                let uri_str = removed.uri.as_str().to_string();
                roots.retain(|r| r != &uri_str);
                tracing::info!("workspace folder removed: {}", uri_str);
            }
            for added in &event.added {
                let uri_str = added.uri.as_str().to_string();
                if !roots.contains(&uri_str) {
                    tracing::info!("workspace folder added: {}", uri_str);
                    roots.push(uri_str);
                }
            }
        }

        // Forward to type provider immediately (async, non-blocking)
        if let Some(tp) = &self.type_provider {
            let added: Vec<serde_json::Value> = event
                .added
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "uri": f.uri.as_str(),
                        "name": f.name
                    })
                })
                .collect();
            let removed: Vec<serde_json::Value> = event
                .removed
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "uri": f.uri.as_str(),
                        "name": f.name
                    })
                })
                .collect();
            let _ = tp.update_workspace_folders(added, removed).await;
        }

        // Spawn background task for the blocking work (registry rebuild + scanner)
        self.spawn_background_init(None, "workspace folder rebuild")
            .await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let _hg = HandlerGuard::new("did_change_watched_files");

        let mut ts_js_resync_ids = Vec::new();
        let mut ts_js_delete_ids = Vec::new();
        let mut vue_resync_ids = Vec::new();
        let mut vue_delete_ids: Vec<(String, String)> = Vec::new(); // (canonical_id, uri_str)
        let mut config_changed = false;

        for event in &params.changes {
            let canonical_id = uri_to_canonical_id(&event.uri);

            // Skip files that are currently open in the editor — the editor's
            // didChange notification is authoritative for open files.
            if self.documents.get(&event.uri).is_some() {
                continue;
            }

            // Skip watcher events for Verter-generated @verter/types stubs.
            // Real installed @verter/types packages (no marker) pass through normally.
            if is_generated_verter_types_event(&canonical_id) {
                continue;
            }

            if is_config_file(&canonical_id) {
                config_changed = true;
                tracing::debug!("did_change_watched_files: config file changed: {canonical_id}");
                // Config files also trigger vite dep check below, but the
                // registry rebuild is the primary action.
            } else if is_vue_file(&canonical_id) {
                if event.typ == FileChangeType::DELETED {
                    vue_delete_ids.push((canonical_id, event.uri.as_str().to_string()));
                } else {
                    vue_resync_ids.push(canonical_id);
                }
            } else {
                // TS/JS source file
                if event.typ == FileChangeType::DELETED {
                    ts_js_delete_ids.push(canonical_id);
                } else {
                    ts_js_resync_ids.push(canonical_id);
                }
            }
        }

        // ── Vue file deletions ─────────────────────────────────────
        for (canonical_id, uri_str) in &vue_delete_ids {
            self.documents.host().invalidate_dependents_of(canonical_id);
            if let Some(state) = self.remove_provider_sync_state(canonical_id).or_else(|| {
                let profile = self.documents.tsx_profile.read().clone();
                self.documents
                    .host()
                    .get_ide(canonical_id, &profile)
                    .and_then(|ide| {
                        self.prepare_vue_provider_sync_transition(canonical_id, ide.is_jsx)
                            .map(|transition| transition.next)
                    })
            }) {
                self.close_provider_state(&state).await;
            }
            self.documents.host().remove(canonical_id);
            self.cached_verter_diags.remove(uri_str.as_str());
            tracing::debug!("did_change_watched_files: removed vue {canonical_id}");
        }

        // ── Vue file creates/changes ───────────────────────────────
        for canonical_id in &vue_resync_ids {
            self.documents.host().invalidate_dependents_of(canonical_id);
            self.resync_background_vue_file(canonical_id).await;
            tracing::debug!("did_change_watched_files: resynced vue {canonical_id}");
        }

        // ── TS/JS file deletions ───────────────────────────────────
        for canonical_id in &ts_js_delete_ids {
            self.documents.host().invalidate_dependents_of(canonical_id);
            if let Some(state) = self.remove_provider_sync_state(canonical_id) {
                self.close_provider_state(&state).await;
            }
            self.documents.host().remove(canonical_id);
            tracing::debug!("did_change_watched_files: removed {canonical_id}");
        }

        // ── TS/JS file creates/changes ─────────────────────────────
        if !ts_js_resync_ids.is_empty() {
            for canonical_id in &ts_js_resync_ids {
                self.documents.host().invalidate_dependents_of(canonical_id);
            }
            if let Some(sync) = &self.project_sync {
                let host = self.documents.host_arc();
                let sync = sync.clone();
                let vfs_workspace = Arc::clone(&self.vfs_workspace);
                let provider_sync_states = Arc::clone(&self.provider_sync_states);
                let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);

                tokio::spawn(async move {
                    for canonical_id in ts_js_resync_ids {
                        crate::workspace_scanner::resync_non_vue_file(
                            &canonical_id,
                            &host,
                            &sync,
                            &vfs_workspace,
                            is_tsgo,
                            &provider_sync_states,
                        )
                        .await;
                        tracing::debug!("did_change_watched_files: resynced {canonical_id}");
                    }
                });
            }
        }

        // ── Config file changes → registry rebuild ─────────────────
        // Also check whether any changed file is a vite config dependency
        // (mirrors the logic in on_file_changed).
        if !config_changed {
            let all_changed: Vec<String> = params
                .changes
                .iter()
                .map(|e| uri_to_canonical_id(&e.uri))
                .collect();
            let ws = self.vfs_workspace.read();
            if let Some(published) = ws.as_ref().and_then(|ws| ws.load_published()) {
                if let Some(views) = published.ext::<crate::workspace_state::LspViews>() {
                    for canonical_id in &all_changed {
                        if views
                            .project_views
                            .iter()
                            .any(|v| v.vite_config_deps.iter().any(|dep| dep == canonical_id))
                        {
                            config_changed = true;
                            tracing::debug!(
                                "did_change_watched_files: vite config dep changed: {canonical_id}"
                            );
                            break;
                        }
                    }
                }
            }
        }
        if config_changed {
            self.trigger_registry_rebuild().await;
        }
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        let _hg = HandlerGuard::new("did_create_files");
        for file in &params.files {
            // Only index .vue files
            if !file.uri.ends_with(".vue") {
                continue;
            }
            let uri: Uri = match file.uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let canonical_id = uri_to_canonical_id(&uri);
            // Load the file through ingress so it's indexed without needing to open in editor
            self.documents.host().ensure_loaded(&canonical_id);
            // Compile and sync to type provider for cross-file type resolution
            self.resync_background_vue_file(&canonical_id).await;
            tracing::debug!("did_create_files: indexed {}", file.uri);
        }
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        let _hg = HandlerGuard::new("did_delete_files");
        for file in &params.files {
            if !file.uri.ends_with(".vue") {
                continue;
            }
            let uri: Uri = match file.uri.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };
            let canonical_id = uri_to_canonical_id(&uri);
            if let Some(state) = self.remove_provider_sync_state(&canonical_id).or_else(|| {
                let profile = self.documents.tsx_profile.read().clone();
                self.documents
                    .host()
                    .get_ide(&canonical_id, &profile)
                    .and_then(|ide| {
                        self.prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                            .map(|transition| transition.next)
                    })
            }) {
                self.close_provider_state(&state).await;
            }
            self.documents.host().remove(&canonical_id);
            self.cached_verter_diags.remove(uri.as_str());
            tracing::debug!("did_delete_files: removed {}", file.uri);
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let _hg = HandlerGuard::new("hover");
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;
        tracing::info!(
            "hover ENTER {} at {}:{}",
            uri.as_str(),
            position.line,
            position.character
        );
        let _timer = self
            .statistics
            .timer("hover", Some(uri.as_str().to_string()));

        // Virtual file: route directly through TSGO (position is already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(Some(info)) = tp.get_hover(&tsx_path, offset).await {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: info.contents,
                            }),
                            range: None,
                        }));
                    }
                }
                return Ok(None);
            }
        }

        let ssr_context = {
            let canonical_id = self.documents.get_canonical_id(uri);
            canonical_id
                .as_deref()
                .map(|cid| self.is_ssr_context(cid))
                .unwrap_or(false)
        };

        let verter_full = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            hover_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                ssr_context,
            )
        })();
        let vue_kind_label = verter_full.as_ref().and_then(|r| r.vue_kind_label.clone());
        let verter_result = verter_full.map(|r| r.hover);

        let child_hover_target = (|| {
            let analysis = self.documents.get_analysis(uri)?;
            let doc = self.documents.get(uri)?;
            let vue_offset = doc.line_index.position_to_offset(position)?;
            hover::child_hover_target_at_offset(vue_offset, &doc.source, &analysis)
        })();
        if let Some(target) = child_hover_target.as_ref() {
            if let Some(child_hover) = self.child_hover_for_target(uri, target) {
                return Ok(Some(child_hover));
            }
        }

        // Slot syntax: verter provides rich hover; type provider returns unhelpful
        // generic types (`() any`, `string`). Skip type provider merge entirely.
        if verter_result.is_some() {
            if let Some(analysis) = self.documents.get_analysis(uri) {
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(vue_offset) = doc.line_index.position_to_offset(position) {
                        if hover::is_on_slot_syntax(vue_offset, &analysis) {
                            return Ok(verter_result);
                        }
                    }
                }
            }
        }

        // Enhance with TypeProvider if available.
        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                // Use validated mapping to avoid querying TSGO at synthetic TSX
                // positions (e.g., <div> → generated JSX) which can crash it.
                let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );

                let type_hover = if let Some(tsx_offset) = tsx_offset {
                    // Log TSX context snippet around the hover offset for debugging
                    if let Some((before, after)) =
                        debug_snippet(&ctx.tsx_content, tsx_offset as usize)
                    {
                        tracing::info!(
                            "hover TSX context at offset {}: «{}⸽{}»",
                            tsx_offset,
                            before.replace('\n', "↵"),
                            after.replace('\n', "↵"),
                        );
                    }
                    match tp.get_hover(&ctx.tsx_path, tsx_offset).await {
                        Ok(hover) => {
                            tracing::info!(
                                "hover type provider result: {}",
                                if hover.is_some() {
                                    hover
                                        .as_ref()
                                        .map(|h| h.contents.as_str())
                                        .unwrap_or("Some(empty)")
                                } else {
                                    "None"
                                }
                            );
                            hover
                        }
                        Err(e) => {
                            tracing::warn!("hover type provider error: {}", e);

                            None
                        }
                    }
                } else {
                    tracing::info!(
                        "hover: vue_to_tsx validation failed for {}:{} — position is in synthetic TSX region",
                        position.line,
                        position.character
                    );
                    None
                };

                // If TSGO returned a result, merge and return.
                if type_hover.is_some() {
                    return Ok(merge::merge_hover(
                        verter_result,
                        type_hover,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                        &ctx.vue_line_index,
                        vue_kind_label.as_deref(),
                    ));
                }

                // Redirect: when TSGO returned nothing and the cursor is on a static
                // `class`/`style` attribute that was merged with a dynamic binding,
                // the static attribute's source position maps to removed TSX content.
                // Retry at the dynamic directive's position instead.
                if let Some(analysis) = self.documents.get_analysis(uri) {
                    let vue_offset = ctx.vue_line_index.position_to_offset(position);
                    if let Some(vue_offset) = vue_offset {
                        if let Some(redirect_offset) =
                            hover::merged_attribute_redirect_offset(vue_offset, &analysis)
                        {
                            // Convert the redirect SFC offset to a Vue line:col position
                            if let Some(redirect_pos) =
                                ctx.vue_line_index.offset_to_position(redirect_offset)
                            {
                                if let Some(redirect_tsx) =
                                    merge::vue_position_to_tsx_offset_validated(
                                        &redirect_pos,
                                        &ctx.vue_line_index,
                                        &ctx.mapper,
                                        &ctx.tsx_line_index,
                                    )
                                {
                                    tracing::info!(
                                        "hover: redirecting merged class/style from vue offset {} to {} (tsx offset {})",
                                        vue_offset, redirect_offset, redirect_tsx
                                    );
                                    if let Ok(redirect_hover) =
                                        tp.get_hover(&ctx.tsx_path, redirect_tsx).await
                                    {
                                        return Ok(merge::merge_hover(
                                            verter_result,
                                            redirect_hover,
                                            &ctx.mapper,
                                            &ctx.tsx_line_index,
                                            &ctx.vue_line_index,
                                            vue_kind_label.as_deref(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                return Ok(merge::merge_hover(
                    verter_result,
                    None,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                    &ctx.vue_line_index,
                    vue_kind_label.as_deref(),
                ));
            } else {
                tracing::info!("hover: no ide_context for {}", uri.as_str());
            }
        } else {
            tracing::info!("hover: no type_provider");
        }

        Ok(verter_result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let _hg = HandlerGuard::new("completion");
        let uri = &params.text_document_position.text_document.uri;
        let _timer = self
            .statistics
            .timer("completion", Some(uri.as_str().to_string()));
        // Increment the generation counter so stale requests can detect they've been superseded.
        let completion_gen = self
            .completion_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let position = &params.text_document_position.position;
        let trigger_character = params
            .context
            .as_ref()
            .and_then(|ctx| ctx.trigger_character.as_deref());
        tracing::info!(
            "completion ENTER {} at {}:{} (trigger={:?})",
            uri.as_str(),
            position.line,
            position.character,
            trigger_character
        );

        // Check coalescing — skip stale requests superseded by newer keystrokes.
        if self
            .completion_generation
            .load(std::sync::atomic::Ordering::Relaxed)
            != completion_gen + 1
        {
            tracing::debug!(
                "completion: skipping stale request (gen {})",
                completion_gen
            );
            return Ok(None);
        }

        // NOTE: We do NOT call ensure_provider_synced here.  The debounced sync in
        // did_change sends the update to TSGO within 50ms of the last keystroke.
        // Flushing inline would serialize: sync → TSGO re-analysis → get_completions,
        // which takes 2-3s on large files and blocks the entire completion pipeline.
        // Instead we let TSGO answer with whatever version it has; if it's stale the
        // response arrives fast and VS Code re-requests after the debounce fires.

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(result) = tp
                        .get_completions(&tsx_path, offset, trigger_character)
                        .await
                    {
                        let items: Vec<CompletionItem> = result
                            .items
                            .into_iter()
                            .filter(|c| {
                                !c.label.starts_with("___VERTER___") && !c.label.starts_with("$V_")
                            })
                            .map(|c| CompletionItem {
                                label: c.label,
                                detail: c.detail,
                                documentation: c.documentation.map(|d| {
                                    Documentation::MarkupContent(MarkupContent {
                                        kind: MarkupKind::Markdown,
                                        value: d,
                                    })
                                }),
                                sort_text: c.sort_text,
                                ..Default::default()
                            })
                            .collect();
                        return Ok(if items.is_empty() {
                            None
                        } else {
                            Some(CompletionResponse::List(CompletionList {
                                is_incomplete: result.is_incomplete,
                                items,
                            }))
                        });
                    }
                }
                return Ok(None);
            }
        }

        let completion_ssr_context = {
            let canonical_id = self.documents.get_canonical_id(uri);
            canonical_id
                .as_deref()
                .map(|cid| self.is_ssr_context(cid))
                .unwrap_or(false)
        };

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let canonical_id = crate::documents::uri_to_canonical_id(uri);
            let resolve_component = |import_source: &str,
                                     component_name: Option<&str>|
             -> Option<verter_host::FileAnalysisSnapshot> {
                let try_follow_reexport =
                    |resolved: &str,
                     comp_name: Option<&str>|
                     -> Option<verter_host::FileAnalysisSnapshot> {
                        if resolved.ends_with(".vue") {
                            return self.documents.host().get_analysis(resolved);
                        }
                        // Ensure the barrel file is loaded so we can inspect its exports
                        if self.documents.host().get_analysis(resolved).is_none() {
                            self.documents.host().ensure_loaded(resolved);
                        }
                        // For non-.vue files (barrel/index), follow re-export chains if we know the component name
                        if let Some(name) = comp_name {
                            if let Some((terminal_id, _, _)) = self
                                .documents
                                .host()
                                .get_export_span_follow_reexports(resolved, name)
                            {
                                if terminal_id.ends_with(".vue") {
                                    // Ensure the terminal .vue file is compiled
                                    if self.documents.host().get_analysis(&terminal_id).is_none() {
                                        self.documents.host().ensure_loaded(&terminal_id);
                                    }
                                    return self.documents.host().get_analysis(&terminal_id);
                                }
                            }
                        }
                        self.documents.host().get_analysis(resolved)
                    };

                // Try 1: Use resolve_import_specifier (handles relative, alias, index files)
                if let Some(resolved) = self.resolve_import_specifier(&canonical_id, import_source)
                {
                    if let Some(a) = try_follow_reexport(&resolved, component_name) {
                        return Some(a);
                    }
                }

                // Try 2: Manual relative resolution (fallback for host-cached files not on disk)
                if import_source.starts_with('.') {
                    let parts: Vec<&str> = canonical_id.split('/').collect();
                    let dir = parts[..parts.len().saturating_sub(1)].join("/");
                    let resolved = if let Some(stripped) = import_source.strip_prefix("./") {
                        format!("{}/{}", dir, stripped)
                    } else if import_source.starts_with("../") {
                        let mut dir_parts: Vec<&str> = dir.split('/').collect();
                        let mut rel = import_source;
                        while let Some(rest) = rel.strip_prefix("../") {
                            dir_parts.pop();
                            rel = rest;
                        }
                        format!(
                            "{}/{}",
                            dir_parts.join("/"),
                            rel.strip_prefix("./").unwrap_or(rel)
                        )
                    } else {
                        format!("{}/{}", dir, import_source)
                    };
                    if let Some(a) = try_follow_reexport(&resolved, component_name) {
                        return Some(a);
                    }
                }

                // Try 3: VFS resolution (path aliases, tsconfig paths, disk probing)
                if let Some(resolved_path) =
                    self.resolve_import_specifier(&canonical_id, import_source)
                {
                    if let Some(a) = try_follow_reexport(&resolved_path, component_name) {
                        return Some(a);
                    }
                }

                // Try 4: Direct lookup (bare specifiers, already-resolved)
                try_follow_reexport(import_source, component_name)
            };
            // Build workspace component list for auto-import
            let ws_components = build_workspace_components(&self.documents.host, &canonical_id);
            completions_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                Some(&resolve_component),
                if ws_components.is_empty() {
                    None
                } else {
                    Some(&ws_components)
                },
                Some(uri.as_str()),
                completion_ssr_context,
            )
        })();

        let verter_is_incomplete = verter_result
            .as_ref()
            .map(|r| r.is_incomplete)
            .unwrap_or(false);
        let verter_items = verter_result.map(|r| r.items);

        // Compute cursor context once — derive attribute vs expression context
        let (is_template_attr_context, in_expression_context) = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let offset = doc.line_index.position_to_offset(position)?;
            let context = classify_cursor_context(offset, &doc.source, &blocks, analysis.as_ref());
            Some(match &context {
                CursorContext::Template(TemplateCursorContext::AttributeName { .. }) => {
                    (true, false)
                }
                CursorContext::Template(
                    TemplateCursorContext::Expression { .. } | TemplateCursorContext::Interpolation,
                ) => (false, true),
                CursorContext::Template(_) => (false, false),
                _ => (false, false),
            })
        })()
        .unwrap_or((false, false));

        // Enhance with TypeProvider if available.
        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsserver)
                && self.current_file_needs_inline_type_provider_sync(uri)
            {
                tracing::debug!(
                    "completion: repairing current-file tsserver sync for {}",
                    uri.as_str()
                );
                self.ensure_current_file_synced(uri).await;
            }
            self.ensure_imported_vue_apis_synced(uri).await;
            let ctx = self.type_provider_context(uri);
            if ctx.is_none() {
                tracing::debug!("completion: no ide_context for {}", uri.as_str());
            }
            if let Some(ctx) = ctx {
                // TSX is always fresh in the type provider — synced eagerly in did_change.
                // Only DTS sync and diagnostics publishing are debounced (300ms via SyncCoordinator).

                let tsx_offset = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                );
                if tsx_offset.is_none() {
                    tracing::debug!(
                        "completion: position mapping failed for {}:{},{}",
                        uri.as_str(),
                        position.line,
                        position.character,
                    );
                }
                // Template completion has two complementary flags based on expression context:
                //
                // 1. `suppress_verter`: In MemberAccess/Literal/Type/PropertyKey contexts,
                //    verter's identifier-level completions are irrelevant — only the TypeProvider
                //    knows the object's members. So we suppress verter items.
                //
                // 2. `skip_type_provider`: In IdentifierExpected context, the TypeProvider
                //    returns ALL globals in scope (AbortController, HTMLElement, Array, etc.)
                //    which are NOT accessible in Vue template expressions (templates use a
                //    render proxy that only exposes script setup bindings). Verter's
                //    template_completions() already provides exactly the right set.
                //
                // | ExpressionContext    | suppress_verter | skip_type_provider |
                // |----------------------|-----------------|--------------------|
                // | IdentifierExpected   | false           | true               |
                // | MemberAccess         | true            | false              |
                // | Literal/Type/PropKey | true            | false              |
                // | Unknown              | false           | false (filtered)   |
                let expr_context = if in_expression_context {
                    tsx_offset.map(|off| {
                        classify_expression_context_with_trigger(
                            &ctx.tsx_content,
                            off as usize,
                            trigger_character,
                        )
                    })
                } else {
                    None
                };

                let suppress_verter = expr_context
                    .as_ref()
                    .map(|ec| {
                        matches!(
                            ec,
                            ExpressionContext::MemberAccess
                                | ExpressionContext::Literal
                                | ExpressionContext::TypePosition
                                | ExpressionContext::PropertyKey
                        )
                    })
                    .unwrap_or(false);

                if let Some(tsx_offset) = tsx_offset {
                    let identifier_prefix = expr_context.as_ref().and_then(|ec| {
                        matches!(
                            ec,
                            ExpressionContext::IdentifierExpected | ExpressionContext::Unknown
                        )
                        .then(|| {
                            identifier_prefix_before_offset(&ctx.tsx_content, tsx_offset as usize)
                        })
                        .flatten()
                        .map(str::to_string)
                    });

                    let skip_type_provider = expr_context
                        .as_ref()
                        .map(|ec| {
                            matches!(ec, ExpressionContext::IdentifierExpected)
                                && identifier_prefix.is_none()
                        })
                        .unwrap_or(false);

                    if skip_type_provider {
                        tracing::debug!(
                            "completion: skipping type provider for IdentifierExpected context"
                        );
                        return Ok(verter_items.map(|items| {
                            CompletionResponse::List(CompletionList {
                                is_incomplete: verter_is_incomplete,
                                items,
                            })
                        }));
                    }
                    // Only forward trigger characters that tsserver/TSGO recognize.
                    // Vue-specific triggers (":", "@", " ") are handled by Verter's
                    // native completions and cause tsserver errors if forwarded.
                    let tp_trigger = trigger_character
                        .filter(|t| matches!(*t, "." | "\"" | "'" | "`" | "/" | "<"))
                        .or_else(|| {
                            (matches!(expr_context, Some(ExpressionContext::MemberAccess))
                                && is_immediately_after_member_access_dot(
                                    &ctx.tsx_content,
                                    tsx_offset as usize,
                                ))
                            .then_some(".")
                        });
                    let mut type_completion_result = tp
                        .get_completions(&ctx.tsx_path, tsx_offset, tp_trigger)
                        .await;
                    if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsserver) {
                        for retry_delay_ms in [50u64, 150, 300] {
                            let needs_retry = matches!(
                                type_completion_result,
                                Err(ref error) if error.message.contains("No content available")
                            );
                            if !needs_retry {
                                break;
                            }
                            tracing::debug!(
                                "completion: retrying tsserver completion after no-content error for {} (delay={}ms)",
                                ctx.tsx_path,
                                retry_delay_ms
                            );
                            self.force_reopen_current_file_in_type_provider(uri).await;
                            self.sync_api_to_provider(uri).await;
                            self.ensure_imported_vue_apis_synced(uri).await;
                            tokio::time::sleep(std::time::Duration::from_millis(retry_delay_ms))
                                .await;
                            type_completion_result = tp
                                .get_completions(&ctx.tsx_path, tsx_offset, tp_trigger)
                                .await;
                        }
                    }
                    match type_completion_result {
                        Ok(mut type_result) => {
                            tracing::debug!(
                                "completion: type provider returned {} items (incomplete={})",
                                type_result.items.len(),
                                type_result.is_incomplete
                            );

                            filter_type_provider_completion_result(
                                &mut type_result,
                                expr_context.as_ref(),
                                identifier_prefix.as_deref(),
                                verter_items.as_ref(),
                            );

                            if matches!(expr_context, Some(ExpressionContext::MemberAccess))
                                && tp_trigger == Some(".")
                                && type_result.items.is_empty()
                            {
                                tracing::debug!(
                                    "completion: retrying member access without dot trigger after empty backend result"
                                );
                                if let Ok(mut retry_result) =
                                    tp.get_completions(&ctx.tsx_path, tsx_offset, None).await
                                {
                                    filter_type_provider_completion_result(
                                        &mut retry_result,
                                        expr_context.as_ref(),
                                        identifier_prefix.as_deref(),
                                        verter_items.as_ref(),
                                    );
                                    if !retry_result.items.is_empty() {
                                        type_result = retry_result;
                                    }
                                }
                            }

                            let (merged, is_incomplete) = merge::merge_completions(
                                if suppress_verter {
                                    Vec::new()
                                } else {
                                    verter_items.unwrap_or_default()
                                },
                                type_result,
                                &ctx.mapper,
                                &ctx.tsx_line_index,
                                &ctx.vue_line_index,
                                Some(&ctx.tsx_path),
                                is_template_attr_context,
                            );
                            return Ok(if merged.is_empty() {
                                None
                            } else {
                                Some(CompletionResponse::List(CompletionList {
                                    is_incomplete: is_incomplete || verter_is_incomplete,
                                    items: merged,
                                }))
                            });
                        }
                        Err(e) => {
                            tracing::warn!("completion: type provider error: {e}");
                        }
                    }
                }
            }
        } else {
            tracing::debug!("completion: no type provider available");
        }

        Ok(verter_items.map(|items| {
            CompletionResponse::List(CompletionList {
                is_incomplete: verter_is_incomplete,
                items,
            })
        }))
    }

    async fn completion_resolve(&self, mut item: CompletionItem) -> Result<CompletionItem> {
        let _hg = HandlerGuard::new("completion_resolve");
        // Check if this item requires auto-import (verter workspace components)
        if let Some(ref data) = item.data {
            if data.get("auto_import").and_then(|v| v.as_bool()) == Some(true) {
                if let (Some(import_path), Some(component_name), Some(doc_uri)) = (
                    data.get("import_path").and_then(|v| v.as_str()),
                    data.get("component_name").and_then(|v| v.as_str()),
                    data.get("uri").and_then(|v| v.as_str()),
                ) {
                    if let Some(edit) =
                        self.build_auto_import_edit(doc_uri, component_name, import_path)
                    {
                        item.additional_text_edits = Some(vec![edit]);
                    }
                }
            }

            // Check if this item is from TSGO and needs resolve for auto-import
            if data.get("tsgo").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(tp) = &self.type_provider {
                    if let (Some(tsx_path), Some(original_data)) = (
                        data.get("tsx_path").and_then(|v| v.as_str()),
                        data.get("original_data"),
                    ) {
                        // Only call resolve if original_data is not null
                        if !original_data.is_null() {
                            if let Ok(Some(resolve_result)) =
                                tp.resolve_completion(tsx_path, original_data.clone()).await
                            {
                                if !resolve_result.additional_text_edits.is_empty() {
                                    // Map TSX positions to Vue positions
                                    if let Some((_, tsx_content, mapper)) =
                                        self.ide_context_by_path(tsx_path)
                                    {
                                        let tsx_li =
                                            LineIndex::new(&tsx_content, self.documents.encoding());
                                        // Find the Vue URI from tsx_path
                                        if let Some(vue_uri) = self.vue_uri_from_ide_path(tsx_path)
                                        {
                                            if let Some(doc) = self.documents.get(&vue_uri) {
                                                let edits: Vec<TextEdit> = resolve_result
                                                    .additional_text_edits
                                                    .iter()
                                                    .filter_map(|e| {
                                                        let range = merge::tsx_range_to_vue_range(
                                                            e.start,
                                                            e.end,
                                                            &tsx_li,
                                                            &mapper,
                                                            &doc.line_index,
                                                        )?;
                                                        Some(TextEdit {
                                                            range,
                                                            new_text: e.new_text.clone(),
                                                        })
                                                    })
                                                    .collect();
                                                if !edits.is_empty() {
                                                    item.additional_text_edits = Some(edits);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(item)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let _hg = HandlerGuard::new("goto_definition");
        let uri = &params.text_document_position_params.text_document.uri;
        let _timer = self
            .statistics
            .timer("definition", Some(uri.as_str().to_string()));
        let position = &params.text_document_position_params.position;
        tracing::debug!(
            "definition: {} at {}:{}",
            uri.as_str(),
            position.line,
            position.character
        );

        self.ensure_provider_synced(uri).await;

        // Virtual file: route directly through TSGO (position is already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_defs) = tp.get_definition(&tsx_path, offset).await {
                        let locations: Vec<Location> = type_defs
                            .into_iter()
                            .filter_map(|d| {
                                // Strip virtual suffixes so user navigates to .vue
                                let vue_source_exists =
                                    |p: &str| self.documents.host().get_source(p).is_some();
                                let target_path =
                                    merge::normalize_vue_path_owned(&d.path, &vue_source_exists);
                                let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                                // Convert byte offsets to positions using vf LineIndex for
                                // same-file refs; for external files, fall back to 0:0
                                let range = if d.path == tsx_path {
                                    Range {
                                        start: vf_li
                                            .offset_to_position(d.start)
                                            .unwrap_or_default(),
                                        end: vf_li.offset_to_position(d.end).unwrap_or_default(),
                                    }
                                } else {
                                    Range::default()
                                };
                                Some(Location {
                                    uri: target_uri,
                                    range,
                                })
                            })
                            .collect();
                        if !locations.is_empty() {
                            return Ok(Some(GotoDefinitionResponse::Array(locations)));
                        }
                    }
                }
                return Ok(None);
            }
        }

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let canonical_id = uri_to_canonical_id(uri);
            let resolve_path = {
                let canonical_id = canonical_id.clone();
                let host = &self.documents.host;
                move |specifier: &str| -> Option<String> {
                    host.resolve_import_via_workspace(&canonical_id, specifier)
                }
            };
            #[allow(clippy::type_complexity)]
            let resolve_fn: Option<&dyn Fn(&str) -> Option<String>> =
                Some(&resolve_path as &dyn Fn(&str) -> Option<String>);

            let encoding = self.position_encoding.read().clone();
            let host = &self.documents.host;
            let resolve_export =
                |target_canonical_id: &str, binding_name: &str| -> Option<Location> {
                    // Follow re-exports (cycle-detected) to find the actual definition
                    let (resolved_id, start, end) = host
                        .get_export_span_follow_reexports(target_canonical_id, binding_name)
                        .or_else(|| {
                            // Fallback to non-following version for backwards compat
                            let (s, e) = host.get_export_span(target_canonical_id, binding_name)?;
                            Some((target_canonical_id.to_string(), s, e))
                        })?;
                    let target_source = host.get_source(&resolved_id)?;
                    let target_li = LineIndex::new(&target_source, encoding.clone());
                    let start_pos = target_li.offset_to_position(start)?;
                    let end_pos = target_li.offset_to_position(end)?;
                    let normalized = resolved_id.replace('\\', "/");
                    let uri_str = if normalized.starts_with('/') {
                        format!("file://{normalized}")
                    } else if normalized.chars().nth(1) == Some(':') {
                        format!("file:///{normalized}")
                    } else {
                        return None;
                    };
                    let target_uri: Uri = uri_str.parse().ok()?;
                    Some(Location {
                        uri: target_uri,
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
                    })
                };
            #[allow(clippy::type_complexity)]
            let resolve_export_fn =
                Some(&resolve_export as &dyn Fn(&str, &str) -> Option<Location>);

            // Unified component contract resolution runs FIRST: props, events,
            // v-model, slots. Returns early if any contract surface was hit.
            if let Some(contract_def) = self.try_component_contract_definition(uri, position) {
                return Some(contract_def);
            }

            // Barrel-file export symbol click: if the cursor is on an export
            // signature in a re-export statement, follow the chain to the terminal.
            if let Some(barrel_def) = self.try_barrel_export_definition(uri, position) {
                return Some(barrel_def);
            }

            let mut def = definition_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                resolve_fn,
                resolve_export_fn,
            )?;

            // Fix up sentinel URIs: if the definition is in the same file, use the document URI
            if let GotoDefinitionResponse::Scalar(ref mut loc) = def {
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI_STR {
                    loc.uri = uri.clone();
                }
            }

            Some(def)
        })();

        tracing::debug!("definition: verter found={}", verter_result.is_some());

        // If verter already resolved a cross-file definition, return it directly.
        // Querying TSGO with a synthetic TSX position often crashes it.
        if let Some(GotoDefinitionResponse::Scalar(ref loc)) = verter_result {
            if loc.uri.as_str() != uri.as_str() {
                tracing::debug!("definition: verter resolved cross-file, skipping type provider");
                return Ok(verter_result);
            }
        }

        // Component contract resolution (props, events, v-model, slots) now runs
        // BEFORE definition_at_position inside the closure above via
        // try_component_contract_definition. The old separate resolve_component_event_definition
        // and resolve_component_prop_definition calls are subsumed by it.

        // Enhance with TypeProvider for cross-file definitions.
        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                // Use validated mapping to avoid querying TSGO at synthetic TSX
                // positions (e.g., <div> → generated JSX) which can crash it.
                if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    tracing::debug!(
                        "definition: querying type provider at tsx offset {}",
                        tsx_offset
                    );
                    match tp.get_definition(&ctx.tsx_path, tsx_offset).await {
                        Ok(type_defs) => {
                            tracing::debug!(
                                "definition: type provider returned {} locations",
                                type_defs.len()
                            );
                            let vue_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            let barrel_resolver =
                                |path: &str, start: u32, end: u32| -> Option<Location> {
                                    self.resolve_barrel_type_provider_location(path, start, end)
                                };
                            let merged = merge::merge_definitions_with_barrel_resolver(
                                verter_result,
                                type_defs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                uri,
                                &vue_source_exists,
                                Some(&barrel_resolver),
                            );
                            // Post-process: if type provider resolved to a barrel file,
                            // follow re-exports to the terminal declaration.
                            return Ok(self.resolve_barrel_locations(merged));
                        }
                        Err(e) => {
                            tracing::warn!("definition: type provider error: {e}");
                        }
                    }
                } else {
                    tracing::debug!(
                        "definition: position mapping failed for {}:{}:{}",
                        uri.as_str(),
                        position.line,
                        position.character
                    );
                }
            }
        }

        Ok(verter_result)
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let _hg = HandlerGuard::new("goto_type_definition");
        let uri = &params.text_document_position_params.text_document.uri;
        let _timer = self
            .statistics
            .timer("type_definition", Some(uri.as_str().to_string()));
        let position = &params.text_document_position_params.position;
        tracing::debug!(
            "type_definition: {} at {}:{}",
            uri.as_str(),
            position.line,
            position.character
        );

        self.ensure_provider_synced(uri).await;

        // Virtual file: route directly through type provider (position is already in TSX coordinates)
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_defs) = tp.get_type_definition(&tsx_path, offset).await {
                        let locations: Vec<Location> = type_defs
                            .into_iter()
                            .filter_map(|d| {
                                let vue_source_exists =
                                    |p: &str| self.documents.host().get_source(p).is_some();
                                if let Some(location) = self
                                    .resolve_barrel_type_provider_location(&d.path, d.start, d.end)
                                {
                                    return Some(location);
                                }
                                let target_path =
                                    merge::normalize_vue_path_owned(&d.path, &vue_source_exists);
                                let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                                let range = if d.path == tsx_path {
                                    Range {
                                        start: vf_li
                                            .offset_to_position(d.start)
                                            .unwrap_or_default(),
                                        end: vf_li.offset_to_position(d.end).unwrap_or_default(),
                                    }
                                } else {
                                    Range::default()
                                };
                                Some(Location {
                                    uri: target_uri,
                                    range,
                                })
                            })
                            .collect();
                        if !locations.is_empty() {
                            return Ok(Some(GotoDefinitionResponse::Array(locations)));
                        }
                    }
                }
                return Ok(None);
            }
        }

        // Type definition is purely a type provider operation — no verter analysis phase.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    tracing::debug!(
                        "type_definition: querying type provider at tsx offset {}",
                        tsx_offset
                    );
                    match tp.get_type_definition(&ctx.tsx_path, tsx_offset).await {
                        Ok(type_defs) => {
                            tracing::debug!(
                                "type_definition: type provider returned {} locations",
                                type_defs.len()
                            );
                            let vue_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            let barrel_resolver =
                                |path: &str, start: u32, end: u32| -> Option<Location> {
                                    self.resolve_barrel_type_provider_location(path, start, end)
                                };
                            return Ok(merge::merge_definitions_with_barrel_resolver(
                                None,
                                type_defs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                uri,
                                &vue_source_exists,
                                Some(&barrel_resolver),
                            ));
                        }
                        Err(e) => {
                            tracing::warn!("type_definition: type provider error: {e}");
                        }
                    }
                } else {
                    tracing::debug!(
                        "type_definition: position mapping failed for {}:{}:{}",
                        uri.as_str(),
                        position.line,
                        position.character
                    );
                }
            }
        }

        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let _hg = HandlerGuard::new("references");
        let uri = &params.text_document_position.text_document.uri;
        let _timer = self
            .statistics
            .timer("references", Some(uri.as_str().to_string()));
        let position = &params.text_document_position.position;
        let include_declaration = params.context.include_declaration;
        tracing::debug!(
            "references: {} at {}:{} (include_decl={})",
            uri.as_str(),
            position.line,
            position.character,
            include_declaration
        );

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_refs) = tp.get_references(&tsx_path, offset).await {
                        let locations: Vec<Location> = type_refs
                            .into_iter()
                            .filter_map(|r| {
                                let vue_source_exists =
                                    |p: &str| self.documents.host().get_source(p).is_some();
                                let target_path =
                                    merge::normalize_vue_path_owned(&r.path, &vue_source_exists);
                                let target_uri: Uri = merge::file_path_to_uri(&target_path)?;
                                let range = if r.path == tsx_path {
                                    Range {
                                        start: vf_li
                                            .offset_to_position(r.start)
                                            .unwrap_or_default(),
                                        end: vf_li.offset_to_position(r.end).unwrap_or_default(),
                                    }
                                } else {
                                    Range::default()
                                };
                                Some(Location {
                                    uri: target_uri,
                                    range,
                                })
                            })
                            .collect();
                        return Ok(if locations.is_empty() {
                            None
                        } else {
                            Some(locations)
                        });
                    }
                }
                return Ok(None);
            }
        }

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let mut locations = references_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                include_declaration,
            )?;

            // Fix up sentinel URIs
            for loc in &mut locations {
                if loc.uri.as_str() == crate::features::references::SAME_FILE_URI_STR {
                    loc.uri = uri.clone();
                }
            }

            Some(locations)
        })();

        tracing::debug!(
            "references: verter found {}",
            verter_result.as_ref().map_or(0, |v| v.len())
        );

        // Enhance with TypeProvider if available.
        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    tracing::debug!(
                        "references: querying type provider at tsx offset {}",
                        tsx_offset
                    );
                    match tp.get_references(&ctx.tsx_path, tsx_offset).await {
                        Ok(type_refs) => {
                            tracing::debug!(
                                "references: type provider returned {} locations",
                                type_refs.len()
                            );
                            let vue_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            return Ok(merge::merge_references(
                                verter_result,
                                type_refs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                &vue_source_exists,
                            ));
                        }
                        Err(e) => {
                            tracing::warn!("references: type provider error: {e}");
                        }
                    }
                } else {
                    tracing::debug!(
                        "references: position mapping failed for {}:{}:{}",
                        uri.as_str(),
                        position.line,
                        position.character
                    );
                }
            }
        }

        Ok(verter_result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let _hg = HandlerGuard::new("prepare_rename");
        let uri = &params.text_document.uri;
        let position = &params.position;

        // Virtual file: not supported (no Verter rename context for generated code)
        if self.documents.get_virtual_source_uri(uri).is_some() {
            return Ok(None);
        }

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let range = prepare_rename(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )?;
            Some(PrepareRenameResponse::Range(range))
        })();

        Ok(result)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let _hg = HandlerGuard::new("rename");
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;
        let new_name = &params.new_name;

        // Virtual file: not supported (renaming in generated code isn't meaningful)
        if self.documents.get_virtual_source_uri(uri).is_some() {
            return Ok(None);
        }

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let mut edit = rename_at_position(
                position,
                new_name,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )?;

            // Fix up sentinel URIs in workspace edit
            if let Some(ref mut changes) = edit.changes {
                let sentinel = crate::features::rename::SAME_FILE_URI.clone();
                if let Some(edits) = changes.remove(&sentinel) {
                    changes.insert(uri.clone(), edits);
                }
            }

            Some(edit)
        })();

        // Enhance with TypeProvider for cross-file renames.
        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    if let Ok(type_locs) = tp.get_rename_locations(&ctx.tsx_path, tsx_offset).await
                    {
                        let vue_source_exists =
                            |p: &str| self.documents.host().get_source(p).is_some();
                        return Ok(merge::merge_rename_locations(
                            verter_result,
                            type_locs,
                            new_name,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                            Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                            &vue_source_exists,
                        ));
                    }
                }
            }
        }

        Ok(verter_result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let _hg = HandlerGuard::new("document_symbol");
        let uri = &params.text_document.uri;

        let symbols = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let symbols = build_document_symbols(&blocks, analysis.as_ref(), &doc.line_index);
            if symbols.is_empty() {
                None
            } else {
                Some(symbols)
            }
        })();

        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let _hg = HandlerGuard::new("folding_range");
        let uri = &params.text_document.uri;

        let ranges = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let ranges = build_folding_ranges(&blocks, analysis.as_ref(), &doc.line_index);
            if ranges.is_empty() {
                None
            } else {
                Some(ranges)
            }
        })();

        Ok(ranges)
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let _hg = HandlerGuard::new("selection_range");
        let uri = &params.text_document.uri;

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            let line_index = &doc.line_index;
            let source_len = doc.source.len() as u32;

            let file_range = Range {
                start: line_index.offset_to_position(0).unwrap_or_default(),
                end: line_index
                    .offset_to_position(source_len)
                    .unwrap_or_default(),
            };

            let ranges: Vec<_> = params
                .positions
                .iter()
                .map(|pos| {
                    let offset = line_index.position_to_offset(pos).unwrap_or(0) as usize;

                    // Find the containing block
                    let block = blocks.iter().find(|b| {
                        let (cs, ce) = b.content_range();
                        offset >= cs as usize && offset <= ce as usize
                    });

                    if let Some(block) = block {
                        let (cs, ce) = block.content_range();
                        let content_range = Range {
                            start: line_index.offset_to_position(cs).unwrap_or_default(),
                            end: line_index.offset_to_position(ce).unwrap_or_default(),
                        };
                        let block_range = Range {
                            start: line_index
                                .offset_to_position(block.open_tag_start)
                                .unwrap_or_default(),
                            end: line_index
                                .offset_to_position(block.close_tag_end)
                                .unwrap_or_default(),
                        };

                        SelectionRange {
                            range: content_range,
                            parent: Some(Box::new(SelectionRange {
                                range: block_range,
                                parent: Some(Box::new(SelectionRange {
                                    range: file_range,
                                    parent: None,
                                })),
                            })),
                        }
                    } else {
                        SelectionRange {
                            range: file_range,
                            parent: None,
                        }
                    }
                })
                .collect();

            Some(ranges)
        })();

        Ok(result)
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let _hg = HandlerGuard::new("document_highlight");
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_highlights) = tp.get_document_highlights(&tsx_path, offset).await
                    {
                        let highlights: Vec<DocumentHighlight> = type_highlights
                            .into_iter()
                            .filter_map(|h| {
                                Some(DocumentHighlight {
                                    range: Range {
                                        start: vf_li.offset_to_position(h.start)?,
                                        end: vf_li.offset_to_position(h.end)?,
                                    },
                                    kind: Some(match h.kind {
                                        crate::tsgo::protocol::TypeDocumentHighlightKind::Read => {
                                            DocumentHighlightKind::READ
                                        }
                                        crate::tsgo::protocol::TypeDocumentHighlightKind::Write => {
                                            DocumentHighlightKind::WRITE
                                        }
                                        _ => DocumentHighlightKind::TEXT,
                                    }),
                                })
                            })
                            .collect();
                        return Ok(if highlights.is_empty() {
                            None
                        } else {
                            Some(highlights)
                        });
                    }
                }
                return Ok(None);
            }
        }

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            highlights_at_position(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )
        })();

        // Enhance with TypeProvider if available
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, tsx_content, mapper)) = self.ide_context(uri) {
                let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                if let Some(doc) = self.documents.get(uri) {
                    if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                        position,
                        &doc.line_index,
                        &mapper,
                        &tsx_li,
                    ) {
                        if let Ok(type_highlights) =
                            tp.get_document_highlights(&tsx_path, tsx_offset).await
                        {
                            return Ok(merge::merge_document_highlights(
                                verter_result,
                                type_highlights,
                                &tsx_li,
                                &mapper,
                                &doc.line_index,
                            ));
                        }
                    }
                }
            }
        }

        Ok(verter_result)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let _hg = HandlerGuard::new("signature_help");
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        // Virtual file: route directly through TSGO
        if let Some(tp) = &self.type_provider {
            if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                if let Some(offset) = vf_li.position_to_offset(position) {
                    if let Ok(type_sig) = tp.get_signature_help(&tsx_path, offset).await {
                        return Ok(merge::merge_signature_help(type_sig));
                    }
                }
                return Ok(None);
            }
        }

        // Extract all context synchronously — no DashMap guard held across await.
        if let Some(tp) = &self.type_provider {
            if let Some(ctx) = self.type_provider_context(uri) {
                if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                    position,
                    &ctx.vue_line_index,
                    &ctx.mapper,
                    &ctx.tsx_line_index,
                ) {
                    if let Ok(type_sig) = tp.get_signature_help(&ctx.tsx_path, tsx_offset).await {
                        return Ok(merge::merge_signature_help(type_sig));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let _hg = HandlerGuard::new("code_action");
        let uri = &params.text_document.uri;
        let range = &params.range;

        let only = params.context.only.as_deref();

        let mut all_actions: Vec<CodeActionOrCommand> = Vec::new();

        // Verter's own code actions (organize imports)
        if let Some(doc) = self.documents.get(uri) {
            let analysis = self.documents.get_analysis(uri);

            if wants_code_action_kind(only, "source.organizeImports") {
                let mut verter_actions =
                    organize_imports_actions(&doc.source, analysis.as_ref(), &doc.line_index);
                fix_placeholder_uris(&mut verter_actions, uri);
                all_actions.extend(verter_actions);
            }

            // Extract component refactoring
            if wants_code_action_kind(only, "refactor.extract") {
                let blocks = scan_sfc_blocks(&doc.source);
                if let Some(extract_action) =
                    crate::features::extract_component::extract_component_action(
                        &doc.source,
                        range,
                        &blocks,
                        &doc.line_index,
                        uri,
                    )
                {
                    all_actions.push(extract_action);
                }
            }

            if wants_code_action_kind(only, "quickfix") {
                let blocks = scan_sfc_blocks(&doc.source);

                // Macro code actions (defineSlots, defineEmits generation/augmentation)
                let cursor_offset = doc.line_index.position_to_offset(&range.start);
                let mut macro_actions = crate::features::macro_actions::macro_code_actions(
                    &doc.source,
                    analysis.as_ref(),
                    &blocks,
                    &doc.line_index,
                    cursor_offset,
                );
                fix_placeholder_uris(&mut macro_actions, uri);
                all_actions.extend(macro_actions);

                // Component code actions (add unknown props/v-models to child)
                if let Some(ref analysis) = analysis {
                    let comp_actions = crate::features::component_actions::component_code_actions(
                        analysis,
                        &|import_source| self.resolve_component_context(uri, import_source, None),
                    );
                    all_actions.extend(comp_actions);

                    // Suggest matching props from parent bindings to child component tags
                    let suggest_actions =
                        crate::features::component_actions::suggest_matching_props(
                            analysis,
                            &doc.source,
                            &doc.line_index,
                            uri,
                            &|import_source| {
                                self.resolve_component_context(uri, import_source, None)
                            },
                        );
                    all_actions.extend(suggest_actions);

                    // Event handler type hint actions
                    let mut event_actions =
                        crate::features::event_type_hints::event_type_hint_actions(
                            analysis,
                            &doc.source,
                            &doc.line_index,
                        );
                    fix_placeholder_uris(&mut event_actions, uri);
                    all_actions.extend(event_actions);
                }
            }

            let wants_quickfix = wants_code_action_kind(only, "quickfix");
            let wants_refactor = wants_code_action_kind(only, "refactor");

            // Action engine quick fixes and refactorings.
            if wants_quickfix || wants_refactor {
                if let Some(ref analysis) = analysis {
                    let canonical_id = uri_to_canonical_id(uri);
                    let linter = self.linter_for_file(&canonical_id);
                    if wants_quickfix {
                        all_actions.extend(
                            crate::features::diagnostics_bridge::action_engine_fixes(
                                &self.action_engine,
                                analysis,
                                &doc.source,
                                &doc.line_index,
                                &linter,
                                &params.context.diagnostics,
                                uri,
                            ),
                        );
                    }
                    if wants_refactor {
                        if let Some(offset) = doc.line_index.position_to_offset(&range.start) {
                            all_actions.extend(
                                crate::features::diagnostics_bridge::action_engine_refactorings(
                                    &self.action_engine,
                                    analysis,
                                    &doc.source,
                                    &doc.line_index,
                                    &linter,
                                    offset,
                                    uri,
                                ),
                            );
                        }
                    }
                }
            }
        }

        // TypeProvider code actions (TSGO quick fixes, refactorings).
        // Skip during typing cooldown to keep TSGO pipeline clear for interactive requests.
        // Extract all context synchronously — no DashMap guard held across await.
        if !self.is_typing_cooldown()
            && (wants_code_action_kind(only, "quickfix")
                || wants_code_action_kind(only, "refactor"))
        {
            if let Some(tp) = &self.type_provider {
                if let Some(ctx) = self.type_provider_context(uri) {
                    let start_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.start,
                        &ctx.vue_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    );
                    let end_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.end,
                        &ctx.vue_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    );
                    if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                        if let Ok(type_actions) = tp.get_code_actions(&ctx.tsx_path, so, eo).await {
                            let vue_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            let actions = merge::merge_code_actions(
                                type_actions,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                &vue_source_exists,
                            );
                            all_actions.extend(actions);
                        }
                    }
                }
            }
        }

        Ok(if all_actions.is_empty() {
            None
        } else {
            Some(all_actions)
        })
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let _hg = HandlerGuard::new("semantic_tokens");
        let uri = &params.text_document.uri;

        // Skip TSGO while typing — serial TSGO pipeline must stay clear
        // for interactive requests. VS Code re-requests after the typing pause.
        // Extract all context synchronously — no DashMap guard held across await.
        if !self.is_typing_cooldown() {
            if let Some(tp) = &self.type_provider {
                if let Some(ctx) = self.type_provider_context(uri) {
                    if let Ok(type_tokens) = tp.get_semantic_tokens(&ctx.tsx_path).await {
                        let tokens = merge::merge_semantic_tokens(
                            type_tokens,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                        );
                        if !tokens.is_empty() {
                            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                                result_id: None,
                                data: tokens,
                            })));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let _hg = HandlerGuard::new("code_lens");
        let uri = &params.text_document.uri;

        let lenses = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            Some(code_lenses(&blocks, analysis.as_ref(), &doc.line_index))
        })();

        match lenses {
            Some(v) if !v.is_empty() => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let _hg = HandlerGuard::new("inlay_hint");
        let uri = &params.text_document.uri;
        let range = &params.range;

        // Skip TSGO while typing — serial TSGO pipeline must stay clear
        // for interactive requests.
        let typing = self.is_typing_cooldown();

        let inlay_enabled = self
            .inlay_hints_enabled
            .load(std::sync::atomic::Ordering::Relaxed);

        // Virtual file: route directly through type provider (positions already in TSX coordinates)
        if !typing && inlay_enabled {
            if let Some(tp) = &self.type_provider {
                if let Some((tsx_path, vf_li)) = self.virtual_file_context(uri) {
                    let start = vf_li.position_to_offset(&range.start);
                    let end = vf_li.position_to_offset(&range.end);
                    if let (Some(so), Some(eo)) = (start, end) {
                        if let Ok(type_hints) = tp.get_inlay_hints(&tsx_path, so, eo).await {
                            let hints: Vec<InlayHint> = type_hints
                                .into_iter()
                                .filter_map(|h| {
                                    let pos = vf_li.offset_to_position(h.position)?;
                                    let kind = h.kind.map(|k| match k {
                                        crate::tsgo::protocol::InlayHintKind::Type => {
                                            InlayHintKind::TYPE
                                        }
                                        crate::tsgo::protocol::InlayHintKind::Parameter => {
                                            InlayHintKind::PARAMETER
                                        }
                                    });
                                    Some(InlayHint {
                                        position: pos,
                                        label: InlayHintLabel::String(h.label),
                                        kind,
                                        text_edits: None,
                                        tooltip: None,
                                        padding_left: h.padding_left,
                                        padding_right: h.padding_right,
                                        data: None,
                                    })
                                })
                                .collect();
                            return Ok(if hints.is_empty() { None } else { Some(hints) });
                        }
                    }
                    return Ok(None);
                }
            }
        }

        // Collect Verter-specific hints (DOM queries, useTemplateRef)
        let mut hints: Vec<InlayHint> = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            Some(crate::features::inlay_hints::verter_inlay_hints(
                &doc.source,
                &blocks,
                &analysis,
                &doc.line_index,
            ))
        })()
        .unwrap_or_default();

        // Standard .vue file: merge with type provider hints when available.
        // Extract all context synchronously — no DashMap guard held across await.
        if !typing && inlay_enabled {
            if let Some(tp) = &self.type_provider {
                if let Some(ctx) = self.type_provider_context(uri) {
                    let start_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.start,
                        &ctx.vue_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    );
                    // Tolerant end mapping: fall back to unvalidated, then TSX EOF.
                    // The visible range end often lands in synthetic JSX (generated for
                    // HTML elements), which fails validation. Inlay hints tolerate an
                    // approximate end bound — only the start must be precise.
                    let end_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.end,
                        &ctx.vue_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    )
                    .or_else(|| {
                        merge::vue_position_to_tsx_offset(
                            &range.end,
                            &ctx.vue_line_index,
                            &ctx.mapper,
                            &ctx.tsx_line_index,
                        )
                    })
                    .or_else(|| Some(ctx.tsx_line_index.source_len()));
                    if let (Some(so), Some(eo)) = (start_offset, end_offset) {
                        match tp.get_inlay_hints(&ctx.tsx_path, so, eo).await {
                            Ok(type_hints) => {
                                tracing::debug!(
                                    "inlay_hint: type provider returned {} hints for {}",
                                    type_hints.len(),
                                    uri.as_str()
                                );
                                let mut tsgo_hints = merge::merge_inlay_hints(
                                    type_hints,
                                    &ctx.tsx_line_index,
                                    &ctx.mapper,
                                    &ctx.vue_line_index,
                                );
                                tracing::debug!(
                                    "inlay_hint: {} hints after merge mapping",
                                    tsgo_hints.len()
                                );
                                hints.append(&mut tsgo_hints);
                            }
                            Err(e) => {
                                tracing::debug!(
                                    "inlay_hint: type provider error for {}: {}",
                                    uri.as_str(),
                                    e
                                );
                            }
                        }
                    } else {
                        tracing::debug!(
                            "inlay_hint: start position mapping failed for {}",
                            uri.as_str()
                        );
                    }
                } else {
                    tracing::debug!("inlay_hint: no type_provider_context for {}", uri.as_str());
                }
            }
        } else {
            tracing::debug!("inlay_hint: skipped type provider (typing cooldown or disabled)");
        }

        // Deduplicate hints at the same position (prefer type provider hints over Verter placeholders)
        hints.sort_by_key(|h| (h.position.line, h.position.character));
        hints.dedup_by(|a, b| a.position == b.position && a.kind == b.kind);

        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let _hg = HandlerGuard::new("linked_editing");
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            linked_editing_ranges(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
            )
        })();

        Ok(result)
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let _hg = HandlerGuard::new("document_link");
        let uri = &params.text_document.uri;

        let links = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let links =
                build_document_links(&doc.source, &blocks, analysis.as_ref(), &doc.line_index);
            if links.is_empty() {
                None
            } else {
                Some(links)
            }
        })();

        Ok(links)
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        let _hg = HandlerGuard::new("document_color");
        let uri = &params.text_document.uri;

        let colors = (|| {
            let doc = self.documents.get(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            Some(color_info::document_colors(
                &doc.source,
                &blocks,
                &doc.line_index,
            ))
        })();

        Ok(colors.unwrap_or_default())
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        let _hg = HandlerGuard::new("color_presentation");
        Ok(color_info::color_presentations(&params.color))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let _hg = HandlerGuard::new("formatting");
        let uri = &params.text_document.uri;

        let edits = (|| {
            let doc = self.documents.get(uri)?;
            let blocks = scan_sfc_blocks(&doc.source);
            let edits = format_document(&doc.source, &blocks, &doc.line_index, &params.options);
            if edits.is_empty() {
                None
            } else {
                Some(edits)
            }
        })();

        Ok(edits)
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let _hg = HandlerGuard::new("on_type_formatting");
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;

        let edits = (|| {
            let doc = self.documents.get(uri)?;
            let offset = doc.line_index.position_to_offset(position)? as usize;
            let snippet = crate::features::auto_close_tag::auto_close_tag(&doc.source, offset)?;

            // Insert the closing tag text right at the cursor position (after the `>`)
            // The `$0` cursor marker is for snippet-capable clients; for the TextEdit
            // we just strip it and insert plain text.
            let plain_text = snippet.replace("$0", "");
            Some(vec![TextEdit {
                range: Range::new(*position, *position),
                new_text: plain_text,
            }])
        })();

        Ok(edits)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        let _hg = HandlerGuard::new("workspace_symbol");
        let symbols = workspace_symbols(&self.documents.host, &params.query);
        Ok(if symbols.is_empty() {
            None
        } else {
            Some(symbols.into())
        })
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let _hg = HandlerGuard::new("prepare_call_hierarchy");
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            call_hierarchy::prepare_call_hierarchy(
                position,
                &doc.source,
                &blocks,
                analysis.as_ref(),
                &doc.line_index,
                uri,
            )
        })();

        Ok(result)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let _hg = HandlerGuard::new("incoming_calls");
        let uri = &params.item.uri;

        let calls = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            Some(call_hierarchy::incoming_calls(
                &params.item,
                &doc.source,
                analysis.as_ref(),
                &doc.line_index,
                uri,
            ))
        })();

        match calls {
            Some(v) if !v.is_empty() => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let _hg = HandlerGuard::new("outgoing_calls");
        let uri = &params.item.uri;

        let calls = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            Some(call_hierarchy::outgoing_calls(
                &params.item,
                analysis.as_ref(),
                &doc.line_index,
                uri,
            ))
        })();

        match calls {
            Some(v) if !v.is_empty() => Ok(Some(v)),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod server_tests;
