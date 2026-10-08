//! The checker compatibility policy: the allowance of each type operation
//! the checker bounds, in the operation's own units, and the diagnostic the
//! checker reports when an operation exhausts it.
//!
//! Each operation's allowance is checked by its budget GATE here, and only a
//! gate mints an [`OperationRefusal`]: the witness that this operation, and
//! not some other work, ran out of its own allowance. The answer after a
//! refusal is the checker's recovery — its error type
//! ([`resource_recovery`]), or a false relation — as a RESOURCE PARTIAL:
//! usable, but incomplete and never kept, because the allowance rather than
//! the types decided it. The only other source of a resource diagnostic is
//! a [`CertifiedDivergence`]: a sound proof that an operation can never
//! reach a value, reported at once with the checker's TS2589 and its
//! recovery ([`divergence_recovery`]) as a complete answer. How much work a
//! demand spends overall is a separate, operational bound: the
//! connected-demand ledger's trip is plain typed incompleteness and never
//! becomes one of these diagnostics.
//!
//! Each counter below starts where the checker's starts and resets where the
//! checker's resets, so a refusal never depends on what an earlier query
//! left in a memo.

use super::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    RecoveryBasis, SemanticNodeData, SemanticNodeId,
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

/// The witness that one type operation exhausted its own allowance: minted
/// only by the budget gates in this module, so a resource diagnostic can
/// never be attributed to an operation that did not run out. Work a demand
/// spent elsewhere, a ledger trip, a child operation's failure or a
/// cancellation are never a refusal.
#[must_use = "a refusal decides its operation's answer"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperationRefusal {
    diagnostic: CheckerDiagnostic,
}

impl OperationRefusal {
    /// The refusal of `operation` with the resource diagnostic `code`.
    const fn of(code: CheckerDiagnosticCode, operation: CheckerDiagnosticOperation) -> Self {
        Self {
            diagnostic: CheckerDiagnostic { code, operation },
        }
    }

    /// The diagnostic the checker reports for the refused operation.
    pub(crate) const fn diagnostic(self) -> CheckerDiagnostic {
        self.diagnostic
    }
}

/// Verter's instantiation budget, checked for one instantiation: `within`
/// is whether the connected demand admits its depth. Past it the
/// instantiation is refused with the checker's TS2589, at Verter's limit.
pub(crate) fn instantiation_budget(within: bool) -> Result<(), OperationRefusal> {
    if within {
        return Ok(());
    }
    Err(OperationRefusal::of(
        CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
        CheckerDiagnosticOperation::InstantiationBudget,
    ))
}

/// A sound, budget-independent proof that one operation can never reach a
/// value: a CERTIFIED divergence. It is minted only by the constructors
/// below, each of which names the one proof class it stands for, so a
/// divergence can never be claimed from growth, from repeated names with
/// different arguments, or from a query that merely meets itself on its
/// path (a deferred position legitimately does, and a circular declaration
/// is the checker's TS2456 instead).
#[must_use = "a certified divergence decides its operation's answer"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CertifiedDivergence {
    operation: CheckerDiagnosticOperation,
}

impl CertifiedDivergence {
    /// The operation reached a state IDENTICAL to one it is still
    /// evaluating on its own path: the same declaration with the same
    /// arguments under the same evaluation context and phase, each step a
    /// deterministic function of that state. The run from there repeats
    /// forever. Only the operation that holds both states may claim it —
    /// a conditional tail run meeting arguments it already took, or the lib
    /// `Awaited<T>` conditional re-entering itself with the same argument.
    pub(crate) const fn exact_recurrence(operation: CheckerDiagnosticOperation) -> Self {
        Self { operation }
    }

    /// The diagnostic the checker reports for the divergence (TS2589).
    pub(crate) const fn diagnostic(self) -> CheckerDiagnostic {
        CheckerDiagnostic {
            code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
            operation: self.operation,
        }
    }
}

/// The checker's error type after `divergence`, as the typed recovery
/// carrier: a complete answer, kept like any semantic answer, because the
/// proof holds under every allowance. `origin` is the diverging
/// operation's authored form, if one already exists.
pub(crate) fn divergence_recovery(
    graph: &SemanticGraphStore,
    divergence: CertifiedDivergence,
    origin: Option<SemanticNodeId>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
        diagnostic: divergence.diagnostic(),
        basis: RecoveryBasis::Certified,
        origin,
    }))
}

/// The checker's error type after `refusal`, as the typed recovery carrier.
/// `origin` is a type that already exists for the refused operation — its
/// authored form — kept for display and never read as its answer. The
/// recovery is a resource partial: the caller marks its result incomplete.
/// A relation refusal (TS2859) recovers as a false relation instead, never
/// as a type.
pub(crate) fn resource_recovery(
    graph: &SemanticGraphStore,
    refusal: OperationRefusal,
    origin: Option<SemanticNodeId>,
) -> SemanticNodeId {
    verter_debug_assert::verter_debug_assert!(
        refusal.diagnostic().recovery().is_some(),
        "a relation refusal recovers as a false relation"
    );
    graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
        diagnostic: refusal.diagnostic(),
        basis: RecoveryBasis::Budget,
        origin,
    }))
}

/// The checker's TS2456 for a type alias whose declared type requires
/// itself: the alias's body applies the alias — directly, or through other
/// aliases — where the checker instantiates eagerly, with no conditional
/// type of the declaration deciding on the way. Through a conditional type's
/// branch the same application is the next step of the tail loop instead,
/// and a position the checker defers (an object member, an array or tuple
/// element) is a lazy reference. A checker error, not a resource limit:
/// the alias is `any` however large the budgets.
///
/// Measured on TypeScript 7.0.2, all four `strictNullChecks` ×
/// `noImplicitAny` settings: `type G<T> = G<[T]>`, `type G<T> = G<T>`,
/// `type G<T> = H<T>; type H<T> = G<[T]>` and `type G<T> = H<[T]>; type H<T>
/// = T extends any ? G<T> : never` report TS2456 and `G<string>` is `any`;
/// `type G<T> = { a: G<[T]> }` and `type G<T> = G<[T]>[]` report nothing;
/// `type G<T> = T extends never ? never : G<[T]>` is TS2589.
pub(crate) const fn circular_type_alias() -> CheckerDiagnostic {
    CheckerDiagnostic {
        code: CheckerDiagnosticCode::CircularTypeAlias,
        operation: CheckerDiagnosticOperation::TypeAliasDeclaration,
    }
}

/// The checker's error type after `diagnostic`, a diagnostic the types
/// themselves decide (never a resource limit): the typed recovery carrier,
/// a complete answer.
pub(crate) fn checker_recovery(
    graph: &SemanticGraphStore,
    diagnostic: CheckerDiagnostic,
) -> SemanticNodeId {
    verter_debug_assert::verter_debug_assert!(
        !diagnostic.code.is_resource_limit(),
        "a resource diagnostic comes only from a refusal or a certified divergence"
    );
    graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
        diagnostic,
        basis: RecoveryBasis::Certified,
        origin: None,
    }))
}

/// The checker's instantiation depth along one evaluation path
/// (`instantiationDepth`), entered and left level by level. Reaching
/// [`INSTANTIATION_DEPTH`] refuses the path with TS2589.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstantiationDepth {
    depth: u32,
}

impl InstantiationDepth {
    /// A path the checker has already entered `depth` levels deep.
    pub(crate) const fn entered(depth: u32) -> Self {
        Self { depth }
    }

    /// Enter `levels` more levels of `operation`: its refusal when the path
    /// reaches the limit there (the levels are entered either way; leave
    /// them with [`Self::leave`]).
    pub(crate) fn enter(
        &mut self,
        levels: u32,
        operation: CheckerDiagnosticOperation,
    ) -> Result<(), OperationRefusal> {
        self.depth += levels;
        if self.depth < INSTANTIATION_DEPTH {
            return Ok(());
        }
        Err(OperationRefusal::of(
            CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
            operation,
        ))
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
/// the same TS2589 there. A body applying its own alias with no conditional
/// type deciding is not a tail run but a circular declaration
/// ([`circular_type_alias`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConditionalTail {
    steps: u32,
}

impl ConditionalTail {
    /// A run the checker enters with `steps` tail steps already counted.
    pub(crate) const fn resumed(steps: u32) -> Self {
        Self { steps }
    }

    /// Take one more tail step of `operation`: its refusal when the run
    /// reaches `budget` there.
    pub(crate) fn step(
        &mut self,
        budget: u32,
        operation: CheckerDiagnosticOperation,
    ) -> Result<(), OperationRefusal> {
        self.steps += 1;
        if self.steps < budget {
            return Ok(());
        }
        Err(OperationRefusal::of(
            CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
            operation,
        ))
    }
}

/// The structured comparisons one relation check has recorded. Reaching
/// [`RELATION_COMPARISONS`] refuses the check with TS2859.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RelationComplexity {
    recorded: u32,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static RELATION_COMPARISONS_FOR_TESTS: std::cell::Cell<Option<u32>> =
        const { std::cell::Cell::new(None) };
}

/// The allowance a relation check records against: the checker's, or a
/// lower one a test installs on this thread to reach it quickly.
pub(crate) fn relation_comparisons() -> u32 {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(allowance) = RELATION_COMPARISONS_FOR_TESTS.with(std::cell::Cell::get) {
        return allowance;
    }
    RELATION_COMPARISONS
}

/// Run `run` with relation checks on this thread recording against
/// `allowance` instead of the checker's, through the same code path.
#[cfg(any(test, feature = "test-support"))]
pub fn with_relation_comparisons_for_tests<R>(allowance: u32, run: impl FnOnce() -> R) -> R {
    let previous = RELATION_COMPARISONS_FOR_TESTS.with(|cell| cell.replace(Some(allowance)));
    let result = run();
    RELATION_COMPARISONS_FOR_TESTS.with(|cell| cell.set(previous));
    result
}

impl RelationComplexity {
    /// Record one structured comparison: the check's TS2859 refusal when it
    /// has none left.
    pub(crate) fn record(&mut self) -> Result<(), OperationRefusal> {
        if self.recorded >= relation_comparisons() {
            return Err(OperationRefusal::of(
                CheckerDiagnosticCode::RelationTooComplex,
                CheckerDiagnosticOperation::Relation,
            ));
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
/// (`checkCrossProductUnion`): `Ok(size)` when `operation` builds it, its
/// TS2590 refusal otherwise.
pub(crate) fn cross_product_union(
    factors: impl IntoIterator<Item = ProductFactor>,
    operation: CheckerDiagnosticOperation,
) -> Result<usize, OperationRefusal> {
    let mut size: u64 = 1;
    for factor in factors {
        size = match factor {
            ProductFactor::Union(width) => size.saturating_mul(width as u64),
            ProductFactor::Never => 0,
            ProductFactor::Single => size,
        };
    }
    if size >= CROSS_PRODUCT_UNION_SIZE {
        return Err(OperationRefusal::of(
            CheckerDiagnosticCode::UnionTooComplex,
            operation,
        ));
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
        let ts2590 = Err(OperationRefusal::of(
            CheckerDiagnosticCode::UnionTooComplex,
            OP,
        ));
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
            check.record().map_err(OperationRefusal::diagnostic),
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
        const TAIL: CheckerDiagnosticOperation = CheckerDiagnosticOperation::ConditionalTail;
        for budget in [1000, 5] {
            let mut run = ConditionalTail::resumed(0);
            assert!((1..budget).all(|_| run.step(budget, TAIL).is_ok()));
            assert_eq!(
                run.step(budget, TAIL).map_err(OperationRefusal::diagnostic),
                Err(CheckerDiagnostic {
                    code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
                    operation: TAIL,
                })
            );
            let mut resumed = ConditionalTail::resumed(1);
            assert!((2..budget).all(|_| resumed.step(budget, TAIL).is_ok()));
            assert!(resumed.step(budget, TAIL).is_err());
        }
    }

    /// A path fails on the level that reaches depth 100, and leaving levels
    /// makes room again.
    #[test]
    fn an_instantiation_path_fails_at_the_checker_depth() {
        const AWAITED: CheckerDiagnosticOperation = CheckerDiagnosticOperation::LibAwaited;
        let mut path = InstantiationDepth::entered(2);
        assert!((3..INSTANTIATION_DEPTH).all(|_| path.enter(1, AWAITED).is_ok()));
        assert!(path.enter(1, AWAITED).is_err());
        path.leave(2);
        assert!(path.enter(1, AWAITED).is_ok());
    }

    /// Every refusal carries a resource diagnostic, and only a resource
    /// diagnostic has no complete recovery.
    #[test]
    fn every_refusal_is_a_resource_diagnostic() {
        let refusals = [
            instantiation_budget(false).unwrap_err(),
            ConditionalTail::resumed(4)
                .step(5, CheckerDiagnosticOperation::ConditionalTail)
                .unwrap_err(),
            cross_product_union([ProductFactor::Union(usize::MAX)], OP).unwrap_err(),
            with_relation_comparisons_for_tests(0, || RelationComplexity::default().record())
                .unwrap_err(),
        ];
        assert!(refusals
            .iter()
            .all(|refusal| refusal.diagnostic().code.is_resource_limit()));
        assert!(!CheckerDiagnosticCode::RecursiveFulfillmentCallback.is_resource_limit());
        assert!(!CheckerDiagnosticCode::ArgumentNotAssignable.is_resource_limit());
    }
}
