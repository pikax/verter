//! The contextual tuples array literal elements take their contextual
//! types from (the tuple arm of `getContextualTypeForElementExpression`):
//! each tuple's shape, and the element types of each middle range its
//! elements read, computed once per tuple (and range) for the dispatcher's
//! lifetime, as the checker keeps a tuple's fixed length on its target.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::call_resolve::ArrayLiteralShape;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{NodeRead, SemanticNodeData, SemanticNodeId, TupleElement};

/// One element of a contextual tuple: its type, and whether it is a rest
/// element (whose type is an array of the elements it stands for).
#[derive(Clone, Copy)]
struct TupleSlot {
    value: SemanticNodeId,
    rest: bool,
}

/// A contextual tuple, its elements bound to its shape: the count of its
/// leading fixed elements, and the count of its trailing fixed elements
/// after its last rest element (none without a rest element).
#[derive(Clone)]
pub(in crate::project_semantic_dispatch) struct ContextualTuple {
    node: SemanticNodeId,
    elements: Arc<[TupleSlot]>,
    fixed: usize,
    fixed_end: usize,
}

impl ContextualTuple {
    fn new(node: SemanticNodeId, elements: &[TupleElement]) -> Self {
        let elements: Arc<[TupleSlot]> = elements
            .iter()
            .map(|element| TupleSlot {
                value: element.value,
                rest: element.rest,
            })
            .collect();
        let fixed = elements
            .iter()
            .take_while(|element| {
                super::call_resolve::record_tuple_context_work(1);
                !element.rest
            })
            .count();
        let fixed_end = if fixed < elements.len() {
            elements
                .iter()
                .rev()
                .take_while(|element| {
                    super::call_resolve::record_tuple_context_work(1);
                    !element.rest
                })
                .count()
        } else {
            0
        };
        Self {
            node,
            elements,
            fixed,
            fixed_end,
        }
    }
}

/// What a contextual tuple gives one array literal element.
#[derive(Clone)]
pub(in crate::project_semantic_dispatch) enum TupleElementContext {
    /// No element of the tuple types the position.
    None,
    /// The tuple's element at the position, counted from its start or
    /// from its end.
    Element(SemanticNodeId),
    /// The element types of a middle range of the tuple — its rest
    /// element's, and those of the fixed elements the literal cannot
    /// place — shared by every literal element that reads the same range.
    Middle(Arc<TupleMiddle>),
}

impl TupleElementContext {
    /// Whether both answers are the same contextual types.
    pub(in crate::project_semantic_dispatch) fn same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Element(left), Self::Element(right)) => left == right,
            (Self::Middle(left), Self::Middle(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

/// The element types of one middle range of a contextual tuple, and their
/// union under the nullability policy that asked for it last.
pub(in crate::project_semantic_dispatch) struct TupleMiddle {
    contexts: Arc<[SemanticNodeId]>,
    union: parking_lot::Mutex<
        Option<(
            verter_session_query::flow::policy::NullabilityPolicy,
            SemanticNodeId,
        )>,
    >,
}

impl TupleMiddle {
    /// The element types of the range.
    pub(in crate::project_semantic_dispatch) fn contexts(&self) -> &[SemanticNodeId] {
        &self.contexts
    }

    /// The union of the range's element types under `nullability`, built
    /// by `union` once per policy in a row.
    pub(in crate::project_semantic_dispatch) fn union_under(
        &self,
        nullability: verter_session_query::flow::policy::NullabilityPolicy,
        union: impl FnOnce(&[SemanticNodeId]) -> SemanticNodeId,
    ) -> SemanticNodeId {
        if let Some((policy, node)) = *self.union.lock() {
            if policy == nullability {
                return node;
            }
        }
        let node = union(&self.contexts);
        *self.union.lock() = Some((nullability, node));
        node
    }
}

/// The contextual tuples one dispatcher has read array literal elements
/// against, by tuple node, and the middle ranges of them it has read, by
/// tuple node and range.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct TupleContexts {
    tuples: FxHashMap<SemanticNodeId, ContextualTuple>,
    middles: FxHashMap<(SemanticNodeId, usize, usize), Arc<TupleMiddle>>,
}

/// The element type of a tuple's rest element `rest`: the element type of
/// its array type, through transparent aliases. `None` when it is not a
/// settled array (an open carrier, a generic).
pub(in crate::project_semantic_dispatch) fn tuple_rest_element(
    nodes: &impl NodeRead,
    rest: SemanticNodeId,
) -> Option<SemanticNodeId> {
    let mut current = rest;
    // bounded-loop: at most 8 transparent Alias hops.
    for _ in 0..8 {
        match nodes.node_data(current).as_deref() {
            Some(SemanticNodeData::Alias(target)) => current = *target,
            Some(SemanticNodeData::Array { element, .. }) => return Some(*element),
            _ => return None,
        }
    }
    None
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The contextual tuple `node`, whose elements are `elements`; its
    /// shape is read once per dispatcher.
    pub(in crate::project_semantic_dispatch) fn contextual_tuple(
        &self,
        node: SemanticNodeId,
        elements: &[TupleElement],
    ) -> ContextualTuple {
        if let Some(tuple) = self.tuple_contexts.borrow().tuples.get(&node) {
            return tuple.clone();
        }
        let tuple = ContextualTuple::new(node, elements);
        self.tuple_contexts
            .borrow_mut()
            .tuples
            .insert(node, tuple.clone());
        tuple
    }

    /// What `tuple` gives the array literal element at `index` of a literal
    /// shaped `shape`: the tuple's own fixed element while no spread came
    /// before it; counted from the end, the tuple's trailing fixed element
    /// at the same distance when the literal's length is known and no
    /// spread follows the element; otherwise the element types of the
    /// tuple's middle — its rest element's, and its trailing fixed
    /// elements' unless the literal's length places the element before
    /// them.
    pub(in crate::project_semantic_dispatch) fn tuple_element_context(
        &self,
        tuple: &ContextualTuple,
        index: usize,
        shape: ArrayLiteralShape,
    ) -> TupleElementContext {
        super::call_resolve::record_tuple_context_work(1);
        if shape.first_spread.is_none_or(|spread| index < spread) && index < tuple.fixed {
            return TupleElementContext::Element(tuple.elements[index].value);
        }
        let offset = match shape.length {
            Some(length) if shape.last_spread.is_none_or(|spread| index > spread) => {
                length.saturating_sub(index)
            }
            _ => 0,
        };
        let fixed_end = if offset > 0 { tuple.fixed_end } else { 0 };
        if offset > 0 && offset <= fixed_end {
            return TupleElementContext::Element(
                tuple.elements[tuple.elements.len() - offset].value,
            );
        }
        let start = shape
            .first_spread
            .map_or(tuple.fixed, |spread| spread.min(tuple.fixed));
        let end = tuple.elements.len() - if shape.length.is_some() { fixed_end } else { 0 };
        if start >= end {
            return TupleElementContext::None;
        }
        let key = (tuple.node, start, end);
        let known = self.tuple_contexts.borrow().middles.get(&key).cloned();
        let middle = known.unwrap_or_else(|| self.tuple_middle(tuple, key));
        if middle.contexts.is_empty() {
            TupleElementContext::None
        } else {
            TupleElementContext::Middle(middle)
        }
    }

    /// The element types of the middle range `key` names of `tuple`,
    /// read once per dispatcher.
    fn tuple_middle(
        &self,
        tuple: &ContextualTuple,
        key: (SemanticNodeId, usize, usize),
    ) -> Arc<TupleMiddle> {
        let (_, start, end) = key;
        let graph = self.graph();
        let contexts: Arc<[SemanticNodeId]> = tuple.elements[start..end]
            .iter()
            .filter_map(|element| {
                super::call_resolve::record_tuple_context_work(1);
                if element.rest {
                    tuple_rest_element(&graph, element.value)
                } else {
                    Some(element.value)
                }
            })
            .collect();
        let middle = Arc::new(TupleMiddle {
            contexts,
            union: parking_lot::Mutex::new(None),
        });
        self.tuple_contexts
            .borrow_mut()
            .middles
            .insert(key, Arc::clone(&middle));
        middle
    }
}
