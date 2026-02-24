//! Template code generation orchestrator for the AST-based pipeline.
//!
//! Walks the [`TemplateAst`] + [`OxcParsedAst`] in DFS order, accumulating
//! transform operations into a [`CodeGenOutput`]. The caller applies these
//! to a [`CodeTransform`] in a single batch at the end.
//!
//! Three backends implement the [`TemplateCodeGen`] trait:
//!
//! - **[`vdom::VdomCodeGen`]** — Default Vue 3 target. Emits `_createElementVNode` /
//!   `_createElementBlock` render function calls with patch flags and dynamic props.
//! - **[`vapor::VaporCodeGen`]** — First-generation Vapor mode. Emits `_template()` +
//!   DOM navigation + `_renderEffect()` with counter-based variable naming (`n0`, `t0`).
//! - **[`vapor2::Vapor2CodeGen`]** — Second-generation Vapor mode. Same output shape as
//!   Vapor but uses NodeId-based variable names (no counters) and a stateless
//!   scope-stack design.
//!
//! All three backends share significant logic through the sibling modules:
//!
//! - [`binding`] — Binding classification and `_ctx.`/`$setup.`/`.value` prefix resolution.
//! - [`walker`] — DFS tree walker that drives the [`TemplateCodeGen`] trait methods.
//! - [`types`] — [`CodeGenOutput`] accumulator and internal data structures.
//! - [`shared`] — Runtime helper constants, patch flag constants, and utility functions.

pub mod binding;
pub mod shared;
pub mod types;
pub mod vapor;
pub mod vapor2;
pub mod vdom;
pub mod walker;

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::ast::types::{CommentNode, ElementNode, InterpolationNode, TemplateAst, TextNode};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedAst, OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use self::binding::{BindingResolver, BindingType};
use self::types::CodeGenOutput;

/// Output mode for template code generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeGenMode {
    /// Virtual DOM render function output.
    Vdom,
    /// Vapor mode (direct DOM manipulation) output.
    Vapor,
    /// Vapor2 — stateless vapor codegen using NodeId-based variable naming.
    #[allow(dead_code)]
    Vapor2,
}

/// Options for template code generation.
#[derive(Debug, Clone)]
pub struct TemplateCodeGenOptions {
    pub mode: CodeGenMode,
    pub is_inline: bool,
    pub is_production: bool,
    pub comments: bool,
    /// When true, strip TypeScript syntax from directive expressions during codegen.
    /// Computed TS removal spans are applied inside `build_prefixed_expr()` to skip
    /// TS-only byte ranges when building resolved expression strings.
    pub force_js: bool,
    /// PascalCase component name derived from the SFC filename (e.g., "TokenBreakdown"
    /// from "TokenBreakdown.vue"). Used to detect recursive self-references in the template:
    /// when a component tag matches this name, the compiler emits
    /// `_resolveComponent("Name", true)` instead of `_resolveComponent("Name")`.
    pub self_name: String,
}

impl Default for TemplateCodeGenOptions {
    fn default() -> Self {
        Self {
            mode: CodeGenMode::Vdom,
            is_inline: false,
            is_production: false,
            comments: true,
            force_js: false,
            self_name: String::new(),
        }
    }
}

/// Trait for VDOM and Vapor template code generation backends.
///
/// The walker calls these methods in DFS order. Implementations accumulate
/// deferred operations into `CodeGenOutput` — no `CodeTransform` is passed.
pub trait TemplateCodeGen<'alloc> {
    /// Called when entering the template root.
    fn enter_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called when leaving the template root.
    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called when entering an element node (before children are visited).
    fn enter_element(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called when leaving an element node (after all children visited).
    fn leave_element(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called for a text node.
    fn visit_text(
        &mut self,
        id: NodeId,
        text: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called for an interpolation node.
    fn visit_interpolation(
        &mut self,
        id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );

    /// Called for a comment node.
    fn visit_comment(
        &mut self,
        id: NodeId,
        comment: &CommentNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    );
}

/// Generate template code from the AST + OXC overlay.
///
/// Accumulates all operations into `CodeGenOutput`, then batch-applies
/// to the `CodeTransform` in a single pass.
///
/// Returns the deduplicated list of runtime helper imports (e.g.
/// `["_createElementVNode", "_toDisplayString"]`) so the caller or a
/// downstream merger can emit the `import { ... } from "vue"` statement.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_template<'alloc>(
    ast: &TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    ct: &mut crate::code_transform::CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    bindings: FxHashMap<&'alloc str, BindingType>,
    options: &TemplateCodeGenOptions,
) -> Vec<&'static str> {
    let resolver = match options.mode {
        CodeGenMode::Vapor | CodeGenMode::Vapor2 => BindingResolver::new_vapor(bindings),
        CodeGenMode::Vdom => BindingResolver::new(bindings, options.is_inline),
    };
    let mut out = CodeGenOutput::new(alloc);

    match options.mode {
        CodeGenMode::Vdom => {
            let mut gen = vdom::VdomCodeGen::new(ast, resolver, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
        CodeGenMode::Vapor => {
            let mut gen = vapor::VaporCodeGen::new(ast, resolver, source, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
        CodeGenMode::Vapor2 => {
            let mut gen = vapor2::Vapor2CodeGen::new(ast, resolver, source, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
    }

    out.apply_to(ct)
}
