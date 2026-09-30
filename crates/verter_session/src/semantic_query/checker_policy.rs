//! The checker compatibility policy: every limit at which the pinned checker
//! gives up on a type operation, in the checker's own units, and the
//! diagnostic it reports there.
//!
//! A limit here decides a checker FACT, never how far Verter computes. When
//! an operation reaches one, the answer is the checker's error type after the
//! diagnostic ([`QueryError::CheckerRecovery`](super::QueryError::CheckerRecovery)),
//! which reads and relates as `any` exactly as the checker's does; where
//! Verter can still name the type itself, the recovery carries that answer
//! beside it as its `beyond` type, for a consumer that wants the type rather
//! than the checker's recovery. How much work Verter spends is a separate,
//! operational question: the connected-demand ledger bounds it, and its trip
//! is typed incompleteness, never one of these facts.
//!
//! Each counter below starts where the checker's starts and resets where the
//! checker's resets, so the fact never depends on what an earlier query left
//! in a memo.
//!
//! Where Verter evaluates past a checker limit on budgets of its own, the
//! diagnostic a budget reports when it is exhausted is here too: the
//! checker's own code and message, at Verter's limit instead of the
//! checker's ([`instantiation_budget`]).

use super::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    SemanticNodeData, SemanticNodeId,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// The checker's `instantiationDepth` limit: an instantiation nested this
/// deep fails with TS2589.
pub(crate) const INSTANTIATION_DEPTH: u32 = 100;

/// The checker's `checkCrossProductUnion` limit: an intersection
/// distributed over unions, a template literal over union spans, or an
/// object spread over union operands whose cross product has at least this
/// many constituents fails with TS2590 before a constituent is built.
pub(crate) const CROSS_PRODUCT_UNION_SIZE: u64 = 100_000;

/// The checker's relation-complexity allowance for one relation check
/// (`checkTypeRelatedTo`'s `relationCount`): it starts at `(16,000,000 -
/// relation cache size) >> 3` and falls by one for each structured
/// comparison result the check records; at zero the relation is false with
/// TS2859. The cache term only lowers the start, so its value over an empty
/// cache, this, is the most the checker ever allows. The count lives on a
/// relation chain's first frame, one chain per check, and skips memoized
/// answers as the checker skips cached ones. The connected-work ledger is
/// to be sized above it (two units per recorded comparison) once it charges
/// the bytes a relation holds; until then it can refuse a relation first,
/// as typed incompleteness.
///
/// Measured on TypeScript 7.0.2: a union of `M` single-property objects
/// against the same objects in reverse order records about `M² / 2`
/// comparisons; `[S] extends [T] ? 1 : 2` is `1` at 1,800 arms and `2` under
/// TS2859 at 2,100.
pub(crate) const RELATION_COMPARISONS: u32 = 16_000_000 >> 3;
/// Verter's instantiation budget, checked for one instantiation: `within`
/// is whether the connected demand's ledger admits its depth. Past it the
/// instantiation is the checker's TS2589 — its code and message — reported
/// at Verter's limit rather than the checker's, which Verter evaluates
/// past.
pub(crate) fn instantiation_budget(within: bool) -> Result<(), CheckerDiagnostic> {
    if within {
        return Ok(());
    }
    Err(CheckerDiagnostic {
        code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
        operation: CheckerDiagnosticOperation::InstantiationBudget,
    })
}

/// The checker's error type after `diagnostic`: the typed recovery carrier,
/// with `beyond` the type Verter names past the checker's limit, if any.
pub(crate) fn checker_recovery(
    graph: &SemanticGraphStore,
    diagnostic: CheckerDiagnostic,
    beyond: Option<SemanticNodeId>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
        diagnostic,
        beyond,
    }))
}

/// The checker's instantiation depth along one evaluation path
/// (`instantiationDepth`), entered and left level by level. Reaching
/// [`INSTANTIATION_DEPTH`] is the TS2589 fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstantiationDepth {
    depth: u32,
}

impl InstantiationDepth {
    /// A path the checker has already entered `depth` levels deep.
    pub(crate) const fn entered(depth: u32) -> Self {
        Self { depth }
    }

    /// Enter `levels` more levels. `false` when the path reaches the
    /// checker's limit there (the levels are entered either way; leave them
    /// with [`Self::leave`]).
    pub(crate) fn enter(&mut self, levels: u32) -> bool {
        self.depth += levels;
        self.depth < INSTANTIATION_DEPTH
    }

    /// Leave `levels` levels entered by [`Self::enter`].
    pub(crate) fn leave(&mut self, levels: u32) {
        self.depth -= levels;
    }
}

/// One tail run of a conditional type, counted as the checker counts it
/// (`getConditionalType`'s `tailCount`) against a budget: the run fails
/// with TS2589 at its budget's step. The checker's own budget is 1,000 —
/// measured on TypeScript 7.0.2 over `type Build<N extends number, Acc
/// extends unknown[] = []> = Acc["length"] extends N ? Acc["length"] :
/// Build<N, [...Acc, 0]>` (all four `strictNullChecks` × `noImplicitAny`
/// settings agree), `Build<999>` is `999` and `Build<1000>` is `any`
/// under TS2589 — and Verter runs to its own, far larger one, reporting
/// the same TS2589 there.
///
/// Known diagnostic gap: a generic alias whose body applies itself
/// directly, `type Grow<T> = Grow<[T]>`, is circular to the checker
/// (TS2456, and `any`); Verter runs its tail loop to the tail budget
/// instead and reports TS2589, with the same `any`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConditionalTail {
    steps: u32,
}

impl ConditionalTail {
    /// A run the checker enters with `steps` tail steps already counted.
    pub(crate) const fn resumed(steps: u32) -> Self {
        Self { steps }
    }

    /// Take one more tail step. `false` when the run reaches `budget`
    /// there.
    pub(crate) fn step(&mut self, budget: u32) -> bool {
        self.steps += 1;
        self.steps < budget
    }
}

/// The structured comparisons one relation check has recorded. Reaching
/// [`RELATION_COMPARISONS`] is the TS2859 fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RelationComplexity {
    recorded: u32,
}

#[cfg(test)]
thread_local! {
    static RELATION_COMPARISONS_FOR_TESTS: std::cell::Cell<Option<u32>> =
        const { std::cell::Cell::new(None) };
}

/// The allowance a relation check records against: the checker's, or a
/// lower one a test installs on this thread to reach it quickly.
fn relation_comparisons() -> u32 {
    #[cfg(test)]
    if let Some(allowance) = RELATION_COMPARISONS_FOR_TESTS.with(std::cell::Cell::get) {
        return allowance;
    }
    RELATION_COMPARISONS
}

/// Run `run` with relation checks on this thread recording against
/// `allowance` instead of the checker's, through the same code path.
#[cfg(test)]
pub(crate) fn with_relation_comparisons_for_tests<R>(allowance: u32, run: impl FnOnce() -> R) -> R {
    let previous = RELATION_COMPARISONS_FOR_TESTS.with(|cell| cell.replace(Some(allowance)));
    let result = run();
    RELATION_COMPARISONS_FOR_TESTS.with(|cell| cell.set(previous));
    result
}

impl RelationComplexity {
    /// Record one structured comparison: the TS2859 fact when the check
    /// has none left.
    pub(crate) fn record(&mut self) -> Result<(), CheckerDiagnostic> {
        if self.recorded >= relation_comparisons() {
            return Err(CheckerDiagnostic {
                code: CheckerDiagnosticCode::RelationTooComplex,
                operation: CheckerDiagnosticOperation::Relation,
            });
        }
        self.recorded += 1;
        Ok(())
    }
}

/// One factor of a cross product, as `getCrossProductUnionSize` counts it: a
/// union contributes its constituents, `never` makes the product empty, and
/// any other type contributes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductFactor {
    /// A union of this many constituents.
    Union(usize),
    /// `never`.
    Never,
    /// Any other type.
    Single,
}

/// The size of the cross product over `factors`, checked against
/// [`CROSS_PRODUCT_UNION_SIZE`] BEFORE any constituent exists
/// (`checkCrossProductUnion`): `Ok(size)` when the checker builds it, the
/// TS2590 fact `operation` raises otherwise.
pub(crate) fn cross_product_union(
    factors: impl IntoIterator<Item = ProductFactor>,
    operation: CheckerDiagnosticOperation,
) -> Result<usize, CheckerDiagnostic> {
    let mut size: u64 = 1;
    for factor in factors {
        size = match factor {
            ProductFactor::Union(width) => size.saturating_mul(width as u64),
            ProductFactor::Never => 0,
            ProductFactor::Single => size,
        };
    }
    if size >= CROSS_PRODUCT_UNION_SIZE {
        return Err(CheckerDiagnostic {
            code: CheckerDiagnosticCode::UnionTooComplex,
            operation,
        });
    }
    Ok(size as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OP: CheckerDiagnosticOperation = CheckerDiagnosticOperation::Intersection;

    /// The product is checked at 100,000 inclusive, counting a union by its
    /// constituents, `never` as an empty product and any other type as one.
    #[test]
    fn the_cross_product_rule_counts_as_the_checker_counts() {
        let ts2590 = Err(CheckerDiagnostic {
            code: CheckerDiagnosticCode::UnionTooComplex,
            operation: OP,
        });
        let product = |factors: &[ProductFactor]| cross_product_union(factors.iter().copied(), OP);
        assert_eq!(
            product(&[ProductFactor::Union(369), ProductFactor::Union(271)]),
            Ok(99_999)
        );
        assert_eq!(
            product(&[ProductFactor::Union(400), ProductFactor::Union(250)]),
            ts2590
        );
        assert_eq!(product(&[ProductFactor::Union(10); 5]), ts2590);
        assert_eq!(product(&[ProductFactor::Union(10); 4]), Ok(10_000));
        assert_eq!(
            product(&[
                ProductFactor::Union(400),
                ProductFactor::Single,
                ProductFactor::Union(249)
            ]),
            Ok(99_600)
        );
        assert_eq!(
            product(&[
                ProductFactor::Union(1_000_000),
                ProductFactor::Never,
                ProductFactor::Union(1_000_000)
            ]),
            Ok(0)
        );
        assert_eq!(
            product(&[
                ProductFactor::Union(usize::MAX),
                ProductFactor::Union(usize::MAX)
            ]),
            ts2590
        );
    }

    /// A relation check records 2,000,000 structured comparisons and is
    /// refused the next.
    #[test]
    fn a_relation_check_records_the_checkers_comparisons() {
        let mut check = RelationComplexity::default();
        for _ in 0..RELATION_COMPARISONS {
            assert_eq!(check.record(), Ok(()));
        }
        assert_eq!(
            check.record(),
            Err(CheckerDiagnostic {
                code: CheckerDiagnosticCode::RelationTooComplex,
                operation: CheckerDiagnosticOperation::Relation,
            })
        );
        assert_eq!(RELATION_COMPARISONS, 2_000_000);
    }

    /// A tail run fails at its budget's step — the checker's at its
    /// 1000th; a run resumed with steps already counted fails that many
    /// steps sooner.
    #[test]
    fn a_tail_run_fails_at_its_budget_step() {
        for budget in [1000, 5] {
            let mut run = ConditionalTail::resumed(0);
            assert!((1..budget).all(|_| run.step(budget)));
            assert!(!run.step(budget));
            let mut resumed = ConditionalTail::resumed(1);
            assert!((2..budget).all(|_| resumed.step(budget)));
            assert!(!resumed.step(budget));
        }
    }

    /// A path fails on the level that reaches depth 100, and leaving levels
    /// makes room again.
    #[test]
    fn an_instantiation_path_fails_at_the_checker_depth() {
        let mut path = InstantiationDepth::entered(2);
        assert!((3..INSTANTIATION_DEPTH).all(|_| path.enter(1)));
        assert!(!path.enter(1));
        path.leave(2);
        assert!(path.enter(1));
    }
}
