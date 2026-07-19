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

use crate::capabilities::{adapter_module_watch_glob, carrier_watch_glob, server_capabilities};
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
        match server.type_provider_kind {
            crate::TypeProviderKind::EditorTsserver => "connected through editor plugin",
            _ if server.type_provider.is_some() => "connected",
            _ => "NONE — no TypeScript intellisense",
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

    // Honest completion-resolve capability: advertise resolve only when the
    // active provider actually implements it.
    let resolve_provider = server
        .type_provider
        .as_ref()
        .is_some_and(|tp| tp.supports_completion_resolve());

    Ok(InitializeResult {
        capabilities: server_capabilities(&encoding, resolve_provider),
        server_info: Some(ServerInfo {
            name: "verter-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        offset_encoding: Some(encoding.as_str().to_owned()),
    })
}

/// Compute the structured provider recommendation for the active serving route.
///
/// tsgo-preferred model: any tsserver-family serving (managed workspace
/// tsserver or the editor-owned tsserver plugin) recommends TSGO. TSGO-family
/// serving and verter-only mode carry NO recommendation — the server never
/// nags users already on the preferred provider, and a no-provider session
/// gets the dedicated degraded-mode warning instead.
///
/// Content rules: portable facts only (no editor-product names, no client
/// settings keys — clients render remediation in their own idiom), and every
/// `known_gaps` entry must be a tree-evidenced real gap of the recommended
/// provider, never marketing over evidence.
pub(super) fn provider_recommendation(
    kind: &crate::TypeProviderKind,
) -> Option<ProviderRecommendation> {
    // Route wording must stay accurate for EVERY serving arrangement of the
    // kind: `Tsserver` covers both the managed workspace tsserver and the
    // extension-hosted TypeScript language service (Experiment E), so it
    // names the family, not a specific install.
    let route = match kind {
        crate::TypeProviderKind::Tsserver => "a tsserver-family TypeScript service",
        crate::TypeProviderKind::EditorTsserver => "the editor-owned tsserver plugin",
        crate::TypeProviderKind::Tsgo | crate::TypeProviderKind::None => return None,
    };
    Some(ProviderRecommendation {
        preferred: "tsgo".into(),
        reason: format!(
            "This workspace is served by {route}. TSGO (the native TypeScript \
             engine) is Verter's recommended type provider."
        ),
        known_gaps: vec![
            "TSGO does not yet provide the 'remove unused declaration' quick fix \
             (TS6133); other quick fixes are unaffected."
                .into(),
        ],
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
    // The structured `recommendation` carries the tsgo-preferred flip:
    // tsserver-family serving recommends TSGO with honest known gaps; the
    // client owns presentation (dismissal, settings gate, notification UI).
    {
        let kind = server.type_provider_kind.to_string().to_lowercase();
        let reason = server.type_provider_reason.clone();
        server
            .client
            .send_notification::<TypeProviderStatus>(TypeProviderStatusParams {
                kind,
                reason: reason.clone(),
                recommendation: provider_recommendation(&server.type_provider_kind),
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

    // Notify extension of MCP HTTP port (dynamic, OS-assigned).
    if let Some(port) = server.mcp_port {
        server
            .client
            .send_notification::<McpReady>(McpReadyParams { port })
            .await;
        tracing::info!("Sent $/verter/mcpReady with port {port}");
    }

    // Emit the resolved per-workspace carrier-store dir so the extension can hand
    // it to VS Code's OWN TypeScript server (via `configurePlugin`); a plain `.ts`
    // opened there then reads the SAME store and resolves imported `.vue`/`.svelte`
    // carriers. The LSP is the single source of the
    // `<temp>/verter-carrier-store/<host-version>/<workspace-hash>/` derivation —
    // it cannot be reproduced extension-side without mirroring the exact recipe. The
    // dir is keyed on the primary workspace root (the one the carrier-publish path
    // and the spawned tsserver share).
    {
        let first_root = server.workspace_roots.lock().await.first().cloned();
        if let Some(root_uri) = first_root {
            let workspace_root = crate::documents::uri_to_canonical_id_from_str(&root_uri);
            let carrier_store_dir =
                crate::external_ts::default_carrier_store_dir_string(&workspace_root);
            server
                .client
                .send_notification::<CarrierStoreReady>(CarrierStoreReadyParams {
                    carrier_store_dir: carrier_store_dir.clone(),
                })
                .await;
            tracing::info!("Sent $/verter/carrierStoreReady with dir {carrier_store_dir}");
        }
    }

    // Eagerly populate type provider workspace roots so that
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
    let mut watchers = vec![FileSystemWatcher {
        // Carrier-file watcher glob, built from the registry's carrier
        // rows. File-watching is a SERVER concern (the client manifest
        // carries no watch globs); this glob is the descriptor-derived
        // authority. It covers carrier extensions today (`.vue`,
        // `.svelte`) — including carrier rows with no registered
        // implementation, whose events produce no provider sync state
        // until a carrier lands.
        glob_pattern: GlobPattern::String(carrier_watch_glob()),
        kind: watch_kind,
    }];
    // Dedicated ADAPTER-MODULE watcher glob (`**/*.{svelte.js,svelte.ts}`),
    // built from `LanguageRegistry::all_adapter_module_extensions()`. A rune
    // module is NOT a carrier and its coverage is its OWN descriptor-derived
    // glob — the generic TS/JS glob below no longer carries rune-module
    // responsibility.
    if let Some(adapter_module_glob) = adapter_module_watch_glob() {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(adapter_module_glob),
            kind: watch_kind,
        });
    }
    watchers.extend([
        FileSystemWatcher {
            // Generic TS/JS glob for ORDINARY TS/JS dependency + config
            // tracking ONLY (rune-module coverage moved to the dedicated
            // adapter-module glob above).
            glob_pattern: GlobPattern::String("**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}".to_string()),
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
            glob_pattern: GlobPattern::String("**/vite.config.{ts,js,mjs,cjs,mts,cts}".to_string()),
            kind: watch_kind,
        },
        FileSystemWatcher {
            glob_pattern: GlobPattern::String("**/package.json".to_string()),
            kind: watch_kind,
        },
    ]);
    let _ = server
        .client
        .register_capability(vec![Registration {
            id: "verter-file-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers,
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
    let is_virtual = uri.as_str().starts_with("verter-virtual://");
    // Registry membership and its open-generation token change under the same
    // lane close holds. Virtual editor projections never participate in carrier
    // repair generations; their URI spelling can legitimately contain encoded
    // source identities and must not create raw/decoded duplicate lane keys.
    let (result, current_canonical_id) = if is_virtual {
        let document_commit_guard = server.did_change_mutex.lock().await;
        let result = server.documents.did_open(&params.text_document);
        let canonical_id = server.documents.get_canonical_id(uri);
        drop(document_commit_guard);
        (result, canonical_id)
    } else {
        let canonical_hint = uri_to_canonical_id(uri);
        let lease = server.ide_sync_lifecycle_lease(&canonical_hint);
        let guard = lease.lock().await;
        let document_commit_guard = server.did_change_mutex.lock().await;
        let result = server.documents.did_open(&params.text_document);
        let canonical_id = server.documents.get_canonical_id(uri);
        if let Some(canonical_id) = canonical_id.as_ref() {
            server.begin_ide_sync_open_generation(canonical_id, lease.lane());
        }
        drop(document_commit_guard);
        drop(guard);
        drop(lease);
        (result, canonical_id)
    };
    if is_virtual {
        tracing::info!("did_open EXIT (virtual): {}", uri.as_str());
        return;
    }
    // Touch MRU for snapshot drain ordering (after did_open registers the canonical ID)
    if let Some(canonical_id) = current_canonical_id.as_ref() {
        server.touch_mru(canonical_id);
        if carrier_language_for(canonical_id).is_some() {
            server.refresh_carrier_dependency_tracking(canonical_id);
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
    let prewarm_imported_carrier_apis = startup_policy.sync_imported_carrier_apis
        && matches!(
            server.type_provider_kind,
            crate::TypeProviderKind::Tsserver | crate::TypeProviderKind::EditorTsserver
        )
        // TEST SEAM: suppressed so the cross-file-rename child-closed lane proves
        // `handle_rename`'s own sync-before-query is the sole sync of the child API.
        && !server.suppress_imported_carrier_prewarm;
    let imported_carrier_priority_ids = server
        .documents
        .get_analysis(uri)
        .map(|analysis| {
            // Primary: analysis.imports already has resolved_canonical_id from host
            // (works even before background_init builds the resolver snapshot)
            let mut ids = collect_imported_carrier_priority_ids_from_imports_with_fallback(
                &analysis.imports,
                current_canonical_id.as_deref(),
                |parent, specifier| server.resolve_import_specifier(parent, specifier),
            );

            // Supplement: module_references for dynamic import()/require() cases
            // that aren't in analysis.imports (needs resolver, may return empty pre-init)
            if let Some(canonical_id) = current_canonical_id.as_ref() {
                let snapshot = server.published_resolver();
                let reader = LspProjectResolverReader::new(&server.documents);
                let dynamic_ids =
                    collect_priority_carrier_public_api_targets_from_module_references(
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
        for import_id in &imported_carrier_priority_ids {
            scanner.signal_priority(import_id.clone());
        }
    }

    if prewarm_imported_carrier_apis {
        for import_id in &imported_carrier_priority_ids {
            server
                .sync_imported_carrier_api_lightweight(import_id)
                .await;
        }
    }

    // Active file IDE sync FIRST (Interactive priority) — enables typed hover immediately.
    // tsserver is the exception: imported carrier public APIs are warmed above so the
    // initial open does not snapshot missing carrier provider modules into the
    // configured project.
    let provider_sync_policy = did_open_provider_sync_policy(server.type_provider_kind);
    if provider_sync_policy.await_ide_sync {
        // Use ensure_current_file_synced for immediate IDE-only sync
        server.ensure_current_file_synced(uri).await;
    }

    // A self-file document (a `.svelte.ts` / `.svelte.js` rune module OR a
    // plain TS-family script) is NOT a carrier — it serves its OWN-path
    // provider buffer. Sync it as UNRESOLVED open-document shadow state so
    // its own buffer is queryable before resolver ownership is ready (a no-op
    // for any carrier or unknown-extension document).
    if current_canonical_id
        .as_deref()
        .and_then(self_file_language_for)
        .is_some()
    {
        server.sync_self_file_shadow_unresolved(uri).await;
    }

    // Imported carrier API warmup SECOND (Normal priority, never blocks active file).
    // TEST SEAM: also gated off by `suppress_imported_carrier_prewarm` — the cross-
    // file-rename discrimination lane must leave `handle_rename`'s own
    // sync-before-query as the SOLE sync of an imported child's API surface, so
    // BOTH the eager (above) and this deferred did_open warmup are suppressed.
    if startup_policy.sync_imported_carrier_apis
        && !prewarm_imported_carrier_apis
        && !server.suppress_imported_carrier_prewarm
    {
        for import_id in &imported_carrier_priority_ids {
            let should_sync =
                !server.is_background_loaded_for_source_kind(import_id, ProviderPathKind::Api);
            if should_sync {
                server
                    .sync_imported_carrier_api_lightweight(import_id)
                    .await;
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
    if let Some(canonical_id) = current_canonical_id.as_ref() {
        server.needs_ide_sync.insert(canonical_id.clone());
        server.needs_deferred_sync.insert(canonical_id.clone());
        server
            .sync_coordinator
            .signal(canonical_id.clone(), uri.as_str().to_string());
    }

    if startup_policy.publish_diagnostics {
        server.publish_full_diagnostics_with_audit(uri).await;
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

    let is_virtual = server.documents.get_virtual_source_uri(&uri).is_some();

    // CRITICAL: Serialize the synchronous document commit/upsert via a
    // tokio::sync::Mutex.
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
    let document_commit_guard = server.did_change_mutex.lock().await;
    tracing::info!(
        "did_change MUTEX_ACQUIRED v{version} wait={:?} thread={:?}",
        mutex_wait_start.elapsed(),
        std::thread::current().id()
    );
    tracing::info!("did_change MUTEX_ACQUIRED v{version}");

    let _timer = server
        .statistics
        .timer("did_change", Some(uri.as_str().to_string()));

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
    // Assign provider turns in the same total order as committed document
    // versions, but do not wait for the predecessor while holding the commit
    // fence. Later edits can therefore commit and serve native features even
    // when an earlier provider update is suspended.
    let provider_update_turn = (!is_virtual).then(|| server.enqueue_did_change_provider_update());
    tracing::info!(
        "did_change UPSERT_DONE v{version} elapsed={:?} thread={:?}",
        upsert_start.elapsed(),
        std::thread::current().id()
    );
    drop(document_commit_guard);

    // Virtual files don't need TSX sync or diagnostics.
    if is_virtual {
        tracing::info!("did_change EXIT (virtual) v{version}");
        return;
    }

    let provider_update_turn =
        provider_update_turn.expect("non-virtual did_change has a provider update turn");
    provider_update_turn.wait().await;

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

            if carrier_language_for(&canonical_id).is_some() {
                server.refresh_carrier_dependency_tracking(&canonical_id);
            }
            server.needs_ide_sync.insert(canonical_id.clone());
            server.needs_deferred_sync.insert(canonical_id.clone());
            server
                .sync_coordinator
                .signal(canonical_id.clone(), uri.as_str().to_string());

            // Eager carrier refresh — make the freshly-edited carrier content
            // visible to the type provider immediately, so the next interactive
            // request (completion, hover, definition) sees it without per-handler
            // inline sync.
            //
            // tsserver: the carrier is a configured-project MEMBER served from the
            // publish store, so a fresh edit must RE-PUBLISH the companions (and
            // fire the change notification) — NOT open the synthetic TSX as a
            // second content authority (the eager `sync_tsx` is a no-op for
            // tsserver). The publish is fail-closed (a no-owner carrier publishes
            // nothing). tsgo keeps the eager `sync_tsx` content open.
            if matches!(
                server.type_provider_kind,
                crate::TypeProviderKind::Tsserver | crate::TypeProviderKind::EditorTsserver
            ) {
                if carrier_language_for(&canonical_id).is_some() {
                    server.publish_carrier_to_external_ts(&canonical_id).await;
                }
            } else if let Some(sync) = &server.project_sync {
                if let Some(ide) = server.documents.get_ide(&uri) {
                    if let Some(ide_path) = server.eager_syncable_ide_path_for_uri(&uri) {
                        if let Err(e) = sync.sync_tsx(&ide_path, &ide.code).await {
                            tracing::warn!("did_change: eager tsx sync failed: {e}");
                        } else {
                            // Record a fresh generation pinning the EXACT IDE bytes
                            // just synced (interactive queries capture this surface).
                            server.record_carrier_ide_snapshot(
                                &canonical_id,
                                &ide_path,
                                &ide.code,
                                ide.source_map.as_deref(),
                            );
                        }
                    }
                }
            }

            // A self-file document (rune module or plain TS-family script) is
            // NOT a carrier — the eager TSX path above never fires for it, and
            // the coordinator's diagnostics route through carrier IDE state.
            // Re-sync its OWN-path provider buffer here so an editor edit
            // refreshes the provider content AND refines the rewrite-aware
            // projection (keeping `provider_projection_context`'s content and
            // mapper consistent — no stale own-buffer content, no lost rewrite
            // columns).
            if self_file_language_for(&canonical_id).is_some() {
                server.sync_self_file_shadow_unresolved(&uri).await;
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

    // Serialize the complete close against foreground IDE-sync repair and bind
    // it to the exact open generation observed at entry. A concurrent reopen
    // advances the generation under this same lifecycle lane, so this close can
    // never remove or retire the reopened document (canonical-key ABA).
    let is_virtual = server.documents.get_virtual_source_uri(uri).is_some();
    let close_canonical_id = (!is_virtual)
        .then(|| server.documents.get_canonical_id(uri))
        .flatten();
    let close_generation = close_canonical_id.as_ref().and_then(|canonical_id| {
        server.current_or_init_ide_sync_open_generation(uri, canonical_id)
    });
    let close_repair_lease = close_canonical_id
        .as_ref()
        .zip(close_generation)
        .map(|(canonical_id, generation)| server.ide_sync_repair_lease(canonical_id, generation));
    let _close_repair_guard = match close_repair_lease.as_ref() {
        Some(lease) => Some(lease.lock().await),
        None => None,
    };
    #[cfg(test)]
    if let Some(canonical_id) = close_canonical_id.as_ref() {
        server
            .maybe_pause_ide_sync_close_after_lock(canonical_id)
            .await;
    }
    if let (Some(canonical_id), Some(generation)) = (close_canonical_id.as_ref(), close_generation)
    {
        if !server.ide_sync_generation_is_open(uri, canonical_id, generation) {
            return;
        }
    }

    // A self-file document (rune module or plain TS-family script) has NO IDE
    // TSX — the carrier-oriented branch below (gated on `get_ide(...).is_some()`)
    // never fires for it. Close + remove its OWN-path Shadow provider state
    // explicitly so the open-document buffer does not linger in the provider.
    if server.documents.get_virtual_source_uri(uri).is_none() && server.project_sync.is_some() {
        if let Some(canonical_id) = server.documents.get_canonical_id(uri) {
            if self_file_language_for(&canonical_id).is_some() {
                server.clear_provider_sync_state(&canonical_id).await;
            }
        }
    }

    // Virtual files don't have TSX in the provider
    if server.documents.get_virtual_source_uri(uri).is_none()
        && server.project_sync.is_some()
        && server.documents.get_ide(uri).is_some()
    {
        let Some(canonical_id) = server.documents.get_canonical_id(uri) else {
            let document_commit_guard = server.did_change_mutex.lock().await;
            if let (Some(canonical_id), Some(generation)) =
                (close_canonical_id.as_ref(), close_generation)
            {
                server.close_ide_sync_open_generation(canonical_id, generation);
                if let Some(lease) = close_repair_lease.as_ref() {
                    lease.retire();
                }
            }
            server.documents.did_close(uri);
            drop(document_commit_guard);
            server.cached_verter_diags.remove(uri.as_str());
            return;
        };
        let state = server
            .provider_sync_state_for_source(&canonical_id)
            .or_else(|| {
                server
                    .documents
                    .get_ide(uri)
                    .and_then(|ide| server.carrier_close_state(&canonical_id, ide.is_jsx))
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
    // Capture canonical_id before did_close clears document state — every step
    // below that needs the closed file's identity (the overlay release, the host
    // evict, the scheduler close) reads it from here, not from `documents`.
    let canonical_id = close_canonical_id;

    // Clear document state FIRST, before releasing the proactive
    // declaration-overlay graph below: `documents.did_close` removes this root
    // from `documents.open_uris()`, the live open-root set the background
    // closure pass reads. Doing it first means a closure pass racing this close
    // observes the root as CLOSED — so when it reconciles the overlay refcount it
    // DROPS this root's re-recorded edges instead of keeping them. If the release
    // ran first, a pass landing in the gap would re-record the just-released
    // edges while the root was still in the open set, and the final reconcile
    // would KEEP them with no future close event — a permanent overlay leak.
    //
    // (Also keeps the required ordering vs `scheduler.close_file()` below: the
    // VFS overlay must clear before `close_file` enqueues a background Source
    // reload that reads via WorkspaceSourceLoader.)
    {
        let document_commit_guard = server.did_change_mutex.lock().await;
        if let (Some(canonical_id), Some(generation)) = (canonical_id.as_ref(), close_generation) {
            server.close_ide_sync_open_generation(canonical_id, generation);
        }
        server.documents.did_close(uri);
        drop(document_commit_guard);
    }

    // Release this root from the proactive declaration-overlay graph: any
    // `.d.<ext>.ts` overlay its closure opened that NO other open root still
    // reaches is closed now. An overlay still reachable from another open root
    // is retained (closing it would strand that root's bare carrier imports).
    // A no-op for tsserver (the refcount is only populated on the tsgo closure
    // pass) and when the closed file was never a carrier root. Runs AFTER
    // `did_close` so the closure pass already sees the root as closed (above).
    if server.project_sync.is_some() {
        if let Some(ref canonical_id) = canonical_id {
            server
                .release_declaration_overlays_for_closed_root(canonical_id)
                .await;
        }
    }

    server.cached_verter_diags.remove(uri.as_str());

    // Evict the host's FileEntry so ensure_loaded / get_source don't
    // serve stale editor-buffer content. Then tell the scheduler to reload
    // from disk.
    if let Some(ref canonical_id) = canonical_id {
        server.documents.host().evict(canonical_id);
        server.documents.host().scheduler().close_file(canonical_id);
    }

    // Mark this exact lane object retired while the close still owns it. The final
    // active/waiting repair lease removes the map entry synchronously on drop;
    // an equal canonical key opened in a newer generation owns a different/live
    // lane and cannot be removed by this lease's exact-pointer drop.
    if let Some(lease) = close_repair_lease.as_ref() {
        lease.retire();
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
    let mut carrier_resync_ids = Vec::new();
    let mut carrier_delete_ids: Vec<(String, String)> = Vec::new(); // (canonical_id, uri_str)
    let mut adapter_module_resync_ids = Vec::new();
    let mut adapter_module_delete_ids = Vec::new();
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
        } else if carrier_language_for(&canonical_id).is_some() {
            // Any framework CARRIER (`.vue`, `.svelte`, …) routes through the
            // shared resync/delete queues. The downstream compile + provider
            // sync is carrier-generic: a carrier-less language's upserts fail
            // with the typed unsupported-language error before any snapshot
            // publishes, so an event for it schedules no resync, queues no
            // provider sync, and touches no provider paths.
            if event.typ == FileChangeType::DELETED {
                carrier_delete_ids.push((canonical_id, event.uri.as_str().to_string()));
            } else {
                carrier_resync_ids.push(canonical_id);
            }
        } else if adapter_module_language_for(&canonical_id).is_some() {
            // A standalone ADAPTER MODULE (`.svelte.ts` / `.svelte.js` rune
            // module) — classified EXPLICITLY (descriptor-derived) rather than
            // falling through the incidental generic-TS `else` arm. A rune
            // module is NOT a carrier; its background resync reuses the non-
            // carrier resync impl (its provider buffer is the own-path rune-
            // module content).
            if event.typ == FileChangeType::DELETED {
                adapter_module_delete_ids.push(canonical_id);
            } else {
                adapter_module_resync_ids.push(canonical_id);
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

    // ── Carrier file deletions ─────────────────────────────────
    //
    // R3: eager dependent invalidation is forbidden. Cache entries
    // are validated on read against the exact facts they recorded;
    // staleness is detected lazily. Workspace edge updates fire via
    // `host().remove(...)` below so the reverse-dep graph stays
    // current for memory-bound GC and affected-files reporting.
    for (canonical_id, uri_str) in &carrier_delete_ids {
        if let Some(state) = server.remove_provider_sync_state(canonical_id).or_else(|| {
            let profile = server.documents.tsx_profile.read().clone();
            server
                .documents
                .host()
                .get_ide(canonical_id, &profile)
                .and_then(|ide| server.carrier_close_state(canonical_id, ide.is_jsx))
        }) {
            server.close_provider_state(&state).await;
        }
        // Drop the carrier's STORE membership too — the provider-buffer close above
        // closes the open companion; this retracts it from `getExternalFiles` so the
        // deleted carrier is no longer advertised to the plugin. Best-effort on the
        // delete path, but a failure is surfaced.
        if let Err(error) = server.retract_carrier_from_external_ts(canonical_id).await {
            tracing::warn!(
                "did_change_watched_files: carrier retract failed for {canonical_id}: {error}"
            );
        }
        server.documents.host().remove(canonical_id);
        server.cached_verter_diags.remove(uri_str.as_str());
        tracing::debug!("did_change_watched_files: removed carrier {canonical_id}");
    }

    // ── Carrier file creates/changes ───────────────────────────
    for canonical_id in &carrier_resync_ids {
        server.resync_background_carrier_file(canonical_id).await;
        tracing::debug!("did_change_watched_files: resynced carrier {canonical_id}");
    }

    // ── TS/JS + adapter-module file deletions ──────────────────
    // A rune module's provider state lives on its OWN canonical path (the
    // Shadow path), so removal is the same remove-state + host-evict the
    // non-carrier path uses.
    for canonical_id in ts_js_delete_ids
        .iter()
        .chain(adapter_module_delete_ids.iter())
    {
        if let Some(state) = server.remove_provider_sync_state(canonical_id) {
            server.close_provider_state(&state).await;
        }
        server.documents.host().remove(canonical_id);
        tracing::debug!("did_change_watched_files: removed {canonical_id}");
    }

    // ── TS/JS + adapter-module file creates/changes ────────────
    // Adapter modules (rune modules) reuse the non-carrier resync impl: their
    // provider buffer is the own-path rune-module content (prelude + rewritten
    // bytes), produced by the same `prepare_non_carrier_provider_sync` path.
    let non_carrier_resync_ids: Vec<String> = ts_js_resync_ids
        .into_iter()
        .chain(adapter_module_resync_ids)
        .collect();
    if !non_carrier_resync_ids.is_empty() {
        if let Some(sync) = &server.project_sync {
            let host = server.documents.host_arc();
            let sync = sync.clone();
            let vfs_workspace = Arc::clone(&server.vfs_workspace);
            let provider_sync_states = Arc::clone(&server.provider_sync_states);
            let provider_surfaces = server.documents.provider_surfaces().clone();
            let is_tsgo = matches!(server.type_provider_kind, crate::TypeProviderKind::Tsgo);

            tokio::spawn(async move {
                for canonical_id in non_carrier_resync_ids {
                    crate::workspace_scanner::resync_non_carrier_file(
                        &canonical_id,
                        &host,
                        &sync,
                        &provider_surfaces,
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
        // Only index framework CARRIER files (`.vue`, `.svelte`, …).
        if carrier_language_for(&file.uri).is_none() {
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
        server.resync_background_carrier_file(&canonical_id).await;
        tracing::debug!("did_create_files: indexed {}", file.uri);
    }
}

pub(super) async fn handle_did_delete_files(
    server: &VerterLanguageServer,
    params: DeleteFilesParams,
) {
    let _hg = HandlerGuard::new("did_delete_files");
    for file in &params.files {
        // Only framework CARRIER files (`.vue`, `.svelte`, …).
        if carrier_language_for(&file.uri).is_none() {
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
                    .and_then(|ide| server.carrier_close_state(&canonical_id, ide.is_jsx))
            })
        {
            server.close_provider_state(&state).await;
        }
        // Retract the carrier's STORE membership (the provider buffer was closed
        // above) so the deleted carrier is no longer advertised via
        // `getExternalFiles`. Best-effort on the delete path (the file is gone; the
        // next authoritative publish re-prunes), but a failure is surfaced.
        if let Err(error) = server.retract_carrier_from_external_ts(&canonical_id).await {
            tracing::warn!("did_delete_files: carrier retract failed for {canonical_id}: {error}");
        }
        server.documents.host().remove(&canonical_id);
        server.cached_verter_diags.remove(uri.as_str());
        tracing::debug!("did_delete_files: removed {}", file.uri);
    }
}

#[cfg(test)]
mod provider_recommendation_tests {
    use super::*;
    use crate::TypeProviderKind;

    /// Serving on the workspace-tsserver route recommends the preferred TSGO
    /// provider — the tsgo-preferred flip. The payload is portable facts only:
    /// no editor-specific remediation strings in server-side content.
    #[test]
    fn tsserver_route_recommends_tsgo_with_portable_wording() {
        let rec = provider_recommendation(&TypeProviderKind::Tsserver)
            .expect("tsserver serving must carry a tsgo recommendation");
        assert_eq!(rec.preferred, "tsgo");
        assert!(
            rec.reason.contains("tsserver"),
            "reason names the active route"
        );
        // Editor-agnostic discipline: presentation belongs to the client.
        assert!(
            !rec.reason.contains("VS Code"),
            "no VS-Code-specific strings server-side"
        );
        assert!(
            !rec.reason.contains("verter.typeProvider"),
            "no client settings-key strings server-side"
        );
    }

    /// The one tree-evidenced real TSGO gap (unported TS6133 remove-unused
    /// quick fix) stays honestly disclosed inside the recommendation payload.
    #[test]
    fn recommendation_discloses_the_real_ts6133_gap_honestly() {
        let rec = provider_recommendation(&TypeProviderKind::Tsserver).unwrap();
        assert!(
            !rec.known_gaps.is_empty(),
            "known gaps must not be marketing-empty"
        );
        assert!(
            rec.known_gaps.iter().any(|g| g.contains("TS6133")),
            "the unported remove-unused quick fix must be disclosed: {:?}",
            rec.known_gaps
        );
        // Negative: the two claims of the retired startup warning must NOT
        // reappear as gaps — (a) "barrel re-exported .vue loses typing" and
        // (b) "referenced-tsconfig (composite) path aliases unresolved on
        // hover". Both were disproven by real-provider evidence; the
        // one-time pull-diagnostics path-alias gap was ALSO freshly
        // disproven (`carrier_diagnostics_resolve_path_alias_tsgo` runs
        // un-ignored and green). This negative pins only the RETIRED claim
        // wording: a future tree-evidenced gap that happens to involve path
        // aliases may still be honestly disclosed.
        for gap in &rec.known_gaps {
            assert!(
                !gap.contains("barrel"),
                "stale barrel-typing claim must stay retired"
            );
            let lower = gap.to_lowercase();
            assert!(
                !(lower.contains("path alias")
                    && (lower.contains("referenced tsconfig")
                        || lower.contains("composite")
                        || lower.contains("hover"))),
                "the retired composite-hover path-alias claim must stay retired: {gap}"
            );
        }
    }

    /// The editor-owned tsserver plugin route is tsserver-family serving and
    /// carries the same recommendation.
    #[test]
    fn editor_tsserver_route_recommends_tsgo() {
        let rec = provider_recommendation(&TypeProviderKind::EditorTsserver)
            .expect("editor-tsserver serving must carry a tsgo recommendation");
        assert_eq!(rec.preferred, "tsgo");
    }

    /// TSGO-family serving and verter-only mode carry NO recommendation —
    /// the server never nags users already on the preferred provider, and a
    /// no-provider session already gets the dedicated degraded-mode warning.
    #[test]
    fn tsgo_and_none_routes_carry_no_recommendation() {
        assert!(provider_recommendation(&TypeProviderKind::Tsgo).is_none());
        assert!(provider_recommendation(&TypeProviderKind::None).is_none());
    }

    /// Wire shape: the recommendation serializes camelCase (`knownGaps`) for
    /// the TS client mirror, and an absent recommendation is omitted entirely.
    #[test]
    fn status_params_serialize_recommendation_camel_case_and_omit_when_absent() {
        let with = TypeProviderStatusParams {
            kind: "tsserver".into(),
            reason: None,
            recommendation: provider_recommendation(&TypeProviderKind::Tsserver),
        };
        let json = serde_json::to_value(&with).unwrap();
        let rec = json.get("recommendation").expect("recommendation present");
        assert_eq!(rec.get("preferred").unwrap(), "tsgo");
        assert!(
            rec.get("knownGaps").is_some(),
            "camelCase knownGaps on the wire"
        );
        assert!(
            rec.get("known_gaps").is_none(),
            "snake_case must not leak to the wire"
        );

        let without = TypeProviderStatusParams {
            kind: "tsgo".into(),
            reason: None,
            recommendation: provider_recommendation(&TypeProviderKind::Tsgo),
        };
        let json = serde_json::to_value(&without).unwrap();
        assert!(
            json.get("recommendation").is_none(),
            "absent recommendation omitted from the wire"
        );
    }
}
