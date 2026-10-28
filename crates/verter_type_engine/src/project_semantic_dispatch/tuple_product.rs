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
//! checked against the checker's tuple-size rule ([`splice_fits`]); the
//! pass stops reading as soon as a refusal is certain, and charges the
//! elements it reads to the connected-work ledger in batches as it goes, so
//! a demand that runs out (or is cancelled) stops the pass and a refusal
//! keeps the work that decided it. At the limit the whole tuple is the
//! checker's recovery — TS2799, or TS2800 while an expression is checked
//! ([`CheckingSite`]). An existing inner tuple is retained as its origin;
//! an outer authored origin is copied only if work and storage admit it.
//! No expanded product is copied on a size refusal. Under the limit, the
//! product's work and storage are admitted before construction. Planning
//! and copying consume credit before each step and commit the consumed
//! prefix on every exit, including cancellation. An unpaid construction
//! folds the ledger's trip and returns a typed incomplete node, without
//! copying the authored list or representing a truncated tuple as complete.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::NormalizedTupleShape;
use crate::project_semantic_dispatch::connected_demand::ConnectedDemandLedger;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::request_ports::CancellationCheckpoint;
use crate::semantic_query::checker_policy::{self, OperationRefusal, TupleSplicePosition};
use crate::semantic_query::{PartialReasonSet, SemanticNodeData, SemanticNodeId, TupleElement};

/// The checker's tuple-size limit (`createNormalizedTupleType`): splicing
/// a tuple of `n` elements after the `prefix` elements the tuple being
/// built has expanded so far fails once `prefix + n` reaches this, before
/// an element is copied. Only a splice is checked: a directly authored
/// tuple, and the elements written after the last splice, are never
/// counted, so a tuple may end up longer than the limit.
///
/// Measured on TypeScript 7.0.2 (`tsc --strict`), with `A<n>` an authored
/// tuple of `n` elements: `[...A9999]`, `[1, ...A9998]` and `[...A5000,
/// ...A4999]` build; `[...A10000]`, `[1, ...A9999]` and `[...A5000,
/// ...A5000]` are TS2799 and `any`; `[...A9999, 1]` (10,000 elements) and
/// an authored 10,001-element tuple build; `[...a, ...b] as const` over
/// values of 5,000 elements each is TS2800 and `any`, and so is a call
/// `f(a)` of `f<T extends unknown[]>(a: T): [...T, ...T]` over 5,000: the
/// checker picks the code by the node it is checking when it builds the
/// tuple, not by where the tuple is written. An enclosing tuple spreading
/// the refused `any` reports nothing more.
pub(crate) const TUPLE_SPLICE_SIZE: usize = 10_000;

/// Whether `incoming` spliced elements fit after the `prefix` elements a
/// tuple has expanded so far ([`TUPLE_SPLICE_SIZE`]).
const fn splice_fits(prefix: usize, incoming: usize) -> bool {
    match prefix.checked_add(incoming) {
        Some(expanded) => expanded < TUPLE_SPLICE_SIZE,
        None => false,
    }
}

/// Elements the sizing pass reads between two charges to the connected
/// ledger: the most it reads past a trip or a cancellation.
const TRAVERSAL_BATCH: usize = 256;

/// An explicit checking-site transition on one dispatcher's demand state.
/// The dispatcher and every suspended frame own the site; no thread-local
/// diagnostic selector participates in tuple construction.
#[must_use = "the site lasts while the guard lives"]
pub(in crate::project_semantic_dispatch) struct CheckingSite<'d> {
    current: &'d verter_execution::sync_cell::SyncCell<TupleSplicePosition>,
    enclosing: TupleSplicePosition,
}

impl Drop for CheckingSite<'_> {
    fn drop(&mut self) {
        self.current.set(self.enclosing);
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    pub(in crate::project_semantic_dispatch) fn checking_site_scope(
        &self,
        site: TupleSplicePosition,
    ) -> CheckingSite<'_> {
        CheckingSite {
            current: &self.checking_site,
            enclosing: self.checking_site.replace(site),
        }
    }
}

/// What splicing an element list produced.
enum Spliced {
    /// The expanded elements.
    Elements(Vec<TupleElement>),
    /// A splice of the list itself reached the checker's tuple-size limit.
    Refused(OperationRefusal),
    /// The connected-work ledger refused to pay for the product, or for
    /// planning it.
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

/// One step of a planned expansion, over the element list it belongs to.
enum SpliceOp {
    /// Keep the elements `start..end` as they are written.
    Keep { start: usize, end: usize },
    /// Splice the expansion of this tuple, planned once.
    Splice(SemanticNodeId),
    /// The element at `index` spreads `tuple`, refused on its own: the
    /// element keeps its place and holds that tuple's recovery.
    Recover { index: usize, tuple: SemanticNodeId },
}

/// A spliced tuple as the plan sized it.
enum PlannedTuple {
    /// Its elements, the steps expanding them and the elements they expand
    /// to.
    Expanded {
        elements: Arc<[TupleElement]>,
        ops: Vec<SpliceOp>,
        len: usize,
    },
    /// A splice of its own reached the limit: it is its own recovery,
    /// one element of whatever spreads it, as the checker builds a tuple
    /// before the tuple spreading it.
    Refused,
}

/// The admitted plan of one element list's splice: its own steps, every
/// tuple it splices sized and expanded once, and the elements the product
/// holds.
struct SplicePlan {
    ops: Vec<SpliceOp>,
    tuples: FxHashMap<SemanticNodeId, PlannedTuple>,
    len: usize,
}

impl SplicePlan {
    /// Every emitted element and every control step visited when executing
    /// the compact plan. Shared subplans execute once; empty expansions
    /// have no copy step at all.
    fn copy_work(&self) -> usize {
        self.tuples.values().fold(
            self.len.saturating_add(self.ops.len()),
            |work, tuple| match tuple {
                PlannedTuple::Expanded { ops, .. } => work.saturating_add(ops.len()),
                PlannedTuple::Refused => work,
            },
        )
    }
}

/// One element list the plan is reading: its elements, the next one, the
/// tuple it is (none for the outermost), the elements it has expanded to so
/// far and its steps.
struct SizingLevel<'e> {
    elements: LevelElements<'e>,
    next: usize,
    tuple: Option<SemanticNodeId>,
    len: usize,
    ops: Vec<SpliceOp>,
}

impl SizingLevel<'_> {
    /// Keep the element at `index` as it is written, extending the run of
    /// kept elements it follows. `Err` when its step's storage is refused.
    fn keep(
        &mut self,
        index: usize,
        credit: &TraversalCredit<'_, '_>,
    ) -> Result<(), PartialReasonSet> {
        self.len += 1;
        if let Some(SpliceOp::Keep { end, .. }) = self.ops.last_mut() {
            if *end == index {
                *end += 1;
                return Ok(());
            }
        }
        self.push(
            SpliceOp::Keep {
                start: index,
                end: index + 1,
            },
            credit,
        )
    }

    /// Append a step, its storage reserved first.
    fn push(
        &mut self,
        op: SpliceOp,
        credit: &TraversalCredit<'_, '_>,
    ) -> Result<(), PartialReasonSet> {
        if self.ops.len() == self.ops.capacity() {
            let additional = self.ops.capacity().max(4);
            credit.reserve(additional.saturating_mul(std::mem::size_of::<SpliceOp>()))?;
            self.ops.reserve_exact(additional);
        }
        self.ops.push(op);
        Ok(())
    }
}

/// The elements of a [`SizingLevel`]: the caller's list, or a spliced
/// tuple's.
enum LevelElements<'e> {
    Outer(&'e [TupleElement]),
    Inner(Arc<[TupleElement]>),
}

impl LevelElements<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Outer(elements) => elements.len(),
            Self::Inner(elements) => elements.len(),
        }
    }

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

/// One compact copy frame: source elements, admitted operations, next
/// operation, and the shared tuple's output start when entering a subplan.
type CopyFrame<'p> = (
    &'p [TupleElement],
    &'p [SpliceOp],
    usize,
    Option<(SemanticNodeId, usize)>,
);

/// Construction bytes planning one more spliced tuple holds before it
/// grows: its plan entry, its resolved spread and its open level.
const PLANNED_TUPLE_BYTES: usize = std::mem::size_of::<(SemanticNodeId, PlannedTuple)>()
    + std::mem::size_of::<(SemanticNodeId, Option<SpliceTarget>)>()
    + std::mem::size_of::<SizingLevel<'static>>()
    + std::mem::size_of::<CopyFrame<'static>>()
    + std::mem::size_of::<(SemanticNodeId, (usize, usize))>()
    + std::mem::size_of::<(SemanticNodeId, SemanticNodeId)>()
    + std::mem::size_of::<SemanticNodeId>();

/// The work the constructor's traversal consumes, one unit before each
/// step it reads — an element of any list it plans, or a transparent alias
/// it passes. The credit is drawn from the connected ledger at most
/// [`TRAVERSAL_BATCH`] units at a time, so a cancellation is observed
/// within a batch, and what it consumed is committed on every exit, a
/// refusal or a trip included: the work recorded is exactly the prefix the
/// traversal read. Outside a demand no work is charged, but a cancelled
/// request still stops the traversal.
struct TraversalCredit<'d, 'a> {
    ledger: Option<&'d ConnectedDemandLedger<'a>>,
    cancellation: CancellationCheckpoint,
    window: usize,
    remaining: usize,
}

impl<'d, 'a> TraversalCredit<'d, 'a> {
    fn open(ledger: &'d ConnectedDemandLedger<'a>, cancellation: CancellationCheckpoint) -> Self {
        Self {
            ledger: ledger.is_active().then_some(ledger),
            cancellation,
            window: 0,
            remaining: 0,
        }
    }

    /// Consume one unit before reading one step.
    #[inline]
    fn step(&mut self) -> Result<(), PartialReasonSet> {
        if self.remaining == 0 {
            self.refresh()?;
            if self.remaining == 0 {
                // Nothing left to draw: one more unit trips the demand.
                return self.ledger.map_or(Ok(()), ConnectedDemandLedger::charge);
            }
        }
        self.remaining -= 1;
        Ok(())
    }

    /// Commit what the window consumed and draw the next one, observing
    /// cancellation.
    fn refresh(&mut self) -> Result<(), PartialReasonSet> {
        self.settle();
        let available = match self.ledger {
            Some(ledger) => ledger.work_available()?,
            None if self.cancellation.is_cancelled() => return Err(PartialReasonSet::CANCELLED),
            None => usize::MAX,
        };
        self.window = available.min(TRAVERSAL_BATCH);
        self.remaining = self.window;
        Ok(())
    }

    /// Commit the units consumed since the last draw.
    fn settle(&mut self) {
        if let Some(ledger) = self.ledger {
            ledger.commit(self.window - self.remaining);
        }
        self.window = 0;
        self.remaining = 0;
    }

    /// Reserve `bytes` of planning storage before it grows.
    fn reserve(&self, bytes: usize) -> Result<(), PartialReasonSet> {
        self.ledger
            .map_or(Ok(()), |ledger| ledger.reserve_bytes(bytes))
    }
}

impl Drop for TraversalCredit<'_, '_> {
    fn drop(&mut self) {
        self.settle();
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    construction_ordinal_seam! {
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
        ///    splice of the list reaching the checker's tuple-size limit makes
        ///    the whole tuple the checker's recovery — TS2799, or TS2800 while
        ///    an expression is checked ([`CheckingSite`]); a spliced tuple
        ///    whose own splice reaches it is that tuple's recovery in its
        ///    place; a product the connected demand cannot pay for returns a
        ///    typed incomplete node (see the module documentation).
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
        fn normalize_tuple_spread(
            &self,
            elements: &[TupleElement],
            readonly: bool,
        ) -> NormalizedTupleShape {
            self.normalize_tuple_spread_at(elements, readonly, self.checking_site.get())
        }
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
        if !splice_fits(prefix, incoming) {
            return ExpressionSplice::Refused(self.refuse_splice(
                checker_policy::tuple_splice_refusal(TupleSplicePosition::Expression),
                None,
            ));
        }
        if self.connected_demand.is_active() {
            let paid = self.connected_demand.charge_units(incoming).and_then(|()| {
                self.connected_demand
                    .reserve_bytes(incoming.saturating_mul(2 * std::mem::size_of::<TupleElement>()))
            });
            if let Err(reasons) = paid {
                self.withhold_from_substitution_memo();
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
                let origin = self.admit_construction(elements.len()).then(|| {
                    self.graph().intern_node(SemanticNodeData::Tuple {
                        elements: crate::semantic_query::PayloadArray::from(
                            elements
                                .iter()
                                .map(TupleElement::copy)
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        ),
                        readonly,
                    })
                });
                return NormalizedTupleShape::Array(self.refuse_splice(refusal, origin));
            }
            Spliced::Unpaid(reasons) => {
                self.withhold_from_substitution_memo();
                self.fold_local_partial_completeness(reasons);
                return NormalizedTupleShape::Array(self.opaque(
                    crate::semantic_query::QueryError::IncompleteSemanticOperand { reasons },
                ));
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
            let mut credit = TraversalCredit::open(&self.connected_demand, self.cancellation);
            let array = self.spread_array(out[0].value, &mut credit);
            drop(credit);
            let array_node = match array {
                Ok(node) => node,
                Err(reasons) => {
                    self.withhold_from_substitution_memo();
                    self.fold_local_partial_completeness(reasons);
                    return NormalizedTupleShape::Array(self.opaque(
                        crate::semantic_query::QueryError::IncompleteSemanticOperand { reasons },
                    ));
                }
            };
            if let Some(array_node) = array_node {
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

    /// The checker's recovery for a refused splice, with `origin` its
    /// authored tuple when one was admitted. The refusal is deposited at
    /// the one canonical-evidence disposition point, which folds it as a
    /// resource partial and withholds every substitution it is part of
    /// from the substitution memo, which admits only complete products.
    fn refuse_splice(
        &self,
        refusal: OperationRefusal,
        origin: Option<SemanticNodeId>,
    ) -> SemanticNodeId {
        let mut evidence =
            crate::project_semantic_dispatch::canonical_algebra::CanonicalEvidence::default();
        evidence.refusal = Some(refusal);
        self.deposit_canonical_evidence(evidence);
        checker_policy::resource_recovery(self.graph(), refusal, origin)
    }

    /// Withhold an unpaid product and every enclosing substitution from
    /// the complete-result memo. Its budget-dependent partiality belongs
    /// exclusively to the enclosing demand's typed refusal mechanism.
    fn withhold_from_substitution_memo(&self) {
        self.canonical_evidence_epoch
            .set(self.canonical_evidence_epoch.get().wrapping_add(1));
    }

    /// Whether the construction bytes of an authored copy of `len`
    /// elements are admitted, and reserved: a refusal's origin is built
    /// only when the demand pays for it, and is left out otherwise.
    fn admit_construction(&self, len: usize) -> bool {
        if !self.connected_demand.is_active() {
            return true;
        }
        let bytes = len.saturating_mul(std::mem::size_of::<TupleElement>());
        bytes <= self.connected_demand.construction_bytes_available()
            && self
                .connected_demand
                .work_available()
                .is_ok_and(|available| available >= len)
            && self.connected_demand.charge_units(len).is_ok()
            && self.connected_demand.reserve_bytes(bytes).is_ok()
    }

    /// Splice every rest element of `elements` whose value settles on a
    /// tuple, planned and paid for before the product is built.
    fn splice_tuple_elements(
        &self,
        elements: &[TupleElement],
        position: TupleSplicePosition,
    ) -> Spliced {
        let plan = {
            let mut credit = TraversalCredit::open(&self.connected_demand, self.cancellation);
            match self.plan_tuple_splice(elements, &mut credit) {
                Ok(plan) => plan,
                Err(None) => {
                    return Spliced::Refused(checker_policy::tuple_splice_refusal(position));
                }
                Err(Some(reasons)) => return Spliced::Unpaid(reasons),
            }
            // The credit commits what planning read here, before the
            // product is paid for.
        };
        // The connected ledger only exists inside a demand; a splice built
        // outside one (a fixture's direct call) is weighed, not charged.
        if self.connected_demand.is_active() {
            let paid = self
                .connected_demand
                .work_available()
                .and_then(|available| {
                    if available < plan.copy_work() {
                        // Prospective admission refuses before allocating a product.
                        return self.connected_demand.charge_units(plan.copy_work());
                    }
                    self.connected_demand
                        .reserve_bytes(plan.len.saturating_mul(std::mem::size_of::<TupleElement>()))
                });
            if let Err(reasons) = paid {
                return Spliced::Unpaid(reasons);
            }
        }
        let mut out = Vec::with_capacity(plan.len);
        let mut credit = TraversalCredit::open(&self.connected_demand, self.cancellation);
        if let Err(reasons) =
            self.copy_planned_splice(elements, &plan, position, &mut out, &mut credit)
        {
            return Spliced::Unpaid(reasons);
        }
        drop(credit);
        verter_debug_assert_eq!(out.len(), plan.len, "the product holds what was paid for");
        Spliced::Elements(out)
    }

    /// Resolve the sole rest-of-array collapse with the same bounded
    /// traversal protocol as tuple spread discovery.
    fn spread_array(
        &self,
        value: SemanticNodeId,
        credit: &mut TraversalCredit<'_, '_>,
    ) -> Result<Option<SemanticNodeId>, PartialReasonSet> {
        let (mut current, mut tortoise) = (value, value);
        let (mut power, mut passed) = (1usize, 0usize);
        loop {
            credit.step()?;
            match self.graph().node_data(current).as_deref() {
                Some(SemanticNodeData::Array { .. }) => return Ok(Some(current)),
                Some(SemanticNodeData::Alias(next)) => current = *next,
                _ => return Ok(None),
            }
            if current == tortoise {
                return Ok(None);
            }
            passed += 1;
            if passed == power {
                tortoise = current;
                power = power.saturating_mul(2);
                passed = 0;
            }
        }
    }

    /// The tuple a spread of `value` splices, when `value` settles on one
    /// through transparent aliases: each alias passed is a step of the
    /// traversal, and a cycle of aliases (found in constant space) settles
    /// on nothing.
    fn spread_target(
        &self,
        value: SemanticNodeId,
        credit: &mut TraversalCredit<'_, '_>,
    ) -> Result<Option<SpliceTarget>, PartialReasonSet> {
        let graph = self.graph();
        let (mut current, mut tortoise) = (value, value);
        let (mut power, mut passed) = (1usize, 0usize);
        loop {
            credit.step()?;
            let Some(data) = graph.node_data(current) else {
                return Ok(None);
            };
            match &*data {
                SemanticNodeData::Tuple { elements, .. } => {
                    return Ok(Some((current, Arc::clone(elements.as_arc()))));
                }
                SemanticNodeData::Alias(next) => {
                    current = *next;
                }
                _ => return Ok(None),
            }
            if current == tortoise {
                return Ok(None);
            }
            passed += 1;
            if passed == power {
                tortoise = current;
                power = power.saturating_mul(2);
                passed = 0;
            }
        }
    }

    /// Plan splicing `elements`: read every tuple it splices, nested ones
    /// included, from a work list, each sized and planned once, as the
    /// checker builds each tuple once — its own splices checked against its
    /// own prefix. A spliced tuple whose own splice reaches the limit is
    /// refused on its own and is one element of the list spreading it; a
    /// splice of `elements` itself reaching the limit refuses the whole
    /// list (`Err(None)`). A tuple spread into itself (a cycle) keeps its
    /// rest element and counts as one element. Every step read is paid for
    /// before it is read, and every step of the plan has its storage
    /// reserved before it is stored (`Err(Some(trip))` when the demand runs
    /// out or is cancelled). Nothing is copied.
    fn plan_tuple_splice(
        &self,
        elements: &[TupleElement],
        credit: &mut TraversalCredit<'_, '_>,
    ) -> Result<SplicePlan, Option<PartialReasonSet>> {
        credit
            .reserve(
                std::mem::size_of::<SizingLevel<'static>>()
                    + std::mem::size_of::<CopyFrame<'static>>(),
            )
            .map_err(Some)?;
        let mut targets: FxHashMap<SemanticNodeId, Option<SpliceTarget>> = FxHashMap::default();
        let mut tuples: FxHashMap<SemanticNodeId, PlannedTuple> = FxHashMap::default();
        let mut splicing: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut levels = vec![SizingLevel {
            elements: LevelElements::Outer(elements),
            next: 0,
            tuple: None,
            len: 0,
            ops: Vec::new(),
        }];
        loop {
            let Some(level) = levels.last_mut() else {
                unreachable!("the outermost level returns the plan");
            };
            if level.next == level.elements.len() {
                let Some(done) = levels.pop() else {
                    unreachable!("a level was read");
                };
                let Some(tuple) = done.tuple else {
                    return Ok(SplicePlan {
                        ops: done.ops,
                        tuples,
                        len: done.len,
                    });
                };
                splicing.remove(&tuple);
                let LevelElements::Inner(inner) = done.elements else {
                    unreachable!("a spliced tuple's level reads its own elements");
                };
                let len = done.len;
                tuples.insert(
                    tuple,
                    PlannedTuple::Expanded {
                        elements: inner,
                        ops: done.ops,
                        len,
                    },
                );
                Self::join_planned(&mut levels, &mut tuples, &mut splicing, tuple, len, credit)?;
                continue;
            };
            credit.step().map_err(Some)?;
            let index = level.next;
            let (rest, value) = level
                .elements
                .get(index)
                .expect("the admitted element exists");
            level.next += 1;
            if !rest {
                level.keep(index, credit).map_err(Some)?;
                continue;
            }
            let target = match targets.get(&value) {
                Some(target) => target.clone(),
                None => {
                    credit.reserve(PLANNED_TUPLE_BYTES).map_err(Some)?;
                    let target = self.spread_target(value, credit).map_err(Some)?;
                    targets.insert(value, target.clone());
                    target
                }
            };
            let Some(level) = levels.last_mut() else {
                unreachable!("the level read is still open");
            };
            let Some((tuple, inner)) = target.filter(|(tuple, _)| !splicing.contains(tuple)) else {
                level.keep(index, credit).map_err(Some)?;
                continue;
            };
            match tuples.get(&tuple) {
                Some(PlannedTuple::Refused) => {
                    level.len += 1;
                    level
                        .push(SpliceOp::Recover { index, tuple }, credit)
                        .map_err(Some)?;
                }
                Some(PlannedTuple::Expanded { len, .. }) => {
                    let len = *len;
                    Self::join_planned(
                        &mut levels,
                        &mut tuples,
                        &mut splicing,
                        tuple,
                        len,
                        credit,
                    )?;
                }
                None => {
                    splicing.insert(tuple);
                    levels.push(SizingLevel {
                        elements: LevelElements::Inner(inner),
                        next: 0,
                        tuple: Some(tuple),
                        len: 0,
                        ops: Vec::new(),
                    });
                }
            }
        }
    }

    /// Splice the planned `tuple` of `len` elements into the open level
    /// spreading it, checked against that level's prefix. A level whose
    /// splice reaches the limit is refused: the outermost list whole
    /// (`Err(None)`), a spliced tuple in its place in the level spreading
    /// it, read no further.
    fn join_planned(
        levels: &mut Vec<SizingLevel<'_>>,
        tuples: &mut FxHashMap<SemanticNodeId, PlannedTuple>,
        splicing: &mut FxHashSet<SemanticNodeId>,
        tuple: SemanticNodeId,
        len: usize,
        credit: &TraversalCredit<'_, '_>,
    ) -> Result<(), Option<PartialReasonSet>> {
        let Some(level) = levels.last_mut() else {
            unreachable!("a spliced tuple has the level that spread it");
        };
        if splice_fits(level.len, len) {
            if len > 0 {
                level.len += len;
                level.push(SpliceOp::Splice(tuple), credit).map_err(Some)?;
            }
            return Ok(());
        }
        let Some(refused) = levels.pop().and_then(|level| level.tuple) else {
            return Err(None);
        };
        splicing.remove(&refused);
        tuples.insert(refused, PlannedTuple::Refused);
        let Some(parent) = levels.last_mut() else {
            unreachable!("a spliced tuple has the level that spread it");
        };
        parent.len += 1;
        parent
            .push(
                SpliceOp::Recover {
                    index: parent.next - 1,
                    tuple: refused,
                },
                credit,
            )
            .map_err(Some)
    }

    /// Copy `elements` into `out` as `plan` expands them: each spliced
    /// tuple's planned steps run once, and a tuple spliced again copies
    /// the elements its first expansion produced, so the copy reads no
    /// more steps than planning stored and no more elements than were
    /// paid for. A tuple refused on its own is its recovery, minted once.
    fn copy_planned_splice(
        &self,
        elements: &[TupleElement],
        plan: &SplicePlan,
        position: TupleSplicePosition,
        out: &mut Vec<TupleElement>,
        credit: &mut TraversalCredit<'_, '_>,
    ) -> Result<(), PartialReasonSet> {
        let mut copied: FxHashMap<SemanticNodeId, (usize, usize)> = FxHashMap::default();
        let mut recoveries: FxHashMap<SemanticNodeId, SemanticNodeId> = FxHashMap::default();
        // Each frame: the elements its steps index, the steps, the next
        // one, and the tuple it expands with where its expansion starts.
        let mut frames: Vec<CopyFrame<'_>> = vec![(elements, &plan.ops, 0, None)];
        while let Some((elements, ops, next, _)) = frames.last_mut() {
            let elements: &[TupleElement] = elements;
            let Some(op) = ops.get(*next) else {
                if let Some((_, _, _, Some((tuple, start)))) = frames.pop() {
                    copied.insert(tuple, (start, out.len()));
                }
                continue;
            };
            credit.step()?;
            *next += 1;
            match op {
                SpliceOp::Keep { start, end } => {
                    for element in &elements[*start..*end] {
                        credit.step()?;
                        out.push(element.copy());
                    }
                }
                SpliceOp::Recover { index, tuple } => {
                    let recovery = *recoveries.entry(*tuple).or_insert_with(|| {
                        self.refuse_splice(
                            checker_policy::tuple_splice_refusal(position),
                            Some(*tuple),
                        )
                    });
                    credit.step()?;
                    let mut element = elements[*index].copy();
                    element.value = recovery;
                    out.push(element);
                }
                SpliceOp::Splice(tuple) => {
                    if let Some(&(start, end)) = copied.get(tuple) {
                        for at in start..end {
                            credit.step()?;
                            let element = out[at].copy();
                            out.push(element);
                        }
                        continue;
                    }
                    let Some(PlannedTuple::Expanded { elements, ops, .. }) = plan.tuples.get(tuple)
                    else {
                        unreachable!("a planned splice expands a planned tuple");
                    };
                    let start = out.len();
                    frames.push((elements, ops, 0, Some((*tuple, start))));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod credit_tests {
    use super::*;

    /// Cancellation arriving inside a locally drawn window is observed at
    /// its next bounded refresh, and every consumed unit is committed.
    #[test]
    fn cancellation_commits_the_consumed_traversal_prefix() {
        use crate::project_semantic_dispatch::connected_demand::DemandCancellation;
        use crate::resolver_core::resolver_context::{ProjectGenerationClock, RequestFlags};
        let token = verter_execution::cancellation::CancellationToken::new();
        let _job =
            verter_execution::cancellation::JobCancellationGuard::replace(Some(token.clone()));
        let flags = RequestFlags::new(
            ProjectGenerationClock::new(Arc::new(std::sync::atomic::AtomicU64::new(0))),
            verter_session_query::facts::policy::FactPolicy::production(),
        );
        let ledger = ConnectedDemandLedger::new(DemandCancellation::from_flags(&flags));
        ledger.set_limits_for_tests(1_000, 24);
        let (_demand, trip) = ledger.enter(false);
        assert!(trip.is_none());
        let mut credit = TraversalCredit::open(&ledger, flags.cancellation_checkpoint());
        credit.step().unwrap();
        token.cancel();
        for _ in 1..TRAVERSAL_BATCH {
            credit.step().unwrap();
        }
        assert_eq!(credit.step(), Err(PartialReasonSet::CANCELLED));
        drop(credit);
        assert_eq!(ledger.work_used_for_tests(), TRAVERSAL_BATCH);
        assert_eq!(ledger.charge(), Err(PartialReasonSet::CANCELLED));
    }
}
