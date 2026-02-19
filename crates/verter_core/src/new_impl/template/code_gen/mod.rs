//! Template code generation for the AST-based pipeline.
//!
//! Walks the [`TemplateAst`] + [`OxcParsedAst`] in DFS order, accumulating
//! transform operations into a [`CodeGenOutput`]. The caller applies these
//! to a [`CodeTransform`] in a single batch at the end.
//!
//! Supports both VDOM and Vapor output modes via the [`TemplateCodeGen`] trait.

pub mod binding;
pub mod shared;
pub mod types;
pub mod vapor;
pub mod vapor2;
pub mod vdom;
pub mod walker;

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::new_impl::ast::types::{
    CommentNode, ElementNode, InterpolationNode, TemplateAst, TextNode,
};
use crate::new_impl::syntax::types::RootNodeTemplate;
use crate::new_impl::template::oxc::types::{OxcParsedAst, OxcParsedElement, OxcParsedExpression};
use crate::new_impl::types::NodeId;

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
    Vapor2,
}

/// Options for template code generation.
#[derive(Debug, Clone)]
pub struct TemplateCodeGenOptions {
    pub mode: CodeGenMode,
    pub is_inline: bool,
    pub is_production: bool,
    pub comments: bool,
}

impl Default for TemplateCodeGenOptions {
    fn default() -> Self {
        Self {
            mode: CodeGenMode::Vdom,
            is_inline: false,
            is_production: false,
            comments: true,
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
pub fn generate_template<'alloc>(
    ast: &TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    ct: &mut crate::code_transform::CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    bindings: FxHashMap<&'alloc str, BindingType>,
    options: &TemplateCodeGenOptions,
) -> Vec<&'static str> {
    let resolver = BindingResolver::new(bindings, options.is_inline);
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
