//! Standalone Verter MCP server entry point.
//!
//! This binary lets IDEs (and other clients) run the MCP server in a
//! separate process from the LSP server: `verter_lsp` has no dependency
//! edge to `verter_mcp`, and clients that need MCP spawn this binary (or
//! the identically-behaving distributed `verter-mcp`) alongside the LSP.
//!
//! Usage:
//!   # stdio transport (default, for local agents):
//!   verter-mcp-server --project-root /path/to/project
//!
//!   # HTTP transport (for remote agents and spawning hosts):
//!   verter-mcp-server --transport http --port 6772 --project-root /path/to/project
//!
//! With `--transport http`, the bound port (OS-assigned under `--port 0`) is
//! announced as a one-line JSON readiness record on stdout — see
//! `verter_mcp::readiness` for the wire shape hosts parse.
//!
//! The implementation is `verter_mcp::run::run`, shared verbatim with the
//! `verter-mcp` binary so the two entry points cannot drift apart.

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
