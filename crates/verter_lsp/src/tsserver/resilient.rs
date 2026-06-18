//! Resilient wrapper around `TsserverTypeProvider` with crash detection and auto-restart.
//!
//! The restart mechanics live in `verter_type_runtime::resilient`; this module only
//! supplies the tsserver-specific respawn strategy and the LSP `Client` bridge.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{LspNotifier, ResilientBackend, ResilientProvider};
use crate::tsserver::ipc::TsserverTypeProvider;
use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::TypeProvider;

struct TsserverBackend {
    node_path: String,
    tsserver_path: String,
    workspace_root: String,
    plugin_path: Option<String>,
}

impl ResilientBackend<TsserverTypeProvider> for TsserverBackend {
    fn log_name(&self) -> &'static str {
        "tsserver"
    }

    fn user_label(&self) -> &'static str {
        "tsserver"
    }

    fn restarting_error(&self) -> &'static str {
        "tsserver is restarting"
    }

    fn spawn<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TsserverTypeProvider, TypeProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            TsserverTypeProvider::spawn(
                &self.node_path,
                &self.tsserver_path,
                &self.workspace_root,
                self.plugin_path.as_deref(),
                Some(crash_notify),
            )
            .await
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn new(
    provider: TsserverTypeProvider,
    crash_notify: Arc<Notify>,
    node_path: String,
    tsserver_path: String,
    workspace_root: String,
    plugin_path: Option<String>,
    client: Arc<OnceCell<Client>>,
    max_restarts: u32,
) -> impl TypeProvider {
    let notifier = Arc::new(LspNotifier::new(client));
    ResilientProvider::new(
        provider,
        crash_notify,
        TsserverBackend {
            node_path,
            tsserver_path,
            workspace_root,
            plugin_path,
        },
        notifier,
        max_restarts,
    )
}
