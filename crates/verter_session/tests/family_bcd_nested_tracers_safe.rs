//! Block 1.H RED test — nested `install_fact_tracer` calls fan facts
//! into both layers via the TLS tracer stack.
//!
//! Pre-Block-1.H the 5 Family B/C/D caches
//! (`MaterializeStructureDb`, `RefCycleResultDb`, `MemoEntry`,
//! `AppConfigNoOverrideProofDb`, `OwnerImportSurfaceDb`) had no
//! `install_fact_tracer` wiring on their cold-compute closures. A
//! nested-call test against the pre-tree would not even compile
//! because the production producers did not invoke
//! `install_fact_tracer`.
//!
//! Post-Block-1.H the producers wrap their cold builds with
//! `install_fact_tracer`. This test installs an OUTER tracer, then
//! drives the AppConfigNoOverrideProofDb producer (which installs
//! its OWN inner tracer). The outer tracer must observe the
//! inner's facts via TLS-stack fan-out.
//!
//! Discrimination: a single fact observed inside the inner cold
//! compute must appear in BOTH the outer tracer's finalised
//! signature AND the inner producer's published
//! fact_dep_signature.

use std::sync::Arc;

use verter_session::for_tests::{
    app_config_no_override_proof_get_or_compute_for_tests, install_fact_tracer_for_tests,
};
use verter_session::resolver_core::FactReadSetFinalise;
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
fn install_nests_safely_outer_observes_inner_cold_facts() {
    let canonical = "/proj/no-app-config.ts";
    let host = make_host_with_file(canonical, "export type Foo = { theme: string };");

    let installs_before = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);

    let key: verter_session::app_config_proof_db::AppConfigNoOverrideProofKey =
        (Arc::from(canonical), Arc::from("button"));

    // Outer tracer installs first. The inner producer call installs
    // its own tracer inside this outer scope.
    let ((), outer_finalise) = install_fact_tracer_for_tests(&host, || {
        let proof = app_config_no_override_proof_get_or_compute_for_tests(&host, &key);
        assert!(
            proof.is_some(),
            "file without `interface AppConfig` must yield a published proof"
        );
    });

    let installs_after = host
        .provenance()
        .app_config_proof_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        installs_after - installs_before,
        1,
        "inner producer's install_fact_tracer must have advanced the counter exactly once"
    );

    // Discrimination: the outer tracer captured the inner cold
    // compute's file_whole_hash fact via TLS-stack fan-out.
    match outer_finalise {
        FactReadSetFinalise::Ok(outer_sig) => {
            assert!(
                outer_sig.iter().any(|f| matches!(
                    f,
                    verter_session::resolver_core::FactVersionRef::FileWholeHash {
                        canonical_id, ..
                    } if canonical_id == canonical
                )),
                "outer tracer must observe the inner producer's FileWholeHash fact \
                 (TLS-stack fan-out). Got: {outer_sig:?}",
            );
        }
        FactReadSetFinalise::Overflow => {
            panic!("outer tracer overflowed unexpectedly");
        }
    }
}
