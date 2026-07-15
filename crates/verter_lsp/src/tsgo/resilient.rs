//! Resilient wrapper around `TsgoTypeProvider` with crash detection and auto-restart.
//!
//! The restart mechanics live in `verter_type_runtime::resilient`; this module only
//! supplies the TSGO-specific respawn strategy and the LSP `Client` bridge.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{LspNotifier, ResilientBackend, ResilientProvider};
use crate::tsgo::ipc::{TsgoOwnedProvider, TsgoTypeProvider};
use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::TypeProvider;

/// The OWNED dual-surface respawn strategy: each (re)spawn produces a
/// [`TsgoOwnedProvider`] — a fresh `tsgo --lsp` process WITH the `--api` checker
/// re-attached over its minted pipe. So a crash recovery restores BOTH surfaces on
/// the new process (no second spawn, no stale attach). The `--api` checker stores no
/// configured project — the owning tsconfig is supplied per query — so a restart
/// re-establishes the PROCESS ONLY; there is no per-project state to restore.
struct TsgoOwnedBackend {
    tsgo_bin: String,
    root_uri: String,
}

impl ResilientBackend<TsgoOwnedProvider> for TsgoOwnedBackend {
    fn log_name(&self) -> &'static str {
        "TSGO(owned)"
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
            dyn std::future::Future<Output = Result<TsgoOwnedProvider, TypeProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let inner = TsgoTypeProvider::spawn_with_crash_signal(
                &self.tsgo_bin,
                &self.root_uri,
                Some(crash_notify),
            )
            .await?;
            let inner = Arc::new(inner);
            match TsgoOwnedProvider::attach(Arc::clone(&inner), &self.tsgo_bin).await {
                Ok(provider) => Ok(provider),
                Err(error) => {
                    let _ = inner.shutdown().await;
                    Err(error)
                }
            }
        })
    }
}

/// Build the production OWNED dual-surface tsgo provider wrapped in the resilient
/// respawn layer: ONE `tsgo --lsp` with the `--api` checker attached, re-attached
/// on every crash recovery. The `--api` checker stores no configured project — the
/// owning tsconfig is supplied per query — so a restart re-establishes the PROCESS
/// ONLY.
pub fn new_owned(
    provider: TsgoOwnedProvider,
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
        TsgoOwnedBackend { tsgo_bin, root_uri },
        notifier,
        max_restarts,
    )
}
