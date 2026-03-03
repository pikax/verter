//! Verter MCP server binary.
//!
//! Exposes Vue SFC analysis, diagnostics, compilation, and cross-file
//! intelligence to AI agents via the Model Context Protocol.
//!
//! Usage:
//!   # stdio transport (default, for local agents):
//!   verter-mcp --project-root /path/to/project
//!
//!   # HTTP transport (for remote agents):
//!   verter-mcp --transport http --port 6772 --project-root /path/to/project

use std::sync::Arc;

use clap::Parser;
use rmcp::ServiceExt;

use verter_diagnostics::Linter;
use verter_host::{HostConfig, VerterHost};
use verter_mcp::config::{Cli, McpServerConfig, Transport};
use verter_mcp::scanner;
use verter_mcp::tools;
use verter_mcp::VerterMcpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // Create host with full analysis scope for maximum data
    let host = Arc::new(VerterHost::new(HostConfig {
        analysis_scope: Some(verter_analysis::AnalysisScope::LSP),
        dev_mode: true,
        ..Default::default()
    }));

    // Create linter with configured preset
    let lint_config = tools::diagnostics::make_lint_config(&cli.lint_preset);
    let linter = Arc::new(Linter::new(lint_config));

    let config = McpServerConfig::from(&cli);

    // Initial project scan
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
