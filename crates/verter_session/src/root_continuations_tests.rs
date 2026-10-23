//! Root continuations: which synchronous dependency waits the public
//! component-meta and compile roots still reach, and the harness that
//! decides whether a poll returns `Pending` before a held dependency is
//! released.
//!
//! The inventory runs the real public roots under a recording
//! [`WaitProbe`] on cold, warm and mixed paths over cross-file
//! dependencies, construction and the store-view capture included. Each
//! path's reached families must stay inside the documented inventory in
//! `docs/arch/root-continuations.md`, and the families every run reaches
//! are pinned: converting a dependency family into a suspension removes it
//! from a path's reached set, and the pin moves with the documentation.
//!
//! The harness polls once on its own thread under a forbidding probe and a
//! bounded deadline, while the test holds a real dependency incomplete. A
//! poll that suspends answers `Pending`; the harness then releases the hold
//! and the poll continues to its answer. A poll that instead enters a
//! synchronous wait fails at once under the probe, and one that blocks
//! somewhere the probe cannot see fails at the deadline. Either way the
//! harness releases the hold and joins its thread before reporting.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use std::time::Duration;

use verter_execution::blocking::{WaitFamily, WaitProbe};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::fact_validation_port::FactValidation as _;
use verter_type_engine::semantic_query::{
    ProjectionMode, ProjectionReductionContext, ScopeId, SemanticQueryKey, ValueRootKey,
};
use verter_type_engine::semantic_query_memo::{Claim, Joined, ReadCapture};

use crate::host_compile::{
    CompileBatchInput, CompileBatchOptions, CompileBatchOutcome, CompileManyTarget,
};
use crate::types::{CompiledProduct, HostConfig, UpsertRequest};
use crate::VerterHost;

const PROPS: &str = "/src/props.ts";
const CHILD: &str = "/src/Child.vue";
const OWNER: &str = "/src/Owner.vue";

const PROPS_SOURCE: &str = "export interface Base { label: string; size: number }\n\
export type Props = Pick<Base, 'label'> & { tone?: 'a' | 'b' }\n";
const CHILD_SOURCE: &str = r#"<script setup lang="ts">
defineProps<{ caption: string }>()
</script>
<template><span>{{ caption }}</span></template>"#;
const OWNER_SOURCE: &str = r#"<script setup lang="ts">
import type { Props } from './props'
import Child from './Child.vue'
defineProps<Props>()
</script>
<template><Child caption="x" /></template>"#;

/// How long a poll that must suspend may take before the harness calls it
/// blocked. Only a failing run waits this long.
const POLL_DEADLINE: Duration = Duration::from_secs(5);

/// The injected deadline of the test that proves a blocked poll fails.
const PLANTED_DEADLINE: Duration = Duration::from_millis(50);

fn upsert(host: &VerterHost, id: &str, source: &str, language: crate::FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(id.to_owned()),
            input_id: id.to_owned(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("fixture upserts");
}

/// A host carrying the owner, its imported props module and its child
/// component.
fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host, PROPS, PROPS_SOURCE, crate::FileLanguage::script_ts());
    upsert(&host, CHILD, CHILD_SOURCE, crate::FileLanguage::vue());
    upsert(&host, OWNER, OWNER_SOURCE, crate::FileLanguage::vue());
    host
}

/// The families `run` reaches on this thread, with its answer.
fn reached<T>(run: impl FnOnce() -> T) -> (BTreeSet<WaitFamily>, T) {
    let probe = WaitProbe::record();
    let answer = run();
    (probe.families(), answer)
}

/// The labels of the owner's published props, sorted.
fn owner_props(host: &VerterHost) -> Vec<String> {
    let output = host
        .get_component_meta_output(OWNER)
        .expect("the owner's metadata resolves")
        .expect("the owner is a component");
    let (analysis, _resolution, _types) = output.into_parts();
    let mut names: Vec<String> = analysis
        .props
        .iter()
        .map(|prop| prop.name.clone())
        .collect();
    names.sort();
    names
}

/// The owner's runtime client and IDE companion through the single-request
/// compile root, as text.
fn compile_owner(host: &VerterHost) -> Vec<String> {
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
        RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
    };
    let request = CompileRequest::new(
        vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::IdeCompanion(IdeProductRequest::default()),
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
    .expect("the demand constructs");
    let response = host
        .compile_request(OWNER, request)
        .expect("the owner compiles");
    response
        .products
        .iter()
        .flat_map(|product| match product {
            CompiledProduct::Runtime { nodes, .. } => {
                nodes.iter().map(|node| node.code.to_string()).collect()
            }
            CompiledProduct::Ide(ide) => vec![ide.code.to_string()],
            CompiledProduct::Analysis(_) => Vec::new(),
        })
        .collect()
}

/// The owner through the batch compile root.
fn compile_owner_batch(host: &VerterHost) -> String {
    let entries = host.compile_many(
        vec![CompileBatchInput {
            canonical_id: OWNER.to_owned(),
            source: Arc::from(OWNER_SOURCE),
            requested_mode: None,
            component_id: None,
        }],
        CompileBatchOptions::default(),
        CompileManyTarget::HostBacked,
    );
    match &entries[..] {
        [entry] => match &entry.outcome {
            CompileBatchOutcome::Produced { code, .. } => code.to_string(),
            CompileBatchOutcome::Failed { errors } => {
                panic!("the owner compiles: {errors:?}")
            }
        },
        other => panic!("one entry per input: {}", other.len()),
    }
}

/// Every family a component-meta or compile root can reach, as listed in
/// `docs/arch/root-continuations.md`.
const ROOT_INVENTORY: &[WaitFamily] = &[
    WaitFamily::SemanticFlight,
    WaitFamily::SemanticDrive,
    WaitFamily::CacheFlight,
    WaitFamily::ResolverFlight,
    WaitFamily::StoreViewBuild,
    WaitFamily::CarrierPublication,
    WaitFamily::SchedulerCompletion,
    WaitFamily::SchedulerAdmission,
    WaitFamily::ScopedCacheNode,
    WaitFamily::WorkspaceResolution,
    WaitFamily::DeclLowering,
    WaitFamily::CpuPool,
];

/// The families only another request's concurrent work makes a root wait
/// on: a flight someone else produces, or a store view someone else builds.
/// A single-request run never reaches them.
const CONTENTION_ONLY: &[WaitFamily] = &[
    WaitFamily::SemanticFlight,
    WaitFamily::CacheFlight,
    WaitFamily::ResolverFlight,
    WaitFamily::StoreViewBuild,
];

/// `reached` stays inside the documented inventory, reaches every `pinned`
/// family, and reaches nothing else outside the contention-only families.
fn assert_reaches(path: &str, reached: &BTreeSet<WaitFamily>, pinned: &[WaitFamily]) {
    let inventory: BTreeSet<WaitFamily> = ROOT_INVENTORY.iter().copied().collect();
    let outside: Vec<_> = reached.difference(&inventory).collect();
    assert!(
        outside.is_empty(),
        "{path} reached waits outside the documented inventory: {outside:?}",
    );
    let pinned: BTreeSet<WaitFamily> = pinned.iter().copied().collect();
    let missing: Vec<_> = pinned.difference(reached).collect();
    assert!(
        missing.is_empty(),
        "{path} no longer reaches {missing:?} (reached {reached:?}): move the pin and the \
         inventory in docs/arch/root-continuations.md with the conversion",
    );
    let allowed: BTreeSet<WaitFamily> = pinned.iter().chain(CONTENTION_ONLY).copied().collect();
    let unpinned: Vec<_> = reached.difference(&allowed).collect();
    assert!(
        unpinned.is_empty(),
        "{path} reached {unpinned:?} beyond its pinned families {pinned:?}",
    );
}

/// The cold component-meta root over cross-file dependencies. Building the
/// host and registering its sources, the root itself, and dropping the
/// host are inventoried separately, so a blocking constructor or drop
/// cannot hide behind the root.
#[test]
fn root_continuations_cold_component_meta_inventory() {
    let (construction, host) = reached(host);
    let (root, props) = reached(|| owner_props(&host));
    let (teardown, ()) = reached(|| drop(host));
    assert_eq!(props, ["label", "tone"]);
    assert_reaches(
        "host construction and registration",
        &construction,
        &[
            WaitFamily::SchedulerCompletion,
            WaitFamily::SchedulerAdmission,
        ],
    );
    assert_reaches(
        "cold component-meta",
        &root,
        &[WaitFamily::SemanticDrive, WaitFamily::DeclLowering],
    );
    assert_reaches("host drop", &teardown, &[]);
}

/// A warm component-meta root re-captures the store view and validates its
/// cached entry. Uncontended, it waits on nothing; the store-view build it
/// can park on is contention-only.
#[test]
fn root_continuations_warm_component_meta_inventory() {
    let host = host();
    let cold = owner_props(&host);
    let (families, warm) = reached(|| owner_props(&host));
    assert_eq!(warm, cold);
    assert_reaches("warm component-meta", &families, &[]);
}

/// A mixed root: the child component's metadata is warm, and the owner is
/// cold again because its props module was edited after both resolved.
#[test]
fn root_continuations_mixed_component_meta_inventory() {
    let host = host();
    let _ = host
        .get_component_meta_output(CHILD)
        .expect("the child resolves")
        .expect("the child is a component");
    let _ = owner_props(&host);
    upsert(
        &host,
        PROPS,
        &PROPS_SOURCE.replace("'a' | 'b'", "'a' | 'c'"),
        crate::FileLanguage::script_ts(),
    );
    let (families, props) = reached(|| owner_props(&host));
    assert_eq!(props, ["label", "tone"]);
    assert_reaches(
        "mixed component-meta",
        &families,
        &[WaitFamily::SemanticDrive, WaitFamily::DeclLowering],
    );
}

/// The single-request compile root, cold then warm, over the same
/// dependencies. Its Vue macro codegen drives a scoped cache node, on the
/// scheduler's CPU pool, warm or cold.
#[test]
fn root_continuations_compile_inventory() {
    let host = host();
    let (cold, cold_code) = reached(|| compile_owner(&host));
    let (warm, warm_code) = reached(|| compile_owner(&host));
    assert_eq!(cold_code, warm_code);
    assert!(cold_code.iter().any(|code| code.contains("caption")));
    for (path, families) in [("cold compile", &cold), ("warm compile", &warm)] {
        assert_reaches(
            path,
            families,
            &[WaitFamily::ScopedCacheNode, WaitFamily::CpuPool],
        );
    }
}

/// The batch compile root parks its calling thread on the host CPU pool
/// for every batch, a single cold input included; what each input then
/// waits on happens on a pool thread, outside this thread's probe.
#[test]
fn root_continuations_batch_compile_inventory() {
    let host = host();
    let (cold, cold_code) = reached(|| compile_owner_batch(&host));
    let (warm, warm_code) = reached(|| compile_owner_batch(&host));
    assert_eq!(cold_code, warm_code);
    for (path, families) in [("cold batch compile", &cold), ("warm batch compile", &warm)] {
        assert_reaches(path, families, &[WaitFamily::CpuPool]);
    }
}

// ── Pending-before-release harness ─────────────────────────────────────

/// Why a poll did not suspend on its held dependency.
#[derive(Debug, PartialEq, Eq)]
enum NotSuspended {
    /// The poll entered a synchronous wait the probe refused.
    EnteredWait(String),
    /// The poll was still running at the deadline.
    Blocked,
    /// The poll answered before the dependency was released.
    ReadyBeforeRelease,
}

/// Wakes the harness thread by sending on a channel.
struct ChannelWaker(parking_lot::Mutex<mpsc::Sender<()>>);

impl Wake for ChannelWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.lock().send(());
    }
}

/// Releases a held dependency exactly once: explicitly, or on drop when an
/// assertion unwinds first.
struct Hold<R: FnOnce()>(Option<R>);

impl<R: FnOnce()> Hold<R> {
    fn release(&mut self) {
        if let Some(release) = self.0.take() {
            release();
        }
    }
}

impl<R: FnOnce()> Drop for Hold<R> {
    fn drop(&mut self) {
        self.release();
    }
}

/// Poll `state` while the dependency `release` frees is held. The first
/// poll runs on the harness thread under a forbidding probe and must answer
/// `Pending` within `deadline`. The hold is then released and the
/// harness re-polls once per wake, each poll again under the probe, until
/// the answer. Whatever happens, the hold is released and the thread joined
/// before this returns.
fn pending_before_release<S, T, P, R>(
    deadline: Duration,
    state: S,
    poll: P,
    release: R,
) -> Result<T, NotSuspended>
where
    S: Send,
    T: Send,
    P: FnMut(&mut S, &Waker) -> Poll<T> + Send,
    R: FnOnce(),
{
    let mut hold = Hold(Some(release));
    let (first_tx, first_rx) = mpsc::channel::<Poll<()>>();
    let (released_tx, released_rx) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let poller = scope.spawn(move || {
            let mut state = state;
            let mut poll = poll;
            let (wake_tx, wake_rx) = mpsc::channel::<()>();
            let waker = Waker::from(Arc::new(ChannelWaker(parking_lot::Mutex::new(wake_tx))));
            let first = {
                let _forbid = WaitProbe::forbid();
                poll(&mut state, &waker)
            };
            if let Poll::Ready(answer) = first {
                let _ = first_tx.send(Poll::Ready(()));
                return Some(answer);
            }
            let _ = first_tx.send(Poll::Pending);
            if released_rx.recv().is_err() {
                return None;
            }
            loop {
                wake_rx
                    .recv_timeout(POLL_DEADLINE)
                    .expect("releasing the dependency wakes the suspended poll");
                let _forbid = WaitProbe::forbid();
                if let Poll::Ready(answer) = poll(&mut state, &waker) {
                    return Some(answer);
                }
            }
        });
        let verdict = match first_rx.recv_timeout(deadline) {
            Ok(Poll::Pending) => None,
            Ok(Poll::Ready(())) => Some(NotSuspended::ReadyBeforeRelease),
            Err(mpsc::RecvTimeoutError::Timeout) => Some(NotSuspended::Blocked),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Some(NotSuspended::EnteredWait(String::new()))
            }
        };
        hold.release();
        if verdict.is_none() {
            released_tx
                .send(())
                .expect("the suspended poller awaits the release");
        }
        drop(released_tx);
        let joined = poller.join();
        match (verdict, joined) {
            (None, Ok(Some(answer))) => Ok(answer),
            (None, Ok(None)) => unreachable!("a released poller answers"),
            (Some(NotSuspended::EnteredWait(_)), Err(panic)) => {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default();
                Err(NotSuspended::EnteredWait(message))
            }
            (Some(verdict), _) => Err(verdict),
            (None, Err(panic)) => std::panic::resume_unwind(panic),
        }
    })
}

const FLIGHT_FILE: &str = "/src/flight.ts";

fn flight_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        FLIGHT_FILE,
        "const a = \"held\" as const;\nconst b = a;\n",
        crate::FileLanguage::script_ts(),
    );
    host
}

fn typeof_key(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    name: &str,
) -> SemanticQueryKey {
    dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(
                FLIGHT_FILE,
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
            ),
            name: verter_type_expr::OwnedStr::from(name),
        },
        ProjectionReductionContext::published(ProjectionMode::Navigate),
    )
}

/// How the subscriber to a held semantic flight is driven.
#[derive(Clone, Copy)]
enum FollowerDrive {
    /// The pollable subscription.
    Poll,
    /// The synchronous wait: the planted blocking wait the probe refuses.
    SynchronousWait,
    /// A park the probe cannot see: the planted blocking wait only the
    /// deadline catches.
    UnannouncedPark,
}

/// Hold another task's semantic flight and drive its subscriber as `drive`
/// says, through the harness.
fn drive_semantic_follower(drive: FollowerDrive, deadline: Duration) -> Result<bool, NotSuspended> {
    let host = flight_host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let key = typeof_key(&dispatch, "a");
    let graph = host.project_type_store().semantic_graph();
    let registry = graph.task_registry_for_tests();
    let producer_task = registry.register_task();
    let follower_task = registry.register_task();
    let mut capture = ReadCapture::default();
    let mut producer_claim = graph
        .begin_query_claim(&host, host.request_flags(), key.clone(), &mut capture)
        .unwrap_or_else(|_| panic!("the typeof is cold"));
    let Claim::Produce(lease) = graph.claim_query(
        &host,
        host.request_flags(),
        &mut producer_claim,
        &producer_task,
        &mut capture,
    ) else {
        panic!("the producer task owns the flight");
    };
    let mut follower_capture = ReadCapture::default();
    let mut follower_claim = graph
        .begin_query_claim(&host, host.request_flags(), key, &mut follower_capture)
        .unwrap_or_else(|_| panic!("the flight is held"));
    let Claim::Subscribed(subscription) = graph.claim_query(
        &host,
        host.request_flags(),
        &mut follower_claim,
        &follower_task,
        &mut follower_capture,
    ) else {
        panic!("the follower subscribes to the held flight");
    };
    let parked = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));
    let release_park = Arc::clone(&parked);
    let host_ref = &host;
    let verdict = pending_before_release(
        deadline,
        (Some(subscription), follower_claim, follower_capture),
        move |(subscription, claim, capture), waker| match drive {
            FollowerDrive::Poll => subscription
                .as_mut()
                .expect("polled until its answer")
                .poll(host_ref, host_ref.request_flags(), claim, capture, waker)
                .map(|joined| matches!(joined, Joined::Retry | Joined::Read(_))),
            FollowerDrive::SynchronousWait => {
                let joined = subscription.take().expect("waited once").wait(
                    host_ref,
                    host_ref.request_flags(),
                    claim,
                    capture,
                );
                Poll::Ready(matches!(joined, Joined::Retry | Joined::Read(_)))
            }
            FollowerDrive::UnannouncedPark => {
                let (released, changed) = &*parked;
                let mut released = released.lock();
                while !*released {
                    changed.wait(&mut released);
                }
                Poll::Ready(true)
            }
        },
        move || {
            let (released, changed) = &*release_park;
            *released.lock() = true;
            changed.notify_all();
            drop(lease);
        },
    );
    drop((producer_claim, capture, producer_task, follower_task));
    assert_eq!(
        graph.wait_graph_counts_for_tests(),
        (0, 0),
        "the harness leaves no wait edge or task open",
    );
    verdict
}

/// The pollable subscriber of a held semantic flight returns `Pending`
/// before the producer releases it, is woken by the release, and answers.
#[test]
fn root_continuations_harness_passes_a_suspending_flight_subscriber() {
    assert_eq!(
        drive_semantic_follower(FollowerDrive::Poll, POLL_DEADLINE),
        Ok(true)
    );
}

/// Planted blocking wait, announced: the synchronous form of the same
/// subscription fails the harness at once, naming its family, and the
/// harness still releases the producer and joins its thread.
#[test]
fn root_continuations_harness_fails_a_planted_synchronous_wait() {
    match drive_semantic_follower(FollowerDrive::SynchronousWait, POLL_DEADLINE) {
        Err(NotSuspended::EnteredWait(message)) => assert!(
            message.contains("SemanticFlight"),
            "the refusal names the family: {message}",
        ),
        other => panic!("a synchronous wait cannot pass for a suspension: {other:?}"),
    }
}

/// Planted blocking wait, unannounced: a park the probe cannot see fails
/// the harness at the deadline, and releasing the hold lets the parked
/// thread finish, so the harness joins it and returns.
#[test]
fn root_continuations_harness_fails_a_planted_unannounced_park_at_the_deadline() {
    let started = std::time::Instant::now();
    assert_eq!(
        drive_semantic_follower(FollowerDrive::UnannouncedPark, PLANTED_DEADLINE),
        Err(NotSuspended::Blocked),
    );
    assert!(
        started.elapsed() < POLL_DEADLINE,
        "the harness returns soon after its injected deadline",
    );
}
