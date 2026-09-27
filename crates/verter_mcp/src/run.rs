//! Shared entry-point body for the `verter-mcp` and `verter-mcp-server`
//! binaries.
//!
//! Both binaries are the same product — `verter-mcp` is the distributed name,
//! `verter-mcp-server` exists so `verter_lsp` never needs a dependency edge to
//! this crate — so they run ONE body. Keeping the body here means a transport
//! or readiness change cannot land in one binary and drift out of the other.

use std::sync::Arc;

use rmcp::ServiceExt;

use verter_diagnostics::Linter;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace};

use crate::config::{Cli, McpServerConfig, Transport};
use crate::readiness;
use crate::scanner;
use crate::tools;
use crate::VerterMcpServer;

/// Run the MCP server for parsed CLI arguments until the transport closes.
///
/// The caller owns process-global setup (tracing subscriber installation);
/// everything after argument parsing lives here.
pub async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    // Process-lifetime containment, the same contract as `verter-lsp`: when a
    // host passes `--client-pid`, this server exits as soon as that process
    // dies — an HTTP listener must never outlive the editor that spawned it
    // (a fixed `--port` orphan would EADDRINUSE the next launch). A host that
    // ASKED for containment and cannot get it is refused, never served
    // uncontained. `None` stays valid for standalone agent launches, whose
    // lifetime is owned by their transport (stdio EOF / Ctrl-C).
    let _client_process_guard = verter_tsgo_api::process::ClientProcessGuard::arm(cli.client_pid)
        .map_err(|error| {
        format!("refusing to start verter-mcp without the requested client containment: {error}")
    })?;
    if let Some(client_pid) = cli.client_pid {
        tracing::info!(client_pid, "armed MCP client-process containment");
    }

    // Create filesystem workspace from project root (if provided)
    let roots: Vec<String> = cli
        .project_root
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let workspace = Arc::new(FilesystemWorkspace::new(FilesystemOptions {
        roots,
        eager_preload: false,
    }));

    // Create host backed by the filesystem workspace
    let host = Arc::new(VerterHost::new(
        {
            let mut config =
                HostConfig::from_query_profile(verter_semantic::profile::QueryProfile::Mcp);
            config.dev_mode = true;
            config
        },
        workspace,
    ));

    // Create linter with configured preset
    let lint_config = tools::diagnostics::make_lint_config(&cli.lint_preset);
    let linter = Arc::new(Linter::new(lint_config));

    let config = McpServerConfig::from(&cli);

    let scan = |host: &Arc<VerterHost>| {
        if let Some(root) = &cli.project_root {
            if !cli.no_scan {
                tracing::info!("Scanning project root: {}", root.display());
                let result = scanner::scan_directory(root, host, false);
                tracing::info!(
                    "Loaded {} files ({} with parse errors) in {:.1}ms",
                    result.files_loaded,
                    result.parse_errors,
                    result.scan_duration_ms
                );
            }
        }
    };

    match cli.transport {
        Transport::Stdio => {
            scan(&host);
            let server = VerterMcpServer::new(host, linter, config);
            tracing::info!("Starting Verter MCP server (stdio transport)");
            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
        }
        Transport::Http => {
            use rmcp::transport::streamable_http_server::{
                session::local::LocalSessionManager, StreamableHttpService,
            };

            // Bind FIRST so a spawning host learns the OS-assigned port
            // immediately; connections queue in the accept backlog while the
            // initial scan runs. Stdout carries EXACTLY the machine-readable
            // readiness record (stdio transport owns stdout for MCP framing,
            // so the record exists only on this arm); humans read stderr.
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", cli.port)).await?;
            let bound_port = listener.local_addr()?.port();
            {
                use std::io::Write;
                let mut stdout = std::io::stdout().lock();
                writeln!(stdout, "{}", readiness::http_ready_record(bound_port))?;
                stdout.flush()?;
            }
            tracing::info!(
                "Starting Verter MCP server (HTTP) at http://127.0.0.1:{}/mcp",
                bound_port
            );

            scan(&host);
            let server = VerterMcpServer::new(host, linter, config);

            let http_service = StreamableHttpService::new(
                move || Ok(server.clone()),
                LocalSessionManager::default().into(),
                Default::default(),
            );

            let router = axum::Router::new().nest_service("/mcp", http_service);

            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.ok();
                })
                .await?;
        }
    }

    Ok(())
}
