//! An answer computed while the process-local retention account has no
//! headroom left is the same complete answer, only uncached: a refused
//! admission keeps the value it was asked to retain.
//!
//! The expected answer is the one TypeScript gives the same three-module
//! spread chain, and the one the lane gives it with headroom.

use super::checker_probe_lane_tests::{
    in_a_fresh_process, mismatches_in, mismatches_on_host, probe_host, test_path, ProbeProject,
};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};

const M0: &str = "export function f0() { return { k0: 0 as const }; }";
const M1: &str = "import { f0 } from './m-0';
export function f1() { const p = f0(); return { ...p, k1: 1 as const }; }";
const PROBE: &str = "import { f1 } from './m-1';
export function f2() { const p = f1(); return { ...p, k2: 2 as const }; }";
const ROWS: &[(&str, &str)] = &[("ReturnType<typeof f2>", "{ k0: 0; k1: 1; k2: 2; }")];

fn return_chain_mismatches() -> Vec<String> {
    let project = ProbeProject {
        files: &[("m-0.ts", M0), ("m-1.ts", M1)],
        ..Default::default()
    };
    mismatches_in(project, PROBE, ROWS)
}

#[test]
fn a_cross_module_return_answers_with_no_retention_headroom() {
    in_a_fresh_process(test_path!(), || {
        let headroom = return_chain_mismatches();
        assert!(
            headroom.is_empty(),
            "with headroom: {}",
            headroom.join("\n")
        );
        // Live handles elsewhere in the process hold the whole ceiling, as
        // a long session's retained parses do; every refusable admission
        // from here on is refused for pressure.
        let _held = SemanticRetentionAccount::process_local()
            .pin(RetentionLimits::DEFAULT_AGGREGATE_CEILING_BYTES);
        let pressed = return_chain_mismatches();
        assert!(
            pressed.is_empty(),
            "without headroom: {}",
            pressed.join("\n")
        );
    });
}

/// `(sibling module, probe module, (probe, checker print))`: an import read
/// in type position and one read through a value's return, each over the
/// one sibling module.
const IMPORT_ROWS: &[(&str, &str, (&str, &str))] = &[
    (
        "export type A = { a: 1 };",
        "import type { A } from './m-0';\ntype T = A;",
        ("T", "A"),
    ),
    (
        M0,
        "import { f0 } from './m-0';",
        ("ReturnType<typeof f0>", "{ k0: 0; }"),
    ),
];

fn import_row_mismatches() -> Vec<String> {
    IMPORT_ROWS
        .iter()
        .flat_map(|&(module, probe_module, row)| {
            let project = ProbeProject {
                files: &[("m-0.ts", module)],
                ..Default::default()
            };
            mismatches_in(project, probe_module, &[row])
        })
        .collect()
}

/// The account once it holds at most `ceiling` bytes, or as it stands
/// when a bounded wait for that runs out. A dropped host's retained parse
/// snapshots are released by its declaration-lowering workers as those
/// threads wind down, after the host's own drop has returned; with no host
/// alive the account only shrinks, so the wait ends as soon as they have.
fn released_to(
    account: &SemanticRetentionAccount,
    ceiling: usize,
) -> verter_session_query::retention::RetentionAccountSnapshot {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let snapshot = account.snapshot();
        if snapshot.total_bytes() <= ceiling || std::time::Instant::now() >= deadline {
            return snapshot;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// A direct import answers the same with no headroom as with it, and the
/// pressure leaves nothing behind: every byte the pressured requests charged
/// is released once their hosts and the pin are gone.
#[test]
fn an_import_answers_as_with_headroom_and_retains_nothing_under_pressure() {
    in_a_fresh_process(test_path!(), || {
        let headroom = import_row_mismatches();
        assert!(
            headroom.is_empty(),
            "with headroom: {}",
            headroom.join("\n")
        );
        let account = SemanticRetentionAccount::process_local();
        let baseline = account.snapshot();
        let held = account.pin(RetentionLimits::DEFAULT_AGGREGATE_CEILING_BYTES);
        let pressed = import_row_mismatches();
        assert!(
            pressed.is_empty(),
            "without headroom: {}",
            pressed.join("\n")
        );
        let during = account.snapshot();
        assert!(
            during.refusals_pressure > baseline.refusals_pressure,
            "the pressured requests were refused retention: {during:?}"
        );
        assert_eq!(
            during.retained_bytes, baseline.retained_bytes,
            "a pressured request retains nothing: {during:?}"
        );
        drop(held);
        let after = released_to(&account, baseline.total_bytes());
        assert!(
            after.total_bytes() <= baseline.total_bytes(),
            "the account returns to its baseline once the pin drops: \
             {after:?}, baseline {baseline:?}"
        );
    });
}

/// A resolution account that refuses every reservation, as one under
/// pressure does, while the process account keeps its headroom.
struct RefusingResolutionAccount;

impl verter_session_query::retention::resolution_charge::ResolutionRetentionAccount
    for RefusingResolutionAccount
{
    fn reserve_retained(
        &self,
        _bytes: usize,
    ) -> Option<verter_session_query::retention::resolution_charge::ResolutionRetentionCharge> {
        None
    }
}

fn traced_type_route(
    host: &crate::VerterHost,
) -> (
    verter_workspace::ResolutionPublication<String>,
    verter_session_query::facts::fact_read_set::FactReadSetFinalise,
) {
    verter_type_engine::fact_signature_helpers::install_fact_tracer(
        &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::unbound(host),
        || host.resolve_type_dependency_canonical("/wb/m-1.ts", "./m-0"),
    )
}

/// A resolution the retention account refuses reaches its consumer as the
/// complete target, unretained, and the compute that read it may not be
/// admitted to any persistent or shared store. With the account admitting,
/// the same read is admitted and leaves its compute admissible.
///
/// Mutation recipe: keep the refused resolution's admission cacheable at
/// the engine's refusal site. The route is then admitted and the enclosing
/// compute stays admissible. Or refuse it at the publication carrier: the
/// target is then withheld.
#[test]
fn an_unretained_resolution_is_delivered_and_its_reader_is_never_admitted() {
    let project = ProbeProject {
        files: &[("m-0.ts", M0), ("m-1.ts", M1)],
        ..Default::default()
    };
    let admitting = probe_host(project);
    let (publication, finalise) = traced_type_route(&admitting);
    assert!(
        matches!(&publication, verter_workspace::ResolutionPublication::Admitted(admitted)
            if admitted.result().map(String::as_str) == Some("/wb/m-0.ts")),
        "control: with retention the route is admitted, got {publication:?}"
    );
    assert!(
        matches!(
            finalise,
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::Ok(_)
        ),
        "control: an admitted route leaves its reader admissible"
    );

    let pressed = probe_host(project);
    pressed
        .ws()
        .install_resolution_retention(std::sync::Arc::new(RefusingResolutionAccount));
    let (publication, finalise) = traced_type_route(&pressed);
    match &publication {
        verter_workspace::ResolutionPublication::Unretained(unretained) => assert_eq!(
            unretained.result().map(String::as_str),
            Some("/wb/m-0.ts"),
            "the complete target is delivered"
        ),
        other => panic!("a pressured route is delivered unretained, got {other:?}"),
    }
    assert!(
        matches!(
            finalise,
            verter_session_query::facts::fact_read_set::FactReadSetFinalise::NonCacheable(_)
        ),
        "a compute that read an unretained route is never admitted"
    );
    assert_eq!(
        pressed.ws().resource_snapshot().resolution.slots,
        0,
        "the refused resolution holds no slot"
    );
}

/// With only the resolution refused retention — every derived cache keeps
/// its headroom — the cross-module return still answers as the checker
/// does.
#[test]
fn a_cross_module_return_answers_when_only_its_resolutions_are_refused() {
    let project = ProbeProject {
        files: &[("m-0.ts", M0), ("m-1.ts", M1)],
        ..Default::default()
    };
    let host = probe_host(project);
    host.ws()
        .install_resolution_retention(std::sync::Arc::new(RefusingResolutionAccount));
    for read in ["cold", "repeated"] {
        let failures = mismatches_on_host(&host, project, PROBE, ROWS);
        assert!(failures.is_empty(), "{read}: {}", failures.join("\n"));
    }
}

/// A cold augmentation scan whose relative target resolution the retention
/// account refuses still matches its augmenter, yet stores nothing in the
/// shared augmentation index: the set was computed from a resolution the
/// process did not keep. With the account admitting, the same scan installs
/// its set.
///
/// Mutation recipe: publish the scanned set whatever resolutions fed it.
/// The pressured scan then installs its entry.
#[test]
fn an_augmentation_scan_over_an_unretained_resolution_stores_no_index_entry() {
    use crate::file_artifact_store::{
        AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind,
    };
    use crate::host_manage::source_augmentation::{AugmentationRequestDriver, AugmentationTarget};

    const AUGMENTED: &str = "export interface A { a: 1 }";
    const AUGMENTER: &str = "declare module './m-0' { interface A { b: 2 } }\nexport {};";
    let project = ProbeProject {
        files: &[("m-0.ts", AUGMENTED), ("aug.ts", AUGMENTER)],
        ..Default::default()
    };
    let scan = |host: &crate::VerterHost| {
        let _ = host.ensure_indexed_ready_serve("/wb/aug.ts");
        let env = host.host_view_env_hashes_for("/wb/m-0.ts");
        let key = AugmentationTargetKey {
            project_identity: host.host_view_project_identity_for("/wb/m-0.ts"),
            resolve_env_hash: env.resolve_env_hash,
            lib_env_hash: env.lib_env_hash,
            population: AugmentationPopulation::Base,
            target: AugmentationTargetKind::ResolvedRelativeCanonical(std::sync::Arc::from(
                "/wb/m-0.ts",
            )),
        };
        let store = host.project_type_store().indexed();
        let set = AugmentationRequestDriver::new(store).ensure_populated(
            &key,
            |augmenter, specifier| {
                AugmentationTarget::from_publication(
                    host.resolve_type_dependency_canonical(augmenter, specifier),
                )
            },
            None,
        );
        (set.entries.len(), store.get_augmenter_set(&key).is_some())
    };

    let admitting = probe_host(project);
    assert_eq!(
        scan(&admitting),
        (1, true),
        "control: an admitted scan matches its augmenter and installs its set"
    );

    let pressed = probe_host(project);
    pressed
        .ws()
        .install_resolution_retention(std::sync::Arc::new(RefusingResolutionAccount));
    assert_eq!(
        scan(&pressed),
        (1, false),
        "a scan over an unretained resolution matches its augmenter and stores nothing"
    );
}
