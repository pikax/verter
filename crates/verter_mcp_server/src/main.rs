//! Standalone Verter MCP server entry point.
//!
//! This binary lets IDEs (and other clients) run the MCP server in a
//! separate process from the LSP server. Decoupling MCP from LSP means
//! that `cargo build -p verter_lsp` does not pull `verter_mcp` into
//! the LSP binary by default; clients that need MCP can either:
//!
//!   * spawn this binary (`verter-mcp-server`) alongside the LSP, or
//!   * build the LSP with `--features mcp` to embed MCP in the LSP
//!     process (legacy behavior).
//!
//! Usage:
//!   # stdio transport (default, for local agents):
//!   verter-mcp-server --project-root /path/to/project
//!
//!   # HTTP transport (for remote agents):
//!   verter-mcp-server --transport http --port 6772 --project-root /path/to/project
//!
//! The implementation is delegated to `verter_mcp` so the standalone
//! and LSP-embedded paths share the same server code, configuration,
//! scanner, and tool surface.

use std::sync::Arc;

use clap::Parser;
use rmcp::ServiceExt;

use verter_diagnostics::Linter;
use verter_mcp::config::{Cli, McpServerConfig, Transport};
use verter_mcp::scanner;
use verter_mcp::tools;
use verter_mcp::VerterMcpServer;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let roots: Vec<String> = cli
        .project_root
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let workspace = Arc::new(FilesystemWorkspace::new(FilesystemOptions {
        roots,
        eager_preload: false,
    }));

    let host = Arc::new(VerterHost::new(
        {
            let mut config =
                HostConfig::from_query_profile(verter_semantic::profile::QueryProfile::Mcp);
            config.dev_mode = true;
            config
        },
        workspace,
    ));

    let lint_config = tools::diagnostics::make_lint_config(&cli.lint_preset);
    let linter = Arc::new(Linter::new(lint_config));

    let config = McpServerConfig::from(&cli);

    if let Some(root) = &cli.project_root {
        if !cli.no_scan {
            tracing::info!("Scanning project root: {}", root.display());
            let result = scanner::scan_directory(root, &host, false);
            tracing::info!(
                "Loaded {} files ({} with parse errors) in {:.1}ms",
                result.files_loaded,
                result.parse_errors,
                result.scan_duration_ms
            );
        }
    }

    let server = VerterMcpServer::new(host, linter, config);

    match cli.transport {
        Transport::Stdio => {
            tracing::info!("Starting Verter MCP server (stdio transport)");
            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
        }
        Transport::Http => {
            use rmcp::transport::streamable_http_server::{
                session::local::LocalSessionManager, StreamableHttpService,
            };

            let port = cli.port;
            let http_service = StreamableHttpService::new(
                move || Ok(server.clone()),
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new().nest_service("/mcp", http_service);
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;

            tracing::info!(
                "Starting Verter MCP server (HTTP) at http://127.0.0.1:{}/mcp",
                port
            );

            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await?;
        }
    }

    Ok(())
}
