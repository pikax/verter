//! Provider-sync state CRUD + context helpers.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering MRU bookkeeping, snapshot-pending queue, transition
//! preparation/commit, sync-state removal, type-provider context
//! materialisation, and virtual-file routing context.
//!
//! All methods were moved verbatim from `server.rs` (now `server/mod.rs`)
//! lines 2516-2857 + the trailing `virtual_file_context` helper. No
//! behaviour change. The sibling lives as a private child module under
//! `server/mod.rs` so it sees the parent's private struct fields without
//! visibility widening.

use tower_lsp_server::ls_types::Uri;

use crate::documents::line_index::LineIndex;
use crate::provider_sync::{
    commit_sync_transition, prepare_sync_transition, remove_sync_state, ProviderPathKind,
    ProviderSyncState,
};
use crate::tsgo::merge;

use super::server_utils::source_id_from_provider_carrier_path;
use super::{TypeProviderContext, VerterLanguageServer};

impl VerterLanguageServer {
    pub(super) fn external_ide_context(&self, ide_path: &str) -> Option<merge::ExternalIdeContext> {
        let (_tsx_path, tsx_content, mapper) = self.ide_context_by_path(ide_path)?;
        let tsx_line_index = LineIndex::new(&tsx_content, self.documents.encoding());
        // Get the Vue file's line index
        let snapshot = self.published_resolver()?;
        let canonical_id = source_id_from_provider_carrier_path(
            &snapshot.resolver,
            self.documents.host(),
            ide_path,
        )?;
        let uri = self.documents.canonical_id_to_uri(&canonical_id)?;
        let doc = self.documents.get(&uri)?;
        Some(merge::ExternalIdeContext {
            tsx_line_index,
            mapper,
            carrier_line_index: doc.line_index.clone(),
        })
    }

    /// Pre-extracted data for type provider calls.
    /// All DashMap guards are dropped before this is returned, so it is safe
    /// to hold this across `.await` points without risking deadlock.
    pub(super) fn type_provider_context(&self, uri: &Uri) -> Option<TypeProviderContext> {
        // Route through the generalized projection context (serves BOTH the
        // carrier-IDE and self-file rune-module projections). The feature layer
        // sees the same `tsx_*` field names regardless of projection.
        let ctx = self.provider_projection_context(uri)?;
        Some(TypeProviderContext {
            tsx_path: ctx.provider_path,
            tsx_content: ctx.provider_content,
            mapper: ctx.mapper,
            tsx_line_index: ctx.provider_line_index,
            carrier_line_index: ctx.source_line_index,
        })
    }

    /// Whether `uri` projects through a SELF-FILE rune-module own buffer.
    ///
    /// Features whose workspace-EDIT positions are not mapped through the
    /// self-file mapper (rename, code actions) are GATED OFF for a self-file
    /// projection — an unmapped edit would land off by the prelude offset (or
    /// inside the prelude) and corrupt the rune module. They stay DEFERRED for
    /// the self-file projection until their edit-mapping lands; the carrier
    /// projection is unaffected.
    pub(super) fn is_self_file_projection(&self, uri: &Uri) -> bool {
        self.documents
            .get_projection(uri)
            .is_some_and(|projection| projection.is_self_file())
    }

    /// Find the Vue URI corresponding to an IDE path.
    pub(super) fn carrier_uri_from_ide_path(&self, ide_path: &str) -> Option<Uri> {
        let snapshot = self.published_resolver()?;
        let canonical_id = source_id_from_provider_carrier_path(
            &snapshot.resolver,
            self.documents.host(),
            ide_path,
        )?;
        self.documents.canonical_id_to_uri(&canonical_id)
    }

    /// Touch a canonical ID in the MRU list (push to front, dedup).
    pub(super) fn touch_mru(&self, canonical_id: &str) {
        let mut mru = self.mru_canonical_ids.lock();
        mru.retain(|id| id != canonical_id);
        mru.insert(0, canonical_id.to_string());
        // Cap at a reasonable size
        mru.truncate(64);
    }

    pub(super) fn queue_snapshot_provider_sync(&self, canonical_id: impl Into<String>) {
        self.pending_snapshot_provider_sync
            .insert(canonical_id.into());
    }

    pub(super) fn provider_sync_state_for_source(
        &self,
        canonical_id: &str,
    ) -> Option<ProviderSyncState> {
        self.provider_sync_states
            .get(canonical_id)
            .map(|entry| entry.clone())
    }

    pub(super) fn prepare_carrier_provider_sync_transition(
        &self,
        canonical_id: &str,
        is_jsx: bool,
    ) -> Option<crate::provider_sync::ProviderSyncTransition> {
        let snapshot = self.published_resolver()?;
        let next_state = crate::provider_sync::carrier_sync_state_for_source(
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

    pub(super) fn prepare_non_carrier_provider_sync_transition(
        &self,
        canonical_id: &str,
    ) -> Option<crate::provider_sync::ProviderSyncTransition> {
        let snapshot = self.published_resolver()?;
        let next_state = crate::provider_sync::non_carrier_sync_state_for_source(
            &snapshot.resolver,
            canonical_id,
        )?;
        Some(prepare_sync_transition(
            &self.provider_sync_states,
            canonical_id,
            next_state,
        ))
    }

    pub(super) fn commit_provider_sync_state(&self, canonical_id: &str, state: ProviderSyncState) {
        commit_sync_transition(&self.provider_sync_states, canonical_id, state);
    }

    pub(super) fn remove_provider_sync_state(
        &self,
        canonical_id: &str,
    ) -> Option<ProviderSyncState> {
        remove_sync_state(&self.provider_sync_states, canonical_id)
    }

    pub(super) async fn clear_provider_sync_state(&self, canonical_id: &str) {
        if let Some(state) = self.remove_provider_sync_state(canonical_id) {
            self.close_provider_state(&state).await;
        }
    }

    /// Preserve (or create) an OPEN Vue document's unresolved provider state
    /// when no project owns it, keeping its IDE TSX live in the provider.
    ///
    /// Editor-liveness invariant: an open Vue document keeps a usable TSX in the
    /// provider even while its owning project is unresolved. Builds the commit
    /// state through the shared [`open_unresolved_carrier_state`] primitive (forces
    /// `Unresolved`, preserves the owner-independent live IDE path, drops the
    /// owner-derived API path), syncs the IDE TSX when fresh `ide_code` is
    /// available, and commits. It NEVER removes the state or closes the TSX.
    pub(super) async fn preserve_open_unresolved_carrier(
        &self,
        canonical_id: &str,
        is_jsx: bool,
        ide_code: Option<&str>,
    ) {
        let previous = self.provider_sync_state_for_source(canonical_id);
        // The DESIRED Unresolved target: owner-independent desired-extension IDE
        // path + the open-vs-update syncability hint. Binding forced
        // `Unresolved`, owner-derived API dropped.
        let target = crate::provider_sync::open_unresolved_carrier_state(
            previous.as_ref(),
            canonical_id,
            is_jsx,
        );

        // Attempt the desired IDE sync when fresh code is available (update-in-
        // place when the desired path is already live, else first-open).
        let mut ide_synced = false;
        if let (Some(sync), Some(ide_code), Some(ide_path)) =
            (&self.project_sync, ide_code, target.ide_path.clone())
        {
            let result = if target.ide_background_loaded {
                sync.sync_tsx(&ide_path, ide_code).await
            } else {
                sync.open_tsx(&ide_path, ide_code).await
            };
            match result {
                Ok(()) => ide_synced = true,
                Err(error) => {
                    tracing::warn!(
                        "preserve_open_unresolved_carrier: failed to sync open unresolved IDE path \
                         {ide_path}: {error}"
                    );
                }
            }
        }

        // Build the committed state + close targets through the SAME per-kind
        // discipline the owner-resolved path uses: a non-synced IDE kind RETAINS
        // the prior LIVE path (never dropped to a dead/None path while the prior
        // is still open in the provider — rows 7 & 9), the owner-derived API is
        // dropped+closed unconditionally, and the orphaned prior IDE path is
        // closed ONLY after a successful flip (close-after-success).
        let commit = crate::provider_sync::open_unresolved_carrier_commit(
            previous.as_ref(),
            target,
            ide_synced,
        );
        self.commit_provider_sync_state(canonical_id, commit.committed);
        if let Some(dropped) = commit.dropped_api {
            self.close_provider_paths(std::slice::from_ref(&dropped))
                .await;
        }
        if let Some(stale) = commit.stale_ide_after_success {
            self.close_provider_paths(std::slice::from_ref(&stale))
                .await;
        }
    }

    pub(super) fn is_background_loaded_for_source_kind(
        &self,
        canonical_id: &str,
        kind: ProviderPathKind,
    ) -> bool {
        self.provider_sync_state_for_source(canonical_id)
            .map(|state| state.background_loaded_for_kind(kind))
            .unwrap_or(false)
    }

    /// Commit a (possibly partial) Vue provider-sync result with the
    /// close-AFTER-successful-sync discipline, shared by every owner-resolved
    /// `.vue` foreground/background sync method.
    ///
    /// Per-kind partial-failure gated: a kind whose replacement did NOT sync
    /// reverts to its previous live path (so the committed state never
    /// advertises an unsynced path); then the new state is committed and ONLY
    /// the genuinely-stale paths are closed (kind synced AND not active). On a
    /// total failure (`synced_kinds` empty) nothing is committed or closed —
    /// the previous state + provider paths are retained intact.
    pub(super) async fn commit_and_close_after_sync(
        &self,
        canonical_id: &str,
        previous_state: Option<&ProviderSyncState>,
        mut committed_state: ProviderSyncState,
        stale_paths: &[(ProviderPathKind, String)],
        synced_kinds: &[ProviderPathKind],
    ) {
        if synced_kinds.is_empty() {
            return;
        }
        crate::provider_sync::revert_unsynced_kinds(
            &mut committed_state,
            previous_state,
            synced_kinds,
        );
        let genuinely_stale = crate::provider_sync::genuinely_stale_after_sync(
            stale_paths,
            &committed_state,
            synced_kinds,
        );
        self.commit_provider_sync_state(canonical_id, committed_state);
        self.close_provider_paths(&genuinely_stale).await;
    }

    pub(super) async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)]) {
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

    pub(super) async fn close_provider_state(&self, state: &ProviderSyncState) {
        let paths = state.active_paths();
        self.close_provider_paths(&paths).await;
    }

    /// Check if a URI is a virtual file and return its TSGO routing context.
    ///
    /// For virtual files (verter-virtual://), the content IS the TSX already.
    /// The cursor position is in TSX coordinates, so we can query TSGO directly
    /// without position mapping.
    ///
    /// Returns `Some((tsx_path, virtual_doc_line_index))` if this is a virtual file
    /// that should be routed through the source .vue file's TSX.
    pub(super) fn virtual_file_context(&self, uri: &Uri) -> Option<(String, LineIndex)> {
        let source_uri_str = self.documents.get_virtual_source_uri(uri)?;
        let source_uri: Uri = source_uri_str.parse().ok()?;

        // Get the TSX path from the source .vue file
        let tsx_path = self.active_ide_path_for_uri(&source_uri)?;

        // Build LineIndex from the virtual file's content (for offset conversion)
        let doc = self.documents.get(uri)?;
        let line_index = doc.line_index.clone();

        Some((tsx_path, line_index))
    }
}
