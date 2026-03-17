use super::*;

// ── Background initialization ───────────────────────────────────────────

/// Spawn the heartbeat task. Sends `$/verter/heartbeat` every 5 seconds.
/// Called first in `initialized()` so the extension always sees heartbeats,
/// even during long background initialization.
pub(super) fn spawn_heartbeat(client: Client) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let active = ACTIVE_HANDLERS.load(std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                "heartbeat TICK ts={ts} active_handlers={active} thread={:?}",
                std::thread::current().id()
            );
            client
                .send_notification::<Heartbeat>(HeartbeatParams { timestamp: ts })
                .await;
            tracing::info!("heartbeat SENT ts={ts}");
        }
    });
}

/// Arguments for the background initialization task.
/// All fields are owned or Arc-wrapped so the task can run independently.
pub(super) struct BackgroundInitArgs {
    pub(super) roots: Vec<String>,
    pub(super) vite_opts: crate::vite_config::ViteConfigOptions,
    pub(super) init_lint_opts: Option<serde_json::Value>,
    pub(super) my_gen: u64,
    pub(super) client: Client,
    pub(super) type_provider: Option<Arc<dyn TypeProvider>>,
    pub(super) project_registry: Arc<parking_lot::RwLock<Option<crate::config::ProjectRegistry>>>,
    pub(super) resolver_snapshot: Arc<parking_lot::RwLock<Option<ResolverSnapshot>>>,
    pub(super) fallback_linter: Arc<parking_lot::RwLock<verter_diagnostics::Linter>>,
    pub(super) workspace_scanner:
        Arc<tokio::sync::Mutex<Option<crate::workspace_scanner::WorkspaceScannerHandle>>>,
    pub(super) init_generation: Arc<std::sync::atomic::AtomicU64>,
    pub(super) project_sync: Option<ProjectSync>,
    pub(super) documents: Arc<DocumentRegistry>,
    pub(super) provider_sync_states: Arc<DashMap<String, ProviderSyncState>>,
    pub(super) pending_snapshot_provider_sync: Arc<DashSet<String>>,
    pub(super) is_tsgo: bool,
    pub(super) cached_verter_diags: Arc<DashMap<String, CachedVerterDiagEntry>>,
    pub(super) position_encoding: Arc<parking_lot::RwLock<PositionEncodingKind>>,
    /// Snapshot of MRU list at init time for drain ordering.
    pub(super) mru_canonical_ids: Arc<parking_lot::Mutex<Vec<String>>>,
    /// VFS workspace handle — populated during background_init with a FilesystemWorkspace.
    pub(super) vfs_workspace:
        Arc<parking_lot::RwLock<Option<Arc<verter_vfs::FilesystemWorkspace>>>>,
}

/// Run all blocking initialization work in the background.
///
/// This function is spawned from `initialized()` and performs:
/// 1. Project registry build (blocking: vite config eval, tsconfig discovery)
/// 2. Type provider workspace sync (async)
/// 3. Lint option merging
/// 4. @verter/types materialisation (blocking FS)
/// 5. Workspace scanner spawn
///
/// Generation checks before each irreversible commit ensure stale init tasks
/// (superseded by `did_change_workspace_folders`) are discarded.
pub(super) async fn background_init(args: BackgroundInitArgs) -> Result<()> {
    let BackgroundInitArgs {
        roots,
        vite_opts,
        init_lint_opts,
        my_gen,
        client,
        type_provider,
        project_registry,
        resolver_snapshot,
        fallback_linter,
        workspace_scanner,
        init_generation,
        project_sync,
        documents,
        provider_sync_states,
        pending_snapshot_provider_sync,
        is_tsgo,
        cached_verter_diags,
        position_encoding,
        mru_canonical_ids,
        vfs_workspace,
    } = args;

    let host = documents.host_arc();
    let tsx_profile = Arc::clone(&documents.tsx_profile);

    // 1. Build project registry (spawn_blocking — blocking I/O: vite eval, tsconfig)
    let roots_for_registry = roots.clone();
    let vite_opts_for_registry = vite_opts.clone();
    let registry_result = tokio::task::spawn_blocking(move || {
        crate::config::ProjectRegistry::from_workspace_roots(
            &roots_for_registry,
            &vite_opts_for_registry,
        )
    })
    .await;

    let build_result = match registry_result {
        Ok(r) => r,
        Err(e) => {
            if e.is_panic() {
                tracing::error!("project registry build panicked: {e}");
                client
                    .show_message(
                        MessageType::WARNING,
                        "Verter: initialization failed (panic in config discovery)",
                    )
                    .await;
            }
            return Err(tower_lsp_server::jsonrpc::Error::internal_error());
        }
    };

    let mut registry = build_result.registry;
    let trust_required = build_result.trust_required;

    // Log discovered projects
    for project in registry.projects() {
        tracing::info!(
            "project config: root={}, tsconfig={:?}, workspace_aliases={}, lint_explicit={}",
            project.root,
            project.tsconfig_path,
            project.workspace_aliases.len(),
            project.lint_explicitly_configured,
        );
    }

    // 2. Type provider: workspace folder sync + path config (async, non-blocking)
    if let Some(tp) = &type_provider {
        let added: Vec<serde_json::Value> = roots
            .iter()
            .map(|uri| {
                serde_json::json!({
                    "uri": uri,
                    "name": uri.rsplit('/').next().unwrap_or(uri)
                })
            })
            .collect();
        let _ = tp.update_workspace_folders(added, vec![]).await;

        for project in registry.projects() {
            let Some(tsconfig_path) = project.tsconfig_path.as_deref() else {
                continue;
            };
            let tsconfig_path = std::path::PathBuf::from(tsconfig_path);
            if let Some((base_url, paths)) = verter_vfs::config::raw_paths_json(&tsconfig_path) {
                tracing::info!(
                    "configuring tsserver paths for {} via {} (baseUrl: {})",
                    project.root,
                    tsconfig_path.display(),
                    base_url,
                );
                if let Err(e) = tp.configure_paths(&base_url, paths).await {
                    tracing::warn!("failed to configure tsserver paths: {e}");
                }
            }
        }

        // Layer 2: Re-open all files with correct projectRootPath now that
        // workspace folders and tsconfig paths are configured.
        let _ = tp.resync_open_files().await;
    }

    // 3. Merge lint options
    if let Some(init_opts) = init_lint_opts {
        let mut resolved = crate::config::ResolvedLintConfig::default();
        crate::config::merge_init_options(&mut resolved, &init_opts);
        if resolved.explicitly_configured {
            *fallback_linter.write() = verter_diagnostics::Linter::new(resolved.config.clone());
            registry.apply_default_lint(&resolved.config);
        }
    }

    // 3b. Propagate conditional_root_narrowing to lint configs
    if tsx_profile.read().conditional_root_narrowing {
        registry.set_conditional_root_narrowing(true);
        fallback_linter
            .write()
            .config_mut()
            .conditional_root_narrowing = true;
    }

    // 4. Generation check → commit registry
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded, discarding registry");
        return Ok(());
    }
    let resolver = registry.to_native_project_resolver();
    *resolver_snapshot.write() = Some(ResolverSnapshot {
        generation: my_gen,
        resolver,
    });

    // Set the host's internal resolver for compilation without syncing to
    // the workspace — the workspace resolver comes from set_project_graph().
    host.set_internal_resolver(
        registry
            .projects()
            .iter()
            .map(|p| p.to_ide_project_config())
            .collect(),
    );

    // Update the existing VFS workspace's project graph. The workspace was
    // created in initialize() with an empty graph; now populate it with the
    // discovered tsconfig/vite configuration so alias resolution works.
    {
        let vfs_vite_opts = verter_vfs::ViteConfigOptions {
            enabled: vite_opts.enabled,
            trusted_files: vite_opts.trusted_files.clone(),
            node_path: vite_opts.node_path.clone(),
        };
        let vfs_build = verter_vfs::ProjectGraph::from_workspace_roots(&roots, &vfs_vite_opts);
        let ws = vfs_workspace
            .read()
            .clone()
            .expect("workspace created in initialize()");
        ws.set_project_graph(vfs_build.graph);
        tracing::info!("VFS project graph updated with {} roots", roots.len());
    }

    *project_registry.write() = Some(registry);

    drain_pending_snapshot_provider_sync(
        project_sync.as_ref(),
        &documents,
        &resolver_snapshot,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        is_tsgo,
        Some(&mru_canonical_ids),
    )
    .await;

    // 4b. Re-resolve aliased imports for open files now that project_registry is built.
    // During did_open, aliased imports (e.g., @/components/MyComp.vue) could not
    // be resolved because project_registry was None. Now that it's available,
    // re-run the import collection pipeline and sync any missing .vue.ts files.
    let aliased_imports_synced = resync_aliased_imports_for_open_files(
        &documents,
        project_sync.as_ref(),
        &resolver_snapshot,
        &provider_sync_states,
        is_tsgo,
    )
    .await;

    // If new imported .vue.ts files were synced, re-open the active files in tsserver
    // so it picks up the newly available modules and clears stale TS2307 diagnostics.
    if aliased_imports_synced {
        if let Some(tp) = &type_provider {
            let _ = tp.resync_open_files().await;
        }
    }

    // 5. Materialize @verter/types (spawn_blocking — blocking FS)
    let roots_for_types = roots.clone();
    let materialize_types =
        tokio::task::spawn_blocking(move || materialize_verter_types(&roots_for_types))
            .await
            .unwrap_or(MaterializeVerterTypesResult {
                any_failed: true,
                wrote_any: false,
            });
    if materialize_types.any_failed {
        tsx_profile.write().embed_ambient_types = true;
    }

    // 6. Generation check → spawn workspace scanner
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded before scanner, discarding");
        return Ok(());
    }

    let roots_for_scan = roots.clone();
    let tsconfig_patterns =
        tokio::task::spawn_blocking(move || collect_tsconfig_patterns(&roots_for_scan))
            .await
            .unwrap_or_default();

    let root_paths: Vec<std::path::PathBuf> = roots
        .iter()
        .map(|uri| std::path::PathBuf::from(crate::documents::uri_to_canonical_id_from_str(uri)))
        .collect();

    let (scanner_done_tx, scanner_done_rx) = tokio::sync::oneshot::channel::<()>();

    let scanner = crate::workspace_scanner::spawn_workspace_scanner(
        crate::workspace_scanner::WorkspaceScannerConfig {
            root_paths,
            host: Arc::clone(&host),
            project_sync: project_sync.clone(),
            resolver_snapshot: Arc::clone(&resolver_snapshot),
            provider_sync_states: Arc::clone(&provider_sync_states),
            is_tsgo,
            tsx_profile: tsx_profile.read().clone(),
            tsconfig_patterns,
            done_tx: Some(scanner_done_tx),
        },
    );

    {
        let mut guard = workspace_scanner.lock().await;
        if let Some(old) = guard.take() {
            old.stop();
        }
        *guard = Some(scanner);
    }

    // Spawn waiter task: after the scanner completes, publish fresh diagnostics
    // for all open files and send $/verter/typeProviderSyncComplete.
    {
        let client = client.clone();
        let documents = documents.clone();
        let cached_verter_diags = Arc::clone(&cached_verter_diags);
        let project_registry = Arc::clone(&project_registry);
        let fallback_linter = Arc::clone(&fallback_linter);
        let type_provider = type_provider.clone();
        let tsx_profile = tsx_profile.clone();
        let resolver_snapshot = Arc::clone(&resolver_snapshot);
        let position_encoding = position_encoding.clone();
        let init_generation = Arc::clone(&init_generation);
        tokio::spawn(async move {
            if scanner_done_rx.await.is_err() {
                return; // Scanner was dropped/cancelled
            }

            // Check generation — bail if superseded
            if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
                tracing::info!(
                    "init gen={my_gen} superseded before typeProviderSyncComplete, discarding"
                );
                return;
            }

            tracing::info!(
                "workspace scanner complete (gen={my_gen}), publishing post-scan diagnostics"
            );

            // Publish fresh diagnostics for all open files
            let open_uris = documents.open_uris();
            for uri_str in &open_uris {
                let uri: Uri = match uri_str.parse() {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                let verter_diags = compute_verter_diagnostics_for(
                    &documents,
                    &uri,
                    &cached_verter_diags,
                    &project_registry,
                    &fallback_linter,
                );

                let diagnostics = if let Some(tp) = &type_provider {
                    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
                    let profile = tsx_profile.read().clone();
                    let ide = documents.host.get_ide(&canonical_id, &profile);

                    if let Some(ide) = ide {
                        let snapshot = resolver_snapshot.read().clone();
                        let Some(tsx_path) = snapshot.as_ref().and_then(|snapshot| {
                            provider_ide_path_for_source(
                                &snapshot.resolver,
                                &canonical_id,
                                ide.is_jsx,
                            )
                        }) else {
                            continue;
                        };
                        let encoding = position_encoding.read().clone();
                        let tsx_li = crate::documents::line_index::LineIndex::new(
                            &ide.code,
                            encoding.clone(),
                        );
                        let mapper = ide
                            .source_map
                            .as_ref()
                            .and_then(|sm| PositionMapper::from_json(sm).ok());
                        let vue_source = documents.host.get_source(&canonical_id);

                        match (tp.get_diagnostics(&tsx_path).await, mapper, vue_source) {
                            (Ok(type_diags), Some(mapper), Some(vue_src)) => {
                                let vue_li = crate::documents::line_index::LineIndex::new(
                                    &vue_src, encoding,
                                );
                                crate::tsgo::merge::merge_diagnostics(
                                    verter_diags,
                                    type_diags,
                                    &tsx_li,
                                    &mapper,
                                    &vue_li,
                                )
                            }
                            _ => verter_diags,
                        }
                    } else {
                        verter_diags
                    }
                } else {
                    verter_diags
                };

                client.publish_diagnostics(uri, diagnostics, None).await;
            }

            client
                .send_notification::<TypeProviderSyncComplete>(TypeProviderSyncCompleteParams {
                    gen: my_gen,
                })
                .await;

            tracing::info!("typeProviderSyncComplete sent (gen={my_gen})");
        });
    }

    // 7. Generation check → notify ready
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        return Ok(());
    }

    // 7a. Publish fresh diagnostics for all open files now that project_registry
    // is built and type_provider is synced. This ensures TS diagnostics appear
    // after background init without requiring an edit.
    {
        let open_uris = documents.open_uris();
        for uri_str in &open_uris {
            let uri: Uri = match uri_str.parse() {
                Ok(u) => u,
                Err(_) => continue,
            };

            let verter_diags = compute_verter_diagnostics_for(
                &documents,
                &uri,
                &cached_verter_diags,
                &project_registry,
                &fallback_linter,
            );

            let diagnostics = if let Some(tp) = &type_provider {
                let canonical_id = crate::documents::uri_to_canonical_id(&uri);
                let profile = tsx_profile.read().clone();
                let ide = documents.host.get_ide(&canonical_id, &profile);

                if let Some(ide) = ide {
                    let snapshot = resolver_snapshot.read().clone();
                    let Some(tsx_path) = snapshot.as_ref().and_then(|snapshot| {
                        provider_ide_path_for_source(&snapshot.resolver, &canonical_id, ide.is_jsx)
                    }) else {
                        continue;
                    };
                    let encoding = position_encoding.read().clone();
                    let tsx_li =
                        crate::documents::line_index::LineIndex::new(&ide.code, encoding.clone());
                    let mapper = ide
                        .source_map
                        .as_ref()
                        .and_then(|sm| PositionMapper::from_json(sm).ok());
                    let vue_source = documents.host.get_source(&canonical_id);

                    match (tp.get_diagnostics(&tsx_path).await, mapper, vue_source) {
                        (Ok(type_diags), Some(mapper), Some(vue_src)) => {
                            let vue_li =
                                crate::documents::line_index::LineIndex::new(&vue_src, encoding);
                            crate::tsgo::merge::merge_diagnostics(
                                verter_diags,
                                type_diags,
                                &tsx_li,
                                &mapper,
                                &vue_li,
                            )
                        }
                        _ => verter_diags,
                    }
                } else {
                    verter_diags
                }
            } else {
                verter_diags
            };

            client.publish_diagnostics(uri, diagnostics, None).await;
        }
    }

    client
        .send_notification::<VerterReady>(VerterReadyParams { gen: my_gen })
        .await;

    // Notify client about Vite configs that need trust approval
    for info in &trust_required {
        tracing::debug!(
            "vite config trust required: {} ({})",
            info.config_path,
            info.reason
        );
        client
            .send_notification::<ViteConfigTrustRequired>(ViteConfigTrustRequiredParams {
                config_path: info.config_path.clone(),
                workspace_root: info.workspace_root.clone(),
                reason: info.reason.clone(),
            })
            .await;
    }

    tracing::info!("background init complete (gen={my_gen})");
    Ok(())
}

pub(super) async fn drain_pending_snapshot_provider_sync(
    project_sync: Option<&ProjectSync>,
    documents: &DocumentRegistry,
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    pending_snapshot_provider_sync: &DashSet<String>,
    is_tsgo: bool,
    mru_canonical_ids: Option<&parking_lot::Mutex<Vec<String>>>,
) {
    let Some(sync) = project_sync else {
        pending_snapshot_provider_sync.clear();
        return;
    };
    let Some(snapshot) = resolver_snapshot.read().clone() else {
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
        let synced = sync_pending_snapshot_provider_file(
            sync,
            documents,
            &snapshot,
            provider_sync_states,
            &canonical_id,
            is_tsgo,
        )
        .await;

        if synced || documents.host.get_source(&canonical_id).is_none() {
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
    resolver_snapshot: &parking_lot::RwLock<Option<ResolverSnapshot>>,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    is_tsgo: bool,
) -> bool {
    let Some(sync) = project_sync else {
        return false;
    };
    let snapshot = match resolver_snapshot.read().clone() {
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
            if already_loaded {
                continue;
            }
        }

        // Load from disk if not in host
        let loaded = crate::compile_blockers::ensure_source_loaded_into_host(host, import_id);
        if !loaded {
            continue;
        }

        // Hydrate compile blockers with the now-available resolver
        crate::compile_blockers::hydrate_cached(
            &crate::compile_blockers::HydrationCache::default(),
            host,
            &snapshot.resolver,
            &crate::compile_blockers::HostFsProjectResolverReader::new(host),
            import_id,
            snapshot.generation,
        );

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
        let Some(api) = host.get_public_api(import_id) else {
            continue;
        };

        let Some(next_state) = crate::provider_sync::vue_sync_state_for_source(
            &snapshot.resolver,
            import_id,
            ide.as_ref().map(|output| output.is_jsx).unwrap_or(false),
        ) else {
            continue;
        };
        let transition = crate::provider_sync::prepare_sync_transition(
            provider_sync_states,
            import_id,
            next_state,
        );

        close_stale_provider_paths(sync, &transition.stale_paths, "aliased_resync").await;
        let mut committed_state = transition.next;
        let mut synced_this_file = false;
        if let Some(ide) = ide {
            let Some(ide_path) = committed_state.ide_path.clone() else {
                continue;
            };
            let result = if committed_state.ide_background_loaded {
                sync.sync_tsx(&ide_path, &ide.code).await
            } else {
                sync.open_tsx(&ide_path, &ide.code).await
            };
            if result.is_ok() {
                committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                synced_any = true;
                synced_this_file = true;
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
                synced_any = true;
                synced_this_file = true;
            }
        }

        if synced_this_file {
            crate::provider_sync::commit_sync_transition(
                provider_sync_states,
                import_id,
                committed_state,
            );
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
                crate::compile_blockers::ensure_source_loaded_into_host(host, &resolved);

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

        // Phase 1: Sync Vue dependencies first (so TSGO has .vue.ts targets before barrel)
        for vue_id in &barrel_vue_deps {
            // Skip if already synced in the main Vue pass
            if let Some(state) = provider_sync_states.get(vue_id.as_str()) {
                if state.ide_background_loaded && state.api_background_loaded {
                    continue;
                }
            }

            let loaded = crate::compile_blockers::ensure_source_loaded_into_host(host, vue_id);
            if !loaded {
                continue;
            }
            crate::compile_blockers::hydrate_cached(
                &crate::compile_blockers::HydrationCache::default(),
                host,
                &snapshot.resolver,
                &crate::compile_blockers::HostFsProjectResolverReader::new(host),
                vue_id,
                snapshot.generation,
            );
            let profile = documents.tsx_profile.read().clone();
            if host.ensure_compiled(vue_id, &profile).is_err() {
                continue;
            }

            configure_provider_paths_for_source(sync, &snapshot, vue_id, true).await;

            let ide = host.get_ide(vue_id, &profile);
            let Some(api) = host.get_public_api(vue_id) else {
                continue;
            };

            let Some(next_state) = crate::provider_sync::vue_sync_state_for_source(
                &snapshot.resolver,
                vue_id,
                ide.as_ref().map(|output| output.is_jsx).unwrap_or(false),
            ) else {
                continue;
            };
            let transition = crate::provider_sync::prepare_sync_transition(
                provider_sync_states,
                vue_id,
                next_state,
            );
            close_stale_provider_paths(sync, &transition.stale_paths, "barrel_vue_dep").await;
            let mut committed_state = transition.next;
            let mut synced_this = false;

            if let Some(ide) = ide {
                if let Some(ide_path) = committed_state.ide_path.clone() {
                    let result = if committed_state.ide_background_loaded {
                        sync.sync_tsx(&ide_path, &ide.code).await
                    } else {
                        sync.open_tsx(&ide_path, &ide.code).await
                    };
                    if result.is_ok() {
                        committed_state.set_background_loaded(ProviderPathKind::Ide, true);
                        synced_any = true;
                        synced_this = true;
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
                    synced_any = true;
                    synced_this = true;
                }
            }
            if synced_this {
                crate::provider_sync::commit_sync_transition(
                    provider_sync_states,
                    vue_id,
                    committed_state,
                );
            }
        }

        // Phase 2: Sync barrel files (their rewritten imports now point to .vue.ts)
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
    snapshot: &ResolverSnapshot,
    canonical_id: &str,
) -> Option<(String, serde_json::Value)> {
    let owner = snapshot.resolver.owner_for_file(canonical_id)?;
    let tsconfig_path = owner.tsconfig_path.as_deref()?;
    verter_vfs::config::raw_paths_json(std::path::Path::new(tsconfig_path))
}

pub(crate) async fn configure_provider_paths_for_source(
    sync: &ProjectSync,
    snapshot: &ResolverSnapshot,
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

pub(super) async fn sync_pending_snapshot_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
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
        sync_pending_non_vue_provider_file(
            sync,
            documents,
            snapshot,
            provider_sync_states,
            canonical_id,
            is_tsgo,
        )
        .await
    }
}

pub(super) async fn sync_pending_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
    let reader = crate::compile_blockers::HostFsProjectResolverReader::new(documents.host());
    crate::compile_blockers::hydrate_vue_compile_blockers(
        documents.host(),
        &snapshot.resolver,
        &reader,
        canonical_id,
    );
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
    let Some(next_state) =
        crate::provider_sync::vue_sync_state_for_source(&snapshot.resolver, canonical_id, is_jsx)
    else {
        return false;
    };
    if is_tsgo {
        configure_provider_paths_for_source(sync, snapshot, canonical_id, true).await;
    }

    let transition = prepare_sync_transition(provider_sync_states, canonical_id, next_state);
    close_stale_provider_paths(sync, &transition.stale_paths, "pending_snapshot").await;

    let mut committed_state = transition.next;
    let is_open = documents.canonical_id_to_uri(canonical_id).is_some();
    let mut synced_any = false;

    if let Some(api) = block_in_place_if_available(|| documents.host.get_public_api(canonical_id)) {
        let Some(dts_path) = committed_state.api_path.clone() else {
            return false;
        };
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
                synced_any = true;
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
            return false;
        };
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
                synced_any = true;
            }
            Err(error) => {
                tracing::warn!(
                    "pending_snapshot: failed to sync provider IDE path {ide_path}: {error}"
                );
            }
        }
    }

    if synced_any {
        commit_sync_transition(provider_sync_states, canonical_id, committed_state);
    }
    synced_any
}

pub(super) async fn sync_pending_non_vue_provider_file(
    sync: &ProjectSync,
    documents: &DocumentRegistry,
    snapshot: &ResolverSnapshot,
    provider_sync_states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    is_tsgo: bool,
) -> bool {
    let Some(source) = documents.host.get_source(canonical_id) else {
        return false;
    };
    let module_references = block_in_place_if_available(|| {
        documents
            .host
            .upsert(verter_host::UpsertRequest {
                canonical_id: Some(canonical_id.to_string()),
                input_id: canonical_id.to_string(),
                source: source.clone(),
                file_kind: verter_host::FileKind::NonSfc,
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
                    .map(|entry| verter_host::DependencyResolution {
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

/// Returns `true` if `types_dir` contains files written by Verter's stub
/// generator (detected via `// Auto-generated by verter-lsp` marker in
/// `index.d.ts` or the minimal `"types":"index.d.ts"` in `package.json`).
/// Returns `false` for a real installed `@verter/types` package.
pub(super) fn is_generated_verter_types_stub(types_dir: &std::path::Path) -> bool {
    let index_path = types_dir.join("index.d.ts");
    let pkg_path = types_dir.join("package.json");
    let index = std::fs::read_to_string(index_path).ok();
    let pkg = std::fs::read_to_string(pkg_path).ok();

    index
        .as_deref()
        .map(|contents| contents.starts_with("// Auto-generated by verter-lsp"))
        .unwrap_or(false)
        || pkg
            .as_deref()
            .map(|contents| contents.contains(r#""types":"index.d.ts""#))
            .unwrap_or(false)
}

/// Check if a canonical ID refers to a file inside a Verter-generated
/// `@verter/types` stub directory. Uses two stages:
/// 1. Cheap path prefix match (filters 99.9%+ of events)
/// 2. Marker-based content check (I/O, only for matching paths)
///
/// Returns `false` for files inside a real installed `@verter/types` package
/// (no marker comment), allowing those watcher events to proceed normally.
pub(crate) fn is_generated_verter_types_event(canonical_id: &str) -> bool {
    let Some(pos) = canonical_id.find("/node_modules/@verter/types/") else {
        return false;
    };
    let types_dir = &canonical_id[..pos + "/node_modules/@verter/types".len()];
    is_generated_verter_types_stub(std::path::Path::new(types_dir))
}

/// Write `desired` to `path` only if the file is missing or its content differs.
/// Returns `Ok(true)` when a write occurred, `Ok(false)` when skipped.
pub(super) fn write_if_changed(path: &std::path::Path, desired: &str) -> std::io::Result<bool> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == desired {
            return Ok(false);
        }
    }
    std::fs::write(path, desired)?;
    Ok(true)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct MaterializeVerterTypesResult {
    pub(super) any_failed: bool,
    pub(super) wrote_any: bool,
}

/// Materialise `@verter/types` in all workspace roots that don't already have it.
/// Returns whether any root failed and whether any files were written.
pub(super) fn materialize_verter_types(roots: &[String]) -> MaterializeVerterTypesResult {
    let mut result = MaterializeVerterTypesResult::default();
    for root_uri in roots {
        let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
        let root_path = std::path::PathBuf::from(&canonical);
        let types_dir = root_path.join("node_modules/@verter/types");
        let pkg_path = types_dir.join("package.json");
        if !pkg_path.exists() || is_generated_verter_types_stub(&types_dir) {
            match std::fs::create_dir_all(&types_dir) {
                Ok(()) => {
                    let dts = verter_host::VERTER_TYPES_STANDALONE_DTS;
                    let pkg = r#"{"name":"@verter/types","types":"index.d.ts"}"#;
                    let dts_written = match write_if_changed(&types_dir.join("index.d.ts"), dts) {
                        Ok(w) => w,
                        Err(e) => {
                            tracing::warn!("failed to write @verter/types index.d.ts: {e}");
                            result.any_failed = true;
                            continue;
                        }
                    };
                    let pkg_written = match write_if_changed(&types_dir.join("package.json"), pkg) {
                        Ok(w) => w,
                        Err(e) => {
                            tracing::warn!("failed to write @verter/types package.json: {e}");
                            continue;
                        }
                    };
                    result.wrote_any |= dts_written || pkg_written;
                    if dts_written || pkg_written {
                        tracing::info!(
                            "@verter/types materialised/refreshed at {}",
                            types_dir.display()
                        );
                    } else {
                        tracing::debug!("@verter/types up-to-date at {}", types_dir.display());
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to create @verter/types dir: {e} — falling back to embed"
                    );
                    result.any_failed = true;
                }
            }
        }
    }
    result
}

/// Collect tsconfig patterns for all workspace roots (blocking FS walk).
pub(super) fn collect_tsconfig_patterns(roots: &[String]) -> Vec<String> {
    let mut patterns = Vec::new();
    for root_uri in roots {
        let canonical = crate::documents::uri_to_canonical_id_from_str(root_uri);
        let root_path = std::path::PathBuf::from(&canonical);
        for entry in verter_vfs::config::discover_tsconfigs(&root_path) {
            patterns.push(format!("{}/**", entry.root));
        }
    }
    patterns
}
