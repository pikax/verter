pub mod analysis;
pub mod capabilities;
pub mod config;
pub mod css;
pub mod documents;
pub mod features;
pub mod server;
pub mod statistics;
pub mod tsgo;
pub mod utils;

#[cfg(test)]
mod integration_tests;

use std::sync::Arc;
use verter_host::VerterHost;

use tsgo::traits::TypeProvider;

/// Configuration for creating a verter LSP server instance.
pub struct LspConfig {
    /// The verter host instance (always required).
    pub host: VerterHost,
    /// Optional type provider for TSGO integration.
    /// When `None`, the LSP runs in verter-only mode.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// How files are synced to the type provider.
    pub project_sync_mode: ProjectSyncMode,
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
