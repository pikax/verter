use std::sync::Arc;

use dashmap::{DashMap, DashSet};
use serde::{Deserialize, Serialize};
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
use crate::statistics::Statistics;
use crate::tsgo::merge;
use crate::tsgo::project_sync::ProjectSync;
use crate::tsgo::traits::TypeProvider;
use crate::LspConfig;

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

// ── Custom protocol types ──────────────────────────────────────────────

/// Server → client notification: TSGO child process started with given PID.
/// The extension tracks this PID to kill orphaned TSGO processes on restart.
pub enum TsgoStarted {}

impl tower_lsp_server::ls_types::notification::Notification for TsgoStarted {
    type Params = TsgoStartedParams;
    const METHOD: &'static str = "$/verter/tsgoStarted";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TsgoStartedParams {
    pub pid: u32,
}

/// Server → client notification: type provider child process started.
/// Supersedes `TsgoStarted` — includes the provider kind so the extension
/// can track both TSGO and tsserver processes.
pub enum TypeProviderStarted {}

impl tower_lsp_server::ls_types::notification::Notification for TypeProviderStarted {
    type Params = TypeProviderStartedParams;
    const METHOD: &'static str = "$/verter/typeProviderStarted";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeProviderStartedParams {
    pub pid: u32,
    pub kind: String,
}

/// Server → client heartbeat notification.
/// Sent every 5 seconds to let the extension detect if the server is frozen.
/// If the extension doesn't receive a heartbeat for 30 seconds, it restarts the server.
pub enum Heartbeat {}

impl tower_lsp_server::ls_types::notification::Notification for Heartbeat {
    type Params = HeartbeatParams;
    const METHOD: &'static str = "$/verter/heartbeat";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartbeatParams {
    pub timestamp: u64,
}

/// Server → client notification: background initialization complete.
/// Sent after project registry, workspace scanner, and type provider are ready.
/// The extension uses this to re-request diagnostics for open docs.
pub enum VerterReady {}

impl tower_lsp_server::ls_types::notification::Notification for VerterReady {
    type Params = VerterReadyParams;
    const METHOD: &'static str = "$/verter/ready";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerterReadyParams {
    pub gen: u64,
}

/// Params for `$/onDidChangeTsOrJsFile` notification.
#[derive(Debug, Deserialize)]
pub struct OnDidChangeTsOrJsFileParams {
    pub uri: String,
    pub changes: Vec<TextChangeEvent>,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeEvent {
    pub text: String,
    pub range: TextChangeRange,
}

#[derive(Debug, Deserialize)]
pub struct TextChangeRange {
    pub start: TextChangePosition,
    pub end: TextChangePosition,
}

#[derive(Debug, Deserialize)]
pub struct TextChangePosition {
    pub line: u32,
    pub character: u32,
}

/// Params for `$/onFileChanged` notification.
#[derive(Debug, Deserialize)]
pub struct OnFileChangedParams {
    pub uri: String,
    #[serde(rename = "type")]
    pub change_type: String,
}

/// Params for `$/getCompiledCode` request.
#[derive(Debug, Deserialize)]
pub struct GetCompiledCodeParams {
    pub uri: String,
}

/// Response for `$/getCompiledCode` request.
#[derive(Debug, Serialize)]
pub struct CompiledCodeResponse {
    pub js: CompiledBlock,
    pub css: CompiledBlock,
    pub wasm: CompiledBlock,
}

#[derive(Debug, Serialize)]
pub struct CompiledBlock {
    pub code: String,
    pub map: Option<String>,
}

/// Params for `$/verter/getVirtualFiles` request.
#[derive(Debug, Deserialize)]
pub struct GetVirtualFilesParams {
    pub uri: String,
}

/// Params for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStyleOverridesParams {
    pub uri: String,
    pub overrides: Vec<StyleOverrideParam>,
}

/// A single style override entry from the client.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleOverrideParam {
    pub index: u32,
    pub code: String,
    pub source_map: Option<String>,
}

/// Response for `$/verter/applyStyleOverrides` request.
#[derive(Debug, Serialize)]
pub struct ApplyStyleOverridesResponse {
    pub success: bool,
}

/// Params for `$/verter/getAnalysis` (and `$/verter/getBindingTypes`) request.
#[derive(Debug, Deserialize)]
pub struct GetAnalysisParams {
    pub uri: String,
}

/// Params for `$/verter/getComponentParents` request.
#[derive(Debug, Deserialize)]
pub struct GetComponentParentsParams {
    pub uri: String,
}

/// A single parent file that uses a component.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentInfo {
    pub file_path: String,
    pub component_name: String,
    pub props: Vec<serde_json::Value>,
    pub slots_used: Vec<String>,
}

/// Response for `$/verter/getComponentParents` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentParentsResponse {
    pub component_path: String,
    pub parents: Vec<ComponentParentInfo>,
}

/// A single virtual file entry in the response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFileEntry {
    pub kind: String,
    pub code: String,
    pub lang: String,
    pub source_map: Option<String>,
    pub stale: bool,
}

/// Generated code block in the virtual files response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeBlock {
    pub code: String,
    pub source_map: Option<String>,
    /// `true` when the SFC script is JavaScript (JSX/JSDoc) rather than TypeScript (TSX).
    pub is_js: bool,
}

/// Response for `$/verter/getVirtualFiles` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualFilesResponse {
    pub ide: Option<CodeBlock>,
    pub api: Option<CodeBlock>,
    pub virtual_files: Vec<VirtualFileEntry>,
}

/// Params for `$/verter/documentDropEdit` request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDropEditParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub dropped_uri: String,
}

/// Params for `$/verter/getStatistics` request.
#[derive(Debug, Deserialize)]
pub struct StatisticsRequestParams {
    #[serde(default)]
    pub include_events: bool,
    pub scope: Option<String>,
}

/// Response for `$/verter/getStatistics` request.
#[derive(Debug, Serialize)]
pub struct StatisticsSnapshot {
    pub enabled: bool,
    pub session: StatisticsSession,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatisticsSession {
    pub by_type: serde_json::Map<String, serde_json::Value>,
    pub by_file: serde_json::Map<String, serde_json::Value>,
}

/// Response for `$/verter/getProjectOverview` request.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewResponse {
    pub files: Vec<ProjectOverviewFile>,
    pub component_graph: Vec<ProjectOverviewComponentEdge>,
    pub stats: ProjectOverviewStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewFile {
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewComponentEdge {
    pub file: String,
    pub uses_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverviewStats {
    pub total_vue_files: usize,
    pub total_components: usize,
    pub total_provide_keys: usize,
    pub total_inject_keys: usize,
    pub files_with_scoped_styles: usize,
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

/// The Verter language server implementation.
///
/// Wraps `verter_host` for SFC analysis and optionally a `TypeProvider`
/// (e.g., TSGO) for richer type information.
///
/// ## Lock Ordering (to prevent deadlocks)
///
/// 1. `workspace_roots` (tokio::sync::Mutex — async, never held across sync locks)
/// 2. `project_registry` (parking_lot::RwLock — sync read lock, never nested with fallback_linter)
/// 3. `fallback_linter` (parking_lot::RwLock — only acquired after project_registry is released)
///
/// Rule: Never acquire `fallback_linter` while holding `project_registry`.
/// Pattern: check project_registry → drop guard → acquire fallback_linter if needed.
pub struct VerterLanguageServer {
    client: Client,
    documents: DocumentRegistry,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_sync: Option<ProjectSync>,
    workspace_roots: tokio::sync::Mutex<Vec<String>>,
    statistics: Arc<Statistics>,
    /// Negotiated position encoding (LSP 3.17). Set during `initialize()`.
    /// Shared with SyncCoordinator so it can compute diagnostics with the correct encoding.
    position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Per-project configuration registry (path aliases, lint config, linters).
    /// Initialized during `initialized()` from workspace roots.
    /// Arc-wrapped so background init can commit the registry without &self.
    project_registry: Arc<parking_lot::RwLock<Option<crate::config::ProjectRegistry>>>,
    /// Fallback linter for files outside any project. Uses default config.
    /// Arc-wrapped so background init can update it without &self.
    fallback_linter: Arc<parking_lot::RwLock<verter_diagnostics::Linter>>,
    /// Action engine — produces quick fixes and refactoring code actions.
    action_engine: verter_actions::ActionEngine,
    /// Lint options from initializationOptions, stored during initialize() for use in initialized().
    init_lint_options: tokio::sync::Mutex<Option<serde_json::Value>>,
    /// Whether vite config alias discovery is enabled (from initializationOptions).
    vite_config_enabled: std::sync::atomic::AtomicBool,
    /// Whether type provider inlay hints are enabled (from initializationOptions).
    inlay_hints_enabled: std::sync::atomic::AtomicBool,
    /// Cached verter diagnostics per document: URI → (version, diagnostics).
    /// Avoids re-running the linter when both push and pull paths request diagnostics
    /// for the same document version.
    cached_verter_diags: DashMap<String, (i32, Vec<Diagnostic>)>,
    /// Set of TSX paths (e.g., "C:/project/src/Foo.vue.tsx") that were synced to the
    /// type provider as background files during workspace scan.  When `did_open()` is
    /// called for one of these files, we use `sync_tsx()` (update) instead of
    /// `open_tsx()` to avoid "already open" errors.  When `did_close()` fires, we keep
    /// the file alive in the provider instead of closing it.
    background_synced_files: Arc<DashMap<String, ()>>,
    /// Which type provider backend is active (TSGO, tsserver, or none).
    type_provider_kind: crate::TypeProviderKind,
    /// When `true`, show a recommendation to switch to TSGO in VS Code settings.
    suggest_tsgo: bool,
    /// Generation counter for completion coalescing. During rapid typing, each keystroke
    /// triggers a completion request. By incrementing this counter, stale requests can
    /// detect they've been superseded and skip the expensive type provider call.
    completion_generation: std::sync::atomic::AtomicU64,
    /// Canonical IDs of files needing provider sync (set in did_change, cleared after sync).
    /// Prevents flooding the type provider with updates during rapid typing.
    needs_provider_sync: Arc<DashSet<String>>,
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
}

impl VerterLanguageServer {
    pub fn new(client: Client, config: LspConfig) -> Self {
        let project_sync = config
            .type_provider
            .as_ref()
            .map(|tp| ProjectSync::new(Arc::clone(tp), config.project_sync_mode));

        let needs_provider_sync = Arc::new(DashSet::new());
        let host = Arc::clone(&config.host);
        let documents = DocumentRegistry::new(config.host);
        let position_encoding = Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16));

        // Create SyncCoordinator if a type provider is connected.
        // The coordinator's debounced loop replaces the old spawn-per-keystroke pattern.
        let sync_coordinator = project_sync.as_ref().map(|ps| {
            crate::sync_coordinator::spawn_sync_coordinator(
                crate::sync_coordinator::SyncCoordinatorDeps {
                    host: Arc::clone(&host),
                    project_sync: ps.clone(),
                    needs_provider_sync: Arc::clone(&needs_provider_sync),
                    tsx_profile: parking_lot::RwLock::new(documents.tsx_profile.read().clone()),
                    client: client.clone(),
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
            project_registry: Arc::new(parking_lot::RwLock::new(None)),
            fallback_linter: Arc::new(parking_lot::RwLock::new(
                verter_diagnostics::Linter::default(),
            )),
            action_engine: verter_actions::ActionEngine::default(),
            init_lint_options: tokio::sync::Mutex::new(None),
            vite_config_enabled: std::sync::atomic::AtomicBool::new(true),
            inlay_hints_enabled: std::sync::atomic::AtomicBool::new(true),
            cached_verter_diags: DashMap::new(),
            background_synced_files: Arc::new(DashMap::new()),
            type_provider_kind: config.type_provider_kind,
            suggest_tsgo: config.suggest_tsgo,
            completion_generation: std::sync::atomic::AtomicU64::new(0),
            needs_provider_sync,
            sync_coordinator,
            last_change_ms: std::sync::atomic::AtomicU64::new(0),
            did_change_mutex: tokio::sync::Mutex::new(()),
            workspace_scanner: Arc::new(tokio::sync::Mutex::new(None)),
            init_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
    /// Caches results per document version to avoid redundant re-computation when both
    /// push (didChange) and pull (textDocument/diagnostic) paths request diagnostics.
    fn compute_verter_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        // Check cache: if version matches, return cached diagnostics.
        let uri_str = uri.as_str();
        if let Some(doc) = self.documents.get(uri) {
            if let Some(cached) = self.cached_verter_diags.get(uri_str) {
                if cached.0 == doc.version {
                    return cached.1.clone();
                }
            }
        }

        let mut diags = if let Some(doc) = self.documents.get(uri) {
            let host_diags = self.documents.get_diagnostics(uri);
            match host_diags {
                Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
                None => vec![],
            }
        } else {
            vec![]
        };

        // Run the diagnostics engine (lint rules: CSS, template, a11y, etc.)
        if let Some(doc) = self.documents.get(uri) {
            if let Some(analysis) = self.documents.get_analysis(uri) {
                let canonical_id = uri_to_canonical_id(uri);

                // Look up per-project lint config (determines which linter + suppress behavior)
                let lint_explicitly_configured = {
                    let registry_guard = self.project_registry.read();
                    registry_guard
                        .as_ref()
                        .and_then(|r| r.linter_for(&canonical_id))
                        .map(|p| p.lint_explicitly_configured)
                        .unwrap_or(false)
                };

                // Run lint rules using per-project linter.
                // Lock ordering: project_registry → release → fallback_linter (never nested).
                {
                    let used_project = {
                        let registry_guard = self.project_registry.read();
                        if let Some(project) = registry_guard
                            .as_ref()
                            .and_then(|r| r.linter_for(&canonical_id))
                        {
                            diags.extend(crate::features::diagnostics_bridge::run_linter(
                                &project.linter,
                                &analysis,
                                &doc.source,
                                &doc.line_index,
                            ));
                            true
                        } else {
                            false
                        }
                    }; // registry_guard dropped here

                    if !used_project {
                        let fl = self.fallback_linter.read();
                        diags.extend(crate::features::diagnostics_bridge::run_linter(
                            &fl,
                            &analysis,
                            &doc.source,
                            &doc.line_index,
                        ));
                    }
                }

                // Component usage diagnostics (unknown props, unknown v-models).
                diags.extend(
                    crate::features::component_diagnostics::component_usage_diagnostics(
                        &analysis,
                        &doc.line_index,
                        &|import_source| self.resolve_component(uri, import_source),
                    ),
                );

                // When lint is not explicitly configured, suppress all lint diagnostics.
                if !lint_explicitly_configured {
                    diags.retain(|d| match &d.code {
                        Some(NumberOrString::String(code)) => !code.starts_with("verter/"),
                        _ => true,
                    });
                }
            }
        }

        // Cache the result
        if let Some(doc) = self.documents.get(uri) {
            self.cached_verter_diags
                .insert(uri_str.to_string(), (doc.version, diags.clone()));
        }

        diags
    }

    /// Compute and push verter-only diagnostics for a document URI.
    async fn publish_diagnostics(&self, uri: &Uri) {
        let verter_diags = self.compute_verter_diagnostics(uri);
        self.publish_diagnostics_with(uri, verter_diags).await;
    }

    /// Publish verter-only diagnostics via the push (`publishDiagnostics`) path.
    ///
    /// TSGO type diagnostics are NOT included here — they are served exclusively
    /// through the pull diagnostics handler (`textDocument/diagnostic`). This
    /// avoids duplication: VS Code shows diagnostics from both push and pull, so
    /// including TSGO in both paths would double every TypeScript error.
    async fn publish_diagnostics_with(&self, uri: &Uri, verter_diags: Vec<Diagnostic>) {
        let _timer = self
            .statistics
            .timer("diagnostics", Some(uri.as_str().to_string()));

        tracing::info!(
            "publish_diagnostics ENTER {} ({} diags)",
            uri.as_str(),
            verter_diags.len()
        );

        self.client
            .publish_diagnostics(uri.clone(), verter_diags, None)
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

    async fn sync_ide_to_provider(&self, uri: &Uri) {
        let _timer = self
            .statistics
            .timer("ide_sync", Some(uri.as_str().to_string()));
        if let Some(sync) = &self.project_sync {
            if let Some(ide) = self.documents.get_ide(uri) {
                let ide_path = self.ide_path_for_uri(uri);
                tracing::info!("sync_ide: {} ({} bytes)", ide_path, ide.code.len());
                if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                    tracing::warn!("sync_ide: failed for {ide_path}: {e}");
                } else {
                    tracing::info!("sync_ide: ok for {}", ide_path);
                }
            } else {
                tracing::info!("sync_ide: no IDE output available for {}", uri.as_str());
            }
        }
    }

    /// Sync the public API (.vue.ts) to the type provider for cross-file component resolution.
    async fn sync_api_to_provider(&self, uri: &Uri) {
        if let Some(sync) = &self.project_sync {
            if let Some(dts_path) = self.dts_path_for_uri(uri) {
                let canonical_id = match self.documents.get_canonical_id(uri) {
                    Some(id) => id,
                    None => return,
                };
                if let Some(api) = self.documents.host.get_public_api(&canonical_id) {
                    if let Err(e) = sync.sync_dts(&dts_path, &api.code).await {
                        tracing::warn!("sync_api: failed for {dts_path}: {e}");
                    }
                }
            }
        }
    }

    /// If the file has pending changes, sync the IDE TSX + API DTS to the type provider NOW.
    /// Called by interactive handlers (hover, completion, etc.) to ensure the provider is up-to-date
    /// before making a query. Uses a tight timeout to avoid blocking interactive requests.
    async fn ensure_provider_synced(&self, uri: &Uri) {
        if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
            if self.needs_provider_sync.remove(&canonical_id).is_some() {
                tracing::info!(
                    "ensure_provider_synced: flushing pending sync for {}",
                    uri.as_str()
                );
                // Use a tight timeout — if the provider is overwhelmed, don't block
                // the interactive request. The debounced task will retry later.
                if tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    tokio::join!(
                        self.sync_ide_to_provider(uri),
                        self.sync_api_to_provider(uri),
                    );
                })
                .await
                .is_err()
                {
                    tracing::warn!(
                        "ensure_provider_synced: sync timed out for {}, proceeding with stale data",
                        uri.as_str()
                    );
                    // Re-insert so a future request or debounced task can retry
                    self.needs_provider_sync.insert(canonical_id);
                }
            }
        }
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
        let ide = self.documents.get_ide(uri);
        if ide.is_none() {
            tracing::info!(
                "ide_context: no IDE output for {} (canonical={})",
                uri.as_str(),
                canonical_id.as_deref().unwrap_or("?")
            );
            return None;
        }
        let ide = ide.unwrap();
        let mapper = self.documents.get_position_mapper(uri);
        if mapper.is_none() {
            tracing::info!("ide_context: no position mapper for {}", uri.as_str());
            return None;
        }
        let ide_path = self.ide_path_for_uri(uri);
        Some((ide_path, ide.code, mapper.unwrap()))
    }

    /// Generate the IDE file path (.tsx or .jsx) for a given Vue file URI.
    fn ide_path_for_uri(&self, uri: &Uri) -> String {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        let ext = if self.documents.is_jsx(uri) {
            ".jsx"
        } else {
            ".tsx"
        };
        format!("{canonical}{ext}")
    }

    /// Generate the DTS declaration file path (.vue.ts) for a given Vue file URI.
    /// Uses TypeScript 5.0 `allowArbitraryExtensions` naming convention:
    /// `import('./Comp.vue')` resolves to `./Comp.vue.ts`
    fn dts_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical = self.documents.get_canonical_id(uri)?;
        let base = canonical.strip_suffix(".vue")?;
        Some(format!("{base}.vue.ts"))
    }

    /// Get IDE content and mapper by IDE path (reverse lookup).
    fn ide_context_by_path(&self, ide_path: &str) -> Option<(String, Arc<str>, PositionMapper)> {
        // IDE path is "{canonical_id}.tsx" or "{canonical_id}.jsx"
        let canonical_id = ide_path
            .strip_suffix(".tsx")
            .or_else(|| ide_path.strip_suffix(".jsx"))?;
        let uri = self.documents.canonical_id_to_uri(canonical_id)?;
        self.ide_context(&uri)
    }

    /// Get external IDE context for a `.vue.tsx` path (used by merge functions for
    /// cross-file position mapping, e.g., CTRL+CLICK on component navigates to target file).
    /// Resolve a component prop attribute to the child component's defineProps field.
    ///
    /// When the cursor is on `foo` in `<MyComp foo="literal">`, this finds:
    /// 1. Which component the prop belongs to (via template analysis)
    /// 2. The child component's analysis (via documents registry)
    /// 3. The matching prop field in the child's defineProps macro
    /// 4. Returns a cross-file definition pointing to the prop's span
    fn resolve_component_prop_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> Option<GotoDefinitionResponse> {
        let doc = self.documents.get(uri)?;
        let analysis = self.documents.get_analysis(uri)?;
        let template = analysis.template.as_ref()?;
        let offset = doc.line_index.position_to_offset(position)?;

        // Find which component prop the cursor is on
        for comp in &template.components {
            for prop in &comp.props {
                if offset >= prop.span.start && offset < prop.span.end {
                    // Cursor is inside this prop's span — resolve to child component
                    let import_source = comp.import_source.as_ref()?;

                    // Resolve import source to canonical ID
                    let canonical_id = uri_to_canonical_id(uri);
                    let child_canonical_id = {
                        let registry_guard = self.project_registry.read();
                        registry_guard
                            .as_ref()
                            .and_then(|reg| reg.resolve_alias(&canonical_id, import_source))
                    }
                    .or_else(|| {
                        if import_source.starts_with('.') {
                            let resolved =
                                verter_host::resolve_external(&canonical_id, import_source);
                            if std::path::Path::new(&resolved).exists() {
                                Some(resolved)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })?;

                    // Get child component's analysis directly from host (works for
                    // background-compiled files, not just open documents)
                    let child_analysis = self.documents.host().get_analysis(&child_canonical_id)?;
                    let child_source = self.documents.host().get_source(&child_canonical_id)?;
                    let child_line_index = LineIndex::new(&child_source, self.documents.encoding());

                    // Build the child file URI
                    let child_uri = crate::features::definition::resolved_import_definition(
                        &child_canonical_id,
                    )
                    .and_then(|resp| match resp {
                        GotoDefinitionResponse::Scalar(loc) => Some(loc.uri),
                        _ => None,
                    })?;

                    // Find matching prop field in child's defineProps
                    for mac in &child_analysis.macros {
                        if let Some(pf) = mac.prop_fields.iter().find(|pf| pf.name == prop.name) {
                            if pf.span.start > 0 || pf.span.end > 0 {
                                let start_pos = child_line_index
                                    .offset_to_position(pf.span.start)
                                    .unwrap_or_default();
                                let end_pos = child_line_index
                                    .offset_to_position(pf.span.end)
                                    .unwrap_or_default();
                                return Some(GotoDefinitionResponse::Scalar(Location {
                                    uri: child_uri,
                                    range: Range {
                                        start: start_pos,
                                        end: end_pos,
                                    },
                                }));
                            }
                        }
                    }

                    // Prop not found in child defineProps — fall back to navigating to child file
                    return Some(GotoDefinitionResponse::Scalar(Location {
                        uri: child_uri,
                        range: Range::default(),
                    }));
                }
            }
        }

        None
    }

    fn external_ide_context(&self, ide_path: &str) -> Option<merge::ExternalIdeContext> {
        let (_tsx_path, tsx_content, mapper) = self.ide_context_by_path(ide_path)?;
        let tsx_line_index = LineIndex::new(&tsx_content, self.documents.encoding());
        // Get the Vue file's line index
        let canonical_id = ide_path
            .strip_suffix(".tsx")
            .or_else(|| ide_path.strip_suffix(".jsx"))?;
        let uri = self.documents.canonical_id_to_uri(canonical_id)?;
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
        let canonical_id = ide_path
            .strip_suffix(".tsx")
            .or_else(|| ide_path.strip_suffix(".jsx"))?;
        self.documents.canonical_id_to_uri(canonical_id)
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

        // Try 1: Relative import
        if import_source.starts_with('.') {
            let parts: Vec<&str> = canonical_id.split('/').collect();
            let dir = parts[..parts.len().saturating_sub(1)].join("/");
            let resolved = resolve_import_path(&dir, import_source);
            if let Some(a) = self.documents.host().get_analysis(&resolved) {
                return Some(a);
            }
        }

        // Try 2: Path alias resolution (per-project)
        {
            let registry_guard = self.project_registry.read();
            if let Some(ref registry) = *registry_guard {
                if let Some(resolved_path) = registry.resolve_alias(&canonical_id, import_source) {
                    if let Some(a) = self.documents.host().get_analysis(&resolved_path) {
                        return Some(a);
                    }
                }
            }
        }

        // Try 3: Direct lookup
        self.documents.host().get_analysis(import_source)
    }

    /// Resolve a child component with full context for cross-file editing.
    fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let analysis = self.resolve_component(parent_uri, import_source)?;
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Resolve the child's canonical ID
        let child_canonical_id = if import_source.starts_with('.') {
            let parts: Vec<&str> = canonical_id.split('/').collect();
            let dir = parts[..parts.len().saturating_sub(1)].join("/");
            resolve_import_path(&dir, import_source)
        } else {
            let registry_guard = self.project_registry.read();
            if let Some(ref registry) = *registry_guard {
                registry
                    .resolve_alias(&canonical_id, import_source)
                    .unwrap_or_else(|| import_source.to_string())
            } else {
                import_source.to_string()
            }
        };

        // Get the child's source
        let child_source_arc = self.documents.host().get_source(&child_canonical_id)?;
        let child_source = child_source_arc.to_string();
        let child_uri: Uri = format!("file:///{}", child_canonical_id).parse().ok()?;
        let blocks = scan_sfc_blocks(&child_source);
        let line_index = LineIndex::new(&child_source, self.documents.encoding());

        Some(crate::features::cross_file::ChildComponentContext {
            uri: child_uri,
            source: child_source,
            analysis,
            blocks,
            line_index,
        })
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
        let tsx_path = self.ide_path_for_uri(&source_uri);

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

        // For non-Vue files tracked by the extension (TS/JS), we notify the
        // type provider so it can update its view of the project.
        if let Some(tp) = &self.type_provider {
            // Reconstruct the full text from the last change (full sync).
            if let Some(last) = params.changes.last() {
                // Convert file:// URI to filesystem path — update_file() calls
                // path_to_uri() internally, so passing a URI would double-wrap it
                // (e.g., file:///file:///...).
                let path = if let Ok(uri) = params.uri.parse::<Uri>() {
                    uri_to_canonical_id(&uri)
                } else {
                    params.uri.clone()
                };
                if let Err(e) = tp.update_file(&path, &last.text).await {
                    tracing::warn!("failed to update file in type provider: {e}");
                }
            }
        }
    }

    /// Handle `$/onFileChanged` notification.
    ///
    /// Called when `node_modules` files are created, updated, or deleted.
    pub async fn on_file_changed(&self, params: OnFileChangedParams) {
        tracing::debug!("$/onFileChanged: {} ({})", params.uri, params.change_type);

        // Handle .vue file changes from the file watcher.
        // These are files not open in the editor — re-sync to type provider.
        if params.uri.ends_with(".vue") {
            let canonical_id = if let Ok(uri) = params.uri.parse::<Uri>() {
                uri_to_canonical_id(&uri)
            } else {
                crate::documents::uri_to_canonical_id_from_str(&params.uri)
            };

            match params.change_type.as_str() {
                "create" | "update" => {
                    self.resync_background_vue_file(&canonical_id).await;
                }
                "delete" => {
                    // Close TSX/DTS in the type provider and clean up.
                    if let Some(sync) = &self.project_sync {
                        let is_tsgo =
                            matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
                        if !is_tsgo {
                            let profile = self.documents.tsx_profile.read().clone();
                            if let Some(ide) = self.documents.host.get_ide(&canonical_id, &profile)
                            {
                                let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
                                let tsx_path = format!("{canonical_id}{ext}");
                                let _ = sync.close_tsx(&tsx_path).await;
                                self.background_synced_files.remove(&tsx_path);
                            }
                        }
                        // Close DTS file
                        let base = canonical_id.strip_suffix(".vue").unwrap_or(&canonical_id);
                        let dts_path = format!("{base}.vue.ts");
                        let _ = sync.close_dts(&dts_path).await;
                        self.background_synced_files.remove(&dts_path);
                    }
                    self.documents.host.remove(&canonical_id);
                }
                _ => {}
            }
        }

        // Future: invalidate module resolution caches, trigger re-analysis
    }

    /// Re-read a non-open .vue file from disk, upsert, compile, and sync to TSGO.
    async fn resync_background_vue_file(&self, canonical_id: &str) {
        tracing::info!(
            "resync_background: START {canonical_id} thread={:?}",
            std::thread::current().id()
        );
        // Read file from disk + upsert + compile (all blocking) — wrapped in block_in_place
        // to prevent tokio worker thread exhaustion during background sync.
        let compile_result = tokio::task::block_in_place(|| {
            let source = match std::fs::read_to_string(canonical_id) {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("resync_background: can't read {canonical_id}: {e}");
                    return None;
                }
            };

            // Upsert into host
            let _ = self.documents.host.upsert(verter_host::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: Arc::from(source.as_str()),
                file_kind: verter_host::FileKind::VueSfc,
                aliases: Vec::new(),
            });

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

        // Sync to type provider
        // For TSGO: only sync DTS (has default export for cross-file imports).
        // IDE files (.vue.tsx) are only synced when the file is open in the editor.
        // For tsserver: sync IDE files (TS plugin resolves .vue → .vue.tsx).
        if let Some(sync) = &self.project_sync {
            let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);

            if !is_tsgo {
                // tsserver: sync IDE output
                if let Some(ide) = self.documents.host.get_ide(canonical_id, &profile) {
                    let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
                    let tsx_path = format!("{canonical_id}{ext}");
                    let is_bg = self.background_synced_files.contains_key(&tsx_path);
                    let result = if is_bg {
                        sync.sync_tsx(&tsx_path, &ide.code).await
                    } else {
                        sync.open_tsx(&tsx_path, &ide.code).await
                    };
                    if result.is_ok() {
                        self.background_synced_files.insert(tsx_path, ());
                    } else if let Err(e) = result {
                        tracing::warn!("resync_background: failed to sync {canonical_id}: {e}");
                    }
                }
            }

            // Sync .vue.ts for cross-file component type resolution
            if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                let base = canonical_id.strip_suffix(".vue").unwrap_or(canonical_id);
                let dts_path = format!("{base}.vue.ts");
                let is_bg = self.background_synced_files.contains_key(&dts_path);
                let result = if is_tsgo {
                    // TSGO: open/update DTS so it's in TSGO's virtual FS
                    if is_bg {
                        sync.sync_dts(&dts_path, &api.code).await
                    } else {
                        sync.open_dts(&dts_path, &api.code).await
                    }
                } else {
                    sync.sync_dts(&dts_path, &api.code).await
                };
                if result.is_ok() && is_tsgo {
                    self.background_synced_files.insert(dts_path, ());
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
        let edit = crate::features::document_drop_edit::document_drop_edit(
            &params.dropped_uri,
            &params.position,
            &doc.source,
            &blocks,
            &doc.line_index,
            uri,
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
            self.publish_diagnostics(&parsed_uri).await;
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
                            // Resolve the import source to an absolute path
                            let resolved = if !src.starts_with('.') {
                                // Non-relative: use per-project path alias resolution
                                let registry_guard = self.project_registry.read();
                                let r = registry_guard
                                    .as_ref()
                                    .and_then(|reg| reg.resolve_alias(&normalized_id, src));
                                tracing::info!(
                                    "  [{}] component '{}' import='{}' (non-relative) → resolved={:?}",
                                    normalized_id.rsplit('/').next().unwrap_or("?"), comp.name, src, r
                                );
                                r.unwrap_or_else(|| src.to_string())
                            } else {
                                // Relative: resolve against importer directory
                                let importer_dir = normalized_id
                                    .rfind('/')
                                    .map(|i| &normalized_id[..i])
                                    .unwrap_or("");
                                let r = resolve_import_path(importer_dir, src);
                                tracing::info!(
                                    "  [{}] component '{}' import='{}' (relative) → resolved='{}'",
                                    normalized_id.rsplit('/').next().unwrap_or("?"),
                                    comp.name,
                                    src,
                                    r
                                );
                                r
                            };
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

/// Resolve a relative import path against an importer's directory.
///
/// Handles `./foo.vue`, `../bar/baz.vue`, etc.
/// Does NOT handle alias imports (e.g., `@/components/Foo.vue`).
/// Build the list of workspace components available for auto-import.
///
/// Scans all known .vue files in the host, derives PascalCase names from filenames,
/// and computes relative import paths from the current file.
fn build_workspace_components(
    host: &verter_host::VerterHost,
    current_file_id: &str,
) -> Vec<crate::features::completion::WorkspaceComponent> {
    let files = host.list_files();
    let current_dir = current_file_id
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");

    let mut components = Vec::new();

    for (file_id, kind) in &files {
        // Only .vue files
        if *kind != verter_host::FileKind::VueSfc {
            continue;
        }
        // Skip the current file
        if file_id == current_file_id {
            continue;
        }
        // Skip node_modules
        if file_id.contains("node_modules") {
            continue;
        }

        // Derive component name from filename: `src/components/MyButton.vue` → `MyButton`
        let filename = file_id.rsplit('/').next().unwrap_or(file_id);
        let stem = filename.strip_suffix(".vue").unwrap_or(filename);
        if stem.is_empty() {
            continue;
        }

        // Convert to PascalCase: `my-button` → `MyButton`, `index` stays `Index`
        let component_name = to_pascal_case(stem);

        // Compute relative path from current file to this file
        let import_path = compute_relative_path(current_dir, file_id);

        components.push(crate::features::completion::WorkspaceComponent {
            name: component_name,
            import_path,
        });
    }

    components
}

/// Convert a kebab-case or mixed-case filename stem to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Compute a relative path from `from_dir` to `to_file`.
fn compute_relative_path(from_dir: &str, to_file: &str) -> String {
    let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to_file.split('/').filter(|s| !s.is_empty()).collect();

    // Find common prefix length
    let common = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = from_parts.len() - common;
    let remaining = &to_parts[common..];

    if ups == 0 {
        format!("./{}", remaining.join("/"))
    } else {
        let up_str = "../".repeat(ups);
        format!("{}{}", up_str, remaining.join("/"))
    }
}

/// Check if a resolved import path matches a target file path.
///
/// Handles cases where the import source omits the `.vue` extension:
/// - `./Popup` → matches `./Popup.vue`
/// - `./Popover` → matches `./Popover/index.vue` or `./Popover/Popover.vue`
fn import_resolved_matches_target(resolved: &str, target: &str) -> bool {
    if resolved == target {
        return true;
    }
    // Skip if resolved already has .vue extension — no fuzzy matching needed
    if resolved.ends_with(".vue") {
        return false;
    }
    // Try: resolved + ".vue"
    if target == format!("{resolved}.vue") {
        return true;
    }
    // Try: resolved/index.vue
    if target == format!("{resolved}/index.vue") {
        return true;
    }
    // Try: resolved/Name.vue where Name is the last segment of resolved
    if let Some(last) = resolved.rsplit('/').next() {
        if !last.is_empty() && target == format!("{resolved}/{last}.vue") {
            return true;
        }
    }
    false
}

fn resolve_import_path(importer_dir: &str, import_source: &str) -> String {
    if !import_source.starts_with('.') {
        // Not a relative import — return as-is (alias import)
        return import_source.to_string();
    }

    let mut parts: Vec<&str> = importer_dir.split('/').filter(|s| !s.is_empty()).collect();

    for segment in import_source.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    // Reconstruct: preserve drive letter on Windows (e.g., "C:/...")
    if importer_dir.chars().nth(1) == Some(':') {
        parts.join("/")
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Extract a TypeScript type annotation from a hover markdown string.
///
/// Handles formats like:
/// - "```typescript\nconst x: number\n```"
/// - "(property) x: string"
/// - "let x: Ref<number>"
fn extract_type_from_hover(contents: &str, binding_name: &str) -> Option<String> {
    // Look for pattern: `name: type` or `name = value`
    let patterns = [format!("{binding_name}: "), format!("{binding_name}:")];

    for line in contents.lines() {
        let trimmed = line.trim().trim_start_matches("```typescript").trim();
        for pattern in &patterns {
            if let Some(idx) = trimmed.find(pattern.as_str()) {
                let after = &trimmed[idx + pattern.len()..];
                let type_str = after.trim().trim_end_matches("```").trim();
                if !type_str.is_empty() {
                    return Some(type_str.to_string());
                }
            }
        }
    }

    None
}

// ── Background initialization ───────────────────────────────────────────

/// Spawn the heartbeat task. Sends `$/verter/heartbeat` every 5 seconds.
/// Called first in `initialized()` so the extension always sees heartbeats,
/// even during long background initialization.
fn spawn_heartbeat(client: Client) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let active = ACTIVE_HANDLERS.load(std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                "heartbeat TICK ts={ts} active_handlers={active} thread={:?}",
                std::thread::current().id()
            );
            client
                .send_notification::<Heartbeat>(HeartbeatParams { timestamp: ts })
                .await;
            tracing::info!("heartbeat SENT ts={ts}");
        }
    });
}

/// Arguments for the background initialization task.
/// All fields are owned or Arc-wrapped so the task can run independently.
struct BackgroundInitArgs {
    roots: Vec<String>,
    node_path: Option<String>,
    vite_config_enabled: bool,
    init_lint_opts: Option<serde_json::Value>,
    my_gen: u64,
    client: Client,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_registry: Arc<parking_lot::RwLock<Option<crate::config::ProjectRegistry>>>,
    fallback_linter: Arc<parking_lot::RwLock<verter_diagnostics::Linter>>,
    workspace_scanner:
        Arc<tokio::sync::Mutex<Option<crate::workspace_scanner::WorkspaceScannerHandle>>>,
    init_generation: Arc<std::sync::atomic::AtomicU64>,
    project_sync: Option<ProjectSync>,
    host: Arc<verter_host::VerterHost>,
    background_synced_files: Arc<DashMap<String, ()>>,
    is_tsgo: bool,
    tsx_profile: Arc<parking_lot::RwLock<verter_host::CompileProfile>>,
}

/// Run all blocking initialization work in the background.
///
/// This function is spawned from `initialized()` and performs:
/// 1. Project registry build (blocking: vite config eval, tsconfig discovery)
/// 2. Type provider workspace sync (async)
/// 3. Lint option merging
/// 4. @verter/types materialisation (blocking FS)
/// 5. Workspace scanner spawn
///
/// Generation checks before each irreversible commit ensure stale init tasks
/// (superseded by `did_change_workspace_folders`) are discarded.
async fn background_init(args: BackgroundInitArgs) -> Result<()> {
    let BackgroundInitArgs {
        roots,
        node_path,
        vite_config_enabled,
        init_lint_opts,
        my_gen,
        client,
        type_provider,
        project_registry,
        fallback_linter,
        workspace_scanner,
        init_generation,
        project_sync,
        host,
        background_synced_files,
        is_tsgo,
        tsx_profile,
    } = args;

    // 1. Build project registry (spawn_blocking — blocking I/O: vite eval, tsconfig)
    let roots_for_registry = roots.clone();
    let np = node_path.clone();
    let registry_result = tokio::task::spawn_blocking(move || {
        crate::config::ProjectRegistry::from_workspace_roots(
            &roots_for_registry,
            np.as_deref(),
            vite_config_enabled,
        )
    })
    .await;

    let mut registry = match registry_result {
        Ok(r) => r,
        Err(e) => {
            if e.is_panic() {
                tracing::error!("project registry build panicked: {e}");
                client
                    .show_message(
                        MessageType::WARNING,
                        "Verter: initialization failed (panic in config discovery)",
                    )
                    .await;
            }
            return Err(tower_lsp_server::jsonrpc::Error::internal_error());
        }
    };

    // Log discovered projects
    for project in registry.projects() {
        tracing::info!(
            "project config: root={}, aliases={}, lint_explicit={}",
            project.root,
            !project.path_resolver.is_empty(),
            project.lint_explicitly_configured,
        );
    }

    // 2. Type provider: workspace folder sync + path config (async, non-blocking)
    if let Some(tp) = &type_provider {
        let added: Vec<serde_json::Value> = roots
            .iter()
            .map(|uri| {
                serde_json::json!({
                    "uri": uri,
                    "name": uri.rsplit('/').next().unwrap_or(uri)
                })
            })
            .collect();
        let _ = tp.update_workspace_folders(added, vec![]).await;

        for project in registry.projects() {
            let project_root_path = std::path::PathBuf::from(&project.root);
            let mut discovery = crate::config::TsConfigDiscovery::new();
            discovery.discover(&project_root_path);

            let candidates = [
                discovery.find_config_for(&project_root_path.join("src/dummy.ts")),
                discovery.configs().first(),
            ];
            for candidate in candidates.into_iter().flatten() {
                if let Some((base_url, paths)) =
                    crate::config::TsConfigPathResolver::raw_paths_json(&candidate.config_path)
                {
                    tracing::info!(
                        "configuring tsserver paths for {} (baseUrl: {})",
                        project.root,
                        base_url,
                    );
                    if let Err(e) = tp.configure_paths(&base_url, paths).await {
                        tracing::warn!("failed to configure tsserver paths: {e}");
                    }
                    break;
                }
            }
        }
    }

    // 3. Merge lint options
    if let Some(init_opts) = init_lint_opts {
        let mut resolved = crate::config::ResolvedLintConfig::default();
        crate::config::merge_init_options(&mut resolved, &init_opts);
        if resolved.explicitly_configured {
            *fallback_linter.write() = verter_diagnostics::Linter::new(resolved.config.clone());
            registry.apply_default_lint(&resolved.config);
        }
    }

    // 3b. Propagate conditional_root_narrowing to lint configs
    if tsx_profile.read().conditional_root_narrowing {
        registry.set_conditional_root_narrowing(true);
        fallback_linter
            .write()
            .config_mut()
            .conditional_root_narrowing = true;
    }

    // 4. Generation check → commit registry
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded, discarding registry");
        return Ok(());
    }
    *project_registry.write() = Some(registry);

    // 5. Materialize @verter/types (spawn_blocking — blocking FS)
    let roots_for_types = roots.clone();
    let any_failed =
        tokio::task::spawn_blocking(move || materialize_verter_types(&roots_for_types))
            .await
            .unwrap_or(true);
    if any_failed {
        tsx_profile.write().embed_ambient_types = true;
    }

    // 6. Generation check → spawn workspace scanner
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded before scanner, discarding");
        return Ok(());
    }

    let roots_for_scan = roots.clone();
    let tsconfig_patterns =
        tokio::task::spawn_blocking(move || collect_tsconfig_patterns(&roots_for_scan))
            .await
            .unwrap_or_default();

    let root_paths: Vec<std::path::PathBuf> = roots
        .iter()
        .map(|uri| std::path::PathBuf::from(crate::documents::uri_to_canonical_id_from_str(uri)))
        .collect();

    let scanner = crate::workspace_scanner::spawn_workspace_scanner(
        crate::workspace_scanner::WorkspaceScannerConfig {
            root_paths,
            host: Arc::clone(&host),
            project_sync: project_sync.clone(),
            background_synced_files: Arc::clone(&background_synced_files),
            is_tsgo,
            tsx_profile: tsx_profile.read().clone(),
            tsconfig_patterns,
        },
    );

    {
        let mut guard = workspace_scanner.lock().await;
        if let Some(old) = guard.take() {
            old.stop();
        }
        *guard = Some(scanner);
    }

    // 7. Generation check → notify ready
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        return Ok(());
    }

    // 7a. Request diagnostic refresh — clears stale diagnostics from a previous
    // session (e.g., TSGO errors that persist after switching to tsserver).
    if let Err(e) = client.workspace_diagnostic_refresh().await {
        tracing::debug!("workspace/diagnostic/refresh failed (client may not support it): {e}");
    }

    client
        .send_notification::<VerterReady>(VerterReadyParams { gen: my_gen })
        .await;

    tracing::info!("background init complete (gen={my_gen})");
    Ok(())
}

/// Materialise `@verter/types` in all workspace roots that don't already have it.
/// Returns `true` if any root failed (caller should fall back to embedding ambient types).
fn materialize_verter_types(roots: &[String]) -> bool {
    let mut any_failed = false;
    for root_uri in roots {
        let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
        let root_path = std::path::PathBuf::from(&canonical);
        let types_index = root_path.join("node_modules/@verter/types/index.d.ts");
        if !types_index.exists() {
            let types_dir = root_path.join("node_modules/@verter/types");
            match std::fs::create_dir_all(&types_dir) {
                Ok(()) => {
                    let dts = verter_host::VERTER_TYPES_STANDALONE_DTS;
                    let pkg = r#"{"name":"@verter/types","types":"index.d.ts"}"#;
                    if let Err(e) = std::fs::write(types_dir.join("index.d.ts"), dts) {
                        tracing::warn!("failed to write @verter/types index.d.ts: {e}");
                        any_failed = true;
                    } else if let Err(e) = std::fs::write(types_dir.join("package.json"), pkg) {
                        tracing::warn!("failed to write @verter/types package.json: {e}");
                    } else {
                        tracing::info!(
                            "@verter/types not installed — materialised at {}",
                            types_dir.display()
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to create @verter/types dir: {e} — falling back to embed"
                    );
                    any_failed = true;
                }
            }
        }
    }
    any_failed
}

/// Collect tsconfig patterns for all workspace roots (blocking FS walk).
fn collect_tsconfig_patterns(roots: &[String]) -> Vec<String> {
    let mut patterns = Vec::new();
    for root_uri in roots {
        let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
        let root_path = std::path::PathBuf::from(&canonical);
        let mut ts_discovery = crate::config::TsConfigDiscovery::new();
        ts_discovery.discover(&root_path);
        patterns.extend(ts_discovery.configs().iter().map(|e| e.pattern.clone()));
    }
    patterns
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
            *self.workspace_roots.lock().await = roots;
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
            // Read viteConfig.enabled setting (default: true)
            if let Some(vite_enabled) = opts
                .get("viteConfig")
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool())
            {
                self.vite_config_enabled
                    .store(vite_enabled, std::sync::atomic::Ordering::Relaxed);
                tracing::info!(
                    "vite config alias discovery: {}",
                    if vite_enabled { "enabled" } else { "disabled" }
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
        }

        Ok(InitializeResult {
            capabilities: server_capabilities(&encoding),
            server_info: Some(ServerInfo {
                name: "verter-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
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
                self.client
                    .send_notification::<TsgoStarted>(TsgoStartedParams { pid })
                    .await;
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

        // C. Read inputs, release locks
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }
        let init_lint_opts = self.init_lint_options.lock().await.take();
        let my_gen = self
            .init_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;

        // C2. Early path configuration — discover tsconfig paths and configure
        // the type provider BEFORE any did_open() can fire. This is fast (reads
        // JSON files only, no Node.js eval). Vite aliases are merged later in
        // background_init. Without this, there's a race: did_open() syncs files
        // to tsserver before configure_paths() runs, creating inferred projects
        // without path aliases (causing "Cannot find module '@/...'" errors).
        if let Some(tp) = &self.type_provider {
            let roots_for_paths = roots.clone();
            for root_uri in &roots_for_paths {
                let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
                let root_path = std::path::PathBuf::from(&canonical);
                let mut discovery = crate::config::TsConfigDiscovery::new();
                discovery.discover(&root_path);

                let candidates = [
                    discovery.find_config_for(&root_path.join("src/dummy.ts")),
                    discovery.configs().first(),
                ];
                for candidate in candidates.into_iter().flatten() {
                    if let Some((base_url, paths)) =
                        crate::config::TsConfigPathResolver::raw_paths_json(&candidate.config_path)
                    {
                        tracing::info!(
                            "early path config: baseUrl={} from {}",
                            base_url,
                            candidate.config_path.display(),
                        );
                        let _ = tp.configure_paths(&base_url, paths).await;
                        break;
                    }
                }
            }
        }

        // D. Clone Arcs for background task
        let args = BackgroundInitArgs {
            roots,
            node_path: crate::tsserver::find_node(),
            vite_config_enabled: self
                .vite_config_enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            init_lint_opts,
            my_gen,
            client: self.client.clone(),
            type_provider: self.type_provider.clone(),
            project_registry: Arc::clone(&self.project_registry),
            fallback_linter: Arc::clone(&self.fallback_linter),
            workspace_scanner: Arc::clone(&self.workspace_scanner),
            init_generation: Arc::clone(&self.init_generation),
            project_sync: self.project_sync.clone(),
            host: Arc::clone(&self.documents.host),
            background_synced_files: Arc::clone(&self.background_synced_files),
            is_tsgo: matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo),
            tsx_profile: Arc::clone(&self.documents.tsx_profile),
        };

        // E. Spawn background init (fire-and-forget)
        tokio::spawn(async move {
            if let Err(e) = background_init(args).await {
                tracing::error!("background initialization failed: {e}");
            }
        });
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
        if result.diagnostics.has_errors {
            tracing::debug!(
                "did_open: {} errors for {}",
                result.diagnostics.diagnostics.len(),
                uri.as_str(),
            );
        }
        // Signal the background scanner to prioritize this file's directory
        if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
            if let Some(scanner) = self.workspace_scanner.lock().await.as_ref() {
                scanner.signal_priority(canonical_id);
            }
        }

        tokio::join!(
            self.sync_ide_to_provider(uri),
            self.sync_api_to_provider(uri),
        );
        self.publish_diagnostics(uri).await;
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
        let update_result = tokio::task::block_in_place(|| {
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
                self.needs_provider_sync.insert(canonical_id.clone());
                if let Some(coordinator) = &self.sync_coordinator {
                    coordinator.signal(canonical_id, uri.as_str().to_string());
                }
            }
        }

        tracing::info!("did_change EXIT v{version}");
        // Skip push diagnostics entirely during rapid typing.
        // Pull diagnostics (textDocument/diagnostic) serve cached verter results.
        // The SyncCoordinator handles fresh diagnostics after typing stops (300ms debounce).
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let _hg = HandlerGuard::new("did_close");
        let uri = &params.text_document.uri;
        tracing::info!("did_close: {}", uri.as_str());
        // Virtual files don't have TSX in the provider
        if self.documents.get_virtual_source_uri(uri).is_none() {
            if let Some(sync) = &self.project_sync {
                // Only close in TSGO if this was a Vue SFC with IDE output
                // (only .vue files get the .tsx/.jsx suffix and are synced to TSGO)
                if self.documents.get_ide(uri).is_some() {
                    let tsx_path = self.ide_path_for_uri(uri);
                    let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);

                    if is_tsgo {
                        // TSGO: always close IDE (.vue.tsx) — it was only opened for
                        // internal type checking of this file. DTS stays alive for imports.
                        if let Err(e) = sync.close_tsx(&tsx_path).await {
                            tracing::warn!("did_close: failed to close TSX in provider: {e}");
                        }
                    } else if self.background_synced_files.contains_key(&tsx_path) {
                        // tsserver: keep background-synced TSX alive for cross-file resolution.
                        tracing::debug!(
                            "did_close: keeping background-synced file in provider: {}",
                            tsx_path
                        );
                    } else {
                        // tsserver: close TSX and DTS for non-background files.
                        if let Err(e) = sync.close_tsx(&tsx_path).await {
                            tracing::warn!("did_close: failed to close TSX in provider: {e}");
                        }
                        if let Some(dts_path) = self.dts_path_for_uri(uri) {
                            let _ = sync.close_dts(&dts_path).await;
                        }
                    }
                }
            }
        }
        self.documents.did_close(uri);
        self.cached_verter_diags.remove(uri.as_str());
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

        // Clone roots snapshot and increment generation for background rebuild
        let roots = self.workspace_roots.lock().await.clone();
        if roots.is_empty() {
            return;
        }
        let my_gen = self
            .init_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;

        // Spawn background task for the blocking work (same as background_init)
        let args = BackgroundInitArgs {
            roots,
            node_path: crate::tsserver::find_node(),
            vite_config_enabled: self
                .vite_config_enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            init_lint_opts: None,
            my_gen,
            client: self.client.clone(),
            type_provider: self.type_provider.clone(),
            project_registry: Arc::clone(&self.project_registry),
            fallback_linter: Arc::clone(&self.fallback_linter),
            workspace_scanner: Arc::clone(&self.workspace_scanner),
            init_generation: Arc::clone(&self.init_generation),
            project_sync: self.project_sync.clone(),
            host: Arc::clone(&self.documents.host),
            background_synced_files: Arc::clone(&self.background_synced_files),
            is_tsgo: matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo),
            tsx_profile: Arc::clone(&self.documents.tsx_profile),
        };

        tokio::spawn(async move {
            if let Err(e) = background_init(args).await {
                tracing::error!("background workspace folder rebuild failed: {e}");
            }
        });
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
            // Read and upsert the file so it's indexed without needing to open in editor
            if let Ok(content) =
                std::fs::read_to_string(uri.path().as_str().trim_start_matches('/'))
            {
                let _ = self.documents.host().upsert(verter_host::UpsertRequest {
                    canonical_id: Some(canonical_id.clone()),
                    input_id: file.uri.clone(),
                    source: Arc::from(content.as_str()),
                    file_kind: verter_host::FileKind::VueSfc,
                    aliases: vec![],
                });
            }
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
            // Close TSX and DTS in the type provider
            if let Some(sync) = &self.project_sync {
                let profile = self.documents.tsx_profile.read().clone();
                if let Some(ide) = self.documents.host().get_ide(&canonical_id, &profile) {
                    let ext = if ide.is_jsx { ".jsx" } else { ".tsx" };
                    let tsx_path = format!("{canonical_id}{ext}");
                    let _ = sync.close_tsx(&tsx_path).await;
                    self.background_synced_files.remove(&tsx_path);
                }
                // Close the .vue.ts declaration file
                let base = canonical_id.strip_suffix(".vue").unwrap_or(&canonical_id);
                let dts_path = format!("{base}.vue.ts");
                let _ = sync.close_dts(&dts_path).await;
                self.background_synced_files.remove(&dts_path);
            }
            self.documents.host().remove(&canonical_id);
            self.cached_verter_diags.remove(uri.as_str());
            tracing::debug!("did_delete_files: removed {}", file.uri);
        }
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let _hg = HandlerGuard::new("diagnostic");
        let uri = &params.text_document.uri;
        tracing::info!("diagnostic ENTER {}", uri.as_str());

        let verter_diags = self.compute_verter_diagnostics(uri);

        // Skip TSGO diagnostics while the user is actively typing.
        // TSGO processes requests serially — queuing diagnostics during typing blocks
        // interactive features like completions.  After the debounced sync fires and TSGO
        // processes the update, VS Code will re-request diagnostics with fresh data.
        let diagnostics = if self.is_typing_cooldown() {
            tracing::debug!(
                "diagnostic (pull): skipping TSGO (typing cooldown) for {}",
                uri.as_str()
            );
            verter_diags
        } else if let Some(tp) = &self.type_provider {
            match self.ide_context(uri) {
                Some((tsx_path, tsx_content, mapper)) => {
                    let tsx_li = LineIndex::new(&tsx_content, self.documents.encoding());
                    let vue_li = self.documents.get(uri).map(|d| d.line_index.clone());
                    match (tp.get_diagnostics(&tsx_path).await, vue_li) {
                        (Ok(type_diags), Some(vue_li)) => {
                            tracing::debug!(
                                "diagnostic (pull): type provider returned {} for {}",
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
                                "diagnostic (pull): type provider error for {}: {e}",
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

        tracing::debug!(
            "diagnostic (pull): returning {} for {}",
            diagnostics.len(),
            uri.as_str()
        );

        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: diagnostics,
                },
            }),
        ))
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
            )
        })();
        let vue_kind_label = verter_full.as_ref().and_then(|r| r.vue_kind_label.clone());
        let verter_result = verter_full.map(|r| r.hover);

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
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        tp.get_hover(&ctx.tsx_path, tsx_offset),
                    )
                    .await
                    {
                        Ok(Ok(hover)) => {
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
                        Ok(Err(e)) => {
                            tracing::warn!("hover type provider error: {}", e);
                            None
                        }
                        Err(_) => {
                            tracing::warn!("hover: type provider timed out");
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
                                    if let Ok(Ok(redirect_hover)) = tokio::time::timeout(
                                        std::time::Duration::from_millis(500),
                                        tp.get_hover(&ctx.tsx_path, redirect_tsx),
                                    )
                                    .await
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

        let verter_result = (|| {
            let doc = self.documents.get(uri)?;
            let analysis = self.documents.get_analysis(uri);
            let blocks = scan_sfc_blocks(&doc.source);
            let canonical_id = crate::documents::uri_to_canonical_id(uri);
            let resolve_component =
                |import_source: &str| -> Option<verter_host::FileAnalysisSnapshot> {
                    // Try 1: Relative import → resolve against current file
                    if import_source.starts_with('.') {
                        let parts: Vec<&str> = canonical_id.split('/').collect();
                        let dir = parts[..parts.len().saturating_sub(1)].join("/");
                        let resolved = if let Some(stripped) = import_source.strip_prefix("./") {
                            format!("{}/{}", dir, stripped)
                        } else if import_source.starts_with("../") {
                            // Simple parent resolution
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
                        if let Some(a) = self.documents.host().get_analysis(&resolved) {
                            return Some(a);
                        }
                    }

                    // Try 2: Path alias resolution (per-project)
                    {
                        let registry_guard = self.project_registry.read();
                        if let Some(ref registry) = *registry_guard {
                            if let Some(resolved_path) =
                                registry.resolve_alias(&canonical_id, import_source)
                            {
                                if let Some(a) = self.documents.host().get_analysis(&resolved_path)
                                {
                                    return Some(a);
                                }
                            }
                        }
                    }

                    // Try 3: Direct lookup (bare specifiers, already-resolved)
                    self.documents.host().get_analysis(import_source)
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
            let ctx = self.type_provider_context(uri);
            if ctx.is_none() {
                tracing::debug!("completion: no ide_context for {}", uri.as_str());
            }
            if let Some(ctx) = ctx {
                // Inline sync: if this file hasn't been synced to TSGO yet, send
                // the current TSX now so TSGO has fresh content for completions.
                // This prevents stale completions (e.g., typing `c.` after `let c = 23;`
                // returning global types instead of number methods).
                if let Some(sync) = &self.project_sync {
                    if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
                        if self.needs_provider_sync.remove(&canonical_id).is_some() {
                            tracing::debug!("completion: inline sync for {}", ctx.tsx_path);
                            if let Err(e) = sync.sync_tsx(&ctx.tsx_path, &ctx.tsx_content).await {
                                tracing::warn!("completion: inline sync failed: {e}");
                            }
                        }
                    }
                }

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
                // Detect expression sub-context (Layer 2)
                // to determine whether verter completions should be suppressed.
                // In member access, literal, type position, or property key contexts,
                // only TypeProvider results are relevant.
                let suppress_verter = in_expression_context
                    && tsx_offset
                        .map(|off| {
                            matches!(
                                classify_expression_context_with_trigger(
                                    &ctx.tsx_content,
                                    off as usize,
                                    trigger_character,
                                ),
                                ExpressionContext::MemberAccess
                                    | ExpressionContext::Literal
                                    | ExpressionContext::TypePosition
                                    | ExpressionContext::PropertyKey
                            )
                        })
                        .unwrap_or(false);
                if let Some(tsx_offset) = tsx_offset {
                    // Check if a newer completion request has arrived. If so, skip
                    // the expensive type provider call and return verter-only results.
                    if self
                        .completion_generation
                        .load(std::sync::atomic::Ordering::Relaxed)
                        != completion_gen + 1
                    {
                        tracing::debug!(
                            "completion: skipping stale type provider call (gen {})",
                            completion_gen
                        );
                        return Ok(verter_items.map(|items| {
                            CompletionResponse::List(CompletionList {
                                is_incomplete: true,
                                items,
                            })
                        }));
                    }
                    // Only forward trigger characters that tsserver/TSGO recognize.
                    // Vue-specific triggers (":", "@", " ") are handled by Verter's
                    // native completions and cause tsserver errors if forwarded.
                    let tp_trigger = trigger_character
                        .filter(|t| matches!(*t, "." | "\"" | "'" | "`" | "/" | "<"));
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        tp.get_completions(&ctx.tsx_path, tsx_offset, tp_trigger),
                    )
                    .await
                    {
                        Ok(Ok(type_result)) => {
                            tracing::debug!(
                                "completion: type provider returned {} items (incomplete={})",
                                type_result.items.len(),
                                type_result.is_incomplete
                            );
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
                        Ok(Err(e)) => {
                            tracing::warn!("completion: type provider error: {e}");
                        }
                        Err(_) => {
                            tracing::warn!(
                                "completion: type provider timed out after 500ms, returning verter-only results"
                            );
                            return Ok(verter_items.map(|items| {
                                CompletionResponse::List(CompletionList {
                                    is_incomplete: true,
                                    items,
                                })
                            }));
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
                                let target_path = merge::normalize_vue_path_owned(&d.path);
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
            let registry_guard = self.project_registry.read();
            let resolve_path = {
                let canonical_id = canonical_id.clone();
                let registry = registry_guard.as_ref();
                move |specifier: &str| -> Option<String> {
                    // First try tsconfig/vite path aliases
                    if let Some(reg) = registry {
                        if let Some(resolved) = reg.resolve_alias(&canonical_id, specifier) {
                            return Some(resolved);
                        }
                    }
                    // Then try relative import resolution
                    if specifier.starts_with('.') {
                        let resolved = verter_host::resolve_external(&canonical_id, specifier);
                        // Try the resolved path as-is, then with common extensions
                        let candidates = if std::path::Path::new(&resolved).extension().is_some() {
                            vec![resolved.clone()]
                        } else {
                            vec![
                                format!("{resolved}.ts"),
                                format!("{resolved}.tsx"),
                                format!("{resolved}.js"),
                                format!("{resolved}.vue"),
                                format!("{resolved}/index.ts"),
                                format!("{resolved}/index.js"),
                                format!("{resolved}/index.vue"),
                            ]
                        };
                        for candidate in candidates {
                            if std::path::Path::new(&candidate).exists() {
                                return Some(candidate);
                            }
                        }
                        // For .vue imports with extension, return even if file doesn't
                        // exist (the host may have it compiled)
                        if resolved.ends_with(".vue") {
                            return Some(resolved);
                        }
                    }
                    None
                }
            };
            #[allow(clippy::type_complexity)]
            let resolve_fn: Option<&dyn Fn(&str) -> Option<String>> =
                Some(&resolve_path as &dyn Fn(&str) -> Option<String>);

            let encoding = self.position_encoding.read().clone();
            let host = &self.documents.host;
            let resolve_export =
                |target_canonical_id: &str, binding_name: &str| -> Option<Location> {
                    let (start, end) = host.get_export_span(target_canonical_id, binding_name)?;
                    let target_source = host.get_source(target_canonical_id)?;
                    let target_li = LineIndex::new(&target_source, encoding.clone());
                    let start_pos = target_li.offset_to_position(start)?;
                    let end_pos = target_li.offset_to_position(end)?;
                    let normalized = target_canonical_id.replace('\\', "/");
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
                if loc.uri.as_str() == crate::features::definition::SAME_FILE_URI {
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

        // Native cross-file prop navigation: cursor on a component prop attribute
        // → navigate to the matching prop field in the child's defineProps.
        if verter_result.is_none() {
            if let Some(prop_def) = self.resolve_component_prop_definition(uri, position) {
                return Ok(Some(prop_def));
            }
        }

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
                            return Ok(merge::merge_definitions(
                                verter_result,
                                type_defs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                uri,
                            ));
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
                                let target_path = merge::normalize_vue_path_owned(&r.path);
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
                if loc.uri.as_str() == crate::features::references::SAME_FILE_URI {
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
                            return Ok(merge::merge_references(
                                verter_result,
                                type_refs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
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
                let sentinel: Uri = crate::features::rename::SAME_FILE_URI.parse().unwrap();
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
                        return Ok(merge::merge_rename_locations(
                            verter_result,
                            type_locs,
                            new_name,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                            Some(&|ide_path: &str| self.external_ide_context(ide_path)),
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

        let mut all_actions: Vec<CodeActionOrCommand> = Vec::new();

        // Verter's own code actions (organize imports)
        if let Some(doc) = self.documents.get(uri) {
            let analysis = self.documents.get_analysis(uri);
            let mut verter_actions =
                organize_imports_actions(&doc.source, analysis.as_ref(), &doc.line_index);
            fix_placeholder_uris(&mut verter_actions, uri);
            all_actions.extend(verter_actions);

            // Extract component refactoring
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
                    &|import_source| self.resolve_component_context(uri, import_source),
                );
                all_actions.extend(comp_actions);

                // Suggest matching props from parent bindings to child component tags
                let suggest_actions = crate::features::component_actions::suggest_matching_props(
                    analysis,
                    &doc.source,
                    &doc.line_index,
                    uri,
                    &|import_source| self.resolve_component_context(uri, import_source),
                );
                all_actions.extend(suggest_actions);

                // Event handler type hint actions
                let mut event_actions = crate::features::event_type_hints::event_type_hint_actions(
                    analysis,
                    &doc.source,
                    &doc.line_index,
                );
                fix_placeholder_uris(&mut event_actions, uri);
                all_actions.extend(event_actions);

                // Action engine quick fixes (e.g., remove unused CSS selector).
                // Lock ordering: project_registry → release → fallback_linter (never nested).
                {
                    let canonical_id = uri_to_canonical_id(uri);
                    let used_project = {
                        let registry_guard = self.project_registry.read();
                        if let Some(project) = registry_guard
                            .as_ref()
                            .and_then(|r| r.linter_for(&canonical_id))
                        {
                            all_actions.extend(
                                crate::features::diagnostics_bridge::action_engine_fixes(
                                    &self.action_engine,
                                    analysis,
                                    &doc.source,
                                    &doc.line_index,
                                    &project.linter,
                                    &params.context.diagnostics,
                                    uri,
                                ),
                            );
                            if let Some(offset) = doc.line_index.position_to_offset(&range.start) {
                                all_actions.extend(
                                    crate::features::diagnostics_bridge::action_engine_refactorings(
                                        &self.action_engine,
                                        analysis,
                                        &doc.source,
                                        &doc.line_index,
                                        &project.linter,
                                        offset,
                                        uri,
                                    ),
                                );
                            }
                            true
                        } else {
                            false
                        }
                    }; // registry_guard dropped here

                    if !used_project {
                        let fl = self.fallback_linter.read();
                        all_actions.extend(
                            crate::features::diagnostics_bridge::action_engine_fixes(
                                &self.action_engine,
                                analysis,
                                &doc.source,
                                &doc.line_index,
                                &fl,
                                &params.context.diagnostics,
                                uri,
                            ),
                        );
                        if let Some(offset) = doc.line_index.position_to_offset(&range.start) {
                            all_actions.extend(
                                crate::features::diagnostics_bridge::action_engine_refactorings(
                                    &self.action_engine,
                                    analysis,
                                    &doc.source,
                                    &doc.line_index,
                                    &fl,
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
        // Extract all context synchronously — no DashMap guard held across await.
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
                        let actions = merge::merge_code_actions(
                            type_actions,
                            &ctx.tsx_line_index,
                            &ctx.mapper,
                            &ctx.vue_line_index,
                        );
                        all_actions.extend(actions);
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
                    let end_offset = merge::vue_position_to_tsx_offset_validated(
                        &range.end,
                        &ctx.vue_line_index,
                        &ctx.mapper,
                        &ctx.tsx_line_index,
                    );
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
                            "inlay_hint: position mapping failed — start={:?}, end={:?}",
                            start_offset,
                            end_offset
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

/// Extract a debug snippet around `offset` in `content`, returning `(before_cursor, after_cursor)`.
/// Returns `None` if the offset is out of bounds.
fn debug_snippet(content: &str, offset: usize) -> Option<(String, String)> {
    if offset > content.len() {
        return None;
    }
    // Snap to char boundaries so we never slice inside a multi-byte UTF-8 sequence
    let snippet_start = content.floor_char_boundary(offset.saturating_sub(20));
    let snippet_end = content.ceil_char_boundary((offset + 30).min(content.len()));
    let cursor = content.floor_char_boundary(offset);
    if snippet_end <= snippet_start || cursor < snippet_start || cursor > snippet_end {
        return None;
    }
    let before = &content[snippet_start..cursor];
    let after = &content[cursor..snippet_end];
    Some((before.to_string(), after.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_snippet_ascii() {
        let content = "abcdefghijklmnopqrstuvwxyz0123456789";
        let (before, after) = debug_snippet(content, 10).unwrap();
        assert_eq!(before, "abcdefghij");
        assert_eq!(after.len(), 26); // 10..40 clamped to 10..36 = 26
    }

    #[test]
    fn debug_snippet_multibyte_offset_inside_char() {
        // "否" is 3 bytes in UTF-8 (E5 90 A6). Place offset at byte 1 = middle of '否'.
        let content = "否abc";
        // byte 0..3 = '否', 3 = 'a', 4 = 'b', 5 = 'c'
        // offset 1 is inside '否' — must NOT panic, snaps to char boundary
        let (before, after) = debug_snippet(content, 1).unwrap();
        // Cursor snaps back to byte 0 (start of '否')
        assert!(before.is_empty(), "cursor snapped to start");
        assert!(after.contains('否'), "after contains the full character");
        assert!(after.contains('a'), "after contains subsequent ASCII");
    }

    #[test]
    fn debug_snippet_multibyte_in_snippet_window() {
        // Reproduces the crash scenario: Chinese characters in JSDoc comments
        // with offset landing in the middle of a multi-byte char
        let content = "  /** 是否显示冷返 */\n  cold?: boolean";
        // '是' starts at byte 6, '否' at byte 9 (each CJK char is 3 bytes)
        // offset 8 lands inside '是' — must NOT panic
        let (before, after) = debug_snippet(content, 8).unwrap();
        // Cursor snaps to byte 6 (start of '是')
        assert!(before.ends_with(' '), "before ends at space before CJK");
        assert!(
            after.starts_with('是'),
            "after starts at snapped char boundary"
        );
        assert!(
            !before.contains('\u{FFFD}'),
            "no replacement chars in before"
        );
        assert!(!after.contains('\u{FFFD}'), "no replacement chars in after");
    }

    #[test]
    fn debug_snippet_at_exact_char_boundary() {
        let content = "abc否def";
        // '否' is at bytes 3..6
        let (before, after) = debug_snippet(content, 3).unwrap();
        assert!(before.ends_with('c'));
        assert!(after.starts_with('否'));
    }

    #[test]
    fn debug_snippet_out_of_bounds() {
        let content = "abc";
        assert!(debug_snippet(content, 100).is_none());
    }

    #[test]
    fn debug_snippet_at_end() {
        let content = "abc";
        let result = debug_snippet(content, 3);
        // offset == len is valid (cursor at end)
        assert!(result.is_some());
    }

    #[test]
    fn needs_provider_sync_insert_and_remove() {
        let set = DashSet::new();
        let id = "C:/project/src/App.vue".to_string();
        set.insert(id.clone());
        assert!(set.contains(&id), "should contain the inserted id");
        let removed = set.remove(&id);
        assert!(removed.is_some(), "remove should return Some");
        assert!(!set.contains(&id), "should no longer contain the id");
    }

    #[test]
    fn resolve_import_path_relative() {
        let result = resolve_import_path("C:/project/src/views", "./Foo.vue");
        assert_eq!(result, "C:/project/src/views/Foo.vue");

        let result = resolve_import_path("C:/project/src/views", "../components/Bar.vue");
        assert_eq!(result, "C:/project/src/components/Bar.vue");
    }

    #[test]
    fn resolve_import_path_alias_returns_raw() {
        // Non-relative imports (aliases) are returned as-is — they can't be resolved
        // without a TsConfigPathResolver
        let result = resolve_import_path("C:/project/src/views", "@/components/Foo.vue");
        assert_eq!(
            result, "@/components/Foo.vue",
            "alias import should be returned as-is (unresolvable by resolve_import_path)"
        );
        // This means `resolved == target_normalized` will never match for aliases,
        // causing component parents to always be empty for alias-based imports.
    }

    #[test]
    fn import_resolved_matches_target_exact() {
        assert!(import_resolved_matches_target(
            "C:/project/src/components/Foo.vue",
            "C:/project/src/components/Foo.vue"
        ));
    }

    #[test]
    fn import_resolved_matches_target_missing_vue_ext() {
        // Import `../Popup` resolves to `C:/proj/src/Popup` (no ext)
        // Target is `C:/proj/src/Popup.vue`
        assert!(import_resolved_matches_target(
            "C:/proj/src/Popup",
            "C:/proj/src/Popup.vue"
        ));
    }

    #[test]
    fn import_resolved_matches_target_directory_index() {
        // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
        // Target is `C:/proj/src/Popover/index.vue`
        assert!(import_resolved_matches_target(
            "C:/proj/src/Popover",
            "C:/proj/src/Popover/index.vue"
        ));
    }

    #[test]
    fn import_resolved_matches_target_directory_same_name() {
        // Import `./Popover` resolves to `C:/proj/src/Popover` (directory)
        // Target is `C:/proj/src/Popover/Popover.vue`
        assert!(import_resolved_matches_target(
            "C:/proj/src/Popover",
            "C:/proj/src/Popover/Popover.vue"
        ));
    }

    #[test]
    fn import_resolved_does_not_match_different_component() {
        assert!(!import_resolved_matches_target(
            "C:/proj/src/Popup",
            "C:/proj/src/Dialog.vue"
        ));
        assert!(!import_resolved_matches_target(
            "C:/proj/src/Popup",
            "C:/proj/src/PopupMenu.vue"
        ));
    }
}
