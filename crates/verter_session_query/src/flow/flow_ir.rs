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

use crate::retention::owned::{Owned, OwnedSlice, OwnedStr};
use verter_no_typeexpr::NoTypeExpr;

use crate::flow::{
    frame_span::FrameSpan,
    skeleton::{
        SkeletonBindingId, SkeletonBindingKind, SkeletonExprSiteId, SkeletonWriteCertainty,
    },
};

// ---------------------------------------------------------------------------
// The slice plan
// ---------------------------------------------------------------------------

pub use super::peeker::ReturnSlicePlan;

// ---------------------------------------------------------------------------
// IR ids
// ---------------------------------------------------------------------------

/// One lowered slot of the slice — the solver-internal slot identity.
/// An IR type only: it is never a public `TypeExpr` / `GraphTypeNode`
/// variant, and never appears on a published type surface.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, verter_retention::heap_size::HeapSize,
)]
pub struct FlowSlotId(u32);

impl FlowSlotId {
    /// Index into [`FlowSliceIR::slots`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One lowered expression record of the slice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, verter_retention::heap_size::HeapSize,
)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub enum FlowPathSegment {
    /// A statically-known property key (interned text).
    Named(OwnedStr),
    /// A computed / unknown key.
    Computed,
}

/// A lowered projection path.
pub type FlowPath = OwnedSlice<FlowPathSegment>;

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

/// One definition contributing to a slot, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
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
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct FlowSlot {
    /// The binding name.
    pub name: OwnedStr,
    /// The binding kind.
    pub kind: SkeletonBindingKind,
    /// The skeleton binding this slot lowers.
    pub binding: SkeletonBindingId,
    /// The binding identifier's span — the DECLARATION-precise slot
    /// identity the content lowering gates on (name identity would
    /// re-conflate shadowing same-named bindings the plan kept
    /// distinct). Never folded into the slice hash (the hash covers the
    /// plan's selected subgraph and is span-free).
    pub span: FrameSpan,
    /// Whether the slot's value contributes to the demand (`false` =
    /// effect-only: mutated, never value-read by the selected path).
    pub value_selected: bool,
    /// Selected definitions in source order.
    pub defs: OwnedSlice<FlowDef>,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// Why an expression record is in the slice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr, verter_retention::heap_size::HeapSize,
)]
pub enum FlowExprRole {
    /// The expression's value contributes to the demanded projection.
    Value,
    /// Only the expression's evaluation effects contribute — its value
    /// is never materialized.
    EffectOnly,
}

/// One identifier read of an opaque expression, resolved against the
/// slice's slot table.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct FlowRead {
    /// The read name.
    pub name: OwnedStr,
    /// The slot the name resolves to, when exactly one selected binding
    /// carries the name (`None` = free / unselected / ambiguous — the
    /// solver resolves lexically at evaluation time).
    pub slot: Option<FlowSlotId>,
}

/// One object-literal key.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub enum FlowObjectKey {
    /// A statically-known key.
    Named(OwnedStr),
    /// A computed key; its key expression rides as its own selected
    /// record when the plan selected it (its evaluation effects survive
    /// independently of the named value).
    Computed(Option<FlowExprId>),
}

/// One SELECTED object-literal entry. Unselected sibling entries are
/// elided — counted, never lowered ([`FlowExprShape::ObjectLiteral`]).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
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
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub enum FlowExprShape {
    /// An object literal carrying ONLY the selected entries, in authored
    /// order. `elided_entries` counts the authored siblings the plan
    /// left unselected, so no consumer can mistake this record for the
    /// full literal.
    ObjectLiteral {
        /// The selected entries in authored order.
        entries: OwnedSlice<FlowObjectEntry>,
        /// Authored entries the plan did not select.
        elided_entries: u32,
    },
    /// Any other expression: a span locator plus the read footprint. The
    /// content evaluates on demand from the retained parse snapshot at
    /// slice-evaluation time.
    Opaque {
        /// Identifier reads attributed to this expression.
        reads: OwnedSlice<FlowRead>,
    },
}

/// One lowered expression record.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct FlowExpr {
    /// The skeleton site this record lowers.
    pub site: SkeletonExprSiteId,
    /// The expression's span — the retained-snapshot locator its content
    /// evaluates through on demand, in this frame's own coordinates.
    pub span: FrameSpan,
    /// Why the record is in the slice.
    pub role: FlowExprRole,
    /// The lowered shape.
    pub shape: FlowExprShape,
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

/// The target of one lowered write effect.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub enum FlowEffectTarget {
    /// A selected slot.
    Slot(FlowSlotId),
    /// A named root outside the slot table (free name, unselected or
    /// shadow-ambiguous binding).
    Named(OwnedStr),
    /// An unresolvable target root.
    Opaque,
}

/// The callee shape of one lowered call effect.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub enum FlowCallee {
    /// A bare identifier callee.
    Named(OwnedStr),
    /// A static member path rooted at an identifier, root first.
    Path(OwnedSlice<OwnedStr>),
    /// Any other callee shape.
    Opaque,
}

/// One evaluation-effect obligation of the slice, in source order. The
/// solver applies these (retypes, widenings, call barriers) before
/// evaluating the value providers that read the affected slots.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
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
        span: FrameSpan,
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
        span: FrameSpan,
    },
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

/// One lowered return contributor.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct FlowReturnEntry {
    /// Source-order ordinal among the function's return sites.
    pub ordinal: u32,
    /// Whether the site is the implicit return of an expression-bodied
    /// arrow.
    pub implicit: bool,
    /// The returned value's expression (`None` for bare `return;`).
    pub argument: Option<FlowExprId>,
    /// The return statement's span.
    pub span: FrameSpan,
}

/// The ordered return contributors of the slice.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct ReturnAccumulator {
    /// The lowered return sites, in source order.
    pub sites: OwnedSlice<FlowReturnEntry>,
}

// ---------------------------------------------------------------------------
// The slice IR
// ---------------------------------------------------------------------------

/// The lowered form of exactly one [`ReturnSlicePlan`]: the selected
/// slots, expressions, effect obligations, and return contributors —
/// nothing outside the plan's reachable subgraph is lowered.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr, verter_retention::heap_size::HeapSize)]
pub struct FlowSliceIR {
    /// The demanded projection path this slice serves.
    pub demanded_path: FlowPath,
    /// The lowered slots, dense-indexed by [`FlowSlotId`].
    pub slots: OwnedSlice<FlowSlot>,
    /// The lowered expressions, dense-indexed by [`FlowExprId`].
    pub exprs: OwnedSlice<FlowExpr>,
    /// The evaluation-effect obligations, in source order.
    pub effects: OwnedSlice<FlowEffect>,
    /// The return contributors.
    pub returns: ReturnAccumulator,
    /// Expression-site demand origins, when the demand originated at
    /// expression sites rather than return sites.
    pub expression_origins: OwnedSlice<FlowExprId>,
}

/// A lowered slice a cache hands its readers: a self-charged cell whose
/// value reaches no raw `Arc` — every list and name the slice keeps is an
/// owned handle charged by its own cell, so a list a reader clones out of
/// a borrowed slice keeps its own charge past the cell and the cache.
pub type SharedFlowSliceIR = Owned<FlowSliceIR>;

verter_retention::measured_by_heap_size!(
    FlowExprId,
    FlowPathSegment,
    FlowDef,
    FlowSlot,
    FlowRead,
    FlowObjectEntry,
    FlowExpr,
    FlowEffect,
    FlowReturnEntry,
    FlowSliceIR,
);

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
