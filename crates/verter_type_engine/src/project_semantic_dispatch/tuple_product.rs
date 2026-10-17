//! The tuple-product constructor: the one place a tuple's variadic elements
//! splice into it. Every tuple intern site that can receive substituted,
//! inferred or flow-evaluated elements normalizes through
//! [`ProjectSemanticDispatch::normalize_tuple_spread`] (a tuple type) or
//! [`ProjectSemanticDispatch::normalize_expression_tuple_spread`] (a tuple an
//! expression produces), so the size of what a splice builds is checked here
//! and nowhere else.
//!
//! A splice is weighed before anything is copied. A first pass reads the
//! element list and every tuple it splices, nested ones included, and sizes
//! each spliced tuple as the checker sizes it — its own splices checked
//! against its own prefix, a tuple met again sized once. Each splice is
//! checked against the checker's tuple-size rule
//! ([`checker_policy::tuple_splice`]): at the limit the whole tuple is the
//! checker's TS2799 recovery (TS2800 in an expression), a resource partial
//! holding the authored tuple as its origin, and nothing is copied. Under
//! it, the elements read and the elements the product will hold are
//! charged to the connected-work ledger, and the product's construction
//! bytes reserved, before the product is built. A refused charge keeps the
//! authored element list as it is — every spread still in place, never a
//! truncated tuple — and folds the ledger's trip into the build, so the
//! answer is a typed partial.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::NormalizedTupleShape;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::checker_policy::{self, OperationRefusal, TupleSplicePosition};
use crate::semantic_query::{PartialReasonSet, SemanticNodeData, SemanticNodeId, TupleElement};

/// What splicing an element list produced.
enum Spliced {
    /// The expanded elements.
    Elements(Vec<TupleElement>),
    /// A splice reached the checker's tuple-size limit.
    Refused(OperationRefusal),
    /// The connected-work ledger refused to pay for the product.
    Unpaid(PartialReasonSet),
}

/// What splicing a tuple an array literal spreads may do
/// ([`ProjectSemanticDispatch::admit_expression_tuple_splice`]).
pub(in crate::project_semantic_dispatch) enum ExpressionSplice {
    /// Copy the source tuple's elements in: weighed and paid for.
    Copy,
    /// The splice reached the checker's tuple-size limit: the literal is
    /// this TS2800 recovery, a resource partial already folded.
    Refused(SemanticNodeId),
    /// The connected-work ledger refused to pay: keep the spread as a rest
    /// element. The trip is already folded.
    Unpaid,
}

/// A spliced tuple: its node and its elements.
type SpliceTarget = (SemanticNodeId, Arc<[TupleElement]>);

/// The weighed splice of an element list.
struct SplicePlan {
    /// The tuple each spread value that splices settles on.
    targets: FxHashMap<SemanticNodeId, SpliceTarget>,
    /// The elements the product holds.
    len: usize,
    /// The elements read to size it.
    read: usize,
}

/// One element list the sizing pass is reading: its elements, the next
/// one, the tuple it is (none for the outermost) and the elements it has
/// expanded to so far.
struct SizingLevel<'e> {
    elements: LevelElements<'e>,
    next: usize,
    tuple: Option<SemanticNodeId>,
    len: usize,
}

/// The elements of a [`SizingLevel`]: the caller's list, or a spliced
/// tuple's.
enum LevelElements<'e> {
    Outer(&'e [TupleElement]),
    Inner(Arc<[TupleElement]>),
}

impl LevelElements<'_> {
    /// The `rest` flag and value of the element at `index`.
    fn get(&self, index: usize) -> Option<(bool, SemanticNodeId)> {
        let elements: &[TupleElement] = match self {
            Self::Outer(elements) => elements,
            Self::Inner(elements) => elements,
        };
        elements
            .get(index)
            .map(|element| (element.rest, element.value))
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Spread-normalise a tuple TYPE's element list (the shared
    /// normalize-on-intern rule for every tuple intern site that can
    /// receive substituted / signature-derived elements):
    ///
    /// 1. **Variadic splice** — a `rest: true` element whose value is a
    ///    settled `Tuple` (through transparent aliases) splices that
    ///    tuple's elements in place, preserving the inner labels /
    ///    optional / rest markers. This is what makes a userland
    ///    `Concat<A, B> = [...A, ...B]` concatenate once `A` / `B`
    ///    substitute to concrete tuples — no utility name special-casing.
    ///    Splicing reaches nested `[...[...T]]` shapes at any depth. A
    ///    splice reaching the checker's tuple-size limit makes the whole
    ///    tuple the checker's TS2799 recovery; a product the connected
    ///    demand cannot pay for keeps the authored elements, as a partial
    ///    (see the module documentation).
    /// 2. **Non-trailing optional reconciliation** — an `optional` marker
    ///    followed anywhere later by a REQUIRED (non-optional, non-rest)
    ///    element converts to a REQUIRED `T | undefined` slot.
    ///    Optional-before-required is unrepresentable TS (TS1257); the
    ///    pinned tsgo materialises `[...[a?: number], string]` as
    ///    `[number | undefined, string]` (length 2). A trailing optional
    ///    run — including one followed only by a rest tail, which IS
    ///    legal (`[a?: number, ...boolean[]]`) — keeps its `?`.
    /// 3. **Sole-rest collapse** — a tuple consisting SOLELY of one
    ///    rest-of-array element IS that array (`[...E[]]` ≡ `E[]`,
    ///    the `(...args: E[])` parameters surface). The collapsed
    ///    array's `readonly` mirrors the OUTER tuple (`readonly`
    ///    parameter — pinned tsgo: `readonly [...number[]]` ≡
    ///    `readonly number[]`; `[...(readonly number[])]` ≡ MUTABLE
    ///    `number[]`), never the inner array's flag.
    ///
    /// A rest element whose value is open / unresolved (a generic, a
    /// carrier, anything not a settled tuple/array) is preserved
    /// verbatim — normalization never forces materialisation.
    pub fn normalize_tuple_spread(
        &self,
        elements: &[TupleElement],
        readonly: bool,
    ) -> NormalizedTupleShape {
        self.normalize_tuple_spread_at(elements, readonly, TupleSplicePosition::Type)
    }

    /// [`Self::normalize_tuple_spread`] for the tuple an expression
    /// produces (an array literal evaluated as a tuple): its refused splice
    /// is the checker's TS2800.
    pub(in crate::project_semantic_dispatch) fn normalize_expression_tuple_spread(
        &self,
        elements: &[TupleElement],
        readonly: bool,
    ) -> NormalizedTupleShape {
        self.normalize_tuple_spread_at(elements, readonly, TupleSplicePosition::Expression)
    }

    /// Weigh splicing the `incoming` elements of a tuple an array literal
    /// spreads after the `prefix` elements its tuple holds so far, and pay
    /// for copying them — into the literal's tuple and its unwidened view —
    /// before one is copied. The literal splices its sources itself (an
    /// optional element reads `| undefined` there), so this is the same
    /// check [`Self::normalize_expression_tuple_spread`] applies, made
    /// before the literal's copy.
    pub(in crate::project_semantic_dispatch) fn admit_expression_tuple_splice(
        &self,
        prefix: usize,
        incoming: usize,
    ) -> ExpressionSplice {
        if let Err(refusal) =
            checker_policy::tuple_splice(prefix, incoming, TupleSplicePosition::Expression)
        {
            return ExpressionSplice::Refused(self.recover_at_operation_budget(refusal, None));
        }
        if self.connected_demand.is_active() {
            let paid = self.connected_demand.charge_units(incoming).and_then(|()| {
                self.connected_demand
                    .reserve_bytes(incoming.saturating_mul(2 * std::mem::size_of::<TupleElement>()))
            });
            if let Err(reasons) = paid {
                self.fold_local_partial_completeness(reasons);
                return ExpressionSplice::Unpaid;
            }
        }
        ExpressionSplice::Copy
    }

    fn normalize_tuple_spread_at(
        &self,
        elements: &[TupleElement],
        readonly: bool,
        position: TupleSplicePosition,
    ) -> NormalizedTupleShape {
        let mut out = match self.splice_tuple_elements(elements, position) {
            Spliced::Elements(out) => out,
            Spliced::Refused(refusal) => {
                let authored = self.graph().intern_node(SemanticNodeData::Tuple {
                    elements: crate::semantic_query::PayloadArray::from(
                        elements
                            .iter()
                            .map(TupleElement::copy)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    readonly,
                });
                return NormalizedTupleShape::Array(
                    self.recover_at_operation_budget(refusal, Some(authored)),
                );
            }
            Spliced::Unpaid(reasons) => {
                self.fold_local_partial_completeness(reasons);
                return NormalizedTupleShape::Tuple(
                    elements.iter().map(TupleElement::copy).collect(),
                );
            }
        };
        // Non-trailing optional reconciliation (reverse scan: a slot's
        // conversion depends only on LATER elements; a converted slot is
        // itself required and forces conversion of earlier optionals).
        let mut required_follows = false;
        for index in (0..out.len()).rev() {
            if out[index].optional && required_follows {
                let undefined_node = self.graph().intern_node(SemanticNodeData::Primitive(
                    crate::semantic_query::PrimitiveKind::Undefined,
                ));
                let widened = self.intern_normalized_union_or_intersection(
                    &[out[index].value, undefined_node],
                    /* is_union */ true,
                );
                out[index].value = widened;
                out[index].optional = false;
            }
            if !out[index].optional && !out[index].rest {
                required_follows = true;
            }
        }
        if out.len() == 1 && out[0].rest {
            if let Some(array_node) = self.settled_node_through_aliases(out[0].value, |data| {
                matches!(data, SemanticNodeData::Array { .. })
            }) {
                if let Some(SemanticNodeData::Array {
                    element,
                    readonly: inner_readonly,
                }) = self.graph().node_data(array_node).as_deref()
                {
                    // The collapsed array's readonly mirrors the OUTER
                    // tuple; reuse the inner node only when the flags
                    // already agree. The replacement intern preserves the
                    // inner node's origin scope so `ProjectPath`
                    // self-rooting over the collapsed base still records
                    // the origin file's `(canonical, whole_hash)` root.
                    if *inner_readonly == readonly {
                        return NormalizedTupleShape::Array(array_node);
                    }
                    let element = *element;
                    return NormalizedTupleShape::Array(self.graph().intern_preserving_scope(
                        array_node,
                        SemanticNodeData::Array { element, readonly },
                    ));
                }
            }
        }
        NormalizedTupleShape::Tuple(out)
    }

    /// Splice every rest element of `elements` whose value settles on a
    /// tuple, weighed and paid for before the product is built.
    fn splice_tuple_elements(
        &self,
        elements: &[TupleElement],
        position: TupleSplicePosition,
    ) -> Spliced {
        let verbatim = || Spliced::Elements(elements.iter().map(TupleElement::copy).collect());
        if !elements.iter().any(|element| element.rest) {
            return verbatim();
        }
        let plan = match self.plan_tuple_splice(elements, position) {
            Ok(plan) => plan,
            Err(refusal) => return Spliced::Refused(refusal),
        };
        if plan.targets.is_empty() {
            return verbatim();
        }
        // The connected ledger only exists inside a demand; a splice built
        // outside one (a fixture's direct call) is weighed, not charged.
        if self.connected_demand.is_active() {
            let paid = self
                .connected_demand
                .charge_units(plan.read.saturating_add(plan.len))
                .and_then(|()| {
                    self.connected_demand
                        .reserve_bytes(plan.len.saturating_mul(std::mem::size_of::<TupleElement>()))
                });
            if let Err(reasons) = paid {
                return Spliced::Unpaid(reasons);
            }
        }
        let mut out = Vec::with_capacity(plan.len);
        self.copy_spliced_elements(elements, &plan.targets, &mut out);
        Spliced::Elements(out)
    }

    /// The tuple a spread of `value` splices, when `value` settles (through
    /// transparent aliases) on one.
    fn splice_target(&self, value: SemanticNodeId) -> Option<SpliceTarget> {
        let tuple = self.settled_node_through_aliases(value, |data| {
            matches!(data, SemanticNodeData::Tuple { .. })
        })?;
        match self.graph().node_data(tuple).as_deref() {
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                Some((tuple, Arc::clone(elements.as_arc())))
            }
            _ => None,
        }
    }

    /// Weigh splicing `elements`: read every tuple it splices, nested ones
    /// included, from a work list, sizing each as the checker sizes it — its
    /// splices checked against its own prefix, a tuple met again sized once
    /// — and check each splice against the checker's tuple-size rule. A
    /// tuple spread into itself (a cycle) keeps its rest element and counts
    /// as one element. Nothing is copied.
    fn plan_tuple_splice(
        &self,
        elements: &[TupleElement],
        position: TupleSplicePosition,
    ) -> Result<SplicePlan, OperationRefusal> {
        let mut targets: FxHashMap<SemanticNodeId, SpliceTarget> = FxHashMap::default();
        let mut sized: FxHashMap<SemanticNodeId, usize> = FxHashMap::default();
        let mut splicing: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut read = 0usize;
        let mut levels = vec![SizingLevel {
            elements: LevelElements::Outer(elements),
            next: 0,
            tuple: None,
            len: 0,
        }];
        loop {
            let Some(level) = levels.last_mut() else {
                unreachable!("the outermost level returns the plan");
            };
            let Some((rest, value)) = level.elements.get(level.next) else {
                let Some(done) = levels.pop() else {
                    unreachable!("a level was read");
                };
                let Some(tuple) = done.tuple else {
                    return Ok(SplicePlan {
                        targets,
                        len: done.len,
                        read,
                    });
                };
                splicing.remove(&tuple);
                sized.insert(tuple, done.len);
                let Some(parent) = levels.last_mut() else {
                    unreachable!("a spliced tuple has the level that spread it");
                };
                parent.len = checker_policy::tuple_splice(parent.len, done.len, position)?;
                continue;
            };
            level.next += 1;
            read += 1;
            if rest {
                let target = match targets.get(&value) {
                    Some((tuple, inner)) => Some((*tuple, Arc::clone(inner))),
                    None => self.splice_target(value).inspect(|(tuple, inner)| {
                        targets.insert(value, (*tuple, Arc::clone(inner)));
                    }),
                };
                if let Some((tuple, inner)) = target.filter(|(tuple, _)| !splicing.contains(tuple))
                {
                    if let Some(&len) = sized.get(&tuple) {
                        level.len = checker_policy::tuple_splice(level.len, len, position)?;
                    } else {
                        splicing.insert(tuple);
                        levels.push(SizingLevel {
                            elements: LevelElements::Inner(inner),
                            next: 0,
                            tuple: Some(tuple),
                            len: 0,
                        });
                    }
                    continue;
                }
            }
            level.len = level.len.saturating_add(1);
        }
    }

    /// Copy `elements` into `out` with every spread [`Self::plan_tuple_splice`]
    /// weighed replaced by its tuple's elements, spliced in turn, so
    /// `[...[...[1, 2]]]` is `[1, 2]` at any nesting. The tuples being
    /// spliced are read from a work list; a tuple spread into itself keeps
    /// its rest element as it is.
    fn copy_spliced_elements(
        &self,
        elements: &[TupleElement],
        targets: &FxHashMap<SemanticNodeId, SpliceTarget>,
        out: &mut Vec<TupleElement>,
    ) {
        // Each level: the elements it splices, the next one, and the tuple
        // it came from (none for the outermost).
        let mut levels: Vec<(LevelElements<'_>, usize, Option<SemanticNodeId>)> =
            vec![(LevelElements::Outer(elements), 0, None)];
        let mut splicing: FxHashSet<SemanticNodeId> = FxHashSet::default();
        while let Some((level, next, _)) = levels.last_mut() {
            let element = match level {
                LevelElements::Outer(elements) => elements.get(*next),
                LevelElements::Inner(elements) => elements.get(*next),
            }
            .map(TupleElement::copy);
            let Some(element) = element else {
                if let Some((_, _, Some(tuple))) = levels.pop() {
                    splicing.remove(&tuple);
                }
                continue;
            };
            *next += 1;
            if element.rest {
                if let Some((tuple, inner)) = targets
                    .get(&element.value)
                    .filter(|(tuple, _)| !splicing.contains(tuple))
                {
                    splicing.insert(*tuple);
                    levels.push((LevelElements::Inner(Arc::clone(inner)), 0, Some(*tuple)));
                    continue;
                }
            }
            out.push(element);
        }
    }
}
