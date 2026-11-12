//! A flow return's evaluation as an owned read.
//!
//! The root function's evaluator state and region run live in their own
//! [`ActivationCell`], beside the nested activations the drive has pushed
//! and the evaluation's [`FlowTables`]. A [`FlowRead`] owns all of them and
//! borrows nothing: each step lends it the driver for one advancement of the
//! drive, and its state stays with it between steps. Synchronous
//! evaluation steps it until the root region completes, then finishes the
//! value from its root activation, in place.

use super::nested::{ActivationCell, ActivationOwned, DriveEnd, NestedActivation};
use super::*;

/// The outcome of a region's evaluation: its contributions, or the failure
/// that stopped it, and whether it falls through.
pub(super) type RegionOutcome = (Result<Vec<FlowContribution>, FlowReturnFailure>, bool);

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
}

// A read is exclusively owned: one evaluation steps it, and it is never
// duplicated. It borrows nothing, so it can be kept between steps.
static_assertions::assert_not_impl_any!(FlowRead: Clone, Copy);
const _: fn() = || {
    fn owns_everything<T: 'static>() {}
    owns_everything::<FlowRead>();
};

impl FlowRead {
    /// A read of the root activation `root` over `tables`, before its first
    /// step.
    pub(super) fn new(tables: FlowTables, root: ActivationCell) -> Self {
        Self {
            tables,
            root,
            stack: Vec::new(),
            delivered: None,
        }
    }

    /// Advance the drive by one step under `dispatch`: the outcome of the
    /// root region once it completes.
    pub(super) fn step<'d, D: FlowDemandDriver>(
        &mut self,
        dispatch: &'d FlowCx<'d, D>,
    ) -> Option<RegionOutcome> {
        let Self {
            tables,
            root,
            stack,
            delivered,
        } = self;
        root.with_dependent_mut(|_, active| {
            match FlowEvaluator::over(dispatch, &mut active.evaluator, tables).drive_step(
                Some(&mut active.run),
                stack,
                delivered,
            )? {
                DriveEnd::Region(outcome) => Some(outcome),
                DriveEnd::Nested(_) => unreachable!("a root drive ends with its region"),
            }
        })
    }

    /// Step the read until its root region completes. The root run's frames
    /// are released then, before the value is finished.
    pub(super) fn evaluate<'d, D: FlowDemandDriver>(
        &mut self,
        dispatch: &'d FlowCx<'d, D>,
    ) -> RegionOutcome {
        let outcome = loop {
            if let Some(outcome) = self.step(dispatch) {
                break outcome;
            }
        };
        verter_debug_assert!(
            self.stack.is_empty(),
            "a completed root region leaves no nested activation"
        );
        self.root
            .with_dependent_mut(|_, active| drop(std::mem::take(&mut active.run.frames)));
        outcome
    }

    /// Run `finish` on the root activation's owned inputs and evaluator
    /// state, with the evaluation's tables.
    pub(super) fn with_root<R>(
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
