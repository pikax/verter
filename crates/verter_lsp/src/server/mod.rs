use std::{collections::HashSet, sync::Arc};

use dashmap::{DashMap, DashSet};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::documents::{uri_to_canonical_id, DocumentRegistry};
use crate::features::cursor_context::ExpressionContext;
use crate::features::diagnostics::map_diagnostics;
use crate::provider_sync::{
    commit_sync_transition, genuinely_stale_after_sync, open_unresolved_carrier_commit,
    open_unresolved_carrier_state, prepare_sync_transition, revert_unsynced_kinds,
    ProviderPathKind, ProviderSyncState,
};
use crate::statistics::Statistics;
use crate::type_provider::project_sync::ProjectSync;
use crate::type_provider::traits::TypeProvider;
use crate::LspConfig;

// `server_tests.rs` (a child of `server`, included via `#[path]`) uses
// `super::*` to bring server-scope identifiers into its tests. Keep
// these imports glob-visible in `mod.rs` for tests-only consumption.
#[cfg(test)]
#[allow(unused_imports)]
use crate::features::cursor_context::{
    classify_cursor_context, classify_expression_context_with_trigger, CursorContext,
    TemplateCursorContext,
};
#[cfg(test)]
#[allow(unused_imports)]
use crate::type_provider::merge;

// ── Handler tracking for freeze diagnosis ──────────────────────────────
// Moved to `handler_guard.rs`. Imported here so the
// `#[path = "../background_init.rs"]` sibling (which references
// ACTIVE_HANDLERS and block_in_place_if_available via `use super::*`
// or implicit module-level lookup) compiles.
mod handler_guard;
#[allow(unused_imports)]
use self::handler_guard::{block_in_place_if_available, ACTIVE_HANDLERS};
// Re-export the runtime-flavor-guarded blocking helper so sibling top-level
// modules (e.g. the background `sync_coordinator` / `background_init` diagnostic
// publish paths) can route VFS source reads through the same guard the server
// handlers use, instead of an unguarded `tokio::task::block_in_place`.
pub(crate) use self::handler_guard::block_in_place_if_available as block_in_place_guarded;

// Provider-sync state CRUD + context helpers. Inherent-impl
// extension methods on `VerterLanguageServer` covering MRU bookkeeping,
// snapshot-pending queue, sync-state CRUD, type-provider context, and
// virtual-file routing context.
mod provider_state;

// Component contract resolution. Inherent-impl extension
// methods on `VerterLanguageServer` covering import-specifier resolution,
// child-component document/context building, barrel re-export following,
// and template-contract definition resolution (props, events, v-model,
// slots).
mod component_resolve;

// Cross-file `<Child prop=…>` rename resolution. Inherent-impl extension
// methods on `VerterLanguageServer` plus the shared classification types
// (`ChildPropUsage`/`ChildPropRenameClass`/`ChildPropDeclarationProof`), covering
// the SHARED prop-usage resolution (also used by the goto-definition props branch),
// inline macro-field declaration resolution, the imported-type declaration
// `get_definition` upgrade hop, and the rename classification the merged-edit
// completeness gate consumes.
mod child_prop_rename;

// Provider-sync orchestration. Inherent-impl extension
// methods on `VerterLanguageServer` covering diagnostics publishing,
// IDE/API/non-carrier sync, ensure_*_synced family, unresolved (pre-snapshot) sync,
// target_ide_path helpers, and the background-init bootstrap.
mod sync_orchestration;

// Custom LSP protocol handlers. The 13 `pub async fn`
// methods invoked via `main.rs` `.custom_method("$/...",
// VerterLanguageServer::<method>)` registrations. Public visibility on
// each method preserves the inherent-method path
// `VerterLanguageServer::<method>` regardless of which impl block hosts
// the body (Rust resolves method paths across all impl blocks in the
// defining crate).
mod custom_methods;

// LSP lifecycle. Trait methods on `LanguageServer for
// VerterLanguageServer` covering initialize, initialized, shutdown,
// did_open, did_change, did_close, did_save,
// did_change_workspace_folders, did_change_watched_files,
// did_create_files, did_delete_files. The trait impl block stays in
// mod.rs; each method delegates to a `handle_*` free function in the
// sibling.
mod lifecycle;

// LSP auxiliary feature handlers. Trait methods on
// `LanguageServer for VerterLanguageServer` covering document_symbol,
// folding_range, selection_range, document_highlight, signature_help,
// code_action, semantic_tokens_full, code_lens, inlay_hint,
// linked_editing_range, document_link, document_color,
// color_presentation, formatting, on_type_formatting, symbol,
// prepare_call_hierarchy, incoming_calls, outgoing_calls. The trait
// impl block stays in mod.rs; each method delegates to a
// `handle_*` free function in the sibling.
mod aux_features;

// LSP navigation feature handlers. Trait methods on
// `LanguageServer for VerterLanguageServer` covering hover, completion,
// completion_resolve.
mod nav_features;

// Navigation method bodies that map source positions onto the generated
// artifact and back: goto_definition, goto_type_definition, references,
// prepare_rename, rename.
mod nav_features_navigation;

// Completion-resolve auto-import edit translation:
// `resolve_provider_auto_import_edits` and `completion_resolve_error`, called
// by `nav_features::handle_completion_resolve`.
mod nav_features_completion_resolve;

// Hover-provenance enrichment: `enrich_hover_with_provenance` and its
// `append_markdown` hover-suffix helper, called by
// `nav_features::handle_hover`.
mod nav_features_hover_provenance;

// Audit-aware wrappers for the navigation feature handlers. Each
// `handle_<method>_with_audit` thunks into the matching plain
// `handle_<method>` body via `crate::audit_harness::run_with_audit`.
// The trait impl in this file calls the `*_with_audit` variants so
// audited LSP requests carry the per-method timeout budget,
// cancellation marker, and records-store publication on the
// production code path.
mod nav_features_audit;

#[path = "../protocol_types.rs"]
pub(crate) mod protocol_types;
pub use self::protocol_types::*;

#[path = "../server_utils.rs"]
mod server_utils;
use self::server_utils::*;
pub(crate) use self::server_utils::{
    adapter_module_language_for, carrier_language_for, compute_verter_diagnostics_for_with_views,
    is_default_export_component_carrier, prepare_non_carrier_provider_sync,
    self_file_provider_content, sync_self_file_shadow_state,
};

#[path = "../background_drain.rs"]
mod background_drain;
#[path = "../background_init.rs"]
mod background_init;
// Glob re-export so `server_tests.rs` (a child of `server`) sees
// `drain_pending_snapshot_provider_sync`, `sync_pending_carrier_provider_file`,
// `is_generated_verter_types_event`, etc. via its `use super::*;`.
pub(crate) use self::background_drain::configure_provider_paths_for_source;
#[cfg(test)]
use self::background_drain::*;
#[cfg(test)]
use self::background_init::*;

/// Lightweight snapshot of the published resolver, replacing the old `ResolverSnapshot`.
///
/// Preserves the `.resolver` field access pattern so callers don't need deep changes.
#[derive(Debug, Clone)]
pub(crate) struct PublishedResolverSnapshot {
    pub(crate) resolver: crate::project_resolver::NativeProjectResolver,
    /// `true` after `background_init` publishes a real snapshot with the
    /// full project graph. `false` during bootstrap (empty resolver).
    pub(crate) ownership_ready: bool,
}

/// Pre-extracted data for type provider calls.
/// All DashMap guards are dropped before this is constructed, so it is safe
/// to hold across `.await` points without risking deadlock.
pub(crate) struct TypeProviderContext {
    pub(crate) tsx_path: String,
    pub(crate) tsx_content: Arc<str>,
    pub(crate) mapper: ProviderPositionMapper,
    pub(crate) tsx_line_index: LineIndex,
    pub(crate) carrier_line_index: LineIndex,
}

/// The generalized per-document provider-projection query context, serving BOTH
/// the carrier-IDE projection and the self-file rune-module projection. The
/// SOLE query path for a document's provider buffer (no parallel rune path).
pub(crate) struct ProviderProjectionContext {
    /// The path the TypeProvider opened: the carrier IDE path, or the rune
    /// module's OWN canonical id (self-file provider buffer served from its own
    /// path).
    pub(crate) provider_path: String,
    /// The bytes the TypeProvider type-checks (IDE TSX, or `<rune prelude> +
    /// <rewritten module bytes>`).
    pub(crate) provider_content: Arc<str>,
    /// The unified source↔provider position mapper (projection-agnostic).
    pub(crate) mapper: ProviderPositionMapper,
    /// Line index over [`Self::provider_content`].
    pub(crate) provider_line_index: LineIndex,
    /// Line index over the user source.
    pub(crate) source_line_index: LineIndex,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedNonCarrierProviderSync {
    pub(crate) provider_path: String,
    pub(crate) rewritten: String,
    pub(crate) resolved_dependencies: Vec<crate::project_resolver::ResolveResult>,
}

pub(crate) struct ResolvedComponentDocument {
    pub(crate) uri: Uri,
    pub(crate) analysis: verter_session::FileAnalysisSnapshot,
    pub(crate) line_index: LineIndex,
}

/// The Verter language server implementation.
///
/// Wraps `verter_session` for SFC analysis and optionally a `TypeProvider`
/// (e.g., TSGO) for richer type information.
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
    vite_config_options: tokio::sync::Mutex<verter_workspace::ViteConfigOptions>,
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
    /// The cross-file-rename provider FENCE. A real (non-advisory) async mutex
    /// the rename transaction holds across its sync-before-query →
    /// snapshot-capture → provider-query → response-parse, so a sync it issues
    /// writes its serialized provider command before the rename request is sent,
    /// and no other rename transaction interleaves its own surface mutations
    /// mid-capture. Concurrent background syncs remain harmless because the
    /// captured snapshots are immutable historical.
    rename_provider_fence: Arc<tokio::sync::Mutex<()>>,
    /// Which type provider backend is active (TSGO, tsserver, or none).
    type_provider_kind: crate::TypeProviderKind,
    /// When `true`, show a recommendation to switch to TSGO in VS Code settings.
    suggest_tsgo: bool,
    /// TEST SEAM: when `true`, suppress the `did_open` imported-carrier-API
    /// prewarm so a cross-file-rename lane can exercise the path where only
    /// `handle_rename`'s own sync-before-query would sync a closed child's API
    /// surface. That lane is `#[ignore]`'d (the Block H-membership tsserver
    /// program-membership gap): suppression does NOT prove `handle_rename`'s own
    /// sync closes the closed child today.
    suppress_imported_carrier_prewarm: bool,
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
    vfs_workspace: Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    /// Opt-in flag for the provenance-enriched hover surface. Default
    /// `false`. Read-only after `initialize()` sets it from
    /// `initializationOptions.hover.provenance`.
    hover_provenance_enabled: std::sync::atomic::AtomicBool,
    /// LRU-100 cache of provenance-enriched hover payloads. Entries
    /// are invalidated on `textDocument/didChange` for the matching
    /// canonical (transitive deps NOT invalidated — codified
    /// limitation).
    hover_provenance_cache: Arc<crate::features::hover_provenance::HoverProvenanceCache>,
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
        let vfs_workspace: Arc<
            parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
        > = Arc::new(parking_lot::RwLock::new(None));

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
                verter_workspace::ViteConfigOptions::default(),
            ),
            inlay_hints_enabled: std::sync::atomic::AtomicBool::new(true),
            cached_verter_diags,
            provider_sync_states,
            rename_provider_fence: Arc::new(tokio::sync::Mutex::new(())),
            type_provider_kind: config.type_provider_kind,
            suggest_tsgo: config.suggest_tsgo,
            suppress_imported_carrier_prewarm: config.suppress_imported_carrier_prewarm,
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
            hover_provenance_enabled: std::sync::atomic::AtomicBool::new(false),
            hover_provenance_cache: Arc::new(
                crate::features::hover_provenance::HoverProvenanceCache::new(),
            ),
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

    /// Install a VFS workspace (test harness access).
    pub(crate) fn install_vfs_workspace(
        &self,
        workspace: Arc<verter_workspace::FilesystemWorkspace>,
    ) {
        *self.vfs_workspace.write() = Some(workspace);
    }

    /// RAW (unmerged) provider code actions for a carrier URI + range + editor diagnostics — the
    /// provider's `getCodeFixes` output BEFORE `merge_code_actions` (test harness access). Lets a
    /// canary tell "provider emitted nothing" from "provider emitted but merge dropped it".
    pub(crate) async fn test_raw_provider_code_actions(
        &self,
        uri: &tower_lsp_server::ls_types::Uri,
        range: tower_lsp_server::ls_types::Range,
        diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
    ) -> Vec<crate::type_provider::protocol::TypeCodeAction> {
        aux_features::raw_provider_code_actions(self, uri, range, diagnostics).await
    }
}

impl LanguageServer for VerterLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        lifecycle::handle_initialize(self, params).await
    }

    async fn initialized(&self, params: InitializedParams) {
        lifecycle::handle_initialized(self, params).await
    }

    async fn shutdown(&self) -> Result<()> {
        lifecycle::handle_shutdown(self).await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        lifecycle::handle_did_open(self, params).await
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        lifecycle::handle_did_change(self, params).await
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        lifecycle::handle_did_close(self, params).await
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        lifecycle::handle_did_save(self, params).await
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        lifecycle::handle_did_change_workspace_folders(self, params).await
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        lifecycle::handle_did_change_watched_files(self, params).await
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        lifecycle::handle_did_create_files(self, params).await
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        lifecycle::handle_did_delete_files(self, params).await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        nav_features_audit::handle_hover_with_audit(self, params).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        nav_features_audit::handle_completion_with_audit(self, params).await
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        nav_features::handle_completion_resolve(self, item).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        nav_features_audit::handle_goto_definition_with_audit(self, params).await
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        nav_features_navigation::handle_goto_type_definition(self, params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        nav_features_audit::handle_references_with_audit(self, params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        nav_features_navigation::handle_prepare_rename(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        nav_features_audit::handle_rename_with_audit(self, params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        aux_features::handle_document_symbol_with_audit(self, params).await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        aux_features::handle_folding_range(self, params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        aux_features::handle_selection_range(self, params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        aux_features::handle_document_highlight(self, params).await
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        aux_features::handle_signature_help(self, params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        aux_features::handle_code_action_with_audit(self, params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        aux_features::handle_semantic_tokens_full_with_audit(self, params).await
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        aux_features::handle_code_lens(self, params).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        aux_features::handle_inlay_hint_with_audit(self, params).await
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        aux_features::handle_linked_editing_range(self, params).await
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        aux_features::handle_document_link(self, params).await
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        aux_features::handle_document_color(self, params).await
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        aux_features::handle_color_presentation(self, params).await
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        aux_features::handle_formatting(self, params).await
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        aux_features::handle_on_type_formatting(self, params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        aux_features::handle_symbol(self, params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        aux_features::handle_prepare_call_hierarchy(self, params).await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        aux_features::handle_incoming_calls(self, params).await
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        aux_features::handle_outgoing_calls(self, params).await
    }
}

#[cfg(test)]
#[allow(
    unused_must_use,
    clippy::unused_enumerate_index,
    clippy::too_many_arguments,
    clippy::cloned_ref_to_slice_refs
)]
#[path = "../server_tests.rs"]
mod server_tests;
