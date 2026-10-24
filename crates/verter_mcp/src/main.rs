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

/// The process allocator (mimalloc on native targets): see `verter_alloc`.
#[global_allocator]
static GLOBAL: verter_alloc::Allocator = verter_alloc::ALLOCATOR;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Before the runtime starts any worker thread: mimalloc's option store is
    // plain memory (see `verter_alloc`).
    verter_alloc::configure();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve())
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    verter_mcp::run::run(cli).await
}

#[cfg(test)]
mod tests {
    /// The binary runs on the allocator `verter_alloc` selects for it: mimalloc
    /// on a native build, so a block from the global heap is mimalloc's.
    #[test]
    fn the_selected_global_allocator_serves_this_binary() {
        assert_eq!(
            verter_alloc::mimalloc_serves_global_allocations(),
            verter_alloc::SELECTED == verter_alloc::AllocatorKind::Mimalloc
        );
    }
}
