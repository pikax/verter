//! Test-support shims for the dispatch DepSignature→fact bridges.
//!
//! `dep_signature_to_fact_signature` and `observe_fence_entry` are
//! crate-private production helpers; integration tests under
//! `crates/verter_session/tests/*.rs` build as a separate crate and
//! cannot reach them directly. These shims expose a discriminating
//! probe of each bridge — the conversion the integration test
//! `tests/cases/g_misc0/dispatch_bridges_convert_project_generation.rs` asserts.
//!
//! The module is gated `cfg(any(test, feature = "test-support"))` at its
//! `lib.rs` declaration site, so release builds never include it.

use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
use crate::semantic_query::DepSignature;

/// Convert a dispatch signature through the production bridge.
pub fn dispatch_dep_signature_facts_for_tests(sig: &DepSignature) -> Vec<FactVersionRef> {
    crate::fact_signature_helpers::dep_signature_to_fact_signature(sig)
}

/// Run `observe_fence_entry` for every `(canonical, version)` pair in
/// `sig` inside a fresh fact-tracer scope and return the finalized
/// observation set.
///
/// Used by `tests/cases/g_misc0/dispatch_bridges_convert_project_generation.rs` to
/// assert the fence-entry bridge converts a `ProjectGeneration` dep
/// into a `FactVersionRef::ProjectGeneration` observation and still
/// emits no observation for a `RouteGeneration` dep.
pub fn observe_fence_entry_for_tests(
    host: &crate::VerterHost,
    sig: &DepSignature,
) -> FactReadSetFinalise {
    let (_, read_set) = host.with_fact_tracer(|| {
        for (canonical, version) in sig.iter() {
            crate::component_meta_audit::observe_fence_entry(host, canonical, version);
        }
    });
    read_set.finalise()
}
