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
//! ran under, captured when it begins. Each poll installs them on the
//! polling thread for the poll's extent and restores the thread's own after
//! it; a root begun outside any request clears the polling thread's request
//! slots instead, so a poll never runs under the poller's request. Dropping
//! a pending root releases its suspended state under the same ambient
//! state.
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

use crate::VerterHost;

/// The request state a root runs under, captured where it begins.
struct RootAmbient {
    request: Option<OpaqueRequestContext>,
    job_cancellation: Option<CancellationToken>,
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
    _job: JobCancellationGuard,
    _request: RequestSlots,
}

impl RootAmbient {
    fn capture() -> Self {
        Self {
            request: current_context(),
            job_cancellation: verter_execution::cancellation::current_job_cancellation_token(),
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
    /// state current here.
    pub(crate) fn new(computation: impl Future<Output = T> + Send + 'h) -> Self {
        Self {
            computation: Some(Box::pin(computation)),
            ambient: RootAmbient::capture(),
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
            task: RootTask::new(async move {
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
