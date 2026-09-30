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
use verter_type_runtime::provider_hub::{DroppedAdmittedState, EngineStart};

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
///
/// `admitted_state_rearm` is invoked whenever a replacement install drops the
/// old epoch's admitted generated state — the router's re-publication hook, so
/// a recovered engine regains its carrier registrations through FRESH
/// admission instead of waiting for the next ordinary publication.
///
/// `restart_pulse` is fired every time an engine of this hub begins serving
/// (the initial start and every crash replacement): the pending provider-sync
/// re-drive treats it as its retry signal, so carrier syncs refused during a
/// replacement gap are re-driven the moment the fresh epoch serves.
pub(crate) fn hub(
    inputs: TsserverEngineInputs,
    client: Arc<OnceCell<Client>>,
    max_restarts: u32,
    admitted_state_rearm: Option<crate::resilient_provider::AdmittedStateRearm>,
    restart_pulse: Arc<tokio::sync::Notify>,
) -> ProviderHub<dyn TypeProvider> {
    ProviderHub::new(
        TsserverBackend { inputs },
        Arc::new(RearmNotifier {
            inner: LspNotifier::new(client, "tsserver"),
            rearm: admitted_state_rearm,
            restart_pulse,
        }),
        HubPolicy::explicit(max_restarts),
    )
}

/// The tsserver hub notifier: the LSP wire notifier, the admitted-state
/// recovery re-arm (which must run off the actor's install path), and the
/// engine-start pulse the pending-sync re-drive waits on.
struct RearmNotifier {
    inner: LspNotifier,
    rearm: Option<crate::resilient_provider::AdmittedStateRearm>,
    restart_pulse: Arc<tokio::sync::Notify>,
}

impl verter_type_runtime::provider_hub::ProviderNotifier for RearmNotifier {
    fn notify(&self, severity: verter_type_runtime::provider_hub::NotifySeverity, message: String) {
        self.inner.notify(severity, message);
    }

    fn provider_started(&self, pid: Option<u32>, start: EngineStart) {
        self.restart_pulse.notify_one();
        self.inner.provider_started(pid, start);
    }

    fn admitted_state_dropped(&self, dropped: &DroppedAdmittedState) {
        if let Some(rearm) = &self.rearm {
            rearm(dropped);
        }
    }
}
