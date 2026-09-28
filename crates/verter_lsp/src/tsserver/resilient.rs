//! The tsserver establishment strategy for the shared provider hub.
//!
//! Establishment, crash recovery, desired-state replay and the serving epoch
//! live in `verter_type_runtime::provider_hub`; this module only supplies how
//! one tsserver engine is spawned and the LSP `Client` bridge.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{
    EstablishFuture, HubPolicy, LspNotifier, ProviderEstablisher, ProviderHub,
};
use crate::tsserver::ipc::TsserverTypeProvider;
use crate::type_provider::traits::TypeProvider;

/// Everything one tsserver engine is spawned from. Every establishment — the
/// first one and every crash recovery — spawns from the same inputs, so a
/// recovered engine points the plugin at the same live carrier store.
pub(crate) struct TsserverEngineInputs {
    pub(crate) node_path: String,
    pub(crate) tsserver_path: String,
    pub(crate) workspace_root: String,
    pub(crate) plugin_path: Option<String>,
    pub(crate) carrier_store_dir: String,
    /// verter_lsp-internal engines keep the plugin's response remap OFF so the
    /// Rust merge layer stays the sole companion→source mapper.
    pub(crate) plugin_response_remap: bool,
}

impl TsserverEngineInputs {
    /// The production inputs: the carrier-publish store dir derived from the
    /// workspace root through the SAME shared resolver the publish backend
    /// uses, and the plugin's response remap off.
    pub(crate) fn production(
        node_path: String,
        tsserver_path: String,
        workspace_root: String,
        plugin_path: Option<String>,
    ) -> Self {
        let carrier_store_dir =
            crate::external_ts::default_carrier_store_dir_string(&workspace_root);
        Self {
            node_path,
            tsserver_path,
            workspace_root,
            plugin_path,
            carrier_store_dir,
            plugin_response_remap: false,
        }
    }
}

struct TsserverBackend {
    inputs: TsserverEngineInputs,
}

impl ProviderEstablisher<dyn TypeProvider> for TsserverBackend {
    fn log_name(&self) -> &'static str {
        "tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "tsserver is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(&'a self, crash_signal: Arc<Notify>) -> EstablishFuture<'a, dyn TypeProvider> {
        Box::pin(async move {
            let inputs = &self.inputs;
            let provider = TsserverTypeProvider::spawn(
                &inputs.node_path,
                &inputs.tsserver_path,
                &inputs.workspace_root,
                inputs.plugin_path.as_deref(),
                Some(&inputs.carrier_store_dir),
                inputs.plugin_response_remap,
                Some(crash_signal),
            )
            .await?;
            Ok(Arc::new(provider) as Arc<dyn TypeProvider>)
        })
    }
}

/// The hub of ONE tsserver engine. Nothing is spawned until the caller
/// establishes it; the hub then owns its recovery for the session.
pub(crate) fn hub(
    inputs: TsserverEngineInputs,
    client: Arc<OnceCell<Client>>,
    max_restarts: u32,
) -> ProviderHub<dyn TypeProvider> {
    ProviderHub::new(
        TsserverBackend { inputs },
        Arc::new(LspNotifier::new(client, "tsserver")),
        HubPolicy::explicit(max_restarts),
    )
}
