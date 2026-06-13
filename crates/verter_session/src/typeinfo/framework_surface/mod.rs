#![deny(missing_docs)]
//! The framework-surface executor module.
//!
//! Home of the executor's CLOSED plan + result vocabulary
//! ([`plan`], [`results`]), the executor body, the relocated Vue resolution
//! delegates, and the first wire `SemanticTypeGraph` encoder. The plan/result
//! vocabulary is framework-neutral: an adapter PLANS
//! typed [`plan::PlannedDemand`]s and NORMALIZES the executor-resolved
//! [`plan::ResolvedSurfaces`] into per-kind [`results::MacroSurfaceDtos`].

mod executor;
mod graph_export;
pub mod plan;
pub mod results;
pub mod vue_exec;

pub use executor::FRAMEWORK_SURFACE_AUDIT_OPERATION;

use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;

use crate::framework::ctx::FrameworkAdapterCtx;
use crate::framework::descriptor::FrameworkAdapterDescriptor;

pub use plan::{
    ComponentExport, FrameworkSurfacePlan, MacroPayloadSelector, PlannedDemand, PlannedResolve,
    ResolvedComponentSelector, ResolvedDemand, ResolvedItem, ResolvedSurfaces, TypeNodeHandle,
};
pub use results::{
    EmitsSurface, ExposeSurface, MacroSurfaceDtos, ModelBinding, ModelSurface, NamedTypeMember,
    NormalizedSurface, NormalizedSurfaces, OptionsSurface, PropsSurface, ResolvedMacroPayload,
    ResolvedOutcome,
};

/// One framework's plan/normalize adapter.
///
/// An adapter PLANS its component surfaces as typed [`PlannedDemand`]s and
/// NORMALIZES the executor-resolved [`ResolvedSurfaces`] into per-kind
/// [`NormalizedSurfaces`]. The adapter reads NOTHING but the facts/carrier-only
/// [`FrameworkAdapterCtx`] — it never resolves types, indexes a file, or calls
/// `ProjectSemanticDispatch`; the executor owns resolution. Planning is pure
/// selector and requested-kind data work; normalization is a pure
/// resolved-data → DTO transform.
pub trait FrameworkSurfaceAdapter: Send + Sync {
    /// The adapter's static descriptor row.
    fn descriptor(&self) -> &FrameworkAdapterDescriptor;

    /// Plan the typed demands for the requested surface kinds against the
    /// resolved component selector.
    fn plan_surfaces(
        &self,
        ctx: &FrameworkAdapterCtx<'_>,
        selector: &ResolvedComponentSelector,
        requested: &[FrameworkSurfaceKind],
    ) -> FrameworkSurfacePlan;

    /// Fold the executor-resolved surfaces into per-kind normalized DTO
    /// bundles. A pure resolved-data → DTO transform: no dispatch, no source
    /// access.
    fn normalize(
        &self,
        ctx: &FrameworkAdapterCtx<'_>,
        resolved: ResolvedSurfaces,
    ) -> NormalizedSurfaces;
}

/// The Vue adapter's typed [`crate::framework::surface_store::FullKey`]
/// remainder.
///
/// The four framework-neutral identity columns (`kind`, `query_level`,
/// `canonical`, `owner_whole_hash`) live on `FullKey`; this is the Vue
/// adapter's typed remainder — the macro index + kind that disambiguates one
/// `.vue`'s macro slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VueSurfaceKey {
    /// Stable index of the macro in the SFC's analysis snapshot.
    pub macro_index: usize,
    /// The macro kind the cached DTO bundle was normalized for.
    pub macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
}
