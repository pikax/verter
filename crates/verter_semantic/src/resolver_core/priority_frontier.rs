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
//! the caller's accumulator) — but [`crate::resolver_core::AttemptOutput`]
//! deliberately has no public truncate/reset. The evaluator therefore
//! returns `KernelAttempt<Option<T>>`, reusing the
//! `CompletedAttempt<T>`/`KernelAttempt<T>` envelope (this file's own
//! envelope: each candidate's own `Complete`
//! carries its own fresh output, `NeedInputs`/`Terminal` carry none
//! (structurally — no rollback needed, discard falls out of the type).
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

use crate::resolver_core::{
    AttemptOutcome, AttemptOutput, CompletedAttempt, InputKey, InputResolutionBudgets,
    KernelAttempt, LoadSet, ResolutionBasis,
};

/// The resumable state of one ordered priority frontier.
///
/// Keeping the rules in a state object lets callers whose candidate source is
/// itself a graph traversal suspend one frontier, evaluate a child frontier,
/// and resume without putting that graph depth on the native call stack.
pub(crate) struct PriorityFrontierState {
    expected_basis: ResolutionBasis,
    accumulated: AttemptOutput,
    blocked: Option<BTreeSet<InputKey>>,
}

impl PriorityFrontierState {
    #[must_use]
    pub(crate) fn new_with_budgets(
        expected_basis: ResolutionBasis,
        _budgets: InputResolutionBudgets,
    ) -> Self {
        Self {
            expected_basis,
            accumulated: AttemptOutput::new(),
            blocked: None,
        }
    }

    /// Applies one candidate. `Some` means the frontier has reached a final
    /// outcome; `None` means the next candidate may be evaluated.
    pub(crate) fn push<T>(
        &mut self,
        candidate: KernelAttempt<Option<T>>,
    ) -> Option<KernelAttempt<Option<T>>> {
        match candidate {
            AttemptOutcome::Complete(CompletedAttempt {
                value: Some(hit),
                output,
            }) => Some(match self.blocked.take() {
                // Rule 5: a lower-priority hit after a higher block cannot
                // win.
                Some(blocked_set) => AttemptOutcome::NeedInputs(LoadSet::new(
                    blocked_set.into_iter().collect(),
                    self.expected_basis,
                )),
                // Rule 2: hit before any block — merge and return.
                None => {
                    if let Err(failure) = self.accumulated.merge(output) {
                        return Some(AttemptOutcome::Terminal(failure));
                    }
                    AttemptOutcome::Complete(CompletedAttempt::new(
                        Some(hit),
                        std::mem::take(&mut self.accumulated),
                    ))
                }
            }),
            AttemptOutcome::Complete(CompletedAttempt {
                value: None,
                output,
            }) => {
                // Rule 1: merge completed-miss outputs BEFORE any block. A
                // miss encountered AFTER a block contributes nothing.
                if self.blocked.is_none() {
                    if let Err(failure) = self.accumulated.merge(output) {
                        return Some(AttemptOutcome::Terminal(failure));
                    }
                }
                None
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
            AttemptOutcome::Terminal(failure) => Some(match self.blocked.take() {
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
            None => AttemptOutcome::Complete(CompletedAttempt::new(
                None,
                std::mem::take(&mut self.accumulated),
            )),
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
