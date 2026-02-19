//! Output types for the AST-to-OXC template expression parsing pass.
//!
//! These types form a parallel data structure to [`TemplateAst`]: for each
//! `AstNode` in the arena, there is a corresponding [`OxcNodeData`] entry
//! containing parsed OXC ASTs, extracted bindings, and static-analysis metadata.

use oxc_ast::ast::Expression;
use oxc_diagnostics::OxcDiagnostic;

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
/// syntax-layer [`PropFlag`](crate::new_impl::ast::types::PropFlag) in O(1)
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
/// [`PropFlag`](crate::new_impl::ast::types::PropFlag) /
/// [`ChildrenFlag`](crate::new_impl::ast::types::ChildrenFlag).
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct ExpressionFlag(pub u16);

impl ExpressionFlag {
    /// An empty flag (no expression analysis results).
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create from a raw `u16` value.
    #[inline(always)]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// True if no flags are set.
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

    /// Returns true if any bits from `mask` are set.
    #[inline(always)]
    pub const fn has_any(self, mask: u16) -> bool {
        (self.0 & mask) != 0
    }

    /// Add a flag.
    #[inline(always)]
    pub const fn add(self, flag: ExpressionFlags) -> Self {
        Self(self.0 | (flag as u16))
    }

    /// Alias for [`add`](Self::add).
    #[inline(always)]
    pub const fn with(self, flag: ExpressionFlags) -> Self {
        self.add(flag)
    }

    /// Remove a flag.
    #[inline(always)]
    pub const fn remove(self, flag: ExpressionFlags) -> Self {
        Self(self.0 & !(flag as u16))
    }

    /// Alias for [`remove`](Self::remove).
    #[inline(always)]
    pub const fn without(self, flag: ExpressionFlags) -> Self {
        self.remove(flag)
    }

    /// Combine two values (bitwise OR).
    #[inline(always)]
    pub const fn union(self, other: ExpressionFlag) -> Self {
        Self(self.0 | other.0)
    }

    /// Clear all flags.
    #[inline(always)]
    pub const fn clear(self) -> Self {
        Self(0)
    }
}

// ---- top-level constants ----

pub const E_STATIC_CLASS_EXPR: ExpressionFlag =
    ExpressionFlag(ExpressionFlags::StaticClassExpr as u16);
pub const E_STATIC_STYLE_EXPR: ExpressionFlag =
    ExpressionFlag(ExpressionFlags::StaticStyleExpr as u16);
pub const E_STATIC_KEY_EXPR: ExpressionFlag = ExpressionFlag(ExpressionFlags::StaticKeyExpr as u16);
pub const E_STATIC_CONDITION: ExpressionFlag =
    ExpressionFlag(ExpressionFlags::StaticCondition as u16);
pub const E_ALL_INTERPOLATIONS_STATIC: ExpressionFlag =
    ExpressionFlag(ExpressionFlags::AllInterpolationsStatic as u16);

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
    pub v_slot: Option<OxcParsedVSlot<'alloc>>,

    /// Parsed regular props (only those with expressions to parse).
    /// Indexed by `prop_index` back into `ElementNode.props`.
    pub props: Vec<OxcParsedProp<'alloc>>,

    /// Accumulated ignored bindings this element provides to its children.
    /// Includes inherited parent bindings + this element's v-for/v-slot locals.
    pub provided_locals: Vec<&'alloc str>,

    /// Per-element expression analysis flags (static class/style/key/condition).
    /// Codegen combines with `PropFlag` in O(1) for final patch-flag decisions.
    pub expression_flag: ExpressionFlag,
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

// ======================== Top-level result ========================

/// The complete OXC-parsed overlay for a [`TemplateAst`].
///
/// `data[node_id.0]` gives the OXC-parsed data for `ast.nodes[node_id.0]`.
/// Same length as `ast.nodes`.
#[derive(Debug)]
pub struct OxcParsedAst<'alloc> {
    pub data: Vec<OxcNodeData<'alloc>>,
}

#[cfg(test)]
mod expression_flag_tests {
    use super::*;

    #[test]
    fn empty_flag() {
        let f = ExpressionFlag::empty();
        assert!(f.is_empty());
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
        assert!(!f.is_empty());
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
    fn union_flags() {
        let a = E_STATIC_CLASS_EXPR.union(E_STATIC_CONDITION);
        let b = E_STATIC_STYLE_EXPR.union(E_STATIC_KEY_EXPR);
        let combined = a.union(b);
        assert!(combined.has(ExpressionFlags::StaticClassExpr));
        assert!(combined.has(ExpressionFlags::StaticStyleExpr));
        assert!(combined.has(ExpressionFlags::StaticKeyExpr));
        assert!(combined.has(ExpressionFlags::StaticCondition));
    }

    #[test]
    fn clear_resets_all() {
        let f = E_STATIC_CLASS_EXPR
            .union(E_STATIC_STYLE_EXPR)
            .union(E_STATIC_KEY_EXPR)
            .clear();
        assert!(f.is_empty());
    }

    #[test]
    fn constants_match_flags() {
        assert!(E_STATIC_CLASS_EXPR.has(ExpressionFlags::StaticClassExpr));
        assert!(E_STATIC_STYLE_EXPR.has(ExpressionFlags::StaticStyleExpr));
        assert!(E_STATIC_KEY_EXPR.has(ExpressionFlags::StaticKeyExpr));
        assert!(E_STATIC_CONDITION.has(ExpressionFlags::StaticCondition));
        assert!(E_ALL_INTERPOLATIONS_STATIC.has(ExpressionFlags::AllInterpolationsStatic));
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

    #[test]
    fn with_and_without_aliases() {
        let f = ExpressionFlag::empty()
            .with(ExpressionFlags::StaticClassExpr)
            .with(ExpressionFlags::StaticKeyExpr)
            .without(ExpressionFlags::StaticKeyExpr);
        assert!(f.has(ExpressionFlags::StaticClassExpr));
        assert!(!f.has(ExpressionFlags::StaticKeyExpr));
    }
}
