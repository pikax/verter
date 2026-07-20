use super::*;

use super::background_drain::{
    drain_pending_snapshot_provider_sync, resync_aliased_imports_for_open_files,
};

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
    pub(super) vite_opts: verter_workspace::ViteConfigOptions,
    pub(super) init_lint_opts: Option<serde_json::Value>,
    pub(super) my_gen: u64,
    pub(super) client: Client,
    pub(super) type_provider: Option<Arc<dyn TypeProvider>>,
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
        Arc<parking_lot::RwLock<Option<Arc<verter_workspace::FilesystemWorkspace>>>>,
    /// The tsserver carrier-publish coordinator (the store-publish membership
    /// path). `Some` only for the tsserver engine; `None` for tsgo and when no
    /// type provider is connected.
    pub(super) carrier_publish_coordinator: Option<crate::external_ts::CarrierPublishCoordinator>,
    pub(super) carrier_transaction_coordinator:
        Arc<crate::external_ts::CarrierTransactionCoordinator>,
    /// The proactive declaration-overlay lifecycle owner (shared with the server's
    /// `did_close` lifecycle).
    pub(super) decl_overlay_owner: Arc<super::DeclOverlayOwner>,
    /// Project-level coalescing singleflight for `resync_open_files`, shared with
    /// the server so concurrent init generations collapse their resync sweeps.
    pub(super) resync_coordinator: Arc<crate::resync_singleflight::ResyncCoordinator>,
}

struct PublishedWorkspaceBuild {
    root: verter_workspace::PublishedRoot,
    trust_required: Vec<verter_workspace::ViteConfigTrustInfo>,
    configured_projects: Vec<(String, String)>,
}

fn build_published_workspace(
    ws: &verter_workspace::FilesystemWorkspace,
    canonical_roots: &[String],
    vite_opts: &verter_workspace::ViteConfigOptions,
    generation: u64,
    init_lint_opts: Option<serde_json::Value>,
    conditional_root_narrowing: bool,
) -> PublishedWorkspaceBuild {
    let build = verter_workspace::build_workspace_snapshot(
        ws,
        canonical_roots,
        verter_workspace::workspace_snapshot::SnapshotGeneration(generation),
        vite_opts,
    );
    let trust_required: Vec<verter_workspace::ViteConfigTrustInfo> = build.trust_required.clone();
    let snapshot = Arc::new(build.snapshot);
    let mut views = crate::workspace_state::build_lsp_views(ws, &snapshot, trust_required.clone());

    if let Some(init_opts) = init_lint_opts.as_ref() {
        crate::workspace_state::apply_default_lint_to_views(&mut views, init_opts);
    }
    if conditional_root_narrowing {
        crate::workspace_state::set_conditional_root_narrowing(&mut views, true);
    }

    let configured_projects = snapshot
        .projects
        .iter()
        .filter_map(|project| match &project.payload {
            verter_workspace::workspace_snapshot::ProjectPayload::Configured {
                tsconfig_path,
                ..
            } => Some((
                project.root.as_str().to_string(),
                tsconfig_path.as_str().to_string(),
            )),
            verter_workspace::workspace_snapshot::ProjectPayload::Fallback { .. } => None,
        })
        .collect();

    PublishedWorkspaceBuild {
        root: verter_workspace::PublishedRoot::with_ext(snapshot, Box::new(views)),
        trust_required,
        configured_projects,
    }
}

/// Admit provider-type materialization into the live initialization generation.
///
/// The filesystem writes themselves are immutable/idempotent, but their failure
/// result changes the shared compiler profile. Keeping the generation test and
/// that mutation in one function makes it impossible for a superseded init to
/// flip `embed_ambient_types` before the caller returns without reconfiguring
/// the provider.
fn admit_materialized_provider_types(
    init_generation: &std::sync::atomic::AtomicU64,
    my_gen: u64,
    tsx_profile: &parking_lot::RwLock<verter_session::CompileProfile>,
    materialize_types: MaterializeVerterTypesResult,
) -> bool {
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        return false;
    }
    if materialize_types.any_failed {
        tsx_profile.write().embed_ambient_types = true;
    }
    true
}

/// Run all blocking initialization work in the background.
///
/// This function is spawned from `initialized()` and performs:
/// 1. Exact workspace snapshot build (blocking: tsconfig discovery/materialization)
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
        carrier_publish_coordinator,
        carrier_transaction_coordinator,
        decl_overlay_owner,
        resync_coordinator,
    } = args;

    let host = documents.host_arc();
    let tsx_profile = Arc::clone(&documents.tsx_profile);
    let canonical_roots: Vec<String> = roots
        .iter()
        .map(|uri| crate::documents::uri_to_canonical_id_from_str(uri))
        .collect();
    let conditional_root_narrowing = tsx_profile.read().conditional_root_narrowing;
    // Normally the VFS workspace is created early in `initialize()` when
    // `workspace_folders` is present in `InitializeParams`. If a client omits
    // workspace folders (or in tests that call `initialized()` directly without
    // going through `initialize()`), the workspace will be `None` here. Create
    // it lazily so `background_init` is self-sufficient in both cases.
    let ws = {
        let existing = vfs_workspace.read().clone();
        match existing {
            Some(ws) => ws,
            None => {
                let new_ws = Arc::new(verter_workspace::FilesystemWorkspace::new(
                    verter_workspace::FilesystemOptions {
                        roots: canonical_roots.clone(),
                        eager_preload: false,
                    },
                ));
                new_ws.set_project_graph(verter_workspace::ProjectGraph::new());
                let ws_dyn: Arc<dyn verter_workspace::WorkspaceAccess> = new_ws.clone();
                host.set_workspace(ws_dyn);
                *vfs_workspace.write() = Some(Arc::clone(&new_ws));
                tracing::info!(
                    "VFS workspace created lazily in background_init with {} roots",
                    canonical_roots.len()
                );
                new_ws
            }
        }
    };

    // 1. Build exact published workspace (spawn_blocking — blocking I/O: tsconfig discovery/materialization)
    let canonical_roots_for_build = canonical_roots.clone();
    let vite_opts_for_build = vite_opts.clone();
    let ws_for_build = Arc::clone(&ws);
    let published_build = tokio::task::spawn_blocking(move || {
        build_published_workspace(
            &ws_for_build,
            &canonical_roots_for_build,
            &vite_opts_for_build,
            my_gen,
            init_lint_opts,
            conditional_root_narrowing,
        )
    })
    .await;

    let PublishedWorkspaceBuild {
        root: published_root,
        trust_required,
        configured_projects,
    } = match published_build {
        Ok(build) => build,
        Err(e) => {
            if e.is_panic() {
                tracing::error!("workspace snapshot build panicked: {e}");
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

    // 2. Generation check before any commit or provider reconfiguration
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded, discarding built snapshot");
        return Ok(());
    }

    // Materialize compiler-owned provider declarations BEFORE the provider is
    // configured/reopened. An existing generated stub may change across a
    // Verter upgrade; reopening first lets tsserver/tsgo cache the old module,
    // while the generated-stub watcher is intentionally ignored to prevent a
    // self-write loop. Publishing the immutable asset first gives the one
    // provider resync below the final bytes on fresh installs and upgrades.
    let roots_for_types = roots.clone();
    let materialize_types =
        tokio::task::spawn_blocking(move || materialize_verter_types(&roots_for_types))
            .await
            .unwrap_or(MaterializeVerterTypesResult {
                any_failed: true,
                wrote_any: false,
            });

    // The blocking materialization may overlap a newer workspace-folder
    // initialization. Revalidate before mutating the shared TSX profile or
    // reconfiguring the provider: immutable stub writes for the old roots are
    // harmless, but stale roots/options must never become live authority.
    if !admit_materialized_provider_types(&init_generation, my_gen, &tsx_profile, materialize_types)
    {
        tracing::info!(
            "init gen={my_gen} superseded during provider-type materialization, discarding"
        );
        return Ok(());
    }

    // 3. Type provider: workspace folder sync + path config (async, non-blocking)
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

        for (project_root, tsconfig_path) in &configured_projects {
            if let Some((base_url, paths)) =
                crate::svelte_assets::owner_provider_path_config(&*ws, tsconfig_path, project_root)
            {
                tracing::info!(
                    "configuring tsserver paths for {} via {} (baseUrl: {})",
                    project_root,
                    tsconfig_path,
                    base_url,
                );
                // Inject the Svelte IDE-projection assets HERE —
                // the COMMON per-owner-project path-config call site, so EVERY
                // provider (extension / TSGO / tsserver) receives the
                // @verter/svelte-jsx shim rows + the transitive `svelte` rows
                // resolved against THIS OWNER PROJECT's install (absent → fail
                // closed). The owner project root is the per-project resolution
                // anchor for multi-`svelte` monorepos.
                if let Err(e) = tp.configure_paths(&base_url, paths).await {
                    tracing::warn!("failed to configure tsserver paths: {e}");
                }
            }
        }

        // Layer 2: Re-open all files with correct projectRootPath now that
        // workspace folders and tsconfig paths are configured. Coalesced so a
        // superseded init generation's resync folds into the live one instead of
        // stacking a second full close+reopen sweep on the interactive lane.
        //
        // Ordering note: awaiting this no longer implies the sweep FINISHED. When
        // another pass is already sweeping, this call folds into it and returns
        // immediately. That is safe here because a folding pass is by definition
        // concurrent with another one, and the generation check below refuses to
        // publish a snapshot for anything but the live generation.
        crate::resync_singleflight::resync_open_files_coalesced(
            &resync_coordinator,
            Arc::clone(tp),
        )
        .await;
    }

    // 4. Generation check → commit snapshot
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded, discarding built snapshot");
        return Ok(());
    }

    ws.publish_snapshot(published_root);
    tracing::info!(
        "Published exact workspace snapshot with consumer views for {} roots",
        canonical_roots.len()
    );

    drain_pending_snapshot_provider_sync(
        project_sync.as_ref(),
        &documents,
        &vfs_workspace,
        &provider_sync_states,
        &pending_snapshot_provider_sync,
        is_tsgo,
        Some(&mru_canonical_ids),
        carrier_publish_coordinator.as_ref(),
        &carrier_transaction_coordinator,
    )
    .await;

    // 4b. Re-resolve aliased imports for open files now that the VFS snapshot is built.
    let aliased_imports_synced = resync_aliased_imports_for_open_files(
        &documents,
        project_sync.as_ref(),
        &vfs_workspace,
        &provider_sync_states,
        is_tsgo,
        carrier_publish_coordinator.as_ref(),
        &decl_overlay_owner,
        // The closure pass runs UNDER this init's generation. A stale (superseded)
        // init pass that reaches the overlay reconcile must not close an overlay a
        // newer live init pass re-established reachability for.
        my_gen,
        &carrier_transaction_coordinator,
    )
    .await;

    // If new imported .vue.ts files were synced, re-open the active files in tsserver
    // so it picks up the newly available modules and clears stale TS2307 diagnostics.
    if aliased_imports_synced {
        if let Some(tp) = &type_provider {
            crate::resync_singleflight::resync_open_files_coalesced(
                &resync_coordinator,
                Arc::clone(tp),
            )
            .await;
        }
    }

    // 6. Generation check → spawn workspace scanner
    if init_generation.load(std::sync::atomic::Ordering::Acquire) != my_gen {
        tracing::info!("init gen={my_gen} superseded before scanner, discarding");
        return Ok(());
    }

    // Get the published snapshot for snapshot-driven scanner classification.
    // When a snapshot is available, the scanner uses `classify_from_snapshot()`
    // for ownership-based tier classification, eliminating the need for
    // `collect_tsconfig_patterns()` glob patterns.
    let scanner_snapshot = vfs_workspace
        .read()
        .as_ref()
        .and_then(|ws| ws.load_published())
        .map(|root| Arc::clone(&root.snapshot));

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
            vfs_workspace: Arc::clone(&vfs_workspace),
            provider_sync_states: Arc::clone(&provider_sync_states),
            provider_surfaces: documents.provider_surfaces().clone(),
            is_tsgo,
            carrier_publish_coordinator: carrier_publish_coordinator.clone(),
            carrier_transaction_coordinator: Arc::clone(&carrier_transaction_coordinator),
            pending_snapshot_provider_sync: Arc::clone(&pending_snapshot_provider_sync),
            tsx_profile: tsx_profile.read().clone(),
            tsconfig_patterns: Vec::new(),
            workspace_snapshot: scanner_snapshot,
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
        let type_provider = type_provider.clone();
        let position_encoding = position_encoding.clone();
        let init_generation = Arc::clone(&init_generation);
        let vfs_workspace = Arc::clone(&vfs_workspace);
        let provider_sync_states = Arc::clone(&provider_sync_states);
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

                let verter_diags = {
                    let vfs_ws = vfs_workspace.read();
                    compute_verter_diagnostics_for_with_views(
                        &documents,
                        &uri,
                        &cached_verter_diags,
                        vfs_ws.as_deref(),
                    )
                };

                let diagnostics = if let Some(tp) = &type_provider {
                    let canonical_id = crate::documents::uri_to_canonical_id(&uri);
                    let encoding = position_encoding.read().clone();
                    crate::sync_coordinator::carrier_provider_diagnostics(
                        &documents,
                        &provider_sync_states,
                        tp.as_ref(),
                        encoding,
                        &canonical_id,
                        verter_diags,
                    )
                    .await
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

            let verter_diags = {
                let vfs_ws = vfs_workspace.read();
                compute_verter_diagnostics_for_with_views(
                    &documents,
                    &uri,
                    &cached_verter_diags,
                    vfs_ws.as_deref(),
                )
            };

            let diagnostics = if let Some(tp) = &type_provider {
                let canonical_id = crate::documents::uri_to_canonical_id(&uri);
                let encoding = position_encoding.read().clone();
                crate::sync_coordinator::carrier_provider_diagnostics(
                    &documents,
                    &provider_sync_states,
                    tp.as_ref(),
                    encoding,
                    &canonical_id,
                    verter_diags,
                )
                .await
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
                    let dts = verter_session::VERTER_TYPES_STANDALONE_DTS;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn superseded_type_materialization_cannot_mutate_the_live_compile_profile() {
        let generation = std::sync::atomic::AtomicU64::new(2);
        let profile = parking_lot::RwLock::new(verter_session::CompileProfile::default());

        let admitted = admit_materialized_provider_types(
            &generation,
            1,
            &profile,
            MaterializeVerterTypesResult {
                any_failed: true,
                wrote_any: false,
            },
        );

        assert!(!admitted, "a superseded initialization must fail admission");
        assert!(
            !profile.read().embed_ambient_types,
            "a stale materialization failure must not change the live compiler profile"
        );
    }

    #[test]
    fn current_type_materialization_failure_enables_the_safe_embedded_fallback() {
        let generation = std::sync::atomic::AtomicU64::new(3);
        let profile = parking_lot::RwLock::new(verter_session::CompileProfile::default());

        let admitted = admit_materialized_provider_types(
            &generation,
            3,
            &profile,
            MaterializeVerterTypesResult {
                any_failed: true,
                wrote_any: false,
            },
        );

        assert!(admitted, "the current initialization must be admitted");
        assert!(
            profile.read().embed_ambient_types,
            "the current materialization failure must retain the safe embedded fallback"
        );
    }

    #[test]
    fn build_published_workspace_materializes_exact_snapshot_with_views() {
        let tmp = tempdir().expect("temp dir");
        let root = crate::test_utils::canonical_test_path(tmp.path());
        std::fs::create_dir_all(tmp.path().join("src")).expect("src dir");
        std::fs::write(
            tmp.path().join("tsconfig.app.json"),
            r#"{
  "include": ["src/**/*"]
}"#,
        )
        .expect("tsconfig should be written");
        std::fs::write(
            tmp.path().join("src").join("App.ts"),
            "export const app = 1;",
        )
        .expect("source file should be written");

        let ws = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let build = build_published_workspace(
            &ws,
            std::slice::from_ref(&root),
            &verter_workspace::ViteConfigOptions::default(),
            7,
            None,
            false,
        );
        let app_path = format!("{root}/src/App.ts");

        assert_eq!(
            build.root.snapshot.generation.0, 7,
            "helper should pin the published snapshot generation"
        );
        assert!(
            build
                .root
                .ext::<crate::workspace_state::LspViews>()
                .is_some(),
            "helper should attach LSP views before publication"
        );
        assert!(
            matches!(
                build
                    .root
                    .snapshot
                    .configured_owner_resolution_for_file(&app_path),
                verter_workspace::ConfiguredOwnerResolution::Unique(_)
            ),
            "exact snapshot builder should materialize include-owned files before publish"
        );
        assert!(
            build
                .root
                .snapshot
                .resolver
                .nearest_config_for_path(&app_path)
                .is_some(),
            "resolver published with the snapshot should own the configured file"
        );
    }
}
