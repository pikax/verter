//! Compact `ReduceIntersection` inputs: empty / unary / binary / interned recipe.
//!
//! Binary construction allocates no recipe. OrderedOperands of length 0/1/2
//! normalize to Empty/Unary/Binary. A two-term sequence that contains a
//! meaningful subgroup stays a recipe.

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::semantic_context::SemanticContextId;
use super::SemanticNodeId;

/// Closed intersection purpose. Production has one mapping; callers cannot
/// supply a contradictory version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IntersectionPurpose {
    /// Checker intersection reduction under the context-selected policy.
    #[default]
    CheckerReduction,
}

/// Interned recipe identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IntersectionInputId(u32);

impl IntersectionInputId {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

/// Compact input reference for `ReduceIntersection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            rest => Self::Recipe(intern_recipe(IntersectionRecipe::OrderedOperands(
                Arc::from(rest),
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
            return Self::Recipe(intern_recipe(IntersectionRecipe::OrderedSteps(Arc::from(
                steps.to_vec().into_boxed_slice(),
            ))));
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
    pub fn as_ordered_values(self) -> Vec<SemanticNodeId> {
        match self {
            Self::Empty => Vec::new(),
            Self::Unary(id) => vec![id],
            Self::Binary(a, b) => vec![a, b],
            Self::Recipe(id) => match lookup_recipe(id) {
                Some(IntersectionRecipe::OrderedOperands(ops)) => ops.to_vec(),
                Some(IntersectionRecipe::OrderedSteps(steps)) => steps
                    .iter()
                    .filter_map(|step| match step {
                        IntersectionTerm::Value(id) => Some(*id),
                        IntersectionTerm::EvaluateSubgroup { .. } => None,
                    })
                    .collect(),
                None => Vec::new(),
            },
        }
    }

    /// Nested evaluation terms, if this input is a tree rather than a flat list.
    #[must_use]
    pub fn as_steps(self) -> Option<Arc<[IntersectionTerm]>> {
        match self {
            Self::Recipe(id) => match lookup_recipe(id) {
                Some(IntersectionRecipe::OrderedSteps(steps)) => Some(steps),
                _ => None,
            },
            _ => None,
        }
    }

    /// True when this input allocated a recipe.
    #[must_use]
    pub const fn is_recipe(self) -> bool {
        matches!(self, Self::Recipe(_))
    }
}

struct RecipeTable {
    by_hash: FxHashMap<u64, Vec<u32>>,
    items: Vec<IntersectionRecipe>,
}

fn recipe_table() -> &'static Mutex<RecipeTable> {
    static TABLE: OnceLock<Mutex<RecipeTable>> = OnceLock::new();
    TABLE.get_or_init(|| {
        Mutex::new(RecipeTable {
            by_hash: FxHashMap::default(),
            items: Vec::new(),
        })
    })
}

fn intern_recipe(recipe: IntersectionRecipe) -> IntersectionInputId {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    recipe.hash(&mut hasher);
    let hash = hasher.finish();
    let mut guard = recipe_table().lock();
    if let Some(ids) = guard.by_hash.get(&hash) {
        for &id in ids {
            if guard.items[id as usize] == recipe {
                return IntersectionInputId(id);
            }
        }
    }
    let id = u32::try_from(guard.items.len()).expect("intersection recipe intern overflow");
    guard.items.push(recipe);
    guard.by_hash.entry(hash).or_default().push(id);
    IntersectionInputId(id)
}

fn lookup_recipe(id: IntersectionInputId) -> Option<IntersectionRecipe> {
    let guard = recipe_table().lock();
    guard.items.get(id.0 as usize).cloned()
}

/// Production default context for reducers that do not yet thread a live one.
#[must_use]
pub fn production_semantic_context() -> SemanticContextId {
    super::semantic_context::SemanticContext::production().intern()
}
