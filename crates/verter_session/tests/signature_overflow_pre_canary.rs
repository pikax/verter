//! Sub-task L pre-canary — `signature_overflow_count == 0` over the
//! steady-state baseline + path-precise corpus.
//!
//! Stage 7's canary asserts the same invariant under the full
//! `repo_first_pass` / `repo_warm_second_pass` loop; this Stage 6d
//! pre-canary runs at unit-test speed against the hermetic baseline
//! and path-precise archetypes so a producer that flatten transitive
//! facts into an over-1024 signature surfaces here, not at Stage 7.
//!
//! **Discrimination.** A non-zero `signature_overflow_count` means
//! the producer's hierarchical-signature path is broken: it
//! observed > 1024 facts directly rather than collapsing a
//! downstream materialiser's `semantic_hash`. Fix at Stage 6d,
//! not Stage 7 (per the plan's verify-bullet).

use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Both Stage 0 fixture corpora exist on disk — the pre-canary
/// depends on them as the steady-state load.
#[test]
fn pre_canary_fixture_corpora_are_in_tree() {
    let baseline = fixture_root().join("cache_baseline");
    let path_precise = fixture_root().join("path_precise");
    assert!(
        baseline.is_dir(),
        "Stage 0 cache_baseline fixture corpus missing at {:?}",
        baseline
    );
    assert!(
        path_precise.is_dir(),
        "Stage 0 path_precise fixture corpus missing at {:?}",
        path_precise
    );
}

/// `signature_overflow_count == 0` on a fresh host that exercises
/// the basic resolver flow. This is the unit-speed pre-canary
/// version of the Stage 7 steady-state assertion: an over-cap
/// signature on the basic flow would mean a producer is flattening
/// transitive facts where it should be folding into a hierarchical
/// `semantic_hash`.
///
/// Reads the aggregated `signature_overflow_count` on the host's
/// `RouteDb` + `ImportedRootDb` — the load-bearing
/// `ValidatedFactCache`s today. The same accessors will be
/// consumed by Stage 7's final canary.
#[test]
fn pre_canary_signature_overflow_count_is_zero_on_basic_resolver_flow() {
    use verter_session::{HostConfig, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());

    // Probe a non-existent canonical so the consumer paths run
    // through every cache layer's cold path at least once. A
    // producer with a flattened transitive signature would push
    // its cache over the cap on the first cold attempt.
    let _ = host.get_component_meta("/__nonexistent__.vue");
    let _ = host.get_analysis("/__nonexistent__.vue");

    let routes = host.project_type_store().routes();
    assert_eq!(
        routes.signature_overflow_count(),
        0,
        "Stage 6d pre-canary: RouteDb signature_overflow_count must be 0 on the basic \
         resolver flow. A non-zero count means a producer flattened transitive facts \
         where it should have folded a downstream materialiser's semantic_hash. Fix \
         the producer at Stage 6d (NOT at Stage 7's canary)."
    );

    let imported_roots = host.project_type_store().imported_roots();
    assert_eq!(
        imported_roots.signature_overflow_count(),
        0,
        "Stage 6d pre-canary: ImportedRootDb signature_overflow_count must be 0 on \
         the basic resolver flow"
    );
}

/// Steady-state pre-canary: a longer-running resolver flow (multiple
/// `get_component_meta` calls + a few overlay/upsert iterations)
/// must NOT push any `ValidatedFactCache` over the
/// `FACT_SIGNATURE_CAP`.
///
/// The Stage 0 baseline corpus is the canonical workload for the
/// `repo_first_pass` / `repo_warm_second_pass` loop; here we
/// approximate it by running the resolver substrate through
/// several N=8 owner queries on synthetic SFCs.
#[test]
fn pre_canary_signature_overflow_count_is_zero_under_steady_state_loop() {
    use std::sync::Arc;
    use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());

    // Upsert a handful of synthetic SFCs + their shared dep.
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/w/types.ts".to_string(),
        source: Arc::from("export interface Props { a: string; b: number }"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    });
    for i in 0..8 {
        let name = format!("/w/Comp{i}.vue");
        let body = format!(
            "<script setup lang=\"ts\">\n\
            import type {{ Props }} from './types'\n\
            defineProps<Props>()\n\
            </script>\n\
            <template><div>{i}</div></template>"
        );
        let _ = host.upsert(UpsertRequest {
            canonical_id: None,
            input_id: name,
            source: Arc::from(body.as_str()),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        });
    }

    for i in 0..8 {
        let name = format!("/w/Comp{i}.vue");
        let _ = host.get_component_meta(&name);
    }

    let routes = host.project_type_store().routes();
    assert_eq!(
        routes.signature_overflow_count(),
        0,
        "Stage 6d pre-canary (steady state): RouteDb signature_overflow_count must \
         stay 0 across N=8 owners over a shared dep. Got {}.",
        routes.signature_overflow_count()
    );

    let imported_roots = host.project_type_store().imported_roots();
    assert_eq!(
        imported_roots.signature_overflow_count(),
        0,
        "Stage 6d pre-canary (steady state): ImportedRootDb signature_overflow_count \
         must stay 0 across the steady-state loop. Got {}.",
        imported_roots.signature_overflow_count()
    );
}
