//! LSP lifecycle method bodies.
//!
//! Free functions hosting the bodies of `impl LanguageServer for
//! VerterLanguageServer` lifecycle methods (initialize, initialized,
//! shutdown, did_open, did_change, did_close, did_save,
//! did_change_workspace_folders, did_change_watched_files,
//! did_create_files, did_delete_files).
//!
//! The trait impl block stays in `mod.rs` (Rust forbids cross-file
//! trait-impl splits); each trait method is a 1-line stub that
//! delegates to the corresponding `handle_<method>` free function
//! here. The free functions take `server: &VerterLanguageServer` as
//! their first argument and access struct fields directly because
//! they live in a private child module of `server/mod.rs`.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

use crate::capabilities::server_capabilities;
use crate::documents::uri_to_canonical_id;
use crate::provider_sync::ProviderPathKind;

use super::background_init::{is_generated_verter_types_event, spawn_heartbeat};
use super::handler_guard::{block_in_place_if_available, HandlerGuard, ACTIVE_HANDLERS};
use super::protocol_types::*;
use super::server_utils::*;
use super::VerterLanguageServer;

pub(super) async fn handle_initialize(
    server: &VerterLanguageServer,
    params: InitializeParams,
) -> Result<InitializeResult> {
    tracing::info!("verter-lsp initializing");
    tracing::info!(
        "type provider: {} ({})",
        if server.type_provider.is_some() {
            "connected"
        } else {
            "NONE — no TypeScript intellisense"
        },
        server.type_provider_kind,
    );

    // ── Position encoding negotiation (LSP 3.17) ────────────────────
    // Prefer UTF-8 (native Rust encoding — no conversion needed),
    // then UTF-32, then UTF-16. Default to UTF-16 per LSP spec.
    let encoding = params
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref())
        .and_then(|encodings| {
            if encodings.contains(&PositionEncodingKind::UTF8) {
                Some(PositionEncodingKind::UTF8)
            } else if encodings.contains(&PositionEncodingKind::UTF32) {
                Some(PositionEncodingKind::UTF32)
            } else if encodings.contains(&PositionEncodingKind::UTF16) {
                Some(PositionEncodingKind::UTF16)
            } else {
                None
            }
        })
        .unwrap_or(PositionEncodingKind::UTF16);
    tracing::info!("negotiated position encoding: {}", encoding.as_str());
    *server.position_encoding.write() = encoding.clone();
    server.documents.set_encoding(encoding.clone());

    // Extract and store all workspace roots
    if let Some(folders) = &params.workspace_folders {
        let mut roots = Vec::new();
        for folder in folders {
            tracing::info!("workspace folder: {}", folder.uri.as_str());
            roots.push(folder.uri.as_str().to_string());
        }
        *server.workspace_roots.lock().await = roots.clone();

        // Create VFS workspace early so relative imports resolve before
        // background_init completes. The project graph starts empty —
        // Step 1's ownership fix makes relative/node_modules/hash imports
        // work without project ownership. background_init later calls
        // set_project_graph() to populate alias resolution.
        let canonical_roots: Vec<String> = roots
            .iter()
            .map(|r| crate::documents::uri_to_canonical_id_from_str(r))
            .collect();
        let ws = std::sync::Arc::new(verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions {
                roots: canonical_roots,
                eager_preload: false,
            },
        ));
        ws.set_project_graph(verter_workspace::ProjectGraph::new());
        let ws_dyn: std::sync::Arc<dyn verter_workspace::WorkspaceAccess> = ws.clone();
        server.documents.host().set_workspace(ws_dyn);
        *server.vfs_workspace.write() = Some(ws);
        tracing::info!(
            "VFS workspace created early in initialize() with {} roots",
            roots.len()
        );
    }

    // Parse initialization options (statistics config, lint config, etc.)
    if let Some(opts) = &params.initialization_options {
        tracing::debug!("initialization options: {opts}");
        if let Some(stats_enabled) = opts
            .get("statistics")
            .and_then(|s| s.get("enabled"))
            .and_then(|v| v.as_bool())
        {
            server.statistics.set_enabled(stats_enabled);
            tracing::info!(
                "statistics: {}",
                if stats_enabled { "enabled" } else { "disabled" }
            );
        }
        // Store lint options for use in initialized()
        if opts.get("lint").is_some() {
            *server.init_lint_options.lock().await = Some(opts.clone());
        }
        // Read viteConfig settings
        {
            let mut vite_opts = server.vite_config_options.lock().await;
            if let Some(vite_config) = opts.get("viteConfig") {
                if let Some(enabled) = vite_config.get("enabled").and_then(|v| v.as_bool()) {
                    vite_opts.enabled = enabled;
                }
                if let Some(trusted) = vite_config.get("trustedFiles").and_then(|v| v.as_array()) {
                    vite_opts.trusted_files = trusted
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.replace('\\', "/")))
                        .collect();
                }
            }
            tracing::info!(
                "vite config: enabled={}, trusted_files={}",
                vite_opts.enabled,
                vite_opts.trusted_files.len()
            );
        }
        // Read inlayHints.enabled setting (default: true)
        if let Some(enabled) = opts
            .get("inlayHints")
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
        {
            server
                .inlay_hints_enabled
                .store(enabled, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                "type provider inlay hints: {}",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        // hover.provenance is opt-in.
        let hover_opts = crate::config::parse_hover_init_options(opts);
        server
            .hover_provenance_enabled
            .store(hover_opts.provenance, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(
            "hover provenance: {}",
            if hover_opts.provenance {
                "enabled"
            } else {
                "disabled (default)"
            }
        );

        let experimental = crate::config::parse_experimental_init_options(opts);
        server
            .documents
            .tsx_profile
            .write()
            .conditional_root_narrowing = experimental.conditional_root_narrowing;
        tracing::info!(
            "conditional root narrowing: {}",
            if experimental.conditional_root_narrowing {
                "enabled"
            } else {
                "disabled"
            }
        );
        server.documents.tsx_profile.write().strict_slots = experimental.strict_slots;
        tracing::info!(
            "strict slots: {}",
            if experimental.strict_slots {
                "enabled"
            } else {
                "disabled"
            }
        );
    }

    Ok(InitializeResult {
        capabilities: server_capabilities(&encoding),
        server_info: Some(ServerInfo {
            name: "verter-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        offset_encoding: Some(encoding.as_str().to_owned()),
    })
}

pub(super) async fn handle_initialized(server: &VerterLanguageServer, _params: InitializedParams) {
    tracing::info!("verter-lsp initialized");

    // A. Spawn heartbeat FIRST — ensures the extension sees heartbeats
    // even while background initialization is running.
    spawn_heartbeat(server.client.clone());

    // B. Send immediate non-blocking notifications
    let tp_label = server.type_provider_kind.to_string();
    server
        .client
        .log_message(
            MessageType::INFO,
            format!(
                "verter-lsp {} initialized (type provider: {tp_label})",
                env!("CARGO_PKG_VERSION")
            ),
        )
        .await;

    // Notify the extension of the type provider child PID for orphan cleanup.
    if let Some(tp) = &server.type_provider {
        if let Some(pid) = tp.child_pid() {
            let kind = server.type_provider_kind.to_string().to_lowercase();
            server
                .client
                .send_notification::<TypeProviderStarted>(TypeProviderStartedParams {
                    pid,
                    kind: kind.clone(),
                })
                .await;
            // Legacy TsgoStarted notification — only send when TSGO is actually active
            if matches!(server.type_provider_kind, crate::TypeProviderKind::Tsgo) {
                server
                    .client
                    .send_notification::<TsgoStarted>(TsgoStartedParams { pid })
                    .await;
            }
        }
    }

    // Send type provider status notification — tells the extension which
    // provider is active (or why none could be started) for the status bar.
    {
        let kind = server.type_provider_kind.to_string().to_lowercase();
        let reason = if matches!(server.type_provider_kind, crate::TypeProviderKind::None) {
            server.type_provider_none_reason.clone()
        } else {
            None
        };
        server
            .client
            .send_notification::<TypeProviderStatus>(TypeProviderStatusParams {
                kind,
                reason: reason.clone(),
            })
            .await;
        // When no type provider is available, also show a warning message
        if matches!(server.type_provider_kind, crate::TypeProviderKind::None) {
            let msg = if let Some(ref r) = reason {
                format!(
                    "Verter: No TypeScript type provider available ({r}). \
                     Hover, completions, and go-to-definition will be limited to \
                     Verter's built-in analysis."
                )
            } else {
                "Verter: No TypeScript type provider available. \
                 Hover, completions, and go-to-definition will be limited to \
                 Verter's built-in analysis."
                    .into()
            };
            server.client.show_message(MessageType::WARNING, msg).await;
        }
    }

    // Suggest switching to TSGO if auto mode chose tsserver
    if server.suggest_tsgo {
        server.client
            .show_message(
                MessageType::INFO,
                "Verter: Using workspace TypeScript (tsserver) for type checking. \
                 For faster performance, install TSGO and set verter.typeProvider to \"tsgo\" in VS Code settings.",
            )
            .await;
    }

    // Warn about TSGO limitations
    if matches!(server.type_provider_kind, crate::TypeProviderKind::Tsgo) {
        server
            .client
            .show_message(
                MessageType::WARNING,
                "Verter: TSGO has known limitations — (1) re-exported .vue components \
                 (e.g. barrel files) may lose their typing; (2) path aliases from \
                 composite/referenced tsconfig files (e.g. tsconfig.app.json) are not \
                 resolved. If you experience issues, switch to tsserver: set \
                 verter.typeProvider to \"tsserver\".",
            )
            .await;
    }

    // Notify extension of MCP HTTP port (dynamic, OS-assigned).
    if let Some(port) = server.mcp_port {
        server
            .client
            .send_notification::<McpReady>(McpReadyParams { port })
            .await;
        tracing::info!("Sent $/verter/mcpReady with port {port}");
    }

    // C0. Eagerly populate type provider workspace roots so that
    // did_open (which can fire before background_init completes) sends
    // a reasonable projectRootPath to tsserver.
    if let Some(tp) = &server.type_provider {
        let roots = server.workspace_roots.lock().await;
        if !roots.is_empty() {
            let added: Vec<serde_json::Value> = roots
                .iter()
                .map(|uri| {
                    serde_json::json!({
                        "uri": uri,
                        "name": uri.rsplit('/').next().unwrap_or(uri)
                    })
                })
                .collect();
            drop(roots);
            let _ = tp.update_workspace_folders(added, vec![]).await;
        }
    }

    // C. Spawn background init (fire-and-forget)
    let init_lint_opts = server.init_lint_options.lock().await.take();
    server
        .spawn_background_init(init_lint_opts, "initialization")
        .await;

    // D. Register file system watchers for external file changes.
    // This enables did_change_watched_files notifications for source files,
    // Vue SFCs, and config files changed outside the editor (e.g., git checkout,
    // build tools, other editors). Enables non-VS Code clients (Neovim, etc.)
    // to get full external change detection via the standard LSP mechanism.
    let watch_kind = Some(WatchKind::Change | WatchKind::Create | WatchKind::Delete);
    let _ = server
        .client
        .register_capability(vec![Registration {
            id: "verter-file-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/*.vue".to_string()),
                        kind: watch_kind,
                    },
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String(
                            "**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}".to_string(),
                        ),
                        kind: watch_kind,
                    },
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/tsconfig*.json".to_string()),
                        kind: watch_kind,
                    },
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/.verterrc.json".to_string()),
                        kind: watch_kind,
                    },
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String(
                            "**/vite.config.{ts,js,mjs,cjs,mts,cts}".to_string(),
                        ),
                        kind: watch_kind,
                    },
                    FileSystemWatcher {
                        glob_pattern: GlobPattern::String("**/package.json".to_string()),
                        kind: watch_kind,
                    },
                ],
            })
            .ok(),
        }])
        .await;
}

pub(super) async fn handle_shutdown(server: &VerterLanguageServer) -> Result<()> {
    tracing::info!("verter-lsp shutting down");
    // Gracefully shut down the type provider (sends LSP shutdown+exit to TSGO).
    if let Some(tp) = &server.type_provider {
        let _ = tp.shutdown().await;
    }
    server
        .client
        .log_message(MessageType::INFO, "verter-lsp shutting down")
        .await;
    Ok(())
}

pub(super) async fn handle_did_open(
    server: &VerterLanguageServer,
    params: DidOpenTextDocumentParams,
) {
    let _hg = HandlerGuard::new("did_open");
    let uri = &params.text_document.uri;
    let _timer = server
        .statistics
        .timer("did_open", Some(uri.as_str().to_string()));
    tracing::info!("did_open: {}", uri.as_str());
    let result = server.documents.did_open(&params.text_document);
    let current_canonical_id = server.documents.get_canonical_id(uri);
    // Touch MRU for snapshot drain ordering (after did_open registers the canonical ID)
    if let Some(canonical_id) = current_canonical_id.as_ref() {
        server.touch_mru(canonical_id);
        if canonical_id.ends_with(".vue") {
            server.refresh_vue_dependency_tracking(canonical_id);
        }
    }
    if result.diagnostics.has_errors {
        tracing::debug!(
            "did_open: {} errors for {}",
            result.diagnostics.diagnostics.len(),
            uri.as_str(),
        );
    }
    let startup_policy = did_open_startup_policy(server.type_provider_kind);
    let prewarm_imported_vue_apis = startup_policy.sync_imported_vue_files
        && matches!(server.type_provider_kind, crate::TypeProviderKind::Tsserver);
    let imported_vue_priority_ids = server
        .documents
        .get_analysis(uri)
        .map(|analysis| {
            // Primary: analysis.imports already has resolved_canonical_id from host
            // (works even before background_init builds the resolver snapshot)
            let mut ids = collect_imported_vue_priority_ids_from_imports_with_fallback(
                &analysis.imports,
                current_canonical_id.as_deref(),
                |parent, specifier| server.resolve_import_specifier(parent, specifier),
            );

            // Supplement: module_references for dynamic import()/require() cases
            // that aren't in analysis.imports (needs resolver, may return empty pre-init)
            if let Some(canonical_id) = current_canonical_id.as_ref() {
                let snapshot = server.published_resolver();
                let reader = LspProjectResolverReader::new(&server.documents);
                let dynamic_ids = collect_priority_vue_targets_from_module_references(
                    snapshot.as_ref(),
                    &reader,
                    canonical_id,
                    &analysis.module_references,
                );
                // Dedup: add only IDs not already in the primary set
                let seen: HashSet<String> = ids.iter().cloned().collect();
                for id in dynamic_ids {
                    if !seen.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            ids
        })
        .unwrap_or_default();
    // Signal the background scanner to prioritize this file's directory
    if let Some(scanner) = server.workspace_scanner.lock().await.as_ref() {
        if let Some(canonical_id) = current_canonical_id.as_ref() {
            scanner.signal_priority(canonical_id.clone());
        }
        for import_id in &imported_vue_priority_ids {
            scanner.signal_priority(import_id.clone());
        }
    }

    if prewarm_imported_vue_apis {
        for import_id in &imported_vue_priority_ids {
            server.sync_imported_vue_api_lightweight(import_id).await;
        }
    }

    // Active file IDE sync FIRST (Interactive priority) — enables typed hover immediately.
    // tsserver is the exception: imported Vue public APIs are warmed above so the initial
    // open does not snapshot missing `.vue.ts` modules into the configured project.
    let provider_sync_policy = did_open_provider_sync_policy(server.type_provider_kind);
    if provider_sync_policy.await_ide_sync {
        // Use ensure_current_file_synced for immediate IDE-only sync
        server.ensure_current_file_synced(uri).await;
    }

    // Imported Vue API warmup SECOND (Normal priority, never blocks active file)
    if startup_policy.sync_imported_vue_files && !prewarm_imported_vue_apis {
        for import_id in &imported_vue_priority_ids {
            let should_sync =
                !server.is_background_loaded_for_source_kind(import_id, ProviderPathKind::Api);
            if should_sync {
                server.sync_imported_vue_api_lightweight(import_id).await;
            }
        }
    }

    // API sync (deferred — queued for coordinator)
    if provider_sync_policy.await_api_sync {
        server.sync_api_to_provider(uri).await;
    } else if provider_sync_policy.background_api_sync {
        server.sync_api_to_provider_in_background(uri.clone());
    }
    // Signal coordinator for fresh diagnostics on open (not just on change).
    // This ensures re-opening a file after external modifications publishes
    // up-to-date merged diagnostics (Verter lint + type provider).
    if let Some(coordinator) = &server.sync_coordinator {
        if let Some(canonical_id) = current_canonical_id.as_ref() {
            server.needs_ide_sync.insert(canonical_id.clone());
            server.needs_deferred_sync.insert(canonical_id.clone());
            coordinator.signal(canonical_id.clone(), uri.as_str().to_string());
        }
    }

    if startup_policy.publish_diagnostics {
        server.publish_full_diagnostics(uri).await;
    }
}

pub(super) async fn handle_did_change(
    server: &VerterLanguageServer,
    params: DidChangeTextDocumentParams,
) {
    let _hg = HandlerGuard::new("did_change");
    let uri = params.text_document.uri.clone();
    let version = params.text_document.version;
    tracing::info!(
        "did_change ENTER v{version} {} thread={:?}",
        uri.as_str(),
        std::thread::current().id()
    );

    // Record change timestamp for typing cooldown (suppresses non-critical TSGO requests)
    server.last_change_ms.store(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        std::sync::atomic::Ordering::Relaxed,
    );

    // CRITICAL: Serialize did_change handlers via a tokio::sync::Mutex.
    //
    // tower-lsp dispatches did_change notifications CONCURRENTLY. Each handler calls
    // host.upsert() + host.ensure_compiled() which acquire std::sync::RwLock (blocking).
    // With N concurrent handlers on M worker threads, if N >= M all threads are blocked
    // on the RwLock, starving the runtime (no timers, heartbeats, or responses fire).
    //
    // By serializing through a tokio::sync::Mutex, waiting handlers YIELD their worker
    // thread instead of blocking it. Only one handler holds the blocking lock at a time.
    tracing::info!(
        "did_change MUTEX_WAIT v{version} active={} thread={:?}",
        ACTIVE_HANDLERS.load(std::sync::atomic::Ordering::Relaxed),
        std::thread::current().id()
    );
    let mutex_wait_start = std::time::Instant::now();
    let _guard = server.did_change_mutex.lock().await;
    tracing::info!(
        "did_change MUTEX_ACQUIRED v{version} wait={:?} thread={:?}",
        mutex_wait_start.elapsed(),
        std::thread::current().id()
    );
    tracing::info!("did_change MUTEX_ACQUIRED v{version}");

    let _timer = server
        .statistics
        .timer("did_change", Some(uri.as_str().to_string()));
    let is_virtual = server.documents.get_virtual_source_uri(&uri).is_some();

    tracing::info!(
        "did_change UPSERT_START v{version} thread={:?}",
        std::thread::current().id()
    );
    let upsert_start = std::time::Instant::now();
    let update_result = block_in_place_if_available(|| {
        server
            .documents
            .did_change_incremental(&uri, version, params.content_changes)
    });
    tracing::info!(
        "did_change UPSERT_DONE v{version} elapsed={:?} thread={:?}",
        upsert_start.elapsed(),
        std::thread::current().id()
    );

    // Virtual files don't need TSX sync or diagnostics.
    if is_virtual {
        tracing::info!("did_change EXIT (virtual) v{version}");
        return;
    }

    let style_only = update_result.changed && update_result.slice_changes.is_style_only();

    // Debounced type provider sync via SyncCoordinator.
    // All keystrokes reset the coordinator's timer → exactly 1 sync fires
    // after 300ms of silence. No concurrent spawned tasks.
    if !style_only {
        if let Some(canonical_id) = server.documents.get_canonical_id(&uri) {
            // Invalidate the hover provenance cache for this file.
            // Transitive deps are NOT invalidated (codified limitation;
            // see
            // `hover_provenance_cache_does_NOT_invalidate_on_transitive_dependency_change`).
            server
                .hover_provenance_cache
                .invalidate_canonical(&canonical_id);

            if canonical_id.ends_with(".vue") {
                server.refresh_vue_dependency_tracking(&canonical_id);
            }
            server.needs_ide_sync.insert(canonical_id.clone());
            server.needs_deferred_sync.insert(canonical_id.clone());
            if let Some(coordinator) = &server.sync_coordinator {
                coordinator.signal(canonical_id, uri.as_str().to_string());
            }

            // Eager TSX sync — send fresh TSX to type provider immediately.
            // sync_tsx is fire-and-forget (~1ms), so this adds negligible latency.
            // This ensures ALL subsequent requests (completion, hover, definition)
            // see fresh content without needing per-handler inline sync.
            if let Some(sync) = &server.project_sync {
                if let Some(ide) = server.documents.get_ide(&uri) {
                    if let Some(ide_path) = server.eager_syncable_ide_path_for_uri(&uri) {
                        if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                            tracing::warn!("did_change: eager tsx sync failed: {e}");
                        }
                    }
                }
            }
        }
    }

    tracing::info!("did_change EXIT v{version}");
    // No diagnostics published during typing — old push diagnostics stay visible
    // and VS Code adjusts their positions as the document changes (line insertions etc.).
    // The SyncCoordinator publishes fresh merged diagnostics after 300ms of silence.
}

pub(super) async fn handle_did_close(
    server: &VerterLanguageServer,
    params: DidCloseTextDocumentParams,
) {
    let _hg = HandlerGuard::new("did_close");
    let uri = &params.text_document.uri;
    tracing::info!("did_close: {}", uri.as_str());
    // Virtual files don't have TSX in the provider
    if server.documents.get_virtual_source_uri(uri).is_none()
        && server.project_sync.is_some()
        && server.documents.get_ide(uri).is_some()
    {
        let Some(canonical_id) = server.documents.get_canonical_id(uri) else {
            server.documents.did_close(uri);
            server.cached_verter_diags.remove(uri.as_str());
            return;
        };
        let state = server
            .provider_sync_state_for_source(&canonical_id)
            .or_else(|| {
                server.documents.get_ide(uri).and_then(|ide| {
                    server
                        .prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                        .map(|transition| transition.next)
                })
            });
        let is_tsgo = matches!(server.type_provider_kind, crate::TypeProviderKind::Tsgo);

        if let Some(state) = state {
            if is_tsgo {
                // TSGO: always close IDE (.vue.tsx) — it was only opened for
                // internal type checking of this file. DTS stays alive for imports.
                if let Some(path) = state.ide_path.as_ref() {
                    server
                        .close_provider_paths(&[(ProviderPathKind::Ide, path.clone())])
                        .await;
                }
            } else if state.ide_background_loaded {
                // tsserver: keep background-synced TSX alive for cross-file resolution.
                tracing::debug!(
                    "did_close: keeping background-synced file in provider: {}",
                    state.ide_path.as_deref().unwrap_or("<missing>")
                );
            } else {
                // tsserver: close TSX and DTS for non-background files.
                server.close_provider_state(&state).await;
                server.remove_provider_sync_state(&canonical_id);
            }
        }
    }
    // Capture canonical_id before did_close clears document state.
    let canonical_id = server.documents.get_canonical_id(uri);

    // Clear the VFS overlay FIRST so the workspace falls back to disk.
    // This must happen before scheduler.close_file() because close_file
    // enqueues a background Source reload that reads via WorkspaceSourceLoader.
    server.documents.did_close(uri);
    server.cached_verter_diags.remove(uri.as_str());

    // Evict the host's FileEntry so ensure_loaded / get_source don't
    // serve stale editor-buffer content. Then tell the scheduler to reload
    // from disk.
    if let Some(ref canonical_id) = canonical_id {
        server.documents.host().evict(canonical_id);
        server.documents.host().scheduler().close_file(canonical_id);
    }
}

pub(super) async fn handle_did_save(
    _server: &VerterLanguageServer,
    _params: DidSaveTextDocumentParams,
) {
    // No-op; document content is already tracked via did_change
}

pub(super) async fn handle_did_change_workspace_folders(
    server: &VerterLanguageServer,
    params: DidChangeWorkspaceFoldersParams,
) {
    let _hg = HandlerGuard::new("did_change_workspace_folders");
    let event = &params.event;

    // Update workspace_roots (quick, non-blocking)
    {
        let mut roots = server.workspace_roots.lock().await;
        for removed in &event.removed {
            let uri_str = removed.uri.as_str().to_string();
            roots.retain(|r| r != &uri_str);
            tracing::info!("workspace folder removed: {}", uri_str);
        }
        for added in &event.added {
            let uri_str = added.uri.as_str().to_string();
            if !roots.contains(&uri_str) {
                tracing::info!("workspace folder added: {}", uri_str);
                roots.push(uri_str);
            }
        }
    }

    // Forward to type provider immediately (async, non-blocking)
    if let Some(tp) = &server.type_provider {
        let added: Vec<serde_json::Value> = event
            .added
            .iter()
            .map(|f| {
                serde_json::json!({
                    "uri": f.uri.as_str(),
                    "name": f.name
                })
            })
            .collect();
        let removed: Vec<serde_json::Value> = event
            .removed
            .iter()
            .map(|f| {
                serde_json::json!({
                    "uri": f.uri.as_str(),
                    "name": f.name
                })
            })
            .collect();
        let _ = tp.update_workspace_folders(added, removed).await;
    }

    // Spawn background task for the blocking work (registry rebuild + scanner)
    server
        .spawn_background_init(None, "workspace folder rebuild")
        .await;
}

pub(super) async fn handle_did_change_watched_files(
    server: &VerterLanguageServer,
    params: DidChangeWatchedFilesParams,
) {
    let _hg = HandlerGuard::new("did_change_watched_files");

    let mut ts_js_resync_ids = Vec::new();
    let mut ts_js_delete_ids = Vec::new();
    let mut vue_resync_ids = Vec::new();
    let mut vue_delete_ids: Vec<(String, String)> = Vec::new(); // (canonical_id, uri_str)
    let mut config_changed = false;

    for event in &params.changes {
        let canonical_id = uri_to_canonical_id(&event.uri);

        // Skip files that are currently open in the editor — the editor's
        // didChange notification is authoritative for open files.
        if server.documents.get(&event.uri).is_some() {
            continue;
        }

        // Skip watcher events for Verter-generated @verter/types stubs.
        // Real installed @verter/types packages (no marker) pass through normally.
        if is_generated_verter_types_event(&canonical_id) {
            continue;
        }

        if is_config_file(&canonical_id) {
            config_changed = true;
            tracing::debug!("did_change_watched_files: config file changed: {canonical_id}");
            // Config files also trigger vite dep check below, but the
            // registry rebuild is the primary action.
        } else if is_vue_file(&canonical_id) {
            if event.typ == FileChangeType::DELETED {
                vue_delete_ids.push((canonical_id, event.uri.as_str().to_string()));
            } else {
                vue_resync_ids.push(canonical_id);
            }
        } else {
            // TS/JS source file
            if event.typ == FileChangeType::DELETED {
                ts_js_delete_ids.push(canonical_id);
            } else {
                ts_js_resync_ids.push(canonical_id);
            }
        }
    }

    // ── Vue file deletions ─────────────────────────────────────
    for (canonical_id, uri_str) in &vue_delete_ids {
        server
            .documents
            .host()
            .invalidate_dependents_of(canonical_id);
        if let Some(state) = server.remove_provider_sync_state(canonical_id).or_else(|| {
            let profile = server.documents.tsx_profile.read().clone();
            server
                .documents
                .host()
                .get_ide(canonical_id, &profile)
                .and_then(|ide| {
                    server
                        .prepare_vue_provider_sync_transition(canonical_id, ide.is_jsx)
                        .map(|transition| transition.next)
                })
        }) {
            server.close_provider_state(&state).await;
        }
        server.documents.host().remove(canonical_id);
        server.cached_verter_diags.remove(uri_str.as_str());
        tracing::debug!("did_change_watched_files: removed vue {canonical_id}");
    }

    // ── Vue file creates/changes ───────────────────────────────
    for canonical_id in &vue_resync_ids {
        server
            .documents
            .host()
            .invalidate_dependents_of(canonical_id);
        server.resync_background_vue_file(canonical_id).await;
        tracing::debug!("did_change_watched_files: resynced vue {canonical_id}");
    }

    // ── TS/JS file deletions ───────────────────────────────────
    for canonical_id in &ts_js_delete_ids {
        server
            .documents
            .host()
            .invalidate_dependents_of(canonical_id);
        if let Some(state) = server.remove_provider_sync_state(canonical_id) {
            server.close_provider_state(&state).await;
        }
        server.documents.host().remove(canonical_id);
        tracing::debug!("did_change_watched_files: removed {canonical_id}");
    }

    // ── TS/JS file creates/changes ─────────────────────────────
    if !ts_js_resync_ids.is_empty() {
        for canonical_id in &ts_js_resync_ids {
            server
                .documents
                .host()
                .invalidate_dependents_of(canonical_id);
        }
        if let Some(sync) = &server.project_sync {
            let host = server.documents.host_arc();
            let sync = sync.clone();
            let vfs_workspace = Arc::clone(&server.vfs_workspace);
            let provider_sync_states = Arc::clone(&server.provider_sync_states);
            let is_tsgo = matches!(server.type_provider_kind, crate::TypeProviderKind::Tsgo);

            tokio::spawn(async move {
                for canonical_id in ts_js_resync_ids {
                    crate::workspace_scanner::resync_non_vue_file(
                        &canonical_id,
                        &host,
                        &sync,
                        &vfs_workspace,
                        is_tsgo,
                        &provider_sync_states,
                    )
                    .await;
                    tracing::debug!("did_change_watched_files: resynced {canonical_id}");
                }
            });
        }
    }

    // ── Config file changes → registry rebuild ─────────────────
    // Also check whether any changed file is a vite config dependency
    // (mirrors the logic in on_file_changed).
    if !config_changed {
        let all_changed: Vec<String> = params
            .changes
            .iter()
            .map(|e| uri_to_canonical_id(&e.uri))
            .collect();
        let ws = server.vfs_workspace.read();
        if let Some(published) = ws.as_ref().and_then(|ws| ws.load_published()) {
            if let Some(views) = published.ext::<crate::workspace_state::LspViews>() {
                for canonical_id in &all_changed {
                    if views
                        .project_views
                        .iter()
                        .any(|v| v.vite_config_deps.iter().any(|dep| dep == canonical_id))
                    {
                        config_changed = true;
                        tracing::debug!(
                            "did_change_watched_files: vite config dep changed: {canonical_id}"
                        );
                        break;
                    }
                }
            }
        }
    }
    if config_changed {
        server.trigger_registry_rebuild().await;
    }
}

pub(super) async fn handle_did_create_files(
    server: &VerterLanguageServer,
    params: CreateFilesParams,
) {
    let _hg = HandlerGuard::new("did_create_files");
    for file in &params.files {
        // Only index .vue files
        if !file.uri.ends_with(".vue") {
            continue;
        }
        let uri: Uri = match file.uri.parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let canonical_id = uri_to_canonical_id(&uri);
        // Load the file through ingress so it's indexed without needing to open in editor
        server.documents.host().ensure_loaded(&canonical_id);
        // Compile and sync to type provider for cross-file type resolution
        server.resync_background_vue_file(&canonical_id).await;
        tracing::debug!("did_create_files: indexed {}", file.uri);
    }
}

pub(super) async fn handle_did_delete_files(
    server: &VerterLanguageServer,
    params: DeleteFilesParams,
) {
    let _hg = HandlerGuard::new("did_delete_files");
    for file in &params.files {
        if !file.uri.ends_with(".vue") {
            continue;
        }
        let uri: Uri = match file.uri.parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let canonical_id = uri_to_canonical_id(&uri);
        if let Some(state) = server
            .remove_provider_sync_state(&canonical_id)
            .or_else(|| {
                let profile = server.documents.tsx_profile.read().clone();
                server
                    .documents
                    .host()
                    .get_ide(&canonical_id, &profile)
                    .and_then(|ide| {
                        server
                            .prepare_vue_provider_sync_transition(&canonical_id, ide.is_jsx)
                            .map(|transition| transition.next)
                    })
            })
        {
            server.close_provider_state(&state).await;
        }
        server.documents.host().remove(&canonical_id);
        server.cached_verter_diags.remove(uri.as_str());
        tracing::debug!("did_delete_files: removed {}", file.uri);
    }
}
