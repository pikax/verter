//! Former directory-based integration target
//! `tests/component_meta_audit_corpus` — Cargo auto-discovered
//! `<dir>/main.rs` as its entry root, so the `Main.vue` corpus case
//! ran in TWO binaries: once here (the directory target) and once via
//! `corpus_audit_tests`'s `mod main`. Consolidated into the `main`
//! binary, this module reproduces the directory-target run so the
//! test surfaces at the SAME internal path
//! `component_meta_audit_corpus::corpus_audit_main_…` the former
//! target had; `corpus_audit_tests::main::corpus_audit_main_…` keeps
//! the second former path via the generated sibling `main.rs`.
//!
//! The body mirrors the generated `main.rs` for `/Main.vue`. It is a
//! hand-maintained consolidation shim (excluded from the generator's
//! parity check and preserved across regeneration) — `include!`ing
//! `main.rs` is not possible because the generated file leads with
//! its own `//!` inner-doc, which `include!` cannot relocate to a
//! module root. Keep this body in step with the generated `/Main.vue`
//! assertion posture.

use verter_session::audited_request::{AuditedRequest, AuditedRequestError};

#[test]
fn corpus_audit_main_produces_audit_record_or_documents_skip() {
    let src = include_str!("fixtures/Main.vue");
    let result = AuditedRequest::builder()
        .files([("/Main.vue", src)])
        .resolve_component_meta("/Main.vue");

    match result {
        Ok((_, _, record)) => {
            assert_eq!(
                record.canonical_id, "/Main.vue",
                "audit record must identify the requested canonical",
            );
            // Hermetic `AuditedRequest` always enables footprint
            // capture — the miner MUST attach a footprint on
            // resolution success. Discriminating: fails if capture
            // wiring regresses, or if a future refactor accidentally
            // drops the miner call for this code path. Would NOT fail
            // for benign partial analysis (missing deps in the hermetic
            // setup) because the footprint attaches regardless of
            // analysis depth.
            assert!(
                record.footprint.is_some(),
                "hermetic AuditedRequest must attach Some(footprint) on resolution success",
            );
        }
        Err(AuditedRequestError::ResolutionFailed) => {
            // Benign: hermetic fixture lacks transitive deps, so
            // `get_component_meta_with_resolution` returned
            // `None`. This is the ONLY error variant we treat as
            // skip — every other variant is a genuine regression
            // (nested-audit guard, multi-request counter, audit
            // record missing from store, config validation).
            eprintln!(
                "corpus_audit_main: hermetic resolution returned None (missing deps) — documenting skip",
            );
        }
        Err(other) => panic!(
            "corpus_audit_main: unexpected audit error — this indicates an audit-wiring regression, not a hermetic-dep gap: {other:?}",
        ),
    }
}
