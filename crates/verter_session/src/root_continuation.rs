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

/// One root computation and the ambient state it runs under.
pub(crate) struct RootTask<'h, T> {
    computation: Option<Pin<Box<dyn Future<Output = T> + Send + 'h>>>,
    ambient: RootAmbient,
}

impl<'h, T> RootTask<'h, T> {
    /// Own `computation` as a root that runs under the ambient request
    /// state current here, as a task of `registry`.
    pub(crate) fn new(
        registry: &TaskRegistry,
        computation: impl Future<Output = T> + Send + 'h,
    ) -> Self {
        Self {
            computation: Some(Box::pin(computation)),
            ambient: RootAmbient::capture(registry),
        }
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
    /// entry's external boundary.
    pub(crate) fn drive(mut self) -> T {
        verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::Root,
            None,
            |waker| self.poll(waker),
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

impl ComponentMetaOutputRoot<'_> {
    /// Advance the request: `Ready` with its answer, or `Pending` once it
    /// waits on a dependency that will wake `waker`. A wake only schedules
    /// a later poll.
    ///
    /// # Panics
    ///
    /// When the root already answered.
    pub fn poll(&mut self, waker: &Waker) -> Poll<ComponentMetaOutputAnswer> {
        self.task.poll(waker)
    }

    /// Drive the request to its answer on this thread.
    pub(crate) fn drive(self) -> ComponentMetaOutputAnswer {
        self.task.drive()
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
    use std::task::Wake;

    struct Noop;

    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }

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
        let waker = Waker::from(Arc::new(Noop));
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
