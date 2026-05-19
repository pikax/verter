//! Dependency-fact validator abstraction for top-level semantic queries.
//!
//! Top-level public entry points that publish to shared caches
//! (`get_component_meta`, exported-type queries that return a final
//! payload) must not warm a shared cache with a result that was torn by
//! a mid-flight workspace change. The publish-side discipline is the
//! completion-fence rule: record the dependency facts a result touched,
//! revalidate them against live host state before publishing, retry a
//! bounded number of times, and never publish a result whose facts no
//! longer match.
//!
//! That discipline is implemented by the cooperative-admission publish
//! path together with [`HostFenceValidator`](crate::host_manage), which
//! revalidates a cached value's recorded dependency signature
//! (`fact_dep_signature` / `ReadSetSignature` carrier) against the live
//! `ProjectTypeStore`. This module owns only the validator abstraction
//! that publish path consults: [`FenceValidator`]. The concrete host
//! validator (`HostFenceValidator`) implements it by checking live
//! whole-hashes, route generations, and the project generation counter.

use crate::semantic_query::DepVersion;

/// Validator abstraction supplied by the publish path. The concrete
/// [`HostFenceValidator`](crate::host_manage) consults live
/// whole-hashes, route generations, and the project generation counter
/// to confirm a recorded dependency fact still matches the host.
pub trait FenceValidator {
    /// Return `true` when `canonical_id`'s live state still matches the
    /// recorded `version` — i.e. the dependency fact has not shifted
    /// since it was observed.
    fn validate(&self, canonical_id: &str, version: &DepVersion) -> bool;
}
