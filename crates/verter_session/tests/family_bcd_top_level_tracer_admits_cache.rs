//! Block 1.H RED test — `install_fact_tracer` works at top-level
//! (no outer tracer present) and the cache admits a non-empty
//! `fact_dep_signature`.
//!
//! Pre-Block-1.H the Family B/C/D caches' cold-compute closures
//! were not wrapped in `install_fact_tracer`; admitted entries
//! could carry empty/legacy-derived signatures only. Post-Block-1.H
//! the AppConfigNoOverrideProofDb producer wraps its cold compute
//! with `install_fact_tracer`; the published entry's
//! `fact_dep_signature` carries the producer's `FileWholeHash`
//! observation captured by the tracer.
//!
//! Discrimination: a successful cold compute on a no-AppConfig
//! file (a) advances the `app_config_proof_fact_tracer_installs`
//! counter by exactly 1, (b) publishes the proof with a non-empty
//! `fact_dep_signature` containing the FileWholeHash fact.

use std::sync::Arc;

use verter_session::for_tests::app_config_no_override_proof_get_or_compute_for_tests;
use verter_session::resolver_core::FactVersionRef;
use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

fn make_host_with_file(canonical: &str, source: &str) -> Arc<VerterHost> {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.analyze_with_audit(canonical);
    host
}

#[test]
fn tracer_works_at_top_level_and_admits_non_empty_signature() {
    let canonical = "/proj/no-app-config.ts";
    let host = make_host_with_file(canonical, "export type Foo = { theme: string };");

    let installs_before = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);

    let key: verter_session::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(canonical), Arc::from("button"));

    let proof = app_config_no_override_proof_get_or_compute_for_tests(&host, &key)
        .expect("file without `interface AppConfig` must yield a published proof");

    let installs_after = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        installs_after - installs_before,
        1,
        "top-level cold compute must advance the counter by exactly 1"
    );
    assert!(
        !proof.fact_dep_signature.is_empty(),
        "published proof must carry a non-empty fact_dep_signature"
    );
    assert!(
        proof.fact_dep_signature.iter().any(|f| matches!(
            f,
            FactVersionRef::FileWholeHash { canonical_id, .. } if canonical_id == canonical
        )),
        "fact_dep_signature must contain the producer's FileWholeHash observation for the decl canonical. \
         Got: {:?}",
        proof.fact_dep_signature,
    );
}
