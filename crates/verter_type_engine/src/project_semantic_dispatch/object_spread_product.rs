//! Factored object-spread products.
//!
//! The correlated alternatives of an object construction program are a
//! product: every union spread operand chooses one of its arms, and every
//! later effect applies on top of each earlier choice. A spread of an
//! `M`-arm union followed by an `N`-arm union has `M × N` alternatives, but
//! only `M + N` distinct contributions. The product is therefore kept
//! FACTORED, as a DAG of three node kinds:
//!
//! * [`ProductNode::Leaf`] — one fragment every alternative through the node
//!   applies (a direct effect, the copied facts of one closed object, a
//!   residual operand);
//! * [`ProductNode::Choice`] — exactly one arm applies; the node's
//!   alternatives are its arms' alternatives, in arm order;
//! * [`ProductNode::Overlay`] — every layer applies, in order; the node's
//!   alternatives are the mixed-radix product of its layers', the first
//!   layer most significant.
//!
//! A spread of another construction program is a [`ProductNode::Nested`]
//! node: a choice over the nested program's own (factored) formula.
//!
//! No alternative exists until a consumer asks for it. A [`ProductCursor`]
//! reads them one at a time by index, folding only the leaves on that
//! alternative's path, and charges each to the connected demand before it
//! folds it: the work and the construction bytes of the alternative are
//! known from the product's bounds before any of it is built, so a demand
//! that cannot pay stops before construction, never after it. A refused
//! read is an `Err` carrying the demand's trip; it never yields an empty or
//! truncated alternative.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_session_query::retention::allocation::{arc_block_bytes, heap_block_bytes, AliasSet};

use super::ProjectSemanticDispatch;
use crate::semantic_query::allocation_footprint::{copy_inline, OwnedHeap};
use crate::semantic_query::{
    ExactOptionalPropertyPolicy, MemberFacets, ObjectProjectionAlternative,
    ObjectProjectionFormula, ObjectProjectionIndex, ObjectProjectionSelector, PartialReasonSet,
    PositiveKeyFact, PositiveKeyPresence, ProjectionEvidence, PropertyKey, SemanticNodeId,
};

/// The measurement of a product's interior: its shared nodes and
/// fragments are private to the product — no caller can clone one out of the
/// owned cell — so each is counted once by pointer here, while the owned
/// handles the interior reaches (names, nested formulas) go to `aliases`.
struct Interior<'a> {
    aliases: &'a mut AliasSet,
    seen: FxHashSet<usize>,
}

impl Interior<'_> {
    fn first<T: ?Sized>(&mut self, value: &Arc<T>) -> bool {
        self.seen.insert(Arc::as_ptr(value).cast::<()>() as usize)
    }

    /// An `Arc<T>`'s block and the heap its value owns, once.
    fn value<T>(&mut self, value: &Arc<T>, heap: impl FnOnce(&T, &mut Self) -> usize) -> usize {
        if self.first(value) {
            arc_block_bytes(std::mem::size_of::<T>()) + heap(value, self)
        } else {
            0
        }
    }

    /// An `Arc<[T]>`'s block, or `None` when already counted.
    fn slice_block<T>(&mut self, value: &Arc<[T]>) -> Option<usize> {
        self.first(value)
            .then(|| arc_block_bytes(std::mem::size_of_val::<[T]>(value)))
    }
}

/// One positively known named member a fragment writes or a fold holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemberState {
    pub(super) key: PropertyKey,
    pub(super) presence: PositiveKeyPresence,
    pub(super) value: ProjectionEvidence<SemanticNodeId>,
    pub(super) facets: ProjectionEvidence<MemberFacets>,
}

/// How a residual spread operand classifies for excess eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResidualExcessClass {
    /// A semantically generic operand: suppresses every direct candidate.
    Generic,
    /// An operand that cannot be classified: indeterminate, not suppression.
    Unclassified,
    /// A residual that carries no generic-spread evidence (for example an
    /// unresolved computed key): no eligibility effect.
    Neutral,
}

/// The facts one spread operand alternative copies onto the object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SpreadFragment {
    pub(super) members: Arc<[MemberState]>,
    pub(super) indices: Arc<[ObjectProjectionIndex]>,
    /// The copied alternative stays open: it taints what precedes it.
    pub(super) residual: bool,
}

/// One fragment of a product. Leaves name graph nodes by ordinal; a
/// direct effect is read back from its program when it is folded.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProductLeaf {
    /// The direct (non-spread) effect at `effect` of `program`.
    Direct {
        program: SemanticNodeId,
        effect: u32,
        /// False when the effect writes an unresolved computed key, which
        /// leaves the alternative's key domain open.
        closed: bool,
    },
    /// The copied facts of one closed object alternative.
    Spread(SpreadFragment),
    /// The required public writes of a property-only spread chain, copied
    /// onto its base: every tail effect of each program, innermost first.
    /// `writes` is how many there are.
    CopiedWrites {
        programs: Arc<[SemanticNodeId]>,
        writes: usize,
    },
    /// A spread operand that stays a residual.
    Residual {
        operand: SemanticNodeId,
        class: ResidualExcessClass,
    },
}

/// The children of a choice or an overlay, with the bounds every
/// alternative through them obeys.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ProductArms {
    nodes: Box<[ProductNode]>,
    /// Choice: the exclusive end index of each arm's alternatives.
    /// Overlay: the stride of each layer (the product of the later layers'
    /// widths).
    marks: Box<[u64]>,
    bounds: ProductBounds,
}

/// A spread of another construction program: a choice over its formula's
/// alternatives, each copied as an object.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NestedProduct {
    pub(super) operand: SemanticNodeId,
    pub(super) formula: ObjectProjectionFormula,
}

/// What every alternative through a node costs at most, and how many
/// alternatives it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProductBounds {
    /// Alternatives through the node (saturating).
    pub(super) width: u64,
    /// Work units folding one alternative through the node costs at most:
    /// one per leaf on its path plus one per member it writes.
    pub(super) units: usize,
    /// Named members and index facts one alternative through the node
    /// writes at most.
    pub(super) facts: usize,
}

impl ProductBounds {
    const LEAF: Self = Self {
        width: 1,
        units: 1,
        facts: 0,
    };
}

/// One node of a factored product. Cloning shares the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProductNode {
    Leaf(Arc<ProductLeaf>),
    Choice(Arc<ProductArms>),
    Overlay(Arc<ProductArms>),
    Nested(Arc<NestedProduct>),
    /// A finished alternative, read back verbatim: a product of exactly one
    /// alternative is folded once when it is built.
    Finished(Arc<ObjectProjectionAlternative>),
}

impl ProductNode {
    pub(super) fn leaf(leaf: ProductLeaf) -> Self {
        Self::Leaf(Arc::new(leaf))
    }

    pub(super) fn nested(operand: SemanticNodeId, formula: ObjectProjectionFormula) -> Self {
        Self::Nested(Arc::new(NestedProduct { operand, formula }))
    }

    /// Exactly one of `arms` applies. A single arm is that arm.
    pub(super) fn choice(mut arms: Vec<ProductNode>) -> Self {
        if arms.len() == 1 {
            return arms.pop().expect("one arm");
        }
        let mut marks = Vec::with_capacity(arms.len());
        let mut bounds = ProductBounds {
            width: 0,
            units: 0,
            facts: 0,
        };
        for arm in &arms {
            let arm = arm.bounds();
            bounds.width = bounds.width.saturating_add(arm.width);
            bounds.units = bounds.units.max(arm.units);
            bounds.facts = bounds.facts.max(arm.facts);
            marks.push(bounds.width);
        }
        Self::Choice(Arc::new(ProductArms {
            nodes: arms.into_boxed_slice(),
            marks: marks.into_boxed_slice(),
            bounds,
        }))
    }

    /// Every one of `layers` applies, in order. A single layer is that
    /// layer; no layer is the empty object.
    pub(super) fn overlay(mut layers: Vec<ProductNode>) -> Self {
        if layers.len() == 1 {
            return layers.pop().expect("one layer");
        }
        let mut marks = vec![1u64; layers.len()];
        let mut stride = 1u64;
        let mut bounds = ProductBounds {
            width: 1,
            units: 0,
            facts: 0,
        };
        for (index, layer) in layers.iter().enumerate().rev() {
            let layer = layer.bounds();
            marks[index] = stride;
            stride = stride.saturating_mul(layer.width);
            bounds.units = bounds.units.saturating_add(layer.units);
            bounds.facts = bounds.facts.saturating_add(layer.facts);
        }
        bounds.width = stride;
        Self::Overlay(Arc::new(ProductArms {
            nodes: layers.into_boxed_slice(),
            marks: marks.into_boxed_slice(),
            bounds,
        }))
    }

    /// The node's alternative count and per-alternative costs.
    pub(super) fn bounds(&self) -> ProductBounds {
        match self {
            Self::Leaf(leaf) => match leaf.as_ref() {
                ProductLeaf::Direct { .. } => ProductBounds {
                    facts: 1,
                    ..ProductBounds::LEAF
                },
                ProductLeaf::Spread(fragment) => {
                    let facts = fragment.members.len() + fragment.indices.len();
                    ProductBounds {
                        width: 1,
                        units: 1 + facts,
                        facts,
                    }
                }
                ProductLeaf::CopiedWrites { writes, .. } => ProductBounds {
                    width: 1,
                    units: 1 + writes,
                    facts: *writes,
                },
                ProductLeaf::Residual { .. } => ProductBounds::LEAF,
            },
            Self::Choice(arms) | Self::Overlay(arms) => arms.bounds,
            Self::Nested(nested) => {
                let inner = nested.formula.product().root.bounds();
                ProductBounds {
                    width: inner.width,
                    units: inner.units.saturating_add(1 + inner.facts),
                    facts: inner.facts,
                }
            }
            Self::Finished(alternative) => {
                let facts = alternative.fact_count();
                ProductBounds {
                    width: 1,
                    units: 1 + facts,
                    facts,
                }
            }
        }
    }

    /// Whether every alternative through the node is free of residual
    /// operands — the formula-wide closed-domain fact, decided without
    /// enumerating a single alternative.
    pub(super) fn closes(&self) -> bool {
        match self {
            Self::Leaf(leaf) => match leaf.as_ref() {
                ProductLeaf::Spread(fragment) => !fragment.residual,
                ProductLeaf::Residual { .. } => false,
                ProductLeaf::Direct { closed, .. } => *closed,
                ProductLeaf::CopiedWrites { .. } => true,
            },
            Self::Choice(arms) | Self::Overlay(arms) => arms.nodes.iter().all(Self::closes),
            Self::Nested(nested) => nested.formula.all_alternatives_closed(),
            Self::Finished(alternative) => alternative.closed().is_some(),
        }
    }

    /// Visit the step `index` selects through this node: the leaves on the
    /// path and, at a nested node, the nested formula with its local index.
    pub(super) fn walk_path<E>(
        &self,
        index: u64,
        visit: &mut impl FnMut(PathStep<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::Leaf(leaf) => visit(PathStep::Leaf(leaf)),
            Self::Choice(arms) => {
                let arm = arms.marks.partition_point(|end| *end <= index);
                let start = arm
                    .checked_sub(1)
                    .map_or(0, |previous| arms.marks[previous]);
                arms.nodes[arm].walk_path(index - start, visit)
            }
            Self::Overlay(arms) => {
                for (layer, stride) in arms.nodes.iter().zip(arms.marks.iter()) {
                    let width = layer.bounds().width.max(1);
                    layer.walk_path((index / stride) % width, visit)?;
                }
                Ok(())
            }
            Self::Nested(nested) => visit(PathStep::Nested(nested, index)),
            Self::Finished(alternative) => visit(PathStep::Finished(alternative)),
        }
    }

    /// Rebuild the node with every spread fragment narrowed by `narrow`,
    /// collapsing arms of a choice that narrow to the same contribution:
    /// a demand that reads only some facts never multiplies by arms that
    /// differ only in facts it does not read.
    pub(super) fn narrowed(
        &self,
        narrow: &impl Fn(&SpreadFragment) -> SpreadFragment,
        nested: &mut impl FnMut(&NestedProduct) -> Result<ProductNode, NarrowFailure>,
        charge: &mut impl FnMut(usize) -> Result<(), NarrowFailure>,
    ) -> Result<Self, NarrowFailure> {
        Ok(match self {
            Self::Leaf(leaf) => match leaf.as_ref() {
                ProductLeaf::Spread(fragment) => Self::leaf(ProductLeaf::Spread(narrow(fragment))),
                _ => self.clone(),
            },
            Self::Choice(arms) => {
                let mut distinct: Vec<ProductNode> = Vec::new();
                for arm in arms.nodes.iter() {
                    let arm = arm.narrowed(narrow, nested, charge)?;
                    // Deduplication compares against every kept arm.
                    charge(1 + distinct.len())?;
                    if !distinct.contains(&arm) {
                        distinct.push(arm);
                    }
                }
                Self::choice(distinct)
            }
            Self::Overlay(arms) => Self::overlay(
                arms.nodes
                    .iter()
                    .map(|layer| layer.narrowed(narrow, nested, charge))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Nested(inner) => nested(inner)?,
            Self::Finished(_) => self.clone(),
        })
    }

    pub(super) fn for_each_retained_node(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Leaf(leaf) => match leaf.as_ref() {
                ProductLeaf::Direct { program, .. } => visit(*program),
                ProductLeaf::Spread(fragment) => {
                    for member in fragment.members.iter() {
                        if let ProjectionEvidence::Proven(node) = member.value {
                            visit(node);
                        }
                    }
                    for index in fragment.indices.iter() {
                        visit(index.key_type());
                        if let ProjectionEvidence::Proven(node) = index.value() {
                            visit(*node);
                        }
                    }
                }
                ProductLeaf::CopiedWrites { programs, .. } => {
                    programs.iter().copied().for_each(&mut *visit);
                }
                ProductLeaf::Residual { operand, .. } => visit(*operand),
            },
            Self::Choice(arms) | Self::Overlay(arms) => {
                for node in arms.nodes.iter() {
                    node.for_each_retained_node(visit);
                }
            }
            Self::Nested(nested) => {
                visit(nested.operand);
                nested.formula.product().for_each_retained_node(visit);
            }
            Self::Finished(alternative) => alternative.for_each_retained_node(visit),
        }
    }
}

/// Why a narrowed node could not be built.
pub(super) enum NarrowFailure {
    /// A nested narrowed projection failed with its own typed reason.
    Query(crate::semantic_query::QueryError),
    /// A nested narrowed projection re-entered an in-flight query.
    Recursive(SemanticNodeId),
    /// The connected demand could not pay for the narrowing work.
    Exhausted(PartialReasonSet),
}

/// One step of an alternative's path.
pub(super) enum PathStep<'p> {
    Leaf(&'p ProductLeaf),
    Nested(&'p NestedProduct, u64),
    Finished(&'p ObjectProjectionAlternative),
}

/// The factored product a formula owns: its root node, the selector its
/// alternatives answer, the exact-optional policy its writes fold under,
/// and whether every alternative is closed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SpreadProduct {
    pub(super) root: ProductNode,
    pub(super) selector: ObjectProjectionSelector,
    pub(super) policy: ExactOptionalPropertyPolicy,
    pub(super) closed: bool,
}

impl SpreadProduct {
    pub(super) fn new(
        root: ProductNode,
        selector: ObjectProjectionSelector,
        policy: ExactOptionalPropertyPolicy,
    ) -> Self {
        let closed = root.closes();
        Self {
            root,
            selector,
            policy,
            closed,
        }
    }

    /// A product of finished alternatives, read back verbatim.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn sealed(alternatives: Vec<ObjectProjectionAlternative>) -> Self {
        let selector = alternatives
            .first()
            .map_or(ObjectProjectionSelector::Surface, |first| first.selector());
        let root = ProductNode::choice(
            alternatives
                .into_iter()
                .map(|alternative| ProductNode::Finished(Arc::new(alternative)))
                .collect(),
        );
        Self::new(root, selector, ExactOptionalPropertyPolicy::Disabled)
    }

    /// The finished alternatives of a product built by [`Self::sealed`].
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn sealed_alternatives(&self) -> Vec<ObjectProjectionAlternative> {
        let mut alternatives = Vec::new();
        for index in 0..self.width() {
            let _ = self.root.walk_path(index, &mut |step| {
                if let PathStep::Finished(alternative) = step {
                    alternatives.push(alternative.clone());
                }
                Ok::<(), ()>(())
            });
        }
        alternatives
    }

    /// Whether the product is one finished alternative.
    pub(super) fn is_finished(&self) -> bool {
        matches!(self.root, ProductNode::Finished(_))
    }

    /// How many correlated alternatives the product has.
    pub(crate) fn width(&self) -> u64 {
        self.root.bounds().width
    }

    pub(crate) fn closed(&self) -> bool {
        self.closed
    }

    pub(crate) fn for_each_retained_node(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.root.for_each_retained_node(visit);
    }
}

impl OwnedHeap for SpreadProduct {
    /// Every node, arm table, fragment and key the product owns, each
    /// shared node once; a nested formula is its own owned allocation.
    /// Graph nodes are ordinals: the arena owns them.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            root,
            selector,
            policy,
            closed,
        } = self;
        node_bytes(
            root,
            &mut Interior {
                aliases,
                seen: FxHashSet::default(),
            },
        ) + selector_bytes(selector, aliases)
            + copy_inline(policy)
            + copy_inline(closed)
    }
}

impl verter_session_query::retention::owned::Measured for SpreadProduct {
    fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        OwnedHeap::owned_heap_bytes(self, aliases)
    }
}

fn selector_bytes(selector: &ObjectProjectionSelector, aliases: &mut AliasSet) -> usize {
    match selector {
        ObjectProjectionSelector::Key(key) => key.owned_heap_bytes(aliases),
        ObjectProjectionSelector::RelationShape(keys) => keys.owned_heap_bytes(aliases),
        ObjectProjectionSelector::Surface
        | ObjectProjectionSelector::IndexDomain(_)
        | ObjectProjectionSelector::Signature(_)
        | ObjectProjectionSelector::EnumerableValueEnvelope(_)
        | ObjectProjectionSelector::ExcessEligibility => 0,
    }
}

fn node_bytes(node: &ProductNode, interior: &mut Interior<'_>) -> usize {
    match node {
        ProductNode::Leaf(leaf) => interior.value(leaf, leaf_bytes),
        ProductNode::Choice(arms) | ProductNode::Overlay(arms) => interior.value(arms, arms_bytes),
        ProductNode::Nested(nested) => interior.value(nested, |nested, interior| {
            let NestedProduct { operand, formula } = nested;
            copy_inline(operand) + formula.owned_heap_bytes(interior.aliases)
        }),
        ProductNode::Finished(alternative) => interior
            .value(alternative, |alternative, interior| {
                alternative.owned_heap_bytes(interior.aliases)
            }),
    }
}

fn arms_bytes(arms: &ProductArms, interior: &mut Interior<'_>) -> usize {
    let ProductArms {
        nodes,
        marks,
        bounds,
    } = arms;
    heap_block_bytes(std::mem::size_of_val::<[ProductNode]>(nodes))
        + nodes
            .iter()
            .map(|node| node_bytes(node, interior))
            .sum::<usize>()
        + heap_block_bytes(std::mem::size_of_val::<[u64]>(marks))
        + copy_inline(bounds)
}

fn leaf_bytes(leaf: &ProductLeaf, interior: &mut Interior<'_>) -> usize {
    match leaf {
        ProductLeaf::Direct {
            program,
            effect,
            closed,
        } => copy_inline(program) + copy_inline(effect) + copy_inline(closed),
        ProductLeaf::Spread(SpreadFragment {
            members,
            indices,
            residual,
        }) => {
            let mut bytes = copy_inline(residual);
            if let Some(block) = interior.slice_block(members) {
                bytes += block;
                for member in members.iter() {
                    bytes += member_bytes(member, interior.aliases);
                }
            }
            if let Some(block) = interior.slice_block(indices) {
                bytes += block;
                for index in indices.iter() {
                    bytes += index.owned_heap_bytes(interior.aliases);
                }
            }
            bytes
        }
        ProductLeaf::CopiedWrites { programs, writes } => {
            interior.slice_block(programs).unwrap_or(0) + copy_inline(writes)
        }
        ProductLeaf::Residual { operand, class } => copy_inline(operand) + copy_inline(class),
    }
}

fn member_bytes(member: &MemberState, aliases: &mut AliasSet) -> usize {
    let MemberState {
        key,
        presence,
        value,
        facets,
    } = member;
    key.owned_heap_bytes(aliases)
        + copy_inline(presence)
        + match value {
            ProjectionEvidence::Proven(node) => copy_inline(node),
            ProjectionEvidence::Indeterminate => 0,
        }
        + match facets {
            ProjectionEvidence::Proven(facets) => facets.owned_heap_bytes(aliases),
            ProjectionEvidence::Indeterminate => 0,
        }
}

/// Reads a formula's alternatives one at a time, in index order.
///
/// Each read charges the alternative's bounded work and construction
/// bytes to the active connected demand BEFORE folding it; a demand that
/// cannot pay ends the cursor with `Err(trip)` (folded into the reading
/// build's completeness), and no later read is attempted. Outside a
/// connected demand nothing is charged.
pub struct ProductCursor<'d, 'a, C: crate::resolver_core::ResolverCapabilities> {
    dispatch: &'d ProjectSemanticDispatch<'a, C>,
    formula: &'d ObjectProjectionFormula,
    next: u64,
    width: u64,
}

impl<'d, 'a, C: crate::resolver_core::ResolverCapabilities> ProductCursor<'d, 'a, C> {
    pub(super) fn new(
        dispatch: &'d ProjectSemanticDispatch<'a, C>,
        formula: &'d ObjectProjectionFormula,
    ) -> Self {
        Self {
            dispatch,
            formula,
            next: 0,
            width: formula.width(),
        }
    }
}

impl<C: crate::resolver_core::ResolverCapabilities> Iterator for ProductCursor<'_, '_, C> {
    type Item = Result<ObjectProjectionAlternative, PartialReasonSet>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.width {
            return None;
        }
        let index = self.next;
        self.next += 1;
        let product = self.formula.product();
        let read = self
            .dispatch
            .charge_product_alternative(product)
            .map(|()| self.dispatch.product_alternative(product, index));
        if read.is_err() {
            self.next = self.width;
        }
        Some(read)
    }

    /// Skip `n` alternatives without building them: an alternative is a
    /// function of its index alone.
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.next = self
            .next
            .saturating_add(u64::try_from(n).unwrap_or(u64::MAX))
            .min(self.width);
        self.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::try_from(self.width - self.next).unwrap_or(usize::MAX);
        (0, Some(remaining))
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// Read `formula`'s correlated alternatives on demand.
    pub fn object_spread_cursor<'d>(
        &'d self,
        formula: &'d ObjectProjectionFormula,
    ) -> ProductCursor<'d, 'a, C> {
        ProductCursor::new(self, formula)
    }

    /// Every alternative of `formula`, read through its cursor: for a
    /// consumer that needs them side by side.
    pub fn object_spread_alternatives(
        &self,
        formula: &ObjectProjectionFormula,
    ) -> Result<Vec<ObjectProjectionAlternative>, PartialReasonSet> {
        self.object_spread_cursor(formula).collect()
    }

    /// The formula-wide exact `keyof`: the keys present in EVERY
    /// alternative, intersected as the checker's `keyof (A | B)` is —
    /// nominally, so `1` in one alternative and `"1"` in another are no
    /// common key. `Ok(None)` unless every alternative is a whole-domain
    /// closed witness; `Err` with the demand's trip when it cannot pay to
    /// read them.
    pub fn object_spread_exact_keyof(
        &self,
        formula: &ObjectProjectionFormula,
    ) -> Result<Option<Vec<PropertyKey>>, PartialReasonSet> {
        if !formula.all_alternatives_closed() {
            return Ok(None);
        }
        let mut common: Option<Vec<PropertyKey>> = None;
        for alternative in self.object_spread_cursor(formula) {
            let alternative = alternative?;
            let Some(keys) = alternative.closed().and_then(|closed| closed.keyof()) else {
                return Ok(None);
            };
            match &mut common {
                None => common = Some(keys.to_vec()),
                Some(common) => common.retain(|key| keys.iter().any(|candidate| candidate == key)),
            }
        }
        Ok(Some(common.unwrap_or_default()))
    }

    /// Charge one alternative of `product` to the active connected demand:
    /// its bounded fold work and the bytes of the alternative it builds,
    /// before any of it is built.
    pub(super) fn charge_product_alternative(
        &self,
        product: &SpreadProduct,
    ) -> Result<(), PartialReasonSet> {
        if !self.connected_demand.is_active() {
            return Ok(());
        }
        let bounds = product.root.bounds();
        let bytes = std::mem::size_of::<ObjectProjectionAlternative>().saturating_add(
            bounds
                .facts
                .saturating_mul(std::mem::size_of::<PositiveKeyFact>()),
        );
        let charged = self
            .connected_demand
            .charge_units(bounds.units)
            .and_then(|()| self.connected_demand.reserve_bytes(bytes));
        if let Err(reasons) = charged {
            self.fold_local_partial_completeness(reasons);
            return Err(reasons);
        }
        Ok(())
    }
}
