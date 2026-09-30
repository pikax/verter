//! Barrier-forced interleavings of the cooperative capture wait and the
//! compatible admission check.
//!
//! - A request cancelled while it (or the flight leader it waits on) waits
//!   on the publication gate, or between an attempt's capture and its
//!   admission, publishes nothing stale: it either returns a typed
//!   cancellation or the answer of the world after the concurrent write, and
//!   neither a sibling sharing its flight nor the retry is poisoned.
//! - A file deleted and then re-added with different content between an
//!   attempt's capture and its admission never lets the attempt admit an
//!   answer read from the old file: the attempt restarts into the new world.
//!
//! Every interleaving is forced by the resolution phase hooks and channels;
//! nothing depends on timing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use verter_semantic::resolver_core::{ResolutionContext, ResolvePhase, ResolveRequestKind};

use crate::changes::WorkspaceChange;
use crate::engine::resolution_test_hooks::{self, ResolutionPhase};
use crate::engine::ResolutionOperation;
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolution_currency::{ResolutionEvidenceSource, ResolutionOutcome};
use crate::traits::WorkspaceRead;
use crate::ResolutionPublication;

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::CodegenBlocker,
    kind: ResolveRequestKind::TypeImport,
};
const ESM: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::CodegenBlocker,
    kind: ResolveRequestKind::EsmImport,
};
const MAIN: &str = "/p/main.ts";
const DEP: &str = "/p/dep.ts";
/// Bounds every channel wait so a broken interleaving fails instead of
/// hanging; no correct run comes near it.
const WAIT: Duration = Duration::from_secs(30);

fn workspace(files: &[(&str, &str)]) -> Arc<MemoryWorkspace> {
    let workspace = MemoryWorkspace::new(MemoryOptions::default());
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    Arc::new(workspace)
}

fn target(outcome: &ResolutionOutcome) -> Option<String> {
    outcome.result().map(|result| result.source_id.clone())
}

fn add_dep() -> Vec<WorkspaceChange> {
    vec![WorkspaceChange::FileChanged {
        canonical_id: DEP.to_string(),
        source: Some(Arc::from("export const dep = 1\n")),
    }]
}

/// Apply `changes` on a writer thread that stops INSIDE its world-write
/// window (epoch odd, publication gate held) until `release` is sent.
/// Returns once the writer is inside the window.
fn hold_a_write_open(
    workspace: &Arc<MemoryWorkspace>,
    changes: Vec<WorkspaceChange>,
) -> (mpsc::Sender<()>, std::thread::JoinHandle<()>) {
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let workspace = Arc::clone(workspace);
    let writer = std::thread::spawn(move || {
        resolution_test_hooks::with_hook(
            ResolutionPhase::WorldWriteHeld,
            move || {
                held_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(WAIT)
                    .expect("the test releases the writer");
            },
            || {
                workspace.apply_changes(changes);
            },
        );
    });
    held_rx
        .recv_timeout(WAIT)
        .expect("the writer enters its world-write window");
    (release_tx, writer)
}

/// Resolve `MAIN`'s `./dep` through an operation `cancelled` can cancel.
fn resolve_cancellable(workspace: &MemoryWorkspace, cancelled: &AtomicBool) -> ResolutionOutcome {
    let mut ledger =
        crate::resolver::InputResolutionLedger::new(workspace.engine.input_resolution_budgets);
    let is_cancelled = || cancelled.load(Ordering::SeqCst);
    workspace
        .engine
        .resolve_import_outcome_for_published_in_operation(
            workspace,
            ResolutionEvidenceSource::ReaderAuthoritative,
            MAIN,
            "./dep",
            CONTEXT,
            ResolutionOperation::unpinned(&mut ledger, &|| true).cancelled_by(&is_cancelled),
        )
}

/// A cancelled request's outcome is a typed cancellation, or the answer of
/// the world after the concurrent write — never the answer of the world
/// before it.
fn assert_cancelled_publishes_nothing_stale(outcome: &ResolutionOutcome) {
    if outcome.non_admission_reason() == Some(verter_audit::NonAdmissionReason::Cancelled) {
        assert!(!outcome.is_cacheable());
        return;
    }
    assert_eq!(
        target(outcome),
        Some(DEP.to_string()),
        "a cancelled request that still answers answers for the world after \
         the write, never the miss it would have read before it: {:?}",
        outcome.non_admission_reason()
    );
}

/// The retry after the interleaving answers for the current world.
fn assert_the_retry_is_current(workspace: &MemoryWorkspace) {
    let retry = workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT);
    assert_eq!(target(&retry), Some(DEP.to_string()));
    assert!(
        retry.is_cacheable(),
        "the retry is admitted: {:?}",
        retry.non_admission_reason()
    );
}

fn await_subscribers(workspace: &MemoryWorkspace, count: usize) {
    let population = WorkspaceRead::resolution_population(workspace);
    let deadline = Instant::now() + WAIT;
    while workspace
        .engine
        .flight_subscribers_for_test(MAIN, "./dep", CONTEXT, population, false)
        < count
    {
        assert!(
            Instant::now() < deadline,
            "{count} subscribers never joined the flight"
        );
        std::thread::yield_now();
    }
}

/// A request cancelled while it waits on the publication gate (a writer
/// adding the file it resolves is inside its window) publishes nothing
/// stale, and the retry answers for the new world.
#[test]
fn a_request_cancelled_while_it_waits_on_the_publication_gate_publishes_nothing_stale() {
    let workspace = workspace(&[(MAIN, "export {}\n")]);
    let (release, writer) = hold_a_write_open(&workspace, add_dep());

    let cancelled = Arc::new(AtomicBool::new(false));
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let request = {
        let workspace = Arc::clone(&workspace);
        let cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            resolution_test_hooks::with_hook(
                ResolutionPhase::PublicationGateWait,
                move || waiting_tx.send(()).unwrap(),
                || resolve_cancellable(&workspace, &cancelled),
            )
        })
    };
    waiting_rx
        .recv_timeout(WAIT)
        .expect("the request waits on the publication gate");
    cancelled.store(true, Ordering::SeqCst);
    release.send(()).unwrap();
    writer.join().expect("the writer completes");

    assert_cancelled_publishes_nothing_stale(&request.join().expect("the request returns"));
    assert_the_retry_is_current(&workspace);
}

/// A request cancelled between its capture and its admission, while a
/// write adding the file it resolves lands, publishes nothing stale, and
/// the retry answers for the new world.
#[test]
fn a_request_cancelled_between_capture_and_admission_publishes_nothing_stale() {
    let workspace = workspace(&[(
        MAIN,
        "export {}
",
    )]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let hook_workspace = Arc::clone(&workspace);
    let hook_cancelled = Arc::clone(&cancelled);
    let mut landed = false;
    let outcome = resolution_test_hooks::with_every_phase_hook(
        move |phase| {
            if phase != ResolutionPhase::PreAdmissionValidation || landed {
                return;
            }
            landed = true;
            // Captured and resolved (a miss); now cancelled, and the file
            // appears before the admission check.
            hook_cancelled.store(true, Ordering::SeqCst);
            let workspace = Arc::clone(&hook_workspace);
            std::thread::spawn(move || {
                workspace.apply_changes(add_dep());
            })
            .join()
            .expect("the write applies");
        },
        || resolve_cancellable(&workspace, &cancelled),
    );
    assert!(cancelled.load(Ordering::SeqCst), "the interleaving ran");
    assert_cancelled_publishes_nothing_stale(&outcome);
    assert_the_retry_is_current(&workspace);
}

/// A subscriber cancelled while the flight leader it waits on is between
/// its capture and its admission — and then waiting on the publication
/// gate behind a writer adding the resolved file — detaches with a typed
/// cancellation. The leader's pre-write attempt is refused at admission and
/// restarted, so the sibling sharing the flight receives the new world's
/// answer, and so does the retry: nothing stale is delivered or cached.
#[test]
fn a_cancelled_subscriber_poisons_neither_its_sibling_nor_the_retry() {
    let workspace = workspace(&[(MAIN, "export {}\n")]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let subscriber_outcome = Arc::new(Mutex::new(None));
    let sibling_outcome = Arc::new(Mutex::new(None));
    let threads = Arc::new(Mutex::new(Vec::new()));
    let writer_release = Arc::new(Mutex::new(None));

    let hook_workspace = Arc::clone(&workspace);
    let hook_cancelled = Arc::clone(&cancelled);
    let hook_subscriber = Arc::clone(&subscriber_outcome);
    let hook_sibling = Arc::clone(&sibling_outcome);
    let hook_threads = Arc::clone(&threads);
    let hook_release = Arc::clone(&writer_release);
    let mut stage = 0usize;
    let leader = resolution_test_hooks::with_every_phase_hook(
        move |phase| match (stage, phase) {
            // The leader has captured the pre-write world and resolved
            // (a miss). Subscribers join its flight, then a writer adding
            // the file enters its window.
            (0, ResolutionPhase::PreAdmissionValidation) => {
                stage = 1;
                {
                    let workspace = Arc::clone(&hook_workspace);
                    let cancelled = Arc::clone(&hook_cancelled);
                    let outcome = Arc::clone(&hook_subscriber);
                    hook_threads
                        .lock()
                        .unwrap()
                        .push(std::thread::spawn(move || {
                            let result = resolve_cancellable(&workspace, &cancelled);
                            *outcome.lock().unwrap() = Some(result);
                        }));
                }
                await_subscribers(&hook_workspace, 1);
                {
                    let workspace = Arc::clone(&hook_workspace);
                    let outcome = Arc::clone(&hook_sibling);
                    hook_threads
                        .lock()
                        .unwrap()
                        .push(std::thread::spawn(move || {
                            let result = workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT);
                            *outcome.lock().unwrap() = Some(result);
                        }));
                }
                await_subscribers(&hook_workspace, 2);
                *hook_release.lock().unwrap() = Some(hold_a_write_open(&hook_workspace, add_dep()));
            }
            // The leader is about to wait on the publication gate the
            // writer holds: cancel the subscriber, let it detach, then
            // release the writer.
            (1, ResolutionPhase::PublicationGateWait) => {
                stage = 2;
                hook_cancelled.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + WAIT;
                while hook_subscriber.lock().unwrap().is_none() {
                    assert!(
                        Instant::now() < deadline,
                        "the cancelled subscriber never detached"
                    );
                    std::thread::yield_now();
                }
                let (release, writer) = hook_release.lock().unwrap().take().unwrap();
                release.send(()).unwrap();
                writer.join().expect("the writer completes");
            }
            _ => {}
        },
        || workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT),
    );
    for thread in threads.lock().unwrap().drain(..) {
        thread.join().unwrap();
    }

    let subscriber = subscriber_outcome.lock().unwrap().take().unwrap();
    assert_eq!(
        subscriber.non_admission_reason(),
        Some(verter_audit::NonAdmissionReason::Cancelled),
        "the cancelled subscriber detaches with a typed refusal"
    );
    assert!(subscriber.result().is_none() && !subscriber.is_cacheable());
    assert_eq!(
        target(&leader),
        Some(DEP.to_string()),
        "the leader's pre-write miss is never admitted: it restarts into the new world"
    );
    assert!(leader.is_cacheable(), "{:?}", leader.non_admission_reason());
    let sibling = sibling_outcome.lock().unwrap().take().unwrap();
    assert_eq!(
        target(&sibling),
        Some(DEP.to_string()),
        "the sibling sharing the flight receives the new world's answer"
    );
    assert_the_retry_is_current(&workspace);
}

// ── Delete, then re-add with different content ──

/// Where the delete-then-re-add lands inside the demanded resolution.
#[derive(Clone, Copy, Debug)]
enum Point {
    /// Between two input rounds of the attempt.
    BetweenInputRounds,
    /// After every read, before the admission check.
    BeforeAdmission,
}

/// Resolve `importer`'s `specifier` in a workspace holding `before`, with
/// `delete` then `re_add` applied as TWO separate writes by another thread
/// at `point` of the first attempt. Returns the outcome, the outer restarts
/// it was charged, and the workspace.
fn resolve_across_delete_and_re_add(
    before: &[(&str, &str)],
    importer: &str,
    specifier: &str,
    context: ResolutionContext,
    delete: Vec<WorkspaceChange>,
    re_add: Vec<WorkspaceChange>,
    point: Point,
) -> (ResolutionOutcome, usize, Arc<MemoryWorkspace>) {
    let workspace = workspace(before);
    let hook_workspace = Arc::clone(&workspace);
    let mut writes = Some((delete, re_add));
    let mut rounds = 0usize;
    let _ = crate::resolver::take_outer_restarts_for_test();
    let outcome = resolution_test_hooks::with_every_phase_hook(
        move |phase| {
            let due = match (point, phase) {
                (Point::BetweenInputRounds, ResolutionPhase::DriverRound) => {
                    rounds += 1;
                    rounds == 2
                }
                (Point::BeforeAdmission, ResolutionPhase::PreAdmissionValidation) => true,
                _ => false,
            };
            if !due {
                return;
            }
            let Some((delete, re_add)) = writes.take() else {
                return;
            };
            let workspace = Arc::clone(&hook_workspace);
            std::thread::spawn(move || {
                workspace.apply_changes(delete);
                workspace.apply_changes(re_add);
            })
            .join()
            .expect("the delete and the re-add apply");
        },
        || workspace.resolve_import_outcome(importer, specifier, context),
    );
    let restarts = crate::resolver::take_outer_restarts_for_test();
    (outcome, restarts, workspace)
}

/// A module deleted and re-added with different content after the attempt
/// read it: the attempt read a file that no longer exists as it was, so it
/// is refused at admission and restarts; what it admits validates against
/// the world after the re-add.
fn assert_module_re_add_restarts(point: Point) {
    let (outcome, restarts, workspace) = resolve_across_delete_and_re_add(
        &[
            ("/p/main.ts", "import { a } from './mod'\n"),
            ("/p/mod.ts", "export const a = 1\n"),
        ],
        "/p/main.ts",
        "./mod",
        CONTEXT,
        vec![WorkspaceChange::FileDeleted {
            canonical_id: "/p/mod.ts".to_string(),
        }],
        vec![WorkspaceChange::FileChanged {
            canonical_id: "/p/mod.ts".to_string(),
            source: Some(Arc::from("export const a = 'changed'\n")),
        }],
        point,
    );
    assert_eq!(target(&outcome), Some("/p/mod.ts".to_string()), "{point:?}");
    assert!(
        restarts >= 1,
        "the attempt that read the old file is never admitted, {point:?}"
    );
    let current = WorkspaceRead::capture_resolution_world(workspace.as_ref())
        .expect("a settled world is capturable");
    match outcome.into_publication() {
        ResolutionPublication::Admitted(admitted) => assert!(
            admitted.signature().validates(current.as_ref()),
            "the admitted witness answers for the world after the re-add, {point:?}"
        ),
        ResolutionPublication::Refused(refusal) => {
            panic!("the restart is admitted, {point:?}: {:?}", refusal.reason())
        }
    }
}

#[test]
fn a_module_deleted_and_re_added_before_admission_restarts_into_the_new_file() {
    assert_module_re_add_restarts(Point::BeforeAdmission);
}

#[test]
fn a_module_deleted_and_re_added_between_input_rounds_restarts_into_the_new_file() {
    assert_module_re_add_restarts(Point::BetweenInputRounds);
}

/// A package manifest deleted and re-added pointing its entry elsewhere:
/// the answer is read FROM the file's content, so an attempt admitted on
/// the old content would answer the old target. It never does.
fn assert_manifest_re_add_answers_the_new_target(point: Point) {
    let (outcome, _restarts, _workspace) = resolve_across_delete_and_re_add(
        &[
            ("/p/src/main.ts", "import { a } from 'pkg'\n"),
            (
                "/p/node_modules/pkg/package.json",
                r#"{"module":"dist/old.js"}"#,
            ),
            ("/p/node_modules/pkg/dist/old.js", "export const a = 1;"),
            ("/p/node_modules/pkg/dist/new.js", "export const a = 2;"),
        ],
        "/p/src/main.ts",
        "pkg",
        ESM,
        vec![WorkspaceChange::FileDeleted {
            canonical_id: "/p/node_modules/pkg/package.json".to_string(),
        }],
        vec![WorkspaceChange::FileChanged {
            canonical_id: "/p/node_modules/pkg/package.json".to_string(),
            source: Some(Arc::from(r#"{"module":"dist/new.js"}"#)),
        }],
        point,
    );
    assert!(
        outcome.is_cacheable(),
        "{point:?}: {:?}",
        outcome.non_admission_reason()
    );
    assert_eq!(
        target(&outcome),
        Some("/p/node_modules/pkg/dist/new.js".to_string()),
        "the answer is read from the re-added manifest, never the deleted one, {point:?}"
    );
}

#[test]
fn a_manifest_deleted_and_re_added_before_admission_answers_the_new_target() {
    assert_manifest_re_add_answers_the_new_target(Point::BeforeAdmission);
}

#[test]
fn a_manifest_deleted_and_re_added_between_input_rounds_answers_the_new_target() {
    assert_manifest_re_add_answers_the_new_target(Point::BetweenInputRounds);
}
