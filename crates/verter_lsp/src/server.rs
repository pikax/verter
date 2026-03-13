use std::{collections::HashSet, sync::Arc};

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
use crate::project_resolver::ProjectResolverReader;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, remove_sync_state, ProviderPathKind,
    ProviderSyncState, ResolverSnapshot,
};
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

/// Server → client notification: MCP HTTP server is ready.
/// Sent during `initialized()` with the actual bound port (may differ from requested
/// when port 0 is used for OS-assigned dynamic ports).
pub enum McpReady {}

impl tower_lsp_server::ls_types::notification::Notification for McpReady {
    type Params = McpReadyParams;
    const METHOD: &'static str = "$/verter/mcpReady";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpReadyParams {
    pub port: u16,
}

/// Server → client notification: a Vite config requires trust for execution.
/// Sent from `background_init()` when static analysis cannot handle a config and
/// the file is not in `trustedFiles`. The extension shows a prompt to the user.
pub enum ViteConfigTrustRequired {}

impl tower_lsp_server::ls_types::notification::Notification for ViteConfigTrustRequired {
    type Params = ViteConfigTrustRequiredParams;
    const METHOD: &'static str = "$/verter/viteConfigTrustRequired";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ViteConfigTrustRequiredParams {
    pub config_path: String,
    pub workspace_root: String,
    pub reason: String,
}

/// Server → client notification: TSGO is active but no project has a tsconfig.
/// Without a `tsconfig.json`, TSGO cannot discover project configuration.
/// The extension should warn the user to add a tsconfig or switch to tsserver.
pub enum TsgoNoTsconfig {}

impl tower_lsp_server::ls_types::notification::Notification for TsgoNoTsconfig {
    type Params = TsgoNoTsconfigParams;
    const METHOD: &'static str = "$/verter/tsgoLimitation";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TsgoNoTsconfigParams {
    pub message: String,
}

/// Server → client notification: type provider status.
/// Sent during `initialized()` to inform the extension which type provider is active
/// (or that none could be started, with a reason).
pub enum TypeProviderStatus {}

impl tower_lsp_server::ls_types::notification::Notification for TypeProviderStatus {
    type Params = TypeProviderStatusParams;
    const METHOD: &'static str = "$/verter/typeProviderStatus";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypeProviderStatusParams {
    /// Which type provider is active: "tsgo", "tsserver", or "none".
    pub kind: String,
    /// Why no type provider is available (only set when kind is "none").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Server → client request: forward a TypeScript query to the extension's
/// in-process `ts.createLanguageService()`. Uses tsserver command format so
/// existing response parsers work unchanged.
pub enum TsQuery {}

impl tower_lsp_server::ls_types::request::Request for TsQuery {
    type Params = TsQueryParams;
    type Result = serde_json::Value;
    const METHOD: &'static str = "$/verter/tsQuery";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TsQueryParams {
    pub command: String,
    pub arguments: serde_json::Value,
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

enum ComponentEventDefinitionResolution {
    NotApplicable,
    NoDefinition,
    Resolved(GotoDefinitionResponse),
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
    documents: Arc<DocumentRegistry>,
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
    /// The current native resolver snapshot used by all provider-path and dependency sync work.
    /// Swapped atomically after background initialization completes.
    resolver_snapshot: Arc<parking_lot::RwLock<Option<ResolverSnapshot>>>,
    /// Fallback linter for files outside any project. Uses default config.
    /// Arc-wrapped so background init can update it without &self.
    fallback_linter: Arc<parking_lot::RwLock<verter_diagnostics::Linter>>,
    /// Action engine — produces quick fixes and refactoring code actions.
    action_engine: verter_actions::ActionEngine,
    /// Lint options from initializationOptions, stored during initialize() for use in initialized().
    init_lint_options: tokio::sync::Mutex<Option<serde_json::Value>>,
    /// Vite config options (enabled, trusted files, node path).
    vite_config_options: tokio::sync::Mutex<crate::vite_config::ViteConfigOptions>,
    /// Whether type provider inlay hints are enabled (from initializationOptions).
    inlay_hints_enabled: std::sync::atomic::AtomicBool,
    /// Cached verter diagnostics per document: URI → (version, diagnostics).
    /// Avoids re-running the linter when both push and pull paths request diagnostics
    /// for the same document version. Arc-wrapped so the SyncCoordinator can read
    /// cached verter diagnostics when publishing merged diagnostics after sync.
    cached_verter_diags: Arc<DashMap<String, (i32, Vec<Diagnostic>)>>,
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
    /// Legacy alias — kept so the coordinator still has a single dirty set to drain.
    /// Points to the same `DashSet` as `needs_deferred_sync`.
    needs_provider_sync: Arc<DashSet<String>>,
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
    /// Tracks unique file URIs that hit "no project found" errors from TSGO.
    /// After 3+ unique files hit this error, sends a `$/verter/tsgoLimitation`
    /// notification suggesting the user check their tsconfig or switch to tsserver.
    no_project_found_uris: DashSet<String>,
    /// Whether the "no project found" limitation notification has already been sent.
    no_project_found_notified: std::sync::atomic::AtomicBool,
    /// Most-recently-used canonical IDs. Updated on did_open, did_change, and
    /// interactive reads (hover, completion, definition). Used for MRU-ordered
    /// snapshot drain — most recently interacted files reconcile first.
    mru_canonical_ids: parking_lot::Mutex<Vec<String>>,
    /// Shared hydration cache: prevents re-hydrating compile blockers when
    /// the file's semantic hash hasn't changed since the last hydration.
    hydration_cache: Arc<crate::compile_blockers::HydrationCache>,
}

impl VerterLanguageServer {
    pub fn new(client: Client, config: LspConfig) -> Self {
        let project_sync = config
            .type_provider
            .as_ref()
            .map(|tp| ProjectSync::new(Arc::clone(tp), config.project_sync_mode));

        let needs_ide_sync = Arc::new(DashSet::new());
        let needs_deferred_sync = Arc::new(DashSet::new());
        // Legacy alias: coordinator drains from needs_deferred_sync
        let needs_provider_sync = Arc::clone(&needs_deferred_sync);
        let documents = Arc::new(DocumentRegistry::new(config.host));
        let position_encoding = Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16));
        let cached_verter_diags = Arc::new(DashMap::new());
        let project_registry = Arc::new(parking_lot::RwLock::new(None));
        let resolver_snapshot = Arc::new(parking_lot::RwLock::new(None));
        let fallback_linter = Arc::new(parking_lot::RwLock::new(
            verter_diagnostics::Linter::default(),
        ));
        let provider_sync_states = Arc::new(DashMap::new());
        let pending_snapshot_provider_sync = Arc::new(DashSet::new());

        // Create SyncCoordinator if a type provider is connected.
        // The coordinator's debounced loop replaces the old spawn-per-keystroke pattern.
        let sync_coordinator = project_sync.as_ref().map(|ps| {
            crate::sync_coordinator::spawn_sync_coordinator(
                crate::sync_coordinator::SyncCoordinatorDeps {
                    documents: Arc::clone(&documents),
                    project_sync: ps.clone(),
                    needs_provider_sync: Arc::clone(&needs_provider_sync),
                    client: client.clone(),
                    type_provider: config.type_provider.clone(),
                    cached_verter_diags: Arc::clone(&cached_verter_diags),
                    position_encoding: Arc::clone(&position_encoding),
                    resolver_snapshot: Arc::clone(&resolver_snapshot),
                    provider_sync_states: Arc::clone(&provider_sync_states),
                    project_registry: Arc::clone(&project_registry),
                    fallback_linter: Arc::clone(&fallback_linter),
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
            project_registry,
            resolver_snapshot,
            fallback_linter,
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
            needs_provider_sync,
            pending_snapshot_provider_sync,
            sync_coordinator,
            last_change_ms: std::sync::atomic::AtomicU64::new(0),
            did_change_mutex: tokio::sync::Mutex::new(()),
            workspace_scanner: Arc::new(tokio::sync::Mutex::new(None)),
            init_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mcp_port: config.mcp_port,
            type_provider_none_reason: config.type_provider_none_reason,
            no_project_found_uris: DashSet::new(),
            no_project_found_notified: std::sync::atomic::AtomicBool::new(false),
            mru_canonical_ids: parking_lot::Mutex::new(Vec::new()),
            hydration_cache: Arc::new(crate::compile_blockers::HydrationCache::new()),
        }
    }

    /// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
    /// Caches results per document version to avoid redundant re-computation when both
    /// push (didChange) and pull (textDocument/diagnostic) paths request diagnostics.
    /// Track a type provider error and detect "no project found" patterns.
    /// After 3+ unique files hit this error, sends a `$/verter/tsgoLimitation`
    /// notification once, suggesting the user check tsconfig or switch to tsserver.
    fn track_type_provider_error(&self, file_path: &str, error_msg: &str) {
        if !error_msg.contains("no project found") {
            return;
        }
        if self
            .no_project_found_notified
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        // Cap tracking at 10 entries to bound memory
        if self.no_project_found_uris.len() < 10 {
            self.no_project_found_uris.insert(file_path.to_string());
        }
        if self.no_project_found_uris.len() >= 3
            && self
                .no_project_found_notified
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
        {
            let client = self.client.clone();
            tokio::spawn(async move {
                client
                    .send_notification::<TsgoNoTsconfig>(TsgoNoTsconfigParams {
                        message: "Multiple .vue files are not covered by any tsconfig.json. \
                                  Check your tsconfig.json \"include\" patterns, or switch to \
                                  tsserver (which handles inferred projects)."
                            .into(),
                    })
                    .await;
            });
        }
    }

    fn compute_verter_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        compute_verter_diagnostics_for(
            &self.documents,
            uri,
            &self.cached_verter_diags,
            &self.project_registry,
            &self.fallback_linter,
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
                self.hydrate_vue_compile_blockers_for_canonical_id(&canonical_id);
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
            self.hydrate_vue_compile_blockers_for_canonical_id(&canonical_id);
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
            if let Some(dts_path) = transition.next.api_path.clone() {
                if let Some(api) = self.documents.host.get_public_api(&canonical_id) {
                    if let Err(e) = sync.sync_dts(&dts_path, &api.code).await {
                        tracing::warn!("sync_api: failed for {dts_path}: {e}");
                    } else {
                        self.commit_provider_sync_state(&canonical_id, transition.next.clone());
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

    async fn sync_non_vue_file_to_provider(
        &self,
        snapshot: &ResolverSnapshot,
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

            let Some(source) = reader.read_text(&canonical_id) else {
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
                Some(&ResolverSnapshot {
                    generation: 0,
                    resolver: resolver.clone(),
                }),
                &reader,
                &canonical_id,
                &source,
                &module_references,
            ) else {
                continue;
            };

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
        if self.resolver_snapshot().is_none() {
            self.pending_snapshot_provider_sync.insert(canonical_id);
            return;
        }
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
            let api = tokio::task::block_in_place(|| host.get_public_api(&canonical_id));
            if let Some(api) = api {
                if let Err(e) = sync.sync_dts(&dts_path, &api.code).await {
                    tracing::warn!("sync_api(background): failed for {dts_path}: {e}");
                } else {
                    commit_sync_transition(&provider_sync_states, &canonical_id, transition.next);
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

        // Hydrate compile blockers
        if let Some(snapshot) = self.resolver_snapshot() {
            // Full hydration with resolver
            let reader =
                crate::compile_blockers::HostFsProjectResolverReader::new(self.documents.host());
            crate::compile_blockers::hydrate_cached(
                &self.hydration_cache,
                self.documents.host(),
                &snapshot.resolver,
                &reader,
                &canonical_id,
                snapshot.generation,
            );
        } else {
            // Pre-snapshot: resolve relative blockers only
            crate::compile_blockers::hydrate_vue_compile_blockers_pre_snapshot(
                self.documents.host(),
                &canonical_id,
            );
        }

        // Recompile + refresh mapper (in case blocker hydration changed TSX)
        self.documents.recompile_and_refresh_mapper(uri);

        let ide = self.documents.get_ide(uri);
        let is_jsx = ide.as_ref().map(|r| r.is_jsx).unwrap_or(false);

        // Determine IDE path — owner-aware or provisional
        let (ide_path, provisional) = if let Some(snapshot) = self.resolver_snapshot() {
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
                let state = if provisional {
                    crate::provider_sync::ProviderSyncState {
                        owner_key: "__provisional__".to_string(),
                        ide_path: Some(ide_path),
                        api_path: None,
                        ..Default::default()
                    }
                } else {
                    let snapshot = self.resolver_snapshot().unwrap();
                    crate::provider_sync::vue_sync_state_for_source(
                        &snapshot.resolver,
                        &canonical_id,
                        is_jsx,
                    )
                    .unwrap_or_else(|| {
                        crate::provider_sync::ProviderSyncState {
                            owner_key: "__provisional__".to_string(),
                            ide_path: Some(ide_path),
                            api_path: None,
                            ..Default::default()
                        }
                    })
                };
                self.commit_provider_sync_state(&canonical_id, state);
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

    /// Legacy wrapper for backward compat — calls `ensure_current_file_synced`.
    async fn ensure_provider_synced(&self, uri: &Uri) {
        self.ensure_current_file_synced(uri).await;
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
        self.hydrate_vue_compile_blockers_for_canonical_id(canonical_id.as_deref()?);
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

    fn resolver_snapshot(&self) -> Option<ResolverSnapshot> {
        self.resolver_snapshot.read().clone()
    }

    fn hydrate_vue_compile_blockers_for_canonical_id(&self, canonical_id: &str) {
        let Some(snapshot) = self.resolver_snapshot() else {
            return;
        };
        let reader =
            crate::compile_blockers::HostFsProjectResolverReader::new(self.documents.host());
        crate::compile_blockers::hydrate_cached(
            &self.hydration_cache,
            self.documents.host(),
            &snapshot.resolver,
            &reader,
            canonical_id,
            snapshot.generation,
        );
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

    /// Generate the IDE file path (.tsx or .jsx) for a given Vue file URI.
    /// Falls back to `provisional_ide_path` when no resolver snapshot is available.
    fn ide_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        if let Some(snapshot) = self.resolver_snapshot() {
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
        let snapshot = self.resolver_snapshot()?;
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
        if let Some(resolved) = self
            .documents
            .host()
            .resolve_import(parent_canonical_id, specifier)
        {
            return Some(resolved);
        }

        {
            let registry_guard = self.project_registry.read();
            if let Some(registry) = registry_guard.as_ref() {
                if let Some(resolved) = registry.resolve_alias(parent_canonical_id, specifier) {
                    return Some(resolved);
                }
            }
        }

        if specifier.starts_with('.') {
            let resolved = verter_host::resolve_external(parent_canonical_id, specifier);
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
            if resolved.ends_with(".vue") {
                return Some(resolved);
            }
        }

        None
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
                    .get_export_span_follow_reexports(&resolved_target, binding, 10)
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

    fn resolve_component_event_definition(
        &self,
        uri: &Uri,
        position: &Position,
    ) -> ComponentEventDefinitionResolution {
        let Some(doc) = self.documents.get(uri) else {
            return ComponentEventDefinitionResolution::NotApplicable;
        };
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return ComponentEventDefinitionResolution::NotApplicable;
        };
        let Some(template) = analysis.template.as_ref() else {
            return ComponentEventDefinitionResolution::NotApplicable;
        };
        let Some(offset) = doc.line_index.position_to_offset(position) else {
            return ComponentEventDefinitionResolution::NotApplicable;
        };

        for element in &template.elements {
            if !element.is_component {
                continue;
            }
            let Some(component) = template.components.iter().find(|component| {
                offset >= component.span.start
                    && offset < component.span.end
                    && (component.name == element.tag
                        || component.name == to_pascal_case(&element.tag))
            }) else {
                continue;
            };

            for directive in &element.directives {
                if directive.name != "on" {
                    continue;
                }
                let Some(arg_span) = directive.arg_span else {
                    continue;
                };
                if offset < arg_span.start || offset >= arg_span.end {
                    continue;
                }

                let Some(event_name) = directive.argument.as_deref() else {
                    return ComponentEventDefinitionResolution::NoDefinition;
                };
                let Some(child) =
                    self.resolve_component_document_for_usage(uri, &analysis, component)
                else {
                    return ComponentEventDefinitionResolution::NoDefinition;
                };
                let locations =
                    self.collect_component_event_definition_locations(&child, event_name);
                return if locations.is_empty() {
                    ComponentEventDefinitionResolution::NoDefinition
                } else {
                    ComponentEventDefinitionResolution::Resolved(goto_response_from_locations(
                        locations,
                    ))
                };
            }
        }

        ComponentEventDefinitionResolution::NotApplicable
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
                    let child = self.resolve_component_document_for_usage(uri, &analysis, comp)?;

                    // Find matching prop field in child's defineProps
                    for mac in child.analysis.macros.iter() {
                        if let Some(prop_field) =
                            mac.prop_fields.iter().find(|field| field.name == prop.name)
                        {
                            if let Some(location) =
                                location_from_span(&child.uri, &child.line_index, prop_field.span)
                            {
                                return Some(GotoDefinitionResponse::Scalar(location));
                            }
                        }
                    }
                    if let Some(child_template) = child.analysis.template.as_ref() {
                        if let Some(prop_definition) = child_template
                            .prop_definitions
                            .iter()
                            .find(|definition| definition.name == prop.name)
                        {
                            if let Some(location) = location_from_span(
                                &child.uri,
                                &child.line_index,
                                prop_definition.span,
                            ) {
                                return Some(GotoDefinitionResponse::Scalar(location));
                            }
                        }
                    }

                    // Prop not found in child defineProps — fall back to navigating to child file
                    return Some(GotoDefinitionResponse::Scalar(Location {
                        uri: child.uri,
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
        let snapshot = self.resolver_snapshot()?;
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
        let snapshot = self.resolver_snapshot()?;
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
        let snapshot = self.resolver_snapshot()?;
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
        let snapshot = self.resolver_snapshot()?;
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
        resolve_component_for(
            self.documents.host(),
            &self.project_registry,
            &canonical_id,
            import_source,
        )
    }

    /// Resolve a child component with full context for cross-file editing.
    fn resolve_component_context(
        &self,
        parent_uri: &Uri,
        import_source: &str,
    ) -> Option<crate::features::cross_file::ChildComponentContext> {
        let canonical_id = uri_to_canonical_id(parent_uri);

        // Resolve the child's canonical ID
        let child_canonical_id = self
            .resolve_import_specifier(&canonical_id, import_source)
            .unwrap_or_else(|| {
                if import_source.starts_with('.') {
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
                }
            });

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
            if !crate::compile_blockers::ensure_source_loaded_into_host(
                self.documents.host(),
                &child_canonical_id,
            ) {
                return None;
            }
            self.hydrate_vue_compile_blockers_for_canonical_id(&child_canonical_id);
            let profile = self.documents.tsx_profile.read().clone();
            let _ = self
                .documents
                .host
                .ensure_compiled(&child_canonical_id, &profile);
        }

        let analysis = self
            .resolve_component(parent_uri, import_source)
            .or_else(|| self.documents.host().get_analysis(&child_canonical_id))?;
        // Get the child's source
        let child_source_arc = self.documents.host().get_source(&child_canonical_id)?;
        let child_source = child_source_arc.to_string();
        let child_uri: Uri = format!("file:///{}", child_canonical_id).parse().ok()?;
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
                let child = self.resolve_component_context(parent_uri, &target.import_source)?;
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
                let child = self.resolve_component_context(parent_uri, &target.import_source)?;
                let public_api = self
                    .documents
                    .host()
                    .get_public_api(&child.canonical_id)
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
                let child = self.resolve_component_context(parent_uri, &target.import_source)?;
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

            if let Some(snapshot) = self.resolver_snapshot() {
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
        let canonical_path = if let Ok(uri) = params.uri.parse::<Uri>() {
            uri_to_canonical_id(&uri)
        } else {
            crate::documents::uri_to_canonical_id_from_str(&params.uri)
        };

        let is_vite_dep = {
            let registry = self.project_registry.read();
            if let Some(reg) = registry.as_ref() {
                reg.projects()
                    .iter()
                    .any(|p| p.vite_config_deps.iter().any(|dep| dep == &canonical_path))
            } else {
                false
            }
        };

        if is_vite_dep {
            tracing::debug!(
                "vite config dependency changed: {} — triggering registry rebuild",
                canonical_path
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
            project_registry: Arc::clone(&self.project_registry),
            resolver_snapshot: Arc::clone(&self.resolver_snapshot),
            fallback_linter: Arc::clone(&self.fallback_linter),
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

    /// Re-read a non-open .vue file from disk, upsert, compile, and sync to TSGO.
    /// Lightweight API sync for imported .vue files during `did_open`.
    ///
    /// Tries to generate and sync the public API (.vue.ts) without disk I/O:
    /// if the host already has the file in memory, `get_public_api` avoids
    /// re-reading from disk. Falls back to `resync_background_vue_file` when
    /// the file hasn't been upserted yet.
    async fn sync_imported_vue_api_lightweight(&self, canonical_id: &str) {
        // Fast path: host already has the file — generate API and sync DTS only.
        if let Some(api) = self.documents.host.get_public_api(canonical_id) {
            if let Some(sync) = &self.project_sync {
                let Some(transition) =
                    self.prepare_vue_provider_sync_transition(canonical_id, false)
                else {
                    self.queue_snapshot_provider_sync(canonical_id.to_string());
                    return;
                };
                self.close_provider_paths(&transition.stale_paths).await;
                let mut committed_state = transition.next;
                if let Some(dts_path) = committed_state.api_path.clone() {
                    let is_bg = self
                        .is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Api);
                    let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
                    let result = if is_tsgo && !is_bg {
                        sync.open_dts(&dts_path, &api.code).await
                    } else {
                        sync.sync_dts(&dts_path, &api.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                        self.commit_provider_sync_state(canonical_id, committed_state);
                    } else if let Err(e) = result {
                        tracing::warn!(
                            "sync_imported_vue_api_lightweight: failed for {dts_path}: {e}"
                        );
                        self.queue_snapshot_provider_sync(canonical_id.to_string());
                    }
                }
            }
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
        let compile_result = tokio::task::block_in_place(|| {
            if !crate::compile_blockers::ensure_source_loaded_into_host(
                &self.documents.host,
                canonical_id,
            ) {
                tracing::debug!("resync_background: can't read {canonical_id}");
                return None;
            }

            self.hydrate_vue_compile_blockers_for_canonical_id(canonical_id);

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
            let Some(ide) = self.documents.host.get_ide(canonical_id, &profile) else {
                return;
            };
            let Some(transition) =
                self.prepare_vue_provider_sync_transition(canonical_id, ide.is_jsx)
            else {
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                return;
            };
            self.close_provider_paths(&transition.stale_paths).await;
            let mut committed_state = transition.next;

            if !is_tsgo {
                // tsserver: sync IDE output
                if let Some(tsx_path) = committed_state.ide_path.clone() {
                    let is_bg = self
                        .is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Ide);
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
            }

            // Sync .vue.ts for cross-file component type resolution
            if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                let Some(dts_path) = committed_state.api_path.clone() else {
                    return;
                };
                let is_bg =
                    self.is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Api);
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
            self.hydrate_vue_compile_blockers_for_canonical_id(&canonical_id);
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

        if let Some(canonical_id) = self.documents.get_canonical_id(&parsed_uri) {
            self.hydrate_vue_compile_blockers_for_canonical_id(&canonical_id);
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
/// Check whether a requested `context.only` filter includes the given code action kind.
///
/// Check whether a canonical ID (or URI string) refers to a config file
/// that should trigger a project registry rebuild when changed on disk.
///
/// Matches: `tsconfig*.json`, `.verterrc.json`, `vite.config.{ts,js,...}`, `package.json`.
fn is_config_file(path: &str) -> bool {
    // Extract the filename (last segment after '/')
    let filename = path.rsplit('/').next().unwrap_or(path);
    if filename.starts_with("tsconfig") && filename.ends_with(".json") {
        return true;
    }
    if filename == ".verterrc.json" || filename == "package.json" {
        return true;
    }
    // vite.config.{ts,js,mjs,cjs,mts,cts}
    if let Some(ext) = filename.strip_prefix("vite.config.") {
        return matches!(ext, "ts" | "js" | "mjs" | "cjs" | "mts" | "cts");
    }
    false
}

/// Check whether a canonical ID (or URI string) refers to a `.vue` file.
fn is_vue_file(path: &str) -> bool {
    path.ends_with(".vue")
}

/// When `only` is `None` (no filter), all kinds are wanted.
/// Otherwise, checks for hierarchical prefix matching (LSP spec):
/// `"quickfix"` matches `"quickfix.foo"` and vice-versa.
fn wants_code_action_kind(only: Option<&[CodeActionKind]>, kind: &str) -> bool {
    match only {
        None => true,
        Some(kinds) => kinds.iter().any(|k| {
            let k = k.as_str();
            k == kind
                || kind.starts_with(k) && kind.as_bytes().get(k.len()) == Some(&b'.')
                || k.starts_with(kind) && k.as_bytes().get(kind.len()) == Some(&b'.')
        }),
    }
}

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

pub(crate) fn quote_wrapped_specifier(raw_text: &str, specifier: &str) -> String {
    let quote = match raw_text.chars().next() {
        Some('\'') => '\'',
        Some('"') => '"',
        Some('`') => '`',
        _ => '\'',
    };
    format!("{quote}{specifier}{quote}")
}

fn provider_ide_path_for_source(
    resolver: &crate::project_resolver::NativeProjectResolver,
    canonical_id: &str,
    is_jsx: bool,
) -> Option<String> {
    resolver.provider_ide_id_for_source(canonical_id, is_jsx)
}

#[cfg(test)]
fn provider_api_path_for_source(
    resolver: &crate::project_resolver::NativeProjectResolver,
    canonical_id: &str,
) -> Option<String> {
    resolver.provider_id_for_source(canonical_id)
}

fn source_id_from_provider_vue_path(
    resolver: &crate::project_resolver::NativeProjectResolver,
    host: &verter_host::VerterHost,
    provider_path: &str,
) -> Option<String> {
    let candidate = resolver.source_id_from_provider_id(provider_path)?;
    // Collision guard: verify backing .vue source exists in host.
    // A real .vue.tsx on disk under a project root would incorrectly match
    // project ownership for .vue even though no .vue file was compiled.
    if candidate.ends_with(".vue") && host.get_source(&candidate).is_none() {
        return None;
    }
    Some(candidate)
}

struct LspProjectResolverReader<'a> {
    documents: &'a DocumentRegistry,
}

impl<'a> LspProjectResolverReader<'a> {
    fn new(documents: &'a DocumentRegistry) -> Self {
        Self { documents }
    }
}

impl crate::project_resolver::ProjectResolverReader for LspProjectResolverReader<'_> {
    fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
        crate::compile_blockers::ensure_source_loaded_into_host(
            self.documents.host(),
            canonical_id,
        );
        self.documents.host().get_source(canonical_id)
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.documents.host().get_source(canonical_id).is_some()
            || std::path::Path::new(&crate::compile_blockers::normalize_fs_path(canonical_id))
                .is_file()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.documents.host().get_source(canonical_id).is_some() {
            return Some(crate::compile_blockers::normalize_fs_path(canonical_id));
        }

        std::fs::canonicalize(crate::compile_blockers::normalize_fs_path(canonical_id))
            .ok()
            .map(|path| crate::compile_blockers::normalize_fs_path(&path.to_string_lossy()))
    }
}

pub(crate) fn rewrite_non_vue_source_with_resolver(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    importer_id: &str,
    source: &str,
    module_references: &[verter_host::ScriptModuleReference],
) -> String {
    let mut rewritten = source.to_string();
    let mut replacements: Vec<(usize, usize, String)> = module_references
        .iter()
        .filter_map(|reference| {
            if reference.analyzability != verter_analysis::ModuleReferenceAnalyzability::Exact {
                return None;
            }

            let specifier = reference.literal_specifier.as_ref()?;
            let resolved = resolver.resolve_with_reader(
                reader,
                &crate::project_resolver::ResolveRequest {
                    importer_id: importer_id.to_string(),
                    specifier: specifier.clone(),
                    kind: module_reference_request_kind(reference),
                    phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                },
            )?;

            let start = reference.expr_span.start as usize;
            let end = reference.expr_span.end as usize;
            source.get(start..end)?;

            Some((
                start,
                end,
                quote_wrapped_specifier(&reference.raw_text, &resolved.provider_specifier),
            ))
        })
        .collect();

    replacements.sort_by(|left, right| right.0.cmp(&left.0));
    for (start, end, replacement) in replacements {
        rewritten.replace_range(start..end, &replacement);
    }

    rewritten
}

pub(crate) fn prepare_non_vue_provider_sync(
    snapshot: Option<&ResolverSnapshot>,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    importer_id: &str,
    source: &str,
    module_references: &[verter_host::ScriptModuleReference],
) -> Option<PreparedNonVueProviderSync> {
    let snapshot = snapshot?;
    let provider_path = snapshot.resolver.provider_id_for_source(importer_id)?;
    let rewritten = rewrite_non_vue_source_with_resolver(
        &snapshot.resolver,
        reader,
        importer_id,
        source,
        module_references,
    );
    let resolved_dependencies = collect_resolved_provider_dependencies(
        &snapshot.resolver,
        reader,
        importer_id,
        module_references,
    );

    Some(PreparedNonVueProviderSync {
        provider_path,
        rewritten,
        resolved_dependencies,
    })
}

pub(crate) fn collect_resolved_provider_dependencies(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    importer_id: &str,
    module_references: &[verter_host::ScriptModuleReference],
) -> Vec<crate::project_resolver::ResolveResult> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for reference in module_references {
        let kind = module_reference_request_kind(reference);
        match reference.analyzability {
            verter_analysis::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = &reference.literal_specifier {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                        },
                    ) {
                        let key = (result.source_id.clone(), result.provider_id.clone());
                        if seen.insert(key) {
                            resolved.push(result);
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::FiniteSet => {
                for specifier in &reference.finite_specifiers {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::ProviderGraph,
                        },
                    ) {
                        let key = (result.source_id.clone(), result.provider_id.clone());
                        if seen.insert(key) {
                            resolved.push(result);
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::UnknownDynamic => {}
        }
    }

    resolved
}

pub(crate) fn module_reference_request_kind(
    reference: &verter_host::ScriptModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
    }
}

fn analyzed_module_reference_request_kind(
    reference: &verter_analysis::AnalyzedModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
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

/// Resolve a component's analysis snapshot from an import source.
///
/// Extracted as a free function so both `VerterLanguageServer` and `SyncCoordinator`
/// can resolve component types for diagnostic computation.
pub(crate) fn resolve_component_for(
    host: &verter_host::VerterHost,
    project_registry: &parking_lot::RwLock<Option<crate::config::ProjectRegistry>>,
    parent_canonical_id: &str,
    import_source: &str,
) -> Option<verter_host::FileAnalysisSnapshot> {
    // Try 1: Relative import
    if import_source.starts_with('.') {
        let parts: Vec<&str> = parent_canonical_id.split('/').collect();
        let dir = parts[..parts.len().saturating_sub(1)].join("/");
        let resolved = resolve_import_path(&dir, import_source);
        if let Some(a) = host.get_analysis(&resolved) {
            return Some(a);
        }
    }

    // Try 2: Path alias resolution (per-project)
    {
        let registry_guard = project_registry.read();
        if let Some(ref registry) = *registry_guard {
            if let Some(resolved_path) = registry.resolve_alias(parent_canonical_id, import_source)
            {
                if let Some(a) = host.get_analysis(&resolved_path) {
                    return Some(a);
                }
            }
        }
    }

    // Try 3: Direct lookup
    host.get_analysis(import_source)
}

fn location_from_span(
    uri: &Uri,
    line_index: &LineIndex,
    span: verter_span::Span,
) -> Option<Location> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    Some(Location {
        uri: uri.clone(),
        range: Range {
            start: line_index.offset_to_position(span.start)?,
            end: line_index.offset_to_position(span.end)?,
        },
    })
}

fn goto_response_from_locations(locations: Vec<Location>) -> GotoDefinitionResponse {
    if locations.len() == 1 {
        GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap())
    } else {
        GotoDefinitionResponse::Array(locations)
    }
}

fn event_name_match_rank(requested: &str, candidate: &str) -> Option<u8> {
    if requested == candidate {
        return Some(0);
    }

    (normalized_event_name(requested) == normalized_event_name(candidate)).then_some(1)
}

fn normalized_event_name(name: &str) -> String {
    let mut parts = name.splitn(2, ':');
    let head = parts.next().unwrap_or_default();
    match parts.next() {
        Some(tail) => format!(
            "{}:{}",
            camelize_event_segment(head),
            camelize_event_segment(tail)
        ),
        None => camelize_event_segment(head),
    }
}

fn event_name_variants(name: &str) -> Vec<String> {
    let mut variants = vec![name.to_string()];
    let normalized = normalized_event_name(name);
    if normalized != name {
        variants.push(normalized);
    }

    let mut parts = name.splitn(2, ':');
    let head = parts.next().unwrap_or_default();
    let hyphenated = match parts.next() {
        Some(tail) => format!(
            "{}:{}",
            hyphenate_event_segment(head),
            hyphenate_event_segment(tail)
        ),
        None => hyphenate_event_segment(head),
    };
    if !hyphenated.is_empty() && !variants.iter().any(|variant| variant == &hyphenated) {
        variants.push(hyphenated);
    }

    variants
}

fn listener_prop_candidates(event_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for variant in event_name_variants(event_name) {
        let candidate = format!("on{}", capitalize_first(&variant));
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn camelize_event_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize_next = false;
    for ch in value.chars() {
        if ch == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn hyphenate_event_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

fn push_unique_location(
    locations: &mut Vec<Location>,
    seen: &mut HashSet<(String, u32, u32, u32, u32)>,
    location: Location,
) {
    let key = (
        location.uri.as_str().to_string(),
        location.range.start.line,
        location.range.start.character,
        location.range.end.line,
        location.range.end.character,
    );
    if seen.insert(key) {
        locations.push(location);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DidOpenStartupPolicy {
    sync_imported_vue_files: bool,
    publish_diagnostics: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DidOpenProviderSyncPolicy {
    await_ide_sync: bool,
    await_api_sync: bool,
    background_api_sync: bool,
}

fn did_open_startup_policy(kind: crate::TypeProviderKind) -> DidOpenStartupPolicy {
    DidOpenStartupPolicy {
        // When a type provider is active, eagerly sync imported .vue files so that
        // hover/completions/go-to-definition work on <ChildComponent> immediately.
        sync_imported_vue_files: !matches!(kind, crate::TypeProviderKind::None),
        publish_diagnostics: false,
    }
}

fn did_open_provider_sync_policy(kind: crate::TypeProviderKind) -> DidOpenProviderSyncPolicy {
    match kind {
        crate::TypeProviderKind::Tsgo => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: true,
            background_api_sync: false,
        },
        crate::TypeProviderKind::Tsserver => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: false,
            background_api_sync: true,
        },
        crate::TypeProviderKind::None => DidOpenProviderSyncPolicy {
            await_ide_sync: true,
            await_api_sync: false,
            background_api_sync: false,
        },
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn collect_imported_vue_priority_ids(
    analysis: &verter_analysis::ScriptAnalysisSnapshot,
) -> Vec<String> {
    collect_imported_vue_priority_ids_from_imports(&analysis.imports)
}

fn collect_imported_vue_priority_ids_from_imports(
    imports: &[verter_analysis::AnalyzedImport],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for import in imports {
        let Some(canonical_id) = import.resolved_canonical_id.as_ref() else {
            continue;
        };
        if !canonical_id.ends_with(".vue") {
            continue;
        }
        if seen.insert(canonical_id.clone()) {
            ids.push(canonical_id.clone());
        }
    }

    ids
}

fn collect_priority_vue_targets_from_module_references(
    snapshot: Option<&ResolverSnapshot>,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    importer_id: &str,
    module_references: &[verter_analysis::AnalyzedModuleReference],
) -> Vec<String> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for reference in module_references {
        let specifiers = if let Some(specifier) = reference.literal_specifier.as_deref() {
            vec![specifier.to_string()]
        } else {
            reference.finite_specifiers.clone()
        };

        for specifier in specifiers {
            let request = crate::project_resolver::ResolveRequest {
                importer_id: importer_id.to_string(),
                specifier,
                kind: analyzed_module_reference_request_kind(reference),
                phase: crate::project_resolver::ResolvePhase::ProviderGraph,
            };
            let Some(resolved) = snapshot.resolver.resolve_with_reader(reader, &request) else {
                continue;
            };
            if resolved.provider_target == crate::project_resolver::ProviderTarget::VuePublicApi
                && seen.insert(resolved.source_id.clone())
            {
                ids.push(resolved.source_id);
            }
        }
    }

    ids
}

/// Compute verter diagnostics (host errors + lint rules + component usage) for a document.
///
/// Extracted as a free function so both `VerterLanguageServer::compute_verter_diagnostics()`
/// and the `SyncCoordinator` can produce fresh diagnostics. Results are cached per document
/// version in `cached_verter_diags` to avoid redundant re-computation.
pub(crate) fn compute_verter_diagnostics_for(
    documents: &DocumentRegistry,
    uri: &Uri,
    cached_verter_diags: &DashMap<String, (i32, Vec<Diagnostic>)>,
    project_registry: &parking_lot::RwLock<Option<crate::config::ProjectRegistry>>,
    fallback_linter: &parking_lot::RwLock<verter_diagnostics::Linter>,
) -> Vec<Diagnostic> {
    // Check cache: if version matches, return cached diagnostics.
    let uri_str = uri.as_str();
    if let Some(doc) = documents.get(uri) {
        if let Some(cached) = cached_verter_diags.get(uri_str) {
            if cached.0 == doc.version {
                return cached.1.clone();
            }
        }
    }

    let mut diags = if let Some(doc) = documents.get(uri) {
        let host_diags = documents.get_diagnostics(uri);
        match host_diags {
            Some(snapshot) => map_diagnostics(&snapshot, &doc.line_index),
            None => vec![],
        }
    } else {
        vec![]
    };

    // Run the diagnostics engine (lint rules: CSS, template, a11y, etc.)
    if let Some(doc) = documents.get(uri) {
        if let Some(analysis) = documents.get_analysis(uri) {
            let canonical_id = uri_to_canonical_id(uri);

            // Look up per-project lint config
            let lint_explicitly_configured = {
                let registry_guard = project_registry.read();
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
                    let registry_guard = project_registry.read();
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
                    let fl = fallback_linter.read();
                    diags.extend(crate::features::diagnostics_bridge::run_linter(
                        &fl,
                        &analysis,
                        &doc.source,
                        &doc.line_index,
                    ));
                }
            }

            // Component usage diagnostics (unknown props, unknown v-models).
            let host = documents.host();
            diags.extend(
                crate::features::component_diagnostics::component_usage_diagnostics(
                    &analysis,
                    &doc.line_index,
                    &|import_source| {
                        resolve_component_for(host, project_registry, &canonical_id, import_source)
                    },
                ),
            );

            // When lint is not explicitly configured, suppress lint diagnostics but
            // keep component usage diagnostics (type-level, not lint rules).
            if !lint_explicitly_configured {
                diags.retain(|d| match &d.code {
                    Some(NumberOrString::String(code)) => {
                        if code == "verter/unknown-prop" || code == "verter/unknown-model" {
                            return true;
                        }
                        !code.starts_with("verter/")
                    }
                    _ => true,
                });
            }
        }
    }

    // Cache the result
    if let Some(doc) = documents.get(uri) {
        cached_verter_diags.insert(uri_str.to_string(), (doc.version, diags.clone()));
    }

    diags
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
    vite_opts: crate::vite_config::ViteConfigOptions,
    init_lint_opts: Option<serde_json::Value>,
    my_gen: u64,
    client: Client,
    type_provider: Option<Arc<dyn TypeProvider>>,
    project_registry: Arc<parking_lot::RwLock<Option<crate::config::ProjectRegistry>>>,
    resolver_snapshot: Arc<parking_lot::RwLock<Option<ResolverSnapshot>>>,
    fallback_linter: Arc<parking_lot::RwLock<verter_diagnostics::Linter>>,
    workspace_scanner:
        Arc<tokio::sync::Mutex<Option<crate::workspace_scanner::WorkspaceScannerHandle>>>,
    init_generation: Arc<std::sync::atomic::AtomicU64>,
    project_sync: Option<ProjectSync>,
    documents: Arc<DocumentRegistry>,
    provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    pending_snapshot_provider_sync: Arc<DashSet<String>>,
    is_tsgo: bool,
    cached_verter_diags: Arc<DashMap<String, (i32, Vec<Diagnostic>)>>,
    position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Snapshot of MRU list at init time for drain ordering.
    mru_canonical_ids: Arc<parking_lot::Mutex<Vec<String>>>,
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
        vite_opts,
        init_lint_opts,
        my_gen,
        client,
        type_provider,
        project_registry,
        resolver_snapshot,
        fallback_linter,
        workspace_scanner,
        init_generation,
        project_sync,
        documents,
        provider_sync_states,
        pending_snapshot_provider_sync,
        is_tsgo,
        cached_verter_diags,
        position_encoding,
        mru_canonical_ids,
    } = args;

    let host = documents.host_arc();
    let tsx_profile = Arc::clone(&documents.tsx_profile);

    // 1. Build project registry (spawn_blocking — blocking I/O: vite eval, tsconfig)
    let roots_for_registry = roots.clone();
    let vite_opts_for_registry = vite_opts.clone();
    let registry_result = tokio::task::spawn_blocking(move || {
        crate::config::ProjectRegistry::from_workspace_roots(
            &roots_for_registry,
            &vite_opts_for_registry,
        )
    })
    .await;

    let build_result = match registry_result {
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

    let mut registry = build_result.registry;
    let trust_required = build_result.trust_required;

    // Log discovered projects
    for project in registry.projects() {
        tracing::info!(
            "project config: root={}, tsconfig={:?}, aliases={}, lint_explicit={}",
            project.root,
            project.tsconfig_path,
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
            let Some(tsconfig_path) = project.tsconfig_path.as_deref() else {
                continue;
            };
            let tsconfig_path = std::path::PathBuf::from(tsconfig_path);
            if let Some((base_url, paths)) =
                crate::config::TsConfigPathResolver::raw_paths_json(&tsconfig_path)
            {
                tracing::info!(
                    "configuring tsserver paths for {} via {} (baseUrl: {})",
                    project.root,
                    tsconfig_path.display(),
                    base_url,
                );
                if let Err(e) = tp.configure_paths(&base_url, paths).await {
                    tracing::warn!("failed to configure tsserver paths: {e}");
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
    // 4a. TSGO limitation warning: no tsconfig found
    if is_tsgo
        && registry
            .projects()
            .iter()
            .all(|p| p.tsconfig_path.is_none())
    {
        tracing::warn!(
            "TSGO active but no project has a tsconfig.json — type checking will be limited"
        );
        client
            .send_notification::<TsgoNoTsconfig>(TsgoNoTsconfigParams {
                message: "No tsconfig.json found. TSGO requires a tsconfig.json for project \
                          configuration discovery. Consider adding one or switching to tsserver \
                          (verter.typeProvider: \"tsserver\")."
                    .to_string(),
            })
            .await;
    }

    let resolver = registry.to_native_project_resolver();
    *resolver_snapshot.write() = Some(ResolverSnapshot {
        generation: my_gen,
        resolver,
    });
    *project_registry.write() = Some(registry);

    drain_pending_snapshot_provider_sync(
        project_sync.as_ref(),
        &documents,
        &resolver_snapshot,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        is_tsgo,
        Some(&mru_canonical_ids),
    )
    .await;

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
            resolver_snapshot: Arc::clone(&resolver_snapshot),
            provider_sync_states: Arc::clone(&provider_sync_states),
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

    // 7a. Publish fresh diagnostics for all open files now that project_registry
    // is built and type_provider is synced. This ensures TS diagnostics appear
    // after background init without requiring an edit.
    {
        let open_uris = documents.open_uris();
        for uri_str in &open_uris {
            let uri: Uri = match uri_str.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };

            let verter_diags = compute_verter_diagnostics_for(
                &documents,
                &uri,
                &cached_verter_diags,
                &project_registry,
                &fallback_linter,
            );

            let diagnostics = if let Some(tp) = &type_provider {
                let canonical_id = crate::documents::uri_to_canonical_id(&uri);
                let profile = tsx_profile.read().clone();
                let ide = documents.host.get_ide(&canonical_id, &profile);

                if let Some(ide) = ide {
                    let snapshot = resolver_snapshot.read().clone();
                    let Some(tsx_path) = snapshot.as_ref().and_then(|snapshot| {
                        provider_ide_path_for_source(&snapshot.resolver, &canonical_id, ide.is_jsx)
                    }) else {
                        continue;
                    };
                    let encoding = position_encoding.read().clone();
                    let tsx_li =
                        crate::documents::line_index::LineIndex::new(&ide.code, encoding.clone());
                    let mapper = ide
                        .source_map
                        .as_ref()
                        .and_then(|sm| PositionMapper::from_json(sm).ok());
                    let vue_source = documents.host.get_source(&canonical_id);

                    match (tp.get_diagnostics(&tsx_path).await, mapper, vue_source) {
                        (Ok(type_diags), Some(mapper), Some(vue_src)) => {
                            let vue_li =
                                crate::documents::line_index::LineIndex::new(&vue_src, encoding);
                            crate::tsgo::merge::merge_diagnostics(
                                verter_diags,
                                type_diags,
                                &tsx_li,
                                &mapper,
                                &vue_li,
                            )
                        }
                        _ => verter_diags,
                    }
                } else {
                    verter_diags
                }
            } else {
                verter_diags
            };

            client.publish_diagnostics(uri, diagnostics, None).await;
        }
    }

    client
        .send_notification::<VerterReady>(VerterReadyParams { gen: my_gen })
        .await;

    // Notify client about Vite configs that need trust approval
    for info in &trust_required {
        tracing::debug!(
            "vite config trust required: {} ({})",
            info.config_path,
            info.reason
        );
        client
            .send_notification::<ViteConfigTrustRequired>(ViteConfigTrustRequiredParams {
                config_path: info.config_path.clone(),
                workspace_root: info.workspace_root.clone(),
                reason: info.reason.clone(),
            })
            .await;
    }

    tracing::info!("background init complete (gen={my_gen})");
    Ok(())
}

async fn drain_pending_snapshot_provider_sync(
    project_sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    pending_snapshot_provider_sync: &DashSet<String>,
    is_tsgo: bool,
    mru_canonical_ids: Option<&parking_lot::Mutex<Vec<String>>>,
) {
    let Some(sync) = project_sync else {
        pending_snapshot_provider_sync.clear();
        return;
    };
    let Some(snapshot) = resolver_snapshot.read().clone() else {
        return;
    };

    // Collect pending IDs and sort by MRU order
    let pending_ids: Vec<String> = {
        let all_pending: Vec<String> = pending_snapshot_provider_sync
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        if let Some(mru_lock) = mru_canonical_ids {
            let mru = mru_lock.lock();
            let mut ordered = Vec::with_capacity(all_pending.len());
            // MRU files first
            for mru_id in mru.iter() {
                if all_pending.contains(mru_id) {
                    ordered.push(mru_id.clone());
                }
            }
            // Then remaining files not in MRU
            for id in &all_pending {
                if !ordered.contains(id) {
                    ordered.push(id.clone());
                }
            }
            ordered
        } else {
            all_pending
        }
    };

    for canonical_id in pending_ids {
        let synced = sync_pending_snapshot_provider_file(
            sync,
            documents,
            &snapshot,
            provider_sync_states,
            &canonical_id,
            is_tsgo,
        )
        .await;

        if synced || documents.host.get_source(&canonical_id).is_none() {
            pending_snapshot_provider_sync.remove(&canonical_id);
        }
    }
}

async fn sync_pending_snapshot_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
    if canonical_id.ends_with(".vue") {
        sync_pending_vue_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
            is_tsgo,
        )
        .await
    } else {
        sync_pending_non_vue_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
        )
        .await
    }
}

async fn sync_pending_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
    let reader = crate::compile_blockers::HostFsProjectResolverReader::new(documents.host());
    crate::compile_blockers::hydrate_vue_compile_blockers(
        documents.host(),
        &snapshot.resolver,
        &reader,
        canonical_id,
    );
    let profile = documents.tsx_profile.read().clone();
    let _ = tokio::task::block_in_place(|| documents.host.ensure_compiled(canonical_id, &profile));
    let ide = tokio::task::block_in_place(|| documents.host.get_ide(canonical_id, &profile));
    let is_jsx = ide.as_ref().map(|output| output.is_jsx).unwrap_or(false);
    let Some(next_state) =
        crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, canonical_id, is_jsx)
    else {
        return false;
    };

    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    close_stale_provider_paths(sync, &transition.stale_paths, "pending_snapshot").await;

    let mut committed_state = transition.next;
    let is_open = documents.canonical_id_to_uri(canonical_id).is_some();
    let mut synced_any = false;

    if let Some(api) = tokio::task::block_in_place(|| documents.host.get_public_api(canonical_id)) {
        let Some(dts_path) = committed_state.api_path.clone() else {
            return false;
        };
        let result = if is_open {
            sync.sync_dts(&dts_path, &api.code).await
        } else if is_tsgo {
            if committed_state.api_background_loaded {
                sync.sync_dts(&dts_path, &api.code).await
            } else {
                sync.open_dts(&dts_path, &api.code).await
            }
        } else if committed_state.api_background_loaded {
            sync.sync_dts(&dts_path, &api.code).await
        } else {
            sync.load_dts(&dts_path, &api.code).await
        };

        match result {
            Ok(()) => {
                if !is_open {
                    committed_state.set_background_loaded(ProviderPathKind::Api, true);
                }
                synced_any = true;
            }
            Err(error) => {
                tracing::warn!(
                    "pending_snapshot: failed to sync provider API path {dts_path}: {error}"
                );
            }
        }
    }

    if !is_tsgo {
        if let Some(ide) = ide {
            let Some(ide_path) = committed_state.ide_path.clone() else {
                return false;
            };
            let result = if is_open || committed_state.ide_background_loaded {
                sync.sync_tsx(&ide_path, &ide.code).await
            } else {
                sync.load_tsx(&ide_path, &ide.code).await
            };

            match result {
                Ok(()) => {
                    if !is_open {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                    }
                    synced_any = true;
                }
                Err(error) => {
                    tracing::warn!(
                        "pending_snapshot: failed to sync provider IDE path {ide_path}: {error}"
                    );
                }
            }
        }
    }

    if synced_any {
        commit_sync_transition(provider_sync_states, canonical_id, committed_state);
    }
    synced_any
}

async fn sync_pending_non_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
) -> bool {
    let Some(source) = documents.host.get_source(canonical_id) else {
        return false;
    };
    let module_references = tokio::task::block_in_place(|| {
        documents
            .host
            .upsert(verter_host::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: source.clone(),
                file_kind: verter_host::FileKind::NonSfc,
                aliases: Vec::new(),
            })
            .map(|result| result.module_references)
            .unwrap_or_default()
    });
    let reader = LspProjectResolverReader::new(documents);
    let Some(prepared) = prepare_non_vue_provider_sync(
        Some(snapshot),
        &reader,
        canonical_id,
        &source,
        &module_references,
    ) else {
        return false;
    };
    let Some(next_state) =
        crate::provider_sync::non_vue_sync_state_for_source(&snapshot.resolver, canonical_id)
    else {
        return false;
    };

    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    close_stale_provider_paths(sync, &transition.stale_paths, "pending_snapshot").await;

    let mut committed_state = transition.next;
    match sync
        .sync_file(&prepared.provider_path, &prepared.rewritten)
        .await
    {
        Ok(()) => {
            committed_state.set_background_loaded(ProviderPathKind::Shadow, true);
            commit_sync_transition(provider_sync_states, canonical_id, committed_state);
            documents.host.set_import_dependencies(
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
            true
        }
        Err(error) => {
            tracing::warn!(
                "pending_snapshot: failed to sync provider shadow path {}: {error}",
                prepared.provider_path
            );
            false
        }
    }
}

async fn close_stale_provider_paths(
    sync: &ProjectSync,
    stale_paths: &[(ProviderPathKind, String)],
    context: &str,
) {
    for (kind, path) in stale_paths {
        let result = match kind {
            ProviderPathKind::Ide => sync.close_tsx(path).await,
            ProviderPathKind::Api => sync.close_dts(path).await,
            ProviderPathKind::Shadow => sync.close_file(path).await,
        };
        if let Err(error) = result {
            tracing::warn!("{context}: failed to close stale provider path {path}: {error}");
        }
    }
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
        // Touch MRU for snapshot drain ordering (after did_open registers the canonical ID)
        if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
            self.touch_mru(&canonical_id);
        }
        if result.diagnostics.has_errors {
            tracing::debug!(
                "did_open: {} errors for {}",
                result.diagnostics.diagnostics.len(),
                uri.as_str(),
            );
        }
        let startup_policy = did_open_startup_policy(self.type_provider_kind);
        let imported_vue_priority_ids = self
            .documents
            .get_analysis(uri)
            .map(|analysis| {
                // Primary: analysis.imports already has resolved_canonical_id from host
                // (works even before background_init builds the resolver snapshot)
                let mut ids = collect_imported_vue_priority_ids_from_imports(&analysis.imports);

                // Supplement: module_references for dynamic import()/require() cases
                // that aren't in analysis.imports (needs resolver, may return empty pre-init)
                if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
                    let snapshot = self.resolver_snapshot();
                    let reader = LspProjectResolverReader::new(&self.documents);
                    let dynamic_ids = collect_priority_vue_targets_from_module_references(
                        snapshot.as_ref(),
                        &reader,
                        &canonical_id,
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
            if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
                scanner.signal_priority(canonical_id);
            }
            for import_id in &imported_vue_priority_ids {
                scanner.signal_priority(import_id.clone());
            }
        }

        // Active file IDE sync FIRST (Interactive priority) — enables typed hover immediately
        let provider_sync_policy = did_open_provider_sync_policy(self.type_provider_kind);
        if provider_sync_policy.await_ide_sync {
            // Use ensure_current_file_synced for immediate IDE-only sync
            self.ensure_current_file_synced(uri).await;
        }

        // Imported Vue API warmup SECOND (Normal priority, never blocks active file)
        if startup_policy.sync_imported_vue_files {
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
            if let Some(canonical_id) = self.documents.get_canonical_id(uri) {
                coordinator.signal(canonical_id, uri.as_str().to_string());
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
                self.needs_ide_sync.insert(canonical_id.clone());
                self.needs_deferred_sync.insert(canonical_id.clone());
                if let Some(coordinator) = &self.sync_coordinator {
                    coordinator.signal(canonical_id, uri.as_str().to_string());
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
            self.resync_background_vue_file(canonical_id).await;
            tracing::debug!("did_change_watched_files: resynced vue {canonical_id}");
        }

        // ── TS/JS file deletions ───────────────────────────────────
        for canonical_id in &ts_js_delete_ids {
            if let Some(state) = self.remove_provider_sync_state(canonical_id) {
                self.close_provider_state(&state).await;
            }
            self.documents.host().remove(canonical_id);
            tracing::debug!("did_change_watched_files: removed {canonical_id}");
        }

        // ── TS/JS file creates/changes ─────────────────────────────
        if !ts_js_resync_ids.is_empty() {
            if let Some(sync) = &self.project_sync {
                let host = self.documents.host_arc();
                let sync = sync.clone();
                let resolver_snapshot = Arc::clone(&self.resolver_snapshot);
                let provider_sync_states = Arc::clone(&self.provider_sync_states);

                tokio::spawn(async move {
                    for canonical_id in ts_js_resync_ids {
                        crate::workspace_scanner::resync_non_vue_file(
                            &canonical_id,
                            &host,
                            &sync,
                            &resolver_snapshot,
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
            let registry = self.project_registry.read();
            if let Some(reg) = registry.as_ref() {
                for canonical_id in &all_changed {
                    if reg
                        .projects()
                        .iter()
                        .any(|p| p.vite_config_deps.iter().any(|dep| dep == canonical_id))
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
            crate::compile_blockers::ensure_source_loaded_into_host(
                self.documents.host(),
                &canonical_id,
            );
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
            let registry_guard = self.project_registry.read();
            canonical_id
                .as_deref()
                .and_then(|cid| registry_guard.as_ref().map(|r| r.is_ssr_context(cid)))
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
                            self.track_type_provider_error(&ctx.tsx_path, &e.to_string());
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
            let registry_guard = self.project_registry.read();
            canonical_id
                .as_deref()
                .and_then(|cid| registry_guard.as_ref().map(|r| r.is_ssr_context(cid)))
                .unwrap_or(false)
        };

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

                let skip_type_provider = expr_context
                    .as_ref()
                    .map(|ec| matches!(ec, ExpressionContext::IdentifierExpected))
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
                        .filter(|t| matches!(*t, "." | "\"" | "'" | "`" | "/" | "<"));
                    match tp
                        .get_completions(&ctx.tsx_path, tsx_offset, tp_trigger)
                        .await
                    {
                        Ok(mut type_result) => {
                            tracing::debug!(
                                "completion: type provider returned {} items (incomplete={})",
                                type_result.items.len(),
                                type_result.is_incomplete
                            );

                            // For Unknown expression context, filter type provider results
                            // to only items matching verter's known template bindings.
                            // Prevents global pollution (AbortController, HTMLElement, etc.)
                            // while preserving richer type provider metadata for known bindings.
                            if matches!(expr_context, Some(ExpressionContext::Unknown)) {
                                let allowlist: std::collections::HashSet<&str> = verter_items
                                    .as_ref()
                                    .map(|items| items.iter().map(|i| i.label.as_str()).collect())
                                    .unwrap_or_default();
                                let before = type_result.items.len();
                                type_result
                                    .items
                                    .retain(|item| allowlist.contains(item.label.as_str()));
                                tracing::debug!(
                                    "completion: filtered type provider for Unknown context: {} → {} items",
                                    before,
                                    type_result.items.len()
                                );
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
                            self.track_type_provider_error(&ctx.tsx_path, &e.to_string());
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
                    // Follow re-exports (up to 10 levels deep) to find the actual definition
                    let (resolved_id, start, end) = host
                        .get_export_span_follow_reexports(target_canonical_id, binding_name, 10)
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

        // Native cross-file prop navigation: cursor on a component prop attribute
        // → navigate to the matching prop field in the child's defineProps.
        if verter_result.is_none() {
            match self.resolve_component_event_definition(uri, position) {
                ComponentEventDefinitionResolution::NotApplicable => {}
                ComponentEventDefinitionResolution::NoDefinition => return Ok(None),
                ComponentEventDefinitionResolution::Resolved(definition) => {
                    return Ok(Some(definition));
                }
            }
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
                            let vue_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            return Ok(merge::merge_definitions(
                                verter_result,
                                type_defs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                uri,
                                &vue_source_exists,
                            ));
                        }
                        Err(e) => {
                            tracing::warn!("definition: type provider error: {e}");
                            self.track_type_provider_error(&ctx.tsx_path, &e.to_string());
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
                            return Ok(merge::merge_definitions(
                                None,
                                type_defs,
                                &ctx.tsx_line_index,
                                &ctx.mapper,
                                &ctx.vue_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                uri,
                                &vue_source_exists,
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
                        &|import_source| self.resolve_component_context(uri, import_source),
                    );
                    all_actions.extend(comp_actions);

                    // Suggest matching props from parent bindings to child component tags
                    let suggest_actions =
                        crate::features::component_actions::suggest_matching_props(
                            analysis,
                            &doc.source,
                            &doc.line_index,
                            uri,
                            &|import_source| self.resolve_component_context(uri, import_source),
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
            // Lock ordering: project_registry → release → fallback_linter (never nested).
            if wants_quickfix || wants_refactor {
                if let Some(ref analysis) = analysis {
                    let canonical_id = uri_to_canonical_id(uri);
                    let used_project = {
                        let registry_guard = self.project_registry.read();
                        if let Some(project) = registry_guard
                            .as_ref()
                            .and_then(|r| r.linter_for(&canonical_id))
                        {
                            if wants_quickfix {
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
                            }
                            if wants_refactor {
                                if let Some(offset) =
                                    doc.line_index.position_to_offset(&range.start)
                                {
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
                            }
                            true
                        } else {
                            false
                        }
                    }; // registry_guard dropped here

                    if !used_project {
                        let fl = self.fallback_linter.read();
                        if wants_quickfix {
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
                        }
                        if wants_refactor {
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
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use futures_util::StreamExt;
    use verter_host::{HostConfig, VerterHost};

    use crate::tsgo::mock::{MockCall, MockTypeProvider};
    use crate::tsgo::protocol::{
        CompletionResult, HoverInfo, InlayHint, RenameLocation, SemanticToken, SignatureHelp,
        TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation,
    };
    use crate::tsgo::traits::{ProviderFuture, TypeProvider};
    use crate::ProjectSyncMode;

    #[derive(Default)]
    struct SlowConfigurePathsProvider {
        configure_paths_started: AtomicUsize,
    }

    impl TypeProvider for SlowConfigurePathsProvider {
        fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }

        fn get_completions(
            &self,
            _path: &str,
            _offset: u32,
            _trigger_character: Option<&str>,
        ) -> ProviderFuture<'_, CompletionResult> {
            Box::pin(async {
                Ok(CompletionResult {
                    items: Vec::new(),
                    is_incomplete: false,
                })
            })
        }

        fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
            Box::pin(async { Ok(None) })
        }

        fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_definition(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_type_definition(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_references(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeLocation>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_rename_locations(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<RenameLocation>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_signature_help(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Option<SignatureHelp>> {
            Box::pin(async { Ok(None) })
        }

        fn get_code_actions(
            &self,
            _path: &str,
            _start_offset: u32,
            _end_offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_document_highlights(
            &self,
            _path: &str,
            _offset: u32,
        ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn get_inlay_hints(
            &self,
            _path: &str,
            _start_offset: u32,
            _end_offset: u32,
        ) -> ProviderFuture<'_, Vec<InlayHint>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn configure_paths(
            &self,
            _base_url: &str,
            _paths: serde_json::Value,
        ) -> ProviderFuture<'_, ()> {
            self.configure_paths_started.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok(())
            })
        }
    }

    fn make_hover_test_service(
        type_provider: Arc<dyn TypeProvider>,
    ) -> tower_lsp_server::LspService<VerterLanguageServer> {
        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let host_for_server = Arc::clone(&host);
        let type_provider_for_server = Arc::clone(&type_provider);
        let (service, _socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: Some(Arc::clone(&type_provider_for_server)),
                    project_sync_mode: crate::ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::Tsserver,
                    suggest_tsgo: false,
                    mcp_port: None,
                    type_provider_none_reason: None,
                },
            )
        });
        service
    }

    fn install_test_resolver(server: &VerterLanguageServer) {
        *server.resolver_snapshot.write() = Some(ResolverSnapshot {
            generation: 1,
            resolver: crate::project_resolver::NativeProjectResolver::new(vec![
                crate::project_resolver::IdeProjectConfig::new(
                    "/workspace".to_string(),
                    "/workspace".to_string(),
                    Some("/workspace/tsconfig.json".to_string()),
                ),
            ]),
        });
    }

    fn open_test_vue(server: &VerterLanguageServer, path: &str, source: &str) -> Uri {
        let uri: Uri = format!("file://{path}").parse().expect("valid test uri");
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: source.to_string(),
        });
        uri
    }

    fn hover_params(uri: &Uri, position: Position) -> HoverParams {
        HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        }
    }

    fn hover_text(hover: Option<Hover>) -> String {
        match hover.expect("hover should exist").contents {
            HoverContents::Markup(m) => m.value,
            HoverContents::Scalar(MarkedString::String(s)) => s,
            HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value,
            HoverContents::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    MarkedString::String(s) => s,
                    MarkedString::LanguageString(ls) => ls.value,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn set_type_hover_at_vue_position(
        server: &VerterLanguageServer,
        provider: &MockTypeProvider,
        uri: &Uri,
        position: Position,
        contents: &str,
    ) {
        let ctx = server
            .type_provider_context(uri)
            .expect("type provider context should exist");
        let tsx_offset = merge::vue_position_to_tsx_offset_validated(
            &position,
            &ctx.vue_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        )
        .expect("vue position should map to tsx");
        provider.set_hover(
            &ctx.tsx_path,
            tsx_offset,
            Some(HoverInfo {
                contents: contents.to_string(),
                range_start: None,
                range_end: None,
            }),
        );
    }

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

    fn test_module_reference(
        raw_text: &str,
        literal_specifier: Option<&str>,
        finite_specifiers: &[&str],
        analyzability: verter_analysis::ModuleReferenceAnalyzability,
        expr_start: usize,
        expr_end: usize,
    ) -> verter_host::ScriptModuleReference {
        verter_host::ScriptModuleReference {
            syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
            semantics: verter_analysis::ModuleReferenceSemantics::Import,
            is_type_only: false,
            raw_text: raw_text.to_string(),
            literal_specifier: literal_specifier.map(str::to_string),
            finite_specifiers: finite_specifiers
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            static_prefix: None,
            analyzability,
            span: verter_span::Span::new(expr_start as u32, expr_end as u32),
            expr_span: verter_span::Span::new(expr_start as u32, expr_end as u32),
        }
    }

    fn test_module_reference_with_semantics(
        raw_text: &str,
        literal_specifier: Option<&str>,
        finite_specifiers: &[&str],
        analyzability: verter_analysis::ModuleReferenceAnalyzability,
        expr_start: usize,
        expr_end: usize,
        semantics: verter_analysis::ModuleReferenceSemantics,
        is_type_only: bool,
    ) -> verter_host::ScriptModuleReference {
        verter_host::ScriptModuleReference {
            semantics,
            is_type_only,
            ..test_module_reference(
                raw_text,
                literal_specifier,
                finite_specifiers,
                analyzability,
                expr_start,
                expr_end,
            )
        }
    }

    fn test_analyzed_module_reference(
        raw_text: &str,
        literal_specifier: Option<&str>,
        finite_specifiers: &[&str],
        analyzability: verter_analysis::ModuleReferenceAnalyzability,
        expr_start: usize,
        expr_end: usize,
    ) -> verter_analysis::AnalyzedModuleReference {
        verter_analysis::AnalyzedModuleReference {
            syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
            semantics: verter_analysis::ModuleReferenceSemantics::Import,
            is_type_only: false,
            raw_text: raw_text.to_string(),
            literal_specifier: literal_specifier.map(str::to_string),
            finite_specifiers: finite_specifiers
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            static_prefix: None,
            analyzability,
            span: verter_span::Span::new(expr_start as u32, expr_end as u32),
            expr_span: verter_span::Span::new(expr_start as u32, expr_end as u32),
        }
    }

    #[derive(Default)]
    struct TestResolverReader {
        files: HashSet<String>,
        texts: HashMap<String, Arc<str>>,
    }

    impl TestResolverReader {
        fn with_files(paths: &[&str]) -> Self {
            let mut reader = Self::default();
            for path in paths {
                let normalized = path.replace('\\', "/");
                reader.files.insert(normalized.clone());
                reader
                    .texts
                    .insert(normalized, Arc::<str>::from("// test file"));
            }
            reader
        }
    }

    impl crate::project_resolver::ProjectResolverReader for TestResolverReader {
        fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
            self.texts.get(&canonical_id.replace('\\', "/")).cloned()
        }

        fn file_exists(&self, canonical_id: &str) -> bool {
            self.files.contains(&canonical_id.replace('\\', "/"))
        }

        fn realpath(&self, canonical_id: &str) -> Option<String> {
            let normalized = canonical_id.replace('\\', "/");
            self.file_exists(&normalized).then_some(normalized)
        }
    }

    async fn make_definition_test_server(
        files: &[(&str, &str, &str)],
    ) -> (
        tempfile::TempDir,
        tower_lsp_server::LspService<VerterLanguageServer>,
        tokio::task::JoinHandle<()>,
        Arc<MockTypeProvider>,
        String,
    ) {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        std::fs::write(workspace.join("tsconfig.json"), "{}").expect("write tsconfig");

        for (relative_path, _language_id, source) in files {
            let file_path = relative_path
                .split('/')
                .fold(workspace.clone(), |path, segment| path.join(segment));
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dirs");
            }
            std::fs::write(&file_path, source).expect("write source file");
        }

        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let host_for_server = Arc::clone(&host);
        let type_provider_for_server = Arc::clone(&type_provider);
        let (service, socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: Some(Arc::clone(&type_provider_for_server)),
                    project_sync_mode: crate::ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::Tsserver,
                    suggest_tsgo: false,
                    mcp_port: None,
                    type_provider_none_reason: None,
                },
            )
        });
        let drain_handle = tokio::spawn(async move {
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });

        let workspace_id = workspace.to_string_lossy().replace('\\', "/");
        let server = service.inner();
        *server.resolver_snapshot.write() = Some(ResolverSnapshot {
            generation: 1,
            resolver: crate::project_resolver::NativeProjectResolver::new(vec![
                crate::project_resolver::IdeProjectConfig::new(
                    workspace_id.clone(),
                    workspace_id.clone(),
                    Some(format!("{workspace_id}/tsconfig.json")),
                ),
            ]),
        });

        for (relative_path, language_id, source) in files {
            let canonical_id = format!("{workspace_id}/{relative_path}");
            let uri = crate::uri::path_to_file_uri(&canonical_id).expect("file uri");
            let _ = server.documents.did_open(&TextDocumentItem {
                uri,
                language_id: (*language_id).to_string(),
                version: 1,
                text: (*source).to_string(),
            });
        }

        (temp, service, drain_handle, provider, workspace_id)
    }

    fn workspace_uri(workspace_id: &str, relative_path: &str) -> Uri {
        crate::uri::path_to_file_uri(&format!("{workspace_id}/{relative_path}")).expect("file uri")
    }

    fn find_document_position(
        server: &VerterLanguageServer,
        uri: &Uri,
        needle: &str,
        delta: usize,
    ) -> Position {
        let doc = server.documents.get(uri).expect("document should be open");
        let offset = doc
            .source
            .find(needle)
            .unwrap_or_else(|| panic!("needle `{needle}` should exist"))
            + delta;
        doc.line_index
            .offset_to_position(offset as u32)
            .expect("valid position")
    }

    fn definition_locations(response: GotoDefinitionResponse) -> Vec<Location> {
        match response {
            GotoDefinitionResponse::Scalar(location) => vec![location],
            GotoDefinitionResponse::Array(locations) => locations,
            GotoDefinitionResponse::Link(links) => links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_range,
                })
                .collect(),
        }
    }

    fn line_for_snippet(source: &str, needle: &str) -> u32 {
        let offset = source
            .find(needle)
            .unwrap_or_else(|| panic!("needle `{needle}` should exist"));
        LineIndex::new_utf16(source)
            .offset_to_position(offset as u32)
            .expect("valid position")
            .line
    }

    fn goto_definition_params(uri: &Uri, position: Position) -> GotoDefinitionParams {
        GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[test]
    fn module_reference_request_kind_uses_require_semantics() {
        let require_reference = test_module_reference_with_semantics(
            "'pkg'",
            Some("pkg"),
            &[],
            verter_analysis::ModuleReferenceAnalyzability::Exact,
            0,
            5,
            verter_analysis::ModuleReferenceSemantics::Require,
            false,
        );
        assert_eq!(
            module_reference_request_kind(&require_reference),
            crate::project_resolver::ResolveRequestKind::RequireCall
        );

        let type_reference = test_module_reference_with_semantics(
            "'pkg'",
            Some("pkg"),
            &[],
            verter_analysis::ModuleReferenceAnalyzability::Exact,
            0,
            5,
            verter_analysis::ModuleReferenceSemantics::Import,
            true,
        );
        assert_eq!(
            module_reference_request_kind(&type_reference),
            crate::project_resolver::ResolveRequestKind::TypeImport
        );
    }

    #[test]
    fn provider_sync_without_snapshot_is_deferred_not_fallback_rewritten() {
        let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
        let foo_expr = "'./Foo.vue'";
        let util_expr = "'./util'";
        let dynamic_expr = "`./${name}.vue`";
        let foo_start = source.find(foo_expr).unwrap();
        let util_start = source.find(util_expr).unwrap();
        let dynamic_start = source.find(dynamic_expr).unwrap();

        let reader =
            TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);

        let prepared = prepare_non_vue_provider_sync(
            None,
            &reader,
            "/workspace/src/App.ts",
            source,
            &[
                test_module_reference(
                    foo_expr,
                    Some("./Foo.vue"),
                    &[],
                    verter_analysis::ModuleReferenceAnalyzability::Exact,
                    foo_start,
                    foo_start + foo_expr.len(),
                ),
                test_module_reference(
                    util_expr,
                    Some("./util"),
                    &[],
                    verter_analysis::ModuleReferenceAnalyzability::Exact,
                    util_start,
                    util_start + util_expr.len(),
                ),
                test_module_reference(
                    dynamic_expr,
                    None,
                    &["./Foo.vue"],
                    verter_analysis::ModuleReferenceAnalyzability::FiniteSet,
                    dynamic_start,
                    dynamic_start + dynamic_expr.len(),
                ),
            ],
        );
        assert!(
            prepared.is_none(),
            "provider sync should be deferred until a resolver snapshot exists"
        );
    }

    #[test]
    fn provider_sync_with_snapshot_uses_resolved_dependencies_only() {
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        let reader =
            TestResolverReader::with_files(&["/workspace/src/Foo.vue", "/workspace/src/util.ts"]);
        let source =
            "import Foo from './Foo.vue';\nimport util from './util';\nconst keep = import(`./${name}.vue`);\n";
        let foo_expr = "'./Foo.vue'";
        let util_expr = "'./util'";
        let dynamic_expr = "`./${name}.vue`";
        let foo_start = source.find(foo_expr).unwrap();
        let util_start = source.find(util_expr).unwrap();
        let dynamic_start = source.find(dynamic_expr).unwrap();

        let prepared = prepare_non_vue_provider_sync(
            Some(&ResolverSnapshot {
                generation: 1,
                resolver,
            }),
            &reader,
            "/workspace/src/App.ts",
            source,
            &[
                test_module_reference(
                    foo_expr,
                    Some("./Foo.vue"),
                    &[],
                    verter_analysis::ModuleReferenceAnalyzability::Exact,
                    foo_start,
                    foo_start + foo_expr.len(),
                ),
                test_module_reference(
                    util_expr,
                    Some("./util"),
                    &[],
                    verter_analysis::ModuleReferenceAnalyzability::Exact,
                    util_start,
                    util_start + util_expr.len(),
                ),
                test_module_reference(
                    dynamic_expr,
                    None,
                    &["./Foo.vue", "./util"],
                    verter_analysis::ModuleReferenceAnalyzability::FiniteSet,
                    dynamic_start,
                    dynamic_start + dynamic_expr.len(),
                ),
            ],
        )
        .expect("resolver snapshot should prepare provider sync");

        let resolved_sources = prepared
            .resolved_dependencies
            .iter()
            .map(|entry| entry.source_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            resolved_sources,
            vec!["/workspace/src/Foo.vue", "/workspace/src/util.ts"],
            "exact and finite-set dependencies should resolve through the native resolver"
        );
        assert!(
            prepared
                .resolved_dependencies
                .iter()
                .any(|entry| entry.provider_specifier == "./Foo.vue.ts"),
            "Vue dependencies should target their provider API paths"
        );
        assert!(
            prepared
                .resolved_dependencies
                .iter()
                .any(|entry| entry.provider_specifier == "./util.ts"),
            "non-Vue workspace dependencies should target provider paths with explicit extension"
        );
        assert!(
            prepared.rewritten.contains("'./Foo.vue.ts'"),
            "exact Vue imports should rewrite through the resolved provider specifier"
        );
        assert!(
            prepared.rewritten.contains("'./util.ts'"),
            "non-Vue workspace imports should rewrite through the resolved provider specifier"
        );
        assert!(
            prepared.rewritten.contains("import(`./${name}.vue`)"),
            "finite-set dynamics must keep the original expression text"
        );
    }

    #[test]
    fn provider_vue_path_helpers_use_original_paths() {
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);

        let ide_path =
            provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", false).unwrap();
        let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

        assert_eq!(
            ide_path, "/workspace/src/App.vue.tsx",
            "Vue IDE path should be canonical_id.tsx"
        );
        assert_eq!(
            api_path, "/workspace/src/App.vue.ts",
            "Vue API path should be canonical_id.ts"
        );
    }

    #[test]
    fn provider_path_helpers_round_trip_through_resolver() {
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        // Host must have the backing .vue source for the collision guard to pass
        let host = VerterHost::new(HostConfig::default());
        host.upsert(verter_host::UpsertRequest {
            canonical_id: Some("/workspace/src/App.vue".to_string()),
            input_id: "/workspace/src/App.vue".to_string(),
            source: "<template><div/></template>".into(),
            file_kind: verter_host::FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        let ide_path =
            provider_ide_path_for_source(&resolver, "/workspace/src/App.vue", true).unwrap();
        let api_path = provider_api_path_for_source(&resolver, "/workspace/src/App.vue").unwrap();

        assert_eq!(
            source_id_from_provider_vue_path(&resolver, &host, &ide_path).as_deref(),
            Some("/workspace/src/App.vue")
        );
        assert_eq!(
            source_id_from_provider_vue_path(&resolver, &host, &api_path).as_deref(),
            Some("/workspace/src/App.vue")
        );
    }

    #[test]
    fn vue_tsx_collision_with_real_file() {
        // A real .vue.tsx file exists but there's no matching .vue source in any project.
        // source_id_from_provider_vue_path should return None (collision guard).
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace/src".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        let host = VerterHost::new(HostConfig::default());

        // "/workspace/src/weird.vue.tsx" has no backing "/workspace/src/weird.vue"
        // registered in any project, so the resolver should not strip the suffix
        assert_eq!(
            source_id_from_provider_vue_path(&resolver, &host, "/other/weird.vue.tsx"),
            None,
            ".vue.tsx with no backing .vue in any project should return None"
        );
    }

    #[test]
    fn vue_tsx_virtual_file_resolves() {
        // A virtual .vue.tsx with a backing .vue source registered in a project.
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        // Host must have the backing .vue source for the collision guard to pass
        let host = VerterHost::new(HostConfig::default());
        host.upsert(verter_host::UpsertRequest {
            canonical_id: Some("/workspace/src/App.vue".to_string()),
            input_id: "/workspace/src/App.vue".to_string(),
            source: "<template><div/></template>".into(),
            file_kind: verter_host::FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();

        assert_eq!(
            source_id_from_provider_vue_path(&resolver, &host, "/workspace/src/App.vue.tsx")
                .as_deref(),
            Some("/workspace/src/App.vue"),
            "virtual .vue.tsx with backing .vue source should resolve to .vue"
        );
    }

    #[test]
    fn vue_tsx_collision_guard_rejects_when_host_missing_source() {
        // The resolver thinks /workspace/src/Real.vue.tsx belongs to the project
        // and strips the suffix to get /workspace/src/Real.vue, but the host
        // has never compiled Real.vue → collision guard must reject.
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        let host = VerterHost::new(HostConfig::default());
        // Do NOT upsert /workspace/src/Real.vue into host

        assert_eq!(
            source_id_from_provider_vue_path(&resolver, &host, "/workspace/src/Real.vue.tsx"),
            None,
            ".vue.tsx in project but no backing .vue in host should return None"
        );
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

    /// Server capabilities must NOT include `diagnostic_provider` (pull diagnostics).
    /// We use push diagnostics exclusively to avoid flickering during typing.
    #[test]
    fn capabilities_do_not_include_pull_diagnostics() {
        let caps = crate::capabilities::server_capabilities(&PositionEncodingKind::UTF16);
        assert!(
            caps.diagnostic_provider.is_none(),
            "diagnostic_provider must be removed — we use push diagnostics only"
        );
    }

    #[test]
    fn did_open_startup_policy_enables_sync_for_tsgo_and_tsserver() {
        let tsgo = did_open_startup_policy(crate::TypeProviderKind::Tsgo);
        assert!(
            tsgo.sync_imported_vue_files,
            "TSGO should eagerly sync imported .vue files"
        );
        assert!(
            !tsgo.publish_diagnostics,
            "should not publish diagnostics inline"
        );

        let tsserver = did_open_startup_policy(crate::TypeProviderKind::Tsserver);
        assert!(
            tsserver.sync_imported_vue_files,
            "tsserver should eagerly sync imported .vue files"
        );
        assert!(
            !tsserver.publish_diagnostics,
            "should not publish diagnostics inline"
        );
    }

    #[test]
    fn did_open_startup_policy_skips_sync_for_no_provider() {
        let none = did_open_startup_policy(crate::TypeProviderKind::None);
        assert!(
            !none.sync_imported_vue_files,
            "no type provider should not eagerly sync imported .vue files"
        );
        assert!(
            !none.publish_diagnostics,
            "should not publish diagnostics inline"
        );
    }

    #[test]
    fn did_open_provider_sync_policy_skips_api_sync_for_tsserver_but_not_tsgo() {
        let tsserver = did_open_provider_sync_policy(crate::TypeProviderKind::Tsserver);
        assert!(
            tsserver.await_ide_sync,
            "tsserver cold open should still await current-file TSX sync"
        );
        assert!(
            !tsserver.await_api_sync,
            "tsserver cold open should not await current-file .vue.ts sync"
        );

        let tsgo = did_open_provider_sync_policy(crate::TypeProviderKind::Tsgo);
        assert!(
            tsgo.await_api_sync,
            "TSGO cold open should continue awaiting API sync"
        );

        let no_provider = did_open_provider_sync_policy(crate::TypeProviderKind::None);
        assert!(
            no_provider.await_ide_sync,
            "the cold-open policy should keep TSX sync enabled regardless of provider kind"
        );
        assert!(
            !no_provider.await_api_sync,
            "verter-only mode should not await API sync"
        );
    }

    #[tokio::test]
    async fn initialized_returns_before_background_configure_paths_completes() {
        let temp_root = std::env::temp_dir().join(format!(
            "verter-lsp-init-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(temp_root.join("src")).expect("temp project should be created");
        std::fs::write(
            temp_root.join("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["src/*"]
    }
  }
}"#,
        )
        .expect("tsconfig should be written");

        let provider = Arc::new(SlowConfigurePathsProvider::default());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let host_for_server = Arc::clone(&host);
        let type_provider_for_server = Arc::clone(&type_provider);
        let (service, socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host_for_server),
                    type_provider: Some(Arc::clone(&type_provider_for_server)),
                    project_sync_mode: crate::ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::Tsserver,
                    suggest_tsgo: false,
                    mcp_port: None,
                    type_provider_none_reason: None,
                },
            )
        });
        let drain_handle = tokio::spawn(async move {
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });

        let server = service.inner();
        server.vite_config_options.lock().await.enabled = false;
        *server.workspace_roots.lock().await = vec![format!(
            "file:///{}",
            temp_root.to_string_lossy().replace('\\', "/")
        )];

        let start = std::time::Instant::now();
        server.initialized(InitializedParams {}).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "initialized() should not wait for configure_paths/background discovery (elapsed {elapsed:?})"
        );

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while provider.configure_paths_started.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("background init should still configure paths after initialized() returns");

        drain_handle.abort();
        drop(service);
    }

    #[test]
    fn collect_imported_vue_priority_ids_keeps_only_resolved_vue_imports() {
        let analysis = verter_analysis::ScriptAnalysisSnapshot {
            imports: vec![
                verter_analysis::AnalyzedImport {
                    source: "./MyComp.vue".to_string(),
                    is_type_only: false,
                    bindings: Vec::new(),
                    span: verter_span::Span::new(0, 0),
                    resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
                },
                verter_analysis::AnalyzedImport {
                    source: "./utils".to_string(),
                    is_type_only: false,
                    bindings: Vec::new(),
                    span: verter_span::Span::new(0, 0),
                    resolved_canonical_id: Some("C:/project/src/utils.ts".to_string()),
                },
                verter_analysis::AnalyzedImport {
                    source: "./Other.vue".to_string(),
                    is_type_only: false,
                    bindings: Vec::new(),
                    span: verter_span::Span::new(0, 0),
                    resolved_canonical_id: None,
                },
                verter_analysis::AnalyzedImport {
                    source: "./MyComp.vue".to_string(),
                    is_type_only: false,
                    bindings: Vec::new(),
                    span: verter_span::Span::new(0, 0),
                    resolved_canonical_id: Some("C:/project/src/MyComp.vue".to_string()),
                },
            ],
            module_references: Vec::new(),
            bindings: Vec::new(),
            macros: Vec::new(),
            macro_type_deps: Vec::new(),
            flags: verter_analysis::AnalysisFlags::empty(),
            exported_functions: Vec::new(),
            vue_api_calls: Vec::new(),
            dom_query_calls: Vec::new(),
            css_var_manipulations: Vec::new(),
            script_binding_occurrences: Vec::new(),
            store_usages: Vec::new(),
            store_definitions: Vec::new(),
            first_await_offset: None,
            type_enhancements: None,
            options_api: None,
            nested_macro_calls: Vec::new(),
        };

        let ids = collect_imported_vue_priority_ids(&analysis);

        assert_eq!(
            ids,
            vec!["C:/project/src/MyComp.vue".to_string()],
            "should keep one resolved .vue canonical id"
        );
        assert!(
            !ids.iter().any(|id| id.ends_with(".ts")),
            "non-Vue imports must be excluded"
        );
    }

    #[test]
    fn did_open_prioritizes_exact_and_finite_dynamic_targets() {
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        let reader = TestResolverReader::with_files(&[
            "/workspace/src/Foo.vue",
            "/workspace/src/Bar.vue",
            "/workspace/src/util.ts",
        ]);
        let targets = collect_priority_vue_targets_from_module_references(
            Some(&ResolverSnapshot {
                generation: 1,
                resolver,
            }),
            &reader,
            "/workspace/src/App.vue",
            &[
                test_analyzed_module_reference(
                    "'./Foo.vue'",
                    Some("./Foo.vue"),
                    &[],
                    verter_analysis::ModuleReferenceAnalyzability::Exact,
                    0,
                    10,
                ),
                test_analyzed_module_reference(
                    "`./${name}.vue`",
                    None,
                    &["./Bar.vue", "./util"],
                    verter_analysis::ModuleReferenceAnalyzability::FiniteSet,
                    11,
                    27,
                ),
            ],
        );

        assert_eq!(
            targets,
            vec![
                "/workspace/src/Foo.vue".to_string(),
                "/workspace/src/Bar.vue".to_string()
            ]
        );
    }

    #[test]
    fn unknown_dynamic_imports_sync_no_provider_dependencies() {
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
            crate::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.app.json".to_string()),
            ),
        ]);
        let reader = TestResolverReader::with_files(&["/workspace/src/Foo.vue"]);
        let targets = collect_priority_vue_targets_from_module_references(
            Some(&ResolverSnapshot {
                generation: 1,
                resolver,
            }),
            &reader,
            "/workspace/src/App.vue",
            &[test_analyzed_module_reference(
                "`./${name}.vue`",
                None,
                &[],
                verter_analysis::ModuleReferenceAnalyzability::UnknownDynamic,
                0,
                15,
            )],
        );

        assert!(
            targets.is_empty(),
            "unknown dynamic imports must not speculate provider dependencies"
        );
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_reaches_child_define_emits() {
        let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
        let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed")
            .expect("component event should resolve");
        let locations = definition_locations(response);
        let target = locations
            .iter()
            .find(|location| location.uri == child_uri)
            .expect("definition should point to MyComp.vue");

        assert_eq!(
            target.range.start.line,
            line_for_snippet(child_source, "custom: [payload: string]"),
            "definition should point to the child defineEmits declaration"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_reaches_child_listener_prop() {
        let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  label: string\n  onAlert?: (payload: string) => void\n}>()\n</script>\n";
        let parent_source = "<script setup lang=\"ts\">\nimport OnEventPropComp from './OnEventPropComp.vue'\nfunction handleAlert(payload: string) {}\n</script>\n<template>\n  <OnEventPropComp label=\"ok\" @alert=\"handleAlert\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/OnEventPropComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/OnEventPropComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed")
            .expect("prop-backed event should resolve");
        let locations = definition_locations(response);
        let target = locations
            .iter()
            .find(|location| location.uri == child_uri)
            .expect("definition should point to OnEventPropComp.vue");

        assert_eq!(
            target.range.start.line,
            line_for_snippet(child_source, "onAlert?: (payload: string) => void"),
            "definition should point to the child listener prop"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_returns_emit_before_listener_prop() {
        let child_source = "<script setup lang=\"ts\">\ndefineProps<{\n  onAlert?: () => void\n}>()\nconst emit = defineEmits<{ alert: [] }>()\n</script>\n";
        let parent_source = "<script setup lang=\"ts\">\nimport BothEventComp from './BothEventComp.vue'\nfunction handleAlert() {}\n</script>\n<template>\n  <BothEventComp @alert=\"handleAlert\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/BothEventComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/BothEventComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@alert=\"handleAlert\"", 1);

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed")
            .expect("event should resolve");
        let locations = definition_locations(response);

        assert_eq!(locations.len(), 2, "should return emit and listener prop");
        assert_eq!(locations[0].uri, child_uri, "emit should resolve in child");
        assert_eq!(
            locations[1].uri, child_uri,
            "listener prop should resolve in child"
        );
        assert_eq!(
            locations[0].range.start.line,
            line_for_snippet(child_source, "alert: []"),
            "defineEmits should come first"
        );
        assert_eq!(
            locations[1].range.start.line,
            line_for_snippet(child_source, "onAlert?: () => void"),
            "listener prop should come second"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_returns_none_when_child_has_no_match() {
        let child_source = "<script setup lang=\"ts\">\ndefineEmits<{ alert: [] }>()\ndefineProps<{ onAlert?: () => void }>()\n</script>\n";
        let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleMissing() {}\n</script>\n<template>\n  <MyComp @missing=\"handleMissing\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@missing=\"handleMissing\"", 1);

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed");

        assert!(
            response.is_none(),
            "unknown child component events should suppress same-file handler fallback"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn resolve_component_document_for_usage_follows_barrel_reexports() {
        let child_source =
            "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
        let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
        let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/components/BarrelComp.vue", "vue", child_source),
                ("src/components/index.ts", "typescript", barrel_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
        let server = service.inner();
        let analysis = server
            .documents
            .get_analysis(&app_uri)
            .expect("parent analysis should exist");
        let template = analysis
            .template
            .as_ref()
            .expect("template analysis should exist");
        let component = template
            .components
            .iter()
            .find(|component| component.name == "BarrelComp")
            .expect("template should include BarrelComp usage");

        assert_eq!(
            component.import_source.as_deref(),
            Some("./components"),
            "template component should retain the raw barrel import source"
        );
        assert_eq!(
            server
                .component_import_binding_name(&analysis, component)
                .as_deref(),
            Some("BarrelComp"),
            "named barrel imports should preserve the local component binding name"
        );

        let parent_canonical_id = uri_to_canonical_id(&app_uri);
        let barrel_canonical_id = server
            .resolve_import_specifier(&parent_canonical_id, "./components")
            .expect("barrel import should resolve to a concrete module");

        assert!(
            barrel_canonical_id.ends_with("/src/components/index.ts"),
            "extensionless barrel imports should resolve to index.ts, got {barrel_canonical_id}"
        );
        assert!(
            server
                .documents
                .host()
                .get_export_span_follow_reexports(&barrel_canonical_id, "BarrelComp", 10)
                .is_some(),
            "barrel export should resolve to the re-exported child"
        );

        let child = server
            .resolve_component_document_for_usage(&app_uri, &analysis, component)
            .expect("component usage should resolve through the barrel");

        assert_eq!(
            child.uri, child_uri,
            "barrel should resolve to the child SFC"
        );
        assert!(
            child.analysis.macros.iter().any(|mac| {
                mac.kind == verter_analysis::AnalyzedMacroKind::DefineEmits
                    && mac.emit_fields.iter().any(|field| field.name == "custom")
            }),
            "resolved child analysis should expose the child's emit declaration"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_handles_barrel_reexports() {
        let child_source =
            "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [] }>()\n</script>\n";
        let barrel_source = "export { default as BarrelComp } from './BarrelComp.vue'\n";
        let parent_source = "<script setup lang=\"ts\">\nimport { BarrelComp } from './components'\nfunction handleCustom() {}\n</script>\n<template>\n  <BarrelComp @custom=\"handleCustom\" />\n</template>\n";
        let (_temp, service, drain_handle, _provider, workspace_id) =
            make_definition_test_server(&[
                ("src/components/BarrelComp.vue", "vue", child_source),
                ("src/components/index.ts", "typescript", barrel_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/components/BarrelComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed")
            .expect("barrel event should resolve");
        let locations = definition_locations(response);
        let target = locations
            .iter()
            .find(|location| location.uri == child_uri)
            .expect("definition should follow the barrel to BarrelComp.vue");

        assert_eq!(
            target.range.start.line,
            line_for_snippet(child_source, "custom: []"),
            "definition should point to the re-exported child emit declaration"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_definition_component_event_name_skips_type_provider_virtual_fallback() {
        let child_source = "<script setup lang=\"ts\">\nconst emit = defineEmits<{ custom: [payload: string] }>()\n</script>\n";
        let parent_source = "<script setup lang=\"ts\">\nimport MyComp from './MyComp.vue'\nfunction handleCustom(payload: string) {}\n</script>\n<template>\n  <MyComp @custom=\"handleCustom\" />\n</template>\n";
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server(&[
                ("src/MyComp.vue", "vue", child_source),
                ("src/App.vue", "vue", parent_source),
            ])
            .await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let child_uri = workspace_uri(&workspace_id, "src/MyComp.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "@custom=\"handleCustom\"", 1);
        let ctx = server
            .type_provider_context(&app_uri)
            .expect("provider context should exist");
        let tsx_offset = merge::vue_position_to_tsx_offset_validated(
            &position,
            &ctx.vue_line_index,
            &ctx.mapper,
            &ctx.tsx_line_index,
        )
        .expect("event position should map into TSX");
        provider.set_definitions(
            &ctx.tsx_path,
            tsx_offset,
            vec![TypeLocation {
                path: ctx.tsx_path.clone(),
                start: 0,
                end: 0,
            }],
        );

        let response = server
            .goto_definition(goto_definition_params(&app_uri, position))
            .await
            .expect("goto definition should succeed")
            .expect("native component event should resolve");
        let locations = definition_locations(response);
        let target = locations
            .iter()
            .find(|location| location.uri == child_uri)
            .expect("native child definition should win");

        assert_eq!(
            target.range.start.line,
            line_for_snippet(child_source, "custom: [payload: string]"),
            "native child definition should be returned instead of the virtual parent file"
        );
        assert!(
            !provider
                .calls()
                .iter()
                .any(|call| matches!(call, MockCall::GetDefinition { .. })),
            "native component event resolution should skip the type provider entirely"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_type_definition_returns_none_without_provider() {
        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let (service, socket) = tower_lsp_server::LspService::new(move |client| {
            VerterLanguageServer::new(
                client,
                LspConfig {
                    host: Arc::clone(&host),
                    type_provider: None,
                    project_sync_mode: crate::ProjectSyncMode::FullProject,
                    type_provider_kind: crate::TypeProviderKind::Tsserver,
                    suggest_tsgo: false,
                    mcp_port: None,
                    type_provider_none_reason: None,
                },
            )
        });
        let drain_handle = tokio::spawn(async move {
            let mut socket = socket;
            while socket.next().await.is_some() {}
        });

        let server = service.inner();
        let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
        let uri: Uri = "file:///test/App.vue".parse().unwrap();
        let _ = server.documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: source.to_string(),
        });

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: 1,
                    character: 6,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let result = server
            .goto_type_definition(params)
            .await
            .expect("handler should not error");

        assert!(
            result.is_none(),
            "type definition should return None without a type provider"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn goto_type_definition_delegates_to_provider() {
        let source = "<script setup lang=\"ts\">\nconst count: number = 0\n</script>\n";
        let (_temp, service, drain_handle, provider, workspace_id) =
            make_definition_test_server(&[("src/App.vue", "vue", source)]).await;

        let app_uri = workspace_uri(&workspace_id, "src/App.vue");
        let server = service.inner();
        let position = find_document_position(server, &app_uri, "count", 0);

        // Set up mock to return a type definition when queried
        if let Some(ctx) = server.type_provider_context(&app_uri) {
            if let Some(tsx_offset) = merge::vue_position_to_tsx_offset_validated(
                &position,
                &ctx.vue_line_index,
                &ctx.mapper,
                &ctx.tsx_line_index,
            ) {
                provider.set_type_definitions(
                    &ctx.tsx_path,
                    tsx_offset,
                    vec![TypeLocation {
                        path: ctx.tsx_path.clone(),
                        start: 0,
                        end: 5,
                    }],
                );
            }
        }

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: app_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let result = server
            .goto_type_definition(params)
            .await
            .expect("handler should not error");

        // Verify the provider was called with get_type_definition (not get_definition)
        assert!(
            provider
                .calls()
                .iter()
                .any(|call| matches!(call, MockCall::GetTypeDefinition { .. })),
            "handler should delegate to get_type_definition on the provider"
        );
        assert!(
            !provider
                .calls()
                .iter()
                .any(|call| matches!(call, MockCall::GetDefinition { .. })),
            "handler should NOT call get_definition"
        );

        // The merge logic should produce a response when the provider returns locations
        assert!(
            result.is_some(),
            "type definition should return locations when provider has results"
        );

        drain_handle.abort();
        drop(service);
    }

    #[tokio::test]
    async fn hover_prefers_child_component_summary_over_import_alias_on_component_tag() {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service(type_provider);
        let server = service.inner();
        install_test_resolver(server);

        let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
        let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp foo="literal" :bar="1" @custom="handler($event)" />
</template>
"#;

        let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

        let mut position = Position {
            line: 5,
            character: 2,
        };
        position.character += 1;

        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "```typescript\n(alias) import MyComp\nimport MyComp\n```",
        );

        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );

        assert!(
            text.contains("Props:"),
            "hover should show props, got: {text}"
        );
        assert!(
            text.contains("foo"),
            "hover should include foo, got: {text}"
        );
        assert!(
            text.contains("string"),
            "hover should include foo type, got: {text}"
        );
        assert!(
            text.contains("bar"),
            "hover should include bar, got: {text}"
        );
        assert!(
            text.contains("number"),
            "hover should include bar type, got: {text}"
        );
        assert!(
            text.contains("Emits:"),
            "hover should show emits, got: {text}"
        );
        assert!(
            text.contains("custom"),
            "hover should include custom emit, got: {text}"
        );
        assert!(
            text.contains("payload"),
            "hover should include payload label, got: {text}"
        );
        assert!(
            !text.contains("(alias) import MyComp"),
            "hover must not prefer import alias hover, got: {text}"
        );
        assert!(
            !text.contains("DefineComponent<{}, {}>"),
            "hover must not degrade to fallback component shell, got: {text}"
        );
    }

    #[tokio::test]
    async fn hover_prefers_child_component_summary_over_import_alias_on_vue_import_binding() {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service(type_provider);
        let server = service.inner();
        install_test_resolver(server);

        let child_source = r#"<script setup lang="ts">
defineProps<{ foo: string; bar: number }>()
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
        let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>

<template>
  <MyComp />
</template>
"#;

        let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

        let position = Position {
            line: 1,
            character: 7,
        };

        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "```typescript\n(alias) import MyComp\nimport MyComp\n```",
        );

        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );

        assert!(
            text.contains("Props:"),
            "hover should show props, got: {text}"
        );
        assert!(
            text.contains("foo"),
            "hover should include foo, got: {text}"
        );
        assert!(
            text.contains("bar"),
            "hover should include bar, got: {text}"
        );
        assert!(
            text.contains("Emits:"),
            "hover should show emits, got: {text}"
        );
        assert!(
            text.contains("custom"),
            "hover should include custom emit, got: {text}"
        );
        assert!(
            !text.contains("(alias) import MyComp"),
            "hover must not prefer import alias hover, got: {text}"
        );
        assert!(
            !text.contains("DefineComponent<{}, {}>"),
            "hover must not degrade to fallback component shell, got: {text}"
        );
    }

    #[tokio::test]
    async fn hover_rewrites_component_event_attr_to_vue_syntax() {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service(type_provider);
        let server = service.inner();
        install_test_resolver(server);

        let child_source = r#"<script setup lang="ts">
const emit = defineEmits<{ custom: [payload: string] }>()
</script>
<template><div /></template>
"#;
        let app_source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <MyComp @custom="handleCustom($event)" />
</template>
"#;

        let _child_uri = open_test_vue(server, "/workspace/src/MyComp.vue", child_source);
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

        let position = Position {
            line: 8,
            character: 11,
        };

        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "```typescript\n(property) onCustom: (payload: string) => void\n```",
        );

        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );

        assert!(
            text.contains("@custom"),
            "hover should use Vue event syntax, got: {text}"
        );
        assert!(
            text.contains("payload"),
            "hover should include payload label, got: {text}"
        );
        assert!(
            text.contains("string"),
            "hover should include payload type, got: {text}"
        );
        assert!(
            !text.contains("onCustom"),
            "hover must not expose TSX on* naming, got: {text}"
        );
        assert!(
            !text.contains(": any"),
            "hover must not degrade to any, got: {text}"
        );
    }

    #[tokio::test]
    async fn hover_rewrites_prop_backed_event_attr_to_vue_syntax() {
        let provider = Arc::new(MockTypeProvider::new());
        let type_provider: Arc<dyn TypeProvider> = provider.clone();
        let service = make_hover_test_service(type_provider);
        let server = service.inner();
        install_test_resolver(server);

        let child_source = r#"<script setup lang="ts">
defineProps<{ label: string; onAlert?: (payload: string) => void }>()
</script>
<template><button>{{ label }}</button></template>
"#;
        let app_source = r#"<script setup lang="ts">
import OnEventPropComp from './OnEventPropComp.vue'
function handleCustom(payload: string) {
  console.log(payload)
}
</script>

<template>
  <OnEventPropComp label="go" @alert="handleCustom" />
</template>
"#;

        let _child_uri = open_test_vue(server, "/workspace/src/OnEventPropComp.vue", child_source);
        let app_uri = open_test_vue(server, "/workspace/src/App.vue", app_source);

        let position = Position {
            line: 8,
            character: 29,
        };

        set_type_hover_at_vue_position(
            server,
            &provider,
            &app_uri,
            position,
            "```typescript\n(property) onAlert?: (payload: string) => void\n```",
        );

        let text = hover_text(
            server
                .hover(hover_params(&app_uri, position))
                .await
                .expect("hover request should succeed"),
        );

        assert!(
            text.contains("@alert"),
            "hover should use Vue event syntax, got: {text}"
        );
        assert!(
            text.contains("payload"),
            "hover should include payload label, got: {text}"
        );
        assert!(
            text.contains("string"),
            "hover should include payload type, got: {text}"
        );
        assert!(
            !text.contains("onAlert"),
            "hover must not expose TSX on* naming, got: {text}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn background_init_drains_pending_snapshot_provider_sync_for_open_vue_file() {
        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let documents = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///workspace/src/App.vue".parse().unwrap();
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template><div /></template>".to_string(),
        });

        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let resolver_snapshot = parking_lot::RwLock::new(Some(ResolverSnapshot {
            generation: 1,
            resolver: crate::project_resolver::NativeProjectResolver::new(vec![
                crate::project_resolver::IdeProjectConfig::new(
                    "/workspace".to_string(),
                    "/workspace".to_string(),
                    Some("/workspace/tsconfig.app.json".to_string()),
                ),
            ]),
        }));
        let provider_sync_states = DashMap::new();
        let pending_snapshot_provider_sync = DashSet::new();
        pending_snapshot_provider_sync.insert("/workspace/src/App.vue".to_string());

        drain_pending_snapshot_provider_sync(
            Some(&sync),
            &documents,
            &resolver_snapshot,
            &provider_sync_states,
            &pending_snapshot_provider_sync,
            false,
            None,
        )
        .await;

        assert!(
            !pending_snapshot_provider_sync.contains("/workspace/src/App.vue"),
            "drained open Vue files should be removed from the pending snapshot queue"
        );

        let state = provider_sync_states
            .get("/workspace/src/App.vue")
            .map(|entry| entry.clone())
            .expect("drained sync should commit owner-aware provider state");
        assert!(
            !state.owner_key.is_empty(),
            "drain must set an owner key on provider state"
        );

        let calls = provider.file_sync_calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::UpdateFile { path, .. } if path.ends_with(".vue.ts")
            )),
            "drain should sync the Vue public API through .vue.ts"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::UpdateFile { path, .. } if path.ends_with(".tsx")
            )),
            "drain should sync the open Vue IDE file through the synthetic TSX path"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sync_pending_vue_provider_file_hydrates_codegen_blockers_before_sync() {
        let temp = tempfile::tempdir().expect("temp dir");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(workspace.join("src/partials")).expect("create partials dir");
        std::fs::write(workspace.join("tsconfig.app.json"), "{}").expect("write tsconfig");
        std::fs::write(
            workspace.join("src/partials/panel.html"),
            "<div>{{ props.msg }}</div>",
        )
        .expect("write external template");
        std::fs::write(
            workspace.join("src/types.ts"),
            "import type { Nested } from '@/nested'\nexport interface Props { msg: Nested }",
        )
        .expect("write types dependency");
        std::fs::write(
            workspace.join("src/nested.ts"),
            "export type Nested = string",
        )
        .expect("write nested dependency");

        let workspace_id = std::fs::canonicalize(&workspace)
            .expect("canonical workspace path")
            .to_string_lossy()
            .replace('\\', "/");
        let app_id = format!("{workspace_id}/src/App.vue");
        let uri = crate::uri::path_to_file_uri(&app_id).expect("file uri");

        let host = Arc::new(VerterHost::new(HostConfig::default()));
        let documents = DocumentRegistry::new(Arc::clone(&host));
        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "vue".to_string(),
            version: 1,
            text: "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>".to_string(),
        });
        let blockers = host
            .get_compile_blockers(&app_id)
            .expect("App.vue should expose compile blockers");

        assert!(
            documents.get_ide(&uri).is_none(),
            "DocumentRegistry alone should still miss IDE output before server hydration"
        );

        let mut project = crate::project_resolver::IdeProjectConfig::new(
            workspace_id.clone(),
            workspace_id.clone(),
            Some(format!("{workspace_id}/tsconfig.app.json")),
        );
        project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
            base_url: Some(workspace_id.clone()),
            paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
        };
        let snapshot = ResolverSnapshot {
            generation: 1,
            resolver: crate::project_resolver::NativeProjectResolver::new(vec![project]),
        };
        let reader = crate::compile_blockers::HostFsProjectResolverReader::new(documents.host());
        let external_resolved = snapshot.resolver.resolve_with_reader(
            &reader,
            &crate::project_resolver::ResolveRequest {
                importer_id: app_id.clone(),
                specifier: "@/partials/panel.html".to_string(),
                kind: crate::project_resolver::ResolveRequestKind::SfcSrcAttr,
                phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
            },
        );
        let type_resolved = snapshot.resolver.resolve_with_reader(
            &reader,
            &crate::project_resolver::ResolveRequest {
                importer_id: app_id.clone(),
                specifier: "@/types".to_string(),
                kind: crate::project_resolver::ResolveRequestKind::TypeImport,
                phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
            },
        );
        assert!(
            external_resolved.is_some(),
            "external src specifier should resolve through the native resolver"
        );
        assert!(
            type_resolved.is_some(),
            "macro type specifier should resolve through the native resolver"
        );
        let external_resolved = external_resolved.expect("external resolve result");
        let type_resolved = type_resolved.expect("type resolve result");
        assert!(
            external_resolved
                .source_id
                .ends_with("/src/partials/panel.html"),
            "external src should resolve to the real template file: {:?}",
            external_resolved
        );
        assert!(
            type_resolved.source_id.ends_with("/src/types.ts"),
            "macro type dep should resolve to the real types file: {:?}",
            type_resolved
        );
        assert!(
            reader.read_text(&external_resolved.source_id).is_some(),
            "reader should load external src text from {:?}",
            external_resolved
        );
        assert!(
            reader.read_text(&type_resolved.source_id).is_some(),
            "reader should load macro type text from {:?}",
            type_resolved
        );
        let provider = Arc::new(MockTypeProvider::new());
        let sync = ProjectSync::new(provider.clone(), ProjectSyncMode::FullProject);
        let provider_sync_states = DashMap::new();

        let synced = sync_pending_vue_provider_file(
            &sync,
            &documents,
            &snapshot,
            &provider_sync_states,
            &app_id,
            false,
        )
        .await;

        assert!(
            synced,
            "pending Vue sync should succeed after blocker hydration"
        );
        assert!(
            host.get_source(&format!("{workspace_id}/src/partials/panel.html"))
                .is_some(),
            "external src files should be loaded into the host during hydration; blockers={blockers:?} files={:?}",
            host.list_files()
        );
        assert!(
            host.get_source(&format!("{workspace_id}/src/types.ts"))
                .is_some(),
            "macro type dependencies should be loaded into the host during hydration"
        );
        assert!(
            host.get_source(&format!("{workspace_id}/src/nested.ts"))
                .is_some(),
            "transitive codegen dependencies should be loaded into the host during hydration"
        );

        let profile = documents.tsx_profile.read().clone();
        assert!(
            host.get_ide(&app_id, &profile).is_some(),
            "hydrated pending sync should restore IDE output for the Vue file"
        );

        let calls = provider.file_sync_calls();
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::UpdateFile { path, .. } if path.ends_with(".vue.ts")
            )),
            "pending sync should push the provider-facing Vue API file"
        );
        assert!(
            calls.iter().any(|call| matches!(
                call,
                MockCall::UpdateFile { path, .. } if path.ends_with(".tsx")
            )),
            "pending sync should push the hydrated TSX output"
        );
    }

    // ── wants_code_action_kind tests ────────────────────────────────

    #[test]
    fn test_wants_code_action_kind_no_filter() {
        // No `only` → all kinds wanted
        assert!(wants_code_action_kind(None, "quickfix"));
        assert!(wants_code_action_kind(None, "source.organizeImports"));
        assert!(wants_code_action_kind(None, "refactor.extract"));
    }

    #[test]
    fn test_wants_code_action_kind_exact_match() {
        let kinds = vec![CodeActionKind::new("quickfix")];
        assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
        assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
        assert!(!wants_code_action_kind(
            Some(&kinds),
            "source.organizeImports"
        ));
    }

    #[test]
    fn test_wants_code_action_kind_prefix_hierarchy() {
        // `only: [refactor]` should match `refactor.extract`
        let kinds = vec![CodeActionKind::new("refactor")];
        assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
        assert!(wants_code_action_kind(Some(&kinds), "refactor"));
        assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));

        // `only: [refactor.extract]` should match `refactor` (parent)
        let kinds = vec![CodeActionKind::new("refactor.extract")];
        assert!(wants_code_action_kind(Some(&kinds), "refactor"));
        assert!(wants_code_action_kind(Some(&kinds), "refactor.extract"));
        assert!(!wants_code_action_kind(Some(&kinds), "quickfix"));
    }

    #[test]
    fn test_wants_code_action_kind_no_false_prefix() {
        // "quickfixExtra" should NOT match "quickfix"
        let kinds = vec![CodeActionKind::new("quickfix")];
        assert!(!wants_code_action_kind(Some(&kinds), "quickfixExtra"));

        // "refactoring" should NOT match "refactor"
        let kinds = vec![CodeActionKind::new("refactor")];
        assert!(!wants_code_action_kind(Some(&kinds), "refactoring"));
    }

    #[test]
    fn test_wants_code_action_kind_multiple_kinds() {
        let kinds = vec![
            CodeActionKind::new("quickfix"),
            CodeActionKind::new("source.organizeImports"),
        ];
        assert!(wants_code_action_kind(Some(&kinds), "quickfix"));
        assert!(wants_code_action_kind(
            Some(&kinds),
            "source.organizeImports"
        ));
        assert!(!wants_code_action_kind(Some(&kinds), "refactor"));
    }

    // ── File watcher helper tests ──────────────────────────────────

    #[test]
    fn test_is_config_file_positive() {
        assert!(is_config_file("file:///project/tsconfig.json"));
        assert!(is_config_file("file:///project/tsconfig.app.json"));
        assert!(is_config_file("file:///project/tsconfig.node.json"));
        assert!(is_config_file("file:///project/.verterrc.json"));
        assert!(is_config_file("file:///project/vite.config.ts"));
        assert!(is_config_file("file:///project/vite.config.js"));
        assert!(is_config_file("file:///project/vite.config.mjs"));
        assert!(is_config_file("file:///project/vite.config.cjs"));
        assert!(is_config_file("file:///project/vite.config.mts"));
        assert!(is_config_file("file:///project/vite.config.cts"));
        assert!(is_config_file("file:///project/package.json"));
    }

    #[test]
    fn test_is_config_file_negative() {
        assert!(!is_config_file("file:///project/src/App.vue"));
        assert!(!is_config_file("file:///project/src/utils.ts"));
        assert!(!is_config_file("file:///project/src/config.ts"));
        assert!(!is_config_file("file:///project/tsconfig-paths.ts"));
        assert!(!is_config_file("file:///project/my.config.ts"));
        assert!(!is_config_file("file:///project/verterrc.json"));
    }

    #[test]
    fn test_is_config_file_windows_paths() {
        // Canonical IDs on Windows use forward slashes
        assert!(is_config_file("C:/project/tsconfig.json"));
        assert!(is_config_file("C:/project/package.json"));
        assert!(is_config_file("C:/project/.verterrc.json"));
        assert!(!is_config_file("C:/project/src/App.vue"));
    }

    #[test]
    fn test_is_vue_file() {
        assert!(is_vue_file("file:///project/src/App.vue"));
        assert!(is_vue_file("C:/project/src/App.vue"));
        assert!(!is_vue_file("file:///project/src/utils.ts"));
        assert!(!is_vue_file("file:///project/tsconfig.json"));
        assert!(!is_vue_file("file:///project/vue.config.js"));
    }
}
