//! The OWNED tsgo establishment strategy for the shared provider hub.
//!
//! Establishment, crash recovery, desired-state replay and the serving epoch
//! live in `verter_type_runtime::provider_hub`; this module only supplies how
//! one owned dual-surface tsgo engine is spawned and the LSP `Client` bridge.

use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;

use crate::resilient_provider::{
    EstablishFuture, HubPolicy, LspNotifier, ProviderEstablisher, ProviderHub,
};
use crate::tsgo::ipc::{TsgoOwnedProvider, TsgoTypeProvider};
use crate::type_provider::protocol::TypeProviderError;
use crate::type_provider::traits::TypeProvider;

/// Each establishment produces a [`TsgoOwnedProvider`] — a fresh `tsgo --lsp`
/// process WITH the version-gated `--api` checker attached over its minted pipe,
/// so a recovery restores BOTH surfaces on the new process (no second spawn, no
/// stale attach). The `--api` checker stores no configured project — the owning
/// tsconfig is supplied per query — so an establishment creates the PROCESS
/// ONLY; the hub replays the editor state.
struct TsgoOwnedBackend {
    tsgo_bin: String,
    root_uri: String,
}

impl ProviderEstablisher<TsgoOwnedProvider> for TsgoOwnedBackend {
    fn log_name(&self) -> &'static str {
        "TSGO(owned)"
    }

    fn user_label(&self) -> &'static str {
        "tsgo"
    }

    fn restarting_error(&self) -> &'static str {
        "tsgo is restarting"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        crash_signal: Arc<Notify>,
    ) -> EstablishFuture<'a, TsgoOwnedProvider> {
        Box::pin(async move {
            let lsp = TsgoTypeProvider::spawn_with_crash_signal(
                &self.tsgo_bin,
                &self.root_uri,
                Some(crash_signal),
            )
            .await
            .map_err(|error| TypeProviderError::new(format!("spawn/initialize failed: {error}")))?;
            let lsp = Arc::new(lsp);
            // A probe / wire-gate / attach failure fails closed rather than
            // silently degrading the typecheck oracle, and never orphans the
            // `--lsp` child it spawned.
            match TsgoOwnedProvider::attach(Arc::clone(&lsp), &self.tsgo_bin).await {
                Ok(provider) => Ok(Arc::new(provider)),
                Err(error) => {
                    let teardown = lsp.shutdown().await;
                    Err(TypeProviderError::new(format!(
                        "spawned --lsp, but the version-gated --api attach failed: {error}; \
                         managed child teardown: {}",
                        teardown
                            .err()
                            .map_or_else(|| "reaped".to_string(), |error| error.to_string())
                    )))
                }
            }
        })
    }
}

/// The wire policy for the owned engine's structural start announcements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedStartAnnouncements {
    /// Announce every engine start (`$/verter/typeProviderStarted`). The
    /// policy of every route whose managed engine is editor-visible from its
    /// first serve — the tsgo route, and the tsserver override reclassified
    /// onto it.
    All,
    /// Announce only crash replacements. The policy of the shared route's
    /// managed fallback: the route attests "managed TSGO remains cold until an
    /// observed attach failure", and that attestation is asserted over the
    /// started-announcement channel, so the fallback's first serve must stay
    /// off it. See [`LspNotifier::recovery_only`].
    RecoveryOnly,
}

/// Establish the production OWNED dual-surface tsgo engine through its hub:
/// ONE `tsgo --lsp` with the `--api` checker attached, recovered (re-spawned,
/// re-attached, replayed) by the hub on every crash within `max_restarts`.
///
/// # Errors
/// Returns the establishment failure; the hub leaves nothing running.
pub async fn establish_owned(
    tsgo_bin: String,
    root_uri: String,
    client: Arc<OnceCell<Client>>,
    max_restarts: u32,
    announcements: OwnedStartAnnouncements,
) -> Result<ProviderHub<TsgoOwnedProvider>, TypeProviderError> {
    let notifier = match announcements {
        OwnedStartAnnouncements::All => LspNotifier::new(client, "tsgo"),
        OwnedStartAnnouncements::RecoveryOnly => LspNotifier::recovery_only(client, "tsgo"),
    };
    let hub = ProviderHub::new(
        TsgoOwnedBackend { tsgo_bin, root_uri },
        Arc::new(notifier),
        HubPolicy::explicit(max_restarts),
    );
    hub.establish().await?;
    Ok(hub)
}
