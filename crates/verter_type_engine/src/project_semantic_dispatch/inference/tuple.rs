//! Tuple inference: what the active session knows of a generic tuple's
//! variadic elements, read by the shared positional plan
//! ([`crate::signature_kernel::tuple_inference_plan`]), and the deposit of
//! the slice the plan assigns one of them. A conditional's `infer`
//! declarations and a call's type parameters take the same plan.

use super::super::dispatch_txn::InferenceOccurrence;
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ArgumentLiteralMode, ConstParamPolicy, InferenceCandidatePriority, PrimitiveKind,
    SemanticNodeData, SemanticNodeId, TupleElement,
};
use crate::signature_kernel::VariadicArityFacts;

/// A variadic element over a type parameter the active session infers.
#[derive(Debug, Clone, Copy)]
pub(in crate::project_semantic_dispatch) struct InferableVariadic {
    /// The inferred declaration (`infer A` or a call's `T`).
    pub(in crate::project_semantic_dispatch) param: SemanticNodeId,
    implied_arity: Option<usize>,
    fixed_constraint_arity: Option<usize>,
    /// What the element stands for when the plan infers nothing to it: its
    /// constraint, else `unknown[]` — the type an uninferred variadic fixes
    /// to.
    pub(in crate::project_semantic_dispatch) fallback: SemanticNodeId,
}

/// Per element of a generic tuple, the inferable variadic it is.
#[derive(Debug, Clone)]
pub(in crate::project_semantic_dispatch) struct VariadicElementFacts {
    pub(in crate::project_semantic_dispatch) elements: Vec<Option<InferableVariadic>>,
}

impl VariadicArityFacts for VariadicElementFacts {
    fn implied_arity(&self, index: usize) -> Option<usize> {
        self.elements.get(index).copied().flatten()?.implied_arity
    }
    fn fixed_constraint_arity(&self, index: usize) -> Option<usize> {
        self.elements
            .get(index)
            .copied()
            .flatten()?
            .fixed_constraint_arity
    }
}

/// What the session records of one inferable variadic, read under the
/// transaction borrow.
struct SessionVariadic {
    param: SemanticNodeId,
    constraint: Option<SemanticNodeId>,
    implied_arity: Option<usize>,
}

/// The occurrence a slice infers at: a speculative slice (a target ending
/// in an optional element) yields to a direct inference
/// (`InferencePriority.SpeculativeTuple`).
pub(in crate::project_semantic_dispatch) fn slice_occurrence(
    occurrence: InferenceOccurrence,
    speculative: bool,
) -> InferenceOccurrence {
    if speculative && occurrence.priority == InferenceCandidatePriority::Argument {
        InferenceOccurrence {
            priority: InferenceCandidatePriority::SpeculativeTuple,
            ..occurrence
        }
    } else {
        occurrence
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The inferable variadic elements of `elements`, a generic tuple:
    /// `None` when no session is active or the session infers none of them.
    pub(in crate::project_semantic_dispatch) fn inferable_variadics(
        &self,
        elements: &[TupleElement],
    ) -> Option<VariadicElementFacts> {
        let graph = self.graph();
        let candidates: Vec<Option<SessionVariadic>> = {
            let txn = self.dispatch_txn.borrow();
            let session = txn.active_session()?;
            elements
                .iter()
                .map(|element| {
                    if !element.rest {
                        return None;
                    }
                    let constraint = match graph.node_data(element.value).as_deref() {
                        Some(
                            SemanticNodeData::Infer { constraint, .. }
                            | SemanticNodeData::TypeParam { constraint, .. },
                        ) => *constraint,
                        _ => return None,
                    }
                    .or_else(|| session.declared_constraint(element.value));
                    session.infers(element.value).then(|| SessionVariadic {
                        param: element.value,
                        constraint,
                        implied_arity: session.implied_arity(element.value),
                    })
                })
                .collect()
        };
        if candidates.iter().all(Option::is_none) {
            return None;
        }
        let unknown_array = graph.intern_node(SemanticNodeData::Array {
            element: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
            readonly: false,
        });
        let elements = candidates
            .into_iter()
            .map(|candidate| {
                candidate.map(
                    |SessionVariadic {
                         param,
                         constraint,
                         implied_arity,
                     }| InferableVariadic {
                        param,
                        implied_arity,
                        fixed_constraint_arity: constraint
                            .and_then(|constraint| self.fixed_tuple_arity(constraint)),
                        fallback: constraint
                            .map(|constraint| {
                                self.settle_through_carriers(constraint)
                                    .unwrap_or(constraint)
                            })
                            .unwrap_or(unknown_array),
                    },
                )
            })
            .collect();
        Some(VariadicElementFacts { elements })
    }

    /// The length of `node` when it is a tuple with no rest or variadic
    /// element (`fixedLength` of a tuple without `ElementFlags.Variable`).
    fn fixed_tuple_arity(&self, node: SemanticNodeId) -> Option<usize> {
        let settled = self.settle_through_carriers(node)?;
        match self.graph().node_data(settled).as_deref() {
            Some(SemanticNodeData::Tuple { elements, .. })
                if elements.iter().all(|element| !element.rest) =>
            {
                Some(elements.len())
            }
            _ => None,
        }
    }

    /// The fixed arity of the type a matching source variadic spreads: its
    /// own length when it is a fixed tuple, else its constraint's — the
    /// positions a generic `...U` stands for, which its one element does
    /// not count. `None` when no fixed tuple bounds it (`U extends
    /// unknown[]`), which no fixed tuple constraint accepts.
    pub(in crate::project_semantic_dispatch) fn spread_fixed_arity(
        &self,
        node: SemanticNodeId,
    ) -> Option<usize> {
        let graph = self.graph();
        let mut node = node;
        // A constraint chain (`U extends V`, `V extends [A, B]`) is short;
        // the bound only guards a cyclic one.
        for _ in 0..8 {
            if let Some(arity) = self.fixed_tuple_arity(node) {
                return Some(arity);
            }
            let declared = match graph.node_data(node).as_deref() {
                Some(
                    SemanticNodeData::Infer { constraint, .. }
                    | SemanticNodeData::TypeParam { constraint, .. },
                ) => *constraint,
                _ => return None,
            };
            node = declared.or_else(|| {
                self.dispatch_txn
                    .borrow()
                    .active_session()
                    .and_then(|session| session.declared_constraint(node))
            })?;
        }
        None
    }

    /// Deposit the slice (a mutable tuple, or the array a single rest
    /// infers) the positional plan assigns an inferable variadic. Under a
    /// call's widening literal mode each element widens as the argument's
    /// element would under the parameter's const policy; the slice itself
    /// stays a tuple, as an array literal in a tuple context does.
    pub(in crate::project_semantic_dispatch) fn relation_deposit_slice(
        &self,
        param_node: SemanticNodeId,
        slice: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let policy = {
            let txn = self.dispatch_txn.borrow();
            txn.active_session()
                .and_then(|session| session.call_const_policy(param_node))
                .filter(|_| txn.call_argument_literal_mode() == Some(ArgumentLiteralMode::Widened))
        };
        let bound = match policy {
            Some(policy) => self.widened_slice(slice, policy),
            None => slice,
        };
        self.relation_deposit_candidate(param_node, bound, occurrence)
    }

    /// `slice` with each element widened under `policy`, its tuple or array
    /// shape kept.
    pub(in crate::project_semantic_dispatch) fn widened_slice(
        &self,
        slice: SemanticNodeId,
        policy: ConstParamPolicy,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let data = graph.node_data(slice);
        match data.as_deref() {
            Some(SemanticNodeData::Tuple { elements, readonly }) => {
                let readonly = *readonly;
                let elements: Vec<TupleElement> = elements
                    .iter()
                    .map(|element| {
                        let mut widened = element.copy();
                        widened.value = self.call_inference_candidate(element.value, policy);
                        widened
                    })
                    .collect();
                drop(data);
                graph.intern_node(SemanticNodeData::Tuple {
                    elements: crate::semantic_query::PayloadArray::from(
                        elements.into_boxed_slice(),
                    ),
                    readonly,
                })
            }
            Some(SemanticNodeData::Array { element, readonly }) => {
                let (element, readonly) = (*element, *readonly);
                drop(data);
                graph.intern_node(SemanticNodeData::Array {
                    element: self.call_inference_candidate(element, policy),
                    readonly,
                })
            }
            _ => slice,
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// Whether a `const` type parameter's contextual type is mutable
    /// array-like — its constraint is assignable to `any[]`
    /// (`isMutableArrayLikeType`) — so an array literal in its const
    /// context is a mutable tuple rather than a readonly one
    /// (`checkArrayLiteral`, `getSpreadArgumentType`): `<const T extends
    /// unknown[]>(x: T)` infers `[1, 2]` from `[1, 2]`.
    pub(in crate::project_semantic_dispatch) fn const_context_is_mutable_array_like(
        &self,
        param_node: SemanticNodeId,
    ) -> bool {
        let graph = self.graph();
        let declared = match graph.node_data(param_node).as_deref() {
            Some(SemanticNodeData::TypeParam { constraint, .. }) => *constraint,
            _ => None,
        };
        let Some(constraint) = declared.or_else(|| {
            self.dispatch_txn
                .borrow()
                .active_session()
                .and_then(|session| session.declared_constraint(param_node))
        }) else {
            return false;
        };
        let any_array = graph.intern_node(SemanticNodeData::Array {
            element: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            readonly: false,
        });
        matches!(
            self.execute_relate_pair(constraint, any_array),
            super::super::dispatch_txn::RelationStep::Assignable { .. }
        )
    }

    /// `candidate`, a `const` parameter's whole-argument tuple, as its
    /// contextual type makes it: mutable in a mutable array-like context.
    pub(in crate::project_semantic_dispatch) fn const_tuple_in_context(
        &self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let elements = match graph.node_data(candidate).as_deref() {
            Some(SemanticNodeData::Tuple {
                elements,
                readonly: true,
            }) => elements.iter().map(TupleElement::copy).collect::<Vec<_>>(),
            _ => return candidate,
        };
        if !self.const_context_is_mutable_array_like(param_node) {
            return candidate;
        }
        graph.intern_preserving_scope(
            candidate,
            SemanticNodeData::Tuple {
                elements: crate::semantic_query::PayloadArray::from(elements.into_boxed_slice()),
                readonly: false,
            },
        )
    }
}
