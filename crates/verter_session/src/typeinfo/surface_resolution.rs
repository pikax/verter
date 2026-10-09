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
pub(crate) fn adopt_surface<T>(resolution: SurfaceResolution<T>) -> Option<T> {
    let fact: FactResult<MemberDomain<T>> = resolution.into_fact();
    fold_result_completeness(fact.status().completeness());
    match fact {
        FactResult::Complete(MemberDomain::NoSurface) | FactResult::Unavailable { .. } => None,
        FactResult::Complete(MemberDomain::Closed(surface) | MemberDomain::Open(surface))
        | FactResult::Approximate {
            value: MemberDomain::Closed(surface) | MemberDomain::Open(surface),
            ..
        } => Some(surface),
        FactResult::Approximate {
            value: MemberDomain::NoSurface,
            ..
        } => None,
    }
}
