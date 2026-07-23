//! Background import-dependency publication and the DependencyReady receipt.
//!
//! One rule governs this module: interactive request handlers NEVER start (or
//! inline-run) the imported-carrier + barrel delivery pass. The pass is
//! background-owned — enqueued by `did_open`, the post-edit debounce, the
//! post-init open-document sweep, and a handler readiness MISS — and only its
//! background completion mints the DependencyReady receipt (the import-set
//! freshness memo on [`super::ImportSyncMemo`]). Handlers may:
//!
//! - CAPTURE a committed receipt for the live revision and query the engine;
//! - ENQUEUE a background publication on a miss and return the best currently
//!   available answer without waiting for that pass.
//!
//! They may not do more. The previous request-started preamble ran inside the
//! deadline-cancelled handler body: its p90 sat on the definition deadline, the
//! deadline dropped the pass mid-flight, and the receipt (whose publication
//! requires a COMPLETE pass) was never recorded — so the next identical request
//! repeated the identical storm, a self-sustaining cancellation loop. Detaching
//! the pass makes cancellation of the REQUEST irrelevant to publication.
//!
//! SurfaceReady — the sibling receipt for the CURRENT file's committed IDE
//! surface — needs no store of its own: it is the existing committed-carrier
//! surface stamp, captured through
//! [`VerterLanguageServer::capture_provider_request_surface`] /
//! `type_provider_context` (both fail closed on a stale or missing surface).

use std::collections::HashSet;

use tower_lsp_server::ls_types::Uri;

use super::server_utils::*;
use super::sync_orchestration::ImportSyncOutcome;
use super::VerterLanguageServer;

/// The debounce for edit-triggered publication, matching the sync
/// coordinator's edit-silence window: one publication per typing burst.
const EDIT_DEBOUNCE_MS: u64 = 300;

type ModuleReferenceSignature = (
    u8,
    u8,
    bool,
    String,
    Option<String>,
    Vec<String>,
    Option<String>,
    u8,
);

/// How urgently an enqueued publication should run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PublicationUrgency {
    /// Run as soon as the per-document lane frees (open / readiness miss /
    /// post-init sweep).
    Immediate,
    /// Wait out the edit-silence window first; a newer edit-triggered enqueue
    /// supersedes this one (typing bursts coalesce onto one pass).
    EditDebounced,
}

/// Whether the import-dependency closure for a document is delivered at the
/// live revision — the handler-facing readiness verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DependencyReadiness {
    /// The DependencyReady receipt is committed for the live revision (or the
    /// document has no provider/dependency tracking to wait for): the handler
    /// may query the engine.
    Ready,
    /// No committed receipt. The handler answers without the provider; a
    /// background publication has been enqueued (or is already in flight) to
    /// heal the miss.
    NotReady,
}

/// Syntax-owned root set consumed by dependency publication. Source positions
/// and local import binding names are deliberately excluded: they do not change
/// which provider files the publication owns. Resolver changes are fenced by
/// the receipt's resolver-snapshot generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DependencyFrontierSignature {
    imports: Vec<(String, Option<String>, bool)>,
    module_references: Vec<ModuleReferenceSignature>,
}

impl DependencyFrontierSignature {
    fn is_rootless(&self) -> bool {
        self.imports.is_empty() && self.module_references.is_empty()
    }
}

impl DependencyReadiness {
    pub(super) fn is_ready(self) -> bool {
        matches!(self, DependencyReadiness::Ready)
    }
}

impl VerterLanguageServer {
    /// Capture the direct dependency roots for one document without walking the
    /// graph or touching the provider. Used to prove that a source edit preserved
    /// an already-delivered dependency closure.
    pub(super) fn dependency_frontier_signature(
        &self,
        canonical_id: &str,
    ) -> Option<DependencyFrontierSignature> {
        let ingress = self.documents.host().get_script_ingress(canonical_id)?;
        let mut imports = ingress
            .imports
            .iter()
            .map(|import| {
                (
                    import.source.clone(),
                    import.resolved_canonical_id.clone(),
                    import.is_type_only,
                )
            })
            .collect::<Vec<_>>();
        imports.sort();

        let mut module_references = ingress
            .module_references
            .iter()
            .map(|reference| {
                (
                    reference.syntax as u8,
                    reference.semantics as u8,
                    reference.is_type_only,
                    reference.raw_text.clone(),
                    reference.literal_specifier.clone(),
                    reference.finite_specifiers.clone(),
                    reference.static_prefix.clone(),
                    reference.analyzability as u8,
                )
            })
            .collect::<Vec<_>>();
        module_references.sort();
        Some(DependencyFrontierSignature {
            imports,
            module_references,
        })
    }

    /// Enqueue a detached background publication of `uri`'s import-dependency
    /// closure (imported carrier APIs + barrel re-export graph). Never awaited
    /// by callers; the spawned task outlives any request, so a request
    /// cancelled at its deadline cannot kill the pass (R4).
    pub(super) fn spawn_import_dependency_publication(
        &self,
        uri: &Uri,
        urgency: PublicationUrgency,
    ) {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        let debounce_epoch = match urgency {
            PublicationUrgency::Immediate => None,
            PublicationUrgency::EditDebounced => {
                Some(self.import_sync.bump_enqueue_epoch(&canonical_id))
            }
        };
        let server = self.clone();
        let uri = uri.clone();
        tokio::spawn(async move {
            if let Some(epoch) = debounce_epoch {
                tokio::time::sleep(std::time::Duration::from_millis(EDIT_DEBOUNCE_MS)).await;
                if !server
                    .import_sync
                    .enqueue_epoch_is_current(&canonical_id, epoch)
                {
                    // A newer edit-triggered enqueue owns the burst.
                    return;
                }
            }
            server
                .publish_import_dependencies(&uri, &canonical_id)
                .await;
        });
    }

    /// The BACKGROUND import-dependency publication pass for one document:
    /// per-document singleflight, freshness re-check, in-flight registration
    /// (used only to coalesce background enqueues), the imported-carrier +
    /// barrel delivery legs, and — only for a COMPLETE pass under a stable
    /// key — the DependencyReady receipt mint.
    ///
    /// A pass with any failed or requeued leg does NOT publish the receipt: the
    /// receipt records that the import set was successfully delivered at this
    /// generation, and a partial pass has not delivered it. The failed state is
    /// simply a cold memo — the next enqueue (readiness miss, edit, open)
    /// retries, so a transient failure never poisons and never strands.
    async fn publish_import_dependencies(&self, uri: &Uri, canonical_id: &str) {
        // Singleflight: coalesce concurrent enqueues onto ONE pass. A follower
        // that acquires the lock after the leader finished sees a fresh memo
        // and returns without re-walking.
        let lock = self.import_sync.lock_for(canonical_id);
        let _guard = lock.lock().await;

        let key = self.import_sync_freshness_key();
        if let Some(key) = key {
            if self.import_sync.is_fresh_at(canonical_id, key) {
                return; // The import set was already delivered at this generation.
            }
        }

        // Register the in-flight join handle only once this task IS the pass
        // (post-lock, post-freshness). Dropped at scope end — after the receipt
        // write below — so a woken joiner re-reads a settled memo.
        let _in_flight = self.import_sync.begin_in_flight(canonical_id);

        let outcome = self
            .ensure_imported_carrier_apis_synced(uri)
            .await
            .and(self.ensure_barrel_imports_synced(uri).await);

        // Publish the receipt only when the whole pass DELIVERED under a stable
        // key — never warm a torn generation, and never warm over a leg that has
        // still to be retried.
        if let Some(key) = key {
            if outcome.is_complete() && self.import_sync_freshness_key() == Some(key) {
                let rootless = self
                    .dependency_frontier_signature(canonical_id)
                    .is_some_and(|frontier| frontier.is_rootless());
                self.import_sync.record_delivered_with_rootless(
                    canonical_id.to_string(),
                    key,
                    rootless,
                );
            }
        }
    }

    /// Capture-only dependency readiness for interactive requests: never waits — a
    /// committed receipt reads [`DependencyReadiness::Ready`]; anything else
    /// reads `NotReady` after (at most) enqueueing a background publication.
    /// Non-destructive navigation callers still query the engine because a
    /// partial answer beats none mid-typing; rename consumes the verdict to
    /// fail closed on cross-file completeness. No caller joins publication.
    pub(super) fn dependency_readiness_capture(&self, uri: &Uri) -> DependencyReadiness {
        if self.type_provider.is_none() {
            return DependencyReadiness::Ready;
        }
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return DependencyReadiness::Ready;
        };
        let Some(key) = self.import_sync_freshness_key() else {
            // Bootstrap: no receipt dimension yet. Enqueue best-effort
            // delivery and report the honest not-ready state; rename fails
            // closed while navigation still queries its current provider view.
            self.spawn_import_dependency_publication(uri, PublicationUrgency::Immediate);
            return DependencyReadiness::NotReady;
        };
        if self.import_sync.is_fresh_at(&canonical_id, key) {
            return DependencyReadiness::Ready;
        }
        if self.import_sync.in_flight_watch(&canonical_id).is_none() {
            self.spawn_import_dependency_publication(uri, PublicationUrgency::Immediate);
        }
        DependencyReadiness::NotReady
    }

    /// The workspace `(content_generation, resolver_snapshot_generation)` pair
    /// that keys the DependencyReady receipt. `None` when no published resolver
    /// exists yet (bootstrap) — publication then delivers without minting.
    pub(super) fn import_sync_freshness_key(&self) -> Option<(u64, u64)> {
        let content_generation = self.documents.host().workspace_read().content_generation();
        let snapshot_generation = {
            let ws = self.vfs_workspace.read();
            let ws = ws.as_ref()?;
            ws.load_published()?.snapshot.generation.0
        };
        Some((content_generation, snapshot_generation))
    }

    /// Run the full publication pass INLINE and await it — TEST-ONLY setup
    /// path for suites that need a deterministic DependencyReady state before
    /// exercising a handler. Production callers use
    /// [`Self::spawn_import_dependency_publication`].
    #[cfg(test)]
    pub(super) async fn publish_import_dependencies_settled(&self, uri: &Uri) {
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return;
        };
        self.publish_import_dependencies(uri, &canonical_id).await;
    }

    /// TEST-ONLY direct entry to the barrel leg, for the barrel-BFS
    /// characterization suite. Production code reaches the leg exclusively
    /// through [`Self::publish_import_dependencies`].
    #[cfg(test)]
    pub(super) async fn ensure_barrel_imports_synced_for_test(&self, uri: &Uri) {
        let _ = self.ensure_barrel_imports_synced(uri).await;
    }

    /// Deliver the imported framework-carrier public-API surfaces (`import X
    /// from './X.vue'` plus dynamically referenced carriers) to the type
    /// provider. PRIVATE to the background publication pass: handlers must
    /// never run this inline (a compile error here is the architectural guard).
    async fn ensure_imported_carrier_apis_synced(&self, uri: &Uri) -> ImportSyncOutcome {
        if matches!(self.type_provider_kind, crate::TypeProviderKind::None) {
            return ImportSyncOutcome::Complete;
        }

        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return ImportSyncOutcome::Complete;
        };
        let Some(ingress) = self.documents.host().get_script_ingress(&canonical_id) else {
            return ImportSyncOutcome::Complete;
        };

        let mut import_ids = collect_imported_carrier_priority_ids_from_imports_with_fallback(
            &ingress.imports,
            Some(&canonical_id),
            |parent, specifier| self.resolve_import_specifier(parent, specifier),
        );

        let snapshot = self.published_resolver();
        let reader = LspProjectResolverReader::new(&self.documents);
        let dynamic_ids = collect_priority_carrier_public_api_targets_from_module_references(
            snapshot.as_ref(),
            &reader,
            &canonical_id,
            &ingress.module_references,
        );
        let mut seen: HashSet<String> = import_ids.iter().cloned().collect();
        for import_id in dynamic_ids {
            if seen.insert(import_id.clone()) {
                import_ids.push(import_id);
            }
        }

        let mut outcome = ImportSyncOutcome::Complete;
        for import_id in import_ids {
            if self.imported_carrier_already_delivered(&import_id) {
                continue;
            }
            outcome = outcome.and(self.sync_imported_carrier_api_lightweight(&import_id).await);
        }
        outcome
    }

    /// Whether `canonical_id`'s provider companions are ALREADY DELIVERED for
    /// its live bytes, so the background publication may skip it entirely.
    ///
    /// The publication is a steady-state-quiet pass: re-running the carrier
    /// gateway for a byte-fresh, already-loaded child would re-record a fresh
    /// provider-surface generation (failing a concurrent request's post-await
    /// surface validation into an empty answer) and, on tsserver, re-publish
    /// the store + fire a store-changed notification for content the engine
    /// already holds. Skip iff the committed state says the companion kinds
    /// are live AND the recorded surface still byte-matches the child's live
    /// source — an edited child (hash mismatch) always takes the full sync.
    fn imported_carrier_already_delivered(&self, canonical_id: &str) -> bool {
        let Some(state) = self.provider_sync_state_for_source(canonical_id) else {
            return false;
        };
        // BOTH companion kinds must be live. Checking only the API (or only a
        // present `ide_path`) is unsound: a pass whose IDE leg FAILED commits
        // API-only state with no `ide_path` at all, and a skip keyed on that
        // state would silently complete — and mint DependencyReady — over an
        // undelivered IDE companion.
        if !state.api_background_loaded || !state.ide_background_loaded {
            return false;
        }
        let store = self.documents.provider_surfaces();
        let recorded = state
            .api_path
            .as_deref()
            .and_then(|path| store.current_snapshot(path))
            .or_else(|| {
                state
                    .ide_path
                    .as_deref()
                    .and_then(|path| store.current_snapshot(path))
            });
        let Some(snapshot) = recorded else {
            return false;
        };
        if snapshot.source_canonical.as_ref() != canonical_id {
            return false;
        }
        let Some(live_source) = self.documents.host().get_source(canonical_id) else {
            return false;
        };
        crate::provider_surface_store::ContentHash::of(&live_source) == snapshot.source_hash
    }

    /// Sync barrel (non-carrier re-export) imports and their framework-carrier
    /// dependencies into the active type provider.
    ///
    /// When a component is imported through a barrel (`import { Comp } from './components'`),
    /// possibly across several `export *` / `export { … } from` hops, `ensure_imported_carrier_apis_synced`
    /// misses both the intermediate `.ts` barrels and the terminal carrier (`.vue` / `.svelte`)
    /// re-export targets. This walks only the `export ... from` graph reachable from the
    /// template's component usages (a cycle-terminated level-BFS), classifies each hop by its RESOLVED
    /// target's carrier-ness
    /// (never by the specifier string, so aliased `@/…` and `export *` re-exports are followed,
    /// and the terminal carrier is reached at any depth), syncs the discovered carrier
    /// dependencies first, then publishes only non-carrier modules whose provider projection
    /// differs from their on-disk bytes. Ordinary imports and unchanged compiled output remain
    /// provider-resolved from disk. PRIVATE to the background publication pass.
    async fn ensure_barrel_imports_synced(&self, uri: &Uri) -> ImportSyncOutcome {
        let Some(sync) = &self.project_sync else {
            return ImportSyncOutcome::Complete;
        };
        let Some(snapshot) = self.published_resolver() else {
            return ImportSyncOutcome::Complete;
        };
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return ImportSyncOutcome::Complete;
        };
        // A tsserver project that explicitly permits TypeScript-extension
        // imports and owns this framework carrier can keep the authored
        // `.vue`/`.svelte` specifiers: the plugin resolves them from the durable
        // store, so publishing rewritten barrel buffers would only create
        // duplicate script identities. Without that explicit permission, retain
        // the `.verter.ts` compatibility projection. The decision is made from
        // the effective owning project, never globally, so adjacent projects can
        // choose independently.
        let tsserver_uses_authored_specifiers =
            matches!(self.type_provider_kind, crate::TypeProviderKind::Tsserver)
                && verter_workspace::path_is_carrier(&canonical_id)
                && self
                    .vfs_workspace
                    .read()
                    .as_ref()
                    .and_then(|workspace| workspace.load_published())
                    .filter(|published| published.ownership_ready)
                    .is_some_and(|published| {
                        crate::carrier_provider_projection::configured_owners_allow_authored_carrier_specifiers(
                            &published.snapshot,
                            &canonical_id,
                        )
                    });
        if tsserver_uses_authored_specifiers {
            return ImportSyncOutcome::Complete;
        }
        let Some(ingress) = self.documents.host().get_script_ingress(&canonical_id) else {
            return ImportSyncOutcome::Complete;
        };

        let host = self.documents.host();
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_carrier_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_carrier = HashSet::new();

        // Cycles terminate through `seen_barrels`. A completeness receipt requires
        // the full finite closure, so the pass yields rather than truncating it.
        // Seed from the active document's direct syntax imports. This may include a
        // few non-component barrels, but avoids requiring template semantic analysis.
        // Direct carriers are already handled by carrier sync.
        let mut frontier: Vec<String> = Vec::new();
        for import in ingress.imports.iter() {
            let Some(resolved) = self.resolve_import_specifier(&canonical_id, &import.source)
            else {
                continue;
            };
            if verter_workspace::path_is_carrier(&resolved) {
                continue;
            }
            if seen_barrels.insert(resolved.clone()) {
                frontier.push(resolved.clone());
                barrel_ids.push(resolved);
            }
        }

        // Level-BFS over re-export hops. Each module reference is resolved through the shared
        // (alias-aware) workspace resolver and classified by its RESOLVED target's carrier-ness
        // — never by the specifier string — so `export * from './x'` and aliased (`@/…`)
        // re-exports are followed, and the terminal carrier is reached at any depth.
        let mut visited_since_yield = 0usize;
        while !frontier.is_empty() {
            let mut next: Vec<String> = Vec::new();
            for barrel_id in &frontier {
                // Order this node's host load with its document lifecycle (see
                // `sync_imported_carrier_api_lightweight`): a barrel can be an
                // OPEN document whose did_open commit is mid-flight.
                let barrel_analysis = {
                    let lifecycle_lane = self.ide_sync_lifecycle_lease(barrel_id);
                    let _lifecycle_guard = lifecycle_lane.lock().await;
                    host.ensure_loaded(barrel_id);
                    host.get_script_ingress(barrel_id)
                };
                let Some(barrel_analysis) = barrel_analysis else {
                    continue;
                };
                for module_ref in barrel_analysis.module_references.iter() {
                    if module_ref.syntax
                        != verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom
                    {
                        continue;
                    }
                    let Some(specifier) = module_ref.literal_specifier.as_deref() else {
                        continue;
                    };
                    let Some(target) = self.resolve_import_specifier(barrel_id, specifier) else {
                        continue;
                    };
                    if verter_workspace::path_is_carrier(&target) {
                        if seen_barrel_carrier.insert(target.clone()) {
                            barrel_carrier_deps.push(target);
                        }
                    } else if seen_barrels.insert(target.clone()) {
                        next.push(target.clone());
                        barrel_ids.push(target);
                    }
                }
                visited_since_yield += 1;
                if visited_since_yield >= 64 {
                    visited_since_yield = 0;
                    tokio::task::yield_now().await;
                }
            }
            frontier = next;
        }

        let mut outcome = ImportSyncOutcome::Complete;

        // Sync carrier dependencies first (so the provider has their virtual
        // IDE targets). Already-delivered byte-fresh carriers are skipped —
        // the same steady-state quietness as the direct-import leg.
        for carrier_id in &barrel_carrier_deps {
            if self.imported_carrier_already_delivered(carrier_id) {
                continue;
            }
            outcome = outcome.and(self.sync_imported_carrier_api_lightweight(carrier_id).await);
        }

        // Publish only barrel files whose export-from specifiers need a provider
        // rewrite (or whose framework self-file projection changes their bytes).
        // Unchanged non-carrier modules remain disk-resolved by the provider;
        // pushing them serializes a local package graph behind DependencyReady.
        for barrel_id in &barrel_ids {
            // Skip if already synced
            if let Some(state) = self.provider_sync_state_for_source(barrel_id) {
                if state.shadow_background_loaded {
                    continue;
                }
            }

            // Order this barrel's host read + provider sync with its document
            // lifecycle (see `sync_imported_carrier_api_lightweight`).
            let lifecycle_lane = self.ide_sync_lifecycle_lease(barrel_id);
            let _lifecycle_guard = lifecycle_lane.lock().await;

            let Some(source) = host.get_source(barrel_id) else {
                continue;
            };
            // Framework carriers never sync as raw scripts.
            if crate::provider_sync::provider_script_language(host, barrel_id).is_none() {
                continue;
            }
            let Some(ingress) = host.get_script_ingress(barrel_id) else {
                continue;
            };
            let module_references: Vec<verter_session::ScriptModuleReference> = ingress
                .module_references
                .iter()
                .filter(|reference| {
                    reference.syntax == verter_semantic::analysis::ModuleReferenceSyntax::ExportFrom
                })
                .map(verter_session::ScriptModuleReference::from)
                .collect();
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
            if prepared.rewritten.as_str() == source.as_ref() {
                continue;
            }

            if let Some(transition) = self.prepare_non_carrier_provider_sync_transition(barrel_id) {
                self.close_provider_paths(&transition.stale_paths).await;
                let result = sync
                    .sync_file(&prepared.provider_path, &prepared.rewritten)
                    .await;
                outcome = outcome.and(ImportSyncOutcome::from_sync(&result));
                if let Err(error) = result {
                    tracing::warn!(
                        "barrel sync: failed to sync {}: {error}",
                        prepared.provider_path
                    );
                } else {
                    self.commit_provider_sync_state(barrel_id, transition.next);
                }
            } else {
                let result = sync
                    .sync_file(&prepared.provider_path, &prepared.rewritten)
                    .await;
                outcome = outcome.and(ImportSyncOutcome::from_sync(&result));
                if let Err(error) = result {
                    tracing::warn!(
                        "barrel sync: failed to sync {}: {error}",
                        prepared.provider_path
                    );
                }
            }
        }
        outcome
    }
}
