//! Facts/carrier-only context for session-side adapters.
//!
//! [`FrameworkAdapterCtx::carrier_for`] reaches an adapter's typed carrier
//! through its registration's [`CarrierLeg`](crate::framework::registry::CarrierLeg)
//! opener — a monomorphic fn pointer installed at registry-build time, no
//! capability token.

use std::sync::Arc;

use verter_language::CarrierParse;
use verter_semantic::analysis::framework_facts::FrameworkScriptFactPayload;

use crate::framework::registry::FrameworkRegistration;
use crate::VerterHost;

/// The facts/carrier-only context the framework-surface executor hands a
/// [`FrameworkSurfaceAdapter`](crate::typeinfo::framework_surface::FrameworkSurfaceAdapter).
///
/// The ctx exposes EXACTLY two operations — [`Self::carrier_for`] and
/// [`Self::script_facts_for`] — pinned by the `framework_adapter_ctx_closed_surface`
/// guard. Neither op resolves types, indexes a file, runs OXC, calls
/// `ProjectSemanticDispatch`, or reads a `StoreView`: the adapter sees only its
/// own typed parse carrier and its own resolved script facts. The ctx holds the
/// adapter's REGISTRATION row (whose `carrier: Option<CarrierLeg>` may be
/// `None`), never a non-optional token — a carrier-less adapter's
/// [`Self::carrier_for`] returns `None` cleanly, never a forged token.
pub struct FrameworkAdapterCtx<'a> {
    registration: &'a FrameworkRegistration,
    host: &'a VerterHost,
}

impl<'a> FrameworkAdapterCtx<'a> {
    /// Construct the ctx over an adapter registration row and the host.
    ///
    /// Module-private to the framework substrate: the framework-surface
    /// executor builds the ctx around each adapter's registration row, and
    /// adapter code only consumes the two closed-surface ops. Exercised by the
    /// ctx closed-surface tests until the executor body consumes it directly.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(registration: &'a FrameworkRegistration, host: &'a VerterHost) -> Self {
        Self { registration, host }
    }

    /// The adapter's typed parse carrier for `canonical`, or `None`.
    ///
    /// Returns `None` cleanly when the adapter has no carrier leg (a
    /// carrier-less framework), when `canonical` carries no framework parse
    /// artifact, or when the artifact's carrier is not a `T`. Drives the
    /// parse-domain artifact materialization internally (ensure-loaded → read
    /// the `framework_parse` slot → the leg's opener, then the typed
    /// downcast) and hands back ONLY the typed carrier — never the neutral
    /// `FrameworkParseArtifact`, never `IndexedReady`.
    pub fn carrier_for<T: CarrierParse>(&self, canonical: &str) -> Option<Arc<T>> {
        let leg = self.registration.carrier.as_ref()?;
        let (_source, framework_parse, _whole_hash) = self.host.current_eval_state(canonical)?;
        let artifact = framework_parse?;
        (leg.open)(&artifact)?
            .__verter_as_any_arc()
            .downcast::<T>()
            .ok()
    }

    /// The adapter's resolved framework script-fact evidence of type `T` for
    /// `canonical`.
    ///
    /// Drives the resolved-validation half of the script-fact seam on demand:
    /// it consults the registration's active providers while preserving exact,
    /// partial, unavailable, and not-applicable outcomes.
    pub fn script_facts_for<T: FrameworkScriptFactPayload>(
        &self,
        canonical: &str,
    ) -> crate::framework::script_facts::ScriptFactEvidence<T> {
        crate::framework::script_facts::resolve_script_facts::<T>(
            self.host,
            self.registration,
            canonical,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use crate::framework::descriptor::vue_descriptor;
    use crate::framework::registry::{FrameworkRegistration, SurfaceRegistration};
    use crate::framework::surface_store::{ErasedFrameworkSurfaceStore, FrameworkSurfaceStore};

    /// A fixture carrier type — its `CarrierParse` impl proves a carrier-less
    /// ctx never opens it.
    struct FixtureCarrier;
    impl CarrierParse for FixtureCarrier {
        fn __verter_as_any(&self) -> &dyn Any {
            self
        }
        fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    /// A fixture resolved-fact payload — its presence proves `script_facts_for`
    /// answers `None` for a provider-less registration.
    #[derive(Debug)]
    struct FixturePayload;
    impl FrameworkScriptFactPayload for FixturePayload {
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    fn fixture_store() -> Arc<dyn ErasedFrameworkSurfaceStore> {
        Arc::new(FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new())
    }

    /// A carrier-LESS registration row (`carrier: None`).
    fn carrier_less_registration() -> FrameworkRegistration {
        FrameworkRegistration {
            descriptor: vue_descriptor(),
            carrier: None,
            synth: None,
            api_projector: None,
            script_fact_providers: Vec::new(),
            surface: SurfaceRegistration::Deferred,
            surface_store: fixture_store(),
        }
    }

    #[test]
    fn carrier_less_ctx_returns_none_without_a_leg() {
        let host = VerterHost::new_standalone(crate::HostConfig::default());
        let registration = carrier_less_registration();
        let ctx = FrameworkAdapterCtx::new(&registration, &host);
        // The carrier-less leg returns None BEFORE touching the host — no
        // panic.
        assert!(ctx.carrier_for::<FixtureCarrier>("/whatever.vue").is_none());
    }

    #[test]
    fn script_facts_for_returns_not_applicable_without_a_provider() {
        let host = VerterHost::new_standalone(crate::HostConfig::default());
        let registration = carrier_less_registration();
        let ctx = FrameworkAdapterCtx::new(&registration, &host);
        // No provider registered ⇒ no resolved facts (the honest answer, not a
        // fabricated empty payload).
        assert!(matches!(
            ctx.script_facts_for::<FixturePayload>("/whatever.vue"),
            crate::framework::script_facts::ScriptFactEvidence::NotApplicable(_)
        ));
    }
}
