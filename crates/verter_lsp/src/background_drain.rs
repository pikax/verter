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

/// Outcome of a single pending-file provider-sync pass, used by the drain loop
/// to decide whether to DEQUEUE the file or KEEP it for a later retry.
///
/// A pass may sync multiple kinds (IDE `.tsx` + API `.vue.ts`) independently;
/// per-kind partial-failure is real (one kind syncs, another fails and reverts).
/// Removing the file from the pending set whenever *any* kind synced would
/// permanently suppress the failed kind — it would never be retried. The drain
/// therefore dequeues ONLY on [`SyncOutcome::FullyReconciled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SyncOutcome {
    /// Every intended kind synced this pass (or there was nothing to sync and
    /// the committed state is already consistent). Safe to dequeue.
    FullyReconciled,
    /// At least one kind synced but at least one OTHER intended kind FAILED.
    /// Keep the file queued so the failed kind is retried on a later drain.
    Partial,
    /// Nothing synced this pass — a total sync failure, or an intentionally
    /// queued unresolved/owner-None state awaiting a future owner. Keep queued.
    Nothing,
}

pub(super) async fn drain_pending_snapshot_provider_sync(
    project_sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    pending_snapshot_provider_sync: &DashSet<String>,
    is_tsgo: bool,
    mru_canonical_ids: Option<&parking_lot::Mutex<Vec<String>>>,
) {
    let Some(sync) = project_sync else {
        pending_snapshot_provider_sync.clear();
        return;
    };
    let Some(snapshot) = ({
        let ws = vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(super::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            })
        })
    }) else {
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
        let outcome = sync_pending_snapshot_provider_file(
            sync,
            documents,
            &snapshot,
            provider_sync_states,
            &canonical_id,
            is_tsgo,
        )
        .await;

        // Dequeue ONLY when the file is fully reconciled (every intended kind
        // synced) or its source has vanished. A `Partial` outcome (a kind failed
        // and was reverted to its prior live path) MUST stay queued so the
        // failed kind is retried on a later drain — otherwise it is permanently
        // suppressed. `Nothing` (total failure / queued-unresolved) also stays.
        let fully_reconciled = matches!(outcome, SyncOutcome::FullyReconciled);
        if fully_reconciled || documents.host.get_source(&canonical_id).is_none() {
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
pub(super) async fn resync_aliased_imports_for_open_files(
    documents: &DocumentRegistry,
    project_sync: Option<&ProjectSync>,
    vfs_workspace: &parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    is_tsgo: bool,
) -> bool {
    let Some(sync) = project_sync else {
        return false;
    };
    let snapshot = {
        let ws = vfs_workspace.read();
        ws.as_ref().and_then(|ws| {
            let published = ws.load_published()?;
            Some(super::PublishedResolverSnapshot {
                resolver: published.snapshot.resolver.clone(),
                ownership_ready: published.ownership_ready,
            })
        })
    };
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
        if !canonical_id.ends_with(".vue") {
            continue;
        }
        let Some(analysis) = host.get_analysis(&canonical_id) else {
            continue;
        };

        // Static imports (same pipeline as did_open line 6103)
        let ids = collect_imported_vue_priority_ids_from_imports_with_fallback(
            &analysis.imports,
            Some(&canonical_id),
            |parent, specifier| resolve_import_specifier_standalone(host, parent, specifier),
        );

        // Dynamic imports via module_references
        let reader = LspProjectResolverReader::new(documents);
        let dynamic_ids = collect_priority_vue_targets_from_module_references(
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
            reconcile_unowned_vue_provider_file(
                sync,
                documents,
                provider_sync_states,
                import_id,
                None,
                snapshot.ownership_ready,
                "aliased_resync",
            )
            .await;
            continue;
        }

        // Compile to generate public API
        let profile = documents.tsx_profile.read().clone();
        if host.ensure_compiled(import_id, &profile).is_err() {
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

        // Owner is present (re-checked above before compile). Build the owner-
        // aware target with the compiled `is_jsx`.
        let is_jsx = ide.as_ref().map(|output| output.is_jsx).unwrap_or(false);
        let Some(next_state) =
            crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, import_id, is_jsx)
        else {
            // Owner was lost between the pre-check and here (a mid-flight snapshot
            // change). Reconcile the open file's binding rather than stranding it.
            reconcile_unowned_vue_provider_file(
                sync,
                documents,
                provider_sync_states,
                import_id,
                ide.as_ref(),
                snapshot.ownership_ready,
                "aliased_resync",
            )
            .await;
            continue;
        };

        // Owner-resolved: the public API is required to sync the API kind. A miss
        // here is a transient compile state — the `Owned` binding is correct, so
        // skipping the sync this pass leaves no stale binding behind.
        let Some(api) = host.get_public_api(import_id) else {
            continue;
        };
        // Sync NEW paths first, then close stale-after-success (per-kind, skip-
        // active) through the shared discipline.
        if sync_owner_resolved_vue_with_close_after_sync(
            sync,
            provider_sync_states,
            import_id,
            next_state,
            ide.as_ref().map(|output| &*output.code),
            &api.code,
            "aliased_resync",
        )
        .await
        {
            synced_any = true;
        }
    }

    // Pass 2 (TSGO only): Sync barrel imports discovered from template component usages.
    // When a component is imported through a barrel (non-Vue re-export file), the Vue
    // file collection above misses both the barrel and its Vue re-export targets.
    // This pass follows the barrel → Vue re-export chain and syncs both.
    if is_tsgo {
        let mut barrel_ids: Vec<String> = Vec::new();
        let mut barrel_vue_deps: Vec<String> = Vec::new();
        let mut seen_barrels = HashSet::new();
        let mut seen_barrel_vue = HashSet::new();

        for uri_str in documents.open_uris() {
            let Ok(uri) = uri_str.parse::<Uri>() else {
                continue;
            };
            let Some(canonical_id) = documents.get_canonical_id(&uri) else {
                continue;
            };
            if !canonical_id.ends_with(".vue") {
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
                if resolved.ends_with(".vue") {
                    continue; // already handled by Vue sync pass
                }
                if !seen_barrels.insert(resolved.clone()) {
                    continue;
                }

                // Load the barrel into the host and scan its module references
                // for .vue import specifiers. This avoids the chicken-and-egg problem
                // where get_export_span_follow_reexports needs Vue files already loaded.
                host.ensure_loaded(&resolved);

                if let Some(barrel_analysis) = host.get_analysis(&resolved) {
                    for module_ref in barrel_analysis.module_references.iter() {
                        if let Some(specifier) = &module_ref.literal_specifier {
                            if specifier.ends_with(".vue") {
                                if let Some(vue_id) =
                                    resolve_import_specifier_standalone(host, &resolved, specifier)
                                {
                                    if vue_id.ends_with(".vue")
                                        && seen_barrel_vue.insert(vue_id.clone())
                                    {
                                        barrel_vue_deps.push(vue_id);
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
        for vue_id in &barrel_vue_deps {
            // Skip if already synced in the main Vue pass — but only when an OPEN
            // barrel-dep `.vue`'s committed binding STILL matches the live
            // resolution (R2-4). An owner change/loss on an open barrel dep must
            // fall through to reconciliation, never short-circuit on a stale
            // binding. A non-open dep keeps its fully-loaded binding as-is.
            if let Some(state) = provider_sync_states.get(vue_id.as_str()) {
                if state.ide_background_loaded && state.api_background_loaded {
                    let is_open = documents.canonical_id_to_uri(vue_id).is_some();
                    let binding_current = !is_open
                        || crate::provider_sync::committed_binding_matches_current(
                            &state,
                            &crate::provider_sync::current_owner_binding_for_source(
                                &snapshot.resolver,
                                vue_id,
                            ),
                        );
                    if binding_current {
                        continue;
                    }
                }
            }

            if !host.ensure_loaded(vue_id) {
                continue;
            }

            // R5-2: detect owner-None / owner-loss and reconcile the open file's
            // BINDING BEFORE the compile gate below (mirrors the aliased pass).
            // Owner resolution is a pure resolver query, so a COMPILE FAILURE
            // must not strand a previously-`Owned` open barrel-dep on its dead
            // owner. The reconcile corrects the binding without fresh IDE output.
            if crate::provider_sync::current_owner_binding_for_source(&snapshot.resolver, vue_id)
                .is_unresolved()
            {
                reconcile_unowned_vue_provider_file(
                    sync,
                    documents,
                    provider_sync_states,
                    vue_id,
                    None,
                    snapshot.ownership_ready,
                    "barrel_vue_dep",
                )
                .await;
                continue;
            }

            let profile = documents.tsx_profile.read().clone();
            if host.ensure_compiled(vue_id, &profile).is_err() {
                continue;
            }

            configure_provider_paths_for_source(sync, &snapshot, vue_id, true).await;

            let ide = host.get_ide(vue_id, &profile);

            // Owner is present (re-checked above before compile). Build the
            // owner-aware target with the compiled `is_jsx`.
            let is_jsx = ide.as_ref().map(|output| output.is_jsx).unwrap_or(false);
            let Some(next_state) =
                crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, vue_id, is_jsx)
            else {
                // Owner lost mid-flight (snapshot changed since the pre-check):
                // reconcile the open file's binding rather than strand it.
                reconcile_unowned_vue_provider_file(
                    sync,
                    documents,
                    provider_sync_states,
                    vue_id,
                    ide.as_ref(),
                    snapshot.ownership_ready,
                    "barrel_vue_dep",
                )
                .await;
                continue;
            };

            // Owner-resolved: the public API is required to sync the API kind; a
            // miss is transient and leaves the correct `Owned` binding in place.
            let Some(api) = host.get_public_api(vue_id) else {
                continue;
            };
            if sync_owner_resolved_vue_with_close_after_sync(
                sync,
                provider_sync_states,
                vue_id,
                next_state,
                ide.as_ref().map(|output| &*output.code),
                &api.code,
                "barrel_vue_dep",
            )
            .await
            {
                synced_any = true;
            }
        }

        // Sync barrel files (their rewritten imports now point to .vue.ts)
        for barrel_id in &barrel_ids {
            if sync_pending_non_vue_provider_file(
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

    synced_any
}

pub(super) fn owner_path_config_for_source(
    snapshot: &super::PublishedResolverSnapshot,
    canonical_id: &str,
) -> Option<(String, serde_json::Value)> {
    let owner = snapshot.resolver.owner_for_file(canonical_id)?;
    let tsconfig_path = owner.tsconfig_path.as_deref()?;
    let ws =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    verter_workspace::config::raw_paths_json(&ws, tsconfig_path)
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

    // Re-inject the Svelte IDE-projection assets (D-av / D-ay) on EVERY
    // provider path-config — a subsequent owned sync sends the supplied `paths`
    // verbatim, so without re-injection it would OVERWRITE the startup-injected
    // `@verter/svelte-jsx` + `svelte/*` rows and strand `.svelte.tsx` module
    // resolution. The owner project root is the per-project resolution anchor.
    let owner_root = snapshot
        .resolver
        .owner_for_file(canonical_id)
        .map(|o| o.root.clone())
        .unwrap_or_default();
    let paths = crate::svelte_assets::inject_svelte_paths(paths, &owner_root);

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

pub(super) async fn sync_pending_snapshot_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> SyncOutcome {
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
        // Non-Vue files have a single Shadow kind: synced fully or not at all.
        if sync_pending_non_vue_provider_file(
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

pub(super) async fn sync_pending_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &super::PublishedResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
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
    let _ = block_in_place_if_available(|| documents.host.ensure_compiled(canonical_id, &profile));
    let ide = block_in_place_if_available(|| documents.host.get_ide(canonical_id, &profile));
    let is_jsx = ide.as_ref().map(|output| output.is_jsx).unwrap_or(false);
    let is_open = documents.canonical_id_to_uri(canonical_id).is_some();
    let Some(next_state) =
        crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, canonical_id, is_jsx)
    else {
        // No owner resolved for this file.
        if is_open {
            // Editor-liveness invariant: an OPEN Vue document keeps a live TSX
            // in the provider even when ownership is unresolved/ambiguous. The
            // drain preserves (or creates) unresolved open-document state and
            // syncs its IDE TSX, then keeps the file queued so a future
            // snapshot with a resolved owner can upgrade it. It must NEVER
            // remove the state or close the TSX merely because owner is None.
            // It is intentionally NOT fully reconciled — it awaits an owner —
            // so it stays queued (`Nothing`).
            sync_open_unresolved_vue_provider_file(
                sync,
                provider_sync_states,
                canonical_id,
                is_jsx,
                ide.as_ref(),
            )
            .await;
            return SyncOutcome::Nothing;
        }
        // Closed or deleted file with no owner: no open-editor invariant to
        // preserve — drop any stale provider state and close its paths. There is
        // nothing left to reconcile for this file.
        if snapshot.ownership_ready {
            remove_provider_sync_state_and_close_paths(
                sync,
                provider_sync_states,
                canonical_id,
                "pending_snapshot",
            )
            .await;
        }
        return SyncOutcome::Nothing;
    };
    if is_tsgo {
        configure_provider_paths_for_source(sync, snapshot, canonical_id, true).await;
    }

    let previous_state = provider_sync_states.get(canonical_id).map(|e| e.clone());
    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    // FINAL DESIGN 7: close-AFTER-sync. Capture the stale paths now but do NOT
    // close them yet — a failed replacement sync must leave the previous open
    // path alive. Stale paths are closed only after the new paths sync
    // successfully and the new state is committed (see below).
    let stale_paths = transition.stale_paths;

    let mut committed_state = transition.next;
    // Track which KINDS actually synced this pass: a transition is closed/
    // committed per-kind, not all-or-nothing. A kind whose replacement sync
    // FAILS must not advance its committed path (it reverts to the previous
    // live path) and must not have its stale path closed.
    let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();
    // Track which KINDS were INTENDED this pass (their source artifact exists +
    // a target path is present). A pass is only `FullyReconciled` when every
    // intended kind synced; a kind that was intended but failed forces a
    // `Partial` outcome so the drain retries it (R2-6).
    let mut attempted_kinds: Vec<ProviderPathKind> = Vec::new();

    if let Some(api) = block_in_place_if_available(|| documents.host.get_public_api(canonical_id)) {
        let Some(dts_path) = committed_state.api_path.clone() else {
            return SyncOutcome::Nothing;
        };
        attempted_kinds.push(ProviderPathKind::Api);
        let result = if is_tsgo {
            if committed_state.api_background_loaded {
                sync.sync_dts(&dts_path, &api.code).await
            } else {
                sync.open_dts(&dts_path, &api.code).await
            }
        } else if is_open || committed_state.api_background_loaded {
            sync.sync_dts(&dts_path, &api.code).await
        } else {
            sync.load_dts(&dts_path, &api.code).await
        };

        match result {
            Ok(()) => {
                if !is_tsgo || !is_open {
                    committed_state.set_background_loaded(ProviderPathKind::Api, true);
                }
                synced_kinds.push(ProviderPathKind::Api);
            }
            Err(error) => {
                tracing::warn!(
                    "pending_snapshot: failed to sync provider API path {dts_path}: {error}"
                );
            }
        }
    }

    if let Some(ide) = ide {
        let Some(ide_path) = committed_state.ide_path.clone() else {
            return SyncOutcome::Nothing;
        };
        attempted_kinds.push(ProviderPathKind::Ide);
        let result = if is_tsgo {
            if committed_state.ide_background_loaded {
                sync.sync_tsx(&ide_path, &ide.code).await
            } else {
                sync.open_tsx(&ide_path, &ide.code).await
            }
        } else if is_open || committed_state.ide_background_loaded {
            sync.sync_tsx(&ide_path, &ide.code).await
        } else {
            sync.load_tsx(&ide_path, &ide.code).await
        };

        match result {
            Ok(()) => {
                if is_tsgo || !is_open {
                    committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                }
                synced_kinds.push(ProviderPathKind::Ide);
            }
            Err(error) => {
                tracing::warn!(
                    "pending_snapshot: failed to sync provider IDE path {ide_path}: {error}"
                );
            }
        }
    }

    if !synced_kinds.is_empty() {
        // Per-kind partial-failure gate: any kind whose replacement did NOT
        // sync reverts to its previous live path so the committed state never
        // advertises an unsynced path. Then publish, and close only the
        // genuinely-stale paths — those whose kind synced AND that the new
        // committed state no longer uses (a same-path rebind of an
        // owner-independent Vue artifact must never be closed).
        revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
        let genuinely_stale =
            genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
        commit_sync_transition(provider_sync_states, canonical_id, committed_state);
        close_stale_provider_paths(sync, &genuinely_stale, "pending_snapshot").await;
    }
    // Classify the outcome for the drain's dequeue decision (R2-6):
    //   * every intended kind synced → FullyReconciled (dequeue);
    //   * some synced but an intended kind FAILED → Partial (retry the failed
    //     kind on a later drain — it must NOT be permanently suppressed);
    //   * nothing synced → Nothing (total failure; prior state retained intact).
    let all_synced = attempted_kinds
        .iter()
        .all(|kind| synced_kinds.contains(kind));
    if synced_kinds.is_empty() {
        SyncOutcome::Nothing
    } else if all_synced {
        SyncOutcome::FullyReconciled
    } else {
        SyncOutcome::Partial
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
pub(super) async fn sync_open_unresolved_vue_provider_file(
    sync: &ProjectSync,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_jsx: bool,
    ide: Option<&verter_session::IdeResponse>,
) -> bool {
    // Build the DESIRED Unresolved target through the shared primitive: the
    // owner-independent desired-extension IDE path + the open-vs-update
    // syncability hint (`ide_background_loaded`). The binding is forced
    // `Unresolved` and the owner-derived API path is dropped — converting a
    // prior `Owned` binding (rather than reusing it) is what lets a later
    // snapshot re-bind the file via `needs_owner_reconcile`.
    let previous = provider_sync_states.get(canonical_id).map(|e| e.clone());
    let target = open_unresolved_vue_state(previous.as_ref(), canonical_id, is_jsx);

    let Some(ide) = ide else {
        // No compiled IDE output this pass (e.g. a transient compile miss): no
        // IDE sync is attempted, so the IDE kind did NOT go live this pass.
        //
        // Route the commit through the SAME per-kind discipline the owner-
        // resolved path uses (`open_unresolved_vue_commit`): a non-synced IDE
        // kind RETAINS the prior LIVE path (never dropped to a dead/None path
        // while the prior is still open in the provider), the binding is forced
        // `Unresolved`, and the owner-derived API path is dropped+closed.
        //
        // With NO prior state this commits the EMPTY `Unresolved` (ide_path=None,
        // binding=Unresolved, dropped_api=None → no close) — recording the open
        // file's unresolved status (queued for retry), uniform with the two
        // `preserve_open_unresolved_vue` callers. This commit is unconditional so
        // all three unresolved-preserve entry points share ONE row-1 behavior; an
        // open file's unresolved state is then observable regardless of which
        // path handled it, and `needs_owner_reconcile` picks it up.
        let commit = open_unresolved_vue_commit(previous.as_ref(), target, false);
        commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
        close_dropped_owner_api_path(sync, commit.dropped_api.as_ref(), "open_unresolved").await;
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
        Ok(()) => true,
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
    let commit = open_unresolved_vue_commit(previous.as_ref(), target, ide_synced);
    commit_sync_transition(provider_sync_states, canonical_id, commit.committed);
    close_dropped_owner_api_path(sync, commit.dropped_api.as_ref(), "open_unresolved").await;
    if let Some(stale) = commit.stale_ide_after_success.as_ref() {
        close_stale_provider_paths(
            sync,
            std::slice::from_ref(stale),
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
    dropped_api: Option<&(ProviderPathKind, String)>,
    context: &str,
) {
    if let Some(dropped) = dropped_api {
        close_stale_provider_paths(sync, std::slice::from_ref(dropped), context).await;
    }
}

/// Owner-None reconciliation for a `.vue` file reached during a background sync
/// pass (aliased-import resync, barrel Vue dependency). Single shared discipline
/// for the editor-liveness invariant:
///   * OPEN file → preserve/create `Unresolved` state and keep its IDE TSX live
///     (delegates to [`sync_open_unresolved_vue_provider_file`]). NEVER removes
///     the state or closes the TSX merely because the owner is None.
///   * closed/deleted file (only once `ownership_ready`) → drop the stale state
///     and close its provider paths.
async fn reconcile_unowned_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    ide: Option<&verter_session::IdeResponse>,
    ownership_ready: bool,
    context: &str,
) {
    if documents.canonical_id_to_uri(canonical_id).is_some() {
        // Open document: keep its TSX live as unresolved open-document state.
        // `is_jsx` is derived from the compiled IDE output (false when absent —
        // a transient compile miss preserves the prior path regardless).
        let is_jsx = ide.map(|output| output.is_jsx).unwrap_or(false);
        sync_open_unresolved_vue_provider_file(
            sync,
            provider_sync_states,
            canonical_id,
            is_jsx,
            ide,
        )
        .await;
        return;
    }
    if ownership_ready {
        remove_provider_sync_state_and_close_paths(
            sync,
            provider_sync_states,
            canonical_id,
            context,
        )
        .await;
    }
}

/// Sync an owner-resolved `.vue` file's IDE/API paths into the provider with the
/// close-AFTER-successful-sync discipline (shared by the aliased-import resync
/// and barrel Vue dependency passes).
///
/// Captures the transition's stale paths, syncs each kind (open if not yet
/// background-loaded, else update), then — per-kind partial-failure gated —
/// reverts any kind whose replacement failed to its previous live path,
/// commits, and closes ONLY the genuinely-stale paths (synced kind, not active).
/// A failed reconciliation leaves the previous path both committed AND open.
/// Returns `true` if any kind synced.
async fn sync_owner_resolved_vue_with_close_after_sync(
    sync: &ProjectSync,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    next_state: ProviderSyncState,
    ide_code: Option<&str>,
    api_code: &str,
    context: &str,
) -> bool {
    let previous_state = provider_sync_states.get(canonical_id).map(|e| e.clone());
    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    let stale_paths = transition.stale_paths;
    let mut committed_state = transition.next;
    let mut synced_kinds: Vec<ProviderPathKind> = Vec::new();

    if let Some(ide_code) = ide_code {
        if let Some(ide_path) = committed_state.ide_path.clone() {
            let result = if committed_state.ide_background_loaded {
                sync.sync_tsx(&ide_path, ide_code).await
            } else {
                sync.open_tsx(&ide_path, ide_code).await
            };
            match result {
                Ok(()) => {
                    committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                    synced_kinds.push(ProviderPathKind::Ide);
                }
                Err(error) => {
                    tracing::warn!(
                        "{context}: failed to sync provider IDE path {ide_path}: {error}"
                    );
                }
            }
        }
    }
    if let Some(dts_path) = committed_state.api_path.clone() {
        let result = if committed_state.api_background_loaded {
            sync.sync_dts(&dts_path, api_code).await
        } else {
            sync.open_dts(&dts_path, api_code).await
        };
        match result {
            Ok(()) => {
                committed_state.set_background_loaded(ProviderPathKind::Api, true);
                synced_kinds.push(ProviderPathKind::Api);
            }
            Err(error) => {
                tracing::warn!("{context}: failed to sync provider API path {dts_path}: {error}");
            }
        }
    }

    if !synced_kinds.is_empty() {
        revert_unsynced_kinds(&mut committed_state, previous_state.as_ref(), &synced_kinds);
        let genuinely_stale =
            genuinely_stale_after_sync(&stale_paths, &committed_state, &synced_kinds);
        commit_sync_transition(provider_sync_states, canonical_id, committed_state);
        close_stale_provider_paths(sync, &genuinely_stale, context).await;
    }
    // On total failure nothing is committed and nothing is closed: the previous
    // state + provider paths are retained intact.
    !synced_kinds.is_empty()
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
pub(super) async fn sync_api_to_provider_background_task(
    sync: ProjectSync,
    snapshot: super::PublishedResolverSnapshot,
    host: Arc<verter_session::VerterHost>,
    provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    canonical_id: String,
    transition: crate::provider_sync::ProviderSyncTransition,
    is_tsgo: bool,
) {
    // Capture the prior committed state for the per-kind revert. `transition`
    // was prepared by a read-only `prepare_sync_transition`, so the DashMap
    // still holds the previous state until the commit below.
    let previous_state = provider_sync_states
        .get(&canonical_id)
        .map(|entry| entry.clone());
    if is_tsgo {
        configure_provider_paths_for_source(&sync, &snapshot, &canonical_id, true).await;
    }
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
        commit_sync_transition(&provider_sync_states, &canonical_id, committed_state);
        close_stale_provider_paths(&sync, &genuinely_stale, "sync_api(background)").await;
    }
    // On API-sync failure nothing is committed and nothing is closed: the prior
    // state + prior API path are retained intact.
}

pub(super) async fn sync_pending_non_vue_provider_file(
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

    if is_tsgo {
        configure_provider_paths_for_source(sync, snapshot, canonical_id, true).await;
    }
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

async fn remove_provider_sync_state_and_close_paths(
    sync: &ProjectSync,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    context: &str,
) {
    if let Some(state) = crate::provider_sync::remove_sync_state(provider_sync_states, canonical_id)
    {
        close_stale_provider_paths(sync, &state.active_paths(), context).await;
    }
}
