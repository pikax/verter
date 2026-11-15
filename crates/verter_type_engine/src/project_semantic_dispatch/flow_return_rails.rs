//! The thread-side state a flow read owns: the rails of its held read
//! observations and transaction-read recordings.
//!
//! A read's frames hold some observations open across the read's steps: a
//! deferred method's or class member's read observation, a nested
//! function's signature recording. Part of what such an observation holds
//! is state on the request, which moves with the request and stays where it
//! is between steps: the build-local taint frame, the degraded-read counter.
//! The other part is state on the thread (the execution-validity, completeness,
//! sticky-deferral, refusal and fact-recorder stacks), and the read may run
//! its next steps on another thread. So that part is owned by the read as
//! plain data, one rail per stack, kept here in one LIFO stack of the read's
//! own ([`ReadRails`]):
//!
//! - **Attach** pushes a rail ON TOP of whatever the thread holds, never
//!   replacing it: what the read's steps observe reaches the rail and every
//!   scope of the consumer below it, as it would inside one call.
//! - **Detach** takes the rail back off, from the top, by identity or depth,
//!   with what it observed, and never bubbles anything into the scopes
//!   below.
//! - A detached rail is plain `Send` data whose drop touches nothing.
//! - What a rail observed folds into its consumer once, at completion
//!   ([`ReadRails::close_observation`], [`ReadRails::close_recording`]).
//!
//! The read attaches the whole stack when an extent of its evaluation
//! begins and detaches it when the extent ends ([`ReadRails::attach`],
//! [`ReadRails::detach`]), in exact order: once per extent, never once per
//! step. A rail opened during an extent is live at once; its frame holds a
//! typed id ([`ObservationRail`], [`RecordingRail`]) and closes it through
//! the stack, which checks that it is the top. If an extent unwinds, its
//! [`RailExtent`] restores the thread's stacks as it found them, merging
//! nothing.
//!
//! A traced compute that advances by poll keeps its completeness scopes on
//! the same stack, as rails of completeness alone
//! ([`ReadRails::open_completeness`]): see
//! [`crate::project_semantic_dispatch::compute_rails::ComputeRails`].

use crate::fact_tracing::refusal_scope::HeldRefusal;
use crate::request_context::{HeldCompleteness, HeldSticky, HeldValidity, ThreadRailMark};
use crate::resolver_core::resolver_context::{FactReadRecording, RecordedFactReads};
use crate::semantic_query::{ExecutionAbort, ResultCompleteness};
use verter_session_query::facts::reuse::NonCacheableReadReason;

/// The thread-side rails of one held read observation: its validity, its
/// sticky deferral and its completeness, attached in that order.
struct ObservationRails {
    validity: HeldValidity,
    sticky: HeldSticky,
    completeness: HeldCompleteness,
}

/// The thread-side rails of one held transaction-read recording: its
/// refusal scope and its fact recorder, attached in that order.
struct RecordingRails {
    refusal: HeldRefusal,
    reads: FactReadRecording,
}

/// One entry of a read's rail stack.
enum ReadRail {
    Observation(ObservationRails),
    Recording(RecordingRails),
    /// A cold-compute completeness scope alone: no validity, no sticky
    /// deferral.
    Completeness(HeldCompleteness),
}

impl ReadRail {
    fn attach(&mut self) {
        match self {
            Self::Observation(rails) => {
                rails.validity.attach();
                rails.sticky.attach();
                rails.completeness.attach();
            }
            Self::Recording(rails) => {
                rails.refusal.attach();
                rails.reads.attach();
            }
            Self::Completeness(completeness) => completeness.attach(),
        }
    }

    fn detach(&mut self) {
        match self {
            Self::Observation(rails) => {
                rails.completeness.detach();
                rails.sticky.detach();
                rails.validity.detach();
            }
            Self::Recording(rails) => {
                rails.reads.detach();
                rails.refusal.detach();
            }
            Self::Completeness(completeness) => completeness.detach(),
        }
    }
}

/// A held read observation's place in its read's rail stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) struct ObservationRail(usize);

/// A held transaction-read recording's place in its read's rail stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) struct RecordingRail(usize);

/// A completeness rail's place in its compute's rail stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) struct CompletenessRailId(usize);

/// What a held read observation's thread-side rails observed, taken at its
/// completion: the abort its validity observed (already handed to the
/// scope below, as an abort always is) and the completeness its reads
/// folded.
pub(in crate::project_semantic_dispatch) struct ClosedObservation {
    pub(in crate::project_semantic_dispatch) abort: Option<ExecutionAbort>,
    pub(in crate::project_semantic_dispatch) completeness: ResultCompleteness,
}

/// What a held transaction-read recording's thread-side rails observed,
/// taken at its completion: the facts it recorded and the refusal it saw.
pub(in crate::project_semantic_dispatch) struct ClosedRecording {
    pub(in crate::project_semantic_dispatch) reads: RecordedFactReads,
    pub(in crate::project_semantic_dispatch) refused: Option<NonCacheableReadReason>,
}

/// A read's thread-side rails, innermost last (see the module
/// documentation). Owned by the read's tables and lent with them to each
/// advancement.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct ReadRails {
    rails: Vec<ReadRail>,
    /// Whether the rails are on the running thread now: between
    /// [`Self::attach`] and [`Self::detach`].
    attached: bool,
}

// A detached read moves between threads with its rails.
static_assertions::assert_impl_all!(ReadRails: Send);

/// One extent's attachment of a read's rails: where the thread's stacks
/// stood before. Plain data; it borrows nothing, so the read stays free to
/// advance while it is held. Dropped before [`ReadRails::detach`] takes it,
/// an unwind, it restores the thread's stacks to where they stood, merging
/// nothing.
#[must_use = "an extent's rails are detached with the extent that attached them"]
pub(in crate::project_semantic_dispatch) struct RailExtent {
    /// Taken when the extent ends with every rail detached.
    thread: Option<ThreadRailMark>,
    refusals: usize,
    recorders: usize,
    armed: bool,
}

impl Drop for RailExtent {
    fn drop(&mut self) {
        if self.armed {
            crate::fact_tracing::tracing::truncate_recorders(self.recorders);
            crate::fact_tracing::refusal_scope::truncate_refusal_scopes(self.refusals);
        }
    }
}

impl ReadRails {
    /// Whether the read holds no rail open.
    pub(in crate::project_semantic_dispatch) fn is_empty(&self) -> bool {
        self.rails.is_empty()
    }

    /// Attach every rail the read holds to the running thread, outermost
    /// first, for one extent of its evaluation.
    pub(in crate::project_semantic_dispatch) fn attach(&mut self) -> RailExtent {
        verter_debug_assert!(!self.attached, "a read's rails are attached once at a time");
        let extent = RailExtent {
            thread: Some(ThreadRailMark::take()),
            refusals: crate::fact_tracing::refusal_scope::refusal_scope_depth(),
            recorders: crate::fact_tracing::tracing::recorder_depth(),
            armed: true,
        };
        for rail in &mut self.rails {
            rail.attach();
        }
        self.attached = true;
        extent
    }

    /// Detach every rail the read holds from the running thread, innermost
    /// first, at the end of the extent `extent` began. What each observed
    /// stays with it; nothing bubbles.
    pub(in crate::project_semantic_dispatch) fn detach(&mut self, mut extent: RailExtent) {
        verter_debug_assert!(self.attached, "only attached rails detach");
        for rail in self.rails.iter_mut().rev() {
            rail.detach();
        }
        self.attached = false;
        verter_debug_assert!(
            extent.recorders == crate::fact_tracing::tracing::recorder_depth()
                && extent.refusals == crate::fact_tracing::refusal_scope::refusal_scope_depth(),
            "a read's extent detaches every rail it attached"
        );
        extent.armed = false;
        if let Some(thread) = extent.thread.take() {
            thread.release();
        }
    }

    /// Open a held read observation's rails on top of the running thread:
    /// its validity inherits the abort of the execution active now, its
    /// completeness starts `Complete`, and the request sticky is deferred.
    pub(in crate::project_semantic_dispatch) fn open_observation(&mut self) -> ObservationRail {
        let mut rail = ReadRail::Observation(ObservationRails {
            validity: HeldValidity::open(),
            sticky: HeldSticky::default(),
            completeness: HeldCompleteness::open(),
        });
        self.push(&mut rail);
        self.rails.push(rail);
        ObservationRail(self.rails.len() - 1)
    }

    /// Close the held read observation `rail` names, the top of the stack:
    /// its validity's abort goes to the scope below, as an ending validity
    /// scope's does, and its completeness is returned for the observation's
    /// consumer, merged nowhere.
    pub(in crate::project_semantic_dispatch) fn close_observation(
        &mut self,
        rail: ObservationRail,
    ) -> ClosedObservation {
        let ReadRail::Observation(rails) = self.pop(rail.0) else {
            unreachable!("an observation rail names an observation")
        };
        let ObservationRails {
            validity,
            mut sticky,
            completeness,
        } = rails;
        let completeness = completeness.close();
        sticky.detach();
        let abort = validity.close();
        ClosedObservation {
            abort,
            completeness,
        }
    }

    /// Open a held transaction-read recording's rails on top of the
    /// running thread: a fresh refusal scope, and a recorder sealing what it
    /// records under `policy`.
    pub(in crate::project_semantic_dispatch) fn open_recording(
        &mut self,
        policy: verter_session_query::facts::policy::FactPolicy,
    ) -> RecordingRail {
        let mut rail = ReadRail::Recording(RecordingRails {
            refusal: HeldRefusal::default(),
            reads: FactReadRecording::new(policy),
        });
        self.push(&mut rail);
        self.rails.push(rail);
        RecordingRail(self.rails.len() - 1)
    }

    /// Close the held transaction-read recording `rail` names, the top of
    /// the stack: what it recorded and the refusal it saw, for the
    /// recording's consumer.
    pub(in crate::project_semantic_dispatch) fn close_recording(
        &mut self,
        rail: RecordingRail,
    ) -> ClosedRecording {
        let ReadRail::Recording(rails) = self.pop(rail.0) else {
            unreachable!("a recording rail names a recording")
        };
        let RecordingRails { mut refusal, reads } = rails;
        let reads = reads.finish();
        refusal.detach();
        ClosedRecording {
            reads,
            refused: refusal.observed(),
        }
    }

    /// Open a completeness rail on top of the running thread, seeded
    /// `Complete`, as a cold-compute completeness scope is seeded.
    pub(in crate::project_semantic_dispatch) fn open_completeness(&mut self) -> CompletenessRailId {
        let mut rail = ReadRail::Completeness(HeldCompleteness::open());
        self.push(&mut rail);
        self.rails.push(rail);
        CompletenessRailId(self.rails.len() - 1)
    }

    /// Close the completeness rail `rail` names, the top of the stack: what
    /// it accumulated, merged nowhere.
    pub(in crate::project_semantic_dispatch) fn close_completeness(
        &mut self,
        rail: CompletenessRailId,
    ) -> ResultCompleteness {
        let ReadRail::Completeness(completeness) = self.pop(rail.0) else {
            unreachable!("a completeness rail names a completeness scope")
        };
        completeness.close()
    }

    /// Put a newly opened rail on the running thread: the read's rails are
    /// attached while it advances, and a rail opened then is live at once.
    fn push(&self, rail: &mut ReadRail) {
        verter_debug_assert!(
            self.attached,
            "a read opens a rail only while its rails are attached"
        );
        rail.attach();
    }

    /// Take the rail at `index` off the stack, where it is the top. It is
    /// still on the running thread: its closer detaches it.
    fn pop(&mut self, index: usize) -> ReadRail {
        verter_debug_assert!(
            self.attached && index + 1 == self.rails.len(),
            "a read's rail closes from the top of its stack"
        );
        self.rails
            .pop()
            .expect("a read's rail closes while it is open")
    }
}

#[cfg(test)]
mod tests {
    use super::ReadRails;
    use crate::fact_tracing::RefusalObservationScope;
    use crate::request_context::{
        current_cold_compute_completeness, current_execution_abort, observe_execution_abort,
        ColdComputeCompletenessScope, ExecutionValidityScope, ThreadRailMark,
    };
    use crate::semantic_query::{ExecutionAbort, PartialReasonSet, ResultCompleteness};
    use verter_session_query::facts::reuse::NonCacheableReadReason;

    fn partial() -> ResultCompleteness {
        ResultCompleteness::Complete.or_partial_if(true, PartialReasonSet::PROPAGATED)
    }

    fn thread_depths() -> (usize, usize, usize, usize, usize) {
        let (aborts, completeness, sticky) = ThreadRailMark::depths_for_tests();
        (
            aborts,
            completeness,
            sticky,
            crate::fact_tracing::refusal_scope::refusal_scope_depth(),
            crate::fact_tracing::tracing::recorder_depth(),
        )
    }

    /// A read's rails detached on one thread move to another as plain data:
    /// dropping them there leaves both threads' stacks as they were, and
    /// nothing they observed reaches either thread's scopes.
    #[test]
    fn rails_detached_on_one_thread_and_dropped_on_another_touch_neither() {
        let lender = thread_depths();
        let completeness = ColdComputeCompletenessScope::enter();
        let mut rails = ReadRails::default();
        let extent = rails.attach();
        let _observation = rails.open_observation();
        let _recording =
            rails.open_recording(verter_session_query::facts::policy::FactPolicy::production());
        crate::request_context::fold_result_completeness(partial());
        rails.detach(extent);
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "a detached rail keeps its partiality to itself"
        );
        std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    let other = thread_depths();
                    let scope = ColdComputeCompletenessScope::enter();
                    drop(rails);
                    assert!(!current_cold_compute_completeness().is_partial());
                    drop(scope);
                    assert_eq!(thread_depths(), other, "the dropping thread is untouched");
                })
                .join()
                .expect("the other thread drops the rails");
        });
        assert!(!current_cold_compute_completeness().is_partial());
        drop(completeness);
        assert_eq!(thread_depths(), lender, "the detaching thread is untouched");
    }

    /// Attaching pushes on top and detaching pops without bubbling: what a
    /// read observes while attached reaches the scopes below it on that
    /// thread at once, as it would inside one call, but the rail's own
    /// partiality and abort never bubble on detach; only its close hands
    /// the abort down.
    #[test]
    fn an_attached_rail_runs_on_top_and_detaches_without_bubbling() {
        let validity = ExecutionValidityScope::enter();
        let completeness = ColdComputeCompletenessScope::enter();
        let mut rails = ReadRails::default();
        let extent = rails.attach();
        let observation = rails.open_observation();
        crate::request_context::fold_result_completeness(partial());
        observe_execution_abort(ExecutionAbort::Cancelled);
        rails.detach(extent);
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "detaching never merges the rail's completeness below"
        );
        assert_eq!(
            current_execution_abort(),
            None,
            "detaching never bubbles an abort"
        );

        let extent = rails.attach();
        let closed = rails.close_observation(observation);
        rails.detach(extent);
        assert!(closed.completeness.is_partial());
        assert_eq!(closed.abort, Some(ExecutionAbort::Cancelled));
        assert_eq!(
            current_execution_abort(),
            Some(ExecutionAbort::Cancelled),
            "the close hands the abort to the scope below"
        );
        assert!(
            !current_cold_compute_completeness().is_partial(),
            "the close leaves the completeness to the observation's consumer"
        );
        drop(completeness);
        drop(validity);
    }

    /// A read that suspends on one thread and resumes on another reports
    /// each step's refusals and facts to the consumer scopes live where that
    /// step ran, and keeps them all in its own rails, as one call would.
    #[test]
    fn a_resumed_read_reports_each_step_to_the_scopes_where_it_ran() {
        let fact =
            |name: &str| verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                canonical_id: name.into(),
                hash: [9; 16],
            };
        let step = |rails: &mut ReadRails, name: &str, reason: NonCacheableReadReason| {
            let outer = RefusalObservationScope::enter();
            let recorder = crate::fact_tracing::tracing::FactReadRecorder::default();
            let installed = crate::fact_tracing::tracing::install_recorder(&recorder);
            let extent = rails.attach();
            crate::resolver_core::resolver_context::with_fact_tracer_cell(
                Default::default(),
                verter_session_query::facts::policy::FactPolicy::production(),
                |_| {
                    crate::fact_tracing::tracing::observe_fan_out(fact(name));
                    crate::fact_tracing::note_non_cacheable_read_fan_out(reason);
                },
            );
            rails.detach(extent);
            drop(installed);
            (outer.observed(), recorder.into_parts().0)
        };
        let mut rails = ReadRails::default();
        let extent = rails.attach();
        let recording =
            rails.open_recording(verter_session_query::facts::policy::FactPolicy::production());
        rails.detach(extent);

        let first = step(&mut rails, "/first.ts", NonCacheableReadReason::FencedServe);
        assert_eq!(
            first,
            (
                Some(NonCacheableReadReason::FencedServe),
                vec![fact("/first.ts")]
            )
        );
        let (second, mut rails) = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    let second = step(&mut rails, "/second.ts", NonCacheableReadReason::LeaseMiss);
                    (second, rails)
                })
                .join()
                .expect("the read resumes on another thread")
        });
        assert_eq!(
            second,
            (
                Some(NonCacheableReadReason::LeaseMiss),
                vec![fact("/second.ts")]
            )
        );

        let extent = rails.attach();
        let closed = rails.close_recording(recording);
        rails.detach(extent);
        assert_eq!(closed.refused, Some(NonCacheableReadReason::LeaseMiss));
        let mut recorded: Vec<_> = closed.reads.facts.iter().cloned().collect();
        recorded.sort();
        assert_eq!(recorded, vec![fact("/first.ts"), fact("/second.ts")]);
        assert!(rails.is_empty());
    }

    /// An extent that unwinds with its rails attached restores the thread's
    /// stacks as it found them, merging nothing into the scopes below.
    #[test]
    fn an_unwinding_extent_restores_the_thread_without_merging() {
        let completeness = ColdComputeCompletenessScope::enter();
        let before = thread_depths();
        let mut rails = ReadRails::default();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _extent = rails.attach();
            let _observation = rails.open_observation();
            let _recording =
                rails.open_recording(verter_session_query::facts::policy::FactPolicy::production());
            crate::request_context::fold_result_completeness(partial());
            panic!("the extent unwinds with its rails attached");
        }));
        assert!(unwound.is_err());
        assert_eq!(thread_depths(), before, "the unwind restores every stack");
        assert!(!current_cold_compute_completeness().is_partial());
        drop(rails);
        assert_eq!(thread_depths(), before);
        drop(completeness);
    }
}
