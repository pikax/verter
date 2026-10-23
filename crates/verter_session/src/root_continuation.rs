//! Pollable host roots.
//!
//! A host root is one public request — component metadata, a compile — as
//! an owned computation. [`VerterHost::begin_component_meta_output`] returns
//! it without running it; each `poll` advances it as far as it can go and
//! answers `Ready`, or returns `Pending` once it waits on a dependency that
//! will wake the poll's waker. The root owns its whole state between polls,
//! so it outlives the call that began it, and any thread may poll it next.
//! The synchronous public entry over each root drives that same root to its
//! answer: there is one implementation of each request.
//!
//! # Ambient request state
//!
//! A root runs under the request context and the scheduler job its creator
//! ran under, captured when it begins, and as one execution task of the
//! host's task registry: the task its creator runs as, or a task of its own
//! when its creator runs as none. The task owns the producers the root
//! claims and is the waiter of every flight it joins, wherever it is
//! polled, so its same-path recursion and its wait cycles are decided by
//! task, never by thread. Each poll installs the request context, the job
//! and the task on the polling thread for the poll's extent and restores
//! the thread's own after it; a root begun outside any request clears the
//! polling thread's request slots instead, so a poll never runs under the
//! poller's request. Dropping a pending root releases its suspended state
//! under the same ambient state.
//!
//! # Polling contract
//!
//! - One poller at a time: `poll` takes the root exclusively. The root is
//!   `Send`, so a pending root may resume on another thread.
//! - A wake only schedules a later poll; it never runs the root.
//! - Polling a root after it answered is a contract violation and panics.
//! - Dropping a pending root abandons it. Whatever it waited on withdraws
//!   its registration; work it already published stays published.
//!
//! Not every dependency a root reaches suspends yet. The waits it still
//! enters synchronously are inventoried in `docs/arch/root-continuations.md`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use verter_execution::cancellation::{CancellationToken, JobCancellationGuard};
use verter_execution::request_context::{
    current_context, AllSlotsClearGuard, OpaqueRequestContext, TlsUninstall,
};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry};

use crate::VerterHost;

/// The request state a root runs under, captured where it begins.
struct RootAmbient {
    request: Option<OpaqueRequestContext>,
    job_cancellation: Option<CancellationToken>,
    registry: TaskRegistry,
    task: ExecutionTask,
}

/// The request slots a poll changed, restored when it ends.
enum RequestSlots {
    /// The polling thread already runs under the root's request.
    Kept,
    Installed(#[allow(dead_code)] Box<dyn TlsUninstall + Send>),
    Cleared(#[allow(dead_code)] AllSlotsClearGuard),
}

/// What one poll installed, restored in reverse order when it drops.
struct InstalledAmbient {
    _task: ExecutionScope,
    _job: JobCancellationGuard,
    _request: RequestSlots,
}

impl RootAmbient {
    fn capture(registry: &TaskRegistry) -> Self {
        Self {
            request: current_context(),
            job_cancellation: verter_execution::cancellation::current_job_cancellation_token(),
            registry: registry.clone(),
            task: ExecutionScope::current(registry).unwrap_or_else(|| registry.register_task()),
        }
    }

    fn install(&self) -> InstalledAmbient {
        let current = current_context();
        let request = match (&self.request, current) {
            (Some(bound), Some(current)) if Arc::ptr_eq(&bound.0, &current.0) => RequestSlots::Kept,
            (Some(bound), _) => RequestSlots::Installed(Arc::clone(&bound.0).install_tls()),
            (None, None) => RequestSlots::Kept,
            (None, Some(_)) => RequestSlots::Cleared(AllSlotsClearGuard::clear_all()),
        };
        InstalledAmbient {
            _request: request,
            _job: JobCancellationGuard::replace(self.job_cancellation.clone()),
            _task: ExecutionScope::install(&self.registry, &self.task),
        }
    }
}

/// Dependency work a pending root hands its driver: run it, then poll the
/// root again.
///
/// A root never runs a dependency's producer inside its own poll, and it
/// never drives the scheduler that dispatches its dependencies. When it
/// needs either, its poll returns `Pending` and the work waits in the root
/// ([`ComponentMetaOutputRoot::take_work`], [`CompileRequestRoot::take_work`])
/// for its driver. The driver runs it on whichever thread it chooses —
/// [`Self::is_producer`] tells CPU work apart from a dispatch step — and
/// polls the root again; nothing else wakes a root that handed out work.
/// Dropping the work instead abandons the dependency it carried, and the
/// root's next poll answers that abandonment.
#[must_use = "a root that handed out work waits for it to run"]
pub struct RootWork<'h>(RootWorkKind<'h>);

enum RootWorkKind<'h> {
    /// A dependency's sole producer, transferred out of the root's poll.
    Produce(Box<dyn FnOnce() + Send + 'h>),
    /// One dispatch step of the scheduler a dependency the root admitted
    /// waits in; answers whether it dispatched anything.
    Dispatch(Box<dyn FnOnce() -> bool + Send + 'h>),
}

impl<'h> RootWork<'h> {
    pub(crate) fn produce(producer: impl FnOnce() + Send + 'h) -> Self {
        Self(RootWorkKind::Produce(Box::new(producer)))
    }

    pub(crate) fn dispatch(step: impl FnOnce() -> bool + Send + 'h) -> Self {
        Self(RootWorkKind::Dispatch(Box::new(step)))
    }

    /// Whether this is a dependency's producer — CPU work a driver places
    /// on a worker — rather than a scheduler dispatch step.
    #[must_use]
    pub fn is_producer(&self) -> bool {
        matches!(self.0, RootWorkKind::Produce(_))
    }

    /// Run the work on this thread. Answers whether it made progress: a
    /// producer always does; a dispatch step only when it dispatched
    /// something, and otherwise the dependency's own settlement wakes the
    /// root.
    pub fn run(self) -> bool {
        match self.0 {
            RootWorkKind::Produce(producer) => {
                producer();
                true
            }
            RootWorkKind::Dispatch(step) => step(),
        }
    }
}

/// Where a root's computation leaves the work it hands its driver.
#[derive(Clone)]
pub(crate) struct RootWorkSlot<'h>(Arc<parking_lot::Mutex<Option<RootWork<'h>>>>);

impl<'h> RootWorkSlot<'h> {
    fn new() -> Self {
        Self(Arc::new(parking_lot::Mutex::new(None)))
    }

    /// Hand `work` to the driver; the computation then returns `Pending`.
    pub(crate) fn offer(&self, work: RootWork<'h>) {
        let previous = self.0.lock().replace(work);
        verter_debug_assert!(
            previous.is_none(),
            "a root hands out one piece of work per poll"
        );
    }

    fn take(&self) -> Option<RootWork<'h>> {
        self.0.lock().take()
    }
}

/// One root computation and the ambient state it runs under.
pub(crate) struct RootTask<'h, T> {
    computation: Option<Pin<Box<dyn Future<Output = T> + Send + 'h>>>,
    ambient: RootAmbient,
    work: RootWorkSlot<'h>,
}

impl<'h, T> RootTask<'h, T> {
    /// Own `computation` as a root that runs under the ambient request
    /// state current here, as a task of `registry`.
    pub(crate) fn new(
        registry: &TaskRegistry,
        computation: impl Future<Output = T> + Send + 'h,
    ) -> Self {
        Self::with_work(registry, |_| computation)
    }

    /// [`Self::new`] for a computation that hands its driver dependency
    /// work through the slot `computation` is built with.
    pub(crate) fn with_work<F>(
        registry: &TaskRegistry,
        computation: impl FnOnce(RootWorkSlot<'h>) -> F,
    ) -> Self
    where
        F: Future<Output = T> + Send + 'h,
    {
        let work = RootWorkSlot::new();
        Self {
            computation: Some(Box::pin(computation(work.clone()))),
            ambient: RootAmbient::capture(registry),
            work,
        }
    }

    /// The dependency work the root's last pending poll handed out.
    pub(crate) fn take_work(&mut self) -> Option<RootWork<'h>> {
        self.work.take()
    }

    /// Advance the root: `Ready` with its answer, or `Pending` once it waits
    /// on a dependency that will wake `waker`.
    ///
    /// # Panics
    ///
    /// When the root already answered.
    pub(crate) fn poll(&mut self, waker: &Waker) -> Poll<T> {
        let computation = self
            .computation
            .as_mut()
            .expect("a host root is not polled after it answered");
        let answer = {
            let _ambient = self.ambient.install();
            computation.as_mut().poll(&mut Context::from_waker(waker))
        };
        if answer.is_ready() {
            self.computation = None;
        }
        answer
    }

    /// Drive the root to its answer on this thread: the synchronous public
    /// entry's external boundary. Dependency producers the root hands out
    /// run where `scheduler` runs producers (its CPU pool), and dispatch
    /// steps run here.
    pub(crate) fn drive(mut self, scheduler: &verter_scheduler::scheduler::Scheduler) -> T {
        verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::Root,
            None,
            |waker| loop {
                if let Poll::Ready(answer) = self.poll(waker) {
                    return Poll::Ready(answer);
                }
                let Some(work) = self.take_work() else {
                    return Poll::Pending;
                };
                let progressed = if work.is_producer() {
                    scheduler.run_transferred_producer(move || {
                        work.run();
                    });
                    true
                } else {
                    work.run()
                };
                if !progressed {
                    return Poll::Pending;
                }
            },
        )
    }
}

impl<T> Drop for RootTask<'_, T> {
    fn drop(&mut self) {
        if let Some(computation) = self.computation.take() {
            let _ambient = self.ambient.install();
            drop(computation);
        }
    }
}

/// What a component-metadata output root answers: the same result
/// [`VerterHost::get_component_meta_output`] returns.
pub type ComponentMetaOutputAnswer = Result<
    Option<crate::meta_resolve::ComponentMetaOutput>,
    crate::meta_resolve::ComponentMetaFailure,
>;

/// The component-metadata output request for one component, as a pollable
/// root ([`VerterHost::begin_component_meta_output`]).
///
/// The root borrows only the host. It owns everything else it needs
/// between polls: the requested component, the session view, the
/// store-view capture its whole computation reads, and the suspended state
/// of the wait it is in.
///
/// It suspends on a store-view build another thread holds: its poll
/// returns `Pending` with the waker registered for that build's release.
/// Its resolution, extraction and materialization still enter their
/// dependency waits synchronously inside the poll; see
/// `docs/arch/root-continuations.md`.
#[must_use = "a root does nothing until it is polled"]
pub struct ComponentMetaOutputRoot<'h> {
    task: RootTask<'h, ComponentMetaOutputAnswer>,
}

impl<'h> ComponentMetaOutputRoot<'h> {
    /// Advance the request: `Ready` with its answer, or `Pending` once it
    /// waits on a dependency that will wake `waker`, or on work it handed
    /// out ([`Self::take_work`]). A wake only schedules a later poll.
    ///
    /// # Panics
    ///
    /// When the root already answered.
    pub fn poll(&mut self, waker: &Waker) -> Poll<ComponentMetaOutputAnswer> {
        self.task.poll(waker)
    }

    /// The dependency work the last pending poll handed the driver, if any:
    /// run it, then poll again.
    pub fn take_work(&mut self) -> Option<RootWork<'h>> {
        self.task.take_work()
    }

    /// Drive the request to its answer on this thread.
    pub(crate) fn drive(
        self,
        scheduler: &verter_scheduler::scheduler::Scheduler,
    ) -> ComponentMetaOutputAnswer {
        self.task.drive(scheduler)
    }
}

/// What a compile root answers: the same result
/// [`VerterHost::compile_request`] returns.
pub type CompileRequestAnswer =
    Result<crate::types::CompileRequestResponse, crate::types::CompileRequestFailure>;

/// One caller-supplied canonical compile request, as a pollable root
/// ([`VerterHost::begin_compile_request`]).
///
/// The root borrows only the host and owns the request and every input it
/// captures. It suspends on the store-view build its macro bundle reads and
/// on the scoped cache node that produces the bundle: while another caller
/// produces the node, while the node waits for dispatch (handing its driver
/// a dispatch step), and while its own producer runs — the producer is
/// handed to the driver ([`Self::take_work`]), never run in the root's
/// poll. The macro traversal inside that producer still enters its own
/// dependency waits synchronously; see `docs/arch/root-continuations.md`.
#[must_use = "a root does nothing until it is polled"]
pub struct CompileRequestRoot<'h> {
    task: RootTask<'h, CompileRequestAnswer>,
}

impl<'h> CompileRequestRoot<'h> {
    /// Advance the request: `Ready` with its answer, or `Pending` once it
    /// waits on a dependency that will wake `waker`, or on work it handed
    /// out ([`Self::take_work`]). A wake only schedules a later poll.
    ///
    /// # Panics
    ///
    /// When the root already answered.
    pub fn poll(&mut self, waker: &Waker) -> Poll<CompileRequestAnswer> {
        self.task.poll(waker)
    }

    /// The dependency work the last pending poll handed the driver, if any:
    /// run it, then poll again.
    pub fn take_work(&mut self) -> Option<RootWork<'h>> {
        self.task.take_work()
    }

    /// Drive the request to its answer on this thread.
    pub(crate) fn drive(
        self,
        scheduler: &verter_scheduler::scheduler::Scheduler,
    ) -> CompileRequestAnswer {
        self.task.drive(scheduler)
    }
}

impl VerterHost {
    /// Begin the component-metadata output request for `canonical_or_alias`
    /// as a pollable root, without running any of it.
    ///
    /// The root answers what [`Self::get_component_meta_output`] answers,
    /// under the request context and scheduler job current here, whichever
    /// thread polls it.
    pub fn begin_component_meta_output(
        &self,
        canonical_or_alias: &str,
    ) -> ComponentMetaOutputRoot<'_> {
        let canonical_or_alias = canonical_or_alias.to_owned();
        ComponentMetaOutputRoot {
            task: RootTask::new(&self.task_registry, async move {
                let view = crate::session_view::HostViewRef::new(self);
                let fixed = self.capture_batch_fixed_view_pollable(&view).await;
                self.get_component_meta_output_via_view_with_fixed_store_view(
                    &canonical_or_alias,
                    &view,
                    &fixed,
                    false,
                )
            }),
        }
    }

    /// Begin the caller-supplied canonical compile request for
    /// `canonical_id` as a pollable root, without running any of it.
    ///
    /// The root answers what [`Self::compile_request`] answers, under the
    /// request context and scheduler job current here, whichever thread
    /// polls it.
    pub fn begin_compile_request(
        &self,
        canonical_id: &str,
        request: verter_compiler::compile_request::CompileRequest,
    ) -> CompileRequestRoot<'_> {
        let canonical_id = canonical_id.to_owned();
        CompileRequestRoot {
            task: RootTask::with_work(&self.task_registry, move |work| async move {
                self.compile_request_pollable(&canonical_id, request, &work)
                    .await
            }),
        }
    }
}

#[cfg(test)]
static_assertions::assert_impl_all!(
    verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        'static,
        crate::resolver_core::HostCapabilities,
    >: Sync
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A request context whose installation is observable through the
    /// scheduler's request-id slot.
    struct InstallingRequest(u64);

    impl verter_execution::request_context::RequestContextLike for InstallingRequest {
        fn request_id(&self) -> u64 {
            self.0
        }

        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            struct Installed(
                #[allow(dead_code)] verter_execution::request_context::OpaqueContextGuard,
            );
            impl TlsUninstall for Installed {
                fn uninstall(self: Box<Self>) {}
            }
            Box::new(Installed(
                verter_execution::request_context::OpaqueContextGuard::install(
                    OpaqueRequestContext(self),
                ),
            ))
        }
    }

    /// Suspends once, then answers.
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if std::mem::replace(&mut self.0, true) {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    /// The task and request each poll of a root ran under.
    type Observed = (
        Option<verter_execution::tasks::TaskId>,
        Option<u64>,
        Option<verter_execution::tasks::TaskId>,
        Option<u64>,
    );

    fn observing_root(registry: &TaskRegistry) -> RootTask<'static, Observed> {
        let registry_in = registry.clone();
        RootTask::new(registry, async move {
            let first = (
                ExecutionScope::current(&registry_in).map(|task| task.id()),
                verter_execution::request_context::current_request_id(),
            );
            YieldOnce(false).await;
            let second = (
                ExecutionScope::current(&registry_in).map(|task| task.id()),
                verter_execution::request_context::current_request_id(),
            );
            (first.0, first.1, second.0, second.1)
        })
    }

    /// A root resumed on another thread runs as the task and under the
    /// request it began under, and leaves neither installed on the thread
    /// that polled it before.
    #[test]
    fn a_resumed_root_runs_as_its_own_task_under_its_own_request() {
        let registry = TaskRegistry::default();
        let request = OpaqueRequestContext(Arc::new(InstallingRequest(41)));
        let (mut root, root_task) = {
            let _request =
                verter_execution::request_context::OpaqueContextGuard::install(request.clone());
            let root = observing_root(&registry);
            let task = root.ambient.task.id();
            (root, task)
        };
        let waker = Waker::noop().clone();
        assert!(root.poll(&waker).is_pending());
        assert!(ExecutionScope::current(&registry).is_none());
        assert_eq!(
            verter_execution::request_context::current_request_id(),
            None
        );
        let observed = std::thread::scope(|scope| {
            scope
                .spawn(move || match root.poll(&waker) {
                    Poll::Ready(observed) => observed,
                    Poll::Pending => panic!("the root answers on its second poll"),
                })
                .join()
                .expect("the root resumes on another thread")
        });
        assert_eq!(
            observed,
            (Some(root_task), Some(41), Some(root_task), Some(41))
        );
        assert_eq!(
            verter_execution::request_context::current_request_id(),
            None
        );
        assert!(ExecutionScope::current(&registry).is_none());
        drop(request);
    }

    /// Roots begun outside any task are tasks of their own; a root begun
    /// inside a task runs as that task, so a synchronous caller's nested
    /// roots keep its same-path recursion.
    #[test]
    fn a_root_adopts_the_task_it_is_begun_in() {
        let registry = TaskRegistry::default();
        let first = observing_root(&registry).ambient.task.id();
        let second = observing_root(&registry).ambient.task.id();
        assert_ne!(first, second);
        let scope = ExecutionScope::enter(&registry);
        let nested = observing_root(&registry).ambient.task.id();
        assert_eq!(nested, scope.task().id());
    }
}
