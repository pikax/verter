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

use super::server_utils::source_id_from_provider_vue_path;
use super::{TypeProviderContext, VerterLanguageServer};

impl VerterLanguageServer {
    pub(super) fn external_ide_context(&self, ide_path: &str) -> Option<merge::ExternalIdeContext> {
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
    pub(super) fn type_provider_context(&self, uri: &Uri) -> Option<TypeProviderContext> {
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
    pub(super) fn vue_uri_from_ide_path(&self, ide_path: &str) -> Option<Uri> {
        let snapshot = self.published_resolver()?;
        let canonical_id =
            source_id_from_provider_vue_path(&snapshot.resolver, self.documents.host(), ide_path)?;
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

    pub(super) fn prepare_vue_provider_sync_transition(
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

    pub(super) fn prepare_non_vue_provider_sync_transition(
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

    pub(super) fn is_background_loaded_for_source_kind(
        &self,
        canonical_id: &str,
        kind: ProviderPathKind,
    ) -> bool {
        self.provider_sync_state_for_source(canonical_id)
            .map(|state| state.background_loaded_for_kind(kind))
            .unwrap_or(false)
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
