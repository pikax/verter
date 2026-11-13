//! A flow return's evaluation as an owned read.
//!
//! The root function's evaluator state and region run live in their own
//! [`ActivationCell`], beside the nested activations the drive has pushed
//! and the evaluation's [`FlowTables`]. A [`FlowRead`] owns all of them and
//! borrows nothing: it is begun over its root activation, each advancement
//! lends it the driver for one stretch of the drive, and its state stays
//! with it between advancements. Its value is finished from its root
//! activation once its root region completes.
//!
//! An advancement ends at the root region's outcome or at a read the body
//! requests ([`FlowStep::Request`]): the activation that requested it keeps
//! its place, and the next advancement is delivered the read's answer
//! ([`ReadDelivery`]). Synchronous evaluation drives the read in place,
//! answering each request as it is made.
//!
//! The thread-side rails of the observations its frames hold open live in
//! its tables ([`super::rails::ReadRails`]): an extent of its evaluation
//! attaches them on top of the running thread when it begins and detaches
//! them when it ends, once per extent, never once per step. Between extents
//! a read is plain owned data that may move to another thread.

use super::nested::{ActivationCell, ActivationOwned, DriveEnd, DriveStep, NestedActivation};
use super::*;

/// The outcome of a region's evaluation: its contributions, or the failure
/// that stopped it, and whether it falls through.
pub(super) type RegionOutcome = (Result<Vec<FlowContribution>, FlowReturnFailure>, bool);

/// A read a flow body requests where its evaluation stops: the query read,
/// how its answer is adopted, and the checking site it is read under. The
/// key is boxed: a request is rare, and every evaluator step that can carry
/// one stays as small on the stack as one that cannot.
pub(in crate::project_semantic_dispatch) struct ReadWait {
    pub(super) key: Box<crate::semantic_query::SemanticQueryKey>,
    pub(super) nesting: ReadNesting,
    pub(super) checking_site: crate::semantic_query::checker_policy::TupleSplicePosition,
}

/// How a requested read's answer is adopted by the body that requested it.
pub(in crate::project_semantic_dispatch) enum ReadNesting {
    /// Read at the body's own frame, as the frame reads it itself.
    Frame,
}

/// The answer to a [`ReadWait`]: the type node it read, or `None` when the
/// read answered none.
pub(in crate::project_semantic_dispatch) struct ReadDelivery(pub(super) Option<SemanticNodeId>);

/// A reading operation's result: its value, or the read it waits on.
#[must_use = "a read that waits is requested by its caller"]
pub(in crate::project_semantic_dispatch) enum ReadStep<T> {
    /// The value, read without a request.
    Ready(T),
    /// The read the value waits on.
    Wait(ReadWait),
}

/// Where a flow read stands after an advancement.
pub(super) enum FlowStep {
    /// Its root region completed with this outcome.
    Region(RegionOutcome),
    /// Its body requests this read; the next advancement is delivered its
    /// answer.
    Request(ReadWait),
}

/// One flow return's evaluation, owned (see the module documentation).
pub(super) struct FlowRead {
    /// The evaluation's memos and product executions, lent to one
    /// advancement at a time.
    tables: FlowTables,
    /// The root function's activation.
    root: ActivationCell,
    /// The nested functions under evaluation above the root, innermost
    /// last.
    stack: Vec<NestedActivation>,
    /// What the activation on top resumes with at the next step.
    delivered: Option<RegionDelivery>,
    /// The last advancement ended at a request, whose answer the next one
    /// is delivered.
    requested: bool,
}

// A read is exclusively owned: one evaluation steps it, and it is never
// duplicated. It borrows nothing, so it can be kept between steps, and
// holds nothing bound to a thread, so it can resume on another one.
static_assertions::assert_not_impl_any!(FlowRead: Clone, Copy);
static_assertions::assert_impl_all!(FlowRead: Send);
const _: fn() = || {
    fn owns_everything<T: 'static>() {}
    owns_everything::<FlowRead>();
};

impl FlowRead {
    /// Begin a read of the root activation `root` over `tables`, before its
    /// first advancement.
    pub(super) fn begin(tables: FlowTables, root: ActivationCell) -> Self {
        Self {
            tables,
            root,
            stack: Vec::new(),
            delivered: None,
            requested: false,
        }
    }

    /// Advance the drive under `dispatch`, with `delivery` the answer to
    /// the request the last advancement ended at, until the root region
    /// completes or the body requests a read.
    pub(super) fn advance<'d, D: FlowDemandDriver>(
        &mut self,
        dispatch: &'d FlowCx<'d, D>,
        delivery: Option<ReadDelivery>,
    ) -> FlowStep {
        verter_debug_assert!(
            delivery.is_some() == self.requested,
            "a read is delivered the answer to its last request, and only then"
        );
        self.requested = false;
        if let Some(delivery) = delivery {
            self.delivered = Some(RegionDelivery::Wait(BodyDelivery::Read(delivery)));
        }
        let Self {
            tables,
            root,
            stack,
            delivered,
            requested,
        } = self;
        root.with_dependent_mut(|_, active| {
            let mut evaluator = FlowEvaluator::over(dispatch, &mut active.evaluator, tables);
            loop {
                match evaluator.drive_step(Some(&mut active.run), stack, delivered) {
                    DriveStep::Continue => {}
                    DriveStep::Request(wait) => {
                        *requested = true;
                        return FlowStep::Request(wait);
                    }
                    DriveStep::End(DriveEnd::Region(outcome)) => return FlowStep::Region(outcome),
                    DriveStep::End(DriveEnd::Nested(_)) => {
                        unreachable!("a root drive ends with its region")
                    }
                }
            }
        })
    }

    /// Advance the read until its root region completes, in one extent,
    /// answering each read its body requests in place: its rails are
    /// attached to the running thread for the extent and detached at its
    /// end. The root run's frames are released then, before the value is
    /// finished.
    pub(super) fn drive_in_place<'d, D: FlowDemandDriver>(
        &mut self,
        dispatch: &'d FlowCx<'d, D>,
    ) -> RegionOutcome {
        let extent = self.tables.rails.attach();
        let mut delivery: Option<ReadDelivery> = None;
        let outcome = loop {
            match self.advance(dispatch, delivery.take()) {
                FlowStep::Region(outcome) => break outcome,
                FlowStep::Request(wait) => {
                    #[cfg(any(test, feature = "test-support"))]
                    super::FLOW_READ_REQUESTS.with(|requests| requests.set(requests.get() + 1));
                    delivery = Some(dispatch.answer_read(wait));
                }
            }
        };
        self.tables.rails.detach(extent);
        verter_debug_assert!(
            self.stack.is_empty() && self.tables.rails.is_empty(),
            "a completed root region leaves no nested activation and holds no read open"
        );
        self.root
            .with_dependent_mut(|_, active| drop(std::mem::take(&mut active.run.frames)));
        outcome
    }

    /// Finish the read: run `finish` on the root activation's owned inputs
    /// and evaluator state, with the evaluation's tables.
    pub(super) fn finish<R>(
        &mut self,
        finish: impl for<'q> FnOnce(
            &'q ActivationOwned,
            &mut FlowEvaluatorState<'q>,
            &mut FlowTables,
        ) -> R,
    ) -> R {
        let tables = &mut self.tables;
        self.root
            .with_dependent_mut(|owned, active| finish(owned, &mut active.evaluator, &mut *tables))
    }

    /// The evaluation's tables, once its activations are done with them.
    pub(super) fn tables(&self) -> &FlowTables {
        &self.tables
    }
}
