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

use super::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    SemanticNodeData, SemanticNodeId,
};
use crate::semantic_query_memo::SemanticGraphStore;

/// The checker's `instantiationDepth` limit: an instantiation nested this
/// deep fails with TS2589.
pub(crate) const INSTANTIATION_DEPTH: u32 = 100;

/// The checker's limit on one TAIL run of a conditional type
/// (`getConditionalType`'s `tailCount`): the run fails with TS2589 at its
/// 1000th tail step.
///
/// Measured on TypeScript 7.0.2 over `type Build<N extends number, Acc
/// extends unknown[] = []> = Acc["length"] extends N ? Acc["length"] :
/// Build<N, [...Acc, 0]>` (all four `strictNullChecks` × `noImplicitAny`
/// settings agree): `Build<998>` is `998`, `Build<999>` is `999`, and
/// `Build<1000>` is `any` under TS2589.
pub(crate) const CONDITIONAL_TAIL_STEPS: u32 = 1000;

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
/// cache, this, is the most the checker ever allows.
pub(crate) const RELATION_COMPARISONS: u32 = 16_000_000 >> 3;

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

/// One tail run of a conditional type (`tailCount`). Reaching
/// [`CONDITIONAL_TAIL_STEPS`] is the TS2589 fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConditionalTail {
    steps: u32,
}

impl ConditionalTail {
    /// A run the checker enters with `steps` tail steps already counted.
    pub(crate) const fn resumed(steps: u32) -> Self {
        Self { steps }
    }

    /// Take one more tail step. `false` when the run reaches the checker's
    /// limit there.
    pub(crate) fn step(&mut self) -> bool {
        self.steps += 1;
        self.steps < CONDITIONAL_TAIL_STEPS
    }
}

/// The structured comparisons one relation check has recorded. Reaching
/// [`RELATION_COMPARISONS`] is the TS2859 fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RelationComplexity {
    recorded: u32,
}

impl RelationComplexity {
    /// Record one structured comparison: the TS2859 fact when the check
    /// has none left.
    pub(crate) fn record(&mut self) -> Result<(), CheckerDiagnostic> {
        if self.recorded >= RELATION_COMPARISONS {
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

    /// A tail run fails at its 1000th step; a run resumed with steps already
    /// counted fails that many steps sooner.
    #[test]
    fn a_tail_run_fails_at_the_checker_step() {
        let mut run = ConditionalTail::resumed(0);
        assert!((1..CONDITIONAL_TAIL_STEPS).all(|_| run.step()));
        assert!(!run.step());
        let mut resumed = ConditionalTail::resumed(1);
        assert!((2..CONDITIONAL_TAIL_STEPS).all(|_| resumed.step()));
        assert!(!resumed.step());
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
