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
    Middle(Arc<[SemanticNodeId]>),
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

/// How many contextual tuples, and how many middle ranges, one dispatcher
/// keeps at most; reading one more starts both afresh.
const TUPLE_CONTEXTS_CAPACITY: usize = 4096;

/// The contextual tuples one dispatcher has read array literal elements
/// against, by tuple node, and the element types of the middle ranges of
/// them it has read, by tuple node and range. Each is derived from its
/// tuple node's payload alone, and a node id is never reused, so an entry
/// answers for its node for as long as that node is read. Neither map holds
/// more than [`TUPLE_CONTEXTS_CAPACITY`] entries.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct TupleContexts {
    tuples: FxHashMap<SemanticNodeId, ContextualTuple>,
    middles: FxHashMap<(SemanticNodeId, usize, usize), Arc<[SemanticNodeId]>>,
}

/// The element type of a tuple's rest element `rest`: the element type of
/// its array type. `None` when it is no array.
fn tuple_rest_element(nodes: &impl NodeRead, rest: SemanticNodeId) -> Option<SemanticNodeId> {
    match nodes.node_data(rest).as_deref() {
        Some(SemanticNodeData::Array { element, .. }) => Some(*element),
        _ => None,
    }
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
        let mut contexts = self.tuple_contexts.borrow_mut();
        if contexts.tuples.len() >= TUPLE_CONTEXTS_CAPACITY {
            contexts.tuples.clear();
        }
        contexts.tuples.insert(node, tuple.clone());
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
        if middle.is_empty() {
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
    ) -> Arc<[SemanticNodeId]> {
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
        let mut known = self.tuple_contexts.borrow_mut();
        if known.middles.len() >= TUPLE_CONTEXTS_CAPACITY {
            known.middles.clear();
        }
        known.middles.insert(key, Arc::clone(&contexts));
        contexts
    }
}
