//! Managed TypeScript provider activated only after the preferred editor-owned route fails.
//!
//! The managed fallback is a [`ProviderHub`] established ON DEMAND: file and
//! configuration lifecycle is recorded in the hub's desired state without
//! starting a process, and the first real fallback query establishes one
//! managed engine chain, replays the latest desired state into it, and only
//! then serves the query. Establishment, replay, singleflight and the retry
//! cooldown are the hub's; this module only supplies the engine-chain factory.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Notify;
use verter_type_runtime::protocol::TypeProviderError;
use verter_type_runtime::provider_hub::{
    EstablishFuture, HubPolicy, ProviderEstablisher, ProviderHub, TracingNotifier,
};
use verter_type_runtime::traits::TypeProvider;

type FactoryFuture =
    Pin<Box<dyn Future<Output = Result<Arc<dyn TypeProvider>, TypeProviderError>> + Send>>;
type Factory = dyn Fn() -> FactoryFuture + Send + Sync;

/// The managed fallback: a hub whose engine is the factory's engine chain.
///
/// A successful activation serves the session. A FAILED activation is retried
/// after [`ACTIVATION_RETRY_COOLDOWN`], so a transient spawn/replay failure
/// recovers on a later fallback query while the cooldown still prevents a hot
/// respawn storm.
pub type LazyManagedTypeProvider = ProviderHub<dyn TypeProvider>;

/// Minimum interval between managed-fallback activation attempts after a failure.
/// Bounds the respawn rate so a persistently failing backend cannot hot-loop,
/// while still letting a transient failure recover on a later query.
pub(crate) const ACTIVATION_RETRY_COOLDOWN: std::time::Duration =
    std::time::Duration::from_millis(250);

/// The managed engine chain as an establishment strategy. The chain it
/// produces owns its own crash recovery (each tier is itself hub-managed), so
/// the crash signal is not wired into it.
struct ManagedFactory {
    factory: Arc<Factory>,
}

impl ProviderEstablisher<dyn TypeProvider> for ManagedFactory {
    fn log_name(&self) -> &'static str {
        "managed fallback"
    }

    fn user_label(&self) -> &'static str {
        "tsgo"
    }

    fn restarting_error(&self) -> &'static str {
        "managed fallback is not active"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        _crash_signal: Arc<Notify>,
    ) -> EstablishFuture<'a, dyn TypeProvider> {
        let factory = Arc::clone(&self.factory);
        Box::pin(async move {
            let provider = factory().await?;
            // The component that OWNS activation is the only one that can
            // report it truthfully: the editor-owned serving path cannot see it.
            tracing::info!(
                provider = provider.provider_id(),
                "managed fallback ACTIVATED — the editor-owned route could not serve a bound demand"
            );
            Ok(provider)
        })
    }
}

/// A managed fallback whose engine chain is created by `factory` on the first
/// fallback query. Lifecycle calls made before activation only update the
/// hub's desired state.
#[must_use]
pub fn new_lazy_managed<F, Fut>(factory: F) -> LazyManagedTypeProvider
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Arc<dyn TypeProvider>, TypeProviderError>> + Send + 'static,
{
    ProviderHub::new(
        ManagedFactory {
            factory: Arc::new(move || Box::pin(factory())),
        },
        Arc::new(TracingNotifier),
        HubPolicy::on_demand(0, ACTIVATION_RETRY_COOLDOWN),
    )
}
