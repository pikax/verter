//! The host's request attachment: composition-owned state that travels beside
//! the engine through the request port, never through the engine binding.
//!
//! The engine's binding carries only the engine's own resources. Host-owned
//! result stores — the per-framework surface DTO stores — the host's inert
//! output lease, and the engine's surface-claim authority are reached by
//! session code through this attachment, which the host builds once at
//! construction from the SAME shared instances its framework registry and
//! project store own. The engine names the
//! attachment only as the opaque
//! [`ResolverCapabilities::HostAttachment`](verter_type_engine::resolver_core::ResolverCapabilities)
//! of its capability family; it has no operation on it.

use std::sync::Arc;

use verter_type_engine::semantic_query::surface_resolution::SurfaceClaimAuthority;

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
    surface_claims: Arc<SurfaceClaimAuthority>,
}

impl SessionAttachment {
    /// Select the typed surface stores from the registry rows that own them,
    /// beside a share of the project store's output lease and surface-claim
    /// authority.
    pub(crate) fn new(
        registry: &crate::framework::FrameworkAdapterRegistry,
        output: crate::output_sinks::OutputLease,
        surface_claims: Arc<SurfaceClaimAuthority>,
    ) -> Self {
        Self {
            output,
            surface_claims,
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

    /// The Vue adapter's surface DTO store; `None` when the host does not
    /// admit the Vue adapter. Callers refuse the demand instead of computing.
    pub(crate) fn vue_surfaces(&self) -> Option<&VueSurfaceStore> {
        self.vue_surfaces.as_deref()
    }

    /// The Svelte adapter's surface DTO store; `None` when the host does not
    /// admit the Svelte adapter. Callers refuse the demand instead of
    /// computing.
    pub(crate) fn svelte_surfaces(&self) -> Option<&SvelteSurfaceStore> {
        self.svelte_surfaces.as_deref()
    }

    /// The host's inert output lease; only a sealed sink capability opens it.
    pub(crate) fn output_lease(&self) -> &crate::output_sinks::OutputLease {
        &self.output
    }

    /// The engine's surface-claim authority, lent to the session code that
    /// publishes resolved surfaces. Reachable only inside this crate.
    pub(crate) fn surface_claims(&self) -> &SurfaceClaimAuthority {
        &self.surface_claims
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
    verter_type_engine::resolver_core::ResolverCapabilities<HostAttachment = SessionAttachment>
{
}

impl<C> SessionCapabilities for C where
    C: verter_type_engine::resolver_core::ResolverCapabilities<HostAttachment = SessionAttachment>
{
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::typeinfo::framework_surface::MacroDtosRefusal;
    use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
    use crate::types::{HostConfig, UpsertRequest};
    use crate::VerterHost;
    use verter_language::FileLanguage;
    use verter_session_query::analysis::types::AnalyzedMacroKind;

    fn host_admitting(name: &str) -> VerterHost {
        VerterHost::new_standalone(HostConfig {
            framework: crate::framework::FrameworkOptions::admitting_names([name])
                .expect("the vertical is composed"),
            ..HostConfig::default()
        })
    }

    fn props_request(owner: &str) -> VueMacroSurfaceRequest {
        VueMacroSurfaceRequest {
            owner_canonical: Arc::from(owner),
            macro_index: 0,
            macro_kind: AnalyzedMacroKind::DefineProps,
            root_identity: [0u8; 16],
            level: TypeInfoQueryLevel::FullMetadata,
        }
    }

    fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(source),
                file_language: FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert");
    }

    /// A host that does not admit the Vue adapter refuses a `.vue` macro DTO
    /// demand with a typed refusal — never a panic on the absent store and
    /// never a complete empty bundle.
    #[test]
    fn vue_macro_dtos_on_a_host_without_vue_is_a_typed_refusal() {
        let host = host_admitting("svelte");
        upsert_ts(
            &host,
            "/w/props.ts",
            "const p = defineProps<{ x: string }>();\n",
        );
        assert_eq!(
            host.vue_macro_dtos(&props_request("/w/props.ts")).err(),
            Some(MacroDtosRefusal::VueNotAdmitted)
        );
    }

    /// The same demand on a Vue-admitting host is served.
    #[test]
    fn vue_macro_dtos_on_a_vue_host_is_served() {
        let host = host_admitting("vue");
        upsert_ts(
            &host,
            "/w/props.ts",
            "const p = defineProps<{ x: string }>();\n",
        );
        assert!(host.vue_macro_dtos(&props_request("/w/props.ts")).is_ok());
    }
}
