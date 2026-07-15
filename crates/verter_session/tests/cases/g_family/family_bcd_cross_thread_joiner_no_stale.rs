//! Cross-thread joiner scenarios: a cold compute on thread A and a
//! concurrent join on thread B both end up observing the producer's
//! cold-build facts.
//!
//! For the AppConfigNoOverrideProofDb (which uses a DashMap
//! rather than the cooperative `execute_cooperative` flow used by
//! MemoEntry), the cross-thread story degenerates to: two threads
//! racing on the same key may both cold-compute, but the entry
//! published in the cache stays valid for both. Discrimination is
//! that the published `fact_dep_signature` remains consistent
//! across thread observations.
//!
//! The producer is wired and concurrent calls both observe a
//! consistent admitted entry.

use std::sync::Arc;
use std::thread;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
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
fn cross_thread_concurrent_compute_admits_consistent_entry() {
    let canonical = "/proj/no-app-config.ts";
    let host = make_host_with_file(canonical, "export type Foo = { theme: string };");

    let key: verter_session::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(canonical), Arc::from("button"));

    let host_a = Arc::clone(&host);
    let key_a = key.clone();
    let handle_a = thread::spawn(move || {
        app_config_no_override_proof_get_or_compute_for_tests(&host_a, &key_a)
    });
    let host_b = Arc::clone(&host);
    let key_b = key.clone();
    let handle_b = thread::spawn(move || {
        app_config_no_override_proof_get_or_compute_for_tests(&host_b, &key_b)
    });

    let proof_a = handle_a.join().expect("thread A panicked");
    let proof_b = handle_b.join().expect("thread B panicked");

    let proof_a = proof_a.expect("thread A must yield a proof");
    let proof_b = proof_b.expect("thread B must yield a proof");

    // Discrimination: both threads observe the same admitted
    // fact_dep_signature length and elements. (Whether they both
    // cold-computed or one served the warm hit depends on the
    // race, but the published entry is consistent — the producer's
    // tracer scope captured the same observation set in either
    // case.)
    assert_eq!(
        proof_a.fact_dep_signature.len(),
        proof_b.fact_dep_signature.len(),
        "both threads must observe a consistent admitted signature length"
    );
    let set_a: std::collections::HashSet<_> = proof_a.fact_dep_signature.iter().collect();
    let set_b: std::collections::HashSet<_> = proof_b.fact_dep_signature.iter().collect();
    assert_eq!(
        set_a, set_b,
        "both threads must observe identical admitted fact sets"
    );
}
