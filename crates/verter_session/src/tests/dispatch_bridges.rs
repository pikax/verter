//! Test-support shim for the dispatch DepSignature→fact bridge.
//!
//! `dep_signature_to_fact_signature` is a crate-private production
//! helper; integration tests under `crates/verter_session/tests/*.rs`
//! build as a separate crate and cannot reach it directly. This shim
//! exposes a discriminating probe of the bridge — the conversion the
//! integration test
//! `tests/cases/g_misc0/dispatch_bridges_convert_project_generation.rs` asserts.
//!
//! The module is gated `cfg(any(test, feature = "test-support"))` at its
//! `lib.rs` declaration site, so release builds never include it.

use crate::resolver_core::FactVersionRef;
use crate::semantic_query::DepSignature;

/// Convert a dispatch signature through the production bridge.
pub fn dispatch_dep_signature_facts_for_tests(sig: &DepSignature) -> Vec<FactVersionRef> {
    crate::fact_signature_helpers::dep_signature_to_fact_signature(sig)
}
