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
//! - **[`vapor::VaporCodeGen`]** — Vapor mode. Emits `_template()` +
//!   DOM navigation + `_renderEffect()` with counter-based variable naming (`n0`, `t0`).
//! - **[`ssr::SsrCodeGen`]** — SSR mode. Emits `_push()` + `_ssrRenderAttrs()` for
//!   server-side string concatenation.
//!
//! All backends share significant logic through the sibling modules:
//!
//! - [`binding`] — Binding classification and `_ctx.`/`$setup.`/`.value` prefix resolution.
//! - [`walker`] — DFS tree walker that drives the [`TemplateCodeGen`] trait methods.
//! - [`types`] — [`CodeGenOutput`] accumulator and internal data structures.
//! - [`shared`] — Runtime helper constants, patch flag constants, and utility functions.

pub mod binding;
pub mod expression;
pub mod shared;
pub mod ssr;
pub mod types;
pub mod vapor;
pub mod vdom;
pub mod walker;

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use crate::ast::types::{CommentNode, ElementNode, InterpolationNode, TemplateAst, TextNode};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedAst, OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use self::binding::{BindingResolver, BindingType};
use self::types::{CodeGenOutput, TemplateImports};

/// Output mode for template code generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeGenMode {
    /// Virtual DOM render function output.
    Vdom,
    /// Vapor mode (direct DOM manipulation) output.
    Vapor,
    /// SSR mode — string-concatenation output (`_push()`, `_ssrRenderAttrs()`, etc.).
    Ssr,
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
    /// Props known to be const across all call sites (from cross-file analysis).
    /// Passed through to the `BindingResolver` to override reactivity level.
    pub const_props: Option<rustc_hash::FxHashSet<String>>,
    /// Whether the SFC has `<style scoped>`. When true and in SSR mode, the
    /// template codegen emits `_scopeId` parameter and appends `${_scopeId}`
    /// to element tags and component render calls.
    #[allow(dead_code)]
    pub has_scoped_style: bool,
    /// Whether to enable static optimizations:
    /// - `_cache[N]` wrapping for fully-static elements (`-1 /* CACHED */`)
    /// - `_hoisted_N` constants for static dynamic-props arrays
    ///
    /// Resolved from `CodegenOptions.hoist_static` (defaults to `true`).
    pub hoist_static: bool,
    /// Scope ID for scoped styles (e.g., `"data-v-a1b2c3d4"`).
    pub scope_id: String,
    /// Style `v-bind()` variables for SSR. Each entry is `(css_var_name, expression)`
    /// e.g. `("--a1b2c3d4-color", "color")`. Non-empty only for SSR mode: the
    /// template injects `const _cssVars = { style: { ... } }` and merges it into
    /// root `_ssrRenderAttrs` so CSS variables appear in the HTML (client uses
    /// `_useCssVars` instead).
    pub ssr_css_vars: Vec<(String, String)>,
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
            const_props: None,
            has_scoped_style: false,
            hoist_static: true,
            scope_id: String::new(),
            ssr_css_vars: Vec::new(),
        }
    }
}

/// Action returned by `enter_element` to control child traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // SkipChildren is part of the walker API, used by SSR codegen
pub enum WalkAction {
    /// Continue visiting children, then call `leave_element`.
    Continue,
    /// Skip children but still call `leave_element` (stack balance).
    SkipChildren,
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
    ///
    /// Returns [`WalkAction::Continue`] to visit children normally, or
    /// [`WalkAction::SkipChildren`] to skip them (e.g., for static hoisting).
    /// In both cases, `leave_element` is still called.
    fn enter_element(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> WalkAction;

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
/// Returns the categorized runtime helper imports so the caller can emit
/// `import { ... } from "vue"` and optionally `import { ... } from "vue/server-renderer"`.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn generate_template<'alloc>(
    ast: &TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    ct: &mut crate::code_transform::CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    bindings: FxHashMap<&'alloc str, BindingType>,
    options: &TemplateCodeGenOptions,
) -> TemplateImports {
    // Convert owned const_props (FxHashSet<String>) to arena-allocated (&'alloc str)
    // for the BindingResolver's lifetime.
    let const_props_alloc: Option<rustc_hash::FxHashSet<&'alloc str>> = options
        .const_props
        .as_ref()
        .map(|set| set.iter().map(|s| alloc.alloc_str(s) as &str).collect());

    let resolver = match options.mode {
        CodeGenMode::Vapor => {
            let mut r = BindingResolver::new_with_const_props(bindings, false, const_props_alloc);
            r.set_vapor(true);
            r
        }
        CodeGenMode::Ssr => {
            let mut r = BindingResolver::new_with_const_props(
                bindings,
                options.is_inline,
                const_props_alloc,
            );
            // Non-inline ssrRender(_ctx, _push, _parent, _attrs) has no $setup
            // param — bindings must go through the instance proxy as _ctx.*.
            r.set_ssr(true);
            r
        }
        CodeGenMode::Vdom => {
            BindingResolver::new_with_const_props(bindings, options.is_inline, const_props_alloc)
        }
    };
    let mut out = CodeGenOutput::new(alloc);

    match options.mode {
        CodeGenMode::Vdom => {
            let mut gen = vdom::VdomCodeGen::new(ast, oxc_ast, resolver, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
        CodeGenMode::Vapor => {
            let mut gen = vapor::VaporCodeGen::new(ast, resolver, source, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
        CodeGenMode::Ssr => {
            let mut gen = ssr::SsrCodeGen::new(ast, oxc_ast, resolver, options);
            walker::walk_template(ast, oxc_ast, source, &mut gen, &mut out);
        }
    }

    out.apply_to(ct)
}
