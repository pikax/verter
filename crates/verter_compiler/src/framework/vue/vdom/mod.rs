//! The Vue virtual-DOM render target.
//!
//! Given a template like `<div :class="cls">{{ msg }}</div>`, it produces a
//! render function body:
//!
//! ```js
//! return (_openBlock(), _createElementBlock("div", {
//!   class: _normalizeClass(_ctx.cls)
//! }, _toDisplayString(_ctx.msg), 3 /* TEXT, CLASS */))
//! ```
//!
//! The target records edits over the template's source positions: each
//! element's open and close tags become `_createElementVNode()` or
//! `_createElementBlock()` calls while the authored child expressions keep
//! their positions, so the source map stays exact. [`VueVdomExecutor`]
//! applies the recorded edits to a transform over the template window,
//! strips TypeScript when asked, freezes the transform and returns the
//! render function as a one-fragment plan; hoisted constants are a second
//! plan, the preamble.
//!
//! Key concepts:
//!
//! - **Child records**: the compile structure's resolved children of the
//!   parent being left; whitespace and branch gaps are decided there.
//! - **Patch flags**: computed per element from its dynamic bindings and
//!   emitted as trailing numeric arguments with dev-mode comments.
//! - **Block tree**: root elements and `v-if`/`v-for` use `_openBlock()` +
//!   `_createElementBlock()`; inner elements use `_createElementVNode()`.
//! - **Scope closes**: `v-if` and `v-for` push a `ScopeClose` entry that
//!   emits the ternary or `_renderList` suffix when the element is left.

mod block;
mod children;
mod comment;
mod component;
mod directives;
mod element;
mod events;
mod interpolation;
mod model;
mod plan;
mod props;
pub(crate) mod slot_content;
mod slots;
mod state;
mod text;

#[cfg(test)]
mod tests;

use verter_audit::observe::ObserveExec;
use verter_span::Span;

use crate::assembly::emit_plan::{EmitPlan, EmitPlanBuilder, MapMode};
use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeConditionKind, InterpolationNode, TagType,
    TextNode,
};
use crate::code_transform::{AnchorKind, CodeTransform, SegmentAnchor};
use crate::compile_request::demand::TargetKind;
use crate::emission::QualifiedSource;
use crate::framework::vue::plan::VueTarget;
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::{
    walk_template, VueChainRole, VueChildKind, VueCompileNodeId, VueTemplateVisitor, VueWalkControl,
};
use crate::framework::vue::target::{
    VueHelperSet, VueTargetInputs, VueTargetOutput, VueTargetRefusal,
};
use crate::framework_common::TemplateRenderExport;
use crate::parser::types::RootNodeTemplate;
use crate::target_plan::TargetExecutor;
use crate::template::binding::prefixed_expr::{prefixed_expr_plan, simple_expr_plan};
use crate::template::code_gen::shared::helpers::{self, VdomHelper};
use crate::template::code_gen::types::{ChildKind, ConditionChainRole, ScopeClose};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::types::NodeId;
use crate::work_ledger::CompilerWorkLedger;

use state::{VdomOut, VdomState};

/// Check if a directive name is `v-bind` (`:` or `v-bind`).
#[inline]
pub(crate) fn is_v_bind(name: &str) -> bool {
    name == ":" || name == "v-bind"
}

/// Check if a directive name is `v-on` (`@` or `v-on`).
#[inline]
pub(crate) fn is_v_on(name: &str) -> bool {
    name == "@" || name == "v-on"
}

/// Compiles a template to a render function that builds virtual nodes.
pub struct VueVdomExecutor;

impl TargetExecutor for VueVdomExecutor {
    const TARGET: TargetKind = TargetKind::ClientTree;
    type Inputs<'a> = &'a VueTargetInputs<'a>;
    type Output<'a> = VueTargetOutput<'a>;
    type Refusal = VueTargetRefusal;

    fn execute<'a, M: ObserveExec>(
        inputs: Self::Inputs<'a>,
        _ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<Self::Output<'a>, Self::Refusal> {
        let options = inputs.options;
        let resolver = VueBindingView::new(
            inputs.semantic.bindings(),
            options.inline,
            false,
            options
                .const_props
                .as_ref()
                .map(|props| props.iter().map(String::as_str).collect()),
            options.ref_bindable_imports.clone(),
        );
        let mut state = VdomState::new(inputs, resolver);
        walk_template(inputs.topology, inputs.structure, &mut state);
        state.finish::<M>()
    }
}

impl<'c> VdomState<'c> {
    /// The template window every edit lies in: the root open tag through the
    /// root close tag, or the content's end when the close tag is absent.
    fn template_window(&self) -> Span {
        let ast = self.ast;
        let root = &ast.root;
        let end = match root.tag_close.as_ref() {
            Some(tc) => tc.end,
            None => root
                .content
                .as_ref()
                .map(|c| c.end)
                .unwrap_or(root.tag_open.end),
        };
        Span::new(root.tag_open.start, end)
    }

    /// Apply the recorded edits to a transform over the template and hand
    /// the render function over as one
    /// fragment, with the hoisted constants as the preamble.
    fn finish<M: ObserveExec>(mut self) -> Result<VueTargetOutput<'c>, VueTargetRefusal> {
        let inputs = self.inputs;
        let options = inputs.options;
        let source = inputs.source.text;
        let out = self.take_out();
        let (edits, runtime) = self.finish_out(out);

        let mut transform = CodeTransform::<M>::for_window_in(
            source,
            self.template_window(),
            inputs.topology.len(),
            inputs.allocator,
            options.map_mode,
        )
        .map_err(VueTargetRefusal::Edit)?;
        edits
            .apply(&mut transform)
            .map_err(VueTargetRefusal::Edit)?;
        let fragment = transform
            .freeze(inputs.source.clone())
            .map_err(VueTargetRefusal::Edit)?;
        let mut plan = EmitPlanBuilder::new(options.map_mode);
        plan.push_fragment(fragment)
            .map_err(VueTargetRefusal::Emit)?;
        let render = plan.finish().map_err(VueTargetRefusal::Emit)?;

        let preamble = match self.module_preamble.take() {
            Some((text, anchors)) => Some(
                preamble_plan(&text, &anchors, &inputs.source, options.map_mode)
                    .map_err(VueTargetRefusal::Emit)?,
            ),
            None => None,
        };

        Ok(VueTargetOutput {
            target: VueTarget::Vdom,
            render,
            preamble,
            helpers: VueHelperSet {
                runtime,
                server: Vec::new(),
            },
            render_export: TemplateRenderExport::Render,
            diagnostics: Vec::new(),
        })
    }

    /// The parser payloads of a compile node: its parser identity, its
    /// element and its parsed expressions.
    fn element_of(
        &self,
        node: VueCompileNodeId,
    ) -> (NodeId, &'c ElementNode, Option<&'c OxcParsedElement<'c>>) {
        let id = self.inputs.structure.template_id(node);
        let topology = self.inputs.topology;
        let syntax = topology
            .syntax(id)
            .expect("an element-like node has a parser identity");
        let element = topology
            .element(id)
            .expect("an element-like node carries the parser element");
        (syntax, element, self.inputs.semantic.element_exprs(id))
    }

    fn syntax_of(&self, node: VueCompileNodeId) -> (VueTemplateNodeId, NodeId) {
        let id = self.inputs.structure.template_id(node);
        let syntax = self
            .inputs
            .topology
            .syntax(id)
            .expect("a visited node has a parser identity");
        (id, syntax)
    }
}

/// The hoisted constants as a plan: authored keys they embed stay anchored
/// to their source, everything else is generated.
fn preamble_plan<'c>(
    text: &str,
    anchors: &[SegmentAnchor],
    source: &QualifiedSource<'c>,
    mode: MapMode,
) -> Result<EmitPlan<'c>, crate::assembly::emit_plan::EmitPlanError> {
    let mut plan = EmitPlanBuilder::new(mode);
    let mut at = 0usize;
    for anchor in anchors {
        let start = anchor.content_offset as usize;
        let end = start + anchor.length as usize;
        if start < at || end > text.len() {
            continue;
        }
        if start > at {
            plan.push_unmapped(&text[at..start]);
        }
        match anchor.kind {
            // The bytes are the authored lexeme: they stay exact source.
            AnchorKind::Exact => plan.push_source(
                source,
                Span::new(anchor.source_pos, anchor.source_pos + anchor.length),
            )?,
            AnchorKind::Point => plan.push_anchored(&text[start..end], source, anchor.source_pos),
        }
        at = end;
    }
    if at < text.len() {
        plan.push_unmapped(&text[at..]);
    }
    plan.finish()
}

impl<'c> VueTemplateVisitor for VdomState<'c> {
    fn enter_template(&mut self) {
        let (ast, source) = (self.ast, self.source);
        let root = &ast.root;
        let mut out = self.take_out();
        self.enter_template_at(root, source, &mut out);
        self.restore_out(out);
    }

    fn leave_template(&mut self) {
        let (ast, source) = (self.ast, self.source);
        let root = &ast.root;
        let mut out = self.take_out();
        self.leave_template_at(root, source, &mut out);
        self.restore_out(out);
    }

    fn enter_element(
        &mut self,
        node: VueCompileNodeId,
        _kind: VueTemplateNodeKind,
    ) -> VueWalkControl {
        let (id, element, oxc) = self.element_of(node);
        let source = self.source;
        let mut out = self.take_out();
        let control = self.enter_element_at(id, element, oxc, source, &mut out);
        self.restore_out(out);
        control
    }

    fn leave_element(&mut self, node: VueCompileNodeId, _kind: VueTemplateNodeKind) {
        let (id, element, oxc) = self.element_of(node);
        let source = self.source;
        let mut out = self.take_out();
        self.leave_element_at(id, element, oxc, source, &mut out);
        self.restore_out(out);
    }

    fn visit_text(&mut self, node: VueCompileNodeId) {
        let (ast, source) = (self.ast, self.source);
        let (_, syntax) = self.syntax_of(node);
        let AstNodeKind::Text(text) = &ast.nodes[syntax.0].kind else {
            return;
        };
        let mut out = self.take_out();
        self.visit_text_at(syntax, text, source, &mut out);
        self.restore_out(out);
    }

    fn visit_interpolation(&mut self, node: VueCompileNodeId) {
        let (ast, source) = (self.ast, self.source);
        let (id, syntax) = self.syntax_of(node);
        let AstNodeKind::Interpolation(interp) = &ast.nodes[syntax.0].kind else {
            return;
        };
        let Some(expression) = self.inputs.semantic.interpolation(id) else {
            return;
        };
        let mut out = self.take_out();
        self.visit_interpolation_at(syntax, interp, expression, source, &mut out);
        self.restore_out(out);
    }

    fn visit_comment(&mut self, node: VueCompileNodeId) {
        let (ast, source) = (self.ast, self.source);
        let (_, syntax) = self.syntax_of(node);
        let AstNodeKind::Comment(comment) = &ast.nodes[syntax.0].kind else {
            return;
        };
        let mut out = self.take_out();
        self.visit_comment_at(syntax, comment, source, &mut out);
        self.restore_out(out);
    }
}

impl<'c> VdomState<'c> {
    pub(super) fn enter_template_at(
        &mut self,
        root: &RootNodeTemplate,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        self.settle_children(out, None);
        // Whether the template has a single effective root: a chain counts
        // once, and whitespace or a disabled comment never counts. This
        // decides whether root-level elements use block helpers
        // (_createElementBlock / _createBlock) vs regular helpers.
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let structure = self.inputs.structure;
        let root_node = structure.compile_id(self.inputs.topology.root());
        let effective = structure
            .children(root_node)
            .iter()
            .filter(|child| {
                child.chain != VueChainRole::Continuation
                    && !matches!(
                        child.kind,
                        VueChildKind::WhitespaceNewline | VueChildKind::WhitespaceSpace
                    )
            })
            .count();
        self.single_root = effective == 1;
        // Reserve `_cache[N]` handler-caching slots before the main walk
        // touches `self.cache_index` — see `VdomNodePlan::handler_plans`'s
        // doc comment.
        self.reserve_handler_caches(root_children, source);
        // Static-array group caching (official `cacheStatic`, a separate
        // pass) numbers after handler caching (the main-transform pass) —
        // see both reservation maps' doc comments. `v-memo` caching still
        // runs interleaved with individual static caching during the main
        // walk, unchanged; no current corpus case combines it with either
        // pre-pass, so their exact relative order there is not modeled.
        self.reserve_array_group_caches(root_children, true);
        // Open tag overwrite is deferred to leave_template where we have
        // full context (children count, v-if status) to emit the correct
        // combined prefix (function signature + return + openBlock).
    }

    pub(super) fn leave_template_at(
        &mut self,
        root: &RootNodeTemplate,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let children = self.build_child_records(root_children, source);

        // The compile structure resolved root whitespace and branch gaps;
        // `settle_children` already removed what it dropped.

        // Hoisted constants (`const _hoisted_1 = ["id"]`) are module-level
        // declarations: they leave the render function as the preamble
        // plan, each constant's authored anchors shifted to its position in
        // the preamble text.
        if !self.hoisted_constants.is_empty() {
            let mut anchors: Vec<SegmentAnchor> = Vec::new();
            let mut preamble = String::with_capacity(self.hoisted_constants.len() * 30);
            for (i, (constant, constant_anchors)) in self.hoisted_constants.iter().enumerate() {
                preamble.push_str("const _hoisted_");
                preamble.push_str(&(i + 1).to_string());
                preamble.push_str(" = ");
                let base = preamble.len() as u32;
                preamble.push_str(constant);
                preamble.push('\n');
                anchors.extend(constant_anchors.iter().map(|a| SegmentAnchor {
                    content_offset: base + a.content_offset,
                    ..*a
                }));
            }
            preamble.push('\n');
            self.module_preamble = Some((preamble, anchors));
        }

        // Function signature prefix. Official `@vue/compiler-core` emits the
        // full `(_ctx, _cache, $props, $setup, $data, $options)` form only when
        // binding metadata exists (a script block) and the template is not
        // inlined; template-only SFCs get the 2-param `(_ctx, _cache)` form —
        // their bodies reference only `_ctx`/`_cache` (no bindings to route
        // through `$props`/`$setup`/`$data`/`$options`).
        let fn_sig = if self.options.is_inline {
            "return (_ctx,_cache) => {\n"
        } else if self.options.has_script {
            "function render(_ctx, _cache, $props, $setup, $data, $options) {\n"
        } else {
            "function render(_ctx, _cache) {\n"
        };

        // Build resolved component declarations (inside the function body)
        // e.g., `const _component_el_button = _resolveComponent("el-button")\n`
        let resolved_comp_preamble = if self.resolved_components.is_empty() {
            String::new()
        } else {
            let mut s = String::with_capacity(self.resolved_components.len() * 60);
            for reference in &self.resolved_components {
                let (tag, var, is_self_ref) = (
                    &reference.tag,
                    &reference.variable,
                    reference.self_reference,
                );
                s.push_str("const ");
                s.push_str(var);
                s.push_str(" = _resolveComponent(\"");
                s.push_str(tag);
                if is_self_ref {
                    s.push_str("\", true)\n");
                } else {
                    s.push_str("\")\n");
                }
            }
            s
        };

        // Function signature, then the resolved component declarations.
        let full_prefix = {
            let mut s = fn_sig.to_string();
            if !resolved_comp_preamble.is_empty() {
                s.push_str(&resolved_comp_preamble);
                s.push('\n');
            }
            s
        };

        // Determine close tag region
        let (close_start, close_end) = match root.tag_close.as_ref() {
            Some(tc) => (tc.start, tc.end),
            None => {
                let pos = root
                    .content
                    .as_ref()
                    .map(|c| c.end)
                    .unwrap_or(root.tag_open.end);
                (pos, pos)
            }
        };

        // Count effective roots: v-if chains collapse into a single root.
        let effective_count = children
            .iter()
            .filter(|c| c.condition != Some(ConditionChainRole::Continuation))
            .count();

        let tag_open = &root.tag_open;

        match effective_count {
            0 => {
                // Empty template — overwrite everything
                let mut buf = String::with_capacity(full_prefix.len() + 16);
                buf.push_str(&full_prefix);
                buf.push_str("return null\n}");
                out.overwrite_or_root_prefix(tag_open.start, close_end, &buf);
            }
            1 => {
                let child = &children[0];
                let is_v_if = child.condition == Some(ConditionChainRole::Start);

                if is_v_if {
                    // Root-level v-if chain — overwrite up to child.start with
                    // the function signature + "return ", then emit the condition
                    // prefix as a separate source-mapped prepend.
                    let mut prefix = String::with_capacity(full_prefix.len() + 32);
                    prefix.push_str(&full_prefix);
                    prefix.push_str("return ");
                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);

                    // Emit the v-if condition prefix with per-segment source mapping.
                    if let Some(ref cond) = child.condition_prefix {
                        children::emit_condition_prefix_mapped(out, child.start, cond);
                    }

                    // Emit condition prefixes for continuation children
                    // (v-else-if elements in the chain) with source mapping.
                    for cont in children.iter().skip(1) {
                        if let Some(ref cond) = cont.condition_prefix {
                            children::emit_condition_prefix_mapped(out, cont.start, cond);
                        }
                    }

                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
                } else if self.root_element_has_v_memo(root_children, source) {
                    // v-memo root: `_withMemo(..., () => (_openBlock(), …))`
                    // owns its openBlock inside the memo factory (emitted by
                    // leave_element), so leave_template must NOT add an outer
                    // `(_openBlock(), …)` wrapper — just `return`.
                    let mut prefix = String::with_capacity(full_prefix.len() + 8);
                    prefix.push_str(&full_prefix);
                    prefix.push_str("return ");
                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
                } else if self.root_element_has_directives_wrap(root_children, source) {
                    // Directives-wrapped root (v-show, native v-model, a
                    // custom directive) with no v-if/v-for of its own:
                    // `process_element_leave` already nests its own
                    // `(_openBlock(), …)` inside `_withDirectives(…)` — see
                    // `root_element_has_directives_wrap`'s doc comment.
                    // leave_template must NOT add a second outer wrapper.
                    let mut prefix = String::with_capacity(full_prefix.len() + 8);
                    prefix.push_str(&full_prefix);
                    prefix.push_str("return ");
                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
                    out.overwrite_or_root_suffix(close_start, close_end, "\n}");
                } else {
                    // Single root — block root with _openBlock + _createElementBlock
                    out.add_vdom_import(VdomHelper::OpenBlock);
                    let mut prefix = String::with_capacity(full_prefix.len() + 24);
                    prefix.push_str(&full_prefix);
                    prefix.push_str("return (_openBlock(), ");
                    out.overwrite_or_root_prefix(tag_open.start, child.start, &prefix);
                    out.overwrite_or_root_suffix(close_start, close_end, ")\n}");
                }
            }
            _ => {
                // Multi-root — wrap in Fragment
                out.add_vdom_import(VdomHelper::OpenBlock);
                out.add_vdom_import(VdomHelper::CreateElementBlock);
                out.add_vdom_import(VdomHelper::Fragment);

                // Prefix: function sig + return + openBlock + Fragment + array open.
                let mut prefix = String::with_capacity(full_prefix.len() + 80);
                prefix.push_str(&full_prefix);
                prefix.push_str("return (_openBlock(), _createElementBlock(_Fragment, null, [");
                out.overwrite_or_root_prefix(tag_open.start, children[0].start, &prefix);

                // Delegate to wrap_array_text_runs for separators AND text
                // wrapping. This handles:
                // - Comma separators between array items
                // - _createTextVNode() wrapping for text/interpolation runs
                // - Condition prefix emission (v-if/v-else-if)
                // - v-for prefix ordering (comma at prev_item_end)
                children::add_children_separators_array(
                    &children,
                    out,
                    &self.options,
                    source,
                    self.ast,
                    root_children,
                );

                // Close fragment + render function.
                //
                // Official Vue flags a root Fragment `STABLE_FRAGMENT |
                // DEV_ROOT_FRAGMENT` (2112) when it exists ONLY because comments
                // sit beside a SINGLE logical non-comment root — so the runtime
                // filters to the real root for fallthrough / HMR. A v-if/v-else
                // chain counts as ONE logical root (its continuation arms do not
                // add). Two or more real roots stay a plain STABLE_FRAGMENT (64).
                let has_comment = children.iter().any(|c| c.kind == ChildKind::Comment);
                let logical_root_count = children
                    .iter()
                    .filter(|c| {
                        !matches!(
                            c.kind,
                            ChildKind::Comment
                                | ChildKind::WhitespaceNewline
                                | ChildKind::WhitespaceSpace
                        ) && c.condition != Some(ConditionChainRole::Continuation)
                    })
                    .count();
                let frag_flag = if has_comment && logical_root_count == 1 {
                    helpers::PATCH_STABLE_FRAGMENT | helpers::PATCH_DEV_ROOT_FRAGMENT
                } else {
                    helpers::PATCH_STABLE_FRAGMENT
                };
                let flag_str =
                    helpers::format_patch_flag(frag_flag, self.options.is_production, |s| {
                        out.alloc_str(s)
                    });
                let mut close_buf = String::with_capacity(32);
                close_buf.push_str("\n], ");
                close_buf.push_str(flag_str);
                close_buf.push_str("))\n}");
                out.overwrite_or_root_suffix(close_start, close_end, &close_buf);
            }
        }
    }

    pub(super) fn enter_element_at(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) -> VueWalkControl {
        helpers::debug_assert_element_bounds(
            source,
            element.tag_open.start,
            element.tag_open.end,
            element.tag_open.name_end,
        );

        // Process structural directives: v-if/v-else-if/v-else, v-for
        if let Some(condition) = &element.v_condition {
            let mut close = directives::condition_scope_close(&condition.kind);
            // Adjust scope close based on whether there's a continuation sibling.
            //
            // If this v-if has a v-else-if/v-else continuation after it,
            // downgrade IfTernary → ElseIfTernary so the scope close emits
            // ` : ` instead of the comment fallback.
            //
            // Conversely, if a v-else-if has NO continuation after it (end of
            // chain without v-else), upgrade ElseIfTernary → IfTernary so the
            // scope close emits `_createCommentVNode("v-if", true)` as the
            // false branch of the ternary.
            let has_next = self.has_next_condition_sibling(id);
            if close == ScopeClose::IfTernary && has_next {
                close = ScopeClose::ElseIfTernary;
            } else if close == ScopeClose::ElseIfTernary && !has_next {
                close = ScopeClose::IfTernary;
            }
            directives::collect_scope_imports(&close, self.options.element_block(), out);

            // Pre-compute resolved condition prefix using OXC binding data.
            // build_child_records only sees AST data (no OXC), so we resolve
            // binding prefixes here where OXC data is available.
            if matches!(
                condition.kind,
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf
            ) {
                let raw_expr = helpers::extract_directive_value(&condition.prop, source);
                let value_start = condition.prop.value_start.unwrap_or(0);
                let map_mode = self.inputs.options.map_mode;
                let resolved = if let Some(oxc_el) = oxc {
                    if let Some(oxc_cond) = &oxc_el.condition {
                        let ts_skip = if self.options.force_js {
                            oxc_cond
                                .expression
                                .as_ref()
                                .map(|e| {
                                    crate::strip_types::typescript::collect_ts_removal_spans(e)
                                })
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        prefixed_expr_plan(
                            raw_expr,
                            value_start,
                            oxc_cond,
                            &self.resolver,
                            &ts_skip,
                            map_mode,
                        )
                    } else {
                        simple_expr_plan(&self.resolver, raw_expr, value_start, map_mode)
                    }
                } else {
                    simple_expr_plan(&self.resolver, raw_expr, value_start, map_mode)
                };
                self.resolved_condition_prefixes.insert(id.0, resolved);
            }

            // NOTE: condition prefix `(expr) ? ` is NOT prepended here.
            // It is emitted by the parent's separator logic (build_child_records
            // stores it in ChildRecord.condition_prefix) to ensure correct
            // ordering relative to comma separators.

            // Both structural directives on ONE element (`v-else v-for`,
            // reka-ui VisuallyHiddenInput): the condition stays OUTER
            // (official v-if-over-v-for priority) and the branch value is
            // the `_renderList` fragment — without it, loop aliases in the
            // branch are free identifiers (ReferenceError at runtime).
            if let Some(v_for) = &element.v_for {
                let is_keyed = element.props.iter().any(|p| {
                    if !p.is_directive {
                        return false;
                    }
                    if let (Some(as_), Some(ae)) = (p.arg_start, p.arg_end) {
                        &source[as_ as usize..ae as usize] == "key"
                    } else {
                        false
                    }
                });
                // v-for on a conditional branch: the outer `_renderList`
                // Fragment carries the if-branch `{ key: n }` (official Vue puts
                // the branch key on the Fragment even when items have their own
                // `:key`).
                let branch_key = directives::condition_branch_index(self.ast, id);
                let (prefix, _for_close, iterable_src) = directives::build_for_prefix(
                    v_for,
                    source,
                    is_keyed,
                    oxc,
                    &self.resolver,
                    branch_key,
                    None,
                    self.options.element_block(),
                    self.options.force_js,
                );
                let condition = match close {
                    ScopeClose::IfTernary => {
                        crate::template::code_gen::types::ConditionBranchClose::IfTernary
                    }
                    ScopeClose::ElseIfTernary => {
                        crate::template::code_gen::types::ConditionBranchClose::ElseIfTernary
                    }
                    _ => crate::template::code_gen::types::ConditionBranchClose::Else,
                };
                let combined = ScopeClose::ForInCondition {
                    is_keyed,
                    condition,
                };
                directives::collect_scope_imports(&combined, self.options.element_block(), out);
                self.v_for_prefixes.push(Some((prefix, iterable_src)));
                self.scope_closes.push(Some(combined));
            } else {
                self.scope_closes.push(Some(close));
                self.v_for_prefixes.push(None);
            }
        } else if let Some(v_for) = &element.v_for {
            // Check if element has a :key prop
            let is_keyed = element.props.iter().any(|p| {
                if !p.is_directive {
                    return false;
                }
                if let (Some(as_), Some(ae)) = (p.arg_start, p.arg_end) {
                    &source[as_ as usize..ae as usize] == "key"
                } else {
                    false
                }
            });
            // v-memo inside v-for → per-item memoization topology (official Vue:
            // `_renderList(src, (i, __, ___, _cached) => { const _memo = deps;
            // if (_cached && _cached.key === KEY && _isMemoSame(...)) return
            // _cached; const _item = <vnode>; _item.memo = _memo; return _item },
            // _cache, N)`), NOT a single global `_cache[N]`.
            let memo_deps = element::resolve_v_memo_deps(
                element,
                source,
                oxc,
                &self.resolver,
                self.options.force_js,
            );
            let (prefix, close, iterable_src) = if let Some(deps) = &memo_deps {
                let key_expr = element::resolve_v_for_key(
                    element,
                    source,
                    oxc,
                    &self.resolver,
                    self.options.force_js,
                )
                .unwrap_or_else(|| "undefined".to_string());
                let memo_idx = self.cache_index;
                self.cache_index += 1;
                let (prefix, close, iterable_src) = directives::build_for_prefix(
                    v_for,
                    source,
                    is_keyed,
                    oxc,
                    &self.resolver,
                    None,
                    Some((deps, &key_expr)),
                    self.options.element_block(),
                    self.options.force_js,
                );
                // Memo close replaces the normal `_renderList` fragment close.
                let flag = if is_keyed {
                    if self.options.is_production {
                        "128"
                    } else {
                        "128 /* KEYED_FRAGMENT */"
                    }
                } else if self.options.is_production {
                    "256"
                } else {
                    "256 /* UNKEYED_FRAGMENT */"
                };
                let suffix = format!(
                    "\n_item.memo = _memo\nreturn _item\n}}, _cache, {memo_idx}), {flag}))"
                );
                self.memo_for_suffixes.insert(id.0, suffix);
                out.add_vdom_import(VdomHelper::IsMemoSame);
                out.add_vdom_import(VdomHelper::WithMemo);
                (prefix, close, iterable_src)
            } else {
                directives::build_for_prefix(
                    v_for,
                    source,
                    is_keyed,
                    oxc,
                    &self.resolver,
                    None,
                    None,
                    self.options.element_block(),
                    self.options.force_js,
                )
            };
            directives::collect_scope_imports(&close, self.options.element_block(), out);
            // NOTE: v-for prefix is NOT prepended here. It is stored and
            // included in the open tag overwrite by process_element_leave.
            // This ensures correct ordering when a sibling text node's
            // closing marker is at the same position as this element's start.
            self.v_for_prefixes.push(Some((prefix, iterable_src)));
            self.scope_closes.push(Some(close));
        } else {
            self.scope_closes.push(None);
            self.v_for_prefixes.push(None);
        }

        // Track slot context: components, <template v-slot>, and native <slot>
        // outlet fallback content create slot contexts where children should
        // use grouped caching instead of individual _cache[N] wrapping.
        // Teleport/KeepAlive take raw VNode-array children (not slot
        // objects), so they stay OUT of slot context. A <slot> outlet's
        // fallback content is itself a slot function body (`() => [...]`,
        // official `buildSlots`/fallback compilation), so it takes the same
        // grouped-caching path as any other slot content.
        let tag_name =
            &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
        let is_slot_parent = (element.tag_type.is_component()
            && !helpers::is_raw_children_builtin(tag_name))
            || (element.tag_type == TagType::Template && element.v_slot.is_some())
            || element.tag_type.is_slot_outlet();
        self.in_slot_context_stack.push(is_slot_parent);

        // Reserve this element's own `_hoisted_N` slot (if any) BEFORE
        // descending into children — see `VdomNodePlan::hoist`.
        if self.options.hoist_static {
            self.try_reserve_element_hoist(id, element, oxc, source);
        }

        VueWalkControl::Continue
    }

    pub(super) fn leave_element_at(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        // Pop the slot context stack (pushed in enter_element).
        self.in_slot_context_stack.pop();

        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );

        // Handle <slot> outlet: generates _renderSlot(_ctx.$slots, "name")
        if el.tag_type.is_slot_outlet() {
            let record = self.process_slot_outlet(el, oxc, source, out);
            // Apply v-for prefix (e.g., `_renderList(items, (item) => {\nreturn `).
            if let Some((prefix, iterable_src)) = self.v_for_prefixes.pop().flatten() {
                if let Some(src_pos) = iterable_src {
                    out.prepend_alloc_mapped(record.start, src_pos, &prefix);
                } else {
                    out.prepend_alloc(record.start, &prefix);
                }
            }
            // Apply scope close suffix for structural directives (v-if/v-for).
            if let Some(scope_close) = self.scope_closes.pop().flatten() {
                self.prepend_scope_close(out, record.end, &scope_close);
            }
            return;
        }

        // Handle <template v-slot:name>: generates slot function body
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let _record = self.process_template_slot(el, source, out);
            // Pop scope closes. For conditional template slots (v-if on v-slot),
            // the scope close is intentionally discarded here — the condition
            // is handled by the parent's leave_component_with_slots using
            // ChildRecord condition data and _createSlots wrapping.
            self.scope_closes.pop();
            self.v_for_prefixes.pop();
            return;
        }

        // Synthetic v-if branch key (official Vue injects `{ key: n }` on a
        // conditional-branch root that has no explicit `:key`). Only for a plain
        // conditional branch — when v-for also applies, the key rides the outer
        // `_renderList` Fragment built in enter_element.
        let injected_key = if el.v_condition.is_some()
            && el.v_for.is_none()
            && !directives::element_has_vnode_key(el, source)
        {
            directives::condition_branch_index(self.ast, _id)
        } else {
            None
        };

        // Handle <template v-if> / <template v-for>: renders as Fragment, not
        // as a <template> element. These are transparent structural wrappers
        // whose children become the Fragment's children.
        if el.tag_type == TagType::Template
            && el.v_slot.is_none()
            && (el.v_condition.is_some() || el.v_for.is_some())
        {
            self.leave_template_fragment(el, source, out, injected_key);
            return;
        }

        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);

        // Determine if this element is at a block-tree root position.
        // Block roots use _createElementBlock/_createBlock (with _openBlock())
        // instead of _createElementVNode/_createVNode.
        //
        // Teleport/KeepAlive are ALWAYS block roots — official Vue emits
        // `(_openBlock(), _createBlock(_Teleport, …))` at ANY nesting depth,
        // not just at a single-root/v-if/v-for position.
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let raw_children_builtin = helpers::is_raw_children_builtin(tag_name);
        let is_root_child = self.ast.nodes[_id.0].parent.is_none();
        let is_single_template_root = is_root_child && self.single_root;

        // v-memo: the element's vnode factory is wrapped in
        // `_withMemo([deps], () => <vnode>, _cache, N)`. Native elements are
        // block-forced (official Vue: the memoized factory returns a block);
        // components keep their normal topology (a nested childless/default-slot
        // component memo stays `_createVNode`). A v-memo block owns its openBlock
        // INSIDE the factory, so the single-root openBlock is suppressed in
        // leave_template for a v-memo root. (v-for + v-memo uses per-item cache
        // topology and is not handled on this path.)
        let memo_deps = if el.v_for.is_none() {
            element::resolve_v_memo_deps(el, source, oxc, &self.resolver, self.options.force_js)
        } else {
            None
        };
        let native_memo = memo_deps.is_some() && !el.tag_type.is_component();

        // Dynamic `<component :is>` is ALWAYS a block root in official Vue —
        // `(_openBlock(), _createBlock(_resolveDynamicComponent(...)))` at any
        // nesting depth, including multi-root fragment children without
        // v-if/v-for (reka-ui CheckboxRoot pattern): the resolved target can
        // be any shape, so the runtime needs the block boundary.
        let is_dynamic_component = component::is_dynamic_component_tag(el, source);
        // A v-for item on a document-order-stable source (see
        // `directives::is_stable_for_source`) is NOT forced into its own
        // block — official's `shouldUseBlock = !isStableFragment ||
        // childBlock.isBlockRequired`. `branch_key` is irrelevant here (only
        // the plain v-for path, no v-if combined) since a v-if+v-for
        // element already has `v_condition.is_some()` forcing block-root
        // through the FIRST disjunct regardless.
        let v_for_is_stable =
            el.v_for.is_some() && directives::is_stable_for_source(oxc, &self.resolver);
        let is_block_root = el.v_condition.is_some()
            || (el.v_for.is_some() && !v_for_is_stable)
            || is_single_template_root
            || raw_children_builtin
            || native_memo
            || is_dynamic_component;
        // Local `_openBlock()`: v-if/v-for branches always; a raw-children
        // built-in or a dynamic component whenever it is NOT the sole single
        // template root (the single-root open block is provided once by
        // leave_template); a v-memo block owns its own openBlock inside the
        // memo factory.
        let force_open_block = (raw_children_builtin && !is_single_template_root)
            || (memo_deps.is_some() && is_block_root)
            || (is_dynamic_component && !is_single_template_root);

        // Emit the `_withMemo([deps], () => ` prefix and `, _cache, N)` suffix
        // around the whole vnode expression. Applied before dispatch so the
        // leave path's overwrites compose inside the wrapper.
        if let Some(deps) = &memo_deps {
            let memo_idx = self.cache_index;
            self.cache_index += 1;
            let vnode_end = el
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(el.tag_open.end);
            out.add_vdom_import(VdomHelper::WithMemo);
            out.prepend_alloc(el.tag_open.start, &format!("_withMemo({deps}, () => "));
            out.prepend_alloc(vnode_end, &format!(", _cache, {memo_idx})"));
        }

        // Handle component with slot children: wrap in slot object instead of array.
        // Teleport/KeepAlive are excluded — they take raw array children below.
        if el.tag_type.is_component()
            && !raw_children_builtin
            && self.has_slot_children(el_children)
        {
            self.leave_component_with_slots(
                _id,
                el,
                oxc,
                el_children,
                source,
                out,
                is_block_root,
                force_open_block,
                injected_key,
            );
            return;
        }

        // Handle component with implicit default slot (non-slot children).
        // Teleport/KeepAlive fall through to the element path (raw array children).
        if el.tag_type.is_component() && !raw_children_builtin && !el_children.is_empty() {
            self.leave_component_with_default_slot(
                _id,
                el,
                oxc,
                el_children,
                source,
                out,
                is_block_root,
                force_open_block,
                injected_key,
            );
            return;
        }

        let mut children = self.build_child_records(el_children, source);
        // Take the reusable buffer, use it, then put it back (std::mem::take pattern)
        let mut buf = std::mem::take(&mut self.buf);
        let v_for_prefix = self.v_for_prefixes.pop().flatten();

        // Determine if this static element should be cached via _cache[N].
        // Skip caching children whose parent is also fully static — the parent's
        // cache encompasses them, so individual caching is redundant.
        // Also skip individual caching when inside a slot context — slot-level
        // cache grouping handles it instead.
        // Covered by a parent's `VdomNodePlan::array_group` entry (this
        // element's WHOLE sibling group is cached as one array) — never an
        // individual `cache_idx`, same as the slot-context case below.
        let plan_key = self.plan_key(_id);
        let array_grouped = self
            .plans
            .get(plan_key)
            .is_some_and(|plan| plan.array_grouped);
        let cache_idx = if self.options.hoist_static
            && el.is_fully_static
            && !is_block_root
            && el.v_condition.is_none()
            && el.v_for.is_none()
            && !array_grouped
            && !self.in_slot_context_stack.last().copied().unwrap_or(false)
        {
            let parent_is_cached = self.ast.nodes[_id.0]
                .parent
                .and_then(|pid| {
                    let pnode = &self.ast.nodes[pid.0];
                    if let AstNodeKind::Element(ref pel) = pnode.kind {
                        // Parent must be fully static AND itself eligible for caching:
                        // - not a block root (block roots aren't cached)
                        // - no structural directives
                        // - not a component (components use slot-level caching)
                        let parent_is_root = pnode.parent.is_none();
                        let parent_is_block_root = pel.v_condition.is_some()
                            || pel.v_for.is_some()
                            || (parent_is_root && self.single_root);
                        Some(
                            pel.is_fully_static
                                && !parent_is_block_root
                                && pel.v_condition.is_none()
                                && pel.v_for.is_none()
                                && !pel.tag_type.is_component(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or(false);
            if parent_is_cached {
                None
            } else {
                let idx = self.cache_index;
                self.cache_index += 1;
                Some(idx)
            }
        } else {
            None
        };

        // When inside a slot context, static elements don't get individual cache
        // wrapping (that's handled by emit_slot_children_with_cache), but they still
        // need the -1 CACHED patchFlag to match Vue's official compiler output.
        // Skip nested static children whose parent is also fully static — the parent's
        // -1 flag covers them (Vue only flags direct slot children, not nested ones).
        let slot_cached = cache_idx.is_none()
            && self.options.hoist_static
            && el.is_fully_static
            && !is_block_root
            && el.v_condition.is_none()
            && el.v_for.is_none()
            && (array_grouped
                || (self.in_slot_context_stack.last().copied().unwrap_or(false)
                    && !self.ast.nodes[_id.0]
                        .parent
                        .and_then(|pid| {
                            let pnode = &self.ast.nodes[pid.0];
                            if let AstNodeKind::Element(ref pel) = pnode.kind {
                                Some(pel.is_fully_static && !pel.tag_type.is_component())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(false)));

        let pre_reserved_hoist = self
            .plans
            .get_mut(plan_key)
            .and_then(|plan| plan.hoist.take());
        let text_run_cache_indices =
            self.reserve_static_text_run_caches(_id, &children, el.children_mode);
        let component_tag = self.component_tag(_id);
        let record = element::process_element_leave(
            el,
            oxc,
            Some(self.oxc_ast),
            &mut children,
            source,
            out,
            &self.options,
            &self.resolver,
            &mut buf,
            v_for_prefix.as_ref().map(|(s, _)| s.as_str()),
            self.ast,
            is_block_root,
            force_open_block,
            injected_key,
            if self.hoisting {
                Some(&mut self.hoisted_constants)
            } else {
                None
            },
            cache_idx,
            Some(&mut self.resolved_components),
            component_tag,
            slot_cached,
            pre_reserved_hoist,
            self.plans
                .get(plan_key)
                .map(|plan| &plan.handler_plans)
                .filter(|caches| !caches.is_empty()),
            self.plans.get(plan_key).and_then(|plan| plan.array_group),
            &text_run_cache_indices,
        );
        buf.clear();
        self.buf = buf;

        // Emit scope close suffix for structural directives. A v-for + v-memo
        // element uses its per-item memo close instead of the normal
        // `_renderList` fragment close.
        let scope_close = self.scope_closes.pop().flatten();
        if let Some(memo_suffix) = self.memo_for_suffixes.remove(&_id.0) {
            out.prepend_alloc(record.end, &memo_suffix);
        } else if let Some(scope_close) = scope_close {
            self.prepend_scope_close(out, record.end, &scope_close);
        }
    }

    pub(super) fn visit_text_at(
        &mut self,
        id: NodeId,
        text_node: &TextNode,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        helpers::debug_assert_slice_bounds(source, text_node.start, text_node.end, "visit_text");
        // Text the compile structure dropped was removed before the walk.
        if self.is_dropped(id) {
            return;
        }
        // Apply text overwrites (condensation, escaping).
        let preserve_whitespace = self
            .inputs
            .topology
            .node_for_syntax(id)
            .is_some_and(|node| {
                self.inputs
                    .structure
                    .facts(self.inputs.structure.compile_id(node))
                    .flags
                    .has(crate::framework::vue::structure::VueNodeFlags::PRESERVE_WHITESPACE)
            });
        let _ = text::process_text(text_node, source, out, preserve_whitespace);
    }

    pub(super) fn visit_interpolation_at(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'c>,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        // The spaces inside the delimiters go with them when the content is
        // written as the official compiler writes it.
        let trimmed;
        let interp = if self.options.trim_interpolations {
            let inner = &source[interp.inner_start as usize..interp.inner_end as usize];
            let lead = (inner.len() - inner.trim_start().len()) as u32;
            let trail = (inner.len() - inner.trim_end().len()) as u32;
            let (inner_start, inner_end) = if lead as usize == inner.len() {
                (interp.inner_start, interp.inner_end)
            } else {
                (interp.inner_start + lead, interp.inner_end - trail)
            };
            trimmed = InterpolationNode {
                inner_start,
                inner_end,
                ..*interp
            };
            &trimmed
        } else {
            interp
        };
        // Apply delimiter overwrites and binding patches.
        let _ = interpolation::process_interpolation(interp, oxc, &self.resolver, out);
        for (start, end) in crate::template::binding::prefixed_expr::expression_ts_removal_spans(
            Some(oxc),
            self.options.force_js,
        ) {
            out.overwrite(oxc.offset + start, oxc.offset + end, "");
        }
    }

    pub(super) fn visit_comment_at(
        &mut self,
        id: NodeId,
        comment_node: &CommentNode,
        source: &'c str,
        out: &mut VdomOut<'c>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            comment_node.start,
            comment_node.end,
            "visit_comment",
        );
        // A comment the compile structure dropped (between branches of one
        // chain, or with comments disabled) was removed before the walk.
        if self.is_dropped(id) {
            return;
        }
        match comment::process_comment(comment_node, source, self.options.comments, out) {
            comment::CommentOutcome::Kept => {}
            comment::CommentOutcome::Dropped { start, end } => {
                out.overwrite(start, end, "");
            }
        }
    }
}
