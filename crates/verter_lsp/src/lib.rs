// The carrier-membership / provider-sync paths must never hold a synchronous guard
// across an `.await` (the single-writer actor + reconciler lock discipline). Deny it
// crate-wide so a regression fails the build, matching `verter_type_runtime`.
#![deny(clippy::await_holding_lock)]

/// Max concurrent in-flight requests the tower-lsp-server serve loop dispatches
/// (`Server::concurrency_level`). tower-lsp-server 0.23 defaults to 4; a handful
/// of slow semantic handlers then occupy every slot, the framed-stdin forwarder
/// stalls, and the server stops reading client stdin entirely — so provider-free
/// control requests (`$/verter/getStatistics`, `$/cancelRequest`) are STARVED and
/// no client-side rescue can land. B12's always-on per-request deadline stops a
/// handler occupying a slot forever; this generous cap additionally guarantees
/// control requests get a slot immediately alongside a burst of semantic work.
pub const LSP_MAX_CONCURRENCY: usize = 64;

pub mod analysis;
pub mod audit_harness;
pub mod capabilities;
pub mod carrier_cache;
pub mod carrier_registry;
pub mod config;
pub mod css;
pub mod documents;
pub mod editor_tsserver;
pub mod extension_provider;
pub mod external_ts;
pub mod external_ts_sync;
pub mod features;
pub mod project_resolver;
pub mod provider_surface_store;
pub mod provider_sync;
pub mod resync_coordinator;
pub mod server;
pub mod statistics;
pub mod svelte_assets;
pub mod sync_coordinator;
pub mod tsgo;
pub mod tsserver;
pub mod type_provider;
pub mod utils;
pub mod vue_assets;
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
mod resilient_provider_tests;
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod test_harness_gating;
#[cfg(test)]
mod test_utils;

use std::sync::Arc;
use verter_session::VerterHost;

use type_provider::traits::TypeProvider;

/// Which TypeScript type provider backend is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeProviderKind {
    /// TSGO (Go-based TypeScript server).
    Tsgo,
    /// tsserver (Node.js-based TypeScript server).
    Tsserver,
    /// The editor's own tsserver, extended by Verter's contributed plugin.
    EditorTsserver,
    /// No type provider — verter-only mode.
    None,
}

impl std::fmt::Display for TypeProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeProviderKind::Tsgo => write!(f, "TSGO"),
            TypeProviderKind::Tsserver => write!(f, "tsserver"),
            TypeProviderKind::EditorTsserver => write!(f, "editor-tsserver"),
            TypeProviderKind::None => write!(f, "none"),
        }
    }
}

/// Configuration for creating a verter LSP server instance.
pub struct LspConfig {
    /// The verter host instance (always required, shared via Arc for MCP embedding).
    pub host: Arc<VerterHost>,
    /// Optional in-process provider actor. This is `None` both in Verter-only mode and
    /// when the editor-owned tsserver/plugin is the attested semantic authority.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// How files are synced to the type provider.
    pub project_sync_mode: ProjectSyncMode,
    /// Which type provider backend is active.
    pub type_provider_kind: TypeProviderKind,
    /// Actual MCP HTTP port (already bound). `None` when MCP is disabled.
    /// The LSP sends a `$/verter/mcpReady` notification during `initialized()`.
    pub mcp_port: Option<u16>,
    /// Human-readable provenance for the selected provider, or the reason no provider
    /// could be started. Sent via `$/verter/typeProviderStatus` for editor status UI.
    pub type_provider_reason: Option<String>,
    /// TEST SEAM: when `true`, `did_open` does NOT eagerly prewarm an imported
    /// child carrier's `{carrier}.ts` PUBLIC-API surface. Production leaves this
    /// `false` (the prewarm makes hover/completion/go-to-def on `<ChildComponent>`
    /// work immediately). With suppression on, the ONLY sync of a closed child's
    /// API surface would come from INSIDE `handle_rename`'s own sync-before-query —
    /// so this seam drives the WOULD-BE discriminator for that path. That lane is
    /// currently `#[ignore]`'d: under tsserver the in-`handle_rename` sync opens the
    /// child too late to join the parent's program (the Block H-membership
    /// program-membership gap), so suppression does NOT prove `handle_rename`'s own
    /// sync closes the closed child today — it pins the seam against which Block
    /// H-membership is validated.
    pub suppress_imported_carrier_prewarm: bool,
}

/// Controls what data `verter_lsp` sends to the type provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSyncMode {
    /// Send resolver-managed project files to the type provider.
    /// `.vue` files are exposed as `.vue.tsx` for IDE queries and `.vue.ts`
    /// for public API resolution; non-carrier files are synced as source files.
    #[default]
    FullProject,
}
