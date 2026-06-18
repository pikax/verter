use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::tsgo::ipc::{find_tsgo_binary, TsgoTypeProvider};
use verter_lsp::tsgo::resilient as tsgo_resilient;
use verter_lsp::tsserver::ipc::TsserverTypeProvider;
use verter_lsp::tsserver::resilient as tsserver_resilient;
use verter_lsp::type_provider::traits::TypeProvider;
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

#[tokio::main]
async fn main() {
    // Initialize tracing (controlled via VERTER_LOG or RUST_LOG env var).
    // ANSI colors are disabled because the output goes to VS Code's debug
    // console which renders escape codes as literal text (e.g. `[2m`, `[0m`).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("VERTER_LOG")
                .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(
        "verter-lsp v{} ({}, built {})",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        env!("VERTER_BUILD_DATE"),
    );

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    // Parse CLI arguments
    let args = CliArgs::parse();

    // The LSP binary no longer hosts the MCP HTTP server in-process.
    // `verter_lsp` and `verter_mcp` ship as independent products; IDEs
    // that previously consumed MCP via `--mcp-port` must spawn the
    // standalone `verter_mcp_server` binary in its own process.
    // The `--mcp-port` CLI flag is still parsed so existing IDE
    // configurations remain syntactically valid, but supplying it
    // now triggers a guidance log only.
    let mcp_actual_port: Option<u16> = {
        if args.mcp_port.is_some() {
            tracing::warn!(
                "--mcp-port is no longer honoured by verter-lsp. \
                 The LSP binary no longer embeds the MCP server. \
                 Spawn `verter-mcp-server --transport http --port <port>` \
                 in its own process to expose the HTTP MCP transport."
            );
        }
        None
    };

    // Deferred client cell — populated inside LspService::build, used by
    // ResilientTypeProvider's crash monitor to send user notifications.
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());

    // Provider selection: auto → detect TS version, pick provider
    let (type_provider, provider_kind, suggest_tsgo, type_provider_none_reason) =
        create_type_provider(&args, &client_cell).await;

    let config = LspConfig {
        host,
        type_provider,
        project_sync_mode: ProjectSyncMode::FullProject,
        type_provider_kind: provider_kind,
        suggest_tsgo,
        mcp_port: mcp_actual_port,
        type_provider_none_reason,
    };

    let client_cell_for_build = Arc::clone(&client_cell);
    let (service, socket) = LspService::build(|client| {
        // Populate the deferred client cell so the crash monitor can send notifications.
        let _ = client_cell_for_build.set(client.clone());
        VerterLanguageServer::new(client, config)
    })
    .custom_method(
        "$/onDidChangeTsOrJsFile",
        VerterLanguageServer::on_did_change_ts_or_js_file,
    )
    .custom_method("$/onFileChanged", VerterLanguageServer::on_file_changed)
    .custom_method(
        "$/verter/watcherStateChanged",
        VerterLanguageServer::on_watcher_state_changed,
    )
    .custom_method("$/getCompiledCode", VerterLanguageServer::get_compiled_code)
    .custom_method(
        "$/verter/getStatistics",
        VerterLanguageServer::get_statistics,
    )
    .custom_method(
        "$/verter/getVirtualFiles",
        VerterLanguageServer::get_virtual_files,
    )
    .custom_method("$/verter/getAnalysis", VerterLanguageServer::get_analysis)
    .custom_method(
        "$/verter/getProjectOverview",
        VerterLanguageServer::get_project_overview,
    )
    .custom_method(
        "$/verter/getBindingTypes",
        VerterLanguageServer::get_binding_types,
    )
    .custom_method(
        "$/verter/getComponentParents",
        VerterLanguageServer::get_component_parents,
    )
    .custom_method(
        "$/verter/documentDropEdit",
        VerterLanguageServer::document_drop_edit,
    )
    .custom_method(
        "$/verter/applyStyleOverrides",
        VerterLanguageServer::apply_style_overrides,
    )
    .custom_method(
        "$/verter/getRouteTree",
        VerterLanguageServer::get_route_tree,
    )
    .custom_method(
        "$/verter/getComponentMeta",
        VerterLanguageServer::get_component_meta,
    )
    .custom_method(
        "$/verter/getComponentMetaSurface",
        VerterLanguageServer::get_component_meta_surface,
    )
    .custom_method(
        "$/verter/getComponentMetaTypeExpansion",
        VerterLanguageServer::get_component_meta_type_expansion,
    )
    .custom_method(
        "$/verter/audit/getRecord",
        VerterLanguageServer::get_audit_record,
    )
    .custom_method(
        "$/verter/audit/getRecent",
        VerterLanguageServer::get_audit_recent,
    )
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Parsed CLI arguments.
struct CliArgs {
    /// Type provider mode: "auto", "tsgo", "tsserver", "off".
    type_provider: String,
    /// Path to TypeScript SDK directory (tsserver.js location).
    tsdk: Option<String>,
    /// Path to the directory containing `@verter/typescript-plugin`.
    plugin_path: Option<String>,
    /// Workspace root (positional argument).
    workspace_root: Option<String>,
    /// MCP HTTP port. Parsed for syntactic compatibility with existing
    /// IDE configurations only — the LSP binary no longer embeds the
    /// MCP server. Supplying this flag now logs a notice that points
    /// consumers at `verter-mcp-server`.
    mcp_port: Option<u16>,
}

impl CliArgs {
    fn parse() -> Self {
        let mut type_provider = "auto".to_string();
        let mut tsdk = None;
        let mut plugin_path = None;
        let mut workspace_root = None;
        let mut mcp_port = None;

        for arg in std::env::args().skip(1) {
            if let Some(val) = arg.strip_prefix("--type-provider=") {
                type_provider = val.to_string();
            } else if let Some(val) = arg.strip_prefix("--tsdk=") {
                tsdk = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--plugin-path=") {
                plugin_path = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--mcp-port=") {
                mcp_port = val.parse().ok();
            } else if arg.starts_with("--mcp-lint-preset=") {
                // Accepted for syntactic compatibility with IDE
                // configurations that previously bundled MCP into the
                // LSP binary. Now that MCP ships separately via
                // `verter-mcp-server`, the lint preset is configured
                // on that binary's CLI; the LSP simply ignores the flag.
                continue;
            } else if !arg.starts_with("--") {
                workspace_root = Some(arg);
            }
        }

        Self {
            type_provider,
            tsdk,
            plugin_path,
            workspace_root,
            mcp_port,
        }
    }
}

/// Create the type provider based on CLI args.
///
/// Auto mode: if TypeScript 5.x is installed in node_modules, use tsserver
/// and recommend switching to TSGO. If no TypeScript is found, try TSGO.
///
/// Returns `(provider, kind, suggest_tsgo, none_reason)` where `none_reason`
/// explains why no provider could be started (only set when provider is None).
async fn create_type_provider(
    args: &CliArgs,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> (
    Option<Arc<dyn TypeProvider>>,
    TypeProviderKind,
    bool,
    Option<String>,
) {
    tracing::info!(
        "create_type_provider: type_provider={:?}, tsdk={:?}, workspace_root={:?}",
        args.type_provider,
        args.tsdk,
        args.workspace_root
    );
    let workspace_root = args
        .workspace_root
        .clone()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let ws_canonical = verter_span::path::canonicalize_path(&workspace_root);

    match args.type_provider.as_str() {
        "off" => {
            tracing::info!("type provider: disabled by --type-provider=off");
            (
                None,
                TypeProviderKind::None,
                false,
                Some("Disabled by configuration (--type-provider=off)".into()),
            )
        }
        "tsgo" => {
            // TSGO only — no fallback
            match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Ok(tp) => (Some(tp), TypeProviderKind::Tsgo, false, None),
                Err(reason) => {
                    tracing::warn!("TSGO unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, false, Some(reason))
                }
            }
        }
        "tsserver" => {
            // tsserver only — no fallback
            match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                Ok(tp) => (Some(tp), TypeProviderKind::Tsserver, false, None),
                Err(reason) => {
                    tracing::warn!("tsserver unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, false, Some(reason))
                }
            }
        }
        "extension" => {
            // Experiment E: extension-hosted TypeScript language service.
            // The extension runs ts.createLanguageService() in-process and
            // responds to $/verter/tsQuery requests over the existing LSP pipe.
            tracing::info!("type provider: extension-hosted (Experiment E)");
            let provider = verter_lsp::extension_provider::ExtensionTypeProvider::new(
                Arc::clone(client_cell),
                &ws_canonical,
            );
            (
                Some(Arc::new(provider) as Arc<dyn TypeProvider>),
                TypeProviderKind::Tsserver,
                false,
                None,
            )
        }
        _ => {
            // "auto" (default): if TS 5.x/6.x installed, use tsserver; else try TSGO.
            // Also prefer tsserver when composite tsconfigs are detected (TSGO upstream
            // limitation: cannot resolve path aliases from referenced configs).
            let tsserver_path =
                verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(&ws_canonical));
            let mut tsserver_reason = None;

            let composite_ws = verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            );
            let has_composite =
                verter_lsp::config::has_solution_style_tsconfig(&composite_ws, &ws_canonical);
            if has_composite {
                tracing::info!(
                    "auto mode: solution-style tsconfig detected at {} \
                     (TSGO cannot resolve path aliases from referenced configs)",
                    ws_canonical
                );
            }

            if let Some(ref ts_path) = tsserver_path {
                let ts_major = verter_lsp::tsserver::detect_ts_major_version(ts_path);
                tracing::info!(
                    "auto mode: detected TypeScript {} at {}",
                    ts_major.map_or("unknown".to_string(), |v| format!("{v}.x")),
                    ts_path.display()
                );

                let prefer_tsserver = ts_major == Some(5) || ts_major == Some(6) || has_composite;

                if prefer_tsserver {
                    match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                        Ok(tp) => return (Some(tp), TypeProviderKind::Tsserver, false, None),
                        Err(reason) => {
                            tracing::warn!("auto mode: tsserver unavailable: {reason}");
                            tsserver_reason = Some(reason);
                        }
                    }
                }
            } else if has_composite {
                // No local TypeScript found, but composite tsconfigs detected.
                // Try spawning tsserver anyway — find_node + global TS might work,
                // and tsserver handles composite configs better than TSGO.
                tracing::info!(
                    "auto mode: no local TypeScript found, but composite tsconfig detected — \
                     attempting tsserver anyway"
                );
                match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                    Ok(tp) => return (Some(tp), TypeProviderKind::Tsserver, false, None),
                    Err(reason) => {
                        tracing::warn!("auto mode: tsserver unavailable: {reason}");
                        tsserver_reason = Some(reason);
                    }
                }
            }

            // No tsserver available or not preferred — try TSGO
            let tsgo_reason = match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Ok(tp) => return (Some(tp), TypeProviderKind::Tsgo, false, None),
                Err(reason) => {
                    tracing::warn!("auto mode: tsgo unavailable: {reason}");
                    reason
                }
            };

            let reason = if let Some(tsserver_reason) = tsserver_reason {
                format!("tsserver unavailable: {tsserver_reason}; tsgo unavailable: {tsgo_reason}")
            } else {
                format!("tsgo unavailable: {tsgo_reason}")
            };
            tracing::info!("no type provider available — running in verter-only mode ({reason})");
            (None, TypeProviderKind::None, false, Some(reason))
        }
    }
}

/// Try to spawn TSGO.
async fn try_spawn_tsgo(
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, String> {
    let tsgo_bin = find_tsgo_binary().map_err(|err| err.to_string())?;
    tracing::info!("found tsgo binary: {tsgo_bin}");

    let root_uri = path_to_file_uri(workspace_root);
    let crash_notify = Arc::new(Notify::new());

    match TsgoTypeProvider::spawn_with_crash_signal(
        &tsgo_bin,
        &root_uri,
        Some(Arc::clone(&crash_notify)),
    )
    .await
    {
        Ok(tp) => {
            tracing::info!("TSGO type provider started (resilient mode)");
            let resilient = tsgo_resilient::new(
                tp,
                crash_notify,
                tsgo_bin,
                root_uri,
                Arc::clone(client_cell),
                3,
            );
            Ok(Arc::new(resilient))
        }
        Err(e) => Err(format!(
            "found tsgo at {tsgo_bin}, but spawn/initialize failed: {e}"
        )),
    }
}

/// Try to spawn tsserver.
async fn try_spawn_tsserver(
    args: &CliArgs,
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, String> {
    let node_path = verter_lsp::tsserver::find_node()
        .ok_or_else(|| "Node.js not found on PATH or standard locations".to_string())?;
    let tsserver_path =
        verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(workspace_root))
            .ok_or_else(|| "TypeScript not installed in workspace".to_string())?;

    tracing::info!(
        "found tsserver: {} (node: {})",
        tsserver_path.display(),
        node_path
    );

    let crash_notify = Arc::new(Notify::new());
    let tsserver_str = tsserver_path.to_string_lossy().to_string();

    match TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_str,
        workspace_root,
        args.plugin_path.as_deref(),
        Some(Arc::clone(&crash_notify)),
    )
    .await
    {
        Ok(tp) => {
            tracing::info!("tsserver type provider started (resilient mode)");
            let resilient = tsserver_resilient::new(
                tp,
                crash_notify,
                node_path,
                tsserver_str,
                workspace_root.to_string(),
                args.plugin_path.clone(),
                Arc::clone(client_cell),
                3,
            );
            Ok(Arc::new(resilient))
        }
        Err(e) => Err(format!(
            "found tsserver at {} (node: {node_path}), but spawn/initialize failed: {e}",
            tsserver_path.display()
        )),
    }
}

/// Convert a file path to a `file://` URI.
fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        // Windows: C:/Users/... → file:///C:/Users/...
        format!("file:///{normalized}")
    }
}
