//! The `TypeInfoSurface` adapter over the engine-owned `SurfaceResolution`.
//!
//! The engine classifies a surface resolution into a member-domain fact
//! ([`SurfaceResolution::into_fact`]); this adapter only adopts that fact for
//! the public accessors that return `Option<surface>`. Nothing is classified
//! here: the answer's availability is the fact's arm, and the legacy
//! completeness signal is derived from the adopted fact's status, never from
//! an ambient discharge.

use verter_type_engine::request_context::fold_result_completeness;
use verter_type_engine::semantic_query::surface_resolution::SurfaceResolution;
use verter_type_engine::semantic_query::{FactResult, MemberDomain};

/// Adopt a surface resolution as the surface it publishes.
///
/// - Complete closed or open domains yield their surface.
/// - A complete `NoSurface` yields `None` and records nothing.
/// - An approximate domain yields its presence-only subset, and an
///   unavailable one yields `None`; both record the fact's causes first, so
///   a subset never passes as the complete surface.
pub(crate) fn adopt_surface(
    resolution: SurfaceResolution<super::surface::TypeInfoSurface>,
) -> Option<super::surface::TypeInfoSurface> {
    adopt_surface_claim(resolution).0
}

/// Adopt a surface and keep the producer claim, including when the surface
/// itself is absent. An unavailable or approximate claim must not disappear
/// just because no member surface was published.
pub(crate) fn adopt_surface_claim(
    resolution: SurfaceResolution<super::surface::TypeInfoSurface>,
) -> (
    Option<super::surface::TypeInfoSurface>,
    verter_session_query::analysis::component_meta::MetadataFactGuarantee,
) {
    let fact: FactResult<MemberDomain<super::surface::TypeInfoSurface>> = resolution.into_fact();
    fold_result_completeness(fact.status().completeness());
    let mut guarantee = metadata_guarantee(
        fact.status(),
        matches!(
            &fact,
            FactResult::Complete(MemberDomain::Open(_))
                | FactResult::Approximate {
                    value: MemberDomain::Open(_),
                    ..
                }
        ),
    );
    let surface = match fact {
        FactResult::Complete(MemberDomain::NoSurface)
        | FactResult::Unavailable { .. }
        | FactResult::Approximate {
            value: MemberDomain::NoSurface,
            ..
        } => None,
        FactResult::Complete(MemberDomain::Closed(surface) | MemberDomain::Open(surface))
        | FactResult::Approximate {
            value: MemberDomain::Closed(surface) | MemberDomain::Open(surface),
            ..
        } => {
            let mut surface = surface;
            guarantee.open |= surface.has_index_signature || surface.keyspace.is_some();
            surface.domain_guarantee = guarantee.clone();
            Some(surface)
        }
    };
    (surface, guarantee)
}

/// Context-free output projection of a demanded fact's own status.
pub(crate) fn metadata_guarantee(
    status: verter_type_engine::semantic_query::FactStatus,
    open: bool,
) -> verter_session_query::analysis::component_meta::MetadataFactGuarantee {
    use verter_type_engine::semantic_query::{FactStatus, PartialReason};
    let (kind, reasons) = match status {
        FactStatus::Complete => (1, None),
        FactStatus::Approximate(causes) => (2, Some(causes)),
        FactStatus::Unavailable(causes) => (3, Some(causes)),
    };
    let causes = reasons
        .map(|reasons| {
            reasons
                .get()
                .iter()
                .map(|reason| match reason {
                    PartialReason::BudgetExceeded => 1,
                    PartialReason::Cancelled => 2,
                    PartialReason::SupersededGeneration => 3,
                    PartialReason::UnstableState => 4,
                    PartialReason::SamePathRecursion => 5,
                    PartialReason::WalkerFatal => 6,
                    PartialReason::Propagated => 7,
                    PartialReason::DeferredEvaluationLimit => 8,
                    PartialReason::StructuralFactDemandLimit => 9,
                    PartialReason::SemanticQueryFault => 10,
                    PartialReason::MissingSemanticNodeData => 11,
                    PartialReason::ProjectionWorkLimit => 12,
                    PartialReason::ConnectedQueryDepthLimit => 13,
                    PartialReason::MissingDependency => 14,
                    PartialReason::FlowReturnUninferred => 15,
                    PartialReason::FlowReturnUnverified => 16,
                    PartialReason::FlowReturnNoSurface => 17,
                    PartialReason::UndecidedConditional => 18,
                    PartialReason::ConnectedMemoryLimit => 19,
                    PartialReason::OperationBudget => 20,
                })
                .collect()
        })
        .unwrap_or_default();
    verter_session_query::analysis::component_meta::MetadataFactGuarantee { kind, causes, open }
}
