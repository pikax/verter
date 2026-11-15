//! The completeness scopes of one traced compute that advances by poll.
//!
//! A compute written in the pollable form (an `async fn` its driver polls)
//! opens cold-compute completeness scopes as it runs: one around the whole
//! compute, one per item it treats, one around each local observation. Each
//! is a completeness rail on one [`ReadRails`] stack the compute owns
//! ([`ComputeRails`]), never a scope of the thread's own. Each poll attaches
//! the stack to the polling thread and detaches it when the poll ends
//! ([`crate::fact_signature_helpers::install_fact_tracer_pollable`]), so
//! between polls the compute holds no scope on any thread, and a scope open
//! at a suspension keeps what it accumulated for the next poll, on whichever
//! thread runs it.
//!
//! While a poll runs, the top rail is the thread's top completeness scope:
//! whatever folds into or reads the thread's top scope (the compute's reads,
//! [`crate::request_context::current_cold_compute_completeness`]) behaves as
//! it does inside one call. Closing a rail ([`CompletenessRail`]'s drop)
//! folds what it accumulated into the scope below, as a
//! [`crate::request_context::ColdComputeCompletenessScope`] does when it
//! drops; [`CompletenessRail::discard`] closes it without that.

use super::flow_return::{CompletenessRailId, RailExtent, ReadRails};
use crate::semantic_query::ResultCompleteness;

/// The completeness scopes of one traced compute that advances by poll
/// (see the module documentation).
#[derive(Default)]
pub struct ComputeRails {
    rails: parking_lot::Mutex<ReadRails>,
    /// Whether the rails are on the running thread: false between polls,
    /// and after a poll that unwound.
    attached: verter_execution::sync_cell::SyncBool,
}

// The rails never stop the compute that owns them from moving between
// threads with them.
static_assertions::assert_impl_all!(ComputeRails: Sync);
static_assertions::assert_impl_all!(CompletenessRail<'static>: Send);

impl ComputeRails {
    /// The rails of a compute that has not run yet: no scope open.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a completeness scope on top of the running thread, seeded
    /// `Complete`. Only a poll of the compute opens one: its rails are
    /// attached then.
    pub fn enter_completeness(&self) -> CompletenessRail<'_> {
        let id = self.rails().open_completeness();
        CompletenessRail {
            rails: self,
            id,
            bubble: true,
        }
    }

    /// Run `body` in a scope of its own and return what it observed there;
    /// the scope then folds into the one below.
    pub fn observe<R>(&self, body: impl FnOnce() -> R) -> (R, ResultCompleteness) {
        let scope = self.enter_completeness();
        let value = body();
        let local = crate::request_context::current_cold_compute_completeness();
        drop(scope);
        (value, local)
    }

    /// Put every open scope on the running thread, outermost first, for one
    /// poll.
    pub(crate) fn attach(&self) -> AttachedComputeRails<'_> {
        let extent = self.rails().attach();
        self.attached.set(true);
        AttachedComputeRails {
            extent: Some(extent),
            rails: self,
            tracers: crate::fact_tracing::tracing::tracer_depth(),
        }
    }

    /// Take every open scope back off the running thread at the end of the
    /// poll `attached` began, keeping what each accumulated. `finished`:
    /// the compute answered in that poll.
    pub(crate) fn detach(&self, mut attached: AttachedComputeRails<'_>, finished: bool) {
        let extent = attached.extent.take().expect("a poll's rails detach once");
        let mut rails = self.rails();
        rails.detach(extent);
        self.attached.set(false);
        verter_debug_assert!(
            crate::fact_tracing::tracing::tracer_depth() == attached.tracers,
            "a compute's poll leaves the tracers above its own as it found them"
        );
        if finished {
            verter_debug_assert!(
                rails.is_empty(),
                "a finished compute holds no completeness scope open"
            );
        }
    }

    fn rails(&self) -> parking_lot::MutexGuard<'_, ReadRails> {
        self.rails
            .try_lock()
            .expect("a compute's rails are borrowed by one scope at a time")
    }
}

/// One poll's attachment of a compute's rails. Dropped before
/// [`ComputeRails::detach`] takes it, an unwind, it restores the thread's
/// stacks as it found them and marks the rails off the thread, so the scopes
/// the abandoned compute drops later touch no thread.
#[must_use = "a poll's rails are detached at the end of the poll"]
pub(crate) struct AttachedComputeRails<'r> {
    extent: Option<RailExtent>,
    rails: &'r ComputeRails,
    /// The thread's tracer depth while the poll runs: the compute's own
    /// tracer is installed before its rails are attached.
    tracers: usize,
}

impl Drop for AttachedComputeRails<'_> {
    fn drop(&mut self) {
        if let Some(extent) = self.extent.take() {
            drop(extent);
            self.rails.attached.set(false);
        }
    }
}

/// One open completeness scope of a compute. Dropping it closes the scope
/// into the one below, as a
/// [`crate::request_context::ColdComputeCompletenessScope`] does;
/// [`Self::discard`] closes it without that.
#[must_use]
pub struct CompletenessRail<'r> {
    rails: &'r ComputeRails,
    id: CompletenessRailId,
    bubble: bool,
}

impl CompletenessRail<'_> {
    /// Close this scope WITHOUT folding what it accumulated into the scope
    /// below.
    pub fn discard(mut self) {
        self.bubble = false;
    }
}

impl Drop for CompletenessRail<'_> {
    fn drop(&mut self) {
        // The compute was abandoned between polls, or its poll unwound and
        // the poll's extent already restored the thread.
        if !self.rails.attached.get() {
            return;
        }
        let observed = self.rails.rails().close_completeness(self.id);
        if self.bubble {
            crate::request_context::fold_cold_compute_completeness(observed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ComputeRails;
    use crate::request_context::{
        current_cold_compute_completeness, fold_result_completeness,
        thread_observation_depths_for_tests, ColdComputeCompletenessScope,
    };
    use crate::semantic_query::{PartialReasonSet, ResultCompleteness};

    fn partial() -> ResultCompleteness {
        ResultCompleteness::Complete.or_partial_if(true, PartialReasonSet::PROPAGATED)
    }

    /// A scope open across a detach keeps what it accumulated off the
    /// thread, has it again on the next attach, and hands it to the scope
    /// below only when it closes.
    #[test]
    fn a_scope_keeps_what_it_held_across_a_detach_and_bubbles_only_on_close() {
        let lender = ColdComputeCompletenessScope::enter();
        let before = thread_observation_depths_for_tests();
        let rails = ComputeRails::new();
        let attached = rails.attach();
        let outer = rails.enter_completeness();
        let inner = rails.enter_completeness();
        fold_result_completeness(partial());
        rails.detach(attached, false);
        assert_eq!(
            thread_observation_depths_for_tests(),
            before,
            "between polls the compute holds no scope on the thread"
        );
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "a detached scope keeps its reasons to itself"
        );

        let attached = rails.attach();
        assert!(
            current_cold_compute_completeness().is_partial(),
            "the reattached scope still holds what it accumulated"
        );
        drop(inner);
        assert!(
            current_cold_compute_completeness().is_partial(),
            "closing the inner scope folds it into the outer one"
        );
        drop(outer);
        rails.detach(attached, true);
        assert_eq!(thread_observation_depths_for_tests(), before);
        assert!(
            current_cold_compute_completeness().is_partial(),
            "the outer scope's close reaches the scope below the compute"
        );
        drop(lender);
    }

    /// A discarded scope folds nothing into the scope below it.
    #[test]
    fn a_discarded_scope_reaches_nothing_below() {
        let lender = ColdComputeCompletenessScope::enter();
        let rails = ComputeRails::new();
        let attached = rails.attach();
        let outer = rails.enter_completeness();
        let inner = rails.enter_completeness();
        fold_result_completeness(partial());
        inner.discard();
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "a discarded scope's reasons stay out of the scope that held it"
        );
        drop(outer);
        rails.detach(attached, true);
        assert!(!current_cold_compute_completeness().is_partial());
        drop(lender);
    }

    /// A poll that unwinds restores the thread itself; a scope the abandoned
    /// compute drops afterwards pops nothing from the thread.
    #[test]
    fn a_scope_dropped_after_its_poll_unwound_touches_no_thread() {
        let rails = ComputeRails::new();
        let attached = rails.attach();
        let scope = rails.enter_completeness();
        fold_result_completeness(partial());
        // What an unwinding poll does: its attachment drops undetached.
        drop(attached);
        let before = thread_observation_depths_for_tests();
        let unrelated = ColdComputeCompletenessScope::enter();
        drop(scope);
        let [aborts, completeness, sticky, tracers] = before;
        assert_eq!(
            thread_observation_depths_for_tests(),
            [aborts, completeness + 1, sticky, tracers],
            "the unrelated scope is still on the thread"
        );
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "the unrelated scope is still the top and still complete"
        );
        drop(unrelated);
        assert_eq!(thread_observation_depths_for_tests(), before);
    }
}
