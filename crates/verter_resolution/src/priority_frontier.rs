//! Shared ordered-candidate fallthrough for resolver frontiers.
//!
//! A REUSABLE PRIVATE helper, not a new public abstraction — every
//! priority-ordered fallthrough site (module resolution's extension/index
//! candidate chain, ambient-lib precedence, …) is meant to route through
//! this ONE combinator rather than re-implementing the same
//! hit/block/miss/terminal precedence logic per call site.
//!
//! ## Signature
//!
//! An evaluator that writes directly into a shared `AttemptOutput` needs
//! a rollback mechanism for the discard rules
//! below (a blocked/terminal candidate must not leave partial writes in
//! the caller's accumulator) — but [`crate::AttemptOutput`]
//! deliberately has no public truncate/reset. Two evaluator shapes keep
//! the discard rules without one:
//!
//! - [`PriorityFrontierState::push`] takes a `KernelAttempt<Option<T>>`,
//!   reusing the `CompletedAttempt<T>`/`KernelAttempt<T>` envelope: each
//!   candidate's own `Complete` carries its own fresh output and
//!   `NeedInputs`/`Terminal` carry none (structurally — no rollback
//!   needed, discard falls out of the type).
//! - [`PriorityFrontierState::push_read`] serves an evaluator that
//!   records straight into the frontier's live output instead of building
//!   an output per candidate. Nothing is ever rolled back, so the split is
//!   in the types: the evaluator first settles its observation reads into a
//!   [`CandidateRead`], and only a settled hit or miss reaches its
//!   [`CandidateRecorder`], the one holder of the output. A candidate that
//!   needs an input cannot have recorded anything. That matters even though
//!   a blocked frontier never publishes its output: witnesses left in the
//!   live output would stay retained in the operation's ledger, where the
//!   budget checks of later discarded candidates would count them. A
//!   recording that fails part-way is terminal: before a block it ends the
//!   frontier and its output with it (rule 6), and after a block nothing is
//!   recorded (rules 1 and 5).
//!
//! The semantics are the ten rules below.
//!
//! ## Rules
//!
//! 1. Before any block, merge completed-miss outputs in candidate order.
//! 2. On a hit before a block, merge its output and return the hit.
//! 3. On the FIRST block, retain ONLY its `LoadSet` — do not publish
//!    accumulated output.
//! 4. Continue through bounded siblings to union further same-basis
//!    missing keys.
//! 5. A known lower-priority hit AFTER a higher block cannot win — stop
//!    and return the blocked set.
//! 6. A terminal before any block propagates.
//! 7. A terminal encountered only speculatively after a higher-priority
//!    block does NOT outrank that block — return the blocked set,
//!    reconsider on retry.
//! 8. A basis mismatch is NOT unioned or loaded — return the mismatching
//!    `LoadSet`; the outer driver detects the mismatch and restarts under
//!    the new basis.
//! 9. Every `NeedInputs`/`Terminal` path discards ALL branch/frontier
//!    output.
//! 10. An exhausted miss publishes the COMPLETE ordered rejected-candidate
//!     witness required by the resolution-witness contract.

use std::collections::BTreeSet;

use crate::{AttemptOutput, CompletedAttempt, InputResolutionRetention, KernelAttempt};
use verter_session_query::resolution::{
    AttemptFailure, AttemptOutcome, InputKey, InputResolutionBudgets, LoadSet, ResolutionBasis,
};

/// The resumable state of one ordered priority frontier.
///
/// Keeping the rules in a state object lets callers whose candidate source is
/// itself a graph traversal suspend one frontier, evaluate a child frontier,
/// and resume without putting that graph depth on the native call stack.
pub(crate) struct PriorityFrontierState {
    expected_basis: ResolutionBasis,
    /// The output kept so far, created on the first output the frontier
    /// keeps. A frontier that ends blocked never builds one.
    accumulated: Option<AttemptOutput>,
    /// The retention scope current when the frontier was created; the
    /// accumulated output is retained there whenever it is created.
    retention: InputResolutionRetention,
    blocked: Option<BTreeSet<InputKey>>,
}

/// What a candidate's observation reads settled, before anything about it
/// is recorded: a hit or a miss carrying what recording it needs, or why it
/// cannot complete yet.
///
/// A short-lived value moved once per candidate; boxing its one large
/// variant would allocate for every missing input instead.
#[allow(clippy::large_enum_variant)]
pub(crate) enum CandidateRead<H, M> {
    Hit(H),
    Miss(M),
    /// One missing input, under the basis of the snapshot that lacked it:
    /// the one-key `NeedInputs` without building its load set.
    NeedInput(InputKey, ResolutionBasis),
    Terminal(AttemptFailure),
}

/// Records a completed candidate for [`PriorityFrontierState::push_read`].
/// It is reached only with a [`CandidateRead::Hit`] or
/// [`CandidateRead::Miss`], so a candidate that needs an input never holds
/// the frontier's output.
///
/// A recorder only records what the read settled: it must not read the
/// attempt view itself. A read made while holding the output could need an
/// input after witnesses were recorded, which is exactly what the split
/// rules out.
pub(crate) trait CandidateRecorder<H, M> {
    type Answer;

    /// Records a hit before any block into the frontier's output (rule 2)
    /// and spells its answer. Recording a witness the output already holds
    /// changes nothing, exactly as merging a separate output would.
    fn record_hit(self, hit: H, output: &mut AttemptOutput)
        -> Result<Self::Answer, AttemptFailure>;

    /// Records a miss before any block into the frontier's output (rule 1).
    fn record_miss(self, miss: M, output: &mut AttemptOutput) -> Result<(), AttemptFailure>;

    /// Checks that a fresh output could retain a miss after a block, which
    /// contributes nothing (rule 1); a failure stops the frontier on its
    /// block (rule 7), as discarding a built output would.
    fn admit_discarded_miss(self, miss: M) -> Result<(), AttemptFailure>;
}

impl PriorityFrontierState {
    #[must_use]
    pub(crate) fn new_with_budgets(
        expected_basis: ResolutionBasis,
        _budgets: InputResolutionBudgets,
    ) -> Self {
        Self {
            expected_basis,
            accumulated: None,
            retention: InputResolutionRetention::current_or_default(),
            blocked: None,
        }
    }

    /// The frontier's output, created when something first records into it.
    fn accumulated_mut(&mut self) -> &mut AttemptOutput {
        let retention = &self.retention;
        self.accumulated
            .get_or_insert_with(|| AttemptOutput::new_in(retention.clone()))
    }

    /// Joins a completed candidate's output to the frontier's, exactly as
    /// merging it into a new empty output would: an output of the same
    /// operation is adopted whole.
    fn keep(&mut self, output: AttemptOutput) -> Result<(), AttemptFailure> {
        match &mut self.accumulated {
            Some(accumulated) => accumulated.merge(output),
            None if output.is_retained_in(&self.retention) => {
                self.accumulated = Some(output);
                Ok(())
            }
            None => {
                // Installed only once the merge succeeds: a failed merge
                // drops the new output and releases what it retained.
                let mut accumulated = AttemptOutput::new_in(self.retention.clone());
                accumulated.merge(output)?;
                self.accumulated = Some(accumulated);
                Ok(())
            }
        }
    }

    fn take_accumulated(&mut self) -> AttemptOutput {
        self.accumulated
            .take()
            .unwrap_or_else(|| AttemptOutput::new_in(self.retention.clone()))
    }

    /// The outcome of a frontier ended on its block: the keys it needs.
    fn blocked_outcome<T>(&self, blocked: BTreeSet<InputKey>) -> KernelAttempt<T> {
        AttemptOutcome::NeedInputs(LoadSet::new(
            blocked.into_iter().collect(),
            self.expected_basis,
        ))
    }

    /// Applies one candidate. `Some` means the frontier has reached a final
    /// outcome; `None` means the next candidate may be evaluated.
    pub(crate) fn push<T>(
        &mut self,
        candidate: KernelAttempt<Option<T>>,
    ) -> Option<KernelAttempt<Option<T>>> {
        match candidate {
            AttemptOutcome::Complete(CompletedAttempt { value, output }) => {
                // Rules 1 and 2: a completed candidate's output joins the
                // frontier's only before any block.
                if self.blocked.is_none() {
                    if let Err(failure) = self.keep(output) {
                        return Some(AttemptOutcome::Terminal(failure));
                    }
                }
                // Rule 1: a miss after a block contributes nothing.
                value.map(|hit| self.hit(hit))
            }
            AttemptOutcome::NeedInputs(load_set) => {
                if load_set.basis() != self.expected_basis {
                    // Rule 8: a basis mismatch is never unioned or loaded.
                    return Some(AttemptOutcome::NeedInputs(load_set));
                }
                let blocked = self.blocked.get_or_insert_with(BTreeSet::new);
                for key in load_set.keys() {
                    if blocked.contains(key) {
                        continue;
                    }
                    blocked.insert(key.clone());
                }
                None
            }
            AttemptOutcome::Terminal(failure) => Some(self.terminal(failure)),
        }
    }

    /// Applies one candidate whose reads are settled, recording it through
    /// `recorder` straight into the frontier's output instead of building an
    /// output for it. A hit after a block is never recorded or spelled: it
    /// cannot win (rule 5). `Some` means the frontier has reached a final
    /// outcome; `None` means the next candidate may be evaluated.
    pub(crate) fn push_read<H, M, R: CandidateRecorder<H, M>>(
        &mut self,
        read: CandidateRead<H, M>,
        recorder: R,
    ) -> Option<KernelAttempt<Option<R::Answer>>> {
        match read {
            CandidateRead::Hit(hit) => Some(match self.blocked.take() {
                // Rule 5: a lower-priority hit after a higher block cannot
                // win.
                Some(blocked) => self.blocked_outcome(blocked),
                None => match recorder.record_hit(hit, self.accumulated_mut()) {
                    // Rule 2: hit before any block — return it with the
                    // output.
                    Ok(answer) => AttemptOutcome::Complete(CompletedAttempt::new(
                        Some(answer),
                        self.take_accumulated(),
                    )),
                    // Rule 6.
                    Err(failure) => AttemptOutcome::Terminal(failure),
                },
            }),
            CandidateRead::Miss(miss) => {
                let recorded = if self.blocked.is_some() {
                    recorder.admit_discarded_miss(miss)
                } else {
                    recorder.record_miss(miss, self.accumulated_mut())
                };
                recorded.err().map(|failure| self.terminal(failure))
            }
            CandidateRead::NeedInput(key, basis) => {
                if basis != self.expected_basis {
                    // Rule 8, exactly as for the one-key load set.
                    return Some(AttemptOutcome::NeedInputs(LoadSet::new(vec![key], basis)));
                }
                self.blocked.get_or_insert_with(BTreeSet::new).insert(key);
                None
            }
            CandidateRead::Terminal(failure) => Some(self.terminal(failure)),
        }
    }

    /// A hit, whose output is already the frontier's.
    fn hit<T>(&mut self, hit: T) -> KernelAttempt<Option<T>> {
        match self.blocked.take() {
            // Rule 5: a lower-priority hit after a higher block cannot win.
            Some(blocked) => self.blocked_outcome(blocked),
            // Rule 2: hit before any block — return it with the output.
            None => {
                AttemptOutcome::Complete(CompletedAttempt::new(Some(hit), self.take_accumulated()))
            }
        }
    }

    fn terminal<T>(&mut self, failure: AttemptFailure) -> KernelAttempt<T> {
        match self.blocked.take() {
            // Rule 6: a terminal before any block propagates.
            None => AttemptOutcome::Terminal(failure),
            // Rule 7: a terminal after a block does not outrank it.
            Some(blocked) => self.blocked_outcome(blocked),
        }
    }

    /// Completes an exhausted frontier (rule 10).
    pub(crate) fn finish<T>(&mut self) -> KernelAttempt<Option<T>> {
        match self.blocked.take() {
            Some(blocked) => self.blocked_outcome(blocked),
            None => AttemptOutcome::Complete(CompletedAttempt::new(None, self.take_accumulated())),
        }
    }
}

/// The shared ordered-candidate-fallthrough combinator — see module docs
/// for the full rule set. `expected_basis` is the basis every candidate's
/// `NeedInputs(LoadSet)` is checked against (rule 8).
///
/// The complete attempt driver uses [`PriorityFrontierState`] directly; this
/// convenience wrapper serves pure ordered candidate sets.
#[allow(dead_code)]
pub(crate) fn priority_frontier<C, T>(
    expected_basis: ResolutionBasis,
    candidates: impl IntoIterator<Item = C>,
    evaluate: impl FnMut(C) -> KernelAttempt<Option<T>>,
) -> KernelAttempt<Option<T>> {
    priority_frontier_with_budgets(
        expected_basis,
        InputResolutionBudgets::default(),
        candidates,
        evaluate,
    )
}

pub(crate) fn priority_frontier_with_budgets<C, T>(
    expected_basis: ResolutionBasis,
    budgets: InputResolutionBudgets,
    candidates: impl IntoIterator<Item = C>,
    mut evaluate: impl FnMut(C) -> KernelAttempt<Option<T>>,
) -> KernelAttempt<Option<T>> {
    let mut state = PriorityFrontierState::new_with_budgets(expected_basis, budgets);

    for candidate in candidates {
        if let Some(outcome) = state.push(evaluate(candidate)) {
            return outcome;
        }
    }

    state.finish()
}

#[cfg(test)]
#[path = "priority_frontier_tests.rs"]
mod priority_frontier_tests;
