//! The host's request attachment: composition-owned state that travels beside
//! the engine through the request port, never through the engine binding.
//!
//! The engine's binding carries only the engine's own resources. Host-owned
//! result stores — the per-framework surface DTO stores — and the host's
//! inert output lease are reached by session code through this attachment,
//! which the host builds once at construction from the SAME shared instances
//! its framework registry and project store own. The engine names the
//! attachment only as the opaque
//! [`ResolverCapabilities::HostAttachment`](crate::resolver_core::ResolverCapabilities)
//! of its capability family; it has no operation on it.

use std::sync::Arc;

use crate::framework::surface_store::FrameworkSurfaceStore;
use crate::typeinfo::framework_surface::{MacroSurfaceDtos, SvelteSurfaceKey, VueSurfaceKey};

/// The Vue adapter's surface DTO store.
pub(crate) type VueSurfaceStore = FrameworkSurfaceStore<VueSurfaceKey, MacroSurfaceDtos>;
/// The Svelte adapter's surface DTO store.
pub(crate) type SvelteSurfaceStore = FrameworkSurfaceStore<SvelteSurfaceKey, MacroSurfaceDtos>;

/// Host-owned state a request reaches beside the engine.
///
/// Built once per host; the store handles are clones of the `Arc`s the
/// framework registry rows own, so every request reads and publishes the one
/// shared instance. A vertical the host's framework options do not admit has
/// no registry row and therefore no store here.
pub struct SessionAttachment {
    vue_surfaces: Option<Arc<VueSurfaceStore>>,
    svelte_surfaces: Option<Arc<SvelteSurfaceStore>>,
    output: crate::output_sinks::OutputLease,
}

impl SessionAttachment {
    /// Select the typed surface stores from the registry rows that own them,
    /// beside a share of the project store's output lease.
    pub(crate) fn new(
        registry: &crate::framework::FrameworkAdapterRegistry,
        output: crate::output_sinks::OutputLease,
    ) -> Self {
        Self {
            output,
            vue_surfaces: typed_surface_store(
                registry,
                &verter_language::FrameworkAdapterId::vue(),
                "Vue",
            ),
            svelte_surfaces: typed_surface_store(
                registry,
                &verter_language::FrameworkAdapterId::svelte(),
                "Svelte",
            ),
        }
    }

    /// The Vue adapter's surface DTO store.
    ///
    /// Panics only when the Vue adapter is not registered — reaching a Vue
    /// surface on such a host is a composition defect.
    pub(crate) fn vue_surfaces(&self) -> &VueSurfaceStore {
        self.vue_surfaces
            .as_deref()
            .expect("the Vue adapter is registered")
    }

    /// The Svelte adapter's surface DTO store.
    ///
    /// Panics only when the Svelte adapter is not registered — reaching a
    /// Svelte surface on such a host is a composition defect.
    pub(crate) fn svelte_surfaces(&self) -> &SvelteSurfaceStore {
        self.svelte_surfaces
            .as_deref()
            .expect("the Svelte adapter is registered")
    }

    /// The host's inert output lease; only a sealed sink capability opens it.
    pub(crate) fn output_lease(&self) -> &crate::output_sinks::OutputLease {
        &self.output
    }
}

/// The ONE downcast at store acquisition from the erased registration row to
/// the adapter's typed store. `None` when the adapter is not registered; a
/// registered row whose store is erased to the wrong concrete type is a build
/// defect and panics.
fn typed_surface_store<K>(
    registry: &crate::framework::FrameworkAdapterRegistry,
    adapter: &verter_language::FrameworkAdapterId,
    label: &str,
) -> Option<Arc<FrameworkSurfaceStore<K, MacroSurfaceDtos>>>
where
    K: Clone + PartialEq + Eq + std::hash::Hash + Send + Sync + 'static,
{
    let row = registry.get(adapter)?;
    Some(
        Arc::clone(&row.surface_store)
            .into_any_arc()
            .downcast()
            .unwrap_or_else(|_| panic!("typed {label} surface store")),
    )
}

/// A capability family whose requests carry the host's [`SessionAttachment`].
pub(crate) trait SessionCapabilities:
    crate::resolver_core::ResolverCapabilities<HostAttachment = SessionAttachment>
{
}

impl<C> SessionCapabilities for C where
    C: crate::resolver_core::ResolverCapabilities<HostAttachment = SessionAttachment>
{
}
