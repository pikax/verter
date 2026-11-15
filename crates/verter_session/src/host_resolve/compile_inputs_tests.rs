//! The request route's compile inputs: blocker hydration, the captured
//! input and the dependency verdict.
//!
//! What these prove:
//! - a missing macro type dependency the host could not establish answers
//!   the typed incomplete failure and names what is missing, while one
//!   that is proved absent stays the missing-dependency refusal with the
//!   diagnostics every other route reports;
//! - supplying exactly the named gaps is enough to compile, and the
//!   answer then equals a fresh host's;
//! - a payload lookup that ended early is incomplete, never missing;
//! - a compile root whose blocker route is resolving, or whose blocker is
//!   loading, answers `Pending` without waiting and resumes to the
//!   synchronous entry's answer;
//! - an owned blocker-route resolution records the witness and owner edge
//!   the synchronous one records, over a filesystem and an in-memory
//!   workspace;
//! - the dependency verdict's walk to a missing export enters no wait
//!   inside a root's poll and decides what the in-place walk decides;
//! - a refused blocker route does not stop the other blockers loading;
//! - a source block hydration loads is in the input the compile captures;
//! - a root that followed a producer whose lookup ended early produces the
//!   bundle itself and answers what a lone request answers;
//! - a shutdown or the request's cancellation while a blocker loads ends
//!   the request with that typed stop before anything is captured, and
//!   only a load that genuinely failed is a gap.

use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
};
use verter_execution::blocking::WaitProbe;

use super::compile_blocker_hydration::BlockerHydration;
use super::compile_inputs::SourceWitnessDemand;
use super::vue_macro_dependency_diagnostics::{collect, collect_pollable};
use crate::block_content::SuppliedBlockScope;
use crate::host_manage::import_route_witness::ResolutionWitnessScope;
use crate::host_test_audit::ResolutionCurrencyObservation;
use crate::root_continuation::RootWorkSlot;
use crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand;
use crate::types::{
    CompileDependencyGap, CompileErrorPolicy, CompileProfile, CompileRequestFailure,
    CompileRequestResponse, CompiledProduct, DependencyResolution, DiagnosticsSnapshot,
    FileLanguage, HostConfig, HostDiagnostic, HostError, UpsertRequest, VirtualNodeKind,
    VirtualQuery,
};
use crate::VerterHost;

const APP: &str = "/src/App.vue";
const BARREL: &str = "/src/protocol/index.ts";
const AUDIO: &str = "/src/protocol/audio.ts";
const VIDEO: &str = "/src/protocol/video.ts";
const TYPES: &str = "/src/types.ts";

/// A component whose props come from a package barrel that re-exports
/// them from a second file.
const BARREL_APP_SOURCE: &str = r#"<script setup lang="ts">
import type { VideoProps } from '@race/protocol'
const props = defineProps<VideoProps>()
</script>
<template><video :src="props.src" /></template>
"#;
const BARREL_SOURCE: &str = "export * from './audio'\nexport * from './video'\n";
const AUDIO_SOURCE: &str = "export interface AudioProps { volume: number }\n";
const VIDEO_SOURCE: &str = "export interface VideoProps { src: string }\n";

/// A component whose props come from a sibling types file.
const TYPES_APP_SOURCE: &str = r#"<script setup lang="ts">
import type { Props } from './types'
const props = defineProps<Props>()
</script>
<template><div>{{ props.label }}</div></template>
"#;
const TYPES_SOURCE: &str = "export interface Props { label: string }\n";

/// A component whose template is an external file. Registering a
/// component loads its script imports but not its block sources, so the
/// template is a blocker that hydration loads.
const VIEW: &str = "/src/view.html";
const VIEW_SOURCE: &str = "<p class=\"from-the-view\">external</p>";
const VIEW_APP_SOURCE: &str = "<template src=\"./view.html\"></template>\n\
     <script setup lang=\"ts\">\nconst ok = true\n</script>\n";

fn strict_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

/// A host over an in-memory workspace holding `files`, none of them
/// loaded yet.
fn workspace_host(files: &[(&str, &str)]) -> VerterHost {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in files {
        workspace.inject_file((*path).to_owned(), Arc::from(*source));
    }
    VerterHost::new(HostConfig::default(), workspace)
}

fn upsert(host: &VerterHost, id: &str, source: &str) {
    let file_language = if id.ends_with(".vue") {
        FileLanguage::vue()
    } else {
        FileLanguage::script_ts()
    };
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_owned()),
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("{id} registers: {error:?}"));
}

/// Route the app's package import to the barrel, as a bundler that
/// resolved it would.
fn route_package_to_barrel(host: &VerterHost) {
    host.set_import_dependencies(
        APP,
        vec![DependencyResolution {
            specifier: "@race/protocol".to_owned(),
            resolved_canonical_id: Some(BARREL.to_owned()),
            possible_canonical_ids: Vec::new(),
        }],
    );
}

/// The app, the barrel and the barrel's first target, without the file
/// that declares the app's props.
fn barrel_host_without_video() -> VerterHost {
    let host = strict_host();
    upsert(&host, BARREL, BARREL_SOURCE);
    upsert(&host, AUDIO, AUDIO_SOURCE);
    upsert(&host, APP, BARREL_APP_SOURCE);
    route_package_to_barrel(&host);
    host
}

/// A runtime client and IDE companion demand, both with maps.
fn request() -> CompileRequest {
    CompileRequest::new(
        vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest {
                runtime_source_map: true,
                ..RuntimeProductRequest::default()
            }),
            CompileProduct::IdeCompanion(IdeProductRequest {
                want_source_map: true,
                ..IdeProductRequest::default()
            }),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Inferred,
            ssr: false,
            script_custom_element: Some(false),
            ..VueCompileRequest::default()
        }),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("the demand constructs")
}

/// Everything a response carries: codes, maps and diagnostics.
fn rendered(response: &CompileRequestResponse) -> String {
    format!("{response:?}")
}

fn runtime_code(response: &CompileRequestResponse) -> String {
    response
        .products
        .iter()
        .filter_map(|product| match product {
            CompiledProduct::Runtime { nodes, .. } => Some(
                nodes
                    .iter()
                    .map(|node| node.code.to_string())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

fn missing_dependency_diagnostics(diagnostics: &DiagnosticsSnapshot) -> Vec<HostDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "HOST_MISSING_MACRO_TYPE_DEP")
        .cloned()
        .collect()
}

/// The gaps of an incomplete answer; any other answer fails the test.
fn incomplete_gaps(
    answer: Result<CompileRequestResponse, CompileRequestFailure>,
) -> (Vec<CompileDependencyGap>, DiagnosticsSnapshot) {
    match answer {
        Err(CompileRequestFailure::DependenciesIncomplete {
            canonical_id,
            gaps,
            diagnostics,
        }) => {
            assert_eq!(canonical_id, APP);
            (gaps, diagnostics)
        }
        other => panic!("expected the incomplete-dependencies answer, got {other:?}"),
    }
}

/// A package barrel whose `export *` target is not supplied: the compile
/// does not claim the type is missing. It answers that its dependencies
/// are incomplete, names the barrel's unresolved route, and keeps the
/// missing-dependency diagnostic every route reports.
#[test]
fn a_compile_whose_reexported_type_file_is_not_supplied_answers_dependencies_incomplete_and_names_the_route(
) {
    let host = barrel_host_without_video();
    let (gaps, diagnostics) = incomplete_gaps(host.compile_request(APP, request()));
    let video_route = CompileDependencyGap::Route {
        importer: BARREL.to_owned(),
        specifier: "./video".to_owned(),
        refused: false,
    };
    assert!(
        gaps.contains(&video_route),
        "the barrel's unresolved route is named: {gaps:?}"
    );
    // The payload lookup through the barrel may itself report that it
    // ended early; nothing else is a gap here.
    assert!(
        gaps.iter().all(|gap| *gap == video_route
            || *gap
                == CompileDependencyGap::Lookup {
                    import_source: "@race/protocol".to_owned(),
                    type_name: "VideoProps".to_owned(),
                }),
        "only the barrel route and the payload lookup are gaps: {gaps:?}"
    );
    let missing = missing_dependency_diagnostics(&diagnostics);
    assert_eq!(missing.len(), 1, "{diagnostics:?}");
    assert_eq!(
        missing[0].message,
        "missing macro type dependency '@race/protocol' for type 'VideoProps' in '/src/App.vue'"
    );
}

/// A type that a fully supplied closure does not export stays the
/// missing-dependency refusal, with the diagnostics the profile lane
/// reports for the same source.
#[test]
fn a_type_absent_from_a_fully_supplied_closure_stays_a_missing_dependency_refusal() {
    let source = TYPES_APP_SOURCE.replace("{ Props }", "{ Missing as Props }");
    let host = strict_host();
    upsert(&host, TYPES, TYPES_SOURCE);
    upsert(&host, APP, &source);

    let diagnostics = match host.compile_request(APP, request()) {
        Err(CompileRequestFailure::Refused {
            canonical_id,
            diagnostics,
        }) => {
            assert_eq!(canonical_id, APP);
            diagnostics
        }
        other => panic!("expected the missing-dependency refusal, got {other:?}"),
    };
    let missing = missing_dependency_diagnostics(&diagnostics);
    assert_eq!(missing.len(), 1, "{diagnostics:?}");
    assert_eq!(
        missing[0].message,
        "missing macro type dependency './types' for type 'Props' in '/src/App.vue'"
    );
    let import = "import type { Missing as Props } from './types'";
    let start = source.find(import).expect("the import is authored") as u32;
    assert_eq!(
        (missing[0].span.start, missing[0].span.end),
        (start, start + import.len() as u32)
    );

    let profile_lane = strict_host();
    upsert(&profile_lane, TYPES, TYPES_SOURCE);
    upsert(&profile_lane, APP, &source);
    let profile_diagnostics = match profile_lane.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(APP.to_owned()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: CompileProfile::default(),
    }) {
        Err(HostError::CompileError(failure)) => failure.diagnostics,
        other => panic!("the profile lane refuses too, got {other:?}"),
    };
    assert_eq!(
        missing,
        missing_dependency_diagnostics(&profile_diagnostics)
    );
}

/// Supplying exactly what the incomplete answer names, and nothing else,
/// lets the next request compile, and it then answers what a host that
/// held every file from the start answers: codes, maps and diagnostics.
#[test]
fn supplying_exactly_the_named_gaps_then_compiling_again_equals_a_fresh_host() {
    let host = barrel_host_without_video();
    let (gaps, _) = incomplete_gaps(host.compile_request(APP, request()));
    for gap in &gaps {
        match gap {
            CompileDependencyGap::Route {
                importer,
                specifier,
                refused: false,
            } if importer == BARREL && specifier == "./video" => {
                upsert(&host, VIDEO, VIDEO_SOURCE);
            }
            CompileDependencyGap::File { canonical } if canonical == VIDEO => {
                upsert(&host, VIDEO, VIDEO_SOURCE);
            }
            // The lookup's own observation is not something to supply.
            CompileDependencyGap::Lookup { .. } => {}
            other => panic!("unexpected gap {other:?}"),
        }
    }
    let supplied = host
        .compile_request(APP, request())
        .expect("the named gaps were everything missing");

    let fresh = strict_host();
    upsert(&fresh, BARREL, BARREL_SOURCE);
    upsert(&fresh, AUDIO, AUDIO_SOURCE);
    upsert(&fresh, VIDEO, VIDEO_SOURCE);
    upsert(&fresh, APP, BARREL_APP_SOURCE);
    route_package_to_barrel(&fresh);
    let fresh = fresh
        .compile_request(APP, request())
        .expect("a host holding every file compiles");

    assert!(runtime_code(&fresh).contains("src"));
    assert_eq!(rendered(&supplied), rendered(&fresh));
}

/// Disarms the forced partial lookup when the test ends, however it ends.
struct PartialLookups<'h>(&'h VerterHost);

impl<'h> PartialLookups<'h> {
    fn arm(host: &'h VerterHost) -> Self {
        host.test_force
            .engine
            .force_result_partial_for_tests
            .store(true, Ordering::Relaxed);
        Self(host)
    }
}

impl Drop for PartialLookups<'_> {
    fn drop(&mut self) {
        self.0
            .test_force
            .engine
            .force_result_partial_for_tests
            .store(false, Ordering::Relaxed);
    }
}

/// A payload lookup that ended early has not shown its type absent: the
/// compile answers incomplete and names the lookup, even though every
/// route on the way resolves and the export exists.
#[test]
fn a_macro_payload_lookup_that_ended_early_is_incomplete_not_missing() {
    let host = strict_host();
    upsert(&host, TYPES, TYPES_SOURCE);
    upsert(&host, APP, TYPES_APP_SOURCE);

    let answer = {
        let _partial = PartialLookups::arm(&host);
        host.compile_request(APP, request())
    };
    let (gaps, diagnostics) = incomplete_gaps(answer);
    assert!(
        gaps.contains(&CompileDependencyGap::Lookup {
            import_source: "./types".to_owned(),
            type_name: "Props".to_owned(),
        }),
        "the early-ended lookup is named: {gaps:?}"
    );
    assert_eq!(missing_dependency_diagnostics(&diagnostics).len(), 1);
}

/// Releases a held source read exactly once: explicitly, or on drop when
/// an assertion unwinds first.
struct HeldRead<'h>(Option<&'h crate::host_test_force::SourceLoadGate>);

impl HeldRead<'_> {
    fn release(&mut self) {
        if let Some(gate) = self.0.take() {
            gate.release();
        }
    }
}

impl Drop for HeldRead<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// Wakes the thread that polls a root.
struct UnparkWaker(std::thread::Thread);

impl Wake for UnparkWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Drive `root` to its answer on this thread: every poll runs under a
/// forbidding probe; handed-out producers run on a thread of their own and
/// dispatch steps run here.
fn drive_root(
    mut root: crate::CompileRequestRoot<'_>,
) -> Result<CompileRequestResponse, CompileRequestFailure> {
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    loop {
        let polled = {
            let _forbid = WaitProbe::forbid();
            root.poll(&waker)
        };
        if let Poll::Ready(answer) = polled {
            return answer;
        }
        match root.take_work() {
            Some(work) if work.is_producer() => std::thread::scope(|scope| {
                scope
                    .spawn(move || work.run())
                    .join()
                    .expect("the producer runs");
            }),
            Some(work) => {
                if !work.run() {
                    std::thread::park();
                }
            }
            None => std::thread::park(),
        }
    }
}

/// A compile root whose template blocker is still loading answers
/// `Pending` without entering a wait, once its template's route has
/// resolved through the producer it handed out, having begun that one load
/// as an owned load and handed its driver a dispatch step. Once the read
/// goes on, another thread drives the root to the answer the synchronous
/// compile entry gives.
#[test]
fn a_compile_root_whose_blocker_is_loading_stays_pending_and_resumes_to_the_synchronous_answer() {
    let files = [(VIEW, VIEW_SOURCE)];
    let host = workspace_host(&files);
    upsert(&host, APP, VIEW_APP_SOURCE);
    assert!(
        host.scheduler.try_get_source(VIEW).is_none(),
        "registering the component does not load its template"
    );
    let loads = |counter: &std::sync::atomic::AtomicUsize| counter.load(Ordering::SeqCst);
    let before = loads(&host.test_force.loads_begun);

    let gate = Arc::clone(&host.test_force.source_load_gate);
    gate.hold(VIEW);
    let mut held = HeldRead(Some(&*gate));
    let mut root = host.begin_compile_request(APP, request());
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let first = {
        let _forbid = WaitProbe::forbid();
        root.poll(&waker)
    };
    assert!(
        first.is_pending(),
        "the root waits for its template's route"
    );
    let resolution = root
        .take_work()
        .expect("the root hands out its route's resolution");
    assert!(resolution.is_producer() && resolution.run());
    let second = {
        let _forbid = WaitProbe::forbid();
        root.poll(&waker)
    };
    assert!(
        second.is_pending(),
        "the root waits for its loading blocker"
    );
    assert_eq!(loads(&host.test_force.loads_begun) - before, 1);
    let work = root
        .take_work()
        .expect("the root hands out its dispatch step");
    assert!(
        !work.is_producer(),
        "a loading blocker needs a dispatch step"
    );
    drop(work);
    held.release();

    let answer = std::thread::scope(|scope| {
        scope
            .spawn(move || drive_root(root))
            .join()
            .expect("the root resumes on another thread")
    })
    .expect("the resumed root compiles");
    assert_eq!(loads(&host.test_force.loads_begun) - before, 1);
    assert_eq!(loads(&host.test_force.blocking_loads_begun), 0);

    // A block source's map label names the host's own revision of it, so
    // the full answer is compared with this host's synchronous one and
    // the generated code with a fresh host's.
    let synchronous = host
        .compile_request(APP, request())
        .expect("the synchronous route compiles");
    assert_eq!(rendered(&answer), rendered(&synchronous));
    let cold = workspace_host(&files);
    upsert(&cold, APP, VIEW_APP_SOURCE);
    let cold = cold
        .compile_request(APP, request())
        .expect("the cold host compiles");
    assert!(runtime_code(&cold).contains("from-the-view"));
    assert_eq!(runtime_code(&answer), runtime_code(&cold));
}

/// A root that follows a producer whose payload lookup ended early does not
/// take that producer's bundle: once the condition that ended the lookup
/// is over, it produces the bundle itself, once, and answers what a lone
/// request on an untouched host answers. The producing root still answers
/// the incomplete failure naming the lookup.
#[test]
fn a_follower_of_a_producer_whose_dependency_lookup_ended_early_produces_its_own_bundle() {
    use crate::root_continuations_tests::{
        drive_root_counting, hold_producer, poll_past_route_resolutions,
    };
    let types_host = || {
        let host = strict_host();
        upsert(&host, TYPES, TYPES_SOURCE);
        upsert(&host, APP, TYPES_APP_SOURCE);
        host
    };
    let fresh = rendered(
        &types_host()
            .compile_request(APP, request())
            .expect("a lone request compiles"),
    );

    let host = types_host();
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let partial = PartialLookups::arm(&host);
    let mut producing = host.begin_compile_request(APP, request());
    let producer = hold_producer(&host, &mut producing, &waker);
    let mut following = host.begin_compile_request(APP, request());
    poll_past_route_resolutions(&host, &mut following);
    assert!(
        following.take_work().is_none_or(|work| !work.is_producer()),
        "the follower waits on the held producer"
    );
    std::thread::scope(|scope| {
        assert!(scope
            .spawn(move || producer.run())
            .join()
            .expect("the producer runs"));
    });
    drop(partial);

    let (gaps, _) = incomplete_gaps(drive_root(producing));
    assert!(
        gaps.contains(&CompileDependencyGap::Lookup {
            import_source: "./types".to_owned(),
            type_name: "Props".to_owned(),
        }),
        "the producing root names its early-ended lookup: {gaps:?}"
    );
    let (answer, handed) = drive_root_counting(&host, following, WaitProbe::forbid);
    let answer = answer.unwrap_or_else(|failure| {
        panic!("the follower produces its own bundle and compiles: {failure:?}")
    });
    assert_eq!(handed.producers, 1, "the follower produces the bundle once");
    assert_eq!(rendered(&answer), fresh);
}

/// A scheduler reset while a compile root's blocker is loading ends the
/// request with the typed shutdown before its input is captured: nothing
/// is captured, the load begun is the only one, and the blocker's source
/// is never integrated, even once its read goes on.
#[test]
fn a_shutdown_during_blocker_loading_answers_a_typed_failure_and_retains_nothing() {
    let host = workspace_host(&[(VIEW, VIEW_SOURCE)]);
    upsert(&host, APP, VIEW_APP_SOURCE);
    let count = |counter: &std::sync::atomic::AtomicUsize| counter.load(Ordering::SeqCst);
    let loads = count(&host.test_force.loads_begun);
    let blocking_loads = count(&host.test_force.blocking_loads_begun);
    let captures = count(&host.test_force.compile_input_capture_count);

    let gate = Arc::clone(&host.test_force.source_load_gate);
    gate.hold(VIEW);
    let mut held = HeldRead(Some(&*gate));
    let mut root = host.begin_compile_request(APP, request());
    let (waker, wakes) = crate::root_continuations_tests::channel_waker();
    let loading = poll_to_loading_blocker(&mut root, &waker);
    assert_eq!(count(&host.test_force.loads_begun) - loads, 1);
    drop(loading);
    while wakes.try_recv().is_ok() {}

    host.scheduler().reset();
    wakes
        .recv_timeout(crate::root_continuations_tests::POLL_DEADLINE)
        .expect("the reset wakes the root");
    let answer = {
        let _forbid = WaitProbe::forbid();
        root.poll(&waker)
    };
    held.release();
    assert!(
        matches!(
            answer,
            Poll::Ready(Err(CompileRequestFailure::Host(HostError::Shutdown)))
        ),
        "the request answers the shutdown: {answer:?}"
    );
    assert_eq!(
        count(&host.test_force.compile_input_capture_count),
        captures,
        "nothing is captured"
    );
    assert_eq!(count(&host.test_force.loads_begun) - loads, 1);
    assert_eq!(count(&host.test_force.blocking_loads_begun), blocking_loads);
    assert!(
        host.scheduler.try_get_source(VIEW).is_none(),
        "the blocker's source is not integrated"
    );
}

/// Cancelling a request while its compile root's blocker is loading wakes
/// the root, which answers the cancellation without capturing its input;
/// the cancelled request leaves nothing behind that stops the next
/// request compiling.
#[test]
fn cancelling_a_request_during_blocker_loading_answers_cancelled_and_captures_nothing() {
    let host = workspace_host(&[(VIEW, VIEW_SOURCE)]);
    upsert(&host, APP, VIEW_APP_SOURCE);
    let captures = || {
        host.test_force
            .compile_input_capture_count
            .load(Ordering::SeqCst)
    };
    let before = captures();

    let gate = Arc::clone(&host.test_force.source_load_gate);
    gate.hold(VIEW);
    let mut held = HeldRead(Some(&*gate));
    let (mut root, cancel) = {
        let _request = verter_type_engine::request_context::install_test_request_for(APP);
        let cancel = verter_type_engine::request_context::current_request_context()
            .expect("the request is installed")
            .cancellation_token();
        (host.begin_compile_request(APP, request()), cancel)
    };
    let (waker, wakes) = crate::root_continuations_tests::channel_waker();
    drop(poll_to_loading_blocker(&mut root, &waker));
    while wakes.try_recv().is_ok() {}

    cancel.cancel();
    wakes
        .recv_timeout(crate::root_continuations_tests::POLL_DEADLINE)
        .expect("cancelling the request wakes the root");
    let answer = {
        let _forbid = WaitProbe::forbid();
        root.poll(&waker)
    };
    assert!(
        matches!(
            answer,
            Poll::Ready(Err(CompileRequestFailure::Host(HostError::Cancelled)))
        ),
        "the request answers its cancellation: {answer:?}"
    );
    assert_eq!(captures(), before, "nothing is captured");
    held.release();
    let next = host
        .compile_request(APP, request())
        .expect("the next request compiles");
    assert!(runtime_code(&next).contains("from-the-view"));
}

/// Only a blocker load that genuinely failed is a gap: a load stopped by
/// cancellation, supersession or shutdown, the scheduler's own shut-down
/// failure included, ends hydration with that stop.
#[test]
fn only_a_failed_blocker_load_is_a_file_gap() {
    use super::compile_blocker_hydration::blocker_load_answer;
    use crate::host_load::HostLoadError;
    use verter_scheduler::job::SchedulerError;
    use verter_type_engine::semantic_query::ExecutionAbort;
    let failed = || {
        HostLoadError::Failed(SchedulerError::FileNotFound {
            file_id: VIEW.to_owned(),
        })
    };
    for (outcome, answer) in [
        (Ok(true), Ok(true)),
        (Ok(false), Ok(false)),
        (
            Err(HostLoadError::Cancelled),
            Err(ExecutionAbort::Cancelled),
        ),
        (
            Err(HostLoadError::Superseded),
            Err(ExecutionAbort::Superseded),
        ),
        (Err(HostLoadError::Shutdown), Err(ExecutionAbort::Shutdown)),
        (
            Err(HostLoadError::Failed(SchedulerError::Shutdown)),
            Err(ExecutionAbort::Shutdown),
        ),
        (Err(failed()), Ok(false)),
        (Err(HostLoadError::Cycle), Ok(false)),
        (Err(HostLoadError::WrongHost), Ok(false)),
        (Err(HostLoadError::NoProgress), Ok(false)),
    ] {
        let shown = format!("{outcome:?}");
        assert_eq!(blocker_load_answer(outcome), answer, "{shown}");
    }
}

/// Poll `root`, whose template is its only blocker, past its template
/// route's resolution to its pending load, every poll under a forbidding
/// probe, and return the dispatch step the load hands out.
fn poll_to_loading_blocker<'h>(
    root: &mut crate::CompileRequestRoot<'h>,
    waker: &Waker,
) -> crate::RootWork<'h> {
    let first = {
        let _forbid = WaitProbe::forbid();
        root.poll(waker)
    };
    assert!(
        first.is_pending(),
        "the root waits for its template's route"
    );
    let resolution = root
        .take_work()
        .expect("the root hands out its route's resolution");
    assert!(resolution.is_producer() && resolution.run());
    let second = {
        let _forbid = WaitProbe::forbid();
        root.poll(waker)
    };
    assert!(
        second.is_pending(),
        "the root waits for its loading blocker"
    );
    let work = root
        .take_work()
        .expect("the root hands out its dispatch step");
    assert!(
        !work.is_producer(),
        "a loading blocker needs a dispatch step"
    );
    work
}

/// The owner-edge observations `host` recorded for `APP`'s `specifier`
/// since they were last taken.
fn owner_edges(host: &VerterHost, specifier: &str) -> usize {
    host.take_resolution_currency_observations()
        .iter()
        .filter(|observation| match observation {
            ResolutionCurrencyObservation::OwnerEdgeRecomputed {
                owner,
                specifier: observed,
                ..
            }
            | ResolutionCurrencyObservation::OwnerEdgeReused {
                owner,
                specifier: observed,
                ..
            } => owner == APP && observed == specifier,
            _ => false,
        })
        .count()
}

/// A compile root whose template's route is still resolving answers
/// `Pending` on its first poll without entering a wait, having handed its
/// driver the resolution's producer: the route records no owner edge before
/// that producer runs. Run on another thread, the producer resolves the
/// route, and the root, driven there, resumes to the answer this host's
/// synchronous compile entry gives.
#[test]
fn a_compile_root_whose_blocker_route_is_resolving_stays_pending_and_resumes_to_the_synchronous_answer(
) {
    let host = workspace_host(&[(VIEW, VIEW_SOURCE)]);
    upsert(&host, APP, VIEW_APP_SOURCE);
    let handed_out = |host: &VerterHost| {
        host.test_force
            .resolution_producers_handed_out
            .load(Ordering::SeqCst)
    };

    host.begin_resolution_currency_observation();
    let mut root = host.begin_compile_request(APP, request());
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let first = {
        let _forbid = WaitProbe::forbid();
        root.poll(&waker)
    };
    assert!(
        first.is_pending(),
        "the root waits for its template's route"
    );
    assert_eq!(
        owner_edges(&host, "./view.html"),
        0,
        "the route is not resolved inside the root's poll"
    );
    assert_eq!(handed_out(&host), 1);
    let resolution = root
        .take_work()
        .expect("the root hands out its route's resolution");
    assert!(
        resolution.is_producer(),
        "a resolving route needs its resolution's producer run"
    );

    let answer = std::thread::scope(|scope| {
        scope
            .spawn(move || {
                assert!(resolution.run(), "the resolution's producer runs");
                drive_root(root)
            })
            .join()
            .expect("the root resumes on another thread")
    })
    .expect("the resumed root compiles");
    assert_ne!(
        owner_edges(&host, "./view.html"),
        0,
        "the resumed root records the route's owner edge"
    );

    // A block source's map label names the host's own revision of it, so
    // the answer is compared with this host's synchronous one.
    let synchronous = host
        .compile_request(APP, request())
        .expect("the synchronous route compiles");
    assert_eq!(rendered(&answer), rendered(&synchronous));
    assert!(runtime_code(&answer).contains("from-the-view"));
}

/// What one blocker-route resolution recorded: its target, the witness it
/// recorded into the scope around it, and its owner-edge observations.
type RouteRecord = (
    verter_workspace::ResolutionPublication<String>,
    Vec<verter_session_query::facts::fact_cache::FactVersionRef>,
    Vec<ResolutionCurrencyObservation>,
);

/// Resolve `importer`'s `./types` as a macro type blocker route, through
/// the owned entry or the synchronous one, and record what it recorded.
/// The owned entry is polled under a forbidding probe, and the producer it
/// hands out runs between polls.
fn blocker_route_record(host: &VerterHost, importer: &str, owned: bool) -> RouteRecord {
    let context = verter_session_query::resolution::ResolutionContext {
        phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
        kind: verter_session_query::resolution::ResolveRequestKind::TypeImport,
    };
    host.begin_resolution_currency_observation();
    let scope = ResolutionWitnessScope::enter();
    let publication = if owned {
        let work = RootWorkSlot::new();
        let mut resolution = std::pin::pin!(
            host.resolve_for_persistent_state_pollable(importer, "./types", context, &work)
        );
        loop {
            let polled = {
                let _forbid = WaitProbe::forbid();
                resolution
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()))
            };
            if let Poll::Ready(publication) = polled {
                break publication;
            }
            let producer = work
                .take()
                .expect("a pending resolution hands out its producer");
            assert!(producer.is_producer() && producer.run());
        }
    } else {
        host.resolve_for_persistent_state(importer, "./types", context)
    };
    (
        publication.map_result(|resolution| resolution.source_id),
        scope.collected(),
        host.take_resolution_currency_observations(),
    )
}

/// A blocker route resolved through the owned handoff answers what the
/// synchronous entry answers and records the same witness and owner edge,
/// over a filesystem workspace and an in-memory one.
#[test]
fn a_pollable_blocker_route_records_the_same_witness_and_owner_edge_as_the_synchronous_one() {
    let check = |make_host: &dyn Fn() -> VerterHost, importer: &str| {
        // One host, so both resolve under its own session population.
        let host = make_host();
        let synchronous = blocker_route_record(&host, importer, false);
        let owned = blocker_route_record(&host, importer, true);
        assert_eq!(owned, synchronous);
        let (publication, witness, observations) = synchronous;
        let target = publication.into_delivered().flatten();
        assert!(
            target
                .as_deref()
                .is_some_and(|target| target.ends_with("/src/types.ts")),
            "the route resolves to the types module: {target:?}"
        );
        assert!(!witness.is_empty(), "the resolution records its witness");
        assert!(
            !observations.is_empty(),
            "the resolution records its owner edge"
        );
    };

    check(
        &|| {
            let host = workspace_host(&[(TYPES, TYPES_SOURCE)]);
            upsert(&host, APP, TYPES_APP_SOURCE);
            host
        },
        APP,
    );

    let root = verter_test_support::unique_temp_dir("pollable_blocker_route");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("the fixture directory is created");
    std::fs::write(root.join("src/types.ts"), TYPES_SOURCE).expect("the types module is written");
    std::fs::write(root.join("src/App.vue"), TYPES_APP_SOURCE).expect("the component is written");
    let canonical = |path: &str| root.join(path).to_string_lossy().replace('\\', "/");
    let app = canonical("src/App.vue");
    check(
        &|| {
            let workspace = Arc::new(verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            ));
            let host = VerterHost::new(HostConfig::default(), workspace);
            upsert(&host, &app, TYPES_APP_SOURCE);
            host
        },
        &app,
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The dependency verdict's walk to a missing export, run as a root's
/// computation, enters no wait inside its polls: the route to the types
/// module resolves through a handed-out producer, and the module, which
/// nobody has loaded, is read through an owned load the walk suspends on
/// while its read is held. The verdict is the one the in-place walk gives
/// over the same inputs: the type is proved absent.
#[test]
fn the_missing_dependency_walk_enters_no_wait_inside_a_root_poll() {
    let source = TYPES_APP_SOURCE.replace("{ Props }", "{ Missing as Props }");
    let compiled = strict_host();
    upsert(&compiled, TYPES, TYPES_SOURCE);
    upsert(&compiled, APP, &source);
    let snapshot = compiled
        .capture_compile_input(
            APP,
            SuppliedBlockScope::Unprofiled,
            BlockerHydration::Complete,
            SourceWitnessDemand::Skip,
        )
        .expect("the component is captured")
        .input;
    let output = compiled
        .produce_vue_macro_codegen(APP, VueMacroCodegenDemand::Runtime)
        .expect("the macro bundle is produced");
    assert!(
        !output.dependency_failures.is_empty(),
        "the imported type is missing"
    );

    let walking = workspace_host(&[(TYPES, TYPES_SOURCE)]);
    let gate = Arc::clone(&walking.test_force.source_load_gate);
    gate.hold(TYPES);
    let mut held = HeldRead(Some(&*gate));
    let work = RootWorkSlot::new();
    let mut walk = Box::pin(collect_pollable(&walking, &snapshot, &output, &[], &work));
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let mut held_load_pending = false;
    let verdict = loop {
        let polled = {
            let _forbid = WaitProbe::forbid();
            walk.as_mut().poll(&mut Context::from_waker(&waker))
        };
        if let Poll::Ready(verdict) = polled {
            break verdict;
        }
        match work.take() {
            Some(producer) if producer.is_producer() => assert!(producer.run()),
            Some(dispatch) => {
                if !held_load_pending {
                    held_load_pending = true;
                    assert_eq!(walking.test_force.loads_begun.load(Ordering::SeqCst), 1);
                    held.release();
                }
                if !dispatch.run() {
                    std::thread::park();
                }
            }
            None => std::thread::park(),
        }
    };
    drop(walk);
    assert!(
        held_load_pending,
        "the walk suspended on the types module's owned load"
    );
    assert_eq!(
        walking
            .test_force
            .blocking_loads_begun
            .load(Ordering::SeqCst),
        0
    );

    let in_place = workspace_host(&[(TYPES, TYPES_SOURCE)]);
    let expected = collect(&in_place, &snapshot, &output, &[]);
    assert_eq!(verdict, expected);
    assert_eq!(expected.diagnostics.len(), 1, "{expected:?}");
    assert_eq!(expected.incomplete, None, "the type is proved absent");
}

/// Clears the forced route refusal when the test ends, however it ends.
struct RefusedRoute<'h>(&'h VerterHost);

impl<'h> RefusedRoute<'h> {
    fn arm(host: &'h VerterHost, specifier: &str) -> Self {
        *host
            .test_force
            .force_type_route_refusal_for_specifier
            .lock() = Some(specifier.to_owned());
        Self(host)
    }
}

impl Drop for RefusedRoute<'_> {
    fn drop(&mut self) {
        *self
            .0
            .test_force
            .force_type_route_refusal_for_specifier
            .lock() = None;
    }
}

/// A refused blocker route is recorded as a gap, and hydration still
/// loads the other blockers.
#[test]
fn a_refused_blocker_route_does_not_stop_the_other_blockers_loading() {
    let host = workspace_host(&[
        ("/src/a.ts", "export interface A { a: string }\n"),
        (VIEW, VIEW_SOURCE),
    ]);
    upsert(
        &host,
        APP,
        r#"<template src="./view.html"></template>
<script setup lang="ts">
import type { A } from './a'
defineProps<A>()
</script>
"#,
    );
    assert!(host.scheduler.try_get_source(VIEW).is_none());

    let hydration = {
        let _refused = RefusedRoute::arm(&host, "./a");
        host.hydrate_compile_blockers(APP)
    };
    assert_eq!(
        hydration,
        BlockerHydration::Incomplete(vec![CompileDependencyGap::Route {
            importer: APP.to_owned(),
            specifier: "./a".to_owned(),
            refused: true,
        }])
    );
    assert!(
        host.scheduler.try_get_source(VIEW).is_some(),
        "the template still loads after the refused route"
    );
}

/// An external template the workspace holds but nobody has loaded is
/// loaded by hydration and compiled from the input captured after it.
#[test]
fn a_source_block_loaded_during_hydration_is_in_the_captured_input() {
    let host = workspace_host(&[(VIEW, VIEW_SOURCE)]);
    upsert(&host, APP, VIEW_APP_SOURCE);
    assert!(
        host.scheduler.try_get_source(VIEW).is_none(),
        "registering the component does not load its template"
    );

    let response = host
        .compile_request(APP, request())
        .expect("the hydrated template compiles");
    assert!(
        runtime_code(&response).contains("from-the-view"),
        "the compiled template is the loaded one: {}",
        runtime_code(&response)
    );
}

/// Every compile lane takes its owner/content capture through the one
/// capture function: the caller-supplied request route, the
/// profile-derived virtual-file lane and the runtime render lane each run
/// it exactly once per compile. A lane that kept an inline capture of its
/// own would leave the count short by one.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn every_compile_lane_captures_through_one_function() {
    let host = strict_host();
    upsert(&host, TYPES, TYPES_SOURCE);
    upsert(&host, APP, TYPES_APP_SOURCE);
    let captures = || {
        host.test_force
            .compile_input_capture_count
            .load(Ordering::Relaxed)
    };
    assert_eq!(captures(), 0, "registering a source captures no input");

    let _ = host
        .compile_request(APP, request())
        .expect("the request route compiles");
    assert_eq!(captures(), 1, "the request route captures once");

    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(APP.to_owned()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: CompileProfile::default(),
        })
        .expect("the profile lane compiles");
    assert_eq!(captures(), 2, "the profile-derived lane captures once");

    let render_profile = CompileProfile {
        target: crate::CompileTarget::BUNDLER,
        is_production: true,
        ..CompileProfile::default()
    };
    host.render_only_main(
        APP,
        &render_profile,
        verter_compiler::compile_request::RuntimeStyleProcessing::Complete,
    )
    .expect("the render lane compiles");
    assert_eq!(captures(), 3, "the runtime render lane captures once");
}

/// A source that is not registered answers `MissingSource` even when the
/// profile also names a grammar the registered parse cannot honour; a
/// registered source answers the grammar refusal.
#[test]
fn an_unregistered_source_answers_missing_source_before_a_grammar_refusal() {
    let profile = CompileProfile {
        delimiters: Some(("[[".to_owned(), "]]".to_owned())),
        ..CompileProfile::default()
    };
    let query = || VirtualQuery {
        raw_id: None,
        canonical_id: Some(APP.to_owned()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    };

    let host = strict_host();
    assert!(matches!(
        host.get_virtual_file(query()),
        Err(HostError::MissingSource { .. })
    ));

    upsert(&host, TYPES, TYPES_SOURCE);
    upsert(&host, APP, TYPES_APP_SOURCE);
    assert!(matches!(
        host.get_virtual_file(query()),
        Err(HostError::GrammarMismatch(_))
    ));
}
