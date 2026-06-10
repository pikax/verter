//! Two sequential cold computes on the same host MUST NOT serve a
//! stale entry from a prior tracer's scope.
//!
//! An inner cold compute that runs inside an outer tracer must not
//! have its empty/partial signature exposed to a SUBSEQUENT
//! independent request after the outer scope completes.
//!
//! A design where the Family B/C/D caches could admit entries observed
//! inside an outer tracer's scope would let later requests see stale
//! entries. Instead, the producer installs its OWN tracer per cold
//! compute; admitted entries reflect ONLY the producer's observations,
//! not any outer tracer's drift.
//!
//! Discrimination: a request driven INSIDE an outer tracer
//! scope and a request driven OUTSIDE one must produce
//! `fact_dep_signature`s that are SUBSTRATE-EQUIVALENT
//! (the producer is fact-scoped to its own cold compute).
//! The second request's signature must NOT inherit the outer
//! tracer's drift.

use std::sync::Arc;

use verter_session::for_tests::{
    app_config_no_override_proof_get_or_compute_for_tests, install_fact_tracer_for_tests,
};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn make_host_with_file(canonical: &str, source: &str) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.analyze_with_audit(canonical);
    host
}

#[test]
fn second_request_after_outer_tracer_scope_does_not_serve_stale_entry() {
    let canonical = "/proj/no-app-config.ts";
    let host = make_host_with_file(canonical, "export type Foo = { theme: string };");

    let key: verter_session::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(canonical), Arc::from("button"));

    // Request 1: drive the producer inside an outer tracer scope.
    let installs_before_r1 = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let (proof_r1_opt, _outer_finalise) = install_fact_tracer_for_tests(&host, || {
        app_config_no_override_proof_get_or_compute_for_tests(&host, &key)
    });
    let installs_after_r1 = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let proof_r1 = proof_r1_opt.expect("first request must yield a proof");
    assert_eq!(
        installs_after_r1 - installs_before_r1,
        1,
        "request 1 must run a cold compute (counter +1)"
    );

    // Request 2: drive the producer OUTSIDE any outer tracer scope.
    // The producer must serve the same cached entry (warm hit) and
    // NOT cold-recompute — the outer-tracer-scope from request 1 did
    // NOT influence the admitted entry's signature.
    let installs_before_r2 = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    let proof_r2 = app_config_no_override_proof_get_or_compute_for_tests(&host, &key)
        .expect("second request must yield a proof");
    let installs_after_r2 = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        installs_after_r2, installs_before_r2,
        "request 2 must hit the warm cache (no cold recompute, no counter advance)"
    );

    // Discrimination: the admitted entry's fact_dep_signature was
    // captured by the PRODUCER's tracer (request 1's outer scope
    // contributed nothing to it). Both request 1 and request 2
    // observe the same signature.
    assert_eq!(
        proof_r1.fact_dep_signature.len(),
        proof_r2.fact_dep_signature.len(),
        "request 1 and request 2 must observe the same admitted signature length"
    );
    for (f1, f2) in proof_r1
        .fact_dep_signature
        .iter()
        .zip(proof_r2.fact_dep_signature.iter())
    {
        assert_eq!(
            f1, f2,
            "request 1 and request 2 must observe identical admitted facts"
        );
    }
}
