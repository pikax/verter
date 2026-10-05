//! The host's terminal output sinks and the lease that lends them the engine's
//! output authority.
//!
//! The engine mints one [`OutputAuthority`] per engine, at
//! [`EngineStores::create`](crate::project_semantic_dispatch::engine_resources::EngineStores::create).
//! The host's composition code stores it privately, wrapped in an
//! [`OutputLease`], and attaches a shared lease to every request beside the
//! engine (the request's host attachment). A lease is inert: it opens ONLY
//! inside this module, and only for one of the sealed sink capabilities below.
//! Code that merely holds a lease — or the dispatch, or the request ports —
//! cannot reach the authority.
//!
//! Each terminal output sink owns a tiny private-field capability type whose
//! constructor is visible only within that sink (`mint: pub(in <sink>)`); a
//! non-sink module that plants a mint gets `E0624`. The sinks are:
//! `meta_resolve::projectors::output_sink` (a dedicated terminal submodule, so
//! the parent `projectors`' non-sink helpers cannot mint),
//! `meta_resolve::materialize::field_types`, `typeinfo::raise`,
//! `typeinfo::framework_surface::svelte_exec`,
//! `typeinfo::framework_surface::vue_exec` (whose reachable scope — `vue_exec`
//! plus its normalizer children — is output-only), and
//! `resolver_core::component_meta_query_engine::surface`. [`OutputProjector`]
//! is sealed against a private marker only this module can name, and it is
//! implemented here for exactly those capability types; its
//! [`OutputProjector::authority`] is the one place a lease opens.
//!
//! Trust boundary: the composition code that constructs the engine stores and
//! the attachment, and these sinks, are trusted. The engine names none of
//! them, and its own handles grant no output power.

use std::sync::Arc;

use crate::project_semantic_dispatch::engine_resources::OutputAuthority;
use crate::project_semantic_dispatch::output_materialization::{
    MaterializedOutputTypeExpr, OutputTypeExpr,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{ProjectionReductionContext, SemanticNodeId};

/// The host's private hold on its engine's [`OutputAuthority`].
///
/// Cloning shares the one authority; it never mints another. The lease opens
/// only inside this module.
#[derive(Clone)]
pub(crate) struct OutputLease(Arc<OutputAuthority>);

impl OutputLease {
    /// Take private hold of the authority minted with the host's engine
    /// stores.
    pub(crate) fn new(authority: OutputAuthority) -> Self {
        Self(Arc::new(authority))
    }

    /// The one opening of a lease, reachable only from this module.
    fn authority(&self) -> &OutputAuthority {
        &self.0
    }
}

mod sealed {
    /// Marker [`super::OutputProjector`] is sealed against. Nameable only from
    /// within `output_sinks`.
    pub trait Sealed {}
}

/// The sealed sink capability: a terminal sink's borrowed hold on the engine's
/// output authority, plus the dispatch it projects through.
///
/// Implemented ONLY for the sink capability types registered below. The two
/// boundary methods return sealed carriers, never a bare `TypeExpr`; a sink
/// unwraps them with [`Self::authority`].
pub(crate) trait OutputProjector: sealed::Sealed {
    /// The capability family of the dispatch this projector reads.
    type Caps: crate::session_attachment::SessionCapabilities;

    /// The dispatch this capability projects through.
    fn dispatch(&self) -> &ProjectSemanticDispatch<'_, Self::Caps>;

    /// The engine's output authority, borrowed for this sink.
    fn authority(&self) -> &OutputAuthority;

    /// Plain SHELL raise: materialize `node` into a sealed [`OutputTypeExpr`]
    /// (`None` is the miss signal).
    fn materialize_output_type_expr(&self, node: SemanticNodeId) -> Option<OutputTypeExpr> {
        self.authority()
            .materialize_output_type_expr(self.dispatch(), node)
    }

    /// REDUCE-then-raise: apply `context`, then materialize the reduced node
    /// into a sealed [`MaterializedOutputTypeExpr`].
    fn materialize_reduced_output_type_expr(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> MaterializedOutputTypeExpr {
        self.authority()
            .materialize_reduced_output_type_expr(self.dispatch(), node, context)
    }
}

/// Define a terminal output-sink capability type in its sink module.
///
/// Generates a `pub(crate)` private-field capability struct borrowing the
/// dispatch and the request's [`OutputLease`], with:
/// - a `new()` constructor visible ONLY within the sink module (`$mint_vis`,
///   e.g. `pub(in crate::meta_resolve::projectors::output_sink)`);
/// - `pub(crate)` `dispatch_for_projector()` / `lease_for_projector()`
///   accessors this module's [`OutputProjector`] impls read through (the
///   lease stays inert outside this module);
/// - private fields, so no module can struct-literal-construct it.
///
/// Expanding the macro elsewhere grants nothing: [`OutputProjector`] is
/// implemented only for the exact types registered below.
macro_rules! define_output_capability {
    ($(#[$meta:meta])* $vis:vis struct $name:ident; mint: $mint_vis:vis) => {
        $(#[$meta])*
        $vis struct $name<
            'disp,
            'ctx,
            C: $crate::session_attachment::SessionCapabilities,
        > {
            dispatch: &'disp $crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx, C>,
            lease: &'disp $crate::output_sinks::OutputLease,
        }

        impl<'disp, 'ctx, C: $crate::session_attachment::SessionCapabilities> $name<'disp, 'ctx, C> {
            /// Mint the capability over the request's dispatch. Visible ONLY
            /// within this output-sink module (`$mint_vis`).
            $mint_vis fn new(
                dispatch: &'disp $crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx, C>,
            ) -> Self {
                Self {
                    dispatch,
                    lease: dispatch.host_attachment().output_lease(),
                }
            }

            /// The dispatch this capability projects through.
            pub(crate) fn dispatch_for_projector(
                &self,
            ) -> &$crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx, C> {
                self.dispatch
            }

            /// The request's inert output lease.
            pub(crate) fn lease_for_projector(&self) -> &$crate::output_sinks::OutputLease {
                self.lease
            }
        }
    };
}
pub(crate) use define_output_capability;

/// Register a sink capability type: seal it and implement [`OutputProjector`]
/// by opening its lease.
macro_rules! register_sink {
    ($($cap:ty),+ $(,)?) => {
        $(
            impl<C: crate::session_attachment::SessionCapabilities> sealed::Sealed for $cap {}
            impl<C: crate::session_attachment::SessionCapabilities> OutputProjector for $cap {
                type Caps = C;
                fn dispatch(&self) -> &ProjectSemanticDispatch<'_, C> {
                    self.dispatch_for_projector()
                }
                fn authority(&self) -> &OutputAuthority {
                    self.lease_for_projector().authority()
                }
            }
        )+
    };
}

register_sink!(
    crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap<'_, '_, C>,
    crate::meta_resolve::materialize::MetaResolveFieldTypesOutputCap<'_, '_, C>,
    crate::typeinfo::raise::TypeinfoRaiseOutputCap<'_, '_, C>,
    crate::typeinfo::framework_surface::svelte_exec::TypeinfoSvelteSurfaceOutputCap<'_, '_, C>,
    crate::typeinfo::framework_surface::vue_exec::TypeinfoVueSurfaceOutputCap<'_, '_, C>,
    crate::resolver_core::component_meta_query_engine::MetaQuerySurfaceOutputCap<'_, '_, C>,
);

/// Test-only sink capability: the carrier round-trip and projector suites
/// drive the boundary methods directly over the host's real lease. It exists
/// ONLY in this crate's unit-test build.
#[cfg(test)]
pub(crate) struct TestOutputCap<'disp, 'ctx, C: crate::session_attachment::SessionCapabilities> {
    dispatch: &'disp ProjectSemanticDispatch<'ctx, C>,
    lease: &'disp OutputLease,
}

#[cfg(test)]
impl<'disp, 'ctx, C: crate::session_attachment::SessionCapabilities> TestOutputCap<'disp, 'ctx, C> {
    /// Mint the test capability over `dispatch`.
    pub(crate) fn new(dispatch: &'disp ProjectSemanticDispatch<'ctx, C>) -> Self {
        Self {
            dispatch,
            lease: dispatch.host_attachment().output_lease(),
        }
    }
}

#[cfg(test)]
impl<C: crate::session_attachment::SessionCapabilities> sealed::Sealed
    for TestOutputCap<'_, '_, C>
{
}

#[cfg(test)]
impl<C: crate::session_attachment::SessionCapabilities> OutputProjector
    for TestOutputCap<'_, '_, C>
{
    type Caps = C;
    fn dispatch(&self) -> &ProjectSemanticDispatch<'_, C> {
        self.dispatch
    }
    fn authority(&self) -> &OutputAuthority {
        self.lease.authority()
    }
}

// Owner-seal witness (every profile): a representative non-sink crate type —
// the graph node id this reverse boundary materializes FROM — must never be a
// sink.
static_assertions::assert_not_impl_any!(SemanticNodeId: OutputProjector);
// A lease shares the one minted authority; it is never default-constructed.
static_assertions::assert_not_impl_any!(OutputLease: Default);

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    use super::{OutputProjector, TestOutputCap};
    use crate::project_semantic_dispatch::engine_resources::EngineStores;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::{SemanticNodeData, SemanticNodeId};

    /// Authority minted for a different engine — even a freshly created one
    /// with the same configuration — must not materialize through a dispatch
    /// over the host's live engine.
    #[test]
    #[should_panic(expected = "output authority used with a dispatch over a different engine")]
    fn foreign_engine_authority_cannot_materialize_through_a_live_dispatch() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let dispatch = ProjectSemanticDispatch::new(&host);
        let (_stores, foreign) = EngineStores::create(
            None,
            verter_session_query::retention::StoreAccount::default(),
            &Arc::new(AtomicU64::new(0)),
        );
        let _ = foreign.materialize_output_type_expr(&dispatch, SemanticNodeId(0));
    }

    /// The host's own lease opens to the authority of its live engine.
    #[test]
    fn host_lease_materializes_through_its_own_engine() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let node =
            host.project_type_store()
                .semantic_graph()
                .intern_node(SemanticNodeData::Primitive(
                    crate::semantic_query::PrimitiveKind::String,
                ));
        let dispatch = ProjectSemanticDispatch::new(&host);
        let cap = TestOutputCap::new(&dispatch);
        let raised = cap
            .materialize_output_type_expr(node)
            .expect("a live primitive node raises")
            .into_type_expr(cap.authority());
        assert!(matches!(
            raised,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ));
    }
}
