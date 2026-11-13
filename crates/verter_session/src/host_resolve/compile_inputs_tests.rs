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
//! - a compile root whose blocker is loading answers `Pending` without
//!   waiting and resumes to the synchronous entry's answer;
//! - a refused blocker route does not stop the other blockers loading;
//! - a source block hydration loads is in the input the compile captures.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
};
use verter_execution::blocking::WaitProbe;

use super::compile_blocker_hydration::BlockerHydration;
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
/// `Pending` on its first poll without entering a wait, having begun that
/// one load as an owned load and handed its driver a dispatch step. Once
/// the read goes on, another thread drives the root to the answer the
/// synchronous compile entry gives.
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
    assert!(first.is_pending(), "the root waits for its loading blocker");
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
