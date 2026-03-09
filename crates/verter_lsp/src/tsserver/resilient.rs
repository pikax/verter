//! Resilient wrapper around `TsserverTypeProvider` with crash detection and auto-restart.
//!
//! The restart mechanics live in `crate::resilient_provider`; this module only
//! supplies the tsserver-specific respawn strategy.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{ResilientBackend, ResilientProvider};
use crate::tsgo::protocol::TypeProviderError;
use crate::tsgo::traits::TypeProvider;
use crate::tsserver::ipc::TsserverTypeProvider;

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
    ResilientProvider::new(
        provider,
        crash_notify,
        TsserverBackend {
            node_path,
            tsserver_path,
            workspace_root,
            plugin_path,
        },
        client,
        max_restarts,
    )
}
