pub mod analysis;
pub mod capabilities;
pub mod config;
pub mod css;
pub mod documents;
pub mod features;
pub mod server;
pub mod statistics;
pub mod tsgo;
pub mod tsserver;
pub mod utils;

#[cfg(test)]
mod integration_tests;

use std::sync::Arc;
use verter_host::VerterHost;

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
}

/// Controls what data `verter_lsp` sends to the type provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSyncMode {
    /// Only send `.vue` -> TSX replacements.
    /// The type provider reads `.ts`, `node_modules/`, `tsconfig.json` from disk.
    #[default]
    TsxOnly,
    /// Send ALL files: `.vue` as TSX, `.ts`/`.js` as-is, `tsconfig.json`, type declarations.
    /// Required when the type provider has no file system access (WASM playground, remote workspaces).
    FullProject,
}
