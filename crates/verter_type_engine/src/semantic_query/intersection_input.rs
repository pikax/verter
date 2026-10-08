//! Compact `ReduceIntersection` inputs: empty / unary / binary / interned recipe.
//!
//! Binary construction allocates no recipe. OrderedOperands of length 0/1/2
//! normalize to Empty/Unary/Binary. A two-term sequence that contains a
//! meaningful subgroup stays a recipe.
//!
//! **Ownership.** An [`IntersectionInputId`] owns its recipe record (see
//! [`crate::semantic_query_memo::intern_table`]); the record is freed when
//! the last key, family or caller holding the id drops it. A recipe's
//! retained children are exactly the nested recipe ids of its
//! `EvaluateSubgroup` terms: a held parent keeps every nested subgroup
//! valid, and [`IntersectionInputRef::for_each_operand`] reaches every
//! node operand however deep the nesting.

use std::sync::Arc;

use super::semantic_context::SemanticContextId;
use super::SemanticNodeId;
use crate::semantic_query_memo::intern_table::{intern_domain, Interned};

/// Closed intersection purpose. Production has one mapping; callers cannot
/// supply a contradictory version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IntersectionPurpose {
    /// Checker intersection reduction under the context-selected policy.
    #[default]
    CheckerReduction,
}

/// Owning interned recipe identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IntersectionInputId(Interned<IntersectionRecipe>);

impl IntersectionInputId {
    /// The interned recipe.
    #[must_use]
    pub fn recipe(&self) -> &IntersectionRecipe {
        self.0.value()
    }
}

intern_domain!(IntersectionRecipe);

/// Compact input reference for `ReduceIntersection`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntersectionInputRef {
    Empty,
    Unary(SemanticNodeId),
    Binary(SemanticNodeId, SemanticNodeId),
    Recipe(IntersectionInputId),
}

/// One interned recipe.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntersectionRecipe {
    OrderedOperands(Arc<[SemanticNodeId]>),
    OrderedSteps(Arc<[IntersectionTerm]>),
}

/// One term of an ordered evaluation tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntersectionTerm {
    Value(SemanticNodeId),
    EvaluateSubgroup {
        input: IntersectionInputRef,
        purpose: IntersectionPurpose,
    },
}

impl IntersectionInputRef {
    /// Binary wrapper: no recipe allocation.
    #[must_use]
    pub const fn binary(left: SemanticNodeId, right: SemanticNodeId) -> Self {
        Self::Binary(left, right)
    }

    /// Unary wrapper: no recipe allocation.
    #[must_use]
    pub const fn unary(only: SemanticNodeId) -> Self {
        Self::Unary(only)
    }

    /// Normalize a flat operand list under proven rewrite laws.
    /// A two-term list with no subgroup becomes Binary.
    #[must_use]
    pub fn from_operands(members: &[SemanticNodeId]) -> Self {
        match members {
            [] => Self::Empty,
            [only] => Self::Unary(*only),
            [a, b] => Self::Binary(*a, *b),
            rest => Self::Recipe(IntersectionInputId(Interned::new(
                IntersectionRecipe::OrderedOperands(Arc::from(rest)),
            ))),
        }
    }

    /// Normalize an evaluation tree. A two-term list with a subgroup does
    /// not collapse to Binary.
    #[must_use]
    pub fn from_steps(steps: &[IntersectionTerm]) -> Self {
        if steps
            .iter()
            .any(|step| matches!(step, IntersectionTerm::EvaluateSubgroup { .. }))
        {
            return Self::Recipe(IntersectionInputId(Interned::new(
                IntersectionRecipe::OrderedSteps(Arc::from(steps)),
            )));
        }
        let values: Vec<SemanticNodeId> = steps
            .iter()
            .filter_map(|step| match step {
                IntersectionTerm::Value(id) => Some(*id),
                IntersectionTerm::EvaluateSubgroup { .. } => None,
            })
            .collect();
        Self::from_operands(&values)
    }

    /// Flatten Values for the n-ary kernel. Subgroups stay nested.
    #[must_use]
    pub fn as_ordered_values(&self) -> Vec<SemanticNodeId> {
        match self {
            Self::Empty => Vec::new(),
            Self::Unary(id) => vec![*id],
            Self::Binary(a, b) => vec![*a, *b],
            Self::Recipe(id) => match id.recipe() {
                IntersectionRecipe::OrderedOperands(ops) => ops.to_vec(),
                IntersectionRecipe::OrderedSteps(steps) => steps
                    .iter()
                    .filter_map(|step| match step {
                        IntersectionTerm::Value(id) => Some(*id),
                        IntersectionTerm::EvaluateSubgroup { .. } => None,
                    })
                    .collect(),
            },
        }
    }

    /// Nested evaluation terms, if this input is a tree rather than a flat list.
    #[must_use]
    pub fn as_steps(&self) -> Option<&[IntersectionTerm]> {
        match self {
            Self::Recipe(id) => match id.recipe() {
                IntersectionRecipe::OrderedSteps(steps) => Some(steps),
                IntersectionRecipe::OrderedOperands(_) => None,
            },
            Self::Empty | Self::Unary(_) | Self::Binary(..) => None,
        }
    }

    /// True when this input allocated a recipe.
    #[must_use]
    pub const fn is_recipe(&self) -> bool {
        matches!(self, Self::Recipe(_))
    }

    /// Visit every node operand this input names, descending into every
    /// nested subgroup recipe, in authored order.
    pub fn for_each_operand(&self, visit: &mut impl FnMut(SemanticNodeId)) {
        match self {
            Self::Empty => {}
            Self::Unary(id) => visit(*id),
            Self::Binary(left, right) => {
                visit(*left);
                visit(*right);
            }
            Self::Recipe(id) => match id.recipe() {
                IntersectionRecipe::OrderedOperands(operands) => {
                    operands.iter().copied().for_each(&mut *visit);
                }
                IntersectionRecipe::OrderedSteps(steps) => {
                    for step in steps.iter() {
                        match step {
                            IntersectionTerm::Value(id) => visit(*id),
                            IntersectionTerm::EvaluateSubgroup { input, .. } => {
                                input.for_each_operand(visit);
                            }
                        }
                    }
                }
            },
        }
    }
}

/// Production default context for reducers that do not yet thread a live one.
#[must_use]
pub fn production_semantic_context() -> SemanticContextId {
    SemanticContextId::production()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resident(recipe: &IntersectionRecipe) -> bool {
        use crate::semantic_query_memo::intern_table::InternDomain;
        IntersectionRecipe::index().get(recipe).is_some()
    }

    fn operands(input: &IntersectionInputRef) -> Vec<SemanticNodeId> {
        let mut seen = Vec::new();
        input.for_each_operand(&mut |id| seen.push(id));
        seen
    }

    /// Node ids no other test mints, so residency probes stay exact while
    /// tests run concurrently in one process.
    fn ids(base: u64, n: u64) -> Vec<SemanticNodeId> {
        (0..n)
            .map(|i| SemanticNodeId(0x51C0_0000_0000_0000 | base << 16 | i))
            .collect()
    }

    #[test]
    fn a_held_parent_keeps_its_nested_recipe_valid_and_reaches_every_operand() {
        let leaf = ids(1, 5);
        let parent = {
            let nested = IntersectionInputRef::from_operands(&leaf[1..4]);
            IntersectionInputRef::from_steps(&[
                IntersectionTerm::Value(leaf[0]),
                IntersectionTerm::EvaluateSubgroup {
                    input: nested,
                    purpose: IntersectionPurpose::CheckerReduction,
                },
                IntersectionTerm::Value(leaf[4]),
            ])
        };
        let nested_recipe = IntersectionRecipe::OrderedOperands(Arc::from(&leaf[1..4]));
        assert!(
            resident(&nested_recipe),
            "the parent owns its subgroup after the minting handle dropped"
        );
        assert_eq!(
            operands(&parent),
            leaf,
            "every nested operand, in authored order"
        );
        let Some([_, IntersectionTerm::EvaluateSubgroup { input, .. }, _]) = parent.as_steps()
        else {
            panic!("three ordered steps");
        };
        assert_eq!(input.as_ordered_values(), leaf[1..4].to_vec());
        let parent_recipe = match &parent {
            IntersectionInputRef::Recipe(id) => id.recipe().clone(),
            other => panic!("{other:?}"),
        };
        drop(parent);
        // The probe value itself holds the child, so release it first.
        assert!(resident(&nested_recipe));
        drop(parent_recipe);
        assert!(
            !resident(&nested_recipe),
            "dropping the parent releases its child"
        );
    }

    #[test]
    fn recipe_churn_leaves_no_record_after_its_owners_drain() {
        let held: Vec<IntersectionInputRef> = (0..2_000u64)
            .map(|i| IntersectionInputRef::from_operands(&ids(0x100 + i, 3)))
            .collect();
        let probes: Vec<IntersectionRecipe> = (0..2_000u64)
            .map(|i| IntersectionRecipe::OrderedOperands(Arc::from(ids(0x100 + i, 3))))
            .collect();
        assert!(probes.iter().all(resident));
        let again = IntersectionInputRef::from_operands(&ids(0x100, 3));
        assert_eq!(again, held[0], "content-equal recipes share one identity");
        drop((held, again));
        assert!(
            !probes.iter().any(resident),
            "every churned recipe is reclaimed once its owners drop"
        );
    }

    #[test]
    fn distinct_recipes_never_compare_equal() {
        let a = IntersectionInputRef::from_operands(&ids(0x9000, 3));
        let b = IntersectionInputRef::from_operands(&ids(0x9001, 3));
        assert_ne!(a, b);
        let c = IntersectionInputRef::from_steps(&[
            IntersectionTerm::Value(SemanticNodeId(1)),
            IntersectionTerm::EvaluateSubgroup {
                input: a.clone(),
                purpose: IntersectionPurpose::CheckerReduction,
            },
        ]);
        let d = IntersectionInputRef::from_steps(&[
            IntersectionTerm::Value(SemanticNodeId(1)),
            IntersectionTerm::EvaluateSubgroup {
                input: b,
                purpose: IntersectionPurpose::CheckerReduction,
            },
        ]);
        assert_ne!(
            c, d,
            "parents differing only in a nested recipe stay distinct"
        );
    }
}
