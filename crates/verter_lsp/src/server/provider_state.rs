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
use crate::type_provider::merge;

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
            // The legacy IDE path keeps its in-context (negotiated-encoding)
            // indexes; the API-only UTF-16→negotiated re-emission is opt-in.
            carrier_negotiated_line_index: None,
        })
    }

    /// THE server-side record choke point for an API-surface sync.
    ///
    /// Records a fresh generation pinning the EXACT `api_code` synced under
    /// `dts_path`, together with the source map parsed from the SAME content.
    /// When the caller already holds the synced content's source map it passes it
    /// in `source_map_json`; otherwise (`None`) the live `get_public_api()` map is
    /// used ONLY when its code byte-matches `api_code`, so a snapshot never pairs
    /// the synced offsets with a source map produced against drifted content.
    pub(super) fn record_carrier_api_snapshot(
        &self,
        canonical_id: &str,
        dts_path: &str,
        api_code: &str,
        source_map_json: Option<&str>,
    ) {
        let store = self.documents.provider_surfaces();
        let host = self.documents.host();
        match source_map_json {
            Some(_) => crate::provider_surface_store::record_carrier_api_surface(
                store,
                Some(&self.documents),
                host,
                canonical_id,
                dts_path,
                api_code,
                source_map_json,
            ),
            // No map in scope → use the live map only if it still matches content.
            None => crate::provider_surface_store::record_carrier_api_surface_code_only(
                store,
                Some(&self.documents),
                host,
                canonical_id,
                dts_path,
                api_code,
            ),
        }
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

    /// Route a carrier's sync through the SINGLE carrier-sync gateway: the membership
    /// decision (publish on owned / retract on owner-loss for tsserver) is FUSED with
    /// the provider-state transition + the sealed receipt that gates the commit. This
    /// is the server-side wrapper every interactive/background carrier-sync entry uses
    /// (it builds the engine membership context from `self`).
    pub(super) async fn reconcile_carrier_via_gateway(
        &self,
        canonical_id: &str,
        is_jsx: bool,
        ide: Option<&verter_session::IdeResponse>,
    ) -> crate::external_ts::CarrierSyncDecision {
        let Some(snapshot) = self.published_resolver() else {
            // No published snapshot yet (bootstrap): nothing to advertise/commit.
            return crate::external_ts::CarrierSyncDecision::Pending;
        };
        // Clone the VFS handle out of the guard so no lock is held across the await.
        let vfs = self.vfs_workspace.read().clone();
        // tsserver: the carrier reaches the provider as a store-backed configured-
        // project member, so the gateway runs the membership reconcile. tgo (no
        // coordinator) ⇒ `None` ⇒ the gateway returns a direct-open transition.
        let membership = match (
            matches!(self.type_provider_kind, crate::TypeProviderKind::Tsserver),
            self.carrier_publish_coordinator.as_ref(),
            vfs.as_ref(),
        ) {
            (true, Some(coordinator), Some(vfs)) => {
                Some(crate::external_ts::CarrierMembershipCtx {
                    coordinator,
                    vfs,
                    ownership_ready: snapshot.ownership_ready,
                })
            }
            _ => None,
        };
        crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
            host: self.documents.host(),
            resolver: &snapshot.resolver,
            provider_sync_states: &self.provider_sync_states,
            provider_surfaces: self.documents.provider_surfaces(),
            documents: Some(&self.documents),
            canonical_id,
            is_jsx,
            ide,
            membership,
            reason: crate::external_ts::ReconcileReason::SourceSynced,
        })
        .await
    }

    /// The carrier provider paths for `canonical_id` for the CLOSE-only path (delete /
    /// file-removed buffer cleanup). NOT a commit — needs no receipt.
    pub(super) fn carrier_close_state(
        &self,
        canonical_id: &str,
        is_jsx: bool,
    ) -> Option<ProviderSyncState> {
        let snapshot = self.published_resolver()?;
        let decl_path = self.documents.host().declaration_carrier_path(canonical_id);
        crate::external_ts::carrier_close_target(
            &snapshot.resolver,
            canonical_id,
            is_jsx,
            decl_path,
        )
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

    /// Commit a CARRIER provider state — GATED on the sealed receipt minted by the
    /// carrier-sync gateway (so a carrier state can never be committed without the
    /// membership decision). Non-carrier (shadow) commits keep
    /// [`Self::commit_provider_sync_state`].
    pub(super) fn commit_carrier_provider_state(
        &self,
        canonical_id: &str,
        state: ProviderSyncState,
        receipt: &crate::external_ts::CarrierProviderCommit,
    ) {
        crate::external_ts::commit_carrier_provider_state(
            &self.provider_sync_states,
            canonical_id,
            state,
            receipt,
        );
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
        // An UNRESOLVED (owner-less) open-document liveness state is membership-free
        // (no publish to forget), so it commits through the plain non-carrier path —
        // the receipt gates only OWNED-publish commits.
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
        receipt: &crate::external_ts::CarrierProviderCommit,
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
        self.commit_carrier_provider_state(canonical_id, committed_state, receipt);
        self.close_provider_paths(&genuinely_stale).await;
    }

    pub(super) async fn close_provider_paths(&self, paths: &[(ProviderPathKind, String)]) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        for (kind, path) in paths {
            // A `Decl` close is ROUTED through THE declaration-overlay lifecycle
            // owner — the SOLE authority that issues a provider `close_dts` for a
            // declaration overlay — so there is no second, UNGUARDED Decl-close path.
            // The owner serializes the close behind the overlay's path lock and
            // re-checks the overlay's reachability + close generation before the
            // destructive close: a still-referenced overlay (or one whose generation
            // advanced via a racing open) is skipped (closing it would strand an open
            // root on TS2307); a `Decl` path that is NOT a proactive overlay (no slot,
            // generation 0) closes through the same path. The owner needs no resolver
            // snapshot here — its per-path serialization (not a compensate-after-close
            // re-open) is what keeps a concurrent open consistent.
            if *kind == ProviderPathKind::Decl {
                let target = self.decl_overlay_owner.close_target_for(path);
                self.decl_overlay_owner
                    .guarded_close(
                        sync,
                        &self.provider_sync_states,
                        std::slice::from_ref(&target),
                    )
                    .await;
                continue;
            }
            // A closing API path is no longer the active synced virtual surface —
            // retire its active generation under a fresh close EPOCH (historical
            // snapshots stay valid for any in-flight rename that already captured
            // them; the `Closing` state keeps the path classifying VirtualDrop
            // until the provider close is CONFIRMED, so a failed close cannot let it
            // degrade to NotVirtual and corrupt a same-named real file). Capture the
            // epoch-stamped close token so the finalize is scoped to THIS close.
            let close_token = if *kind == ProviderPathKind::Api {
                Some(self.documents.provider_surfaces().forget(path))
            } else {
                None
            };
            let result = match kind {
                ProviderPathKind::Ide => sync.close_tsx(path).await,
                ProviderPathKind::Api => sync.close_dts(path).await,
                ProviderPathKind::Shadow => sync.close_file(path).await,
                // Delegated above (the guarded close is the SOLE Decl-close path).
                ProviderPathKind::Decl => unreachable!("Decl is delegated to the guarded close"),
            };
            match result {
                // Only a CONFIRMED API close finalizes, and only via THIS close's
                // token — if the path was reopened (or retired again by a newer
                // close) during the await, the epoch no longer matches and the
                // finalize is a no-op (the fresh snapshot is preserved). On an error
                // the token is dropped, so the `Closing` state persists (fail
                // closed). Ide/Shadow are not carrier-API surfaces (token is None).
                Ok(()) => {
                    if let Some(token) = close_token {
                        self.documents.provider_surfaces().finalize_close(token);
                    }
                }
                Err(error) => {
                    tracing::warn!("failed to close provider path {path}: {error}");
                }
            }
        }
    }

    pub(super) async fn close_provider_state(&self, state: &ProviderSyncState) {
        let paths = state.active_paths();
        self.close_provider_paths(&paths).await;
    }

    /// Release a now-closed carrier ROOT from the proactive declaration-overlay
    /// graph: drop it from every overlay's reachability set and CLOSE every
    /// `.d.<ext>.ts` overlay no longer reachable from any open root.
    ///
    /// An overlay still reached by a DIFFERENT open root is retained (closing it
    /// would strand that root's bare carrier imports on TS2307). The closed
    /// overlays are also stripped from their owner carrier's committed provider
    /// state so the Decl kind does not linger as a falsely-live path.
    pub(super) async fn release_declaration_overlays_for_closed_root(&self, root_canonical: &str) {
        let now_unreferenced = self.decl_overlay_owner.release_root(root_canonical);
        if now_unreferenced.is_empty() {
            return;
        }
        // Route the Decl close through THE declaration-overlay lifecycle owner — the
        // SOLE path that issues a provider `close_dts` for a declaration overlay (the
        // closure pass's reconcile uses the same owner). It serializes the close
        // behind the overlay's path lock and re-checks reachability + the close
        // generation before the destructive close, so this did_close-side close can
        // never clobber a concurrent reopen by another still-open root (TS2307
        // stranding). It also strips the `Decl` kind from each owner carrier's
        // committed state for the overlays it actually closes.
        let Some(sync) = &self.project_sync else {
            return;
        };
        self.decl_overlay_owner
            .guarded_close(sync, &self.provider_sync_states, &now_unreferenced)
            .await;
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
