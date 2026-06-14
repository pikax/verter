//! Output types for the AST-to-OXC template expression parsing pass.
//!
//! These types form a parallel data structure to [`TemplateAst`]: for each
//! `AstNode` in the arena, there is a corresponding [`OxcNodeData`] entry
//! containing parsed OXC ASTs, extracted bindings, and static-analysis metadata.

use std::cell::OnceCell;

use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;

use crate::ast::types::TemplateAst;
use crate::types::NodeId;

// Re-export Dynamism so `use self::types::*` in mod.rs picks it up.
pub use crate::utils::oxc::Dynamism;

use crate::utils::oxc::{
    vue::{VForWithBindings, VSlotWithBindings},
    BindingExtractionResult,
};

// ======================== ExpressionFlag ========================

/// Per-element expression analysis flags set by the OXC pass.
///
/// Each variant is an independent bit. Codegen combines these with the
/// syntax-layer [`PropFlag`](crate::ast::types::PropFlag) in O(1)
/// to make final patch-flag decisions.
///
/// Example: if `PropFlag` has `HasDynamicClass` and `ExpressionFlag` has
/// `StaticClassExpr`, codegen knows the `:class` binding is actually constant.
#[repr(u16)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ExpressionFlags {
    /// `:class` / `v-bind:class` expression is [`Dynamism::Static`].
    StaticClassExpr = 1,

    /// `:style` / `v-bind:style` expression is [`Dynamism::Static`].
    StaticStyleExpr = 1 << 1,

    /// `:key` / `v-bind:key` expression is [`Dynamism::Static`].
    StaticKeyExpr = 1 << 2,

    /// `v-if` / `v-else-if` condition expression is [`Dynamism::Static`].
    StaticCondition = 1 << 3,

    /// All interpolation children of this element are [`Dynamism::Static`].
    AllInterpolationsStatic = 1 << 4,
}

#[allow(dead_code)] // Used in tests
impl ExpressionFlags {
    /// Convert a single flag into an [`ExpressionFlag`] wrapper.
    #[inline(always)]
    pub const fn into_flag(self) -> ExpressionFlag {
        ExpressionFlag(self as u16)
    }

    /// Returns the canonical name for debugging.
    #[inline(always)]
    pub const fn name(self) -> &'static str {
        match self {
            ExpressionFlags::StaticClassExpr => "STATIC_CLASS_EXPR",
            ExpressionFlags::StaticStyleExpr => "STATIC_STYLE_EXPR",
            ExpressionFlags::StaticKeyExpr => "STATIC_KEY_EXPR",
            ExpressionFlags::StaticCondition => "STATIC_CONDITION",
            ExpressionFlags::AllInterpolationsStatic => "ALL_INTERPOLATIONS_STATIC",
        }
    }
}

/// Runtime wrapper for per-element expression analysis flags.
///
/// Same bitflag wrapper pattern as
/// [`PropFlag`](crate::ast::types::PropFlag) /
/// [`ChildrenFlag`](crate::ast::types::ChildrenFlag).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ExpressionFlag(pub u16);

impl ExpressionFlag {
    /// An empty flag (no expression analysis results).
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// True if no flags are set.
    #[cfg(test)]
    #[inline(always)]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Bitwise membership check.
    #[inline(always)]
    pub const fn contains(self, flag: ExpressionFlags) -> bool {
        (self.0 & (flag as u16)) != 0
    }

    /// Alias for [`contains`](Self::contains).
    #[inline(always)]
    pub const fn has(self, flag: ExpressionFlags) -> bool {
        self.contains(flag)
    }

    /// Add a flag.
    #[inline(always)]
    pub const fn add(self, flag: ExpressionFlags) -> Self {
        Self(self.0 | (flag as u16))
    }

    /// Remove a flag.
    #[inline(always)]
    pub const fn remove(self, flag: ExpressionFlags) -> Self {
        Self(self.0 & !(flag as u16))
    }
}

// ======================== Parsed expression ========================

/// OXC-parsed result for a single template expression.
///
/// Used for directive values, dynamic args, interpolation content,
/// and v-if/v-else-if conditions.
///
/// The expression AST retains **substring-relative spans** (not adjusted
/// to file positions). Use `offset` to convert: `file_pos = ast_span + offset`.
/// Bindings are already file-relative via [`BindingContext::base_offset`].
#[derive(Debug)]
pub struct OxcParsedExpression<'alloc> {
    /// Byte position of the expression slice in the original source.
    /// Add this to expression AST spans to get file-relative positions.
    pub offset: u32,

    /// Parsed OXC expression AST. Spans are substring-relative (0-based).
    /// `None` if parsing failed or the span was empty.
    pub expression: Option<Expression<'alloc>>,

    /// Parse errors, if any. Spans are **file-relative** (adjusted for reporting).
    #[allow(dead_code)] // Read by tests and downstream consumers
    pub errors: Option<Vec<OxcDiagnostic>>,

    /// Extracted identifier bindings. Positions are **file-relative**
    /// (adjusted via `BindingContext::base_offset`).
    pub bindings: Option<BindingExtractionResult<'alloc>>,

    /// Three-state dynamism classification.
    pub dynamism: Dynamism,
}

// ======================== Parsed prop ========================

/// OXC-parsed result for a single prop on an element.
///
/// Only created for props that need OXC parsing (directives with values,
/// dynamic args). Static attributes are not included.
#[derive(Debug)]
pub struct OxcParsedProp<'alloc> {
    /// Index into the element's `props: Vec<NodeProp>` for correlation.
    pub prop_index: usize,

    /// Parsed dynamic arg expression (e.g., `:[key]`).
    /// `None` for static args or non-dynamic-arg props.
    pub arg: Option<OxcParsedExpression<'alloc>>,

    /// Parsed directive value expression (e.g., `:id="expr"`).
    /// `None` for directives without a value or static attributes.
    pub exp: Option<OxcParsedExpression<'alloc>>,
}

// ======================== Parsed structural directives ========================

/// OXC-parsed v-for directive result.
#[derive(Debug)]
pub struct OxcParsedVFor<'alloc> {
    /// Parsed v-for expression with extracted locals and references.
    pub parsed: VForWithBindings<'alloc>,
}

/// OXC-parsed v-slot directive result.
#[derive(Debug)]
pub struct OxcParsedVSlot<'alloc> {
    /// Parsed v-slot expression with extracted locals and references.
    #[allow(dead_code)] // Populated for codegen consumers
    pub parsed: VSlotWithBindings<'alloc>,
}

// ======================== Parsed element ========================

/// All OXC-parsed data for a single element node.
///
/// Contains parsed expressions from structural directives (v-if, v-for, v-slot)
/// and regular directive props, plus the accumulated scope for children.
#[derive(Debug)]
pub struct OxcParsedElement<'alloc> {
    /// Parsed v-if / v-else-if condition. `None` for v-else or no condition.
    pub condition: Option<OxcParsedExpression<'alloc>>,

    /// Parsed v-for directive. `None` if no v-for.
    pub v_for: Option<OxcParsedVFor<'alloc>>,

    /// Parsed v-slot directive. `None` if no v-slot.
    #[allow(dead_code)] // Populated for codegen consumers
    pub v_slot: Option<OxcParsedVSlot<'alloc>>,

    /// Parsed regular props (only those with expressions to parse).
    /// Sparse — static attributes and value-less directives are skipped.
    /// Each entry's `prop_index` maps back into `ElementNode.props`.
    pub props: Vec<OxcParsedProp<'alloc>>,

    /// Dense correlation table from `ElementNode.props` index to the slot in
    /// [`props`] holding that prop's parsed expression, or `None` when the prop
    /// has nothing to parse (static attribute or value-less directive).
    ///
    /// Length equals the FULL `ElementNode.props.len()` (NOT the sparse
    /// [`props`] length), so [`OxcParsedElement::prop`] is an O(1) index rather
    /// than a linear scan over `prop_index`.
    ///
    /// [`props`]: OxcParsedElement::props
    pub prop_lookup: Vec<Option<u32>>,

    /// Additional ignored bindings from this element's v-for/v-slot locals.
    /// `None` means "same as parent — no locals added" (avoids Vec clone).
    /// `Some(vec)` includes inherited parent bindings + this element's locals.
    pub provided_locals: Option<Vec<&'alloc str>>,

    /// Per-element expression analysis flags (static class/style/key/condition).
    /// Codegen combines with `PropFlag` in O(1) for final patch-flag decisions.
    pub expression_flag: ExpressionFlag,
}

impl<'alloc> OxcParsedElement<'alloc> {
    /// O(1) lookup of the OXC-parsed prop for a given `ElementNode.props` index.
    ///
    /// Returns `None` when the prop carries no parsed expression (static attribute
    /// or value-less directive) or when `prop_index` is out of range. This is the
    /// indexed replacement for scanning `props` for a matching `prop_index`.
    #[inline]
    pub fn prop(&self, prop_index: usize) -> Option<&OxcParsedProp<'alloc>> {
        self.prop_lookup
            .get(prop_index)
            .copied()
            .flatten()
            .map(|slot| &self.props[slot as usize])
    }
}

// ======================== Node data enum ========================

/// OXC-parsed data for a single AST node.
///
/// Only `Element` and `Interpolation` nodes produce meaningful data.
/// `Text` and `Comment` nodes produce `None`.
#[derive(Debug)]
pub enum OxcNodeData<'alloc> {
    /// Parsed element with structural directives, props, and scope.
    /// Boxed to reduce enum size (OxcParsedElement is ~456 bytes vs ~128 for Interpolation).
    Element(Box<OxcParsedElement<'alloc>>),

    /// Parsed interpolation expression (`{{ expr }}`).
    Interpolation(OxcParsedExpression<'alloc>),

    /// Text and Comment nodes — no expressions to parse.
    None,
}

// ======================== Slot summary facts ========================

/// Classification of a single slot child for IDE strict-slot checking.
///
/// Mirrors the four child shapes the IDE strict-slot emitter understands. The
/// concrete source positions (tag-name span, text/interpolation start) are NOT
/// stored here — they are resolved on demand from [`SlotChildFact::node_id`], so
/// the fact stays a tiny `Copy` value and the byte offsets always come straight
/// from the AST node the consumer is emitting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotChildKind {
    /// A child Vue component (`<Foo>`); the constructor name is the tag name.
    Component,
    /// A child native HTML element (`<div>`); the tag name keys
    /// `HTMLElementTagNameMap`.
    HtmlElement,
    /// A non-whitespace text child.
    Text,
    /// An interpolation child (`{{ expr }}`).
    Interpolation,
}

/// One classified child inside a slot group.
///
/// Carries the originating [`NodeId`] so the strict-slot adapter resolves the
/// exact tag-name / text source span from the AST node when it emits, rather
/// than the slot scan eagerly slicing strings into the shared overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotChildFact {
    /// AST node this child was classified from.
    pub node_id: NodeId,
    /// Which strict-slot child shape this node is.
    pub kind: SlotChildKind,
}

/// A named slot group: the children a component receives for one slot name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotGroup {
    /// Slot name (`"default"`, `"header"`, …), in first-seen source order.
    pub name: String,
    /// Classified children for this slot, in source order. Always non-empty —
    /// empty groups are never recorded.
    pub children: Vec<SlotChildFact>,
}

/// Slot facts for one component element.
///
/// Built once per component, lazily, on the first strict-slot demand for that
/// component (see [`OxcParsedAst::slot_summary`]) and read by the IDE
/// strict-slot lane. The two fields intentionally model the two distinct
/// downstream rules:
///
/// - `groups` drives `strictRenderSlot` — only NON-EMPTY slot groups, with each
///   child classified by shape (a `<template #name>` contributes its inner
///   classified children; direct non-template content contributes the
///   `"default"` group).
/// - `provided_slot_names` drives `checkRequiredSlots` — every declared slot
///   name (including an empty `<template #name>`) plus a trailing `"default"`
///   when any non-template content (including whitespace text) is present. This
///   set is deliberately broader than `groups`: a component whose only content
///   is a named template plus surrounding whitespace records `"default"` here
///   yet has no default group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentSlotSummary {
    /// Non-empty strict-slot groups in first-seen source order.
    pub groups: Vec<SlotGroup>,
    /// Provided slot names for `checkRequiredSlots`, in source order.
    pub provided_slot_names: Vec<String>,
}

// ======================== Top-level result ========================

/// The complete OXC-parsed overlay for a [`TemplateAst`].
///
/// `data[node_id.0]` gives the OXC-parsed data for `ast.nodes[node_id.0]`.
/// Same length as `ast.nodes`.
#[derive(Debug)]
pub struct OxcParsedAst<'alloc> {
    pub data: Vec<OxcNodeData<'alloc>>,
    /// NodeId-aligned IDE slot summaries, each built lazily and independently on
    /// first demand: `slot_summaries[id.0]` resolves to `Some` for a static
    /// component eligible for slot checking and `None` otherwise (non-component
    /// nodes and dynamic `<component :is>`). The outer cell allocates the
    /// per-node slot vec once (no AST touch); each inner cell is filled the
    /// first time the IDE strict-slot lane reaches that component via
    /// [`OxcParsedAst::slot_summary`], so only components the lane actually
    /// reaches are scanned and the runtime VDOM/Vapor and SSR lanes pay nothing.
    /// Owned data (no allocator borrow), so it lives independently of the
    /// parsed-expression arena.
    slot_summaries: OnceCell<Vec<OnceCell<Option<ComponentSlotSummary>>>>,
}

impl<'alloc> OxcParsedAst<'alloc> {
    /// Wrap the per-node parsed data, with slot summaries unbuilt.
    pub fn new(data: Vec<OxcNodeData<'alloc>>) -> Self {
        Self {
            data,
            slot_summaries: OnceCell::new(),
        }
    }

    /// The IDE slot summary for the component at `id`, built once and memoized.
    ///
    /// Returns `Some` for a static, slot-checkable component and `None` for any
    /// other node. The first call for a given `id` scans that component's direct
    /// children and caches the result in its per-node cell; every later call for
    /// the same `id` returns the cached value without rescanning. `ast` and
    /// `source` are consulted only on the cold (first) call for each component.
    pub fn slot_summary(
        &self,
        id: NodeId,
        ast: &TemplateAst,
        source: &str,
    ) -> Option<&ComponentSlotSummary> {
        let cells = self.slot_summaries.get_or_init(|| {
            let mut cells = Vec::with_capacity(ast.nodes.len());
            cells.resize_with(ast.nodes.len(), OnceCell::new);
            cells
        });
        cells[id.0]
            .get_or_init(|| super::slot_summary::build_slot_summary(id, ast, source))
            .as_ref()
    }

    /// Iterate all [`OxcParsedExpression`] references in the AST.
    ///
    /// Yields every expression from interpolations, element conditions,
    /// and directive prop values/args. Useful for applying bulk transforms
    /// (e.g., TypeScript stripping) across all template expressions.
    pub fn iter_expressions(&self) -> impl Iterator<Item = &OxcParsedExpression<'alloc>> {
        self.data.iter().flat_map(|node| match node {
            OxcNodeData::Interpolation(expr) => {
                vec![expr]
            }
            OxcNodeData::Element(el) => {
                let mut exprs = Vec::new();
                if let Some(ref cond) = el.condition {
                    exprs.push(cond);
                }
                for prop in &el.props {
                    if let Some(ref arg) = prop.arg {
                        exprs.push(arg);
                    }
                    if let Some(ref exp) = prop.exp {
                        exprs.push(exp);
                    }
                }
                exprs
            }
            OxcNodeData::None => Vec::new(),
        })
    }
}

#[cfg(test)]
mod expression_flag_tests {
    use super::*;

    #[test]
    fn empty_flag() {
        let f = ExpressionFlag::empty();
        assert_eq!(f.0, 0);
        assert!(!f.has(ExpressionFlags::StaticClassExpr));
    }

    #[test]
    fn add_and_check() {
        let f = ExpressionFlag::empty()
            .add(ExpressionFlags::StaticClassExpr)
            .add(ExpressionFlags::StaticCondition);
        assert!(f.has(ExpressionFlags::StaticClassExpr));
        assert!(f.has(ExpressionFlags::StaticCondition));
        assert!(!f.has(ExpressionFlags::StaticStyleExpr));
    }

    #[test]
    fn remove_flag() {
        let f = ExpressionFlag::empty()
            .add(ExpressionFlags::StaticClassExpr)
            .add(ExpressionFlags::StaticStyleExpr)
            .remove(ExpressionFlags::StaticStyleExpr);
        assert!(f.has(ExpressionFlags::StaticClassExpr));
        assert!(!f.has(ExpressionFlags::StaticStyleExpr));
    }

    #[test]
    fn into_flag_round_trip() {
        assert!(ExpressionFlags::StaticClassExpr
            .into_flag()
            .has(ExpressionFlags::StaticClassExpr));
        assert!(ExpressionFlags::StaticStyleExpr
            .into_flag()
            .has(ExpressionFlags::StaticStyleExpr));
        assert!(ExpressionFlags::StaticKeyExpr
            .into_flag()
            .has(ExpressionFlags::StaticKeyExpr));
        assert!(ExpressionFlags::StaticCondition
            .into_flag()
            .has(ExpressionFlags::StaticCondition));
        assert!(ExpressionFlags::AllInterpolationsStatic
            .into_flag()
            .has(ExpressionFlags::AllInterpolationsStatic));
    }

    #[test]
    fn flag_names() {
        assert_eq!(ExpressionFlags::StaticClassExpr.name(), "STATIC_CLASS_EXPR");
        assert_eq!(ExpressionFlags::StaticStyleExpr.name(), "STATIC_STYLE_EXPR");
        assert_eq!(ExpressionFlags::StaticKeyExpr.name(), "STATIC_KEY_EXPR");
        assert_eq!(ExpressionFlags::StaticCondition.name(), "STATIC_CONDITION");
        assert_eq!(
            ExpressionFlags::AllInterpolationsStatic.name(),
            "ALL_INTERPOLATIONS_STATIC"
        );
    }
}
