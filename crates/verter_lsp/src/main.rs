use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use verter_host::{HostConfig, VerterHost};
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::tsgo::ipc::{find_tsgo_binary, TsgoTypeProvider};
use verter_lsp::tsgo::resilient::ResilientTypeProvider;
use verter_lsp::tsgo::traits::TypeProvider;
use verter_lsp::tsserver::ipc::TsserverTypeProvider;
use verter_lsp::tsserver::resilient::ResilientTsserverProvider;
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};

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

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let host = Arc::new(VerterHost::new(HostConfig {
        analysis_level: verter_host::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    // Parse CLI arguments
    let args = CliArgs::parse();

    // Optionally start the MCP HTTP server alongside LSP (shares the same VerterHost).
    if let Some(mcp_port) = args.mcp_port {
        let mcp_host = Arc::clone(&host);
        let mcp_lint_preset = args.mcp_lint_preset.clone();
        tokio::spawn(async move {
            if let Err(e) = start_mcp_http(mcp_host, mcp_port, &mcp_lint_preset).await {
                tracing::warn!("MCP HTTP server failed to start: {e}");
            }
        });
    }

    // Deferred client cell — populated inside LspService::build, used by
    // ResilientTypeProvider's crash monitor to send user notifications.
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());

    // Provider selection: auto → detect TS version, pick provider
    let (type_provider, provider_kind, suggest_tsgo) =
        create_type_provider(&args, &client_cell).await;

    let config = LspConfig {
        host,
        type_provider,
        project_sync_mode: ProjectSyncMode::TsxOnly,
        type_provider_kind: provider_kind,
        suggest_tsgo,
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
    /// MCP HTTP port. When set, starts an HTTP MCP endpoint alongside LSP stdio.
    mcp_port: Option<u16>,
    /// Lint preset for the MCP server's diagnostic tools.
    mcp_lint_preset: String,
}

impl CliArgs {
    fn parse() -> Self {
        let mut type_provider = "auto".to_string();
        let mut tsdk = None;
        let mut plugin_path = None;
        let mut workspace_root = None;
        let mut mcp_port = None;
        let mut mcp_lint_preset = "recommended".to_string();

        for arg in std::env::args().skip(1) {
            if let Some(val) = arg.strip_prefix("--type-provider=") {
                type_provider = val.to_string();
            } else if let Some(val) = arg.strip_prefix("--tsdk=") {
                tsdk = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--plugin-path=") {
                plugin_path = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--mcp-port=") {
                mcp_port = val.parse().ok();
            } else if let Some(val) = arg.strip_prefix("--mcp-lint-preset=") {
                mcp_lint_preset = val.to_string();
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
            mcp_lint_preset,
        }
    }
}

/// Create the type provider based on CLI args.
///
/// Auto mode: if TypeScript 5.x is installed in node_modules, use tsserver
/// and recommend switching to TSGO. If no TypeScript is found, try TSGO.
async fn create_type_provider(
    args: &CliArgs,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> (Option<Arc<dyn TypeProvider>>, TypeProviderKind, bool) {
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
    let ws_canonical = workspace_root.replace('\\', "/");

    match args.type_provider.as_str() {
        "off" => {
            tracing::info!("type provider: disabled by --type-provider=off");
            (None, TypeProviderKind::None, false)
        }
        "tsgo" => {
            // TSGO only — no fallback
            match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Some(tp) => (Some(tp), TypeProviderKind::Tsgo, false),
                None => {
                    tracing::warn!("TSGO unavailable — running in verter-only mode");
                    (None, TypeProviderKind::None, false)
                }
            }
        }
        "tsserver" => {
            // tsserver only — no fallback
            match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                Some(tp) => (Some(tp), TypeProviderKind::Tsserver, false),
                None => {
                    tracing::warn!("tsserver unavailable — running in verter-only mode");
                    (None, TypeProviderKind::None, false)
                }
            }
        }
        _ => {
            // "auto" (default): detect workspace TypeScript version
            // If TS 5.x is installed → use tsserver, recommend TSGO
            // If no TS installed → try TSGO, else none
            let tsserver_path =
                verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(&ws_canonical));

            if let Some(ref ts_path) = tsserver_path {
                let ts_major = verter_lsp::tsserver::detect_ts_major_version(ts_path);
                tracing::info!(
                    "auto mode: detected TypeScript {} at {}",
                    ts_major.map_or("unknown".to_string(), |v| format!("{v}.x")),
                    ts_path.display()
                );

                if ts_major == Some(5) || ts_major == Some(6) {
                    // TS 5.x/6.x installed — use tsserver with the workspace version
                    if let Some(tp) = try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                        return (Some(tp), TypeProviderKind::Tsserver, true);
                    }
                }
            }

            // No TS found or tsserver failed — try TSGO
            if let Some(tp) = try_spawn_tsgo(&ws_canonical, client_cell).await {
                return (Some(tp), TypeProviderKind::Tsgo, false);
            }
            tracing::info!("no type provider available — running in verter-only mode");
            (None, TypeProviderKind::None, false)
        }
    }
}

/// Try to spawn TSGO.
async fn try_spawn_tsgo(
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Option<Arc<dyn TypeProvider>> {
    let tsgo_bin = find_tsgo_binary()?;
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
            let resilient = ResilientTypeProvider::new(
                tp,
                crash_notify,
                tsgo_bin,
                root_uri,
                Arc::clone(client_cell),
                3,
            );
            Some(Arc::new(resilient))
        }
        Err(e) => {
            tracing::warn!("TSGO spawn failed: {e}");
            None
        }
    }
}

/// Try to spawn tsserver.
async fn try_spawn_tsserver(
    args: &CliArgs,
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Option<Arc<dyn TypeProvider>> {
    let node_path = verter_lsp::tsserver::find_node()?;
    let tsserver_path =
        verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(workspace_root))?;

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
            let resilient = ResilientTsserverProvider::new(
                tp,
                crash_notify,
                node_path,
                tsserver_str,
                workspace_root.to_string(),
                args.plugin_path.clone(),
                Arc::clone(client_cell),
                3,
            );
            Some(Arc::new(resilient))
        }
        Err(e) => {
            tracing::warn!("tsserver spawn failed: {e}");
            None
        }
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

/// Start an HTTP MCP server that shares the LSP's VerterHost.
async fn start_mcp_http(
    host: Arc<VerterHost>,
    port: u16,
    lint_preset: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    };
    use verter_mcp::{McpServerConfig, VerterMcpServer};

    let lint_config = verter_mcp::tools::diagnostics::make_lint_config(lint_preset);
    let linter = Arc::new(verter_diagnostics::Linter::new(lint_config));
    let config = McpServerConfig::default();
    let server = VerterMcpServer::new(host, linter, config);

    let http_service = StreamableHttpService::new(
        move || Ok(server.clone()),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", http_service);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    let actual_port = listener.local_addr()?.port();

    tracing::info!("MCP HTTP server running at http://127.0.0.1:{actual_port}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
