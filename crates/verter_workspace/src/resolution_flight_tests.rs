//! Concurrent identical cold queries share one producer run; each
//! subscriber validates the delivered answer for its own view; a cancelled
//! subscriber detaches without stopping the flight; an abandoned flight
//! sends its subscribers back to resolve for themselves.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};

use crate::engine::resolution_test_hooks::{self, ResolutionPhase};
use crate::engine::ResolutionOperation;
use crate::memory::MemoryWorkspace;
use crate::resolution_currency::{
    ResolutionEvidenceSource, ResolutionOutcome, ResolutionOverlaySnapshot,
};
use crate::traits::WorkspaceRead;

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::CodegenBlocker,
    kind: ResolveRequestKind::TypeImport,
};
const MAIN: &str = "/p/main.ts";

fn workspace(files: &[(&str, &str)]) -> Arc<MemoryWorkspace> {
    let workspace = MemoryWorkspace::new(Default::default());
    workspace.inject_file(MAIN.to_string(), Arc::from("export {}\n"));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    Arc::new(workspace)
}

fn target(outcome: &ResolutionOutcome) -> Option<String> {
    outcome.result().map(|result| result.source_id.clone())
}

#[cfg(feature = "semantic-observe")]
fn producer_runs(workspace: &MemoryWorkspace) -> u64 {
    workspace
        .vfs_provenance_snapshot()
        .import_resolution_cache_miss_count
}

/// Wait (bounded) until `count` subscribers wait on `./dep`'s flight.
fn await_subscribers(workspace: &MemoryWorkspace, overlay_domain: bool, count: usize) {
    let population = WorkspaceRead::resolution_population(workspace);
    let deadline = Instant::now() + Duration::from_secs(20);
    while workspace.engine.flight_subscribers_for_test(
        MAIN,
        "./dep",
        CONTEXT,
        population,
        overlay_domain,
    ) < count
    {
        assert!(
            Instant::now() < deadline,
            "{count} subscribers never joined the flight"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

type Answers = Arc<Mutex<Vec<(Option<String>, bool)>>>;

fn record(answers: &Answers, outcome: &ResolutionOutcome) {
    answers
        .lock()
        .unwrap()
        .push((target(outcome), outcome.trace().reused()));
}

#[test]
fn concurrent_identical_cold_queries_run_one_producer() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    let answers: Answers = Arc::default();
    let followers = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_answers = Arc::clone(&answers);
    let hook_followers = Arc::clone(&followers);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            for _ in 0..2 {
                let workspace = Arc::clone(&hook_workspace);
                let answers = Arc::clone(&hook_answers);
                hook_followers
                    .lock()
                    .unwrap()
                    .push(std::thread::spawn(move || {
                        record(
                            &answers,
                            &workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
                        );
                    }));
            }
            await_subscribers(&hook_workspace, false, 2);
        },
        || workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
    );
    for follower in followers.lock().unwrap().drain(..) {
        follower.join().unwrap();
    }
    assert_eq!(target(&leader), Some("/p/dep.ts".to_string()));
    assert_eq!(
        *answers.lock().unwrap(),
        vec![(Some("/p/dep.ts".to_string()), true); 2],
        "both subscribers adopt the delivered answer"
    );
    // Reads optional audit capture: runs in the instrumentation lane.
    #[cfg(feature = "semantic-observe")]
    assert_eq!(
        producer_runs(&workspace),
        1,
        "one producer run for three demands"
    );
    assert_eq!(workspace.resource_snapshot().resolution_flights, 0);
}

/// A retention account that refuses every reservation.
struct RefusingAccount;

impl verter_session_query::retention::resolution_charge::ResolutionRetentionAccount
    for RefusingAccount
{
    fn reserve_retained(
        &self,
        _bytes: usize,
    ) -> Option<verter_session_query::retention::resolution_charge::ResolutionRetentionCharge> {
        None
    }
}

/// Under retention pressure the leader's complete answer is not retained,
/// yet still delivered: its subscribers adopt it, restating its witness,
/// instead of each running the producer again. Every demand — leader and
/// subscribers alike — delivers the answer unretained, so no consumer can
/// root a retained entry on it.
///
/// Mutation recipes: on a refused reservation, leave the flight without a
/// delivery (each subscriber then resolves for itself and the producer runs
/// three times); or let a subscriber keep its adopted answer cacheable (it
/// then publishes as admitted).
#[test]
fn a_refused_retention_still_serves_the_flight_subscribers() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    crate::traits::WorkspaceAccess::install_resolution_retention(
        workspace.as_ref(),
        Arc::new(RefusingAccount),
    );
    let outcomes: Arc<Mutex<Vec<ResolutionOutcome>>> = Arc::default();
    let followers = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_outcomes = Arc::clone(&outcomes);
    let hook_followers = Arc::clone(&followers);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            for _ in 0..2 {
                let workspace = Arc::clone(&hook_workspace);
                let outcomes = Arc::clone(&hook_outcomes);
                hook_followers
                    .lock()
                    .unwrap()
                    .push(std::thread::spawn(move || {
                        let outcome = workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT);
                        outcomes.lock().unwrap().push(outcome);
                    }));
            }
            await_subscribers(&hook_workspace, false, 2);
        },
        || workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
    );
    for follower in followers.lock().unwrap().drain(..) {
        follower.join().unwrap();
    }
    assert_eq!(target(&leader), Some("/p/dep.ts".to_string()));
    let fresh = WorkspaceRead::capture_resolution_world(workspace.as_ref()).expect("captured");
    let assert_unretained = |outcome: ResolutionOutcome, who: &str| {
        assert_eq!(
            outcome.non_admission_reason(),
            Some(verter_audit::NonAdmissionReason::RetentionPressure),
            "{who}"
        );
        match outcome.into_publication() {
            crate::ResolutionPublication::Unretained(unretained) => {
                assert_eq!(
                    unretained.result().map(|result| result.source_id.as_str()),
                    Some("/p/dep.ts"),
                    "{who} delivers the complete answer"
                );
                assert!(
                    unretained.witness().validates(fresh.as_ref()),
                    "{who}'s witness is the answer's own, valid in its world"
                );
            }
            other => panic!("{who} delivers the answer unretained, got {other:?}"),
        }
    };
    assert_unretained(leader, "the leader");
    for outcome in outcomes.lock().unwrap().drain(..) {
        assert!(
            outcome.trace().reused(),
            "the subscriber adopted the delivery"
        );
        assert_unretained(outcome, "a subscriber");
    }
    // Reads optional audit capture: runs in the instrumentation lane.
    #[cfg(feature = "semantic-observe")]
    assert_eq!(
        producer_runs(&workspace),
        1,
        "one producer run for three demands under retention pressure"
    );
    assert_eq!(workspace.resource_snapshot().resolution.slots, 0);
    assert_eq!(workspace.resource_snapshot().resolution_flights, 0);
}

/// A flight for an importer the workspace has already retired delivers an
/// answer no slot holds, yet nothing refused to retain it: the leader and
/// every subscriber that adopts it answer admitted under its own witness,
/// never as retention pressure.
///
/// Mutation recipe: deliver the retired importer's answer on the lane a
/// refused retention uses. The subscribers then publish unretained, with a
/// pressure non-admission no account reported.
#[test]
fn a_retired_importers_flight_subscribers_stay_admitted() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    workspace.remove_file(MAIN);
    let outcomes: Arc<Mutex<Vec<ResolutionOutcome>>> = Arc::default();
    let followers = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_outcomes = Arc::clone(&outcomes);
    let hook_followers = Arc::clone(&followers);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            for _ in 0..2 {
                let workspace = Arc::clone(&hook_workspace);
                let outcomes = Arc::clone(&hook_outcomes);
                hook_followers
                    .lock()
                    .unwrap()
                    .push(std::thread::spawn(move || {
                        let outcome = workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT);
                        outcomes.lock().unwrap().push(outcome);
                    }));
            }
            await_subscribers(&hook_workspace, false, 2);
        },
        || workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
    );
    for follower in followers.lock().unwrap().drain(..) {
        follower.join().unwrap();
    }
    let assert_admitted = |outcome: ResolutionOutcome, who: &str| {
        assert_eq!(outcome.non_admission_reason(), None, "{who}");
        match outcome.into_publication() {
            crate::ResolutionPublication::Admitted(admitted) => assert_eq!(
                admitted.result().map(|result| result.source_id.as_str()),
                Some("/p/dep.ts"),
                "{who} delivers the complete answer"
            ),
            other => panic!("{who} answers admitted, got {other:?}"),
        }
    };
    assert_admitted(leader, "the leader");
    for outcome in outcomes.lock().unwrap().drain(..) {
        assert!(
            outcome.trace().reused(),
            "the subscriber adopted the delivery"
        );
        assert_admitted(outcome, "a subscriber");
    }
    #[cfg(feature = "semantic-observe")]
    assert_eq!(
        producer_runs(&workspace),
        1,
        "one producer run for three demands"
    );
    assert_eq!(workspace.resource_snapshot().resolution.slots, 0);
    assert_eq!(workspace.resource_snapshot().resolution_flights, 0);
}

#[test]
fn a_subscriber_never_adopts_an_answer_that_is_not_valid_for_its_overlay() {
    let workspace = workspace(&[]);
    let creates = ResolutionOverlaySnapshot::new(
        [(
            "/p/dep.ts".to_string(),
            Arc::<str>::from("export const dep = 1\n"),
        )],
        [],
    );
    let answers: Answers = Arc::default();
    let followers = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_answers = Arc::clone(&answers);
    let hook_followers = Arc::clone(&followers);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let workspace = Arc::clone(&hook_workspace);
            let answers = Arc::clone(&hook_answers);
            hook_followers
                .lock()
                .unwrap()
                .push(std::thread::spawn(move || {
                    // Another overlay that changes a fact, but not the dep.
                    let unrelated = ResolutionOverlaySnapshot::new(
                        [("/q/scratch.ts".to_string(), Arc::<str>::from("export {}\n"))],
                        [],
                    );
                    record(
                        &answers,
                        &workspace.resolve_import_outcome_with_overlay(
                            &unrelated, MAIN, "./dep", CONTEXT,
                        ),
                    );
                }));
            await_subscribers(&hook_workspace, true, 1);
        },
        || workspace.resolve_import_outcome_with_overlay(&creates, MAIN, "./dep", CONTEXT),
    );
    for follower in followers.lock().unwrap().drain(..) {
        follower.join().unwrap();
    }
    assert_eq!(target(&leader), Some("/p/dep.ts".to_string()));
    assert_eq!(
        *answers.lock().unwrap(),
        vec![(None, false)],
        "the subscriber resolves for its own overlay, which has no dep"
    );
}

#[test]
fn a_cancelled_subscriber_detaches_and_the_flight_serves_the_others() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    let answers: Answers = Arc::default();
    let cancelled_outcome = Arc::new(Mutex::new(None));
    let threads = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_answers = Arc::clone(&answers);
    let hook_cancelled = Arc::clone(&cancelled_outcome);
    let hook_threads = Arc::clone(&threads);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let cancel = Arc::new(AtomicBool::new(false));
            {
                let workspace = Arc::clone(&hook_workspace);
                let cancel = Arc::clone(&cancel);
                let outcome = Arc::clone(&hook_cancelled);
                hook_threads
                    .lock()
                    .unwrap()
                    .push(std::thread::spawn(move || {
                        let mut ledger = crate::resolver::InputResolutionLedger::new(
                            workspace.engine.input_resolution_budgets,
                        );
                        let is_cancelled = || cancel.load(Ordering::SeqCst);
                        let result = workspace
                            .engine
                            .resolve_import_outcome_for_published_in_operation(
                                &*workspace,
                                ResolutionEvidenceSource::ReaderAuthoritative,
                                MAIN,
                                "./dep",
                                CONTEXT,
                                ResolutionOperation::unpinned(&mut ledger, &|| true)
                                    .cancelled_by(&is_cancelled),
                            );
                        *outcome.lock().unwrap() = Some(result.non_admission_reason());
                    }));
            }
            await_subscribers(&hook_workspace, false, 1);
            {
                let workspace = Arc::clone(&hook_workspace);
                let answers = Arc::clone(&hook_answers);
                hook_threads
                    .lock()
                    .unwrap()
                    .push(std::thread::spawn(move || {
                        record(
                            &answers,
                            &workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
                        );
                    }));
            }
            await_subscribers(&hook_workspace, false, 2);
            cancel.store(true, Ordering::SeqCst);
            // The cancelled subscriber detaches while the flight still runs.
            await_detached(&hook_cancelled);
        },
        || workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
    );
    for thread in threads.lock().unwrap().drain(..) {
        thread.join().unwrap();
    }
    assert_eq!(
        *cancelled_outcome.lock().unwrap(),
        Some(Some(verter_audit::NonAdmissionReason::Cancelled)),
        "the cancelled subscriber returns a typed refusal"
    );
    assert_eq!(target(&leader), Some("/p/dep.ts".to_string()));
    assert_eq!(
        *answers.lock().unwrap(),
        vec![(Some("/p/dep.ts".to_string()), true)],
        "the flight still serves its other subscriber"
    );
    // Reads optional audit capture: runs in the instrumentation lane.
    #[cfg(feature = "semantic-observe")]
    assert_eq!(producer_runs(&workspace), 1);
}

fn await_detached(outcome: &Mutex<Option<Option<verter_audit::NonAdmissionReason>>>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while outcome.lock().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the cancelled subscriber never detached"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn an_abandoned_flight_sends_its_subscribers_back_to_resolve() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    let answers: Answers = Arc::default();
    let followers = Arc::new(Mutex::new(Vec::new()));
    let hook_workspace = Arc::clone(&workspace);
    let hook_answers = Arc::clone(&answers);
    let hook_followers = Arc::clone(&followers);
    let leader = resolution_test_hooks::with_hook(
        ResolutionPhase::PreAdmissionValidation,
        move || {
            let workspace = Arc::clone(&hook_workspace);
            let answers = Arc::clone(&hook_answers);
            hook_followers
                .lock()
                .unwrap()
                .push(std::thread::spawn(move || {
                    record(
                        &answers,
                        &workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
                    );
                }));
            await_subscribers(&hook_workspace, false, 1);
        },
        || {
            // The producer's final fence refuses: nothing is admitted.
            let mut ledger = crate::resolver::InputResolutionLedger::new(
                workspace.engine.input_resolution_budgets,
            );
            workspace
                .engine
                .resolve_import_outcome_for_published_in_operation(
                    &*workspace,
                    ResolutionEvidenceSource::ReaderAuthoritative,
                    MAIN,
                    "./dep",
                    CONTEXT,
                    ResolutionOperation::unpinned(&mut ledger, &|| false),
                )
        },
    );
    for follower in followers.lock().unwrap().drain(..) {
        follower.join().unwrap();
    }
    assert!(
        leader.non_admission_reason().is_some(),
        "the producer admitted nothing"
    );
    assert_eq!(
        *answers.lock().unwrap(),
        vec![(Some("/p/dep.ts".to_string()), false)],
        "the subscriber resolved for itself"
    );
    // Reads optional audit capture: runs in the instrumentation lane.
    #[cfg(feature = "semantic-observe")]
    assert_eq!(
        producer_runs(&workspace),
        1,
        "the refused producer admitted nothing; the one admitted run is the subscriber's own"
    );
    assert_eq!(workspace.resource_snapshot().resolution_flights, 0);
}
