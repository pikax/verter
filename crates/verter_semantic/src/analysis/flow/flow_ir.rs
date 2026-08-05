//! [`ReturnSlicePlan`] — the demand planner's reachability result — and
//! [`FlowSliceIR`] — the arena-free lowered form of exactly that selected
//! subgraph.
//!
//! Both carriers are `Send + Sync + 'static` and transitively
//! `TypeExpr`-free (`NoTypeExpr`-certified): the IR stores interned
//! names, ordinals, spans, and ids only. Expression CONTENT that the
//! solver must evaluate stays behind a span locator
//! ([`FlowExpr::span`]) and is evaluated on demand from the retained
//! parse snapshot at slice-evaluation time — never stored lowered here.

use std::sync::Arc;

use verter_no_typeexpr::NoTypeExpr;

use super::flow_graph::FlowNodeId;
use super::peeker::{DemandSegment, SliceOrigin};
use super::{SkeletonBindingId, SkeletonBindingKind, SkeletonExprSiteId, SkeletonWriteCertainty};

// ---------------------------------------------------------------------------
// The slice plan
// ---------------------------------------------------------------------------

/// The demand planner's result: exactly the subgraph reachable from the
/// demand origins under the two edge-class families' stop conditions.
/// Node sets are sorted ascending by dense node index and disjoint —
/// `effect_only_nodes` holds nodes reached ONLY through effect edges
/// (their value is never materialized; their evaluation effects are).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct ReturnSlicePlan {
    /// The demand origins the reachability started from.
    pub origins: Arc<[SliceOrigin]>,
    /// The demanded projection path (empty = whole value).
    pub demand_path: Arc<[DemandSegment]>,
    /// Value-selected nodes (their value contributes to the demand),
    /// sorted ascending.
    pub value_nodes: Arc<[FlowNodeId]>,
    /// Effect-only nodes (evaluation effects survive; value is never
    /// materialized), sorted ascending, disjoint from `value_nodes`.
    pub effect_only_nodes: Arc<[FlowNodeId]>,
}

impl ReturnSlicePlan {
    /// Whether `node` is selected at all (value or effect).
    #[must_use]
    pub fn is_selected(&self, node: FlowNodeId) -> bool {
        self.is_value(node) || self.is_effect_only(node)
    }

    /// Whether `node` is value-selected.
    #[must_use]
    pub fn is_value(&self, node: FlowNodeId) -> bool {
        self.value_nodes
            .binary_search_by_key(&node.index(), |n| n.index())
            .is_ok()
    }

    /// Whether `node` is effect-only.
    #[must_use]
    pub fn is_effect_only(&self, node: FlowNodeId) -> bool {
        self.effect_only_nodes
            .binary_search_by_key(&node.index(), |n| n.index())
            .is_ok()
    }
}

// ---------------------------------------------------------------------------
// IR ids
// ---------------------------------------------------------------------------

/// One lowered slot of the slice — the solver-internal slot identity.
/// An IR type only: it is never a public `TypeExpr` / `GraphTypeNode`
/// variant, and never appears on a published type surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowSlotId(u32);

impl FlowSlotId {
    /// Index into [`FlowSliceIR::slots`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One lowered expression record of the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowExprId(u32);

impl FlowExprId {
    /// Index into [`FlowSliceIR::exprs`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// One segment of a lowered projection path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FlowPathSegment {
    /// A statically-known property key (interned text).
    Named(Arc<str>),
    /// A computed / unknown key.
    Computed,
}

/// A lowered projection path.
pub type FlowPath = Arc<[FlowPathSegment]>;

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

/// One definition contributing to a slot, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowDef {
    /// The providing expression.
    pub value: FlowExprId,
    /// The written projection path under the slot (empty = whole-slot).
    pub path: FlowPath,
    /// Whether the write definitely happens when its site evaluates.
    pub certainty: SkeletonWriteCertainty,
}

/// One lowered slot: a selected binding with the selected subset of its
/// definitions. An effect-only slot (mutated but never read by the
/// demanded path) carries no selected definitions.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowSlot {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind.
    pub kind: SkeletonBindingKind,
    /// The skeleton binding this slot lowers.
    pub binding: SkeletonBindingId,
    /// The binding identifier's span — the DECLARATION-precise slot
    /// identity the content lowering gates on (name identity would
    /// re-conflate shadowing same-named bindings the plan kept
    /// distinct). Never folded into the slice hash (the hash covers the
    /// plan's selected subgraph and is span-free).
    pub span: verter_span::Span,
    /// Whether the slot's value contributes to the demand (`false` =
    /// effect-only: mutated, never value-read by the selected path).
    pub value_selected: bool,
    /// Selected definitions in source order.
    pub defs: Arc<[FlowDef]>,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// Why an expression record is in the slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FlowExprRole {
    /// The expression's value contributes to the demanded projection.
    Value,
    /// Only the expression's evaluation effects contribute — its value
    /// is never materialized.
    EffectOnly,
}

/// One identifier read of an opaque expression, resolved against the
/// slice's slot table.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowRead {
    /// The read name.
    pub name: Arc<str>,
    /// The slot the name resolves to, when exactly one selected binding
    /// carries the name (`None` = free / unselected / ambiguous — the
    /// solver resolves lexically at evaluation time).
    pub slot: Option<FlowSlotId>,
}

/// One object-literal key.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowObjectKey {
    /// A statically-known key.
    Named(Arc<str>),
    /// A computed key; its key expression rides as its own selected
    /// record when the plan selected it (its evaluation effects survive
    /// independently of the named value).
    Computed(Option<FlowExprId>),
}

/// One SELECTED object-literal entry. Unselected sibling entries are
/// elided — counted, never lowered ([`FlowExprShape::ObjectLiteral`]).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowObjectEntry {
    /// A property provisioning one key.
    Property {
        /// The property key.
        key: FlowObjectKey,
        /// The entry's value expression.
        value: FlowExprId,
    },
    /// A spread entry (`...src`).
    Spread {
        /// The spread source expression.
        source: FlowExprId,
    },
}

/// The lowered shape of one selected expression.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowExprShape {
    /// An object literal carrying ONLY the selected entries, in authored
    /// order. `elided_entries` counts the authored siblings the plan
    /// left unselected, so no consumer can mistake this record for the
    /// full literal.
    ObjectLiteral {
        /// The selected entries in authored order.
        entries: Arc<[FlowObjectEntry]>,
        /// Authored entries the plan did not select.
        elided_entries: u32,
    },
    /// Any other expression: a span locator plus the read footprint. The
    /// content evaluates on demand from the retained parse snapshot at
    /// slice-evaluation time.
    Opaque {
        /// Identifier reads attributed to this expression.
        reads: Arc<[FlowRead]>,
    },
}

/// One lowered expression record.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowExpr {
    /// The skeleton site this record lowers.
    pub site: SkeletonExprSiteId,
    /// The expression's span — the retained-snapshot locator its content
    /// evaluates through on demand.
    pub span: verter_span::Span,
    /// Why the record is in the slice.
    pub role: FlowExprRole,
    /// The lowered shape.
    pub shape: FlowExprShape,
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

/// The target of one lowered write effect.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowEffectTarget {
    /// A selected slot.
    Slot(FlowSlotId),
    /// A named root outside the slot table (free name, unselected or
    /// shadow-ambiguous binding).
    Named(Arc<str>),
    /// An unresolvable target root.
    Opaque,
}

/// The callee shape of one lowered call effect.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowCallee {
    /// A bare identifier callee.
    Named(Arc<str>),
    /// A static member path rooted at an identifier, root first.
    Path(Arc<[Arc<str>]>),
    /// Any other callee shape.
    Opaque,
}

/// One evaluation-effect obligation of the slice, in source order. The
/// solver applies these (retypes, widenings, call barriers) before
/// evaluating the value providers that read the affected slots.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowEffect {
    /// A write into a binding performed by a selected expression.
    Write {
        /// The expression whose evaluation performs the write.
        site: FlowExprId,
        /// The write's root target.
        target: FlowEffectTarget,
        /// The projection path under the root (empty = whole-slot).
        path: FlowPath,
        /// Whether the write definitely happens when the site evaluates.
        certainty: SkeletonWriteCertainty,
        /// The written value's expression, when selected.
        value: Option<FlowExprId>,
        /// The write expression's span.
        span: verter_span::Span,
    },
    /// A call / construct performed by a selected expression.
    Call {
        /// The expression whose evaluation performs the call.
        site: FlowExprId,
        /// The callee shape.
        callee: FlowCallee,
        /// Whether this is a `new` construct site.
        new_construct: bool,
        /// The call expression's span.
        span: verter_span::Span,
    },
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

/// One lowered return contributor.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowReturnEntry {
    /// Source-order ordinal among the function's return sites.
    pub ordinal: u32,
    /// Whether the site is the implicit return of an expression-bodied
    /// arrow.
    pub implicit: bool,
    /// The returned value's expression (`None` for bare `return;`).
    pub argument: Option<FlowExprId>,
    /// The return statement's span.
    pub span: verter_span::Span,
}

/// The ordered return contributors of the slice.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct ReturnAccumulator {
    /// The lowered return sites, in source order.
    pub sites: Arc<[FlowReturnEntry]>,
}

// ---------------------------------------------------------------------------
// The slice IR
// ---------------------------------------------------------------------------

/// The lowered form of exactly one [`ReturnSlicePlan`]: the selected
/// slots, expressions, effect obligations, and return contributors —
/// nothing outside the plan's reachable subgraph is lowered.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowSliceIR {
    /// The demanded projection path this slice serves.
    pub demanded_path: FlowPath,
    /// The lowered slots, dense-indexed by [`FlowSlotId`].
    pub slots: Arc<[FlowSlot]>,
    /// The lowered expressions, dense-indexed by [`FlowExprId`].
    pub exprs: Arc<[FlowExpr]>,
    /// The evaluation-effect obligations, in source order.
    pub effects: Arc<[FlowEffect]>,
    /// The return contributors.
    pub returns: ReturnAccumulator,
    /// Expression-site demand origins, when the demand originated at
    /// expression sites rather than return sites.
    pub expression_origins: Arc<[FlowExprId]>,
}

impl FlowSliceIR {
    /// The slot record for `id`.
    #[must_use]
    pub fn slot(&self, id: FlowSlotId) -> &FlowSlot {
        &self.slots[id.index()]
    }

    /// The expression record for `id`.
    #[must_use]
    pub fn expr(&self, id: FlowExprId) -> &FlowExpr {
        &self.exprs[id.index()]
    }
}
