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
//! - [`PriorityFrontierState::push_recorded`] serves an evaluator that
//!   records straight into the frontier's live output, through the
//!   [`CandidateOutput`] the frontier hands it, instead of building an
//!   output per candidate. Nothing is ever rolled back, so such an
//!   evaluator reads every observation first and records only once its
//!   outcome is known to be complete: a candidate that needs an input
//!   records nothing. That is checked in debug builds. It matters even
//!   though a blocked frontier never publishes its output: witnesses left
//!   in the live output stay retained in the operation's ledger, where the
//!   budget checks of later discarded candidates would count them. A
//!   `Terminal` may follow partial writes, because before a block it ends
//!   the frontier and its output with it (rule 6), and after a block the
//!   frontier records nothing (rules 1 and 5).
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
    AttemptOutcome, InputKey, InputResolutionBudgets, LoadSet, ResolutionBasis,
};

/// The resumable state of one ordered priority frontier.
///
/// Keeping the rules in a state object lets callers whose candidate source is
/// itself a graph traversal suspend one frontier, evaluate a child frontier,
/// and resume without putting that graph depth on the native call stack.
pub(crate) struct PriorityFrontierState {
    expected_basis: ResolutionBasis,
    /// Whether the candidate being evaluated has recorded into
    /// `accumulated` (see the module docs: a candidate that needs an input
    /// must not have).
    candidate_recorded: bool,
    /// The output kept so far, created on the first output the frontier
    /// keeps. A frontier that ends blocked never builds one.
    accumulated: Option<AttemptOutput>,
    /// The retention scope current when the frontier was created; the
    /// accumulated output is retained there whenever it is created.
    retention: InputResolutionRetention,
    blocked: Option<BTreeSet<InputKey>>,
}

/// Where a candidate evaluated for [`PriorityFrontierState::push_recorded`]
/// puts its output.
pub(crate) enum CandidateOutput<'a> {
    /// No block is held, so a completed candidate's output joins the
    /// frontier's output in candidate order (rules 1 and 2). The candidate
    /// records straight into it: recording a witness the frontier already
    /// holds changes nothing, exactly as merging a separate output would.
    Accumulate(Accumulator<'a>),
    /// A block is held, so a completed miss contributes nothing (rule 1) and
    /// a hit cannot win (rule 5). The candidate only checks that a fresh
    /// output could retain its witnesses; a failure stops the frontier on
    /// the block (rule 7), as discarding a built output would.
    Discard,
}

/// The frontier's output, created when a candidate first records into it.
pub(crate) struct Accumulator<'a> {
    output: &'a mut Option<AttemptOutput>,
    retention: &'a InputResolutionRetention,
    recorded: &'a mut bool,
}

impl Accumulator<'_> {
    /// The frontier's output, for a candidate whose outcome is known to be
    /// complete (a hit, a miss, or a terminal failure while recording).
    pub(crate) fn output(&mut self) -> &mut AttemptOutput {
        *self.recorded = true;
        let retention = self.retention;
        self.output
            .get_or_insert_with(|| AttemptOutput::new_in(retention.clone()))
    }
}

/// A candidate whose output was already handled through [`CandidateOutput`].
pub(crate) enum RecordedCandidate<T> {
    Hit(T),
    /// A hit evaluated after a block, where it cannot win (rule 5): its
    /// answer is never spelled.
    DiscardedHit,
    Miss,
    NeedInputs(LoadSet),
    /// One missing input, under the basis of the snapshot that lacked it:
    /// the one-key `NeedInputs` without building its load set.
    NeedInput(InputKey, ResolutionBasis),
    Terminal(verter_session_query::resolution::AttemptFailure),
}

impl PriorityFrontierState {
    #[must_use]
    pub(crate) fn new_with_budgets(
        expected_basis: ResolutionBasis,
        _budgets: InputResolutionBudgets,
    ) -> Self {
        Self {
            expected_basis,
            candidate_recorded: false,
            accumulated: None,
            retention: InputResolutionRetention::current_or_default(),
            blocked: None,
        }
    }

    /// Where the next candidate's output goes; see [`CandidateOutput`].
    pub(crate) fn candidate_output(&mut self) -> CandidateOutput<'_> {
        self.candidate_recorded = false;
        if self.blocked.is_some() {
            return CandidateOutput::Discard;
        }
        CandidateOutput::Accumulate(Accumulator {
            output: &mut self.accumulated,
            retention: &self.retention,
            recorded: &mut self.candidate_recorded,
        })
    }

    /// Joins a completed candidate's output to the frontier's, exactly as
    /// merging it into a new empty output would: an output of the same
    /// operation is adopted whole.
    fn keep(
        &mut self,
        output: AttemptOutput,
    ) -> Result<(), verter_session_query::resolution::AttemptFailure> {
        match &mut self.accumulated {
            Some(accumulated) => accumulated.merge(output),
            None if output.is_retained_in(&self.retention) => {
                self.accumulated = Some(output);
                Ok(())
            }
            None => {
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
                self.push_recorded(match value {
                    Some(hit) => RecordedCandidate::Hit(hit),
                    None => RecordedCandidate::Miss,
                })
            }
            AttemptOutcome::NeedInputs(load_set) => {
                self.push_recorded(RecordedCandidate::NeedInputs(load_set))
            }
            AttemptOutcome::Terminal(failure) => {
                self.push_recorded(RecordedCandidate::Terminal(failure))
            }
        }
    }

    /// Applies one candidate whose output went where
    /// [`Self::candidate_output`] said. `Some` means the frontier has reached
    /// a final outcome; `None` means the next candidate may be evaluated.
    pub(crate) fn push_recorded<T>(
        &mut self,
        candidate: RecordedCandidate<T>,
    ) -> Option<KernelAttempt<Option<T>>> {
        // A pure predicate, checked in debug builds only.
        #[cfg(debug_assertions)]
        assert!(
            !(self.candidate_recorded
                && matches!(
                    candidate,
                    RecordedCandidate::NeedInput(..) | RecordedCandidate::NeedInputs(_)
                )),
            "a candidate that needs an input recorded into the frontier's output"
        );
        self.candidate_recorded = false;
        match candidate {
            RecordedCandidate::Hit(hit) => Some(match self.blocked.take() {
                // Rule 5: a lower-priority hit after a higher block cannot
                // win.
                Some(blocked_set) => AttemptOutcome::NeedInputs(LoadSet::new(
                    blocked_set.into_iter().collect(),
                    self.expected_basis,
                )),
                // Rule 2: hit before any block — return it with the output.
                None => AttemptOutcome::Complete(CompletedAttempt::new(
                    Some(hit),
                    self.take_accumulated(),
                )),
            }),
            // Rule 5, for a hit only ever evaluated after a block.
            RecordedCandidate::DiscardedHit => Some(AttemptOutcome::NeedInputs(LoadSet::new(
                self.blocked
                    .take()
                    .expect("a discarded hit is evaluated only after a block")
                    .into_iter()
                    .collect(),
                self.expected_basis,
            ))),
            // Rule 1: a miss after a block contributes nothing.
            RecordedCandidate::Miss => None,
            RecordedCandidate::NeedInputs(load_set) => {
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
            RecordedCandidate::NeedInput(key, basis) => {
                if basis != self.expected_basis {
                    // Rule 8, exactly as for the one-key load set.
                    return Some(AttemptOutcome::NeedInputs(LoadSet::new(vec![key], basis)));
                }
                self.blocked.get_or_insert_with(BTreeSet::new).insert(key);
                None
            }
            RecordedCandidate::Terminal(failure) => Some(match self.blocked.take() {
                // Rule 6: a terminal before any block propagates.
                None => AttemptOutcome::Terminal(failure),
                // Rule 7: a terminal after a block does not outrank it.
                Some(blocked_set) => AttemptOutcome::NeedInputs(LoadSet::new(
                    blocked_set.into_iter().collect(),
                    self.expected_basis,
                )),
            }),
        }
    }

    /// Completes an exhausted frontier (rule 10).
    pub(crate) fn finish<T>(&mut self) -> KernelAttempt<Option<T>> {
        match self.blocked.take() {
            Some(keys) => AttemptOutcome::NeedInputs(LoadSet::new(
                keys.into_iter().collect(),
                self.expected_basis,
            )),
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
