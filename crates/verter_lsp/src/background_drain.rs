//! Snapshot-driven provider-sync drain lifecycle.
//!
//! After `background_init` publishes the workspace snapshot, open and imported
//! files queued during the pre-snapshot bootstrap window are reconciled against
//! the resolved owners here: the drain syncs each file's IDE/API artifacts into
//! the type provider, upgrades unresolved open-document state to owner-aware
//! state, and retires genuinely-stale provider paths (close-after-sync).
//!
//! Split out of `background_init` (a sibling `#[path]` child module of
//! `server`); both share `use super::*;` so the same `super::` / `crate::`
//! paths resolve. This module owns the editor-liveness invariant for the drain:
//! an OPEN Vue document's provider state is preserved (never closed) while
//! ownership is unresolved, and a failed owner transition leaves the previous
//! open path alive.

use super::*;

#[path = "background_drain_owner_loss.rs"]
mod owner_loss;
use owner_loss::{reconcile_unowned_carrier_buffer, reconcile_unowned_carrier_provider_file};

/// Outcome of a single pending-file provider-sync pass, used by the drain loop
/// to decide whether to DEQUEUE the file or KEEP it for a later retry.
///
/// A pass may sync multiple kinds (IDE `.tsx` + API `.vue.ts`) independently;
/// per-kind partial-failure is real (one kind syncs, another fails and reverts).
/// Removing the file from the pending set whenever *any* kind synced would
/// permanently suppress the failed kind — it would never be retried. The drain
/// therefore dequeues only on [`SyncOutcome::FullyReconciled`] (every kind
/// synced) or [`SyncOutcome::Terminal`] (a settled terminal no-owner state) — see
/// [`sync_outcome_dequeues`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyncOutcome {
    /// Every intended kind synced this pass (or there was nothing to sync and
    /// the committed state is already consistent). Safe to dequeue.
    FullyReconciled,
    /// The carrier settled into a TERMINAL no-owner state (`NoProject` /
    /// `Ambiguous`). Safe to dequeue: retrying a terminal ownership decision
    /// would only re-run the same fail-closed result. A later config change that
    /// resolves an owner is re-driven by that change's own reconcile, not by a
    /// stale drain retry.
    Terminal,
    /// At least one kind synced but at least one OTHER intended kind FAILED.
    /// Keep the file queued so the failed kind is retried on a later drain.
    Partial,
    /// Nothing synced this pass — a total sync failure, a transient advertise
    /// miss (`Pending`), or a still-transient `NotReady` bootstrap awaiting an
    /// authoritative owner. The sole RETRYABLE outcomes; keep queued.
    Nothing,
}

/// Whether a drain [`SyncOutcome`] should DEQUEUE the carrier from the pending set.
///
/// Dequeue on [`SyncOutcome::FullyReconciled`] (every kind synced) and
/// [`SyncOutcome::Terminal`] (a settled terminal no-owner state — never retried).
/// [`SyncOutcome::Partial`] and [`SyncOutcome::Nothing`] stay queued so a failed
/// kind or a still-transient (`NotReady` / `Pending`) carrier is retried.
pub(super) fn sync_outcome_dequeues(outcome: SyncOutcome) -> bool {
    matches!(
        outcome,
        SyncOutcome::FullyReconciled | SyncOutcome::Terminal
    )
}

/// The live carrier-publish context threaded into the drain for the tsserver
/// engine. When present, a carrier's companions are PUBLISHED into the on-disk
/// store the `@verter/typescript-plugin` reads (making the carrier a configured-
/// project member) INSTEAD of being opened directly into the provider via
/// `provider.open_file`. `None` for tsgo (whose carrier companions reach the
/// engine through the project-bound `--api` direct open — `open_project` +
/// `root_files`) and for unit tests that assert the mock provider's open/sync calls.
pub(super) struct CarrierPublishCtx<'a> {
    /// The tsserver publish coordinator (drives the store-publish membership), or
    /// `None` for tsgo direct-open. Ownership is resolved from `vfs` for BOTH engines.
    pub(super) coordinator: Option<&'a crate::external_ts::CarrierPublishCoordinator>,
    /// The managed semantic provider's delivery leg is independent from the
    /// editor membership store.
    pub(super) provider_delivery: crate::external_ts::CarrierProviderDelivery,
    /// The published filesystem workspace — the SINGLE ownership-resolution source for
    /// both engines.
    pub(super) vfs: Arc<verter_workspace::FilesystemWorkspace>,
    /// Whether the captured ownership snapshot is authoritative (vs cold-bootstrap):
    /// the reconciler's cold-vs-ready signal so a cold drain defers without thrash.
    pub(super) ownership_ready: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn drain_pending_snapshot_provider_sync(
    project_sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    pending_snapshot_provider_sync: &DashSet<String>,
    is_tsgo: bool,
    mru_canonical_ids: Option<&parking_lot::Mutex<Vec<String>>>,
    carrier_publish_coordinator: Option<&crate::external_ts::CarrierPublishCoordinator>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    if project_sync.is_none() && carrier_publish_coordinator.is_none() {
        pending_snapshot_provider_sync.clear();
        return;
    }
    // Capture the published filesystem workspace once (the carrier-publish
    // ownership-resolution source) alongside the resolver snapshot, so the
    // tsserver publish path resolves against the same published snapshot.
    let (snapshot, vfs_handle) = {
        let ws = vfs_workspace.read();
        let Some(ws) = ws.as_ref() else {
            return;
        };
        let Some(published) = ws.load_published() else {
            return;
        };
        (
            super::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            },
            Arc::clone(ws),
        )
    };
    // The carrier-publish context — ALWAYS present here (a published `vfs_handle` was
    // captured above), carrying the single ownership-resolution vfs for BOTH engines.
    // Its `coordinator` is `None` for tsgo (project-bound `--api` direct
    // carrier-companion open) and when no coordinator is wired.
    let carrier_publish = CarrierPublishCtx {
        coordinator: carrier_publish_coordinator,
        provider_delivery: if is_tsgo {
            crate::external_ts::CarrierProviderDelivery::DirectOpen
        } else {
            crate::external_ts::CarrierProviderDelivery::StoreBacked
        },
        vfs: Arc::clone(&vfs_handle),
        ownership_ready: snapshot.ownership_ready,
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
        let outcome = sync_pending_snapshot_provider_file(
            project_sync,
            documents,
            &snapshot,
            provider_sync_states,
            &canonical_id,
            is_tsgo,
            Some(&carrier_publish),
            carrier_coordinator,
        )
        .await;

        // Dequeue when the file is fully reconciled (every intended kind synced),
        // when it settled into a TERMINAL no-owner state (never retried), or when
        // its source has vanished. A `Partial` outcome (a kind failed and was
        // reverted to its prior live path) MUST stay queued so the failed kind is
        // retried on a later drain — otherwise it is permanently suppressed.
        // `Nothing` (total failure / transient `NotReady` / `Pending`) also stays.
        if sync_outcome_dequeues(outcome) || documents.host.get_source(&canonical_id).is_none() {
            pending_snapshot_provider_sync.remove(&canonical_id);
        }
    }
}

/// Re-resolve aliased imports for all currently open `.vue` files and sync any
/// newly-discovered imported `.vue.ts` files to the type provider.
///
/// During `did_open`, aliased imports (e.g., `@/components/MyComp.vue`) fail to
/// resolve because `project_registry` is `None` — it's populated later by
/// `background_init`. This function runs **after** the registry is committed and
/// re-runs the same import-collection pipeline, so the provider gets the missing
/// `.vue.ts` files before the E2E diagnostic check.
#[allow(clippy::too_many_arguments)]
pub(super) async fn resync_aliased_imports_for_open_files(
    documents: &DocumentRegistry,
    project_sync: Option<&ProjectSync>,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    is_tsgo: bool,
    carrier_publish_coordinator: Option<&crate::external_ts::CarrierPublishCoordinator>,
    decl_overlay_owner: &DeclOverlayOwner,
    pass_generation: u64,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> bool {
    let Some(sync) = project_sync else {
        return false;
    };
    let (snapshot, vfs_handle) = {
        let ws = vfs_workspace.read();
        match ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some((
                super::PublishedResolverSnapshot {
                    resolver: published.snapshot.resolver.clone(),
                    ownership_ready: published.ownership_ready,
                },
                Arc::clone(ws),
            ))
        }) {
            Some((snapshot, vfs)) => (Some(snapshot), Some(vfs)),
            None => (None, None),
        }
    };
    // The carrier-publish context for the aliased-import carrier sync — present
    // whenever a published `vfs` was captured (the single ownership-resolution source);
    // its `coordinator` is `None` for tsgo.
    let carrier_publish = vfs_handle.map(|vfs| CarrierPublishCtx {
        coordinator: carrier_publish_coordinator,
        provider_delivery: if is_tsgo {
            crate::external_ts::CarrierProviderDelivery::DirectOpen
        } else {
            crate::external_ts::CarrierProviderDelivery::StoreBacked
        },
        vfs,
        ownership_ready: snapshot
            .as_ref()
            .map(|s| s.ownership_ready)
            .unwrap_or(false),
    });
    let snapshot = match snapshot {
        Some(s) => s,
        None => return false,
    };

    let host = documents.host();
    let mut synced_any = false;
    let mut all_import_ids: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for uri_str in documents.open_uris() {
        let Ok(uri) = uri_str.parse::<Uri>() else {
            continue;
        };
        let Some(canonical_id) = documents.get_canonical_id(&uri) else {
            continue;
        };
        if carrier_language_for(&canonical_id).is_none() {
            continue;
        }
        let Some(analysis) = host.get_analysis(&canonical_id) else {
            continue;
        };

        // Static imports (same pipeline as did_open line 6103)
        let ids = collect_imported_carrier_priority_ids_from_imports_with_fallback(
            &analysis.imports,
            Some(&canonical_id),
            |parent, specifier| resolve_import_specifier_standalone(host, parent, specifier),
        );

        // Dynamic imports via module_references
        let reader = LspProjectResolverReader::new(documents);
        let dynamic_ids = collect_priority_carrier_public_api_targets_from_module_references(
            Some(&snapshot),
            &reader,
            &canonical_id,
            &analysis.module_references,
        );

        for id in ids.into_iter().chain(dynamic_ids) {
            if seen.insert(id.clone()) {
                all_import_ids.push(id);
            }
        }
    }

    // Lightweight sync: compile and sync the provider artifacts needed by the backend.
    for import_id in &all_import_ids {
        if let Some(state) = provider_sync_states.get(import_id.as_str()) {
            let already_loaded = if is_tsgo {
                state.ide_background_loaded && state.api_background_loaded
            } else {
                state.api_background_loaded
            };
            // R2-4: only short-circuit an already-loaded import when its
            // committed owner binding STILL matches the live snapshot resolution.
            // An OPEN `.vue` whose owner changed or disappeared must NOT be
            // skipped on a stale binding — it would otherwise stay stranded on
            // the dead owner (the `no ide_context` class). For a non-open import
            // a fully-loaded binding is left as-is (closed files have no editor-
            // liveness invariant to reconcile here).
            if already_loaded {
                let is_open = documents.canonical_id_to_uri(import_id).is_some();
                let binding_current = !is_open
                    || crate::provider_sync::committed_binding_matches_current(
                        &state,
                        &crate::provider_sync::current_owner_binding_for_source(
                            &snapshot.resolver,
                            import_id,
                        ),
                    );
                if binding_current {
                    continue;
                }
            }
        }

        // Load dependency into host (also feeds the scheduler via upsert).
        // The scheduler's extract_deps + auto-ingress handles recursive
        // dependency walking, replacing the old hydrate_cached flow.
        if !host.ensure_loaded(import_id) {
            continue;
        }

        // R5-2: detect owner-None / owner-loss and reconcile the open file's
        // BINDING BEFORE the compile gate below. Owner resolution is a pure
        // resolver query (it does not need compile output), so a COMPILE FAILURE
        // must not short-circuit the reconcile and strand a previously-`Owned`
        // OPEN `.vue` on its dead owner. The reconcile preserves an open file's
        // live TSX (Unresolved binding, owner-derived `.vue.ts` dropped+closed)
        // and corrects the binding even without fresh IDE output (`ide = None`).
        if crate::provider_sync::current_owner_binding_for_source(&snapshot.resolver, import_id)
            .is_unresolved()
        {
            reconcile_unowned_carrier_provider_file(
                sync,
                documents,
                provider_sync_states,
                &snapshot,
                import_id,
                None,
                "aliased_resync",
                carrier_publish.as_ref(),
                carrier_coordinator,
            )
            .await;
            continue;
        }

        // Compile to generate public API. IDE-sync: gate on the IDE/TSX surface
        // (not the runtime `Main`) so a Main-less carrier (Svelte) — which has a
        // `CachedTsx` but no `Main` — is not skipped. `Ok(false)` (no IDE
        // surface) skips; otherwise proceed to the owner-aware provider sync.
        let profile = documents.tsx_profile.read().clone();
        if !host
            .ensure_ide_compiled(import_id, &profile)
            .unwrap_or(false)
        {
            continue;
        }

        if is_tsgo {
            configure_provider_paths_for_source(sync, &snapshot, import_id, true).await;
        }

        let ide = if is_tsgo {
            host.get_ide(import_id, &profile)
        } else {
            None
        };

        // Route the owner-resolved carrier (or an owner lost mid-flight) through the
        // SINGLE carrier-sync gateway: tsserver publishes the membership, tsgo opens
        // the companions directly, and a mid-flight owner loss is reconciled inside
        // (a `NotReady` / `Unresolved` no-owner outcome). Any synced kind is progress.
        if let CarrierApplyOutcome::Applied { synced, .. } = apply_owner_resolved_carrier_sync(
            Some(sync),
            documents,
            provider_sync_states,
            &snapshot,
            import_id,
            ide.as_ref(),
            "aliased_resync",
            carrier_publish.as_ref(),
            carrier_coordinator,
        )
        .await
        {
            if !synced.is_empty() {
                synced_any = true;
            }
        }
    }

    // Pass 2 (TSGO only): Sync barrel imports discovered from template component usages.
    // When a component is imported through a barrel (non-carrier re-export file), the
    // carrier file collection above misses both the barrel and its carrier re-export
    // targets. This pass follows the barrel → carrier re-export chain and syncs both.
    if is_tsgo {
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_carrier_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_carrier = HashSet::new();

        for uri_str in documents.open_uris() {
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };
            let Some(canonical_id) = documents.get_canonical_id(&uri) else {
                continue;
            };
            if carrier_language_for(&canonical_id).is_none() {
                continue;
            }
            let Some(analysis) = host.get_analysis(&canonical_id) else {
                continue;
            };
            let Some(template) = analysis.template.as_ref() else {
                continue;
            };

            for component in &template.components {
                let Some(import_source) = component.import_source.as_deref() else {
                    continue;
                };
                let Some(resolved) =
                    resolve_import_specifier_standalone(host, &canonical_id, import_source)
                else {
                    continue;
                };
                if verter_workspace::path_is_carrier(&resolved) {
                    continue; // a directly-resolved carrier is already handled by the carrier pass
                }
                if !seen_barrels.insert(resolved.clone()) {
                    continue;
                }

                // Load the barrel into the host and scan its module references
                // for carrier (`.vue`, `.svelte`, …) specifiers. This avoids the
                // chicken-and-egg problem where get_export_span_follow_reexports
                // needs carrier files already loaded.
                host.ensure_loaded(&resolved);

                if let Some(barrel_analysis) = host.get_analysis(&resolved) {
                    for module_ref in barrel_analysis.module_references.iter() {
                        if let Some(specifier) = &module_ref.literal_specifier {
                            if verter_workspace::path_is_carrier(specifier) {
                                if let Some(carrier_id) =
                                    resolve_import_specifier_standalone(host, &resolved, specifier)
                                {
                                    if verter_workspace::path_is_carrier(&carrier_id)
                                        && seen_barrel_carrier.insert(carrier_id.clone())
                                    {
                                        barrel_carrier_deps.push(carrier_id);
                                    }
                                }
                            }
                        }
                    }
                }

                barrel_ids.push(resolved);
            }
        }

        // Sync Vue dependencies first (so TSGO has .vue.ts targets before barrel)
        for carrier_id in &barrel_carrier_deps {
            // Skip if already synced in the main Vue pass — but only when an OPEN
            // barrel-dep `.vue`'s committed binding STILL matches the live
            // resolution (R2-4). An owner change/loss on an open barrel dep must
            // fall through to reconciliation, never short-circuit on a stale
            // binding. A non-open dep keeps its fully-loaded binding as-is.
            if let Some(state) = provider_sync_states.get(carrier_id.as_str()) {
                if state.ide_background_loaded && state.api_background_loaded {
                    let is_open = documents.canonical_id_to_uri(carrier_id).is_some();
                    let binding_current = !is_open
                        || crate::provider_sync::committed_binding_matches_current(
                            &state,
                            &crate::provider_sync::current_owner_binding_for_source(
                                &snapshot.resolver,
                                carrier_id,
                            ),
                        );
                    if binding_current {
                        continue;
                    }
                }
            }

            if !host.ensure_loaded(carrier_id) {
                continue;
            }

            // R5-2: detect owner-None / owner-loss and reconcile the open file's
            // BINDING BEFORE the compile gate below (mirrors the aliased pass).
            // Owner resolution is a pure resolver query, so a COMPILE FAILURE
            // must not strand a previously-`Owned` open barrel-dep on its dead
            // owner. The reconcile corrects the binding without fresh IDE output.
            if crate::provider_sync::current_owner_binding_for_source(
                &snapshot.resolver,
                carrier_id,
            )
            .is_unresolved()
            {
                reconcile_unowned_carrier_provider_file(
                    sync,
                    documents,
                    provider_sync_states,
                    &snapshot,
                    carrier_id,
                    None,
                    "barrel_carrier_dep",
                    carrier_publish.as_ref(),
                    carrier_coordinator,
                )
                .await;
                continue;
            }

            // IDE-sync: gate on the IDE/TSX surface (not the runtime `Main`) so
            // a Main-less carrier (Svelte) is not skipped here.
            let profile = documents.tsx_profile.read().clone();
            if !host
                .ensure_ide_compiled(carrier_id, &profile)
                .unwrap_or(false)
            {
                continue;
            }

            configure_provider_paths_for_source(sync, &snapshot, carrier_id, true).await;

            let ide = host.get_ide(carrier_id, &profile);

            // Route the owner-resolved carrier (or an owner lost mid-flight) through
            // the SINGLE carrier-sync gateway. Any synced kind counts as progress.
            if let CarrierApplyOutcome::Applied { synced, .. } = apply_owner_resolved_carrier_sync(
                Some(sync),
                documents,
                provider_sync_states,
                &snapshot,
                carrier_id,
                ide.as_ref(),
                "barrel_carrier_dep",
                carrier_publish.as_ref(),
                carrier_coordinator,
            )
            .await
            {
                if !synced.is_empty() {
                    synced_any = true;
                }
            }
        }

        // Sync barrel files (their rewritten imports now point to .vue.ts)
        for barrel_id in &barrel_ids {
            if sync_pending_non_carrier_provider_file(
                sync,
                documents,
                &snapshot,
                provider_sync_states,
                barrel_id,
                true,
            )
            .await
            {
                synced_any = true;
            }
        }
    }

    // Pass 3 (TSGO only): proactively open the transitive DECLARATION closure.
    //
    // tsgo resolves a bare framework-carrier import (`import B from "./B.vue"`)
    // to the virtual `B.d.<ext>.ts` declaration via its native basename-append
    // probe — but tsgo has NO module-resolution hook, so every declaration an
    // importing carrier (transitively) needs must already be OPEN as an overlay
    // when that carrier is type-checked, or the import fails with TS2307. This
    // pass walks the transitive closure of carrier dependencies reachable from
    // the OPEN carrier roots and opens each one's `.d.<ext>.ts`, recording the
    // reachability so the `did_close` lifecycle can release them.
    //
    // tsserver serves carrier companions through the publish store (not direct
    // overlay opens), so the proactive overlay graph is a tsgo-only concern —
    // scoped exactly like the carrier-open passes above.
    if is_tsgo {
        synced_any |= decl_overlay_owner
            .open_declaration_closure_for_open_files(
                sync,
                documents,
                provider_sync_states,
                &snapshot,
                pass_generation,
            )
            .await;
    }

    synced_any
}

pub(super) fn owner_path_config_for_source(
    snapshot: &super::PublishedResolverSnapshot,
    canonical_id: &str,
) -> Option<(String, serde_json::Value)> {
    let owner = snapshot.resolver.nearest_config_for_path(canonical_id)?;
    let tsconfig_path = owner.tsconfig_path.as_deref()?;
    let ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    crate::svelte_assets::owner_provider_path_config(&ws, tsconfig_path, &owner.root)
}

pub(crate) async fn configure_provider_paths_for_source(
    sync: &ProjectSync,
    snapshot: &super::PublishedResolverSnapshot,
    canonical_id: &str,
    background: bool,
) {
    let Some((base_url, paths)) = owner_path_config_for_source(snapshot, canonical_id) else {
        return;
    };

    let result = if background {
        sync.configure_paths_background(&base_url, paths).await
    } else {
        sync.configure_paths(&base_url, paths).await
    };

    if let Err(error) = result {
        tracing::warn!(
            "failed to configure provider paths for {canonical_id} (baseUrl={base_url}): {error}"
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn sync_pending_snapshot_provider_file(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> SyncOutcome {
    if carrier_language_for(canonical_id).is_some() {
        sync_pending_carrier_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
            is_tsgo,
            carrier_publish,
            carrier_coordinator,
        )
        .await
    } else {
        let Some(sync) = sync else {
            return SyncOutcome::Nothing;
        };
        // Non-carrier files have a single Shadow kind: synced fully or not at all.
        if sync_pending_non_carrier_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
            is_tsgo,
        )
        .await
        {
            SyncOutcome::FullyReconciled
        } else {
            SyncOutcome::Nothing
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn sync_pending_carrier_provider_file(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> SyncOutcome {
    // Ensure the file and its deps are loaded. The scheduler's extract_deps
    // + auto-ingress handles recursive dependency walking.
    documents.host().ensure_loaded(canonical_id);
    // Hydration may load new dependencies (macro type deps, external templates)
    // that affect the compilation output. Invalidate compile slots so
    // ensure_compiled recompiles, and bump diagnostics_generation so the LSP
    // cache treats the next diagnostic request as a cache miss.
    documents.host.invalidate_compile_slots(canonical_id);
    documents.host.bump_diagnostics_generation(canonical_id);
    let profile = documents.tsx_profile.read().clone();
    // IDE-sync: drive the IDE/TSX surface (not the runtime `Main`) so a
    // Main-less carrier (Svelte) populates its `CachedTsx` before `get_ide`.
    let _ =
        block_in_place_if_available(|| documents.host.ensure_ide_compiled(canonical_id, &profile));
    let ide = block_in_place_if_available(|| documents.host.get_ide(canonical_id, &profile));
    if is_tsgo {
        if let Some(sync) = sync {
            configure_provider_paths_for_source(sync, snapshot, canonical_id, true).await;
        }
    }

    // Route through the SINGLE carrier-sync gateway: tsserver PUBLISHES the carrier
    // companions into the on-disk store the plugin reads (the configured-project
    // membership), tsgo opens the companions directly, and an owner loss RETRACTS the
    // membership + preserves an open document / removes a closed one. The receipt
    // gates every commit (the gap-E bug class).
    let outcome = apply_owner_resolved_carrier_sync(
        sync,
        documents,
        provider_sync_states,
        snapshot,
        canonical_id,
        ide.as_ref(),
        "pending_snapshot",
        carrier_publish,
        carrier_coordinator,
    )
    .await;
    classify_carrier_apply_outcome(outcome)
}

/// Classify a carrier apply result into the drain's dequeue decision (R2-6):
///   * every intended kind synced → `FullyReconciled` (dequeue);
///   * some synced but an intended kind FAILED → `Partial` (retry on a later
///     drain — never permanently suppress the failed kind);
///   * a TERMINAL no-owner decision (`NoProject` / `Ambiguous`) → `Terminal`
///     (dequeue — a terminal ownership state is never retried; the buffer-side
///     preserve-open / remove-closed already ran and any `verter(project)`
///     diagnostic is published separately);
///   * a still-transient `NotReady` bootstrap, a `Pending` advertise miss, or a
///     total sync failure → `Nothing` (keep queued for a later retry).
///
/// This is the SINGLE point that distinguishes the sole retryable ownership state
/// (`NotReady`) from the terminal ones — a terminal carrier must never be retried
/// into a provider on every drain.
fn classify_carrier_apply_outcome(outcome: CarrierApplyOutcome) -> SyncOutcome {
    match outcome {
        CarrierApplyOutcome::Applied { attempted, synced } => {
            if synced.is_empty() {
                SyncOutcome::Nothing
            } else if attempted.iter().all(|kind| synced.contains(kind)) {
                SyncOutcome::FullyReconciled
            } else {
                SyncOutcome::Partial
            }
        }
        // Terminal no-owner (the gateway retracted the membership; the buffer-side
        // preserve-open / remove-closed already ran): settle + dequeue, never retry.
        CarrierApplyOutcome::Unresolved => SyncOutcome::Terminal,
        // Transient bootstrap (`NotReady`) or a `Pending` advertise/compile miss:
        // keep the file queued for a future snapshot/drain that may resolve it.
        CarrierApplyOutcome::NotReady | CarrierApplyOutcome::Pending => SyncOutcome::Nothing,
    }
}

/// Preserve (or create) an OPEN Vue document's unresolved provider state when
/// no project owns it, and keep its IDE TSX live in the provider.
///
/// Editor-liveness invariant: an open Vue document must keep a usable TSX in
/// the type provider even while its owning project is unresolved or ambiguous.
/// This helper:
///   * reuses the existing committed state's IDE path when present, otherwise
///     builds local unresolved `{source}.tsx`/`.jsx` state;
///   * opens (or updates, if already background-loaded) that TSX in the
///     provider so hover/completion keep working;
///   * commits the unresolved state and leaves the file QUEUED for a future
///     snapshot to upgrade once an owner resolves.
///
/// It never closes the open document's existing paths and never removes its
/// state. Returns `false` so the drain keeps the file in the pending set for
/// later owner reconciliation.
pub(super) async fn sync_open_unresolved_carrier_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_jsx: bool,
    ide: Option<&verter_session::IdeResponse>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> bool {
    let provider_surfaces = documents.provider_surfaces();
    // Build the DESIRED Unresolved target through the shared primitive: the
    // owner-independent desired-extension IDE path + the open-vs-update
    // syncability hint (`ide_background_loaded`). The binding is forced
    // `Unresolved` and the owner-derived API path is dropped — converting a
    // prior `Owned` binding (rather than reusing it) is what lets a later
    // snapshot re-bind the file via `needs_owner_reconcile`.
    let previous = provider_sync_states.get(canonical_id).map(|e| e.clone());
    // Converting a previously-committed OWNED carrier to Unresolved is an owner-loss for
    // the admission barrier: advance it so a late owned token — captured before this
    // conversion — can never resurrect the obsolete owner into the now-unstamped slot.
    if previous
        .as_ref()
        .is_some_and(|state| state.commit_stamp.is_some())
    {
        carrier_coordinator.advance_barrier(canonical_id);
    }
    let target = open_unresolved_carrier_state(previous.as_ref(), canonical_id, is_jsx);

    let Some(ide) = ide else {
        // No compiled IDE output this pass (e.g. a transient compile miss): no
        // IDE sync is attempted, so the IDE kind did NOT go live this pass.
        //
        // Route the commit through the SAME per-kind discipline the owner-
        // resolved path uses (`open_unresolved_carrier_commit`): a non-synced IDE
        // kind RETAINS the prior LIVE path (never dropped to a dead/None path
        // while the prior is still open in the provider), the binding is forced
        // `Unresolved`, and the owner-derived API path is dropped+closed.
        //
        // With NO prior state this commits the EMPTY `Unresolved` (ide_path=None,
        // binding=Unresolved, dropped_api=None → no close) — recording the open
        // file's unresolved status (queued for retry), uniform with the two
        // `preserve_open_unresolved_carrier` callers. This commit is unconditional so
        // all three unresolved-preserve entry points share ONE row-1 behavior; an
        // open file's unresolved state is then observable regardless of which
        // path handled it, and `needs_owner_reconcile` picks it up.
        let commit = open_unresolved_carrier_commit(previous.as_ref(), target, false);
        commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
        close_dropped_owner_api_path(
            sync,
            provider_surfaces,
            commit.dropped_api.as_ref(),
            "open_unresolved",
        )
        .await;
        return false;
    };
    let Some(ide_path) = target.ide_path.clone() else {
        return false;
    };

    // Attempt the desired IDE sync: update-in-place when the desired path is
    // already live (a same-extension preserve), else first-open.
    let result = if target.ide_background_loaded {
        sync.sync_tsx(&ide_path, &ide.code).await
    } else {
        sync.open_tsx(&ide_path, &ide.code).await
    };
    let ide_synced = match result {
        Ok(()) => {
            // Record a fresh generation pinning the EXACT IDE bytes just synced
            // (interactive queries capture this surface).
            let provider_code = sync
                .synced_tsx_content(&ide_path)
                .unwrap_or_else(|| std::sync::Arc::clone(&ide.code));
            crate::provider_surface_store::record_carrier_ide_surface(
                provider_surfaces,
                Some(documents),
                documents.host(),
                canonical_id,
                &ide_path,
                provider_code.as_ref(),
                ide.source_map.as_deref(),
            );
            true
        }
        Err(error) => {
            tracing::warn!(
                "pending_snapshot: failed to sync open unresolved Vue IDE path {ide_path}: {error}"
            );
            false
        }
    };

    // Build the committed state + close targets through the shared per-kind
    // discipline: the IDE kind reverts to the prior live path on a failed/absent
    // sync (rows 7 & 9), the owner-derived API is dropped+closed unconditionally,
    // and the orphaned prior IDE path is closed ONLY after a successful flip.
    let commit = open_unresolved_carrier_commit(previous.as_ref(), target, ide_synced);
    commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
    close_dropped_owner_api_path(
        sync,
        provider_surfaces,
        commit.dropped_api.as_ref(),
        "open_unresolved",
    )
    .await;
    if let Some(stale) = commit.stale_ide_after_success.as_ref() {
        close_stale_provider_paths(
            sync,
            provider_surfaces,
            &non_decl_close_targets(std::slice::from_ref(stale)),
            "open_unresolved_ext_flip",
        )
        .await;
    }
    // Stay queued: a future snapshot with a resolved owner upgrades this state.
    false
}

/// Close the owner-derived API path dropped by an open-document owned→unowned
/// conversion (see [`crate::provider_sync::dropped_api_path_on_unowned_conversion`]).
///
/// A no-op when nothing was dropped. The helper only ever yields an
/// [`ProviderPathKind::Api`] target — the open document's IDE TSX is preserved
/// and is never closed here — so this routes through the shared leaf
/// [`close_stale_provider_paths`] dispatch.
async fn close_dropped_owner_api_path(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    dropped_api: Option<&(ProviderPathKind, String)>,
    context: &str,
) {
    if let Some(dropped) = dropped_api {
        close_stale_provider_paths(
            sync,
            provider_surfaces,
            &non_decl_close_targets(std::slice::from_ref(dropped)),
            context,
        )
        .await;
    }
}

/// The applied result of an owner-resolved both-kinds carrier gateway sync.
enum CarrierApplyOutcome {
    /// Committed (tsserver `Published` membership, or tsgo `DirectOpen` buffer sync).
    /// `attempted` = the kinds with a target path this pass; `synced` = those that
    /// actually landed (a tsserver publish marks both store-resident ⇒ attempted ==
    /// synced).
    Applied {
        attempted: Vec<ProviderPathKind>,
        synced: Vec<ProviderPathKind>,
    },
    /// TERMINAL no-owner (`NoProject` / `Ambiguous`): the gateway retracted the
    /// membership and the buffer-side owner-loss handling already ran. Nothing
    /// committed. Never retried — a settled terminal ownership decision.
    Unresolved,
    /// Transient bootstrap (`NotReady`): ownership is not yet authoritative. The
    /// gateway deferred without thrash and the buffer-side preserve-open ran.
    /// The sole RETRYABLE ownership state — keep queued for a later snapshot.
    NotReady,
    /// Nothing advertised this pass (cold defer / transient miss / fail-closed).
    Pending,
}

/// Route an owner-resolved (or owner-lost) `.vue`/`.svelte` carrier through the
/// SINGLE carrier-sync gateway and APPLY the result. Shared by the main drain, the
/// aliased-import resync, and the barrel Vue-dependency pass:
///   * tsserver ⇒ `Published`: the store membership serves both companions; commit
///     the both-resident state with the receipt (no buffer I/O).
///   * tsgo ⇒ `DirectOpen`: per-kind open/sync (open if not background-loaded, else
///     update), revert any failed kind to its prior live path, commit with the
///     receipt, and close only the genuinely-stale paths.
///   * owner-loss ⇒ `Unresolved` (terminal `NoProject` / `Ambiguous`) or
///     `NotReady` (transient bootstrap): the gateway retracted/deferred the
///     membership; the buffer-side preserve-open / remove-closed handling runs
///     here. They differ only in the caller's dequeue decision.
///   * cold/transient advertise miss ⇒ `Pending`.
#[allow(
    clippy::too_many_arguments,
    reason = "carrier sync needs the provider-surface store + documents alongside the sync state"
)]
async fn apply_owner_resolved_carrier_sync(
    sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    snapshot: &super::PublishedResolverSnapshot,
    canonical_id: &str,
    ide: Option<&verter_session::IdeResponse>,
    context: &str,
    carrier_publish: Option<&CarrierPublishCtx<'_>>,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) -> CarrierApplyOutcome {
    let is_jsx = ide.map(|output| output.is_jsx).unwrap_or(false);
    let membership = carrier_publish
        .and_then(|publish| publish.coordinator)
        .map(|coordinator| crate::external_ts::CarrierMembershipCtx {
            coordinator,
            provider_delivery: carrier_publish
                .expect("coordinator came from the publish context")
                .provider_delivery,
        });
    match crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
        host: documents.host(),
        vfs: carrier_publish.map(|publish| publish.vfs.as_ref()),
        ownership_ready: carrier_publish.is_some_and(|publish| publish.ownership_ready),
        resolver: &snapshot.resolver,
        provider_sync_states,
        provider_surfaces: documents.provider_surfaces(),
        documents: Some(documents),
        canonical_id,
        is_jsx,
        ide,
        membership,
        admission: carrier_coordinator,
        reason: crate::external_ts::ReconcileReason::SourceSynced,
    })
    .await
    {
        crate::external_ts::CarrierSyncDecision::Published {
            committed_state,
            receipt,
        } => {
            // The plugin serves both store-resident companions: no buffer I/O.
            let mut kinds: Vec<ProviderPathKind> = Vec::new();
            if committed_state.api_path.is_some() {
                kinds.push(ProviderPathKind::Api);
            }
            if committed_state.ide_path.is_some() {
                kinds.push(ProviderPathKind::Ide);
            }
            if carrier_coordinator.admit_owned(
                provider_sync_states,
                canonical_id,
                committed_state,
                &receipt,
            ) == crate::external_ts::AdmitOutcome::Superseded
            {
                // A newer transaction already committed (or an owner-loss advanced the
                // barrier): nothing synced this pass — keep queued for a fresh transaction.
                return CarrierApplyOutcome::Pending;
            }
            CarrierApplyOutcome::Applied {
                attempted: kinds.clone(),
                synced: kinds,
            }
        }
        crate::external_ts::CarrierSyncDecision::DirectOpen {
            transition,
            pending,
        } => {
            let Some(sync) = sync else {
                tracing::error!(
                    "{context}: direct-open carrier decision has no managed provider sync"
                );
                return CarrierApplyOutcome::Pending;
            };
            let previous_state = provider_sync_states.get(canonical_id).map(|e| e.clone());
            let stale_paths = transition.stale_paths;
            let mut committed_state = transition.next;
            let mut attempted: Vec<ProviderPathKind> = Vec::new();
            let mut synced: Vec<ProviderPathKind> = Vec::new();

            let api = block_in_place_if_available(|| documents.host.get_public_api(canonical_id));
            if let (Some(api), Some(dts_path)) = (api.as_ref(), committed_state.api_path.clone()) {
                attempted.push(ProviderPathKind::Api);
                let result = if committed_state.api_background_loaded {
                    sync.sync_dts(&dts_path, &api.code).await
                } else {
                    sync.open_dts(&dts_path, &api.code).await
                };
                match result {
                    Ok(()) => {
                        committed_state.set_background_loaded(ProviderPathKind::Api, true);
                        synced.push(ProviderPathKind::Api);
                        crate::provider_surface_store::record_carrier_api_surface(
                            documents.provider_surfaces(),
                            Some(documents),
                            documents.host(),
                            canonical_id,
                            &dts_path,
                            &api.code,
                            api.source_map.as_deref(),
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            "{context}: failed to sync provider API path {dts_path}: {error}"
                        );
                    }
                }
            }
            if let (Some(ide), Some(ide_path)) = (ide, committed_state.ide_path.clone()) {
                attempted.push(ProviderPathKind::Ide);
                let result = if committed_state.ide_background_loaded {
                    sync.sync_tsx(&ide_path, &ide.code).await
                } else {
                    sync.open_tsx(&ide_path, &ide.code).await
                };
                match result {
                    Ok(()) => {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                        synced.push(ProviderPathKind::Ide);
                        // Record a fresh generation pinning the EXACT IDE bytes just
                        // synced (interactive queries capture this surface).
                        let provider_code = sync
                            .synced_tsx_content(&ide_path)
                            .unwrap_or_else(|| std::sync::Arc::clone(&ide.code));
                        crate::provider_surface_store::record_carrier_ide_surface(
                            documents.provider_surfaces(),
                            Some(documents),
                            documents.host(),
                            canonical_id,
                            &ide_path,
                            provider_code.as_ref(),
                            ide.source_map.as_deref(),
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            "{context}: failed to sync provider IDE path {ide_path}: {error}"
                        );
                    }
                }
            }

            if !synced.is_empty() {
                revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced);
                let genuinely_stale =
                    genuinely_stale_after_sync(&stale_paths, &committed_state, &synced);
                // A kind opened: NOW mint the receipt (post-open), attesting EXACTLY the
                // kinds that actually opened this pass, and commit through the coordinator.
                let ide_surface = committed_state
                    .ide_path
                    .as_deref()
                    .and_then(|path| sync.synced_tsx_surface(path));
                let receipt = pending.confirm_opened_with_ide_surface(&synced, ide_surface);
                if carrier_coordinator.admit_owned(
                    provider_sync_states,
                    canonical_id,
                    committed_state,
                    &receipt,
                ) == crate::external_ts::AdmitOutcome::Superseded
                {
                    // Superseded mid-flight: treat as no progress (keep queued).
                    return CarrierApplyOutcome::Pending;
                }
                close_stale_provider_paths(
                    sync,
                    documents.provider_surfaces(),
                    &non_decl_close_targets(&genuinely_stale),
                    context,
                )
                .await;
            }
            CarrierApplyOutcome::Applied { attempted, synced }
        }
        crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
            // Settle the non-owned disposition through the coordinator (requeue the
            // transient, advance the owner-loss barrier for the terminal), then run the
            // SAME buffer-side owner-loss handling (preserve an open document's live TSX /
            // remove a closed one) for a settled no-owner class. The dequeue decision is the
            // returned class: `NotReady` transient (keep queued), `Unresolved` terminal
            // (settle + dequeue, never retry). `Pending` commits nothing and keeps queued.
            match carrier_coordinator.settle(not_owned, canonical_id, None) {
                crate::external_ts::SettleClass::NotReady => {
                    if let Some(sync) = sync {
                        reconcile_unowned_carrier_buffer(
                            sync,
                            documents,
                            provider_sync_states,
                            canonical_id,
                            ide,
                            snapshot.ownership_ready,
                            context,
                            carrier_coordinator,
                        )
                        .await;
                    }
                    CarrierApplyOutcome::NotReady
                }
                crate::external_ts::SettleClass::Unresolved => {
                    if let Some(sync) = sync {
                        reconcile_unowned_carrier_buffer(
                            sync,
                            documents,
                            provider_sync_states,
                            canonical_id,
                            ide,
                            snapshot.ownership_ready,
                            context,
                            carrier_coordinator,
                        )
                        .await;
                    }
                    CarrierApplyOutcome::Unresolved
                }
                crate::external_ts::SettleClass::Pending => CarrierApplyOutcome::Pending,
            }
        }
    }
}

/// Background API-only (`.vue.ts`) provider sync for a `.vue` file.
///
/// This is the awaitable body spawned by
/// `VerterLanguageServer::sync_api_to_provider_in_background`. It manages ONLY
/// the API (`Api`) kind and routes through the shared
/// close-after-successful-sync discipline with `synced_kinds = [Api]`:
///   * sync the NEW API path first (open if not yet background-loaded, else
///     update);
///   * on success, [`revert_unsynced_kinds`] reverts every non-API kind
///     (notably the IDE `.tsx`) back to its PRIOR live path — this path NEVER
///     re-syncs, rebinds, or closes the IDE TSX (the IDE kind is owned by the
///     dedicated IDE-sync path);
///   * commit, then close ONLY the genuinely-stale API path
///     ([`genuinely_stale_after_sync`] gates on `synced_kinds`, so a stale IDE
///     path is never closed here);
///   * on API-sync failure, nothing is committed and nothing is closed — the
///     prior state and prior API path are retained intact (no close-before-sync).
#[allow(
    clippy::too_many_arguments,
    reason = "the spawned API-sync task needs the provider-surface store alongside its sync inputs"
)]
pub(super) async fn sync_api_to_provider_background_task(
    sync: ProjectSync,
    snapshot: super::PublishedResolverSnapshot,
    host: Arc<verter_session::VerterHost>,
    vfs: Option<Arc<verter_workspace::FilesystemWorkspace>>,
    provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    provider_surfaces: crate::provider_surface_store::ProviderSurfaceStore,
    canonical_id: String,
    is_jsx: bool,
    is_tsgo: bool,
    carrier_coordinator: Arc<crate::external_ts::CarrierTransactionCoordinator>,
    pending_snapshot_provider_sync: Arc<dashmap::DashSet<String>>,
) {
    if is_tsgo {
        configure_provider_paths_for_source(&sync, &snapshot, &canonical_id, true).await;
    }
    // Route through the SINGLE carrier-sync gateway. This API-only background task
    // is the tsgo path (the tsserver coordinator route returns before spawning it),
    // so the gateway returns `DirectOpen` carrying the transition + a POST-open
    // authorization; the receipt is minted from `pending` only after the API buffer
    // opens (below). No membership context ⇒ no store publish. Ownership resolves from
    // the SAME published `vfs` the scanner reads.
    let (transition, pending) =
        match crate::external_ts::reconcile_carrier_source(crate::external_ts::CarrierSyncRequest {
            host: &host,
            vfs: vfs.as_deref(),
            ownership_ready: snapshot.ownership_ready,
            resolver: &snapshot.resolver,
            provider_sync_states: &provider_sync_states,
            provider_surfaces: &provider_surfaces,
            documents: None,
            canonical_id: &canonical_id,
            is_jsx,
            ide: None,
            membership: None,
            admission: &carrier_coordinator,
            reason: crate::external_ts::ReconcileReason::SourceSynced,
        })
        .await
        {
            crate::external_ts::CarrierSyncDecision::DirectOpen {
                transition,
                pending,
            } => (transition, pending),
            // No owner (a settled non-owned outcome) or nothing to advertise: the dedicated
            // owner-loss / IDE-sync paths own the provider state. Settle the non-owned
            // disposition so the requeue / owner-loss barrier advance is not dropped; this
            // API-only background task does no buffer conversion.
            crate::external_ts::CarrierSyncDecision::NotOwned(not_owned) => {
                let _ = carrier_coordinator.settle(not_owned, &canonical_id, None);
                return;
            }
            // A tsserver `Published` outcome cannot occur here (`membership: None` ⇒ tsgo
            // direct-open only); the store publish is the tsserver path's job.
            crate::external_ts::CarrierSyncDecision::Published { .. } => return,
        };
    // Capture the prior committed state for the per-kind revert. `transition`
    // was prepared by a read-only `prepare_sync_transition`, so the DashMap
    // still holds the previous state until the commit below.
    let previous_state = provider_sync_states
        .get(&canonical_id)
        .map(|entry| entry.clone());
    let Some(dts_path) = transition.next.api_path.clone() else {
        return;
    };
    let Some(api) = block_in_place_if_available(|| host.get_public_api(&canonical_id)) else {
        return;
    };
    let stale_paths = transition.stale_paths;
    let mut committed_state = transition.next;
    let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

    let result = if committed_state.api_background_loaded {
        sync.sync_dts(&dts_path, &api.code).await
    } else {
        sync.open_dts(&dts_path, &api.code).await
    };
    match result {
        Ok(()) => {
            committed_state.set_background_loaded(ProviderPathKind::Api, true);
            synced_kinds.push(ProviderPathKind::Api);
            // Record a fresh generation pinning the synced content + its
            // same-content source map. This spawned task has no
            // `DocumentRegistry`; the carrier source resolves host/VFS-only.
            crate::provider_surface_store::record_carrier_api_surface(
                &provider_surfaces,
                None,
                &host,
                &canonical_id,
                &dts_path,
                &api.code,
                api.source_map.as_deref(),
            );
        }
        Err(error) => {
            tracing::warn!("sync_api(background): failed for {dts_path}: {error}");
        }
    }

    if !synced_kinds.is_empty() {
        // Retain the prior IDE kind (Ide ∉ synced_kinds → reverted to prior),
        // commit, and close ONLY the genuinely-stale API path. The live IDE
        // `.tsx` is never closed here.
        revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
        let genuinely_stale =
            genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
        // The API buffer opened: NOW mint the receipt (post-open), attesting EXACTLY the
        // kinds that actually opened this pass, and commit through the coordinator.
        let receipt = pending.confirm_opened(&synced_kinds);
        // Gate the stale-path close on ADMISSION and never drop the outcome: a `Superseded`
        // commit (a newer transaction reclaimed the source / an owner-loss advanced the
        // barrier) requeues the source and closes NOTHING — the computed stale paths may be
        // the newer transaction's LIVE buffers. Only an admitted commit closes them.
        if carrier_coordinator.admit_owned(
            &provider_sync_states,
            &canonical_id,
            committed_state,
            &receipt,
        ) == crate::external_ts::AdmitOutcome::Superseded
        {
            pending_snapshot_provider_sync.insert(canonical_id.clone());
            return;
        }
        close_stale_provider_paths(
            &sync,
            &provider_surfaces,
            &non_decl_close_targets(&genuinely_stale),
            "sync_api(background)",
        )
        .await;
    }
    // On API-sync failure nothing is committed and nothing is closed: the prior
    // state + prior API path are retained intact, and the pending drops unconfirmed
    // so no receipt is minted.
}

pub(super) async fn sync_pending_non_carrier_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
    let Some(source) = documents.host.get_source(canonical_id) else {
        return false;
    };
    // Framework carriers never sync to the provider as raw scripts.
    let Some(file_language) =
        crate::provider_sync::provider_script_language(&documents.host, canonical_id)
    else {
        return false;
    };
    let module_references = block_in_place_if_available(|| {
        documents
            .host
            .upsert(verter_session::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: source.clone(),
                file_language,
                aliases: Vec::new(),
            })
            .map(|result| result.module_references)
            .unwrap_or_default()
    });
    let reader = LspProjectResolverReader::new(documents);
    let Some(prepared) = prepare_non_carrier_provider_sync(
        Some(snapshot),
        &reader,
        canonical_id,
        &source,
        &module_references,
    ) else {
        return false;
    };
    let Some(next_state) =
        crate::provider_sync::non_carrier_sync_state_for_source(&snapshot.resolver, canonical_id)
    else {
        return false;
    };

    if is_tsgo {
        configure_provider_paths_for_source(sync, snapshot, canonical_id, true).await;
    }
    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    close_stale_provider_paths(
        sync,
        documents.provider_surfaces(),
        &non_decl_close_targets(&transition.stale_paths),
        "pending_snapshot",
    )
    .await;

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
                    .map(|entry| verter_session::DependencyResolution {
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

pub(super) async fn close_stale_provider_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    stale_paths: &[(NonDeclProviderPathKind, String)],
    context: &str,
) {
    for (kind, path) in stale_paths {
        // EVERY closing store-backed surface (IDE / API / Shadow) is no longer
        // the active synced virtual surface — retire its active generation under
        // a fresh close EPOCH (in-flight captures stay valid; the `Closing`
        // state keeps the path failing closed until the provider close is
        // CONFIRMED). Retiring only the API role would leave a closed IDE /
        // Shadow surface `Current` — capturable by an interactive query against
        // a CLOSED provider buffer. Capture the epoch-stamped token so the
        // finalize is scoped to THIS close.
        let close_token = provider_surfaces.forget(path);
        // A declaration overlay (`Decl`) is unrepresentable here — its lifecycle is
        // owned by `DeclOverlayOwner`, never this generic close.
        let result = match kind {
            NonDeclProviderPathKind::Ide => sync.close_tsx(path).await,
            NonDeclProviderPathKind::Api => sync.close_dts(path).await,
            NonDeclProviderPathKind::Shadow => sync.close_file(path).await,
        };
        match result {
            // Only a CONFIRMED close finalizes, and only via THIS close's token —
            // a reopen (or newer close) during the await makes the epoch mismatch
            // and the finalize a no-op (the fresh snapshot survives). An error
            // drops the token, leaving the `Closing` state (fail closed).
            Ok(()) => {
                provider_surfaces.finalize_close(close_token);
            }
            Err(error) => {
                tracing::warn!("{context}: failed to close stale provider path {path}: {error}");
            }
        }
    }
}

async fn remove_provider_sync_state_and_close_paths(
    sync: &ProjectSync,
    provider_surfaces: &crate::provider_surface_store::ProviderSurfaceStore,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    context: &str,
    carrier_coordinator: &crate::external_ts::CarrierTransactionCoordinator,
) {
    // Advance-before-mutate: the coordinator advances the owner-loss barrier BEFORE it
    // vacates the slot when the removed state was a previously-committed carrier, so a late
    // owned token captured before this removal can never resurrect the obsolete owner into
    // the vacated slot.
    if let Some(state) =
        carrier_coordinator.advance_barrier_and_remove(provider_sync_states, canonical_id)
    {
        // The declaration overlay (`Decl`), if any, is NOT closed here: its
        // lifecycle is owned by `DeclOverlayOwner` and released only when no open
        // carrier root still reaches it (via the `did_close` release). A background
        // state removal closes only the non-decl artifacts.
        close_stale_provider_paths(
            sync,
            provider_surfaces,
            &state.active_non_decl_paths(),
            context,
        )
        .await;
    }
}

#[cfg(test)]
#[path = "background_drain_tests.rs"]
mod tests;
