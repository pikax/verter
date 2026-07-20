use std::{collections::HashSet, sync::Arc};

use dashmap::{DashMap, DashSet};
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::documents::{uri_to_canonical_id, DocumentRegistry};
use crate::features::cursor_context::ExpressionContext;
use crate::features::diagnostics::map_diagnostics;
use crate::provider_sync::{
    commit_sync_transition, genuinely_stale_after_sync, non_decl_close_targets,
    open_unresolved_carrier_commit, open_unresolved_carrier_state, prepare_sync_transition,
    revert_unsynced_kinds, NonDeclProviderPathKind, ProviderPathKind, ProviderSyncState,
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
mod nav_features_css;
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
    attr_name_match_rank, carrier_language_for, compute_verter_diagnostics_for_with_views,
    is_default_export_component_carrier, prepare_non_carrier_provider_sync,
    select_best_ranked_candidate, self_file_language_for, sync_self_file_shadow_state,
    to_pascal_case,
};

#[path = "../background_drain.rs"]
mod background_drain;
#[path = "../background_drain_decl_closure.rs"]
mod background_drain_decl_closure;
#[path = "../background_init.rs"]
mod background_init;
// Glob re-export so `server_tests.rs` (a child of `server`) sees
// `drain_pending_snapshot_provider_sync`, `sync_pending_carrier_provider_file`,
// `is_generated_verter_types_event`, etc. via its `use super::*;`.
pub(crate) use self::background_drain::configure_provider_paths_for_source;
// The declaration-overlay lifecycle owner is reached by the drain
// (`background_drain`), the server struct, and the `did_close` lifecycle —
// glob-export it at module scope so all three resolve the bare name.
#[cfg(test)]
use self::background_drain::*;
pub(crate) use self::background_drain_decl_closure::DeclOverlayOwner;
// `carrier_dependency_ids` is asserted by the dual-resolution-rail unit test;
// `DeclCloseTarget` is named only by the lifecycle regression tests.
#[cfg(test)]
pub(crate) use self::background_drain_decl_closure::{carrier_dependency_ids, DeclCloseTarget};
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
    /// The captured immutable provider surface every field above was built
    /// from. Handlers re-validate it AFTER the provider await (via
    /// `provider_context_still_valid`) and DROP the provider contribution on a
    /// mismatch — a response produced against a superseded surface must never
    /// be mapped/published (fail closed).
    pub(crate) snapshot: Arc<crate::provider_surface_store::ProviderSurfaceSnapshot>,
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
    /// The captured immutable provider surface every field above was built
    /// from — the post-await re-validation identity (fail closed on mismatch).
    pub(crate) snapshot: Arc<crate::provider_surface_store::ProviderSurfaceSnapshot>,
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

/// One generation-aware IDE-sync repair lane. Retirement belongs to the lane
/// object, never merely to its canonical-id key, so a stale close cannot retire
/// a reopened document's replacement lane (the key-reuse/ABA case).
struct IdeSyncRepairLane {
    mutex: tokio::sync::Mutex<()>,
    generation: std::sync::atomic::AtomicU64,
    retired: std::sync::atomic::AtomicBool,
}

impl IdeSyncRepairLane {
    fn new(generation: u64) -> Self {
        Self {
            mutex: tokio::sync::Mutex::new(()),
            generation: std::sync::atomic::AtomicU64::new(generation),
            retired: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// One participant in a document's generation-bound IDE-sync repair lane. A
/// closed lane is retired synchronously by the final participant's drop, so
/// cleanup is event-driven and never needs a polling task.
struct IdeSyncRepairLease {
    canonical_id: String,
    lane: Arc<IdeSyncRepairLane>,
    lanes: Arc<DashMap<String, Arc<IdeSyncRepairLane>>>,
}

impl IdeSyncRepairLease {
    async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.lane.mutex.lock().await
    }

    fn retire(&self) {
        self.lane
            .retired
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn lane(&self) -> &Arc<IdeSyncRepairLane> {
        &self.lane
    }
}

impl Drop for IdeSyncRepairLease {
    fn drop(&mut self) {
        if !self.lane.retired.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        self.lanes.remove_if(&self.canonical_id, |_, current| {
            Arc::ptr_eq(current, &self.lane) && Arc::strong_count(&self.lane) == 2
        });
    }
}

#[cfg(test)]
struct IdeSyncPausePoint {
    canonical_id: String,
    arrived: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

/// One ordered provider-publication turn assigned while the document commit
/// fence is held. Waiting happens only after the commit is visible, so slow
/// provider I/O cannot delay later registry commits. The drop notification also
/// makes cancellation skip a turn instead of wedging every later edit.
struct DidChangeProviderTurn {
    predecessor: Option<Arc<tokio::sync::Notify>>,
    completion: Arc<tokio::sync::Notify>,
}

impl DidChangeProviderTurn {
    async fn wait(&self) {
        if let Some(predecessor) = &self.predecessor {
            predecessor.notified().await;
        }
    }
}

impl Drop for DidChangeProviderTurn {
    fn drop(&mut self) {
        self.completion.notify_one();
    }
}

#[cfg(test)]
struct CompletionSnapshotPause {
    arrived: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
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
    /// The proactive declaration-overlay lifecycle owner: the SOLE authority for
    /// the `.d.<ext>.ts` overlay graph (reachability folded with the per-overlay
    /// close generation) and the only code that issues a provider `close_dts` for a
    /// declaration overlay. The drain's closure pass opens/reconciles overlays
    /// through it; the `did_close` lifecycle releases a closed root through it. The
    /// per-declaration-path serialization lock inside the owner makes a stale close
    /// unable to clobber a concurrent open of the same overlay (TS2307 stranding) or
    /// resurrect one no live root reaches. See [`DeclOverlayOwner`].
    decl_overlay_owner: Arc<DeclOverlayOwner>,
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
    /// TEST SEAM: when `true`, suppress the `did_open` imported-carrier-API
    /// prewarm so a cross-file-rename lane can exercise the path where only
    /// `handle_rename`'s own sync-before-query would sync a closed child's API
    /// surface. That lane is `#[ignore]`'d (the Block H-membership tsserver
    /// program-membership gap): suppression does NOT prove `handle_rename`'s own
    /// sync closes the closed child today.
    suppress_imported_carrier_prewarm: bool,
    /// E2E attribution seam for TypeScript-provider completion parity. Verter still
    /// owns carrier generation, synchronization, and source mapping, but its native
    /// completion producer contributes no items and cannot act as a provider fallback.
    ///
    /// This is deliberately gated by both `VERTER_E2E_TEST=1` and
    /// `VERTER_E2E_PROVIDER_ONLY_COMPLETIONS=1`; normal product launches cannot enable
    /// it accidentally through a single generic environment variable.
    provider_only_completions: std::sync::atomic::AtomicBool,
    /// Canonical IDs needing **interactive IDE sync** (set by did_change, cleared by
    /// `ensure_current_file_synced`). Only the IDE TSX path is flushed on hover/completion.
    needs_ide_sync: Arc<DashSet<String>>,
    /// Per-document singleflight for the interactive IDE-sync repair
    /// (`ensure_current_file_synced`). A hover/completion/definition storm on one
    /// document must coalesce into ONE repair, not N concurrent foreground repairs
    /// stampeding the provider (recompile + carrier gateway + sync per request).
    /// The guard serializes repairs per canonical id; a waiter re-checks freshness
    /// after acquiring it and returns without re-repairing when a concurrent repair
    /// already made the document fresh.
    ide_sync_repair_locks: Arc<DashMap<String, Arc<IdeSyncRepairLane>>>,
    /// Current open-document generation per canonical ID. A repair captures this
    /// before lane acquisition and revalidates it after locking; close removes
    /// only its exact generation, so reopen/key reuse cannot be mistaken for the
    /// document instance that initiated stale work.
    ide_sync_open_generations: Arc<DashMap<String, u64>>,
    ide_sync_next_generation: std::sync::atomic::AtomicU64,
    /// Per-document import-set sync memo (former B13). Records the workspace
    /// `(content_generation, resolver_snapshot_generation)` after a successful
    /// imported-carrier + barrel preamble, so a go-to-definition storm on an
    /// unchanged document skips the per-request import-graph BFS re-walk +
    /// carrier gateway reconcile entirely. Both generations are safe superset
    /// signals: ANY content edit (this doc OR a dependency) bumps
    /// `content_generation`, and any resolver re-publish bumps the snapshot
    /// generation, so a stale skip is impossible.
    import_sync_memo: Arc<DashMap<String, (u64, u64)>>,
    /// Per-document singleflight for the import-set preamble: concurrent
    /// definition/completion requests on one document coalesce onto ONE pass
    /// instead of stampeding duplicate syncs of shared UI-kit carriers.
    import_sync_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    #[cfg(test)]
    ide_sync_before_lease_pause: parking_lot::Mutex<Option<IdeSyncPausePoint>>,
    #[cfg(test)]
    ide_sync_after_lease_pause: parking_lot::Mutex<Option<IdeSyncPausePoint>>,
    #[cfg(test)]
    ide_sync_close_after_lock_pause: parking_lot::Mutex<Option<IdeSyncPausePoint>>,
    /// Canonical IDs needing **deferred API/.vue.ts sync** + owner-aware reconciliation.
    /// Set by did_change and by the interactive path (when API is deferred).
    /// Cleared by the coordinator's debounced sync after a resolver snapshot exists.
    needs_deferred_sync: Arc<DashSet<String>>,
    /// Source IDs whose provider sync depends on a resolver snapshot that is not ready yet.
    /// Drained after background initialization commits a new snapshot.
    pending_snapshot_provider_sync: Arc<DashSet<String>>,
    /// Handle for the SyncCoordinator — replaces the spawn-per-keystroke debounce.
    /// Signals are sent per keystroke; the coordinator coalesces them and syncs
    /// after 300ms of silence. Always spawned: the debounced PUBLISH half
    /// (Verter-owned lint / unused-declaration / template diagnostics) never
    /// depends on an in-process provider — routes without one (the
    /// editor-owned tsserver plugin, verter-only mode) still publish on
    /// open/change; only the provider-sync half is provider-gated.
    sync_coordinator: crate::sync_coordinator::SyncCoordinatorHandle,
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
    /// Tail of the ordered provider-publication chain. Each `did_change`
    /// appends a turn while holding `did_change_mutex`, then releases the commit
    /// fence before awaiting its predecessor. This keeps provider writes ordered
    /// without allowing blocked provider I/O to stall registry commits.
    did_change_provider_tail: parking_lot::Mutex<Option<Arc<tokio::sync::Notify>>>,
    #[cfg(test)]
    completion_snapshot_pauses:
        parking_lot::Mutex<std::collections::VecDeque<CompletionSnapshotPause>>,
    #[cfg(test)]
    completion_before_final_pause: parking_lot::Mutex<Option<CompletionSnapshotPause>>,
    #[cfg(test)]
    completion_final_snapshot_pause: parking_lot::Mutex<Option<CompletionSnapshotPause>>,
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
    /// Selection provenance, or why no provider could be started. Sent via
    /// `$/verter/typeProviderStatus`.
    type_provider_reason: Option<String>,
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
    /// The live carrier-publish coordinator for the tsserver engine — the seam
    /// that makes a framework carrier a member of its REAL configured project by
    /// publishing its companions into the on-disk store the
    /// `@verter/typescript-plugin` reads. `Some` only when the active provider is
    /// tsserver: for tsserver the carrier companions reach the engine through the
    /// store + plugin membership (NOT a direct `provider.open_file`); for tsgo the
    /// carrier companions reach the engine through the project-bound `--api` direct
    /// open (`open_project` + `root_files`). The backend it holds resolves the SAME
    /// store dir the tsserver spawn delivers to the plugin.
    carrier_publish_coordinator: Option<crate::external_ts::CarrierPublishCoordinator>,
    /// The per-source carrier transaction coordinator — the SINGLE authority for carrier
    /// provider-state admission (the receipt-gated commit + IDE-surface stamp), the
    /// owner-loss barrier (the tombstone that survives a state removal), and the non-owned
    /// retry disposition (requeue / owner-loss barrier advance). Shared (`Arc`) across the
    /// server, the SyncCoordinator, the background drain, and the workspace scanner so all
    /// carrier admissions serialize on one barrier map. Engine-agnostic (both tsserver and
    /// tsgo route their commits through it).
    carrier_transaction_coordinator: Arc<crate::external_ts::CarrierTransactionCoordinator>,
}

fn e2e_provider_only_completions_enabled() -> bool {
    matches!(std::env::var("VERTER_E2E_TEST").as_deref(), Ok("1"))
        && matches!(
            std::env::var("VERTER_E2E_PROVIDER_ONLY_COMPLETIONS").as_deref(),
            Ok("1")
        )
}

impl VerterLanguageServer {
    fn enqueue_did_change_provider_update(&self) -> DidChangeProviderTurn {
        let completion = Arc::new(tokio::sync::Notify::new());
        let predecessor = self
            .did_change_provider_tail
            .lock()
            .replace(Arc::clone(&completion));
        DidChangeProviderTurn {
            predecessor,
            completion,
        }
    }

    #[cfg(test)]
    fn pause_next_completion_after_snapshot(
        &self,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        self.completion_snapshot_pauses
            .lock()
            .push_back(CompletionSnapshotPause {
                arrived: Arc::clone(&arrived),
                release: Arc::clone(&release),
            });
        (arrived, release)
    }

    #[cfg(test)]
    async fn maybe_pause_completion_after_snapshot(&self) {
        let pause = self.completion_snapshot_pauses.lock().pop_front();
        if let Some(pause) = pause {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
    }

    #[cfg(test)]
    fn pause_completion_before_final_native(
        &self,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *self.completion_before_final_pause.lock() = Some(CompletionSnapshotPause {
            arrived: Arc::clone(&arrived),
            release: Arc::clone(&release),
        });
        (arrived, release)
    }

    #[cfg(test)]
    async fn maybe_pause_completion_before_final_native(&self) {
        let pause = self.completion_before_final_pause.lock().take();
        if let Some(pause) = pause {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
    }

    #[cfg(test)]
    fn pause_final_completion_after_snapshot(
        &self,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *self.completion_final_snapshot_pause.lock() = Some(CompletionSnapshotPause {
            arrived: Arc::clone(&arrived),
            release: Arc::clone(&release),
        });
        (arrived, release)
    }

    #[cfg(test)]
    async fn maybe_pause_final_completion_after_snapshot(&self) {
        let pause = self.completion_final_snapshot_pause.lock().take();
        if let Some(pause) = pause {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
    }

    /// Acquire the current lifecycle lane. Open and close use this before
    /// mutating registry membership, so a reopen cannot land in the middle of a
    /// close of the prior document generation.
    fn ide_sync_lifecycle_lease(&self, canonical_id: &str) -> IdeSyncRepairLease {
        let lane = match self.ide_sync_repair_locks.entry(canonical_id.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .retired
                    .load(std::sync::atomic::Ordering::Acquire)
                {
                    let generation = self
                        .ide_sync_open_generations
                        .get(canonical_id)
                        .map(|entry| *entry)
                        .unwrap_or(0);
                    let replacement = Arc::new(IdeSyncRepairLane::new(generation));
                    entry.insert(Arc::clone(&replacement));
                    replacement
                } else {
                    Arc::clone(entry.get())
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let generation = self
                    .ide_sync_open_generations
                    .get(canonical_id)
                    .map(|entry| *entry)
                    .unwrap_or(0);
                let lane = Arc::new(IdeSyncRepairLane::new(generation));
                entry.insert(Arc::clone(&lane));
                lane
            }
        };
        IdeSyncRepairLease {
            canonical_id: canonical_id.to_string(),
            lane,
            lanes: Arc::clone(&self.ide_sync_repair_locks),
        }
    }

    /// Acquire only the lane belonging to `generation`. A stale repair never
    /// inserts or replaces the lane of a closed/reopened document: it receives a
    /// detached retired lane, fails generation revalidation after locking, and
    /// disappears on drop without touching the map.
    fn ide_sync_repair_lease(&self, canonical_id: &str, generation: u64) -> IdeSyncRepairLease {
        let generation_is_current = self
            .ide_sync_open_generations
            .get(canonical_id)
            .is_some_and(|current| *current == generation);
        let lane = if generation_is_current {
            match self.ide_sync_repair_locks.entry(canonical_id.to_string()) {
                dashmap::mapref::entry::Entry::Occupied(entry)
                    if !entry
                        .get()
                        .retired
                        .load(std::sync::atomic::Ordering::Acquire)
                        && entry
                            .get()
                            .generation
                            .load(std::sync::atomic::Ordering::Acquire)
                            == generation =>
                {
                    Arc::clone(entry.get())
                }
                dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                    // Re-check while owning the map entry. If this request lost
                    // the generation race, it must not replace the winner's lane.
                    if self
                        .ide_sync_open_generations
                        .get(canonical_id)
                        .is_some_and(|current| *current == generation)
                    {
                        let replacement = Arc::new(IdeSyncRepairLane::new(generation));
                        entry.insert(Arc::clone(&replacement));
                        replacement
                    } else {
                        let detached = Arc::new(IdeSyncRepairLane::new(generation));
                        detached
                            .retired
                            .store(true, std::sync::atomic::Ordering::Release);
                        detached
                    }
                }
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    if self
                        .ide_sync_open_generations
                        .get(canonical_id)
                        .is_some_and(|current| *current == generation)
                    {
                        let lane = Arc::new(IdeSyncRepairLane::new(generation));
                        entry.insert(Arc::clone(&lane));
                        lane
                    } else {
                        let detached = Arc::new(IdeSyncRepairLane::new(generation));
                        detached
                            .retired
                            .store(true, std::sync::atomic::Ordering::Release);
                        detached
                    }
                }
            }
        } else {
            let detached = Arc::new(IdeSyncRepairLane::new(generation));
            detached
                .retired
                .store(true, std::sync::atomic::Ordering::Release);
            detached
        };
        IdeSyncRepairLease {
            canonical_id: canonical_id.to_string(),
            lane,
            lanes: Arc::clone(&self.ide_sync_repair_locks),
        }
    }

    fn begin_ide_sync_open_generation(
        &self,
        canonical_id: &str,
        lane: &Arc<IdeSyncRepairLane>,
    ) -> u64 {
        let generation = self
            .ide_sync_next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        lane.generation
            .store(generation, std::sync::atomic::Ordering::Release);
        lane.retired
            .store(false, std::sync::atomic::Ordering::Release);
        self.ide_sync_repair_locks
            .insert(canonical_id.to_string(), Arc::clone(lane));
        self.ide_sync_open_generations
            .insert(canonical_id.to_string(), generation);
        generation
    }

    /// Test helpers often register directly through `DocumentRegistry`; lazily
    /// establish the same open generation production `did_open` records.
    fn current_or_init_ide_sync_open_generation(
        &self,
        uri: &Uri,
        canonical_id: &str,
    ) -> Option<u64> {
        if self.documents.get_canonical_id(uri).as_deref() != Some(canonical_id) {
            return None;
        }
        if let Some(generation) = self.ide_sync_open_generations.get(canonical_id) {
            return Some(*generation);
        }
        let generation = self
            .ide_sync_next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let generation = match self
            .ide_sync_open_generations
            .entry(canonical_id.to_string())
        {
            dashmap::mapref::entry::Entry::Occupied(entry) => *entry.get(),
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                entry.insert(generation);
                generation
            }
        };
        Some(generation)
    }

    fn ide_sync_generation_is_open(&self, uri: &Uri, canonical_id: &str, generation: u64) -> bool {
        self.documents.get_canonical_id(uri).as_deref() == Some(canonical_id)
            && self
                .ide_sync_open_generations
                .get(canonical_id)
                .is_some_and(|current| *current == generation)
    }

    fn close_ide_sync_open_generation(&self, canonical_id: &str, generation: u64) {
        self.ide_sync_open_generations
            .remove_if(canonical_id, |_, current| *current == generation);
    }

    #[cfg(test)]
    fn pause_next_ide_sync_before_lease(
        &self,
        canonical_id: &str,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        Self::pause_next_ide_sync_at(&self.ide_sync_before_lease_pause, canonical_id)
    }

    #[cfg(test)]
    fn pause_next_ide_sync_after_lease(
        &self,
        canonical_id: &str,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        Self::pause_next_ide_sync_at(&self.ide_sync_after_lease_pause, canonical_id)
    }

    #[cfg(test)]
    fn pause_next_ide_sync_close_after_lock(
        &self,
        canonical_id: &str,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        Self::pause_next_ide_sync_at(&self.ide_sync_close_after_lock_pause, canonical_id)
    }

    #[cfg(test)]
    fn pause_next_ide_sync_at(
        slot: &parking_lot::Mutex<Option<IdeSyncPausePoint>>,
        canonical_id: &str,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        *slot.lock() = Some(IdeSyncPausePoint {
            canonical_id: canonical_id.to_string(),
            arrived: Arc::clone(&arrived),
            release: Arc::clone(&release),
        });
        (arrived, release)
    }

    #[cfg(test)]
    async fn maybe_pause_ide_sync_before_lease(&self, canonical_id: &str) {
        Self::maybe_pause_ide_sync_at(&self.ide_sync_before_lease_pause, canonical_id).await;
    }

    #[cfg(test)]
    async fn maybe_pause_ide_sync_after_lease(&self, canonical_id: &str) {
        Self::maybe_pause_ide_sync_at(&self.ide_sync_after_lease_pause, canonical_id).await;
    }

    #[cfg(test)]
    async fn maybe_pause_ide_sync_close_after_lock(&self, canonical_id: &str) {
        Self::maybe_pause_ide_sync_at(&self.ide_sync_close_after_lock_pause, canonical_id).await;
    }

    #[cfg(test)]
    async fn maybe_pause_ide_sync_at(
        slot: &parking_lot::Mutex<Option<IdeSyncPausePoint>>,
        canonical_id: &str,
    ) {
        let pause = {
            let mut slot = slot.lock();
            if slot
                .as_ref()
                .is_some_and(|pause| pause.canonical_id == canonical_id)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(pause) = pause {
            pause.arrived.notify_one();
            pause.release.notified().await;
        }
    }

    pub fn new(client: Client, config: LspConfig) -> Self {
        let project_sync = config.type_provider.as_ref().map(|tp| {
            // Bind the sync to the active engine kind so the carrier-companion
            // content opens are suppressed for tsserver (the plugin serves the
            // carrier from the publish store) and flow through for tsgo.
            ProjectSync::new_with_kind(
                Arc::clone(tp),
                config.project_sync_mode,
                config.type_provider_kind,
            )
        });

        let needs_ide_sync = Arc::new(DashSet::new());
        let ide_sync_repair_locks = Arc::new(DashMap::new());
        let ide_sync_open_generations = Arc::new(DashMap::new());
        let needs_deferred_sync = Arc::new(DashSet::new());
        let documents = Arc::new(DocumentRegistry::new(config.host));
        let position_encoding = Arc::new(parking_lot::RwLock::new(PositionEncodingKind::UTF16));
        let cached_verter_diags = Arc::new(DashMap::new());
        let provider_sync_states = Arc::new(DashMap::new());
        let decl_overlay_owner = Arc::new(DeclOverlayOwner::default());
        let pending_snapshot_provider_sync = Arc::new(DashSet::new());
        let vfs_workspace: Arc<
            parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
        > = Arc::new(parking_lot::RwLock::new(None));

        // The live editor-membership publisher. Managed tsgo still opens its own
        // companion buffers directly, but VS Code's TypeScript service is a
        // separate consumer and requires the same durable carrier store for
        // imports from ordinary `.ts`/`.js` files.
        let carrier_publish_coordinator = match (&config.type_provider, config.type_provider_kind) {
            (Some(_), crate::TypeProviderKind::Tsgo) => {
                let backend = Arc::new(
                    crate::external_ts::TsserverEngineBackend::with_default_host_version(),
                );
                Some(
                    crate::external_ts::CarrierPublishCoordinator::new_editor_owned(
                        backend,
                        crate::external_ts::default_carrier_store_host_version(),
                    ),
                )
            }
            (Some(provider), crate::TypeProviderKind::Tsserver) => {
                let backend = Arc::new(
                    crate::external_ts::TsserverEngineBackend::with_default_host_version(),
                );
                // The negotiated TypeScript version is informational on the minted
                // binding (membership keys on env dims + content hash, not this
                // string); the project's real version refines it when known.
                Some(crate::external_ts::CarrierPublishCoordinator::new(
                    backend,
                    Arc::clone(provider),
                    crate::external_ts::default_carrier_store_host_version(),
                ))
            }
            (None, crate::TypeProviderKind::EditorTsserver) => {
                let backend = Arc::new(
                    crate::external_ts::TsserverEngineBackend::with_default_host_version(),
                );
                Some(
                    crate::external_ts::CarrierPublishCoordinator::new_editor_owned(
                        backend,
                        crate::external_ts::default_carrier_store_host_version(),
                    ),
                )
            }
            _ => None,
        };

        // The per-source carrier transaction coordinator (admission gate + owner-loss
        // barrier + non-owned retry disposition), shared across the server, SyncCoordinator,
        // drain, and scanner so every carrier admission serializes on ONE barrier map.
        let carrier_transaction_coordinator =
            Arc::new(crate::external_ts::CarrierTransactionCoordinator::new());

        // The SyncCoordinator's debounced loop replaces the old
        // spawn-per-keystroke pattern. Spawned UNCONDITIONALLY: its publish
        // half carries Verter-owned diagnostics on every route; the
        // provider-sync half no-ops when no in-process provider is connected
        // (editor-owned tsserver plugin serving, verter-only mode).
        let sync_coordinator = crate::sync_coordinator::spawn_sync_coordinator(
            crate::sync_coordinator::SyncCoordinatorDeps {
                documents: Arc::clone(&documents),
                project_sync: project_sync.clone(),
                needs_provider_sync: Arc::clone(&needs_deferred_sync),
                pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
                client: client.clone(),
                type_provider: config.type_provider.clone(),
                cached_verter_diags: Arc::clone(&cached_verter_diags),
                position_encoding: Arc::clone(&position_encoding),
                provider_sync_states: Arc::clone(&provider_sync_states),
                vfs_workspace: Arc::clone(&vfs_workspace),
                type_provider_kind: config.type_provider_kind,
                carrier_publish_coordinator: carrier_publish_coordinator.clone(),
                carrier_transaction_coordinator: Arc::clone(&carrier_transaction_coordinator),
            },
        );

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
            decl_overlay_owner,
            rename_provider_fence: Arc::new(tokio::sync::Mutex::new(())),
            type_provider_kind: config.type_provider_kind,
            suppress_imported_carrier_prewarm: config.suppress_imported_carrier_prewarm,
            provider_only_completions: std::sync::atomic::AtomicBool::new(
                e2e_provider_only_completions_enabled(),
            ),
            needs_ide_sync,
            ide_sync_repair_locks,
            ide_sync_open_generations,
            ide_sync_next_generation: std::sync::atomic::AtomicU64::new(1),
            import_sync_memo: Arc::new(DashMap::new()),
            import_sync_locks: Arc::new(DashMap::new()),
            #[cfg(test)]
            ide_sync_before_lease_pause: parking_lot::Mutex::new(None),
            #[cfg(test)]
            ide_sync_after_lease_pause: parking_lot::Mutex::new(None),
            #[cfg(test)]
            ide_sync_close_after_lock_pause: parking_lot::Mutex::new(None),
            needs_deferred_sync,
            pending_snapshot_provider_sync,
            sync_coordinator,
            last_change_ms: std::sync::atomic::AtomicU64::new(0),
            did_change_mutex: tokio::sync::Mutex::new(()),
            did_change_provider_tail: parking_lot::Mutex::new(None),
            #[cfg(test)]
            completion_snapshot_pauses: parking_lot::Mutex::new(std::collections::VecDeque::new()),
            #[cfg(test)]
            completion_before_final_pause: parking_lot::Mutex::new(None),
            #[cfg(test)]
            completion_final_snapshot_pause: parking_lot::Mutex::new(None),
            workspace_scanner: Arc::new(tokio::sync::Mutex::new(None)),
            init_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            mcp_port: config.mcp_port,
            type_provider_reason: config.type_provider_reason,
            mru_canonical_ids: parking_lot::Mutex::new(Vec::new()),
            vfs_workspace,
            hover_provenance_enabled: std::sync::atomic::AtomicBool::new(false),
            hover_provenance_cache: Arc::new(
                crate::features::hover_provenance::HoverProvenanceCache::new(),
            ),
            carrier_publish_coordinator,
            carrier_transaction_coordinator,
        }
    }

    pub(super) fn provider_only_completions(&self) -> bool {
        self.provider_only_completions
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The attested editor tsserver plugin owns carrier hover, navigation, and
    /// rename directly in VS Code. The LSP has no local TypeProvider in this
    /// topology and must not register a competing partial answer for the same
    /// request; VS Code selects a single rename provider and can otherwise hide
    /// the editor plugin's complete script+template edit set.
    pub(super) fn editor_owns_carrier_source_features(&self) -> bool {
        matches!(
            self.type_provider_kind,
            crate::TypeProviderKind::EditorTsserver
        )
    }

    #[cfg(test)]
    pub(crate) fn set_provider_only_completions_for_test(&self, enabled: bool) {
        self.provider_only_completions
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
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

    /// MERGED (Verter lint/template + type-provider) diagnostics for a carrier URI,
    /// mapped back onto the carrier source ranges (test harness access).
    ///
    /// Returns the exact set [`Self::publish_full_diagnostics`] would push to the
    /// client, so a real-provider test can assert a specific TS diagnostic
    /// (e.g. TS1192 default-import-of-no-default) surfaces on the `.vue` carrier.
    pub(crate) async fn test_merged_diagnostics(
        &self,
        uri: &tower_lsp_server::ls_types::Uri,
    ) -> Vec<tower_lsp_server::ls_types::Diagnostic> {
        self.compute_full_diagnostics(uri).await
    }

    /// The committed carrier [`crate::provider_sync::ProviderSyncState`] for a
    /// carrier-source URI (test harness access), or `None` when no provider-sync
    /// state has been committed for it.
    ///
    /// Read-only: it clones the entry the carrier-sync gateway commits into the
    /// server's shared `provider_sync_states` map (the provider-neutral ownership
    /// backbone), so a real-provider test can assert that a `.vue`/`.svelte`
    /// carrier whose diagnostics flow actually became an OWNED, background-loaded
    /// project member through that backbone — not merely that a diagnostic happened
    /// to appear.
    pub(crate) fn test_provider_sync_state(
        &self,
        uri: &tower_lsp_server::ls_types::Uri,
    ) -> Option<crate::provider_sync::ProviderSyncState> {
        let canonical_id = self.documents.get_canonical_id(uri)?;
        self.provider_sync_state_for_source(&canonical_id)
    }

    /// The server's shared declaration-overlay lifecycle owner (test harness
    /// access). The SAME `Arc` the `did_close` lifecycle releases through and the
    /// background closure pass opens through, so a concurrency test can race the
    /// real `handle_did_close` against the real closure pass on one shared owner and
    /// assert no overlay edge leaks (via the owner's `test_slot_*` accessors).
    pub(crate) fn test_decl_overlay_owner(&self) -> &Arc<DeclOverlayOwner> {
        &self.decl_overlay_owner
    }

    /// Run ONE real proactive-declaration-overlay closure pass
    /// ([`DeclOverlayOwner::open_declaration_closure_for_open_files`]) against the
    /// server's OWN shared state — its `project_sync`, `documents`,
    /// `provider_sync_states`, published resolver snapshot, and the shared
    /// declaration-overlay owner (test harness access).
    ///
    /// This is the EXACT pass `background_init` runs (same owner, same shared
    /// `Arc`s), so a test can interleave it with the real `handle_did_close` and
    /// exercise the production `[RELEASE]`-vs-`[DIDCLOSE]` ordering on one shared
    /// owner. Returns `false` (no-op) when no project sync or published snapshot is
    /// available.
    pub(crate) async fn test_run_declaration_closure_pass(&self, pass_generation: u64) -> bool {
        let Some(sync) = self.project_sync.as_ref() else {
            return false;
        };
        let Some(snapshot) = self.published_resolver() else {
            return false;
        };
        self.decl_overlay_owner
            .open_declaration_closure_for_open_files(
                sync,
                &self.documents,
                &self.provider_sync_states,
                &snapshot,
                pass_generation,
            )
            .await
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

#[cfg(test)]
mod request_surface_guard_tests;
