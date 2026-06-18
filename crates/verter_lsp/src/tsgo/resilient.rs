//! Resilient wrapper around `TsgoTypeProvider` with crash detection and auto-restart.
//!
//! The restart mechanics live in `verter_type_runtime::resilient`; this module only
//! supplies the TSGO-specific respawn strategy and the LSP `Client` bridge.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{LspNotifier, ResilientBackend, ResilientProvider};
use crate::tsgo::ipc::TsgoTypeProvider;
use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::TypeProvider;

struct TsgoBackend {
    tsgo_bin: String,
    root_uri: String,
}

impl ResilientBackend<TsgoTypeProvider> for TsgoBackend {
    fn log_name(&self) -> &'static str {
        "TSGO"
    }

    fn user_label(&self) -> &'static str {
        "tsgo"
    }

    fn restarting_error(&self) -> &'static str {
        "tsgo is restarting"
    }

    fn spawn<'a>(
        &'a self,
        crash_notify: Arc<Notify>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TsgoTypeProvider, TypeProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            TsgoTypeProvider::spawn_with_crash_signal(
                &self.tsgo_bin,
                &self.root_uri,
                Some(crash_notify),
            )
            .await
        })
    }
}

pub fn new(
    provider: TsgoTypeProvider,
    crash_notify: Arc<Notify>,
    tsgo_bin: String,
    root_uri: String,
    client: Arc<OnceCell<Client>>,
    max_restarts: u32,
) -> impl TypeProvider {
    let notifier = Arc::new(LspNotifier::new(client));
    ResilientProvider::new(
        provider,
        crash_notify,
        TsgoBackend { tsgo_bin, root_uri },
        notifier,
        max_restarts,
    )
}
