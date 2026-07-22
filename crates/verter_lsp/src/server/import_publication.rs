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
//! - JOIN a publication already in flight (await its watch, then re-check);
//! - ENQUEUE a background publication and answer without the provider.
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

use super::handler_guard::block_in_place_if_available;
use super::server_utils::*;
use super::sync_orchestration::ImportSyncOutcome;
use super::VerterLanguageServer;

/// The debounce for edit-triggered publication, matching the sync
/// coordinator's edit-silence window: one publication per typing burst.
const EDIT_DEBOUNCE_MS: u64 = 300;

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

impl DependencyReadiness {
    pub(super) fn is_ready(self) -> bool {
        matches!(self, DependencyReadiness::Ready)
    }
}

impl VerterLanguageServer {
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
    /// (the join handle interactive requests await), the imported-carrier +
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
                self.import_sync
                    .record_delivered(canonical_id.to_string(), key);
            }
        }
    }

    /// Dependency readiness for an interactive navigation request
    /// (definition / typeDefinition / rename): capture the committed receipt,
    /// JOIN an in-flight publication for the live revision, or enqueue a
    /// background publication and report [`DependencyReadiness::NotReady`].
    ///
    /// Joining awaits work that is ALREADY running — bounded by the caller's
    /// ambient request deadline — and re-checks the receipt when it settles.
    /// This method never runs a delivery leg inline and never takes the
    /// publication lane lock, so it can never become the request-started
    /// preamble it replaces.
    pub(super) async fn dependency_readiness_join(&self, uri: &Uri) -> DependencyReadiness {
        if self.type_provider.is_none() {
            // No in-process provider: there is no engine query to gate.
            return DependencyReadiness::Ready;
        }
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            // No canonical identity ⇒ no dependency tracking applies; the
            // provider-context capture is the remaining (fail-closed) gate.
            return DependencyReadiness::Ready;
        };
        loop {
            let Some(key) = self.import_sync_freshness_key() else {
                // Bootstrap (no published resolver): receipts cannot be minted
                // yet. Publication still delivers what it can; the post-init
                // sweep re-publishes and mints once a snapshot exists.
                self.spawn_import_dependency_publication(uri, PublicationUrgency::Immediate);
                return DependencyReadiness::NotReady;
            };
            if self.import_sync.is_fresh_at(&canonical_id, key) {
                return DependencyReadiness::Ready;
            }
            match self.import_sync.in_flight_watch(&canonical_id) {
                Some(mut watch) => {
                    // JOIN: await the running publication (value change or
                    // sender drop both resolve), then re-check from the top.
                    if watch.changed().await.is_err() {
                        // Dead registration (publisher panicked between
                        // registering and its guard drop): clear it so the
                        // re-check enqueues instead of spinning.
                        self.import_sync.clear_dead_in_flight(&canonical_id, &watch);
                    }
                }
                None => {
                    self.spawn_import_dependency_publication(uri, PublicationUrgency::Immediate);
                    return DependencyReadiness::NotReady;
                }
            }
        }
    }

    /// Capture-only dependency readiness for completion: never waits — a
    /// committed receipt reads [`DependencyReadiness::Ready`]; anything else
    /// reads `NotReady` after (at most) enqueueing a background publication.
    /// Completion still queries the engine either way (a partial member list
    /// beats none mid-typing); the verdict exists for callers that want it and
    /// for the enqueue-on-miss healing side effect.
    pub(super) fn dependency_readiness_capture(&self, uri: &Uri) -> DependencyReadiness {
        if self.type_provider.is_none() {
            return DependencyReadiness::Ready;
        }
        let Some(canonical_id) = self.documents.get_canonical_id(uri) else {
            return DependencyReadiness::Ready;
        };
        let Some(key) = self.import_sync_freshness_key() else {
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
    fn import_sync_freshness_key(&self) -> Option<(u64, u64)> {
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
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return ImportSyncOutcome::Complete;
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

        let mut outcome = ImportSyncOutcome::Complete;
        for import_id in import_ids {
            outcome = outcome.and(self.sync_imported_carrier_api_lightweight(&import_id).await);
        }
        outcome
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
    /// provider decides the actual symbol). PRIVATE to the background publication pass.
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
        let Some(analysis) = self.documents.get_analysis(uri) else {
            return ImportSyncOutcome::Complete;
        };
        let Some(template) = analysis.template.as_ref() else {
            return ImportSyncOutcome::Complete;
        };

        let host = self.documents.host();
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_carrier_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_carrier = HashSet::new();

        // Bounds (defensive): a pathological or cyclic re-export graph must never stall
        // the background pass. Truncate (with a trace) rather than spin.
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

        let mut outcome = ImportSyncOutcome::Complete;

        // Sync carrier dependencies first (so the provider has their virtual
        // IDE targets).
        for carrier_id in &barrel_carrier_deps {
            outcome = outcome.and(self.sync_imported_carrier_api_lightweight(carrier_id).await);
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
