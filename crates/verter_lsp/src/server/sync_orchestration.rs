//! Provider-sync orchestration.
//!
//! Inherent-impl extension methods on [`super::VerterLanguageServer`]
//! covering diagnostics publishing, IDE/API/non-carrier provider sync,
//! background-init bootstrap, and unresolved (pre-snapshot) sync paths.
//!
//! All methods were moved verbatim from `server.rs` (now `server/mod.rs`).
//! No behaviour change. The sibling lives as a private child module
//! under `server/mod.rs` so it sees the parent's private struct fields
//! without visibility widening.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::ls_types::*;

use verter_workspace::WorkspaceRead;

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::external_ts::AbsentReason;
use crate::provider_sync::ProviderPathKind;
use crate::type_provider::auto_import::{
    resolve_script_import_anchor, ScriptImportInsertionAnchor,
};
use crate::type_provider::merge;

use super::background_drain::configure_provider_paths_for_source;
use super::background_init::{background_init, BackgroundInitArgs};
use super::handler_guard::block_in_place_if_available;
use super::server_utils::*;
use super::{ProviderProjectionContext, PublishedResolverSnapshot, VerterLanguageServer};

impl VerterLanguageServer {
    pub(super) async fn publish_full_diagnostics(&self, uri: &Uri) {
        let diagnostics = self.compute_full_diagnostics(uri).await;
        self.publish_diagnostics_raw(uri, diagnostics).await;
    }

    /// Compute the merged (Verter lint/template + type-provider) diagnostic set for
    /// `uri`, mapped back onto the carrier source ranges, WITHOUT publishing it.
    ///
    /// This is the body shared with [`Self::publish_full_diagnostics`]; splitting the
    /// computation from the push lets real-provider tests observe the merged set
    /// directly (the test harness drains the client socket, so a pushed set is not
    /// otherwise readable).
    pub(super) async fn compute_full_diagnostics(&self, uri: &Uri) -> Vec<Diagnostic> {
        let verter_diags = self.compute_verter_diagnostics(uri);

        if let Some(tp) = &self.type_provider {
            match self.provider_projection_context(uri) {
                Some(ctx) => {
                    match tp.get_diagnostics(&ctx.provider_path).await {
                        Ok(type_diags) => {
                            // Post-await validation: diagnostics produced against a
                            // surface that no longer matches must be DROPPED (fail
                            // closed) — the debounced coordinator republishes after
                            // the next sync lands.
                            if !self.provider_request_surface_still_valid(uri, &ctx.snapshot) {
                                tracing::debug!(
                                    "publish_full_diagnostics: dropping provider diagnostics \
                                     — captured surface no longer valid"
                                );
                                return verter_diags;
                            }
                            tracing::debug!(
                                "publish_full_diagnostics: type provider returned {} for {}",
                                type_diags.len(),
                                uri.as_str()
                            );
                            // Related-span map-back uses the same cross-file/carrier
                            // resolver inputs the code-action/definition merges use:
                            // the external resolver bridges a FOREIGN carrier `.tsx`
                            // related span, the VFS source reader a real `.ts` one.
                            let carrier_source_exists =
                                |p: &str| self.documents.host().get_source(p).is_some();
                            let negotiated_encoding = self.position_encoding.read().clone();
                            merge::merge_diagnostics(
                                verter_diags,
                                type_diags,
                                &ctx.provider_path,
                                &ctx.provider_line_index,
                                &ctx.mapper,
                                &ctx.source_line_index,
                                Some(&|ide_path: &str| self.external_ide_context(ide_path)),
                                &carrier_source_exists,
                                negotiated_encoding,
                                &|p: &str| {
                                    block_in_place_if_available(|| {
                                        self.documents.host().workspace_read().read_file(p)
                                    })
                                },
                            )
                        }
                        Err(e) => {
                            tracing::warn!(
                                "publish_full_diagnostics: type provider error for {}: {e}",
                                uri.as_str()
                            );
                            verter_diags
                        }
                    }
                }
                None => verter_diags,
            }
        } else {
            verter_diags
        }
    }

    /// Audit-aware wrapper for [`Self::publish_full_diagnostics`].
    ///
    /// Routes the push-diagnostics path through
    /// [`crate::audit_harness::run_with_audit`] so the per-method
    /// timeout budget, cancellation marker, and records-store
    /// publication apply uniformly with the request-side handlers.
    /// When `audit_enabled = false` this short-circuits to the raw
    /// publish path with no observability cost.
    pub(super) async fn publish_full_diagnostics_with_audit(&self, uri: &Uri) {
        let host = self.documents.host_arc();
        if !host.config().audit_enabled {
            self.publish_full_diagnostics(uri).await;
            return;
        }
        let canonical_id = crate::audit_harness::canonical_id_for_uri(host.as_ref(), uri);
        let budget = host.config().lsp_method_timeouts.diagnostics;
        let uri_for_body = uri.clone();
        let _ = crate::audit_harness::run_with_audit::<usize, _, _>(
            &host,
            verter_audit::payloads::tags::LspMethodTag::Diagnostics,
            canonical_id,
            None,
            budget,
            async move {
                self.publish_full_diagnostics(&uri_for_body).await;
                let count = self
                    .cached_verter_diags
                    .get(uri_for_body.as_str())
                    .map(|e| e.value().2.len())
                    .unwrap_or(0);
                Ok::<usize, tower_lsp_server::jsonrpc::Error>(count)
            },
            |payload, count| {
                payload.num_diagnostics = Some(u32::try_from(*count).unwrap_or(u32::MAX));
                payload.response_size_bytes =
                    u32::try_from(count.saturating_mul(160)).unwrap_or(u32::MAX);
            },
        )
        .await;
    }

    /// Low-level: push pre-computed diagnostics to the client.
    pub(super) async fn publish_diagnostics_raw(&self, uri: &Uri, diagnostics: Vec<Diagnostic>) {
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
    pub(super) fn build_auto_import_edit(
        &self,
        doc_uri_str: &str,
        component_name: &str,
        import_path: &str,
    ) -> Option<TextEdit> {
        let uri: Uri = doc_uri_str.parse().ok()?;
        let doc = self.documents.get(&uri)?;
        let analysis = self.documents.get_analysis(&uri)?;

        // Skip if the component is already imported.
        for import in &analysis.imports {
            if import.bindings.iter().any(|b| b.name == component_name) {
                return None;
            }
        }

        // Resolve the insertion anchor from the SFC's own block/import facts (shared with the
        // TypeProvider auto-import path). `AnalyzedImport.span` is SFC-absolute; pass the spans
        // straight through. Component auto-import only targets an existing `<script setup>` block;
        // defer when none exists.
        let user_import_spans: Vec<(u32, u32)> = analysis
            .imports
            .iter()
            .map(|imp| (imp.span.start, imp.span.end))
            .collect();
        let anchor = resolve_script_import_anchor(&doc.source, &user_import_spans);
        let ScriptImportInsertionAnchor::ExistingScriptSetup { .. } = anchor else {
            return None;
        };

        let import_stmt = format!("import {} from '{}'\n", component_name, import_path);
        anchor.build_edit(&[import_stmt], &doc.line_index)
    }

    #[allow(dead_code)] // Used by sync_coordinator, may be useful for future callers
    pub(super) async fn sync_ide_to_provider(&self, uri: &Uri) {
        let _timer = self
            .statistics
            .timer("ide_sync", Some(uri.as_str().to_string()));
        let Some(sync) = &self.project_sync else {
            return;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        self.documents.host().ensure_loaded(&canonical_id);
        let Some(ide) = self.documents.get_ide(uri) else {
            tracing::debug!("sync_ide: no IDE output available for {}", uri.as_str());
            return;
        };
        // Route through the SINGLE carrier-sync gateway: the membership decision
        // (store-backed configured-project member for tsserver) is FUSED with the
        // provider-state commit. tsserver ⇒ `Published`; tsgo ⇒ `DirectOpen` (open the
        // IDE companion buffer directly).
        match self
            .reconcile_carrier_via_gateway(&canonical_id, ide.is_jsx, Some(&ide))
            .await
        {
            crate::external_ts::CarrierSyncDecision::Published {
                committed_state,
                receipt,
            } => {
                self.commit_carrier_provider_state(&canonical_id, committed_state, &receipt);
            }
            crate::external_ts::CarrierSyncDecision::DirectOpen {
                transition,
                receipt,
            } => {
                // Close-AFTER-sync (skip-active, per-kind) — uniform with every
                // owner-resolved Vue sync path.
                let previous_state = self.provider_sync_state_for_source(&canonical_id);
                let stale_paths = transition.stale_paths;
                let mut committed_state = transition.next;
                let Some(ide_path) = committed_state.ide_path.clone() else {
                    return;
                };
                tracing::info!("sync_ide: {} ({} bytes)", ide_path, ide.code.len());
                let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();
                if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                    tracing::warn!("sync_ide: failed for {ide_path}: {e}");
                } else {
                    committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                    synced_kinds.push(ProviderPathKind::Ide);
                    // Record a fresh generation pinning the EXACT IDE bytes just
                    // synced (interactive queries capture this surface).
                    self.record_carrier_ide_snapshot(
                        &canonical_id,
                        &ide_path,
                        &ide.code,
                        ide.source_map.as_deref(),
                    );
                    tracing::info!("sync_ide: ok for {}", ide_path);
                }
                self.commit_and_close_after_sync(
                    &canonical_id,
                    previous_state.as_ref(),
                    committed_state,
                    &stale_paths,
                    &synced_kinds,
                    &receipt,
                )
                .await;
            }
            crate::external_ts::CarrierSyncDecision::Unowned
            | crate::external_ts::CarrierSyncDecision::Pending => {
                self.pending_snapshot_provider_sync.insert(canonical_id);
            }
        }
    }

    /// Sync the public API (.vue.ts) to the type provider for cross-file component resolution.
    pub(super) async fn sync_api_to_provider(&self, uri: &Uri) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        let canonical_id = match self.documents.get_canonical_id(uri) {
            Some(id) => id,
            None => return,
        };
        self.documents.host().ensure_loaded(&canonical_id);
        if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
            if let Some(snapshot) = self.published_resolver() {
                configure_provider_paths_for_source(sync, &snapshot, &canonical_id, false).await;
            }
        }
        let ide = self.documents.get_ide(uri);
        let is_jsx = ide
            .as_ref()
            .map(|ide| ide.is_jsx)
            .unwrap_or_else(|| self.documents.is_jsx(uri));
        // Route through the SINGLE carrier-sync gateway: membership fused with the
        // provider-state commit. tsserver ⇒ `Published` (the plugin serves both
        // companions); tsgo ⇒ `DirectOpen` (open the API companion buffer directly).
        match self
            .reconcile_carrier_via_gateway(&canonical_id, is_jsx, ide.as_ref())
            .await
        {
            crate::external_ts::CarrierSyncDecision::Published {
                committed_state,
                receipt,
            } => {
                self.commit_carrier_provider_state(&canonical_id, committed_state, &receipt);
            }
            crate::external_ts::CarrierSyncDecision::DirectOpen {
                transition,
                receipt,
            } => {
                // Close-AFTER-sync: capture stale + prior state, sync, then commit +
                // close only genuinely-stale paths (this path can touch an open Vue
                // file, so a failed replacement must not close the live path).
                let previous_state = self.provider_sync_state_for_source(&canonical_id);
                let stale_paths = transition.stale_paths;
                let mut committed_state = transition.next;
                let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();
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
                            synced_kinds.push(ProviderPathKind::Api);
                            // Record a fresh generation pinning the synced content +
                            // its same-content source map under this virtual path.
                            self.record_carrier_api_snapshot(
                                &canonical_id,
                                &dts_path,
                                &api.code,
                                api.source_map.as_deref(),
                            );
                        }
                    }
                }
                self.commit_and_close_after_sync(
                    &canonical_id,
                    previous_state.as_ref(),
                    committed_state,
                    &stale_paths,
                    &synced_kinds,
                    &receipt,
                )
                .await;
            }
            crate::external_ts::CarrierSyncDecision::Unowned
            | crate::external_ts::CarrierSyncDecision::Pending => {
                self.pending_snapshot_provider_sync.insert(canonical_id);
            }
        }
    }

    /// Publish a carrier's companions into the project-bound store the
    /// `@verter/typescript-plugin` reads — the tsserver carrier-membership path
    /// for the INTERACTIVE sync entry points (the drain has its own publish).
    ///
    /// Returns `true` when the tsserver publish path HANDLED this carrier (the
    /// caller must NOT also open the companions directly), `false` when there is
    /// no coordinator (tsgo / no provider) so the caller runs the direct
    /// `open_file`/`sync_*` path. Resolution is fail-closed inside the coordinator
    /// (a no-owner/ambiguous source publishes nothing — but it is still
    /// "handled": the carrier is intentionally not a member, never opened).
    ///
    /// Recompiles the IDE + public-API surfaces (the same content the direct path
    /// syncs), records the API provider surface for span classification, and
    /// publishes through the shared coordinator (which mints the witness, runs the
    /// two-phase store publish, and evicts the post-warm sticky resolution).
    pub(super) async fn publish_carrier_to_external_ts(&self, canonical_id: &str) -> bool {
        // Only a framework carrier (`.vue`/`.svelte`) with a tsserver coordinator is
        // published; a non-carrier file (or tsgo / no provider) is NOT handled here —
        // it falls through to the caller's direct-open path.
        if self.carrier_publish_coordinator.is_none() {
            return false;
        }
        if carrier_language_for(canonical_id).is_none() {
            return false;
        }
        self.documents.host().ensure_loaded(canonical_id);
        let profile = self.documents.tsx_profile.read().clone();
        let _ = block_in_place_if_available(|| {
            self.documents
                .host
                .ensure_ide_compiled(canonical_id, &profile)
        });
        let ide =
            block_in_place_if_available(|| self.documents.host.get_ide(canonical_id, &profile));
        let is_jsx = ide.as_ref().map(|o| o.is_jsx).unwrap_or(false);

        // This is the MEMBERSHIP-refresh entry (eager carrier refresh / cross-file
        // prewarm): route through the SINGLE carrier-sync gateway for the store
        // membership decision (publish on owned / retract on owner-loss). The
        // provider-state COMMIT is the caller's responsibility here, so the gateway's
        // `Published` committed_state + receipt are intentionally discarded — only the
        // store membership is refreshed. Fail-closed inside the gateway.
        let _ = self
            .reconcile_carrier_via_gateway(canonical_id, is_jsx, ide.as_ref())
            .await;
        true
    }

    /// Retract a carrier source from the external-TS engine — the DELETE /
    /// file-removed transition for the tsserver engine. Routes through the
    /// reconciler's `remove_source_membership`, which retracts the durable on-disk
    /// store, closes the provider companion buffers, and tombstones the ledger so the
    /// carrier disappears from the ledger-backed `getExternalFiles`. `Ok(())` when
    /// there is no tsserver coordinator (tsgo / no provider); a reconcile failure is
    /// PROPAGATED (never swallowed) so a delete caller can surface it.
    pub(super) async fn retract_carrier_from_external_ts(
        &self,
        canonical_id: &str,
    ) -> Result<(), crate::external_ts::ReconcileErr> {
        if let Some(coordinator) = self.carrier_publish_coordinator.as_ref() {
            // The Tombstoned outcome is observed via the ledger; the delete caller
            // needs only success-vs-failure (propagated).
            let _outcome = coordinator
                .remove_membership(canonical_id, AbsentReason::Deleted)
                .await?;
        }
        Ok(())
    }

    /// The source-membership ledger — INTERNAL transition bookkeeping (the
    /// reconciler's sole writer is its `commit`). `None` for tsgo / no-provider (no
    /// external-TS engine). It is NOT the production serve path: live `getExternalFiles`
    /// is served CROSS-PROCESS from the on-disk store `ready_files`. This in-process
    /// ledger read is the TEST-SIDE authority the production-path tests assert
    /// membership against (the value the reconciler keeps consistent with that store).
    #[cfg(test)]
    #[must_use]
    pub(super) fn membership_ledger(&self) -> Option<&Arc<crate::external_ts::MembershipLedger>> {
        self.carrier_publish_coordinator
            .as_ref()
            .map(|coordinator| coordinator.backend().membership_ledger())
    }

    /// The carrier-companion provider paths advertised for `project` per the in-process
    /// ledger — a TEST-SIDE view of the reconciler's membership bookkeeping, NOT the
    /// production `getExternalFiles` path (which the plugin serves cross-process from
    /// the on-disk store `ready_files`). Empty for tsgo / no-provider.
    #[cfg(test)]
    #[must_use]
    pub(super) fn external_ts_advertised_for_project(&self, project: &str) -> Vec<String> {
        self.carrier_publish_coordinator
            .as_ref()
            .map(|coordinator| coordinator.backend().external_files_for_project(project))
            .unwrap_or_default()
    }

    pub(super) async fn sync_carrier_public_api_by_canonical_id(&self, canonical_id: &str) {
        if let Some(uri) = self.documents.canonical_id_to_uri(canonical_id) {
            self.sync_api_to_provider(&uri).await;
        } else {
            self.resync_background_carrier_file(canonical_id).await;
        }
    }

    pub(super) fn refresh_carrier_dependency_tracking(&self, canonical_id: &str) {
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
                .map(|entry| verter_session::DependencyResolution {
                    specifier: entry.provider_specifier.clone(),
                    resolved_canonical_id: Some(entry.source_id.clone()),
                    possible_canonical_ids: Vec::new(),
                })
                .collect(),
        );
    }

    pub(super) async fn sync_non_carrier_file_to_provider(
        &self,
        snapshot: &PublishedResolverSnapshot,
        canonical_id: &str,
        source: Arc<str>,
        module_references: &[verter_session::ScriptModuleReference],
    ) {
        let reader = LspProjectResolverReader::new(&self.documents);
        let Some(prepared) = prepare_non_carrier_provider_sync(
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
            if let Some(transition) =
                self.prepare_non_carrier_provider_sync_transition(canonical_id)
            {
                // TODO(follow-up): this non-carrier (shadow/real) write is intentionally NOT
                // recorded into the ProviderSurfaceStore. It is fail-closed-safe today because
                // (1) only CarrierApi surfaces are ever recorded/vouched (the rename capture set
                // filters on `kind == CarrierApi`), (2) a non-carrier `provider_path` lives in a
                // disjoint namespace from a `{carrier}.ts` virtual path (a real rune module's own
                // canonical path is never a sibling component's CarrierApi path), and (3)
                // `close_provider_paths` below retires (forgets) any stale CarrierApi path BEFORE
                // this write, so no stale CarrierApi generation can survive to vouch over this
                // content. If the store later grows Real/Shadow snapshot producers, record this
                // write here so the store stays the complete authority on every synced surface.
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
                    .map(|entry| verter_session::DependencyResolution {
                        specifier: entry.provider_specifier.clone(),
                        resolved_canonical_id: Some(entry.source_id.clone()),
                        possible_canonical_ids: Vec::new(),
                    })
                    .collect(),
            );
        }

        let carrier_public_api_targets = prepared
            .resolved_dependencies
            .iter()
            .filter(|dependency| {
                dependency.provider_target
                    == crate::project_resolver::ProviderTarget::CarrierPublicApi
            })
            .map(|dependency| dependency.source_id.clone())
            .collect::<Vec<_>>();
        for carrier_public_api_target in carrier_public_api_targets {
            self.sync_carrier_public_api_by_canonical_id(&carrier_public_api_target)
                .await;
        }

        let non_carrier_provider_graph_targets = prepared
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
        self.sync_non_carrier_provider_graph(
            &snapshot.resolver,
            non_carrier_provider_graph_targets,
        )
        .await;
    }

    pub(super) async fn sync_non_carrier_provider_graph(
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
            if !seen.insert(canonical_id.clone()) {
                continue;
            }
            // Framework carriers never sync through the non-carrier graph
            // as raw scripts: `.vue` targets sync through the Vue
            // public-api path, and a carrier-less row (`.svelte`)
            // produces no provider sync state.
            let Some(file_language) =
                crate::provider_sync::provider_script_language(&self.documents.host, &canonical_id)
            else {
                continue;
            };

            let Some(source) = reader.read_file(&canonical_id) else {
                continue;
            };

            let module_references = self
                .documents
                .host
                .upsert(verter_session::UpsertRequest {
                    canonical_id: Some(canonical_id.clone()),
                    input_id: canonical_id.clone(),
                    source: Arc::clone(&source),
                    file_language,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default();

            let Some(prepared) = prepare_non_carrier_provider_sync(
                Some(&PublishedResolverSnapshot {
                    resolver: resolver.clone(),
                    ownership_ready: true,
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
                    ownership_ready: true,
                };
                configure_provider_paths_for_source(sync, &snapshot, &canonical_id, true).await;
            }
            if let Some(transition) =
                self.prepare_non_carrier_provider_sync_transition(&canonical_id)
            {
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
                        .map(|entry| verter_session::DependencyResolution {
                            specifier: entry.provider_specifier.clone(),
                            resolved_canonical_id: Some(entry.source_id.clone()),
                            possible_canonical_ids: Vec::new(),
                        })
                        .collect(),
                );
            }

            for dependency in resolved_dependencies {
                if dependency.provider_target
                    == crate::project_resolver::ProviderTarget::CarrierPublicApi
                {
                    self.sync_carrier_public_api_by_canonical_id(&dependency.source_id)
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

    pub(super) fn sync_api_to_provider_in_background(&self, uri: Uri) {
        let Some(sync) = self.project_sync.clone() else {
            return;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(&uri) else {
            return;
        };
        // tsserver: the API companion reaches the engine through the store-backed
        // membership (the background ProjectSync content task below is a no-op for
        // tsserver), so queue the carrier for the drain, which reconciles it through
        // the single membership reconciler. The direct background task is the tsgo
        // route.
        if self.carrier_publish_coordinator.is_some() {
            self.pending_snapshot_provider_sync.insert(canonical_id);
            return;
        }
        let Some(snapshot) = self.published_resolver() else {
            self.pending_snapshot_provider_sync.insert(canonical_id);
            return;
        };
        let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
        let is_jsx = self.documents.is_jsx(&uri);
        // The background task routes through the SINGLE carrier-sync gateway (tsgo
        // direct-open) for its transition + receipt, then applies the per-kind
        // close-after-successful-sync discipline: it manages ONLY the API kind,
        // reverts the IDE kind to its prior live path, and must never close or
        // rebind the live IDE `.tsx`.
        let host = self.documents.host_arc();
        let provider_sync_states = Arc::clone(&self.provider_sync_states);
        let provider_surfaces = self.documents.provider_surfaces().clone();
        tokio::spawn(
            super::background_drain::sync_api_to_provider_background_task(
                sync,
                snapshot,
                host,
                provider_sync_states,
                provider_surfaces,
                canonical_id,
                is_jsx,
                is_tsgo,
            ),
        );
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
    /// **Without snapshot**: pre-snapshot blocker hydration + unresolved IDE sync.
    pub(super) async fn ensure_current_file_synced(&self, uri: &Uri) {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };

        // Touch MRU for snapshot drain ordering
        self.touch_mru(&canonical_id);

        let current_state = self.provider_sync_state_for_source(&canonical_id);
        let has_committed_state = current_state.is_some();
        let ide_already_synced = current_state
            .as_ref()
            .map(|s| s.ide_background_loaded)
            .unwrap_or(false);
        let published = self.published_resolver();
        let ownership_ready = published
            .as_ref()
            .map(|snapshot| snapshot.ownership_ready)
            .unwrap_or(false);
        // Reconcile ONLY when the committed binding is out of date with the live
        // resolution under a ready snapshot — i.e. on a binding MISMATCH:
        //   * owner gain — `Unresolved` committed, the live snapshot now resolves
        //     an owner (the bootstrap/unowned→owned upgrade);
        //   * R2-5 owner change/loss — a previously-`Owned` OPEN `.vue` whose
        //     owner changed or disappeared (the live snapshot resolves a
        //     different owner or None). Without this, an already-synced open file
        //     early-returns on a stale `Owned` binding and stays stranded on a
        //     dead owner.
        //
        // A committed `Unresolved` binding whose current resolution is ALSO None
        // is NOT a mismatch — it is steady state (FRESH). Treating it as needing
        // reconcile (R3-3) re-compiled + re-synced the TSX on EVERY foreground
        // pass (hover/completion) for an open unowned file, a per-keystroke perf
        // regression. `committed_binding_matches_current` (Unresolved==Unresolved)
        // returns true for that case, so it does not reconcile.
        let needs_owner_reconcile = match (current_state.as_ref(), published.as_ref()) {
            (Some(state), Some(snapshot)) if ownership_ready => {
                !crate::provider_sync::committed_binding_matches_current(
                    state,
                    &crate::provider_sync::current_owner_binding_for_source(
                        &snapshot.resolver,
                        &canonical_id,
                    ),
                )
            }
            _ => false,
        };
        let needs_sync = self.needs_ide_sync.remove(&canonical_id).is_some();

        if !needs_sync && has_committed_state && ide_already_synced && !needs_owner_reconcile {
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

        // Recompile + refresh mapper (in case blocker hydration changed TSX) BEFORE
        // the carrier-sync gateway runs, so a tsserver membership publish advertises
        // the freshly-compiled companions.
        self.documents.recompile_and_refresh_mapper(uri);

        let ide = self.documents.get_ide(uri);
        let is_jsx = ide.as_ref().map(|r| r.is_jsx).unwrap_or(false);

        // Route the carrier MEMBERSHIP decision through the SINGLE carrier-sync
        // gateway and capture the receipt that GATES the owner-resolved commit below.
        // tsserver: PUBLISHES the carrier's companions into the store the plugin reads
        // (the configured-project membership) and returns `Published`; the IDE
        // companion is STILL opened below (tsserver `geterr` runs on open buffers) —
        // membership (plugin) + open buffer (diagnostics) complement. tsgo: returns
        // `DirectOpen` (no store), minting the receipt for the direct open+commit. An
        // owner-loss (`Unowned`) RETRACTS the membership inside the gateway and yields
        // no receipt (the open-document liveness commit below is membership-free).
        let owned_commit_receipt = self
            .reconcile_carrier_via_gateway(&canonical_id, is_jsx, ide.as_ref())
            .await
            .into_owned_receipt();

        // Determine sync plan: owner-aware, unresolved, or skip.
        let snapshot = self.published_resolver();
        let (ide_path, unresolved) = match &snapshot {
            Some(snap) if snap.ownership_ready => {
                // Ready snapshot: only sync if file has an owner.
                let Some(ide_path) =
                    provider_ide_path_for_source(&snap.resolver, &canonical_id, is_jsx)
                else {
                    // Non-carrier file: IDE sync not applicable.
                    return;
                };
                if crate::provider_sync::current_owner_binding_for_source(
                    &snap.resolver,
                    &canonical_id,
                )
                .is_unresolved()
                {
                    // Ready snapshot but no owner for this Vue file. Editor-
                    // liveness invariant: an OPEN document keeps its TSX live
                    // as UNRESOLVED open-document state instead of closing it.
                    //
                    // Route an OPEN file through the shared
                    // `preserve_open_unresolved_carrier` primitive (forces
                    // `Unresolved`, preserves the live IDE TSX, drops AND
                    // CLOSES the stale owner-derived `.vue.ts`), queue it for a
                    // future owner reconciliation, and return. This is the
                    // owner-loss/mismatch reconcile (R2-5): a previously-
                    // `Owned` open file that lost its owner must shed its stale
                    // `Owned` binding + owner-derived API path here, not retain
                    // them via an early return or an inline commit that leaks
                    // the dropped `.vue.ts`. The gateway above already RETRACTED
                    // the STORE/ledger membership; this preserve is membership-free.
                    if self.documents.canonical_id_to_uri(&canonical_id).is_some() {
                        self.preserve_open_unresolved_carrier(
                            &canonical_id,
                            is_jsx,
                            ide.as_ref().map(|output| &*output.code),
                        )
                        .await;
                        self.queue_snapshot_provider_sync(canonical_id.clone());
                        self.needs_deferred_sync.insert(canonical_id);
                        return;
                    }
                    // Closed/non-open Vue file with no owner: fall through with
                    // `unresolved = true` (the shared commit records an
                    // `Unresolved` binding carrying this IDE path).
                    (ide_path, true)
                } else {
                    (ide_path, false)
                }
            }
            Some(snap) => {
                // Bootstrap snapshot (ownership_ready = false): unresolved sync allowed.
                let Some(ide_path) =
                    provider_ide_path_for_source(&snap.resolver, &canonical_id, is_jsx)
                else {
                    // Non-carrier file: IDE sync not applicable.
                    return;
                };
                if matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                    configure_provider_paths_for_source(sync, snap, &canonical_id, false).await;
                }
                (ide_path, true)
            }
            None => {
                // No VFS workspace at all: unresolved. Any framework carrier
                // projects its IDE virtual path through the carrier-generic
                // derivation (`Foo.svelte` → `Foo.svelte.tsx`), never a
                // hardcoded `.vue` suffix.
                if carrier_language_for(&canonical_id).is_none() {
                    return;
                }
                (
                    verter_workspace::carrier_ide_provider_path(&canonical_id, is_jsx),
                    true,
                )
            }
        };

        let Some(ide) = ide else {
            return;
        };

        let previous_ide_path = current_state
            .as_ref()
            .and_then(|state| state.ide_path.clone());
        let ide_path_loaded = current_state
            .as_ref()
            .map(|state| {
                state.ide_path.as_deref() == Some(ide_path.as_str()) && state.ide_background_loaded
            })
            .unwrap_or(false);

        // FIX-7: open/sync the NEW IDE path FIRST, then close the old one only
        // AFTER a successful sync (see the success arm below). On a jsx↔tsx
        // transition whose new sync fails, the old path must stay open and the
        // committed state must not point at the unsynced path — so the close is
        // deferred past the commit, never run before the open.

        // Choose open_file vs update_file based on existing state
        let result = if ide_path_loaded {
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
                // Record a fresh generation pinning the EXACT IDE bytes just
                // synced (interactive queries capture this surface).
                self.record_carrier_ide_snapshot(
                    &canonical_id,
                    &ide_path,
                    &ide.code,
                    ide.source_map.as_deref(),
                );
                // Commit state
                let mut state = if unresolved {
                    crate::provider_sync::ProviderSyncState {
                        owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
                        ide_path: Some(ide_path.clone()),
                        api_path: None,
                        ..Default::default()
                    }
                } else {
                    // Owner-resolved carrier: the owner-aware provider paths (the
                    // same computation the gateway used to mint the receipt). A miss
                    // (snapshot lost mid-sync) falls back to an unresolved state.
                    self.carrier_close_state(&canonical_id, is_jsx)
                        .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
                            owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
                            ide_path: Some(ide_path.clone()),
                            api_path: None,
                            ..Default::default()
                        })
                };
                state.set_background_loaded(ProviderPathKind::Ide, true);
                // An owner-resolved (carrier) commit is GATED on the gateway receipt;
                // an UNRESOLVED open-document liveness state is membership-free and
                // commits through the plain non-carrier path. If the gateway did not
                // advertise this pass (transient compile/membership defer), the owned
                // state is left uncommitted and queued — NEVER committed ungated.
                if unresolved {
                    self.commit_provider_sync_state(&canonical_id, state);
                } else if let Some(receipt) = &owned_commit_receipt {
                    self.commit_carrier_provider_state(&canonical_id, state, receipt);
                } else {
                    self.queue_snapshot_provider_sync(canonical_id.clone());
                }

                // FIX-7: only NOW — after the new path synced and the new state
                // is committed — close the previous IDE path if it differs. On
                // failure (the arms below) the old path is left untouched.
                if let Some(stale_ide_path) = previous_ide_path
                    .as_ref()
                    .filter(|path| path.as_str() != ide_path.as_str())
                {
                    // Retire the stale path's recorded surface under a close
                    // EPOCH (a `Current` surface for a closed buffer would stay
                    // capturable); finalize only on a CONFIRMED close.
                    let close_token = self
                        .documents
                        .provider_surfaces()
                        .forget(stale_ide_path.as_str());
                    match sync.close_tsx(stale_ide_path).await {
                        Ok(()) => {
                            self.documents
                                .provider_surfaces()
                                .finalize_close(close_token);
                        }
                        Err(error) => {
                            tracing::warn!(
                                "ensure_current_file_synced: failed to close stale IDE path {}: {error}",
                                stale_ide_path
                            );
                        }
                    }
                }

                if unresolved {
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

    pub(super) async fn force_reopen_current_file_in_type_provider(&self, uri: &Uri) {
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
        let Some(ide_path) = self.active_ide_path_for_uri(uri) else {
            return;
        };

        // Retire the surface for the close half of the close+reopen: a capture
        // racing the gap must fail closed rather than resolve a surface whose
        // provider buffer is mid-flip. The successful reopen below re-records a
        // fresh generation (re-Currenting the path); the close token is then
        // intentionally superseded and never finalized.
        let close_token = self.documents.provider_surfaces().forget(&ide_path);
        let closed = match sync.close_tsx(&ide_path).await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(
                    "force_reopen_current_file_in_type_provider: failed to close {}: {error}",
                    ide_path
                );
                false
            }
        };

        match sync.open_tsx(&ide_path, &ide.code).await {
            Ok(()) => {
                // Record a fresh generation pinning the EXACT IDE bytes just
                // reopened (interactive queries capture this surface).
                self.record_carrier_ide_snapshot(
                    &canonical_id,
                    &ide_path,
                    &ide.code,
                    ide.source_map.as_deref(),
                );
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
                // Reopen failed AFTER a confirmed close: the buffer is fully
                // gone, so finalize the retire (the path is unknown until the
                // queued re-sync records it fresh). An unconfirmed close keeps
                // the `Closing` state (drop the token — fail closed).
                if closed {
                    self.documents
                        .provider_surfaces()
                        .finalize_close(close_token);
                }
                self.needs_ide_sync.insert(canonical_id);
            }
        }
    }

    /// Legacy wrapper for backward compat — calls `ensure_current_file_synced`.
    pub(super) async fn ensure_provider_synced(&self, uri: &Uri) {
        self.ensure_current_file_synced(uri).await;
        self.ensure_imported_carrier_apis_synced(uri).await;
        self.ensure_barrel_imports_synced(uri).await;
    }

    pub(super) async fn ensure_imported_carrier_apis_synced(&self, uri: &Uri) {
        if matches!(self.type_provider_kind, crate::TypeProviderKind::None) {
            return;
        }

        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return;
        };

        let mut import_ids = collect_imported_carrier_priority_ids_from_imports_with_fallback(
            &analysis.imports,
            Some(&canonical_id),
            |parent, specifier| self.resolve_import_specifier(parent, specifier),
        );

        let snapshot = self.published_resolver();
        let reader = LspProjectResolverReader::new(&self.documents);
        let dynamic_ids = collect_priority_carrier_public_api_targets_from_module_references(
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
            self.sync_imported_carrier_api_lightweight(&import_id).await;
        }
    }

    /// Sync barrel (non-carrier re-export) imports and their framework-carrier
    /// dependencies into the active type provider.
    ///
    /// When a component is imported through a barrel (`import { Comp } from './components'`),
    /// possibly across several `export *` / `export { … } from` hops, `ensure_imported_carrier_apis_synced`
    /// misses both the intermediate `.ts` barrels and the terminal carrier (`.vue` / `.svelte`)
    /// re-export targets. This walks the re-export graph reachable from the template's component
    /// usages (a bounded level-BFS), classifies each hop by its RESOLVED target's carrier-ness
    /// (never by the specifier string, so aliased `@/…` and `export *` re-exports are followed,
    /// and the terminal carrier is reached at any depth), syncs the discovered carrier
    /// dependencies first, then syncs the intermediate barrels. Provider-neutral: both tsgo and
    /// tsserver benefit (a bounded over-sync of unrelated barrel imports is acceptable — the
    /// provider decides the actual symbol).
    pub(super) async fn ensure_barrel_imports_synced(&self, uri: &Uri) {
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
        let mut barrel_carrier_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_carrier = HashSet::new();

        // Bounds (defensive): a pathological or cyclic re-export graph must never stall
        // did_open. Truncate (with a trace) rather than spin.
        const MAX_BFS_DEPTH: usize = 8;
        const MAX_NON_CARRIER_NODES: usize = 128;
        const MAX_RESOLVED_REFS: usize = 1024;
        const MAX_CARRIER_TARGETS: usize = 512;
        let mut resolved_refs_remaining: usize = MAX_RESOLVED_REFS;
        // Trace each size cap at most once — the first time a genuinely-new node is
        // dropped because the cap is full — so the truncation the comment promises is
        // observable. Cheap: one trace per cap, never per skipped item.
        let mut non_carrier_cap_traced = false;
        let mut carrier_cap_traced = false;

        // Seed the frontier from template component import sources that resolve to a
        // non-carrier (barrel) module. A directly-resolved carrier is already handled by
        // carrier sync.
        let mut frontier: Vec<String> = Vec::new();
        for component in &template.components {
            let Some(import_source) = component.import_source.as_deref() else {
                continue;
            };
            let Some(resolved) = self.resolve_import_specifier(&canonical_id, import_source) else {
                continue;
            };
            if verter_workspace::path_is_carrier(&resolved) {
                continue;
            }
            if seen_barrels.insert(resolved.clone()) {
                if barrel_ids.len() < MAX_NON_CARRIER_NODES {
                    frontier.push(resolved.clone());
                    barrel_ids.push(resolved);
                } else if !non_carrier_cap_traced {
                    non_carrier_cap_traced = true;
                    tracing::debug!(
                        "barrel sync: non-carrier node cap ({MAX_NON_CARRIER_NODES}) reached; \
                         truncating remaining barrel modules"
                    );
                }
            }
        }

        // Level-BFS over re-export hops. Each module reference is resolved through the shared
        // (alias-aware) workspace resolver and classified by its RESOLVED target's carrier-ness
        // — never by the specifier string — so `export * from './x'` and aliased (`@/…`)
        // re-exports are followed, and the terminal carrier is reached at any depth.
        let mut depth = 0usize;
        while !frontier.is_empty() && depth < MAX_BFS_DEPTH {
            let mut next: Vec<String> = Vec::new();
            for barrel_id in &frontier {
                host.ensure_loaded(barrel_id);
                let Some(barrel_analysis) = host.get_analysis(barrel_id) else {
                    continue;
                };
                for module_ref in barrel_analysis.module_references.iter() {
                    let Some(specifier) = module_ref.literal_specifier.as_deref() else {
                        continue;
                    };
                    if resolved_refs_remaining == 0 {
                        tracing::debug!(
                            "barrel sync: resolved-ref budget exhausted; truncating re-export walk"
                        );
                        break;
                    }
                    resolved_refs_remaining -= 1;
                    let Some(target) = self.resolve_import_specifier(barrel_id, specifier) else {
                        continue;
                    };
                    if verter_workspace::path_is_carrier(&target) {
                        if seen_barrel_carrier.insert(target.clone()) {
                            if barrel_carrier_deps.len() < MAX_CARRIER_TARGETS {
                                barrel_carrier_deps.push(target);
                            } else if !carrier_cap_traced {
                                carrier_cap_traced = true;
                                tracing::debug!(
                                    "barrel sync: carrier-target cap ({MAX_CARRIER_TARGETS}) reached; \
                                     truncating remaining carrier re-export targets"
                                );
                            }
                        }
                    } else if seen_barrels.insert(target.clone()) {
                        if barrel_ids.len() < MAX_NON_CARRIER_NODES {
                            next.push(target.clone());
                            barrel_ids.push(target);
                        } else if !non_carrier_cap_traced {
                            non_carrier_cap_traced = true;
                            tracing::debug!(
                                "barrel sync: non-carrier node cap ({MAX_NON_CARRIER_NODES}) reached; \
                                 truncating remaining barrel modules"
                            );
                        }
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        if !frontier.is_empty() {
            tracing::debug!(
                "barrel sync: BFS depth/size bound reached; truncating remaining re-export hops"
            );
        }

        // Sync carrier dependencies first (so the provider has their virtual
        // IDE targets).
        for carrier_id in &barrel_carrier_deps {
            self.sync_imported_carrier_api_lightweight(carrier_id).await;
        }

        // Sync barrel files. Carrier import specifiers already carry their
        // resolvable suffix before reaching the provider — the compiler rewrites
        // in-project carrier imports to the `.vue.tsx` IDE carrier, and the
        // resolver rewrites non-carrier importer specifiers to the `.verter.ts`
        // API carrier — so the provider sends content unmodified.
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
            // Framework carriers never sync as raw scripts.
            let Some(file_language) =
                crate::provider_sync::provider_script_language(host, barrel_id)
            else {
                continue;
            };
            let module_references = block_in_place_if_available(|| {
                host.upsert(verter_session::UpsertRequest {
                    canonical_id: Some(barrel_id.clone()),
                    input_id: barrel_id.clone(),
                    source: source.clone(),
                    file_language,
                    aliases: Vec::new(),
                })
                .map(|result| result.module_references)
                .unwrap_or_default()
            });
            let reader = LspProjectResolverReader::new(&self.documents);
            let Some(prepared) = prepare_non_carrier_provider_sync(
                Some(&snapshot),
                &reader,
                barrel_id,
                &source,
                &module_references,
            ) else {
                continue;
            };

            configure_provider_paths_for_source(sync, &snapshot, barrel_id, false).await;

            if let Some(transition) = self.prepare_non_carrier_provider_sync_transition(barrel_id) {
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

    pub(super) fn current_file_needs_inline_type_provider_sync(&self, uri: &Uri) -> bool {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return false;
        };

        if self.needs_ide_sync.contains(&canonical_id) {
            return true;
        }

        let Some(state) = self.provider_sync_state_for_source(&canonical_id) else {
            return true;
        };

        if state.is_unresolved()
            && self
                .published_resolver()
                .map(|snapshot| snapshot.ownership_ready)
                .unwrap_or(false)
        {
            return true;
        }

        if !state.ide_background_loaded {
            return true;
        }

        let Some(ide_path) = self.target_ide_path_for_uri(uri) else {
            return false;
        };

        state.ide_path.as_deref() != Some(ide_path.as_str())
    }

    /// Returns true if the user is actively typing (last change was within the cooldown window).
    /// Used to suppress non-critical TSGO requests (diagnostics, semantic tokens, inlay hints)
    /// during rapid typing.  TSGO processes requests serially, so queuing these during typing
    /// blocks interactive requests like completions.
    pub(super) fn is_typing_cooldown(&self) -> bool {
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

    /// Get the carrier IDE context for TypeProvider queries: `(ide_path,
    /// ide_code, mapper)`. Thin carrier-compat wrapper over
    /// [`Self::provider_projection_context`] for the two cross-file carrier
    /// callers (`ide_context_by_path` / `external_ide_context`) that need the
    /// narrow `(provider_path, provider_content, mapper)` shape; every feature
    /// handler routes through `provider_projection_context` /
    /// `type_provider_context`.
    pub(super) fn ide_context(
        &self,
        uri: &Uri,
    ) -> Option<(String, Arc<str>, ProviderPositionMapper)> {
        let ctx = self.provider_projection_context(uri)?;
        Some((ctx.provider_path, ctx.provider_content, ctx.mapper))
    }

    /// Capture the immutable provider-surface snapshot an interactive
    /// provider-backed query for `uri` must be built from — the request-scoped
    /// capture half of the fail-closed request-snapshot discipline.
    ///
    /// ONE point resolves everything: canonical id → projection kind → the
    /// surface's provider path → the store's CURRENT generation-stamped
    /// [`ProviderSurfaceSnapshot`]. The snapshot is immutable and internally
    /// consistent (content, source map, carrier source, line indexes all
    /// recorded from the SAME sync), so no interleaving `did_change`/`did_close`
    /// can tear the tuple the way the former independent live reads could.
    ///
    /// Fail-closed gates (any miss ⇒ `None`, never a partial/torn context):
    /// - no recorded CURRENT surface at the resolved provider path;
    /// - a surface whose kind does not match the document's projection
    ///   (`CarrierIde` for a carrier, `Shadow` for a self-file rune module);
    /// - a surface recorded for a DIFFERENT source canonical;
    /// - an open-document source that no longer byte-matches the captured
    ///   carrier source (an edit landed after the last successful sync — the
    ///   provider still holds the old surface, so a fresh mapper/content pair
    ///   would be torn, not merely stale).
    pub(super) fn capture_provider_request_surface(
        &self,
        uri: &Uri,
    ) -> Option<Arc<crate::provider_surface_store::ProviderSurfaceSnapshot>> {
        let canonical_id = self.documents.get_canonical_id(uri)?;
        self.documents.host().ensure_loaded(&canonical_id);

        let projection = self.documents.get_projection(uri)?;
        let store = self.documents.provider_surfaces();
        let snapshot = match projection {
            crate::documents::provider_projection::DocumentProviderProjection::CarrierIde {
                ..
            } => {
                // Carrier: the surface is keyed by the committed live carrier
                // IDE path. The committed-path read is a KEY lookup only — the
                // snapshot it resolves to is the sole content/mapper authority,
                // and the source/canonical gates below reject a stale key.
                let provider_path = self.active_ide_path_for_uri(uri)?;
                let snapshot = store.current_snapshot(&provider_path)?;
                (snapshot.kind == crate::provider_surface_store::ProviderSurfaceKind::CarrierIde)
                    .then_some(snapshot)?
            }
            crate::documents::provider_projection::DocumentProviderProjection::SelfFile {
                ..
            } => {
                // Self-file rune module: the provider buffer is served from the
                // module's OWN canonical path.
                let snapshot = store.current_snapshot(&canonical_id)?;
                (snapshot.kind == crate::provider_surface_store::ProviderSurfaceKind::Shadow)
                    .then_some(snapshot)?
            }
        };
        if snapshot.source_canonical.as_ref() != canonical_id.as_str() {
            return None;
        }
        self.request_surface_matches_live_source(uri, &snapshot)
            .then_some(snapshot)
    }

    /// Whether the OPEN document's live source still byte-matches the captured
    /// surface's carrier source — the source-identity half of the request-
    /// snapshot validation. A closed document (registry miss) does NOT match
    /// (fail closed: a `did_close` racing the request retires the context).
    pub(super) fn request_surface_matches_live_source(
        &self,
        uri: &Uri,
        snapshot: &crate::provider_surface_store::ProviderSurfaceSnapshot,
    ) -> bool {
        let Some(doc) = self.documents.get(uri) else {
            return false;
        };
        // Cheap same-allocation short-circuit before hashing.
        if std::ptr::eq(doc.source.as_ptr(), snapshot.carrier_source.as_ptr())
            && doc.source.len() == snapshot.carrier_source.len()
        {
            return true;
        }
        crate::provider_surface_store::ContentHash::of(&doc.source) == snapshot.source_hash
    }

    /// Whether a captured request surface is STILL valid — the post-await
    /// validation gate every provider-backed handler runs before mapping or
    /// publishing a provider response. Both halves are required:
    /// - `captured_snapshot_still_honored`: the store's current generation for
    ///   the path still agrees with the captured one (a mid-request re-sync
    ///   advancing the surface, or a close retiring it, invalidates);
    /// - the open document source still byte-matches the captured carrier
    ///   source (a mid-request edit invalidates even while the surface
    ///   generation is unchanged).
    ///
    /// `false` ⇒ the provider response was produced against a surface that no
    /// longer matches the live state; mapping it would be WRONG (not merely
    /// stale) — the caller must DROP the provider contribution.
    pub(super) fn provider_request_surface_still_valid(
        &self,
        uri: &Uri,
        snapshot: &crate::provider_surface_store::ProviderSurfaceSnapshot,
    ) -> bool {
        self.documents
            .provider_surfaces()
            .captured_snapshot_still_honored(snapshot)
            && self.request_surface_matches_live_source(uri, snapshot)
    }

    /// Post-await validation for a [`TypeProviderContext`]-carrying handler:
    /// `true` iff the context's captured surface is still honored AND the open
    /// document still matches it. On `false` the handler drops the provider
    /// contribution (fail closed).
    pub(super) fn provider_context_still_valid(
        &self,
        uri: &Uri,
        ctx: &super::TypeProviderContext,
    ) -> bool {
        self.provider_request_surface_still_valid(uri, &ctx.snapshot)
    }

    /// The generalized per-document provider-projection query context, serving
    /// BOTH the carrier-IDE projection (`.vue` / `.svelte` → IDE TSX) and the
    /// self-file projection (`.svelte.ts` / `.svelte.js` rune module → own-path
    /// provider buffer). This is the SOLE query path — there is no parallel
    /// rune-only query path.
    ///
    /// Built EXCLUSIVELY from ONE captured immutable
    /// [`ProviderSurfaceSnapshot`](crate::provider_surface_store::ProviderSurfaceSnapshot)
    /// (see [`Self::capture_provider_request_surface`]): the provider path,
    /// content, mapper, and BOTH line indexes all come from the same recorded
    /// surface, so the tuple can never be torn by a concurrent
    /// `did_change`/`did_close`. A surface with no usable source map fails
    /// closed (`None`) — a provider result could not be mapped back.
    ///
    /// - `provider_path`: the path the TypeProvider opened — the carrier IDE
    ///   path for a carrier, or the module's OWN canonical id for a self-file
    ///   rune module (its provider buffer is served from its own path).
    /// - `provider_content`: the bytes the TypeProvider type-checks (the IDE
    ///   TSX, or `<rune prelude> + <rewritten module bytes>`).
    /// - `mapper`: the unified source↔provider mapper (projection-agnostic).
    /// - `provider_line_index` / `source_line_index`: line indexes over the
    ///   captured provider content and captured carrier source, in the
    ///   session's negotiated encoding.
    /// - `snapshot`: the captured surface itself, for the post-await
    ///   re-validation gate ([`Self::provider_request_surface_still_valid`]).
    pub(super) fn provider_projection_context(
        &self,
        uri: &Uri,
    ) -> Option<ProviderProjectionContext> {
        let snapshot = self.capture_provider_request_surface(uri)?;
        // No usable source map ⇒ the provider's offsets could not be mapped
        // back onto the carrier ⇒ fail closed.
        let mapper = snapshot.source_map.as_ref().map(|m| (**m).clone())?;
        let encoding = self.documents.encoding();
        let provider_line_index = LineIndex::new(&snapshot.provider_content, encoding.clone());
        let source_line_index = LineIndex::new(&snapshot.carrier_source, encoding);
        Some(ProviderProjectionContext {
            provider_path: snapshot.stamp.provider_path.to_string(),
            provider_content: Arc::clone(&snapshot.provider_content),
            mapper,
            provider_line_index,
            source_line_index,
            snapshot,
        })
    }

    /// Sync an OPEN rune module's self-file provider buffer to the provider as
    /// UNRESOLVED open-document state, keyed at the module's OWN canonical path
    /// (the Shadow provider path), so it is QUERYABLE before resolver ownership
    /// is ready.
    ///
    /// Mirrors [`Self::sync_carrier_ide_unresolved`] but for a SELF-FILE rune
    /// module: the provider buffer is `<rune prelude> + <rewritten module
    /// bytes>`, the provider path is the canonical id itself (not a derived
    /// `.tsx` path), and it does NOT depend on
    /// `non_carrier_sync_state_for_source` (which requires resolver ownership).
    /// It refreshes the document's rewrite-aware projection from the same
    /// replacements it applied, so own-buffer position mapping is exact.
    pub(super) async fn sync_self_file_shadow_unresolved(&self, uri: &Uri) -> bool {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return false;
        };
        let Some(file_language) = super::server_utils::adapter_module_language_for(&canonical_id)
        else {
            return false;
        };
        let Some(sync) = &self.project_sync else {
            return false;
        };
        // Route through the SHARED self-file shadow-sync primitive — the SAME
        // path the debounced coordinator uses, so a rune module's Shadow state
        // is never forked between the editor ingress and the debounced tick.
        super::server_utils::sync_self_file_shadow_state(
            &self.documents,
            sync,
            &self.provider_sync_states,
            self.published_resolver().as_ref(),
            uri,
            &canonical_id,
            &file_language,
        )
        .await
    }

    /// Load the current workspace snapshot's resolver, if a published snapshot exists.
    ///
    /// Returns a `ResolverSnapshot`-like wrapper with a `.resolver` field for
    /// compatibility with existing access patterns (`snapshot.resolver.method()`).
    pub(super) fn published_resolver(&self) -> Option<PublishedResolverSnapshot> {
        let ws = self.vfs_workspace.read();
        let ws = ws.as_ref()?;
        let published = ws.load_published()?;
        Some(PublishedResolverSnapshot {
            resolver: published.snapshot.resolver.clone(),
            ownership_ready: published.ownership_ready,
        })
    }

    /// Check if a file is in an SSR context using the published LspViews.
    pub(super) fn is_ssr_context(&self, canonical_id: &str) -> bool {
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
    pub(super) fn find_project_root(&self, canonical_id: &str) -> Option<String> {
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
    pub(super) fn linter_for_file(&self, canonical_id: &str) -> verter_diagnostics::Linter {
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

    /// Generate an unresolved public API path (`Foo.vue.ts` / `Foo.svelte.ts`)
    /// without resolver ownership.
    ///
    /// Mirrors `provider_id_for_source()` for any framework carrier and is used
    /// during cold start before `background_init()` has built the resolver
    /// snapshot. The virtual suffix is the carrier-generic derivation, never a
    /// hardcoded `.vue` suffix.
    pub(super) fn unresolved_api_path_for_canonical_id(
        &self,
        canonical_id: &str,
    ) -> Option<String> {
        carrier_language_for(canonical_id)
            .is_some()
            .then(|| verter_workspace::carrier_api_provider_path(canonical_id))
    }

    pub(super) async fn sync_carrier_ide_unresolved(
        &self,
        canonical_id: &str,
        ide_code: &str,
        is_jsx: bool,
    ) -> bool {
        let Some(sync) = &self.project_sync else {
            return false;
        };
        // Carrier-generic IDE virtual-file derivation (`Foo.svelte` →
        // `Foo.svelte.tsx`), never a hardcoded `.vue` suffix. This bootstrap
        // unresolved path already knows it holds a carrier.
        let ide_path = verter_workspace::carrier_ide_provider_path(canonical_id, is_jsx);

        let mut state = self
            .provider_sync_state_for_source(canonical_id)
            .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
                owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
                ..Default::default()
            });
        // This is a bootstrap "unresolved" sync — unresolved BY DEFINITION. A
        // reused prior state may carry a stale `Owned` binding; force it back to
        // `Unresolved` so we never commit an `Owned` binding from here (which
        // would strand the file on a dead owner via a false `needs_owner_reconcile`).
        state.owner_binding = crate::provider_sync::ProviderOwnerBinding::Unresolved;

        let needs_open =
            state.ide_path.as_deref() != Some(ide_path.as_str()) || !state.ide_background_loaded;
        let result = if needs_open {
            sync.open_tsx(&ide_path, ide_code).await
        } else {
            sync.sync_tsx(&ide_path, ide_code).await
        };

        match result {
            Ok(()) => {
                // Record a fresh generation pinning the EXACT IDE bytes just
                // synced (before `ide_path` is moved). No source map in scope
                // here → the choke attaches the live IDE artifact's map only if
                // it still byte-matches `ide_code`.
                self.record_carrier_ide_snapshot(canonical_id, &ide_path, ide_code, None);
                state.ide_path = Some(ide_path);
                state.ide_background_loaded = true;
                self.commit_provider_sync_state(canonical_id, state);
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                true
            }
            Err(error) => {
                tracing::warn!("sync_carrier_ide_unresolved: failed for {canonical_id}: {error}");
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                false
            }
        }
    }

    pub(super) async fn sync_carrier_api_unresolved(
        &self,
        canonical_id: &str,
        api_code: &str,
    ) -> bool {
        let Some(sync) = &self.project_sync else {
            return false;
        };
        let Some(dts_path) = self.unresolved_api_path_for_canonical_id(canonical_id) else {
            return false;
        };

        let mut state = self
            .provider_sync_state_for_source(canonical_id)
            .unwrap_or_else(|| crate::provider_sync::ProviderSyncState {
                owner_binding: crate::provider_sync::ProviderOwnerBinding::Unresolved,
                ..Default::default()
            });
        // Bootstrap "unresolved" sync — unresolved BY DEFINITION. Force the
        // (possibly-reused) binding back to `Unresolved` so a stale `Owned`
        // binding from a prior committed state is never re-committed here.
        state.owner_binding = crate::provider_sync::ProviderOwnerBinding::Unresolved;

        let needs_open =
            state.api_path.as_deref() != Some(dts_path.as_str()) && !state.api_background_loaded;
        let result = if needs_open {
            sync.open_dts(&dts_path, api_code).await
        } else {
            sync.sync_dts(&dts_path, api_code).await
        };

        match result {
            Ok(()) => {
                // Record a fresh generation pinning the EXACT content just synced
                // under this virtual path (before `dts_path` is moved). No source
                // map in scope here → the choke uses the live map only if it still
                // byte-matches `api_code`.
                self.record_carrier_api_snapshot(canonical_id, &dts_path, api_code, None);
                state.api_path = Some(dts_path);
                state.api_background_loaded = true;
                self.commit_provider_sync_state(canonical_id, state);
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                true
            }
            Err(error) => {
                tracing::warn!("sync_carrier_api_unresolved: failed for {canonical_id}: {error}");
                self.queue_snapshot_provider_sync(canonical_id.to_string());
                false
            }
        }
    }

    /// Get the active IDE file path (.tsx or .jsx) currently materialized in the
    /// provider — i.e. a path the provider has actually opened.
    ///
    /// This must only return committed sync state, never a resolver-derived
    /// target path. Defense-in-depth for the editor-liveness invariant (R3-1):
    /// the path is returned ONLY when its kind is `ide_background_loaded` (live
    /// in the provider). Hover / completion / go-to use this path to query the
    /// type provider; returning a path the provider never opened would route
    /// those queries to a dead TSX (the `no ide_context` failure class). The
    /// write-side preserve discipline already guarantees a committed `ide_path`
    /// is only present when live; this gate keeps the read side honest even if a
    /// future producer regresses.
    pub(super) fn active_ide_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());

        self.provider_sync_states.get(&canonical).and_then(|state| {
            state
                .ide_background_loaded
                .then(|| state.ide_path.clone())
                .flatten()
        })
    }

    /// Get the target IDE path formula for a Vue file URI.
    ///
    /// This is safe for sync planning, but not for live provider queries.
    /// When no published resolver exists yet, fall back to the local
    /// unresolved carrier IDE formula (`Foo.svelte.tsx` / `Foo.vue.jsx`, …).
    pub(super) fn target_ide_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical = self
            .documents
            .get_canonical_id(uri)
            .unwrap_or_else(|| uri.as_str().to_string());
        let is_jsx = self.documents.is_jsx(uri);

        self.published_resolver()
            .and_then(|snapshot| {
                provider_ide_path_for_source(&snapshot.resolver, &canonical, is_jsx)
            })
            .or_else(|| {
                carrier_language_for(&canonical)
                    .is_some()
                    .then(|| verter_workspace::carrier_ide_provider_path(&canonical, is_jsx))
            })
    }

    /// Returns the active IDE path only when the provider is already bound to
    /// the current desired artifact path and can be updated in place.
    pub(super) fn eager_syncable_ide_path_for_uri(&self, uri: &Uri) -> Option<String> {
        let canonical_id = self.documents.get_canonical_id(uri)?;
        let state = self.provider_sync_state_for_source(&canonical_id)?;
        if !state.ide_background_loaded {
            return None;
        }
        let desired_path = self.target_ide_path_for_uri(uri)?;
        (state.ide_path.as_deref() == Some(desired_path.as_str())).then_some(desired_path)
    }

    /// Get IDE content and mapper by IDE path (reverse lookup).
    pub(super) fn ide_context_by_path(
        &self,
        ide_path: &str,
    ) -> Option<(String, Arc<str>, ProviderPositionMapper)> {
        let snapshot = self.published_resolver()?;
        let canonical_id = source_id_from_provider_carrier_path(
            &snapshot.resolver,
            self.documents.host(),
            ide_path,
        )?;
        let uri = self.documents.canonical_id_to_uri(&canonical_id)?;
        self.ide_context(&uri)
    }
    pub(super) async fn spawn_background_init(
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
            carrier_publish_coordinator: self.carrier_publish_coordinator.clone(),
            decl_overlay_owner: Arc::clone(&self.decl_overlay_owner),
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
    pub(super) async fn trigger_registry_rebuild(&self) {
        self.spawn_background_init(None, "vite config rebuild")
            .await;
    }

    /// Re-read a non-open .vue file from disk, upsert, compile, and sync it to the provider.
    /// Lightweight imported-Vue sync for `did_open`.
    ///
    /// Tries to generate and sync the required Vue artifacts without disk I/O:
    /// if the host already has the file in memory, `get_public_api` avoids
    /// re-reading from disk. Falls back to `resync_background_carrier_file` when
    /// the file hasn't been upserted yet.
    pub(super) async fn sync_imported_carrier_api_lightweight(&self, canonical_id: &str) {
        let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
        let profile = self.documents.tsx_profile.read().clone();
        let snapshot = self.published_resolver();
        let ownership_ready = snapshot
            .as_ref()
            .map(|s| s.ownership_ready)
            .unwrap_or(false);

        // Fast path: host already has the file — sync directly from cached artifacts.
        if let Some(api) = self.documents.host.get_public_api(canonical_id) {
            let ide = if is_tsgo {
                self.documents.host.get_ide(canonical_id, &profile)
            } else {
                None
            };

            if !ownership_ready {
                // Bootstrap: unresolved sync is allowed
                if let Some(ide) = ide.as_ref() {
                    let _ = self
                        .sync_carrier_ide_unresolved(canonical_id, &ide.code, ide.is_jsx)
                        .await;
                }
                let _ = self
                    .sync_carrier_api_unresolved(canonical_id, &api.code)
                    .await;
                return;
            }

            if let Some(sync) = &self.project_sync {
                let is_jsx = ide.as_ref().map(|output| output.is_jsx).unwrap_or(false);
                // Route through the SINGLE carrier-sync gateway (membership fused with
                // the provider-state transition + receipt). An imported child carrier
                // reaches tsserver as a store-backed configured-project member
                // (`Published`); tsgo opens the companions directly (`DirectOpen`).
                match self
                    .reconcile_carrier_via_gateway(canonical_id, is_jsx, ide.as_ref())
                    .await
                {
                    crate::external_ts::CarrierSyncDecision::Published {
                        committed_state,
                        receipt,
                    } => {
                        self.commit_carrier_provider_state(canonical_id, committed_state, &receipt);
                    }
                    crate::external_ts::CarrierSyncDecision::DirectOpen {
                        transition,
                        receipt,
                    } => {
                        // Owner-resolved: sync NEW paths first, then close stale-after-
                        // success (per-kind, skip-active). This branch can touch an open
                        // file, so a failed replacement must not close the live path.
                        let previous_state = self.provider_sync_state_for_source(canonical_id);
                        let stale_paths = transition.stale_paths;
                        let mut committed_state = transition.next;
                        let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

                        if let Some(ide) = ide.as_ref() {
                            if let Some(ide_path) = committed_state.ide_path.clone() {
                                let result = if committed_state.ide_background_loaded {
                                    sync.sync_tsx(&ide_path, &ide.code).await
                                } else {
                                    sync.open_tsx(&ide_path, &ide.code).await
                                };
                                if result.is_ok() {
                                    committed_state
                                        .set_background_loaded(ProviderPathKind::Ide, true);
                                    synced_kinds.push(ProviderPathKind::Ide);
                                    // Record a fresh generation pinning the EXACT IDE
                                    // bytes just synced (interactive queries capture
                                    // this surface).
                                    self.record_carrier_ide_snapshot(
                                        canonical_id,
                                        &ide_path,
                                        &ide.code,
                                        ide.source_map.as_deref(),
                                    );
                                } else if let Err(error) = result {
                                    tracing::warn!(
                                        "sync_imported_carrier_api_lightweight: failed for {ide_path}: {error}"
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
                                synced_kinds.push(ProviderPathKind::Api);
                                // Record a fresh generation pinning the EXACT content +
                                // its same-content source map under this virtual path.
                                self.record_carrier_api_snapshot(
                                    canonical_id,
                                    &dts_path,
                                    &api.code,
                                    api.source_map.as_deref(),
                                );
                            } else if let Err(e) = result {
                                tracing::warn!(
                                    "sync_imported_carrier_api_lightweight: failed for {dts_path}: {e}"
                                );
                                self.queue_snapshot_provider_sync(canonical_id.to_string());
                            }
                        }

                        self.commit_and_close_after_sync(
                            canonical_id,
                            previous_state.as_ref(),
                            committed_state,
                            &stale_paths,
                            &synced_kinds,
                            &receipt,
                        )
                        .await;
                    }
                    crate::external_ts::CarrierSyncDecision::Unowned => {
                        // Ready snapshot but no owner. Editor-liveness invariant: an
                        // OPEN imported `.vue` (imported-by-an-open-file) must keep its
                        // TSX live as Unresolved open-document state — NEVER clear+close.
                        // Only a genuinely non-open import is removed. The gateway has
                        // already RETRACTED the STORE/ledger membership; the open-document
                        // liveness commit below is membership-free (no receipt).
                        if self.documents.canonical_id_to_uri(canonical_id).is_some() {
                            self.preserve_open_unresolved_carrier(
                                canonical_id,
                                is_jsx,
                                ide.as_ref().map(|output| &*output.code),
                            )
                            .await;
                        } else {
                            self.clear_provider_sync_state(canonical_id).await;
                        }
                        self.queue_snapshot_provider_sync(canonical_id.to_string());
                    }
                    crate::external_ts::CarrierSyncDecision::Pending => {
                        self.queue_snapshot_provider_sync(canonical_id.to_string());
                    }
                }
            }
            return;
        }

        if !ownership_ready {
            // Bootstrap: no owner snapshot yet, unresolved sync allowed.
            let compiled = block_in_place_if_available(|| {
                self.documents.host.remove(canonical_id);
                if !self.documents.host.ensure_loaded(canonical_id) {
                    return false;
                }

                self.documents.host().ensure_loaded(canonical_id);

                // IDE-sync: gate on the IDE/TSX surface (not the runtime
                // `Main`) so a Main-less carrier (Svelte) is not skipped — its
                // IDE TSX still syncs to the provider below.
                let profile = self.documents.tsx_profile.read().clone();
                self.documents
                    .host
                    .ensure_ide_compiled(canonical_id, &profile)
                    .unwrap_or(false)
            });

            if compiled {
                if is_tsgo {
                    if let Some(ide) = self.documents.host.get_ide(canonical_id, &profile) {
                        let _ = self
                            .sync_carrier_ide_unresolved(canonical_id, &ide.code, ide.is_jsx)
                            .await;
                    }
                }
                if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                    let _ = self
                        .sync_carrier_api_unresolved(canonical_id, &api.code)
                        .await;
                    return;
                }
            }

            self.queue_snapshot_provider_sync(canonical_id.to_string());
            return;
        }

        // Slow path: file not in host yet — full disk read + upsert + compile + sync.
        self.resync_background_carrier_file(canonical_id).await;
    }

    pub(super) async fn resync_background_carrier_file(&self, canonical_id: &str) {
        tracing::info!(
            "resync_background: START {canonical_id} thread={:?}",
            std::thread::current().id()
        );
        // R5-2: detect owner-None / owner-loss and reconcile the binding BEFORE
        // the destructive disk reload + compile gate below. Owner resolution is
        // a pure resolver query, so a COMPILE FAILURE (the `compile_result`
        // early-return below) must not strand a previously-`Owned` OPEN `.vue`
        // on its dead owner. Route the owner-None case through the shared
        // reconcile with NO IDE output (`sync_compiled_carrier_to_provider` corrects
        // the binding — preserve open / clear non-open — without needing a
        // successful compile). Only run when a published resolver exists; the
        // bootstrap (pre-snapshot) window still falls through to the sync below.
        if let Some(snapshot) = self.published_resolver() {
            if crate::provider_sync::current_owner_binding_for_source(
                &snapshot.resolver,
                canonical_id,
            )
            .is_unresolved()
            {
                self.sync_compiled_carrier_to_provider(canonical_id, None)
                    .await;
                return;
            }
        }
        // Load from disk + upsert + compile (all blocking) — wrapped in block_in_place
        // to prevent tokio worker thread exhaustion during background sync.
        let compile_result = block_in_place_if_available(|| {
            self.documents.host.remove(canonical_id);
            if !self.documents.host.ensure_loaded(canonical_id) {
                tracing::debug!("resync_background: can't read {canonical_id}");
                return None;
            }

            self.documents.host().ensure_loaded(canonical_id);

            // Compile. IDE-sync: gate on the IDE/TSX surface (not the runtime
            // `Main`) so a Main-less carrier (Svelte) is not stranded — its IDE
            // TSX feeds `sync_compiled_carrier_to_provider` below.
            let profile = self.documents.tsx_profile.read().clone();
            if !self
                .documents
                .host
                .ensure_ide_compiled(canonical_id, &profile)
                .unwrap_or(false)
            {
                return None;
            }
            Some(profile)
        });
        tracing::info!("resync_background: COMPILED {canonical_id}");

        let Some(profile) = compile_result else {
            return;
        };

        self.refresh_carrier_dependency_tracking(canonical_id);

        let ide = self.documents.host.get_ide(canonical_id, &profile);
        self.sync_compiled_carrier_to_provider(canonical_id, ide.as_ref())
            .await;
    }

    /// Sync an already-loaded + compiled `.vue` file's IDE/API artifacts to the
    /// type provider, owner-aware, with the editor-liveness + close-AFTER-
    /// successful-sync discipline.
    ///
    /// Separated from [`Self::resync_background_carrier_file`] (which owns the
    /// destructive disk reload) so the owner-resolution + sync DECISION is
    /// directly exercisable. `ide` is the compiled IDE output if available.
    ///
    /// R3-5: the owner-None binding reconciliation runs BEFORE requiring fresh
    /// IDE output. A transient compile miss (`ide == None`) must still let an
    /// owner-loss reconcile correct a previously-`Owned` OPEN `.vue`'s binding —
    /// otherwise it stays stranded on its stale owner (the `no ide_context`
    /// class). `is_jsx` defaults to false when the IDE output is absent (the
    /// desired-extension target for the unresolved-preserve path).
    pub(super) async fn sync_compiled_carrier_to_provider(
        &self,
        canonical_id: &str,
        ide: Option<&verter_session::IdeResponse>,
    ) {
        let Some(sync) = &self.project_sync else {
            return;
        };
        let is_tsgo = matches!(self.type_provider_kind, crate::TypeProviderKind::Tsgo);
        let is_jsx = ide.map(|output| output.is_jsx).unwrap_or(false);
        if is_tsgo {
            if let Some(snapshot) = self.published_resolver() {
                configure_provider_paths_for_source(sync, &snapshot, canonical_id, true).await;
            }
        }
        // Route the owner-resolved sync through the SINGLE carrier-sync gateway: the
        // membership decision is FUSED with the provider-state transition + the
        // sealed receipt that gates the commit. tsserver advertised ⇒ `Published`
        // (commit the store-resident state); tsgo ⇒ `DirectOpen` (per-kind open +
        // commit); owner loss ⇒ `Unowned` (retract done in the gateway; preserve the
        // open-document liveness state or clear a non-open file); bootstrap/degraded
        // ⇒ `Pending` (keep queued).
        match self
            .reconcile_carrier_via_gateway(canonical_id, is_jsx, ide)
            .await
        {
            crate::external_ts::CarrierSyncDecision::Published {
                committed_state,
                receipt,
            } => {
                self.commit_carrier_provider_state(canonical_id, committed_state, &receipt);
            }
            crate::external_ts::CarrierSyncDecision::DirectOpen {
                transition,
                receipt,
            } => {
                // Close-AFTER-sync (per-kind, skip-active): capture stale + prior
                // state, sync each kind, then commit (receipt-gated) + close
                // genuinely-stale.
                let previous_state = self.provider_sync_state_for_source(canonical_id);
                let stale_paths = transition.stale_paths;
                let mut committed_state = transition.next;
                let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

                // Owner-resolved IDE sync runs only when fresh IDE code is available.
                // On a transient IDE compile miss (`ide == None`) the owner binding is
                // still correct (`Owned`), so the IDE kind simply does not sync this
                // pass — the API kind below may still sync, and no stale binding is
                // left behind.
                if let (Some(ide), Some(tsx_path)) = (ide, committed_state.ide_path.clone()) {
                    let is_bg = self
                        .is_background_loaded_for_source_kind(canonical_id, ProviderPathKind::Ide);
                    let result = if is_bg {
                        sync.sync_tsx(&tsx_path, &ide.code).await
                    } else {
                        sync.open_tsx(&tsx_path, &ide.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                        synced_kinds.push(ProviderPathKind::Ide);
                        // Record a fresh generation pinning the EXACT IDE bytes just
                        // synced (interactive queries capture this surface).
                        self.record_carrier_ide_snapshot(
                            canonical_id,
                            &tsx_path,
                            &ide.code,
                            ide.source_map.as_deref(),
                        );
                    } else if let Err(e) = result {
                        tracing::warn!("resync_background: failed to sync {canonical_id}: {e}");
                    }
                }

                // Sync .vue.ts as secondary provider support output.
                if let Some(api) = self.documents.host.get_public_api(canonical_id) {
                    if let Some(dts_path) = committed_state.api_path.clone() {
                        let is_bg = self.is_background_loaded_for_source_kind(
                            canonical_id,
                            ProviderPathKind::Api,
                        );
                        let result = if is_bg {
                            sync.sync_dts(&dts_path, &api.code).await
                        } else {
                            // First-time DTS sync: open_dts sends it to the provider
                            // (load_dts only caches locally, breaking cross-file ops).
                            sync.open_dts(&dts_path, &api.code).await
                        };
                        if result.is_ok() {
                            committed_state.set_background_loaded(ProviderPathKind::Api, true);
                            synced_kinds.push(ProviderPathKind::Api);
                            // Record a fresh generation pinning the synced content +
                            // its same-content source map under this virtual path.
                            self.record_carrier_api_snapshot(
                                canonical_id,
                                &dts_path,
                                &api.code,
                                api.source_map.as_deref(),
                            );
                        }
                    }
                }

                self.commit_and_close_after_sync(
                    canonical_id,
                    previous_state.as_ref(),
                    committed_state,
                    &stale_paths,
                    &synced_kinds,
                    &receipt,
                )
                .await;
            }
            crate::external_ts::CarrierSyncDecision::Unowned => {
                // Ready snapshot but no owner. Editor-liveness invariant: an OPEN Vue
                // document keeps its TSX live as Unresolved open-document state — only
                // a genuinely non-open (background) file is cleared. The gateway has
                // already RETRACTED the STORE/ledger membership; the open-document
                // liveness commit below is membership-free (no receipt).
                if self.documents.canonical_id_to_uri(canonical_id).is_some() {
                    self.preserve_open_unresolved_carrier(
                        canonical_id,
                        is_jsx,
                        ide.map(|output| &*output.code),
                    )
                    .await;
                } else {
                    self.clear_provider_sync_state(canonical_id).await;
                }
                self.queue_snapshot_provider_sync(canonical_id.to_string());
            }
            crate::external_ts::CarrierSyncDecision::Pending => {
                self.queue_snapshot_provider_sync(canonical_id.to_string());
            }
        }
    }
}
