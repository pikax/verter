use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_semantic::facts::version::FactVersionRef;
use verter_semantic::resolver_core::{
    AttemptFailure, AttemptOutcome, AttemptOutput, CompletedAttempt,
    ConsumedResolutionObservationKey, InputKey, InputLoadIntegrityReason,
    InputResolutionBudgetMeter, InputResolutionBudgets, KernelAttempt, LoadSet, PathProbe,
    ResolutionContext, ResolvePhase, ResolveRequestKind, ResolverObservation,
};

use super::resolution_conversion_tests::{
    project, run_kernel_core_in, with_aliases, LedgerScope, ResolutionFixture,
};
use crate::engine::resolution_test_hooks::{self, ResolutionPhase};
use crate::resolution_currency::{CanonicalResolutionId, ResolutionFactKey};
use crate::resolver::{
    drive_attempt, drive_attempt_with_bounded_io, load_requested_workspace_inputs,
    reset_resolution_driver_churn_for_test, resolution_driver_churn_for_test,
    take_input_resolution_budget_events_for_test, unsupported_input_failure, InputResolutionLedger,
    LoadedResolutionInput, LoadedResolutionInputBatch, ResolutionInputReservationBatch,
    ResolutionInputs,
};
use crate::traits::{WorkspaceAccess, WorkspaceRead};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

fn tightened_budgets(
    attempts: u32,
    unique_keys: u32,
    input_bytes: u64,
    driver_depth: u32,
    churn: u32,
) -> InputResolutionBudgets {
    InputResolutionBudgets::try_tightened(attempts, unique_keys, input_bytes, driver_depth, churn)
        .expect("test policy must be a valid tightening")
}

fn path_key(path: &'static str) -> InputKey {
    InputKey::PathProbe {
        path: Arc::from(path),
    }
}

fn retained_fact(byte: u8) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: format!("/retained/{byte}.ts"),
        hash: [byte; 16],
    }
}

#[test]
fn loaded_observation_metadata_reuses_input_key_arc_identity() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let keys = vec![
        InputKey::PathProbe {
            path: Arc::from("/identity/probe.ts"),
        },
        InputKey::RealPath {
            path: Arc::from("/identity/real.ts"),
        },
        InputKey::PackageManifest {
            directory: Arc::from("/identity/pkg"),
        },
    ];
    let mut inputs = ResolutionInputs::default();
    load_requested_workspace_inputs(&workspace, &mut inputs, &keys)
        .expect("the three workspace-owned resolver input kinds must load");

    for key in &keys {
        assert!(
            inputs.metadata_key_shares_input_arc_for_test(key),
            "request metadata must retain the exact InputKey Arc instead of allocating a second owned spelling: {key:?}"
        );
    }

    // Revert control: storing `path.to_string()` / `directory.to_string()`
    // in the request metadata maps makes each pointer-identity check fail.
}

#[test]
fn successful_multi_wave_driver_reuses_delta_and_defers_terminal_key_copies() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let first = InputKey::PathProbe {
        path: Arc::from("/churn/first.ts"),
    };
    let second = InputKey::RealPath {
        path: Arc::from("/churn/second.ts"),
    };
    let mut wave = 0_u8;
    reset_resolution_driver_churn_for_test();
    let mut ledger = InputResolutionLedger::default();

    let result = drive_attempt(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, basis| {
            wave += 1;
            match wave {
                1 => AttemptOutcome::NeedInputs(LoadSet::new(vec![first.clone()], basis)),
                2 => AttemptOutcome::NeedInputs(LoadSet::new(vec![second.clone()], basis)),
                _ => AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new())),
            }
        },
    );

    result.expect("the two-load request must complete on its third wave");
    assert_eq!(
        resolution_driver_churn_for_test(),
        (0, 1),
        "successful requests must not copy terminal-only unresolved keys and must allocate one reusable delta buffer"
    );

    // Revert controls: restoring `unresolved = load_set.keys().to_vec()`
    // makes the first field 2; restoring `LoadSet::delta` per wave makes the
    // second field 2.
}

#[test]
fn manifest_name_only_edit_changes_the_replayed_fingerprint() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file(
        "/proj/src/main.ts".to_string(),
        Arc::from("import type { Value } from 'pkg';\n"),
    );
    workspace.inject_file(
        "/proj/node_modules/pkg/index.d.ts".to_string(),
        Arc::from("export interface Value {}\n"),
    );
    workspace.inject_file(
        "/proj/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"name":"pkg-before","types":"./index.d.ts"}"#),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project("/proj", "/proj/tsconfig.json")]);

    let first =
        WorkspaceRead::resolve_import_outcome(&workspace, "/proj/src/main.ts", "pkg", CONTEXT);
    assert_eq!(
        first.result().map(|result| result.source_id.as_str()),
        Some("/proj/node_modules/pkg/index.d.ts"),
        "precondition: the first manifest-backed resolve must succeed"
    );
    assert!(first.is_cacheable(), "the first answer must be admitted");
    assert!(
        !first.trace().reused(),
        "precondition: the first answer must be computed, not reused"
    );

    let population = WorkspaceRead::resolution_population(&workspace);
    let manifest_key = ResolutionFactKey::Manifest {
        canonical: CanonicalResolutionId::new("/proj/node_modules/pkg/package.json"),
        population,
    };
    let first_version = workspace
        .engine
        .resolution_fact_version_for_test(population, &manifest_key);

    workspace.inject_file(
        "/proj/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"name":"pkg-after","types":"./index.d.ts"}"#),
    );

    let second =
        WorkspaceRead::resolve_import_outcome(&workspace, "/proj/src/main.ts", "pkg", CONTEXT);
    let second_version = workspace
        .engine
        .resolution_fact_version_for_test(population, &manifest_key);

    assert_eq!(
        second.result().map(|result| result.source_id.as_str()),
        Some("/proj/node_modules/pkg/index.d.ts"),
        "the name-only rewrite must preserve the semantically unchanged target"
    );
    assert_ne!(
        second_version, first_version,
        "required: replaying a manifest after a name-only rewrite must observe a new fact version"
    );
    assert!(
        !second.trace().reused(),
        "forbidden: a name-only edit must not be fingerprint-invisible or serve the stale first answer"
    );
    assert!(
        second.trace().recomputed(),
        "the second answer must be recomputed after the manifest fact moves"
    );
}

#[test]
fn directory_members_signature_records_only_consumed_members() {
    let consumed_directory = "/enumerated/consumed-util-ts";
    let prefetched_directory = "/enumerated/prefetched-util-tsx";
    let fixture = ResolutionFixture::new(&["/proj/src/util.ts"])
        .with_probe_directory_observation("/proj/src/util.ts", consumed_directory)
        .with_probe_directory_observation("/proj/src/util.tsx", prefetched_directory);
    let projects = vec![with_aliases(
        project("/proj", "/proj/tsconfig.json"),
        &[("@/", "/proj/src")],
    )];

    let run = run_kernel_core_in(
        &fixture,
        projects,
        "/proj/src/main.ts",
        "@/util",
        LedgerScope::DriverAcceptanceOutsideTheLedger,
    );
    assert_eq!(run.resolved.as_deref(), Some("/proj/src/util.ts"));

    let directory_members: Vec<_> = run
        .replayed_facts
        .iter()
        .filter_map(|fact| match fact {
            ResolutionFactKey::DirectoryMembers { .. } => fact.canonical_id().map(str::to_string),
            _ => None,
        })
        .collect();
    assert_eq!(
        directory_members,
        vec![consumed_directory.to_string()],
        "required: the replayed signature must record only the directory member enumerated by the consumed probe"
    );
    assert!(
        !directory_members
            .iter()
            .any(|canonical| canonical == prefetched_directory),
        "forbidden: a prefetched-but-unconsumed member must not enter the replayed signature"
    );
}

#[test]
fn every_consumed_selector_replays_once_in_wave_order() {
    let fixture = ResolutionFixture::new(&["/proj/src/util.ts"]);
    let projects = vec![with_aliases(
        project("/proj", "/proj/tsconfig.json"),
        &[("@/", "/proj/src")],
    )];

    let run = run_kernel_core_in(
        &fixture,
        projects,
        "/proj/src/main.ts",
        "@/util",
        LedgerScope::DriverAcceptanceOutsideTheLedger,
    );
    assert_eq!(run.resolved.as_deref(), Some("/proj/src/util.ts"));
    assert!(
        run.waves >= 3,
        "required: manifest, path-probe, and realpath loading must span at least three waves; got {}",
        run.waves
    );

    let manifest_selector = run.ordered_selectors.iter().position(|selector| {
        matches!(
            selector,
            ConsumedResolutionObservationKey::PackageManifest { directory }
                if directory.as_ref() == "/proj/src/util"
        ) || matches!(
            selector,
            ConsumedResolutionObservationKey::PathProbe { path }
                if path.as_ref() == "/proj/src/util/package.json"
        )
    });
    let path_selector = run.ordered_selectors.iter().position(|selector| {
        matches!(
            selector,
            ConsumedResolutionObservationKey::PathProbe { path }
                if path.as_ref() == "/proj/src/util.ts"
        )
    });
    let realpath_selector = run.ordered_selectors.iter().position(|selector| {
        matches!(selector, ConsumedResolutionObservationKey::RealPath { path }
            if path.as_ref() == "/proj/src/util.ts")
    });
    match (manifest_selector, path_selector, realpath_selector) {
        (Some(manifest), Some(path), Some(realpath)) => assert!(
            manifest < path && path < realpath,
            "required: consumed order must be manifest-check < path-probe < realpath; got manifest={manifest}, path={path}, realpath={realpath}"
        ),
        positions => panic!(
            "forbidden: the ordered consumed sequence lost a required selector: {positions:?}"
        ),
    }

    let mut replay_counts = BTreeMap::<String, usize>::new();
    for fact in &run.replayed_facts {
        *replay_counts.entry(format!("{fact:?}")).or_default() += 1;
    }
    for selector in &run.ordered_selectors {
        let matches = run
            .replayed_facts
            .iter()
            .filter(|fact| fact_matches_selector(fact, selector))
            .count();
        assert_eq!(
            matches, 1,
            "forbidden: consumed selector {selector:?} must replay exactly once, but matched {matches} facts in {:?}",
            run.replayed_facts
        );
    }
    assert!(
        replay_counts.values().all(|count| *count == 1),
        "forbidden: replayed ResolutionFactKey entries must retain first-observation order without set-style loss or duplicates: {replay_counts:?}"
    );

    let manifest_fact = run.replayed_facts.iter().position(|fact| {
        matches!(
            fact,
            ResolutionFactKey::Manifest { .. } | ResolutionFactKey::PathProbe { .. }
        ) && fact.canonical_id() == Some("/proj/src/util/package.json")
    });
    let path_fact = run.replayed_facts.iter().position(|fact| {
        matches!(fact, ResolutionFactKey::PathProbe { .. })
            && fact.canonical_id() == Some("/proj/src/util.ts")
    });
    let realpath_fact = run.replayed_facts.iter().position(|fact| {
        matches!(fact, ResolutionFactKey::Realpath { .. })
            && fact.canonical_id() == Some("/proj/src/util.ts")
    });
    match (manifest_fact, path_fact, realpath_fact) {
        (Some(manifest), Some(path), Some(realpath)) => assert!(
            manifest < path && path < realpath,
            "required: replay must preserve manifest-check < path-probe < realpath order; got manifest={manifest}, path={path}, realpath={realpath}"
        ),
        positions => panic!("forbidden: replay omitted a required ordered fact: {positions:?}"),
    }
}

fn fact_matches_selector(
    fact: &ResolutionFactKey,
    selector: &ConsumedResolutionObservationKey,
) -> bool {
    match (fact, selector) {
        (
            ResolutionFactKey::PathProbe { .. },
            ConsumedResolutionObservationKey::PathProbe { path },
        ) => fact.canonical_id() == Some(path.as_ref()),
        (
            ResolutionFactKey::Realpath { .. },
            ConsumedResolutionObservationKey::RealPath { path },
        ) => fact.canonical_id() == Some(path.as_ref()),
        (
            ResolutionFactKey::Manifest { .. },
            ConsumedResolutionObservationKey::PackageManifest { directory },
        ) => fact.canonical_id().is_some_and(|canonical| {
            canonical == verter_semantic::resolver_core::join_paths(directory, "package.json")
        }),
        (
            ResolutionFactKey::RecoveryScope { .. },
            ConsumedResolutionObservationKey::RecoveryScope {
                canonical_prefix: selector_prefix,
            },
        ) => fact.canonical_id() == Some(selector_prefix.as_ref()),
        _ => false,
    }
}

#[test]
fn basis_change_mid_flight_restarts_cleanly_on_the_new_basis() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested_key = InputKey::PathProbe {
        path: Arc::from("/basis/input.ts"),
    };
    let mut first_basis = None;
    let mut observed_bases = Vec::new();
    let mut changed_basis = false;
    let mut ledger = InputResolutionLedger::default();

    let result = drive_attempt(
        &workspace,
        &mut ledger,
        |_, _| true,
        |view, basis| {
            observed_bases.push(basis);
            if !changed_basis {
                first_basis = Some(basis);
                changed_basis = true;
                workspace.inject_file(
                    "/basis/pivot.ts".to_string(),
                    Arc::from("export const pivot = true;\n"),
                );
                return AttemptOutcome::NeedInputs(LoadSet::new(
                    vec![requested_key.clone()],
                    basis,
                ));
            }

            match view.path_probe("/basis/input.ts") {
                AttemptOutcome::Complete(_) => {
                    AttemptOutcome::Complete(CompletedAttempt::new(basis, AttemptOutput::new()))
                }
                AttemptOutcome::NeedInputs(load_set) => AttemptOutcome::NeedInputs(load_set),
                AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
            }
        },
    );

    let first_basis = first_basis.expect("the first attempt must capture a basis");
    let completed_basis = result.expect("required: the attempt must complete on the new basis");
    assert_ne!(
        completed_basis, first_basis,
        "required: a mid-flight resolution-world change must restart on the new basis"
    );
    assert!(
        observed_bases
            .iter()
            .skip(1)
            .all(|basis| *basis != first_basis),
        "forbidden: no post-change attempt may continue on the stale first basis: {observed_bases:?}"
    );
    let (attempts, unique, _, depth, churn) = ledger.consumed_for_test();
    assert_eq!((attempts, unique, depth, churn), (3, 1, 2, 1));
    assert_eq!(
        InputResolutionLedger::default().consumed_for_test(),
        (0, 0, 0, 0, 0),
        "an independent operation must start with a fresh ledger"
    );
}

#[test]
fn unsatisfiable_input_surfaces_terminal_no_progress_with_unresolved_keys() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let unresolved = InputKey::PathProbe {
        path: Arc::from("/unsatisfiable/input.ts"),
    };
    let mut ledger = InputResolutionLedger::default();

    let failure = drive_attempt(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, _| {
            AttemptOutcome::<CompletedAttempt<()>>::Terminal(
                AttemptFailure::InputResolutionNoProgress {
                    unresolved: vec![unresolved.clone()],
                },
            )
        },
    )
    .expect_err("required: an unsatisfiable input must terminate as no-progress");

    assert_eq!(
        *failure,
        AttemptFailure::InputResolutionNoProgress {
            unresolved: vec![unresolved],
        },
        "required: terminal no-progress must preserve the unresolved keys"
    );
    assert!(
        !matches!(
            *failure,
            AttemptFailure::InputResolutionAttemptLimit { .. }
                | AttemptFailure::InputLoadUnavailable { .. }
        ),
        "forbidden: terminal no-progress must not collapse into an attempt-limit or load failure"
    );
}

#[test]
fn default_depth_breach_precedes_the_later_attempt_ceiling() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let mut attempt = 0_u32;
    let mut ledger = InputResolutionLedger::default();

    let failure = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, basis| {
            let key = InputKey::PathProbe {
                path: Arc::from(format!("/limit/{attempt}.ts")),
            };
            attempt += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![key], basis))
        },
    )
    .expect_err("required: exhausting the driver budget must surface a typed limit failure");

    assert_eq!(
        *failure,
        AttemptFailure::InputResolutionDepthLimit {
            unresolved: vec![InputKey::PathProbe {
                path: Arc::from("/limit/64.ts"),
            }],
            depth: 64,
        },
        "required: the earlier depth breach must retain its consumed value and rejected delta"
    );
    assert!(
        !matches!(
            *failure,
            AttemptFailure::InputResolutionNoProgress { .. }
                | AttemptFailure::InputLoadUnavailable { .. }
        ),
        "forbidden: a limit breach must not collapse into no-progress or load-unavailable"
    );
}

#[test]
fn transient_load_failure_retries_and_completes() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested_key = InputKey::PathProbe {
        path: Arc::from("/transient/input.ts"),
    };
    let mut fail_first_load = true;
    let mut load_attempts = 0_u32;
    let mut ledger = InputResolutionLedger::default();

    let result = drive_attempt_with_bounded_io(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_attempts += 1;
            if fail_first_load {
                fail_first_load = false;
                return Err(AttemptFailure::TransientInputLoadFailure {
                    key: Box::new(reservation.keys()[0].clone()),
                });
            }
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |view, basis| match view.path_probe("/transient/input.ts") {
            AttemptOutcome::Complete(_) => {
                AttemptOutcome::Complete(CompletedAttempt::new("complete", AttemptOutput::new()))
            }
            AttemptOutcome::NeedInputs(_) => {
                AttemptOutcome::NeedInputs(LoadSet::new(vec![requested_key.clone()], basis))
            }
            AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
        },
    );

    assert!(
        !matches!(
            &result,
            Err(failure)
                if matches!(
                    failure.as_ref(),
                    AttemptFailure::TransientInputLoadFailure { .. }
                )
        ),
        "forbidden: a transient first load failure must not surface as InputLoadUnavailable"
    );
    assert_eq!(
        result.expect("required: a transient load failure must be retried"),
        "complete"
    );
    assert_eq!(
        load_attempts, 2,
        "required: the same input must be loaded again after the transient failure"
    );
}

#[test]
fn transient_preflight_failure_retries_and_completes() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested_key = path_key("/transient/preflight.ts");
    let mut preflight_attempts = 0_u32;
    let mut load_attempts = 0_u32;
    let mut kernel_attempts = 0_u32;
    let mut ledger = InputResolutionLedger::default();

    let result = drive_attempt_with_bounded_io(
        &workspace,
        &mut ledger,
        |keys, basis| {
            preflight_attempts += 1;
            if preflight_attempts == 1 {
                return Err(AttemptFailure::TransientInputLoadFailure {
                    key: Box::new(keys[0].clone()),
                });
            }
            workspace.preflight_resolution_inputs_bounded(keys, basis)
        },
        |reservation| {
            load_attempts += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |view, basis| {
            kernel_attempts += 1;
            match view.path_probe("/transient/preflight.ts") {
                AttemptOutcome::Complete(_) => AttemptOutcome::Complete(CompletedAttempt::new(
                    "complete",
                    AttemptOutput::new(),
                )),
                AttemptOutcome::NeedInputs(_) => {
                    AttemptOutcome::NeedInputs(LoadSet::new(vec![requested_key.clone()], basis))
                }
                AttemptOutcome::Terminal(failure) => AttemptOutcome::Terminal(failure),
            }
        },
    );

    assert_eq!(
        result.expect("an explicitly transient preflight failure must be retried"),
        "complete"
    );
    assert_eq!(preflight_attempts, 2);
    assert_eq!(load_attempts, 1);
    assert_eq!(kernel_attempts, 3);

    // Mutation control: deleting the explicit transient-preflight retry arm
    // returns the first failure and leaves every counter at one or zero.
}

#[test]
fn permanent_input_load_unavailable_is_immediate_terminal_and_not_retried() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested_key = path_key("/permanent/input.ts");
    let mut load_attempts = 0_u32;
    let mut kernel_attempts = 0_u32;
    let mut ledger = InputResolutionLedger::default();

    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_attempts += 1;
            Err(AttemptFailure::InputLoadUnavailable {
                key: Box::new(reservation.keys()[0].clone()),
            })
        },
        |_, _| true,
        |_, basis| {
            kernel_attempts += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![requested_key.clone()], basis))
        },
    )
    .expect_err("permanent/default load unavailability must surface immediately");

    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputLoadUnavailable { key } if key.as_ref() == &requested_key
    ));
    assert_eq!(
        load_attempts, 1,
        "permanent unavailability is not retryable"
    );
    assert_eq!(
        kernel_attempts, 1,
        "permanent unavailability must not be replaced by a later attempt or byte limit"
    );

    // Mutation control: treating `InputLoadUnavailable` as the retryable arm
    // drives this fixture until a five-limit failure replaces the required
    // terminal result and increments both counters.
}

#[test]
fn permanent_preflight_unavailable_is_immediate_terminal_and_not_retried() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested_key = path_key("/permanent/preflight.ts");
    let mut preflight_attempts = 0_u32;
    let mut load_attempts = 0_u32;
    let mut kernel_attempts = 0_u32;
    let mut ledger = InputResolutionLedger::default();

    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, _basis| {
            preflight_attempts += 1;
            Err(AttemptFailure::InputLoadUnavailable {
                key: Box::new(keys[0].clone()),
            })
        },
        |_| {
            load_attempts += 1;
            unreachable!("permanent preflight failure must prevent payload loading")
        },
        |_, _| true,
        |_, basis| {
            kernel_attempts += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![requested_key.clone()], basis))
        },
    )
    .expect_err("permanent/default preflight unavailability must surface immediately");

    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputLoadUnavailable { key } if key.as_ref() == &requested_key
    ));
    assert_eq!(preflight_attempts, 1);
    assert_eq!(load_attempts, 0);
    assert_eq!(kernel_attempts, 1);
}

#[test]
fn attempt_budget_is_inclusive_and_rejects_before_the_next_kernel_invocation() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let key = path_key("/a");
    let mut invocations = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(2, 8, 128, 4, 2));
    let result = drive_attempt(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, basis| {
            invocations += 1;
            if invocations == 1 {
                AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis))
            } else {
                AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new()))
            }
        },
    );
    result.expect("the inclusive second invocation must be admitted");
    assert_eq!(invocations, 2);

    let _ = take_input_resolution_budget_events_for_test();
    invocations = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(1, 8, 128, 4, 2));
    let failure = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, basis| {
            invocations += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis))
        },
    )
    .expect_err("the second invocation must breach a maximum of one");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionAttemptLimit { attempts: 1, .. }
    ));
    assert_eq!(invocations, 1, "the rejected kernel call must not run");
    assert_eq!(
        take_input_resolution_budget_events_for_test(),
        vec![
            verter_semantic::resolver_core::InputResolutionBudgetExhaustion {
                meter: InputResolutionBudgetMeter::Attempts,
                consumed: 1,
                prospective: 2,
                maximum: 1,
            }
        ]
    );
}

#[test]
fn kernel_retention_terminal_emits_exact_audit_once_and_publishes_nothing() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let budgets = InputResolutionBudgets::try_tightened_with_retention(8, 8, 128, 4, 2, 1, 1)
        .expect("test policy");
    let _ = take_input_resolution_budget_events_for_test();
    let mut publications = 0;
    let mut ledger = InputResolutionLedger::new(budgets);
    let failure = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| {
            publications += 1;
            true
        },
        |_, _| {
            AttemptOutcome::Terminal(
                AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                    retained: 1,
                    prospective: 2,
                    maximum: 1,
                },
            )
        },
    )
    .expect_err("retention exhaustion is terminal");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
            retained: 1,
            prospective: 2,
            maximum: 1,
        }
    ));
    assert_eq!(publications, 0);
    assert_eq!(
        take_input_resolution_budget_events_for_test(),
        vec![
            verter_semantic::resolver_core::InputResolutionBudgetExhaustion {
                meter: InputResolutionBudgetMeter::CompletedWitnessRetention,
                consumed: 1,
                prospective: 2,
                maximum: 1,
            }
        ]
    );

    let mut alias_publications = 0;
    let mut alias_ledger = InputResolutionLedger::new(budgets);
    let alias_failure = drive_attempt::<()>(
        &workspace,
        &mut alias_ledger,
        |_, _| {
            alias_publications += 1;
            true
        },
        |_, _| {
            AttemptOutcome::Terminal(AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
                retained: 1,
                prospective: 2,
                maximum: 1,
            })
        },
    )
    .expect_err("alias retention exhaustion is terminal");
    assert!(matches!(
        alias_failure.as_ref(),
        AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
            retained: 1,
            prospective: 2,
            maximum: 1,
        }
    ));
    assert_eq!(alias_publications, 0);
    assert_eq!(
        take_input_resolution_budget_events_for_test(),
        vec![
            verter_semantic::resolver_core::InputResolutionBudgetExhaustion {
                meter: InputResolutionBudgetMeter::AliasGeometryRetention,
                consumed: 1,
                prospective: 2,
                maximum: 1,
            }
        ]
    );

    let mut fresh = InputResolutionLedger::new(budgets);
    drive_attempt(
        &workspace,
        &mut fresh,
        |_, _| true,
        |_, _| AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new())),
    )
    .expect("a later independent operation starts clean");
}

#[test]
fn real_resolution_facade_maps_completed_witness_breach_to_budget_exceeded_without_publication() {
    let budgets = InputResolutionBudgets::try_tightened_with_retention(16, 64, 8_192, 8, 2, 1, 1)
        .expect("test policy");
    let workspace = crate::memory::MemoryWorkspace::new_with_input_resolution_budgets(
        Default::default(),
        budgets,
    );
    workspace.inject_file(
        "/proj/src/main.ts".to_string(),
        Arc::from("import './target';"),
    );
    workspace.inject_file(
        "/proj/src/target.ts".to_string(),
        Arc::from("export const target = 1;"),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project("/proj", "/proj/tsconfig.json")]);
    let _ = take_input_resolution_budget_events_for_test();

    let outcome =
        WorkspaceRead::resolve_import_outcome(&workspace, "/proj/src/main.ts", "./target", CONTEXT);
    assert_eq!(
        outcome.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded)
    );
    assert!(!outcome.trace().published());
    assert_eq!(
        workspace.engine.lazy_resolution_slot_len_for_test(
            "/proj/src/main.ts",
            "./target",
            CONTEXT,
            verter_semantic::resolver_core::ResolutionPopulation::Base,
        ),
        0,
        "the breached attempt cannot publish a positive or negative candidate"
    );
    assert_eq!(
        take_input_resolution_budget_events_for_test(),
        vec![
            verter_semantic::resolver_core::InputResolutionBudgetExhaustion {
                meter: InputResolutionBudgetMeter::CompletedWitnessRetention,
                consumed: 1,
                prospective: 2,
                maximum: 1,
            }
        ]
    );

    // Mutation recipe: bypass the completed-witness charge in any real
    // `AttemptOutput::record_*` path. The facade becomes cacheable/published
    // or the exact exhaustion payload disappears.
}

#[test]
fn completed_output_charge_survives_apply_until_operation_restart_or_end() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let budgets = InputResolutionBudgets::try_tightened_with_retention(8, 8, 128, 4, 2, 1, 1)
        .expect("test policy");
    let mut ledger = InputResolutionLedger::new(budgets);
    let mut applied = 0;

    drive_attempt(
        &workspace,
        &mut ledger,
        |_, output| {
            applied += 1;
            assert_eq!(output.observed_facts(), &[retained_fact(1)]);
            true
        },
        |view, _| {
            view.input_resolution_retention().scope(|| {
                let mut output = AttemptOutput::new();
                output
                    .record_fact(retained_fact(1))
                    .expect("first retained output witness");
                AttemptOutcome::Complete(CompletedAttempt::new((), output))
            })
        },
    )
    .expect("the completed attempt applies");
    assert_eq!(applied, 1);

    let failure = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| panic!("an overflowing second output must not apply"),
        |view, _| {
            view.input_resolution_retention().scope(|| {
                let mut output = AttemptOutput::new();
                let result = output.record_fact(retained_fact(2));
                match result {
                    Ok(()) => AttemptOutcome::Complete(CompletedAttempt::new((), output)),
                    Err(failure) => AttemptOutcome::Terminal(failure),
                }
            })
        },
    )
    .expect_err("the first applied output must remain charged");
    assert_eq!(
        failure.as_ref(),
        &AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
            retained: 1,
            prospective: 2,
            maximum: 1,
        }
    );

    ledger
        .charge_outer_restart(&workspace)
        .expect("the explicit restart remains within churn policy");
    drive_attempt(
        &workspace,
        &mut ledger,
        |_, _| true,
        |view, _| {
            view.input_resolution_retention().scope(|| {
                let mut output = AttemptOutput::new();
                output
                    .record_fact(retained_fact(2))
                    .expect("fresh output after restart");
                AttemptOutcome::Complete(CompletedAttempt::new((), output))
            })
        },
    )
    .expect("the restarted operation completes");
    drop(ledger);

    // Mutation recipe: remove the ledger's ownership transfer after apply.
    // The second drive then admits and applies instead of returning max+1.
}

#[test]
fn real_transaction_holds_completed_output_through_final_fence_then_releases_on_publish_or_abandon()
{
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    workspace.inject_file(
        "/proj/src/main.ts".to_string(),
        Arc::from("import './published'; import './abandoned';"),
    );
    workspace.inject_file(
        "/proj/src/published.ts".to_string(),
        Arc::from("export const published = 1;"),
    );
    workspace.inject_file(
        "/proj/src/abandoned.ts".to_string(),
        Arc::from("export const abandoned = 1;"),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project("/proj", "/proj/tsconfig.json")]);
    let published = workspace
        .load_published()
        .expect("published resolver index");
    let _ = resolution_test_hooks::take_completed_outputs_at_final_fence();
    let _ = resolution_test_hooks::take_completed_outputs_at_publication();

    let mut published_ledger = InputResolutionLedger::default();
    let outcome = workspace
        .engine
        .resolve_import_outcome_for_published_in_operation(
            &workspace,
            crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
            &published,
            "/proj/src/main.ts",
            "./published",
            CONTEXT,
            &mut published_ledger,
            &|| true,
        );
    assert!(outcome.is_cacheable());
    assert!(outcome.trace().published());
    assert_eq!(
        resolution_test_hooks::take_completed_outputs_at_final_fence(),
        Some(1),
        "the actual completed output must remain owned at the final fence"
    );
    assert_eq!(
        resolution_test_hooks::take_completed_outputs_at_publication(),
        Some(1),
        "the actual completed output must remain owned through admission and publication"
    );
    assert_eq!(
        published_ledger.applied_output_count_for_test(),
        0,
        "publication ends the completed-output retention lifetime"
    );

    let mut abandoned_ledger = InputResolutionLedger::default();
    let abandoned = workspace
        .engine
        .resolve_import_outcome_for_published_in_operation(
            &workspace,
            crate::resolution_currency::ResolutionEvidenceSource::ReaderAuthoritative,
            &published,
            "/proj/src/main.ts",
            "./abandoned",
            CONTEXT,
            &mut abandoned_ledger,
            &|| false,
        );
    assert_eq!(
        abandoned.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::ResolutionViewSuperseded)
    );
    assert!(!abandoned.trace().published());
    assert_eq!(
        abandoned_ledger.applied_output_count_for_test(),
        0,
        "an abandoned final fence releases the completed output immediately"
    );

    // Mutation recipe: drop the ownership transfer after apply, or move the
    // release before the final fence. The fence count changes from one; remove
    // either terminal release and the corresponding postcondition turns RED.
}

#[test]
fn unique_key_budget_is_cumulative_inclusive_and_precedes_loading() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let first = path_key("/u1");
    let second = path_key("/u2");
    let mut wave = 0;
    let mut load_calls = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(8, 2, 128, 4, 2));
    let result = drive_attempt_with_bounded_io(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| {
            wave += 1;
            match wave {
                1 => AttemptOutcome::NeedInputs(LoadSet::new(vec![first.clone()], basis)),
                2 => AttemptOutcome::NeedInputs(LoadSet::new(vec![second.clone()], basis)),
                _ => AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new())),
            }
        },
    );
    result.expect("two cumulative keys must meet the inclusive maximum");
    assert_eq!(load_calls, 2);

    wave = 0;
    load_calls = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(8, 1, 128, 4, 2));
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| {
            wave += 1;
            let keys = if wave == 1 {
                vec![first.clone()]
            } else {
                vec![second.clone()]
            };
            AttemptOutcome::NeedInputs(LoadSet::new(keys, basis))
        },
    )
    .expect_err("the second cumulative key must breach the maximum");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionUniqueKeyLimit { unique_keys: 1, .. }
    ));
    assert_eq!(load_calls, 1, "the breaching wave must not load");

    load_calls = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(8, 1, 128, 4, 2));
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| {
            AttemptOutcome::NeedInputs(LoadSet::new(vec![first.clone(), second.clone()], basis))
        },
    )
    .expect_err("two keys in one wave must breach unique-key maximum one");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionUniqueKeyLimit { unique_keys: 0, .. }
    ));
    assert_eq!(load_calls, 0);
}

#[test]
fn byte_budget_charges_key_and_reservation_before_loading_without_refund() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let key = path_key("/b");
    let mut load_calls = 0;
    let mut wave = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 3, 2, 2));
    let result = drive_attempt_with_bounded_io(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| {
            wave += 1;
            if wave == 1 {
                AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis))
            } else {
                AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new()))
            }
        },
    );
    result.expect("two spelling bytes plus one payload byte must fit exactly");
    assert_eq!(ledger.consumed_for_test().2, 3);

    load_calls = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 2, 2, 2));
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis)),
    )
    .expect_err("the reservation must breach before the load call");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionByteLimit { bytes: 2, .. }
    ));
    assert_eq!(load_calls, 0);

    let mut failed_once = false;
    let mut wave = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 3, 2, 2));
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            if !failed_once {
                failed_once = true;
                Err(AttemptFailure::TransientInputLoadFailure {
                    key: Box::new(reservation.keys()[0].clone()),
                })
            } else {
                workspace.load_preflighted_resolution_inputs(reservation)
            }
        },
        |_, _| true,
        |_, basis| {
            wave += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis))
        },
    )
    .expect_err("a retry must recharge its one-byte reservation");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionByteLimit { bytes: 3, .. }
    ));
    assert_eq!(
        ledger.consumed_for_test().1,
        1,
        "the key spelling is charged once"
    );
}

#[test]
fn depth_budget_is_inclusive_and_wins_a_multi_breach_before_loading() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let first = path_key("/d1");
    let second = path_key("/depth-two");
    let mut wave = 0;
    let mut load_calls = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(8, 1, 8, 1, 2));
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            load_calls += 1;
            workspace.load_preflighted_resolution_inputs(reservation)
        },
        |_, _| true,
        |_, basis| {
            wave += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(
                vec![if wave == 1 {
                    first.clone()
                } else {
                    second.clone()
                }],
                basis,
            ))
        },
    )
    .expect_err("the second accepted-wave ordinal must breach depth one");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionDepthLimit { depth: 1, .. }
    ));
    assert_eq!(load_calls, 1, "depth must precede unique and byte checks");
}

#[test]
fn churn_budget_is_inclusive_and_rejects_before_loader_work() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let key = path_key("/churn");
    let actual_basis = crate::resolution_currency::resolution_basis_for_reader(&workspace)
        .expect("memory workspace publishes a basis");
    assert_ne!(
        actual_basis,
        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder()
    );
    let mismatched = verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder();
    let mut wave = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 128, 2, 1));
    let result = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| true,
        |_, _| {
            wave += 1;
            if wave == 1 {
                AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], mismatched))
            } else {
                AttemptOutcome::Complete(CompletedAttempt::new((), AttemptOutput::new()))
            }
        },
    );
    result.expect("one mismatch restart must fit the inclusive maximum");

    wave = 0;
    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 128, 2, 1));
    let failure = drive_attempt::<()>(
        &workspace,
        &mut ledger,
        |_, _| panic!("a mismatched LoadSet must never load"),
        |_, _| {
            wave += 1;
            AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], mismatched))
        },
    )
    .expect_err("the second mismatch restart must breach churn one");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionChurnLimit { churn: 1, .. }
    ));
    assert_eq!(wave, 2);
}

#[test]
fn bounded_loader_rejects_reservation_and_capture_integrity_failures() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let key = path_key("/integrity");

    for (expected, mode) in [
        (InputLoadIntegrityReason::KeySetMismatch, 0_u8),
        (InputLoadIntegrityReason::BasisMismatch, 1),
        (InputLoadIntegrityReason::ActualOverReservation, 2),
        (InputLoadIntegrityReason::IncompleteBoundedCapture, 3),
    ] {
        let case_key = if mode == 2 {
            InputKey::RealPath {
                path: Arc::from("/integrity-realpath"),
            }
        } else {
            key.clone()
        };
        let mut ledger = InputResolutionLedger::default();
        let failure = drive_attempt_with_bounded_io::<()>(
            &workspace,
            &mut ledger,
            |keys, basis| {
                let reservation = workspace.preflight_resolution_inputs_bounded(keys, basis)?;
                Ok(match mode {
                    0 => ResolutionInputReservationBatch::new(
                        vec![path_key("/wrong")],
                        basis,
                        reservation.entries().to_vec(),
                    )
                    .expect("reservation"),
                    1 => ResolutionInputReservationBatch::new(
                        keys.to_vec(),
                        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder(),
                        reservation.entries().to_vec(),
                    )
                    .expect("reservation"),
                    _ => reservation,
                })
            },
            |reservation| {
                let basis = reservation.basis();
                let loaded = match mode {
                    2 => LoadedResolutionInputBatch::new(
                        reservation.keys().to_vec(),
                        basis,
                        vec![LoadedResolutionInput::RealPath {
                            key: reservation.keys()[0].clone(),
                            value: Some("far-too-large".to_string()),
                            directories: Vec::new(),
                        }],
                        true,
                    ),
                    3 => LoadedResolutionInputBatch::new(
                        reservation.keys().to_vec(),
                        basis,
                        vec![LoadedResolutionInput::PathProbe {
                            key: reservation.keys()[0].clone(),
                            value: PathProbe::Absent,
                            directories: Vec::new(),
                        }],
                        false,
                    ),
                    _ => workspace
                        .load_preflighted_resolution_inputs(reservation)
                        .map(Some)?,
                };
                Ok(loaded.expect("test capture bytes must not overflow"))
            },
            |_, _| true,
            |_, basis| AttemptOutcome::NeedInputs(LoadSet::new(vec![case_key.clone()], basis)),
        )
        .expect_err("the selected integrity mutation must be terminal");
        assert!(matches!(
            failure.as_ref(),
            AttemptFailure::InputLoadIntegrity { reason, .. } if *reason == expected
        ));
    }

    let mut ledger = InputResolutionLedger::new(tightened_budgets(4, 4, 128, 2, 2));
    let mut load_calls = 0;
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |keys, basis| {
            Ok(workspace
                .preflight_resolution_inputs_bounded(keys, basis)?
                .with_reserved_bytes_for_test(u64::MAX))
        },
        |_| {
            load_calls += 1;
            unreachable!()
        },
        |_, _| true,
        |_, basis| AttemptOutcome::NeedInputs(LoadSet::new(vec![key.clone()], basis)),
    )
    .expect_err("reservation arithmetic overflow must be a byte breach");
    assert!(matches!(
        failure.as_ref(),
        AttemptFailure::InputResolutionByteLimit { .. }
    ));
    assert_eq!(load_calls, 0);
}

#[test]
fn reservation_identity_is_rejected_before_the_load_seam() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let requested = path_key("/reservation-identity");

    for (expected, corrupt_basis) in [
        (InputLoadIntegrityReason::KeySetMismatch, false),
        (InputLoadIntegrityReason::BasisMismatch, true),
    ] {
        let mut load_calls = 0;
        let mut ledger = InputResolutionLedger::default();
        let failure = drive_attempt_with_bounded_io::<()>(
            &workspace,
            &mut ledger,
            |keys, basis| {
                let reservation = workspace.preflight_resolution_inputs_bounded(keys, basis)?;
                ResolutionInputReservationBatch::new(
                    if corrupt_basis {
                        keys.to_vec()
                    } else {
                        vec![path_key("/wrong-reservation")]
                    },
                    if corrupt_basis {
                        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder()
                    } else {
                        basis
                    },
                    reservation.entries().to_vec(),
                )
                .ok_or_else(|| AttemptFailure::InputResolutionByteLimit {
                    unresolved: keys.to_vec(),
                    bytes: u64::MAX,
                })
            },
            |reservation| {
                load_calls += 1;
                workspace.load_preflighted_resolution_inputs(reservation)
            },
            |_, _| true,
            |_, basis| AttemptOutcome::NeedInputs(LoadSet::new(vec![requested.clone()], basis)),
        )
        .expect_err("the corrupted reservation must be terminal");
        assert!(matches!(
            failure.as_ref(),
            AttemptFailure::InputLoadIntegrity { reason, .. } if *reason == expected
        ));
        assert_eq!(load_calls, 0, "reservation identity is a pre-load fence");
    }
}

#[test]
fn reservation_and_actual_batch_byte_aggregation_reject_checked_arithmetic_overflow() {
    assert_eq!(
        super::resolver::checked_reservation_byte_total([Some(u64::MAX), Some(1)]),
        None
    );
    assert_eq!(
        super::resolver::checked_actual_byte_total([Some(u64::MAX), Some(1)]),
        None
    );
    assert_eq!(
        super::resolver::checked_reservation_byte_total([Some(u64::MAX)]),
        Some(u64::MAX)
    );
    assert_eq!(
        super::resolver::checked_actual_byte_total([Some(u64::MAX)]),
        Some(u64::MAX)
    );
}

#[test]
fn unsupported_input_is_terminal_before_mixed_delta_preflight_or_charging() {
    let workspace = crate::memory::MemoryWorkspace::new(Default::default());
    let unsupported = InputKey::FileContent {
        canonical: Arc::from("/unsupported.ts"),
    };
    let supported = path_key("/supported.ts");
    let mut preflight_calls = 0;
    let mut ledger = InputResolutionLedger::default();
    let failure = drive_attempt_with_bounded_io::<()>(
        &workspace,
        &mut ledger,
        |_, _| {
            preflight_calls += 1;
            unreachable!()
        },
        |_| unreachable!(),
        |_, _| true,
        |_, basis| {
            AttemptOutcome::NeedInputs(LoadSet::new(
                vec![supported.clone(), unsupported.clone()],
                basis,
            ))
        },
    )
    .expect_err("unsupported capability is terminal");
    assert_eq!(
        *failure,
        AttemptFailure::ObservationUnavailable {
            observation: verter_semantic::resolver_core::ResolverObservationKind::WholeHash,
        }
    );
    assert_eq!(preflight_calls, 0);
    assert_eq!(ledger.consumed_for_test(), (1, 0, 0, 0, 0));
}

#[test]
fn all_unsupported_input_families_map_to_their_exact_observation_kind() {
    use verter_semantic::analysis::function_program::{FunctionDeclarationRef, FunctionProgramKey};
    use verter_semantic::facts::SymbolSpace;
    use verter_semantic::resolver_core::{
        AugmentationPopulation, AugmentationTargetKey, AugmentationTargetKind, DeclarationSpace,
        FlowFunctionObservationKey, ProjectIdentity, ResolverObservationKind,
    };
    use verter_type_expr::facts::{FunctionPartIdentity, TopLevelOwnerId};

    let declaration_key = |space| InputKey::DeclBody {
        canonical: Arc::from("/decl.ts"),
        owner: TopLevelOwnerId::ordinary_file(),
        name: Arc::from("Decl"),
        space,
    };
    let augmentation = InputKey::ModuleAugmentationIndex {
        target: AugmentationTargetKey {
            project_identity: ProjectIdentity([0; 16]),
            resolve_env_hash: [0; 16],
            lib_env_hash: [0; 16],
            population: AugmentationPopulation::Base,
            target: AugmentationTargetKind::GlobalAugmentation,
        },
    };
    let skeleton = InputKey::FlowFunctionSkeleton {
        key: FlowFunctionObservationKey {
            canonical_id: Arc::from("/flow.ts"),
            function: FunctionProgramKey {
                declaration: FunctionDeclarationRef {
                    owner: TopLevelOwnerId::ordinary_file(),
                    name: Arc::from("flow"),
                    space: SymbolSpace::Value,
                },
                part: FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            },
            flow_body_stable_hash: [0; 16],
            flow_body_exact_hash: [0; 16],
            parse_env_hash: [0; 16],
        },
    };

    for (key, expected) in [
        (
            InputKey::FileContent {
                canonical: Arc::from("/whole.ts"),
            },
            ResolverObservationKind::WholeHash,
        ),
        (
            declaration_key(DeclarationSpace::Type),
            ResolverObservationKind::TypeDecl,
        ),
        (
            declaration_key(DeclarationSpace::Value),
            ResolverObservationKind::ValueDecl,
        ),
        (
            augmentation,
            ResolverObservationKind::ModuleAugmentationIndex,
        ),
        (skeleton, ResolverObservationKind::FunctionBodySkeleton),
    ] {
        assert_eq!(
            unsupported_input_failure(&key),
            Some(AttemptFailure::ObservationUnavailable {
                observation: expected,
            })
        );
    }
}

#[test]
fn oversized_manifest_is_rejected_before_parse_or_cache_and_a_later_request_is_cold() {
    let budgets = tightened_budgets(32, 128, 4_096, 16, 4);
    let workspace = crate::memory::MemoryWorkspace::new_with_input_resolution_budgets(
        Default::default(),
        budgets,
    );
    workspace.inject_file(
        "/proj/src/main.ts".to_string(),
        Arc::from("import type { Value } from 'pkg';\n"),
    );
    workspace.inject_file(
        "/proj/node_modules/pkg/index.d.ts".to_string(),
        Arc::from("export interface Value {}\n"),
    );
    workspace.inject_file(
        "/proj/node_modules/pkg/package.json".to_string(),
        Arc::from(format!(
            r#"{{"types":"./index.d.ts","padding":"{}"}}"#,
            "x".repeat(5_000)
        )),
    );
    WorkspaceAccess::configure_resolver(&workspace, vec![project("/proj", "/proj/tsconfig.json")]);

    let refused =
        WorkspaceRead::resolve_import_outcome(&workspace, "/proj/src/main.ts", "pkg", CONTEXT);
    assert!(refused.result().is_none());
    assert_eq!(
        refused.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::BudgetExceeded)
    );
    assert_eq!(
        workspace.engine.package_index.read().found_count(),
        0,
        "the oversized manifest must not be parsed or admitted"
    );
    assert!(
        workspace
            .engine
            .cached_resolution_query_for_test(
                "/proj/src/main.ts",
                "pkg",
                CONTEXT,
                workspace.resolution_population(),
            )
            .is_none(),
        "the budget-refused negative must not enter the resolution cache"
    );
    assert!(
        workspace
            .engine
            .reverse_deps_for("/proj/node_modules/pkg/index.d.ts")
            .is_empty(),
        "a byte-limit refusal must not admit a reverse edge from discarded resolver output"
    );

    workspace.inject_file(
        "/proj/node_modules/pkg/package.json".to_string(),
        Arc::from(r#"{"types":"./index.d.ts"}"#),
    );
    let later =
        WorkspaceRead::resolve_import_outcome(&workspace, "/proj/src/main.ts", "pkg", CONTEXT);
    assert_eq!(
        later.result().map(|result| result.source_id.as_str()),
        Some("/proj/node_modules/pkg/index.d.ts")
    );
    assert!(later.is_cacheable());
    assert!(
        !later.trace().reused(),
        "the later request must recompute cold"
    );
}

#[test]
fn late_integrity_failure_keeps_positive_and_negative_manifest_state_operation_local() {
    for (source, label) in [
        (
            Some(Arc::<str>::from(r#"{"types":"./index.d.ts"}"#)),
            "positive",
        ),
        (None, "negative"),
    ] {
        let workspace = crate::memory::MemoryWorkspace::new(Default::default());
        let manifest_path = "/proj/node_modules/pkg/package.json";
        if let Some(source) = source {
            workspace.inject_file(manifest_path.to_string(), source);
        }
        let key = InputKey::PackageManifest {
            directory: Arc::from("/proj/node_modules/pkg"),
        };
        let mut ledger = InputResolutionLedger::default();
        let failure = drive_attempt_with_bounded_io(
            &workspace,
            &mut ledger,
            |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
            |reservation| {
                let loaded = workspace.load_preflighted_resolution_inputs(reservation)?;
                LoadedResolutionInputBatch::new(
                    loaded.keys().to_vec(),
                    loaded.basis(),
                    loaded.entries().to_vec(),
                    false,
                )
                .ok_or_else(|| AttemptFailure::InputLoadIntegrity {
                    unresolved: reservation.keys().to_vec(),
                    reason: InputLoadIntegrityReason::IncompleteBoundedCapture,
                })
            },
            |_inputs, _output| true,
            |_view, basis| KernelAttempt::<()>::NeedInputs(LoadSet::new(vec![key.clone()], basis)),
        )
        .expect_err("the deliberately incomplete batch must fail integrity");

        assert!(matches!(
            *failure,
            AttemptFailure::InputLoadIntegrity {
                reason: InputLoadIntegrityReason::IncompleteBoundedCapture,
                ..
            }
        ));
        assert!(
            workspace
                .engine
                .package_index
                .read()
                .get_cached(manifest_path)
                .is_none(),
            "{label} package-index state must remain cold after late integrity failure"
        );
    }
}

#[test]
fn later_unique_key_limit_does_not_publish_an_earlier_manifest_load() {
    let budgets = tightened_budgets(8, 1, 32_768, 4, 2);
    let workspace = crate::memory::MemoryWorkspace::new_with_input_resolution_budgets(
        Default::default(),
        budgets,
    );
    let manifest_path = "/proj/node_modules/pkg/package.json";
    workspace.inject_file(
        manifest_path.to_string(),
        Arc::from(r#"{"types":"./index.d.ts"}"#),
    );
    let manifest = InputKey::PackageManifest {
        directory: Arc::from("/proj/node_modules/pkg"),
    };
    let later = path_key("/proj/later.ts");
    let mut runs = 0;
    let mut ledger = InputResolutionLedger::new(budgets);
    let failure = drive_attempt(
        &workspace,
        &mut ledger,
        |_inputs, _output| true,
        |_view, basis| {
            runs += 1;
            let key = if runs == 1 {
                manifest.clone()
            } else {
                later.clone()
            };
            KernelAttempt::<()>::NeedInputs(LoadSet::new(vec![key], basis))
        },
    )
    .expect_err("the second distinct key must breach the tightened unique-key maximum");

    assert!(matches!(
        *failure,
        AttemptFailure::InputResolutionUniqueKeyLimit { unique_keys: 1, .. }
    ));
    assert!(workspace
        .engine
        .package_index
        .read()
        .get_cached(manifest_path)
        .is_none());
}

#[test]
fn late_outer_churn_keeps_manifest_caches_and_reverse_edges_cold() {
    for (manifest_source, label) in [
        (Some(r#"{"types":"./index.d.ts"}"#), "positive"),
        (None, "negative"),
    ] {
        let budgets = tightened_budgets(32, 128, 32_768, 16, 1);
        let workspace = Arc::new(
            crate::memory::MemoryWorkspace::new_with_input_resolution_budgets(
                Default::default(),
                budgets,
            ),
        );
        let importer = "/proj/src/main.ts";
        let manifest_path = "/proj/node_modules/pkg/package.json";
        let target = "/proj/node_modules/pkg/index.d.ts";
        workspace.inject_file(importer.to_string(), Arc::from("import 'pkg';"));
        workspace.inject_file(target.to_string(), Arc::from("export {};"));
        if let Some(source) = manifest_source {
            workspace.inject_file(manifest_path.to_string(), Arc::from(source));
        }
        WorkspaceAccess::configure_resolver(
            workspace.as_ref(),
            vec![project("/proj", "/proj/tsconfig.json")],
        );

        let mutations = Arc::new(AtomicUsize::new(0));
        let mutate_workspace = Arc::clone(&workspace);
        let mutation_count = Arc::clone(&mutations);
        let outcome = resolution_test_hooks::with_repeating_hook(
            ResolutionPhase::PreAdmissionValidation,
            move || {
                mutation_count.fetch_add(1, Ordering::AcqRel);
                mutate_workspace
                    .engine
                    .bump_content_generation_for("/proj/unrelated.ts");
            },
            || WorkspaceRead::resolve_import_outcome(workspace.as_ref(), importer, "pkg", CONTEXT),
        );

        assert_eq!(mutations.load(Ordering::Acquire), 2);
        assert_eq!(
            outcome.non_admission_reason(),
            Some(verter_audit::NonAdmissionReason::BudgetExceeded)
        );
        assert!(
            workspace
                .engine
                .package_index
                .read()
                .get_cached(manifest_path)
                .is_none(),
            "{label} manifest state must remain operation-local after churn exhaustion"
        );
        assert!(
            workspace.engine.reverse_deps_for(target).is_empty(),
            "the discarded {label} attempt must not publish a reverse edge"
        );
        assert!(workspace
            .engine
            .cached_resolution_query_for_test(
                importer,
                "pkg",
                CONTEXT,
                workspace.resolution_population(),
            )
            .is_none());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_payload_and_package_index_stay_cold_after_late_batch_integrity_failure() {
    let dir = tempfile::tempdir().expect("temporary workspace");
    let package_dir = dir.path().join("node_modules/pkg");
    std::fs::create_dir_all(&package_dir).expect("package directory");
    let manifest = package_dir.join("package.json");
    std::fs::write(&manifest, r#"{"types":"./index.d.ts"}"#).expect("manifest write");
    let package_dir =
        verter_semantic::resolver_core::normalize_canonical_id(&package_dir.to_string_lossy());
    let manifest =
        verter_semantic::resolver_core::normalize_canonical_id(&manifest.to_string_lossy());
    let workspace = crate::filesystem::FilesystemWorkspace::new(Default::default());
    let key = InputKey::PackageManifest {
        directory: Arc::from(package_dir),
    };
    let mut ledger = InputResolutionLedger::default();

    let _ = drive_attempt_with_bounded_io(
        &workspace,
        &mut ledger,
        |keys, basis| workspace.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| {
            let loaded = workspace.load_preflighted_resolution_inputs(reservation)?;
            LoadedResolutionInputBatch::new(
                loaded.keys().to_vec(),
                loaded.basis(),
                loaded.entries().to_vec(),
                false,
            )
            .ok_or_else(|| AttemptFailure::InputLoadIntegrity {
                unresolved: reservation.keys().to_vec(),
                reason: InputLoadIntegrityReason::IncompleteBoundedCapture,
            })
        },
        |_inputs, _output| true,
        |_view, basis| KernelAttempt::<()>::NeedInputs(LoadSet::new(vec![key.clone()], basis)),
    )
    .expect_err("the deliberately incomplete batch must fail integrity");

    assert!(
        !workspace.engine.snapshot.read().contains(&manifest),
        "native manifest payload must remain operation-local on terminal failure"
    );
    assert!(workspace
        .engine
        .package_index
        .read()
        .get_cached(&manifest)
        .is_none());
}

#[test]
fn workspace_ingress_uses_only_the_semantic_owned_whole_policy_value() {
    let tightened =
        InputResolutionBudgets::try_tightened_with_retention(128, 512, 65_536, 32, 4, 64, 32)
            .expect("test policy");
    let memory_default = crate::memory::MemoryWorkspace::new(Default::default());
    let memory_tightened = crate::memory::MemoryWorkspace::new_with_input_resolution_budgets(
        Default::default(),
        tightened,
    );
    let filesystem_default = crate::filesystem::FilesystemWorkspace::new(Default::default());
    let filesystem_tightened =
        crate::filesystem::FilesystemWorkspace::new_with_input_resolution_budgets(
            Default::default(),
            tightened,
        );

    assert_eq!(
        memory_default.engine.input_resolution_budgets,
        InputResolutionBudgets::default()
    );
    assert_eq!(memory_tightened.engine.input_resolution_budgets, tightened);
    assert_eq!(
        filesystem_default.engine.input_resolution_budgets,
        InputResolutionBudgets::default()
    );
    assert_eq!(
        filesystem_tightened.engine.input_resolution_budgets,
        tightened
    );
}
