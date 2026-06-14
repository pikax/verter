pub mod analysis;
pub mod audit_harness;
pub mod capabilities;
pub mod config;
pub mod css;
pub mod documents;
pub mod extension_provider;
pub mod features;
pub mod project_resolver;
pub mod provider_sync;
pub mod server;
pub mod statistics;
pub mod svelte_assets;
pub mod sync_coordinator;
pub mod tsgo;
pub mod tsserver;
pub mod utils;
pub mod workspace_scanner;
pub mod workspace_state;

mod resilient_provider;
mod uri;

#[cfg(test)]
#[allow(
    unused_must_use,
    clippy::unused_enumerate_index,
    clippy::unnecessary_to_owned,
    clippy::redundant_iter_cloned
)]
mod integration_tests;
#[cfg(test)]
mod real_provider_tests;
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod test_utils;

use std::sync::Arc;
use verter_session::VerterHost;

use tsgo::traits::TypeProvider;

/// Which TypeScript type provider backend is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeProviderKind {
    /// TSGO (Go-based TypeScript server).
    Tsgo,
    /// tsserver (Node.js-based TypeScript server).
    Tsserver,
    /// No type provider — verter-only mode.
    None,
}

impl std::fmt::Display for TypeProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeProviderKind::Tsgo => write!(f, "TSGO"),
            TypeProviderKind::Tsserver => write!(f, "tsserver"),
            TypeProviderKind::None => write!(f, "none"),
        }
    }
}

/// Configuration for creating a verter LSP server instance.
pub struct LspConfig {
    /// The verter host instance (always required, shared via Arc for MCP embedding).
    pub host: Arc<VerterHost>,
    /// Optional type provider for TSGO integration.
    /// When `None`, the LSP runs in verter-only mode.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// How files are synced to the type provider.
    pub project_sync_mode: ProjectSyncMode,
    /// Which type provider backend is active.
    pub type_provider_kind: TypeProviderKind,
    /// When `true`, show a recommendation to switch to TSGO in VS Code settings.
    /// Set by `auto` mode when tsserver is chosen because TS 5.x was detected.
    pub suggest_tsgo: bool,
    /// Actual MCP HTTP port (already bound). `None` when MCP is disabled.
    /// The LSP sends a `$/verter/mcpReady` notification during `initialized()`.
    pub mcp_port: Option<u16>,
    /// Why no type provider could be started (only set when `type_provider` is `None`).
    /// Sent to the extension via `$/verter/typeProviderStatus` so it can show a meaningful
    /// status bar warning (e.g., "Node.js not found", "TypeScript not installed").
    pub type_provider_none_reason: Option<String>,
}

/// Controls what data `verter_lsp` sends to the type provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSyncMode {
    /// Send resolver-managed project files to the type provider.
    /// `.vue` files are exposed as `.vue.tsx` for IDE queries and `.vue.ts`
    /// for public API resolution; non-Vue files are synced as source files.
    #[default]
    FullProject,
}
