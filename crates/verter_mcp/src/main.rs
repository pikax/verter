//! Verter MCP server binary.
//!
//! Exposes Vue SFC analysis, diagnostics, compilation, and cross-file
//! intelligence to AI agents via the Model Context Protocol.
//!
//! Usage:
//!   # stdio transport (default, for local agents):
//!   verter-mcp --project-root /path/to/project
//!
//!   # HTTP transport (for remote agents and spawning hosts):
//!   verter-mcp --transport http --port 6772 --project-root /path/to/project
//!
//! With `--transport http`, the bound port (OS-assigned under `--port 0`) is
//! announced as a one-line JSON readiness record on stdout — see
//! `verter_mcp::readiness` for the wire shape hosts parse.

use clap::Parser;

use verter_mcp::config::Cli;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    // The server and its runtime run on sized stacks
    // (`verter_mcp::run::RUNTIME_STACK_BYTES`), not on this main thread.
    verter_mcp::run::run_blocking(cli)
}
