//! Test-support shims for the dispatch DepSignature→fact bridges.
//!
//! `accumulate_dispatch_dep_signature` and `observe_fence_entry` are
//! `pub(crate)` production helpers; integration tests under
//! `crates/verter_session/tests/*.rs` build as a separate crate and
//! cannot reach them directly. These shims expose a discriminating
//! probe of each bridge — the conversion the integration test
//! `tests/dispatch_bridges_convert_project_generation.rs` asserts.
//!
//! The module is gated `cfg(any(test, debug_assertions))` at its
//! `lib.rs` declaration site, so release builds never include it.

use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
use crate::semantic_query::DepSignature;

/// Run `accumulate_dispatch_dep_signature` against `sig` and return
/// the per-request accumulator contents it produced.
///
/// The accumulator is reset before the call and drained after, so the
/// returned `Vec` is exactly the `FactVersionRef` set this one `sig`
/// contributed. Used by `tests/dispatch_bridges_convert_project_generation.rs`
/// to assert the dispatch accumulator bridge converts a
/// `ProjectGeneration` dep and still drops a `RouteGeneration` dep.
pub fn accumulate_dispatch_dep_signature_for_tests(sig: &DepSignature) -> Vec<FactVersionRef> {
    crate::meta_resolve::reset_dispatch_dep_signature_accumulator();
    crate::meta_resolve::accumulate_dispatch_dep_signature(sig);
    crate::meta_resolve::drain_dispatch_dep_signature_accumulator()
}

/// Run `observe_fence_entry` for every `(canonical, version)` pair in
/// `sig` inside a fresh fact-tracer scope and return the finalized
/// observation set.
///
/// Used by `tests/dispatch_bridges_convert_project_generation.rs` to
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
