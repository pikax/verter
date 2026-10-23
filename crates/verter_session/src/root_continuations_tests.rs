//! Root continuations: the pollable host roots, which synchronous
//! dependency waits they still reach, and the harness that decides whether
//! a poll returns `Pending` before a held dependency is released.
//!
//! The inventory polls the real public roots under a recording
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
//! harness releases the hold and joins its thread before reporting. Its
//! cases over a held semantic flight live with the semantic task tests.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use std::time::Duration;

use verter_execution::blocking::{WaitFamily, WaitProbe};

use crate::host_compile::{
    CompileBatchInput, CompileBatchOptions, CompileBatchOutcome, CompileManyTarget,
};
use crate::types::{CompiledProduct, HostConfig, UpsertRequest};
use crate::{ComponentMetaOutputAnswer, ComponentMetaOutputRoot, VerterHost};

static_assertions::assert_impl_all!(ComponentMetaOutputRoot<'static>: Send);

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
pub(crate) const POLL_DEADLINE: Duration = Duration::from_secs(5);

/// The injected deadline of the test that proves a blocked poll fails.
pub(crate) const PLANTED_DEADLINE: Duration = Duration::from_millis(50);

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

/// Wakes the thread that polls a root.
struct UnparkWaker(std::thread::Thread);

impl Wake for UnparkWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll `root` to its answer on this thread, parking while it is pending.
/// The loop enters no wait of its own, so a probe around it sees only the
/// waits the root's polls enter.
fn poll_to_answer(mut root: ComponentMetaOutputRoot<'_>) -> ComponentMetaOutputAnswer {
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    loop {
        if let Poll::Ready(answer) = root.poll(&waker) {
            return answer;
        }
        std::thread::park();
    }
}

/// The owner's output through its pollable root, as text.
fn owner_output_through_root(host: &VerterHost) -> String {
    let output = poll_to_answer(host.begin_component_meta_output(OWNER))
        .expect("the owner's metadata resolves")
        .expect("the owner is a component");
    format!("{output:?}")
}

/// The labels of the owner's published props, sorted, polled through the
/// owner's root.
fn owner_props(host: &VerterHost) -> Vec<String> {
    props_of(poll_to_answer(host.begin_component_meta_output(OWNER)))
}

fn props_of(answer: ComponentMetaOutputAnswer) -> Vec<String> {
    let output = answer
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

/// The owner's runtime client and IDE companion demand.
fn owner_compile_request() -> verter_compiler::compile_request::CompileRequest {
    use verter_compiler::compile_request::{
        CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
        RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
    };
    CompileRequest::new(
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
    .expect("the demand constructs")
}

/// The owner's runtime client and IDE companion through the synchronous
/// compile entry, as text.
fn compile_owner(host: &VerterHost) -> Vec<String> {
    compiled_texts(
        host.compile_request(OWNER, owner_compile_request())
            .expect("the owner compiles"),
    )
}

/// What the work a compile root handed its driver reached, by kind.
#[derive(Default)]
struct HandedOut {
    producers: usize,
    dispatch_steps: usize,
}

/// Drive the owner's compile root on this thread: every poll runs under
/// `probe`, and the work it hands out runs outside it — a producer on a
/// thread of its own, a dispatch step here.
fn compile_owner_through_root(
    host: &VerterHost,
    probe: fn() -> WaitProbe,
) -> (Vec<String>, HandedOut) {
    let mut root = host.begin_compile_request(OWNER, owner_compile_request());
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let mut handed = HandedOut::default();
    loop {
        let polled = {
            let _probe = probe();
            root.poll(&waker)
        };
        if let Poll::Ready(answer) = polled {
            return (compiled_texts(answer.expect("the owner compiles")), handed);
        }
        match root.take_work() {
            Some(work) if work.is_producer() => {
                handed.producers += 1;
                std::thread::scope(|scope| {
                    scope
                        .spawn(move || work.run())
                        .join()
                        .expect("the producer runs")
                });
            }
            Some(work) => {
                handed.dispatch_steps += 1;
                if !work.run() {
                    std::thread::park();
                }
            }
            None => std::thread::park(),
        }
    }
}

fn compiled_texts(response: crate::types::CompileRequestResponse) -> Vec<String> {
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
    WaitFamily::Root,
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
/// dependencies, polled under a forbidding probe: its polls enter no wait.
/// Its macro bundle's scoped cache node elects it producer, and the root
/// hands that producer to its driver instead of running it, cold or warm;
/// the producer's own macro traversal runs where the driver runs it. The
/// answer is the synchronous entry's.
#[test]
fn root_continuations_compile_root_hands_its_producer_to_its_driver() {
    let host = host();
    let (cold_code, cold) = compile_owner_through_root(&host, WaitProbe::forbid);
    let (warm_code, warm) = compile_owner_through_root(&host, WaitProbe::forbid);
    assert_eq!(cold_code, warm_code);
    assert!(cold_code.iter().any(|code| code.contains("caption")));
    assert_eq!((cold.producers, warm.producers), (1, 1));
    assert_eq!(compile_owner(&host), cold_code);
}

/// A compile root whose macro bundle another root is producing suspends on
/// it: its poll answers `Pending` without entering a wait while the other
/// root's handed-out producer is held, and running that producer wakes it
/// to the same answer.
#[test]
fn root_continuations_compile_root_suspends_on_another_roots_producer() {
    let host = host();
    let expected = compile_owner(&host);
    let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
    let mut producing = host.begin_compile_request(OWNER, owner_compile_request());
    let producer = loop {
        assert!(producing.poll(&waker).is_pending());
        match producing.take_work() {
            Some(work) if work.is_producer() => break work,
            Some(dispatch) => {
                let _ = dispatch.run();
            }
            None => std::thread::park(),
        }
    };
    let following = host.begin_compile_request(OWNER, owner_compile_request());
    let answer = pending_before_release(
        POLL_DEADLINE,
        following,
        |root, waker| root.poll(waker),
        || {
            assert!(producer.run());
            loop {
                if let Poll::Ready(answer) = producing.poll(&waker) {
                    assert_eq!(
                        compiled_texts(answer.expect("the owner compiles")),
                        expected
                    );
                    break;
                }
                if let Some(work) = producing.take_work() {
                    let _ = work.run();
                }
            }
        },
    )
    .expect("the follower suspends until the producer runs");
    assert_eq!(
        compiled_texts(answer.expect("the owner compiles")),
        expected
    );
}

/// The synchronous compile entry drives the same root at its boundary: it
/// enters the root driver's wait and runs the handed-out producer on the
/// scheduler's CPU pool, cold or warm.
#[test]
fn root_continuations_compile_inventory() {
    let host = host();
    let (cold, cold_code) = reached(|| compile_owner(&host));
    let (warm, warm_code) = reached(|| compile_owner(&host));
    assert_eq!(cold_code, warm_code);
    assert!(cold_code.iter().any(|code| code.contains("caption")));
    for (path, families) in [("cold compile", &cold), ("warm compile", &warm)] {
        assert_reaches(path, families, &[WaitFamily::Root, WaitFamily::CpuPool]);
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

// ── Component-metadata root ────────────────────────────────────────────

/// A component-metadata root outlives the call that began it: the entry
/// returns the unpolled root, another thread polls it to its answer, and
/// that answer is the one the synchronous entry gives.
#[test]
fn root_continuations_component_meta_root_resumes_after_its_entry_returns() {
    fn begin(host: &VerterHost) -> ComponentMetaOutputRoot<'_> {
        host.begin_component_meta_output(OWNER)
    }
    let host = host();
    let root = begin(&host);
    let polled = std::thread::scope(|scope| {
        scope
            .spawn(move || poll_to_answer(root))
            .join()
            .expect("the root answers on the thread that polls it")
    });
    let polled = format!(
        "{:?}",
        polled
            .expect("the owner's metadata resolves")
            .expect("the owner is a component")
    );
    let synchronous = format!(
        "{:?}",
        host.get_component_meta_output(OWNER)
            .expect("the owner's metadata resolves")
            .expect("the owner is a component")
    );
    assert_eq!(polled, synchronous);
}

/// A thread holding the host's next store-view build at the test gate,
/// with the build claim taken.
fn hold_store_view_build<'s>(
    scope: &'s std::thread::Scope<'s, '_>,
    host: &'s VerterHost,
) -> std::thread::ScopedJoinHandle<'s, ()> {
    let gate = &crate::resolver_store::TEST_BUILD_GATE;
    // The cached view goes stale, so the next read builds.
    host.bump_store_view_epoch();
    gate.arm();
    let builder = scope.spawn(move || {
        gate.enroll_current_thread();
        let _ = host.resolver_store_view_read();
    });
    assert!(
        gate.wait_for_builder_parked(POLL_DEADLINE),
        "the builder holds the store-view build claim",
    );
    builder
}

/// The component-metadata root suspends on a store-view build another
/// thread holds: its first poll returns `Pending` without entering a wait,
/// the build's release wakes it, and it resumes from the wait to the same
/// answer, its remaining (warm) work entering no wait either.
#[test]
fn root_continuations_component_meta_root_suspends_behind_a_held_store_view_build() {
    let host = host();
    let cold = owner_output_through_root(&host);
    let answer = std::thread::scope(|scope| {
        let builder = hold_store_view_build(scope, &host);
        pending_before_release(
            POLL_DEADLINE,
            host.begin_component_meta_output(OWNER),
            |root, waker| root.poll(waker),
            || {
                crate::resolver_store::TEST_BUILD_GATE.release();
                builder.join().expect("the held build publishes");
            },
        )
    });
    let answer = answer.expect("the root suspends until the held build is released");
    let answer = format!(
        "{:?}",
        answer
            .expect("the owner's metadata resolves")
            .expect("the owner is a component")
    );
    assert_eq!(answer, cold);
    assert_eq!(host.store_view_manager().build_release_registrations(), 0);
}

/// The synchronous store-view capture behind the same held build enters
/// the store-view wait, which the harness refuses: the planted blocking
/// form of the boundary the root suspends on fails the same check.
#[test]
fn root_continuations_synchronous_capture_behind_a_held_build_is_refused() {
    let host = host();
    let _ = host.resolver_store_view_read();
    let verdict = std::thread::scope(|scope| {
        let builder = hold_store_view_build(scope, &host);
        pending_before_release(
            POLL_DEADLINE,
            (),
            |(), _waker| {
                let view = crate::session_view::HostViewRef::new(&host);
                Poll::Ready(host.capture_batch_fixed_view(&view).is_current())
            },
            || {
                crate::resolver_store::TEST_BUILD_GATE.release();
                builder.join().expect("the held build publishes");
            },
        )
    });
    match verdict {
        Err(NotSuspended::EnteredWait(message)) => {
            assert!(message.contains("StoreViewBuild"), "{message}");
        }
        other => panic!("the synchronous capture must be refused, got {other:?}"),
    }
    assert_eq!(host.store_view_manager().build_release_registrations(), 0);
}

/// Dropping a root suspended behind a held store-view build withdraws its
/// wait: the build's release finds nobody registered, and the dropped root
/// leaves no wait behind.
#[test]
fn root_continuations_dropping_a_suspended_component_meta_root_withdraws_its_wait() {
    let host = host();
    let _ = owner_output_through_root(&host);
    std::thread::scope(|scope| {
        let builder = hold_store_view_build(scope, &host);
        let mut root = host.begin_component_meta_output(OWNER);
        let waker = Waker::from(Arc::new(UnparkWaker(std::thread::current())));
        let first = {
            let _forbid = WaitProbe::forbid();
            root.poll(&waker)
        };
        let registered = host.store_view_manager().build_release_registrations();
        drop(root);
        let after_drop = host.store_view_manager().build_release_registrations();
        crate::resolver_store::TEST_BUILD_GATE.release();
        builder.join().expect("the held build publishes");
        assert!(first.is_pending(), "the root waits behind the held build");
        assert_eq!((registered, after_drop), (1, 0));
    });
}

// ── Pending-before-release harness ─────────────────────────────────────

/// Why a poll did not suspend on its held dependency.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotSuspended {
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
pub(crate) fn pending_before_release<S, T, P, R>(
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
