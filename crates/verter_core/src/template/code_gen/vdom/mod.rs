//! VDOM (Virtual DOM) template code generation backend.
//!
//! This is the default Vue 3 compilation target. Given a template like
//! `<div :class="cls">{{ msg }}</div>`, it produces a render function body:
//!
//! ```js
//! return (_openBlock(), _createElementBlock("div", {
//!   class: _normalizeClass(_ctx.cls)
//! }, _toDisplayString(_ctx.msg), 3 /* TEXT, CLASS */))
//! ```
//!
//! ## Codegen strategy
//!
//! Unlike the Vapor backends which build output strings and replace the entire
//! `<template>` block, the VDOM backend uses **in-place source overwrites**.
//! Each element's open/close tags are overwritten with `_createElementVNode()`
//! or `_createElementBlock()` calls while leaving source positions of child
//! expressions intact for sourcemap fidelity.
//!
//! Key concepts:
//!
//! - **Child records** — built on-demand from the AST in `leave_element` /
//!   `leave_template`, avoiding a per-element state stack.
//! - **Patch flags** — computed per-element from dynamic bindings (e.g.
//!   `PatchFlags::CLASS`, `PatchFlags::TEXT`), emitted as trailing numeric args
//!   with dev-mode comments.
//! - **Block tree** — root elements and `v-if`/`v-for` use `_openBlock()` +
//!   `_createElementBlock()` for optimized patching; inner elements use
//!   `_createElementVNode()`.
//! - **Scope closes** — structural directives (`v-if`, `v-for`) push a
//!   `ScopeClose` entry to emit ternary/`_renderList` suffixes on leave.
//!
//! ## Shared vs unique logic
//!
//! Binding resolution (`_ctx.`/`$setup.`/`.value` prefixing) is shared via
//! [`super::binding::BindingResolver`]. Runtime helper constants and import
//! bitflags are shared via [`super::shared::helpers`]. The DFS walk is shared
//! via [`super::walker`]. Element-specific codegen (props, directives,
//! whitespace resolution, children separators) lives in this module's
//! submodules.

mod block;
mod children;
pub mod comment;
mod component;
pub mod directives;
pub mod element;
pub mod interpolation;
pub mod props;
mod slots;
pub mod text;

use rustc_hash::FxHashMap;

use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeConditionKind, InterpolationNode, TagType,
    TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use super::binding::BindingResolver;
use super::shared::helpers::{self, VdomHelper};
use super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, ScopeClose};
use super::{TemplateCodeGen, TemplateCodeGenOptions};

/// VDOM code generation backend.
///
/// Produces `_createElementVNode()` / `_createElementBlock()` calls with
/// patch flags, dynamic props arrays, and proper children wrapping.
///
/// Child records are built on-demand from the AST in `leave_element` /
/// `leave_template`, eliminating the need for a per-element state stack.
pub struct VdomCodeGen<'ast, 'alloc> {
    /// Reference to the template AST arena for O(1) node lookups.
    ast: &'ast TemplateAst,
    resolver: BindingResolver<'alloc>,
    options: TemplateCodeGenOptions,
    /// Reusable buffer for building open/close tag strings.
    /// Uses `std::mem::take` pattern to avoid per-element allocation.
    buf: String,
    /// Scope close stack for structural directives (v-if, v-for).
    /// Pushed in `enter_element`, popped in `leave_element`.
    scope_closes: Vec<Option<ScopeClose>>,
    /// v-for prefix stack. Stored during `enter_element` and consumed by
    /// `process_element_leave` to include in the open tag overwrite. This
    /// ensures correct ordering when a sibling text node ends at the same
    /// position as the v-for element starts.
    /// Tuple: (prefix_string, iterable_source_start) for source map mapping.
    v_for_prefixes: Vec<Option<(String, Option<u32>)>>,
    /// Pre-computed condition prefixes with binding resolution.
    /// Populated during `enter_element` (where OXC data is available) and
    /// consumed by `build_child_records` (which only sees AST data).
    /// Keyed by AST node index.
    resolved_condition_prefixes: FxHashMap<usize, String>,
    /// Whether the template has a single effective root element (not multi-root).
    /// Set in `enter_template`, used by `leave_element` to determine if a root
    /// element should be a block root (`_createElementBlock` / `_createBlock`).
    single_root: bool,
    /// Hoisted constant strings (e.g., `["id"]`) collected during codegen.
    /// Emitted as `const _hoisted_N = ...` before the render function.
    /// Deduplicated: identical arrays share the same `_hoisted_N` reference.
    hoisted_constants: Vec<String>,
    /// Cache index counter for `_cache[N]` static element wrapping.
    /// Incremented each time a fully-static element is cached.
    cache_index: usize,
}

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        resolver: BindingResolver<'alloc>,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            resolver,
            options: options.clone(),
            buf: String::with_capacity(128),
            scope_closes: Vec::new(),
            v_for_prefixes: Vec::new(),
            resolved_condition_prefixes: FxHashMap::default(),
            single_root: false,
            hoisted_constants: Vec::new(),
            cache_index: 0,
        }
    }

    /// Build child records from AST children (O(n) scan).
    ///
    /// Replaces the old per-element `ElementState.children` accumulator.
    /// Children are classified on-demand from the AST when the parent's
    /// leave phase needs them.
    pub(super) fn build_child_records(
        &self,
        children: &[NodeId],
        source: &str,
    ) -> Vec<ChildRecord> {
        let mut records = Vec::with_capacity(children.len());
        for &child_id in children {
            let node = &self.ast.nodes[child_id.0];
            match &node.kind {
                AstNodeKind::Text(text_node) => {
                    let content = &source[text_node.start as usize..text_node.end as usize];
                    if let Some(kind) = text::classify_text_kind(content) {
                        records.push(ChildRecord {
                            start: text_node.start,
                            end: text_node.end,
                            kind,
                            condition: None,
                            condition_prefix: None,
                            condition_expr_start: None,
                            condition_binding_prefix_len: 0,
                        });
                    }
                }
                AstNodeKind::Interpolation(interp) => {
                    records.push(ChildRecord {
                        start: interp.start,
                        end: interp.end,
                        kind: ChildKind::Interpolation,
                        condition: None,
                        condition_prefix: None,
                        condition_expr_start: None,
                        condition_binding_prefix_len: 0,
                    });
                }
                AstNodeKind::Element(el) => {
                    let end = el
                        .tag_close
                        .as_ref()
                        .map(|tc| tc.end)
                        .unwrap_or(el.tag_open.end);

                    let (condition, condition_prefix, condition_expr_start, cond_prefix_len) =
                        match el.v_condition.as_ref() {
                            Some(c) => {
                                let role = match c.kind {
                                    ElementNodeConditionKind::If => ConditionChainRole::Start,
                                    ElementNodeConditionKind::ElseIf
                                    | ElementNodeConditionKind::Else => {
                                        ConditionChainRole::Continuation
                                    }
                                };
                                // Build condition prefix for v-if/v-else-if (not v-else).
                                // Uses pre-resolved expression from enter_element (which has
                                // OXC binding data for correct $setup./$props. prefixes).
                                let (prefix, expr_start, binding_prefix_len) = match c.kind {
                                    ElementNodeConditionKind::If
                                    | ElementNodeConditionKind::ElseIf => {
                                        // Use pre-resolved expression (avoids clone + format!):
                                        // borrow from the HashMap if available, else compute.
                                        let resolved =
                                            self.resolved_condition_prefixes.get(&child_id.0);
                                        let raw_expr =
                                            helpers::extract_directive_value(&c.prop, source);
                                        let expr_str = match resolved {
                                            Some(s) => s.as_str(),
                                            None => {
                                                if raw_expr.is_empty() {
                                                    "true"
                                                } else {
                                                    raw_expr
                                                }
                                            }
                                        };
                                        // Compute binding prefix length so we can split
                                        // the condition prefix into unmapped + mapped
                                        // segments for accurate source mapping.
                                        let bp_len = self.resolver.simple_expr_prefix_len(raw_expr);
                                        let mut s = String::with_capacity(expr_str.len() + 5);
                                        s.push('(');
                                        s.push_str(expr_str);
                                        s.push_str(") ? ");
                                        (Some(s), c.prop.value_start, bp_len)
                                    }
                                    ElementNodeConditionKind::Else => (None, None, 0),
                                };
                                (Some(role), prefix, expr_start, binding_prefix_len)
                            }
                            None => (None, None, None, 0),
                        };
                    records.push(ChildRecord {
                        start: el.tag_open.start,
                        end,
                        kind: ChildKind::Element,
                        condition,
                        condition_prefix,
                        condition_expr_start,
                        condition_binding_prefix_len: cond_prefix_len,
                    });
                }
                AstNodeKind::Comment(comment) => {
                    if self.options.comments {
                        records.push(ChildRecord {
                            start: comment.start,
                            end: comment.end,
                            kind: ChildKind::Comment,
                            condition: None,
                            condition_prefix: None,
                            condition_expr_start: None,
                            condition_binding_prefix_len: 0,
                        });
                    }
                }
            }
        }

        records
    }
}

impl<'ast, 'alloc> TemplateCodeGen<'alloc> for VdomCodeGen<'ast, 'alloc> {
    fn enter_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        // Pre-compute whether the template has a single effective root.
        // This determines whether root-level elements use block helpers
        // (_createElementBlock / _createBlock) vs regular helpers.
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut effective = 0usize;
        for &child_id in root_children {
            let node = &self.ast.nodes[child_id.0];
            match &node.kind {
                AstNodeKind::Element(el) => {
                    // v-else-if / v-else continuations don't count as separate roots
                    if el.v_condition.as_ref().is_some_and(|c| {
                        matches!(
                            c.kind,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                        )
                    }) {
                        continue;
                    }
                    effective += 1;
                }
                AstNodeKind::Text(text) => {
                    // Whitespace-only text nodes will be removed by leave_template
                    let content = &source[text.start as usize..text.end as usize];
                    if !content.trim().is_empty() {
                        effective += 1;
                    }
                }
                AstNodeKind::Interpolation(_) => effective += 1,
                AstNodeKind::Comment(_) => {} // Comments don't count as roots
            }
        }
        self.single_root = effective == 1;
        // Open tag overwrite is deferred to leave_template where we have
        // full context (children count, v-if status) to emit the correct
        // combined prefix (function signature + return + openBlock).
    }

    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut children = self.build_child_records(root_children, source);

        // Resolve whitespace at root level. Leading and trailing whitespace
        // are dropped from the children vec WITHOUT overwrites — the combined
        // open/close tag overwrites below cover those source regions.
        // Interior whitespace is resolved with overwrites as usual.
        {
            // Drop leading whitespace (no overwrite)
            let leading = children
                .iter()
                .take_while(|c| element::is_whitespace_kind(c.kind))
                .count();
            children.drain(..leading);

            // Drop trailing whitespace (no overwrite)
            while children
                .last()
                .is_some_and(|c| element::is_whitespace_kind(c.kind))
            {
                children.pop();
            }

            // Resolve interior whitespace (with overwrites)
            let mut i = 0;
            while i < children.len() {
                match children[i].kind {
                    ChildKind::WhitespaceNewline => {
                        let removed = children.remove(i);
                        out.overwrite(removed.start, removed.end, "");
                    }
                    ChildKind::WhitespaceSpace => {
                        out.overwrite(children[i].start, children[i].end, " ");
                        children[i].kind = ChildKind::Text;
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
        }

        // Strip comments/text between v-if chain members (at root level too)
        element::strip_interstitial_condition_nodes(&mut children, out, true);

        // Build hoisted constant preamble (e.g., `const _hoisted_1 = ["id"]\n`)
        let hoisted_preamble = if self.hoisted_constants.is_empty() {
            String::new()
        } else {
            let mut preamble = String::with_capacity(self.hoisted_constants.len() * 30);
            for (i, constant) in self.hoisted_constants.iter().enumerate() {
                preamble.push_str("const _hoisted_");
                preamble.push_str(&(i + 1).to_string());
                preamble.push_str(" = ");
                preamble.push_str(constant);
                preamble.push('\n');
            }
            preamble.push('\n');
            preamble
        };

        // Function signature prefix
        let fn_sig = if self.options.is_inline {
            "return (_ctx,_cache) => {\n"
        } else {
            "function render(_ctx, _cache, $props, $setup, $data, $options) {\n"
        };

        // Combined preamble: hoisted constants + function signature
        let full_prefix = if hoisted_preamble.is_empty() {
            fn_sig.to_string()
        } else {
            let mut s = hoisted_preamble;
            s.push_str(fn_sig);
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
                out.overwrite(tag_open.start, close_end, &buf);
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
                    out.overwrite(tag_open.start, child.start, &prefix);

                    // Emit the v-if condition prefix with source mapping
                    if let Some(ref cond) = child.condition_prefix {
                        if let Some(expr_start) = child.condition_expr_start {
                            children::emit_condition_prefix_mapped(
                                out,
                                child.start,
                                expr_start,
                                cond,
                                child.condition_binding_prefix_len,
                            );
                        } else {
                            out.prepend_alloc(child.start, cond);
                        }
                    }

                    // Emit condition prefixes for continuation children
                    // (v-else-if elements in the chain) with source mapping.
                    for cont in children.iter().skip(1) {
                        if let Some(ref cond) = cont.condition_prefix {
                            if let Some(expr_start) = cont.condition_expr_start {
                                children::emit_condition_prefix_mapped(
                                    out,
                                    cont.start,
                                    expr_start,
                                    cond,
                                    cont.condition_binding_prefix_len,
                                );
                            } else {
                                out.prepend_alloc(cont.start, cond);
                            }
                        }
                    }

                    out.overwrite(close_start, close_end, "\n}");
                } else {
                    // Single root — block root with _openBlock + _createElementBlock
                    out.add_vdom_import(VdomHelper::OpenBlock);
                    let mut prefix = String::with_capacity(full_prefix.len() + 24);
                    prefix.push_str(&full_prefix);
                    prefix.push_str("return (_openBlock(), ");
                    out.overwrite(tag_open.start, child.start, &prefix);
                    out.overwrite(close_start, close_end, ")\n}");
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
                out.overwrite(tag_open.start, children[0].start, &prefix);

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

                // Close fragment + render function
                let flag_str = helpers::format_patch_flag(
                    helpers::PATCH_STABLE_FRAGMENT,
                    self.options.is_production,
                    |s| out.alloc_str(s),
                );
                let mut close_buf = String::with_capacity(32);
                close_buf.push_str("\n], ");
                close_buf.push_str(flag_str);
                close_buf.push_str("))\n}");
                out.overwrite(close_start, close_end, &close_buf);
            }
        }
    }

    fn enter_element(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> super::WalkAction {
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
            directives::collect_scope_imports(&close, out);

            // Pre-compute resolved condition prefix using OXC binding data.
            // build_child_records only sees AST data (no OXC), so we resolve
            // binding prefixes here where OXC data is available.
            if matches!(
                condition.kind,
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf
            ) {
                let raw_expr = helpers::extract_directive_value(&condition.prop, source);
                let resolved = if let Some(oxc_el) = oxc {
                    if let Some(oxc_cond) = &oxc_el.condition {
                        use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
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
                        build_prefixed_expr(
                            raw_expr,
                            condition.prop.value_start.unwrap_or(0),
                            oxc_cond,
                            &self.resolver,
                            &ts_skip,
                        )
                    } else {
                        self.resolver.resolve_simple_expr(raw_expr)
                    }
                } else {
                    self.resolver.resolve_simple_expr(raw_expr)
                };
                self.resolved_condition_prefixes.insert(id.0, resolved);
            }

            // NOTE: condition prefix `(expr) ? ` is NOT prepended here.
            // It is emitted by the parent's separator logic (build_child_records
            // stores it in ChildRecord.condition_prefix) to ensure correct
            // ordering relative to comma separators.
            self.scope_closes.push(Some(close));
            self.v_for_prefixes.push(None);
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
            let (prefix, close, iterable_src) =
                directives::build_for_prefix(v_for, source, is_keyed, oxc, &self.resolver);
            directives::collect_scope_imports(&close, out);
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
        super::WalkAction::Continue
    }

    fn leave_element(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );

        // Handle <slot> outlet: generates _renderSlot(_ctx.$slots, "name")
        if el.tag_type.is_slot_outlet() {
            let record = self.process_slot_outlet(el, source, out);
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
                let suffix =
                    directives::format_scope_close(&scope_close, self.options.is_production);
                if !suffix.is_empty() {
                    out.prepend_static(record.end, suffix);
                }
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

        // Handle <template v-if> / <template v-for>: renders as Fragment, not
        // as a <template> element. These are transparent structural wrappers
        // whose children become the Fragment's children.
        if el.tag_type == TagType::Template
            && el.v_slot.is_none()
            && (el.v_condition.is_some() || el.v_for.is_some())
        {
            self.leave_template_fragment(el, source, out);
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
        let is_root_child = self.ast.nodes[_id.0].parent.is_none();
        let is_block_root =
            el.v_condition.is_some() || el.v_for.is_some() || (is_root_child && self.single_root);

        // Handle component with slot children: wrap in slot object instead of array
        if el.tag_type.is_component() && self.has_slot_children(el_children) {
            self.leave_component_with_slots(el, oxc, el_children, source, out, is_block_root);
            return;
        }

        // Handle component with implicit default slot (non-slot children)
        if el.tag_type.is_component() && !el_children.is_empty() {
            self.leave_component_with_default_slot(
                el,
                oxc,
                el_children,
                source,
                out,
                is_block_root,
            );
            return;
        }

        let mut children = self.build_child_records(el_children, source);
        // Take the reusable buffer, use it, then put it back (std::mem::take pattern)
        let mut buf = std::mem::take(&mut self.buf);
        let v_for_prefix = self.v_for_prefixes.pop().flatten();

        // Determine if this static element should be cached via _cache[N]
        let cache_idx = if self.options.hoist_static
            && el.is_fully_static
            && !is_block_root
            && el.v_condition.is_none()
            && el.v_for.is_none()
        {
            let idx = self.cache_index;
            self.cache_index += 1;
            Some(idx)
        } else {
            None
        };

        let record = element::process_element_leave(
            el,
            oxc,
            &mut children,
            source,
            out,
            &self.options,
            &self.resolver,
            &mut buf,
            v_for_prefix.as_ref().map(|(s, _)| s.as_str()),
            self.ast,
            is_block_root,
            Some(&mut self.hoisted_constants),
            cache_idx,
        );
        buf.clear();
        self.buf = buf;

        // Emit scope close suffix for structural directives
        if let Some(scope_close) = self.scope_closes.pop().flatten() {
            let suffix = directives::format_scope_close(&scope_close, self.options.is_production);
            if !suffix.is_empty() {
                out.prepend_static(record.end, suffix);
            }
        }
    }

    fn visit_text(
        &mut self,
        id: NodeId,
        text_node: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(source, text_node.start, text_node.end, "visit_text");
        // Skip text between v-if chain members (Vue discards these).
        // Don't emit an overwrite here — the parent's leave phase handles removal
        // (via strip_interstitial_condition_nodes or gap-filling).
        if self.is_interstitial_condition_node(id) {
            return;
        }
        // Apply text overwrites (condensation, escaping).
        // Child classification is handled by build_child_records from the AST.
        let _ = text::process_text(text_node, source, out);
    }

    fn visit_interpolation(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        _source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Apply delimiter overwrites and binding patches.
        // Child classification is handled by build_child_records from the AST.
        let _ = interpolation::process_interpolation(interp, oxc, &self.resolver, out);
    }

    fn visit_comment(
        &mut self,
        id: NodeId,
        comment_node: &CommentNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            comment_node.start,
            comment_node.end,
            "visit_comment",
        );
        // Skip comments between v-if chain members (Vue discards these).
        // Emit removal overwrite directly — the parent's leave phase may not
        // include this comment in its child records (when options.comments=false,
        // build_child_records excludes comments, so strip_interstitial_condition_nodes
        // can't find them). At root level, gap-filling also doesn't cover these.
        if self.is_interstitial_condition_node(id) {
            out.overwrite(comment_node.start, comment_node.end, "");
            return;
        }
        // Apply comment overwrites (or removal if disabled).
        // Child classification is handled by build_child_records from the AST.
        let _ = comment::process_comment(comment_node, source, self.options.comments, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::{
        AstNode, ChildrenFlag, ChildrenMode, ElementContent, PropFlag, TagType, TemplateAst,
    };
    use crate::parser::types::RootNodeTemplateContent;
    use crate::types::NodeTag;
    use oxc_allocator::Allocator;
    use rustc_hash::FxHashMap;
    use smallvec::SmallVec;

    /// Create a minimal empty TemplateAst for tests that don't need AST lookups.
    fn make_empty_ast(root: &RootNodeTemplate) -> TemplateAst {
        TemplateAst {
            nodes: Vec::new(),
            root: root.clone(),
        }
    }

    /// Create a minimal ElementNode for test ASTs.
    fn make_simple_element(
        open_start: u32,
        open_end: u32,
        open_name_end: u32,
        close_start: u32,
        close_end: u32,
        close_name_end: u32,
    ) -> crate::ast::types::ElementNode {
        crate::ast::types::ElementNode {
            tag_open: NodeTag {
                start: open_start,
                end: open_end,
                name_end: open_name_end,
            },
            tag_close: Some(NodeTag {
                start: close_start,
                end: close_end,
                name_end: close_name_end,
            }),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: Vec::new(),
            content: Some(ElementContent {
                start: open_end,
                end: close_start,
                children: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
            is_fully_static: false,
        }
    }

    fn make_options_standalone() -> TemplateCodeGenOptions {
        TemplateCodeGenOptions {
            is_inline: false,
            is_production: false,
            ..Default::default()
        }
    }

    fn make_options_inline() -> TemplateCodeGenOptions {
        TemplateCodeGenOptions {
            is_inline: true,
            is_production: false,
            ..Default::default()
        }
    }

    fn make_resolver(_alloc: &Allocator) -> BindingResolver<'_> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    fn make_root(
        tag_open: NodeTag,
        tag_close: Option<NodeTag>,
        content: Option<RootNodeTemplateContent>,
    ) -> RootNodeTemplate {
        RootNodeTemplate {
            tag_open,
            tag_close,
            lang: None,
            attributes: Vec::new(),
            content,
        }
    }

    fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
        let mut ct = crate::code_transform::CodeTransform::new(source, alloc);
        out.apply_to(&mut ct);
        ct.build_string()
    }

    // ==================== enter_template ====================

    #[test]
    fn enter_template_standalone_defers_to_leave() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            None,
            None,
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);
        gen.enter_template(&root, "", &mut out);

        // Open tag overwrite is deferred to leave_template
        assert_eq!(out.overwrites.len(), 0);
    }

    #[test]
    fn enter_template_inline_defers_to_leave() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_inline();
        let resolver = make_resolver(&alloc);

        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            None,
            None,
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);
        gen.enter_template(&root, "", &mut out);

        // Open tag overwrite is deferred to leave_template
        assert_eq!(out.overwrites.len(), 0);
    }

    // ==================== leave_template: empty ====================

    #[test]
    fn leave_template_empty_returns_null() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // <template></template>  (0-10 open, 10-21 close)
        let source = "<template></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 10,
                end: 21,
                name_end: 20,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 10,
                children: SmallVec::new(),
            }),
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&root, source, &mut out);
        gen.leave_template(&root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        assert!(result.contains("return null"));
        assert!(result.ends_with('}'));
    }

    // ==================== leave_template: single root ====================

    #[test]
    fn leave_template_single_root_prepends_return() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // Simulate: <template><div></div></template>
        // positions: 0-10 open, 10-15 <div>, 15-21 </div>, 21-32 close
        let source = "<template><div></div></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 21,
                end: 32,
                name_end: 31,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 21,
                children: SmallVec::from_elem(NodeId(0), 1),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![AstNode {
                kind: AstNodeKind::Element(Box::new(make_simple_element(10, 15, 14, 15, 21, 20))),
                parent: None,
                index_in_parent: 0,
            }],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        // Open tag replaced with function signature
        assert!(result.starts_with("function render("));
        // Single root uses block root: _openBlock() wrapper
        assert!(
            result.contains("return (_openBlock(), "),
            "Expected _openBlock() for single root, got: {result}"
        );
        // Close tag replaced with closing paren + newline + "}"
        assert!(result.ends_with(")\n}"));
    }

    // ==================== leave_template: multi root ====================

    #[test]
    fn leave_template_multi_root_wraps_in_fragment() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // <template><div></div><span></span></template>
        // 0-10 open, 10-15 <div>, 15-21 </div>, 21-27 <span>, 27-34 </span>, 34-45 close
        let source = "<template><div></div><span></span></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 34,
                end: 45,
                name_end: 44,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 34,
                children: SmallVec::from_slice(&[NodeId(0), NodeId(1)]),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        10, 15, 14, 15, 21, 20,
                    ))),
                    parent: None,
                    index_in_parent: 0,
                },
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        21, 27, 26, 27, 34, 33,
                    ))),
                    parent: None,
                    index_in_parent: 1,
                },
            ],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        assert!(result.contains("_openBlock()"));
        assert!(result.contains("_createElementBlock(_Fragment, null, ["));
        assert!(result.contains("64 /* STABLE_FRAGMENT */"));
        assert!(result.ends_with("))\n}"));
    }

    #[test]
    fn leave_template_multi_root_production_no_comment() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = TemplateCodeGenOptions {
            is_inline: false,
            is_production: true,
            ..Default::default()
        };
        let resolver = make_resolver(&alloc);

        // <template><div></div><span></span></template>
        let source = "<template><div></div><span></span></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 34,
                end: 45,
                name_end: 44,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 34,
                children: SmallVec::from_slice(&[NodeId(0), NodeId(1)]),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        10, 15, 14, 15, 21, 20,
                    ))),
                    parent: None,
                    index_in_parent: 0,
                },
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        21, 27, 26, 27, 34, 33,
                    ))),
                    parent: None,
                    index_in_parent: 1,
                },
            ],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        // Production: no comment after 64
        assert!(result.contains("\n], 64)"));
        assert!(!result.contains("/*"));
    }

    // ==================== Block-tree optimization (full pipeline) ====================

    /// Helper: compile a Vue SFC source and return the template code (VDOM mode).
    fn gen_vdom_template(source: &str) -> String {
        use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
        let alloc = oxc_allocator::Allocator::new();
        let options = CodegenOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            force_js: true,
            ..Default::default()
        };
        let result = compile(source, &options, &verter_opts, &alloc);
        assert!(
            result.errors.is_empty(),
            "compile errors: {:?}",
            result.errors
        );
        let tpl = result
            .template
            .as_ref()
            .expect("should have template block");
        tpl.code.clone()
    }

    #[test]
    fn block_tree_single_root_element_uses_create_element_block() {
        let code = gen_vdom_template("<template><div>hello</div></template>");
        assert!(
            code.contains("_createElementBlock(\"div\""),
            "Single root element should use _createElementBlock, got:\n{code}"
        );
        assert!(
            !code.contains("_createElementVNode(\"div\""),
            "Single root element should NOT use _createElementVNode, got:\n{code}"
        );
        assert!(
            code.contains("_openBlock()"),
            "Single root should have _openBlock(), got:\n{code}"
        );
    }

    #[test]
    fn block_tree_single_root_component_uses_create_block() {
        let code = gen_vdom_template(
            "<template><MyComp/></template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>",
        );
        assert!(
            code.contains("_createBlock("),
            "Single root component should use _createBlock, got:\n{code}"
        );
        assert!(
            !code.contains("_createVNode("),
            "Single root component should NOT use _createVNode, got:\n{code}"
        );
    }

    #[test]
    fn block_tree_vif_element_uses_block() {
        let code = gen_vdom_template(
            "<template><div v-if=\"show\">A</div><span v-else>B</span></template>",
        );
        // Each v-if branch should have its own (_openBlock(), _createElementBlock(...))
        assert!(
            code.contains("(_openBlock(), _createElementBlock(\"div\""),
            "v-if element branch should use (_openBlock(), _createElementBlock(...)), got:\n{code}"
        );
        assert!(
            code.contains("(_openBlock(), _createElementBlock(\"span\""),
            "v-else element branch should use (_openBlock(), _createElementBlock(...)), got:\n{code}"
        );
        // Should NOT use regular _createElementVNode for v-if branches
        assert!(
            !code.contains("_createElementVNode(\"div\""),
            "v-if branch should NOT use _createElementVNode, got:\n{code}"
        );
    }

    #[test]
    fn block_tree_vif_component_uses_block() {
        let code = gen_vdom_template(
            "<template><MyComp v-if=\"show\"/><OtherComp v-else/></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nimport OtherComp from './Other.vue'\n</script>",
        );
        assert!(
            code.contains("(_openBlock(), _createBlock("),
            "v-if component branch should use (_openBlock(), _createBlock(...)), got:\n{code}"
        );
        assert!(
            !code.contains("_createVNode("),
            "v-if component should NOT use _createVNode, got:\n{code}"
        );
    }

    #[test]
    fn block_tree_vfor_component_uses_block() {
        let code = gen_vdom_template(
            "<template><div><MyComp v-for=\"item in items\" :key=\"item.id\"/></div></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst items = []\n</script>",
        );
        assert!(
            code.contains("(_openBlock(), _createBlock("),
            "v-for component should use (_openBlock(), _createBlock(...)), got:\n{code}"
        );
        assert!(
            !code.contains("_createVNode("),
            "v-for component should NOT use _createVNode, got:\n{code}"
        );
    }

    #[test]
    fn block_tree_multi_root_children_use_regular_helpers() {
        // Use interpolations to ensure children are dynamic (not hoisted)
        let code = gen_vdom_template(
            "<template><div>{{ a }}</div><p>{{ b }}</p></template>\n<script setup>\nconst a = 1, b = 2\n</script>",
        );
        // Multi-root: individual children should use _createElementVNode, not block variant
        assert!(
            code.contains("_createElementVNode(\"div\""),
            "Multi-root children should use _createElementVNode for div, got:\n{code}"
        );
        assert!(
            code.contains("_createElementVNode(\"p\""),
            "Multi-root children should use _createElementVNode for p, got:\n{code}"
        );
        // The Fragment wrapper itself should use _createElementBlock
        assert!(
            code.contains("_createElementBlock(_Fragment"),
            "Multi-root should wrap in _createElementBlock(_Fragment, ...), got:\n{code}"
        );
        // Children should NOT use block variants
        assert!(
            !code.contains("_createElementBlock(\"div\""),
            "Multi-root children should NOT use _createElementBlock, got:\n{code}"
        );
    }

    #[test]
    fn block_tree_inner_elements_use_regular_helpers() {
        // Use a dynamic inner element (with :class binding) to prevent static hoisting
        let code = gen_vdom_template(
            "<template><div><span :class=\"cls\">inner</span></div></template>\n<script setup>\nconst cls = 'x'\n</script>",
        );
        // Root div should use block variant
        assert!(
            code.contains("_createElementBlock(\"div\""),
            "Root element should use _createElementBlock, got:\n{code}"
        );
        // Inner span should use regular variant
        assert!(
            code.contains("_createElementVNode(\"span\""),
            "Inner element should use _createElementVNode, got:\n{code}"
        );
        // Inner span should NOT use block variant
        assert!(
            !code.contains("_createElementBlock(\"span\""),
            "Inner element should NOT use _createElementBlock, got:\n{code}"
        );
    }

    // ==================== normalizeProps / guardReactiveProps ====================

    #[test]
    fn normalize_props_vbind_spread_alone() {
        // v-bind="attrs" alone → _normalizeProps(_guardReactiveProps(attrs))
        let code = gen_vdom_template(
            "<template><div v-bind=\"attrs\">hi</div></template>\n<script setup>\nconst attrs = {}\n</script>",
        );
        assert!(
            code.contains("_normalizeProps(_guardReactiveProps("),
            "v-bind spread alone should use _normalizeProps(_guardReactiveProps(...)), got:\n{code}"
        );
        assert!(
            !code.contains("_mergeProps("),
            "v-bind spread alone should NOT use _mergeProps, got:\n{code}"
        );
    }

    #[test]
    fn normalize_props_vbind_spread_on_component() {
        // Component with v-bind="props" alone → _normalizeProps(_guardReactiveProps(props))
        let code = gen_vdom_template(
            "<template><MyComp v-bind=\"compProps\" /></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst compProps = {}\n</script>",
        );
        assert!(
            code.contains("_normalizeProps(_guardReactiveProps("),
            "Component v-bind spread should use _normalizeProps(_guardReactiveProps(...)), got:\n{code}"
        );
    }

    #[test]
    fn normalize_props_vbind_spread_with_regular_props_uses_merge_only() {
        // v-bind="attrs" + class="foo" → _mergeProps({...}, attrs) — NO normalizeProps
        let code = gen_vdom_template(
            "<template><div class=\"foo\" v-bind=\"attrs\">hi</div></template>\n<script setup>\nconst attrs = {}\n</script>",
        );
        assert!(
            code.contains("_mergeProps("),
            "v-bind spread + regular props should use _mergeProps, got:\n{code}"
        );
        assert!(
            !code.contains("_normalizeProps("),
            "v-bind spread + regular props should NOT use _normalizeProps, got:\n{code}"
        );
        assert!(
            !code.contains("_guardReactiveProps("),
            "v-bind spread + regular props should NOT use _guardReactiveProps, got:\n{code}"
        );
    }

    #[test]
    fn normalize_props_dynamic_attr_name() {
        // :[attrName]="value" → _normalizeProps({ [attrName || ""]: value })
        let code = gen_vdom_template(
            "<template><div :[attrName]=\"value\">content</div></template>\n<script setup>\nconst attrName = 'id'\nconst value = '1'\n</script>",
        );
        assert!(
            code.contains("_normalizeProps("),
            "Dynamic attr name should use _normalizeProps, got:\n{code}"
        );
        assert!(
            !code.contains("_guardReactiveProps("),
            "Dynamic attr name should NOT use _guardReactiveProps, got:\n{code}"
        );
        // The dynamic key should use computed property syntax with || ""
        assert!(
            code.contains("|| \"\""),
            "Dynamic attr key should have || \"\" fallback, got:\n{code}"
        );
    }

    // ==================== toHandlers (v-on spread) ====================

    #[test]
    fn to_handlers_von_spread_alone_on_element() {
        // v-on="handlers" → _toHandlers(handlers, true) on elements
        let code = gen_vdom_template(
            "<template><div v-on=\"handlers\">hi</div></template>\n<script setup>\nconst handlers = {}\n</script>",
        );
        assert!(
            code.contains("_toHandlers("),
            "v-on spread should use _toHandlers, got:\n{code}"
        );
        assert!(
            code.contains(", true)"),
            "v-on spread on element should have true arg, got:\n{code}"
        );
    }

    #[test]
    fn to_handlers_von_spread_on_component() {
        // v-on="handlers" on component → _toHandlers(handlers) without true
        let code = gen_vdom_template(
            "<template><MyComp v-on=\"handlers\" /></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst handlers = {}\n</script>",
        );
        assert!(
            code.contains("_toHandlers("),
            "Component v-on spread should use _toHandlers, got:\n{code}"
        );
        assert!(
            !code.contains("_toHandlers($setup.handlers, true)"),
            "Component v-on spread should NOT have true arg, got:\n{code}"
        );
    }

    #[test]
    fn to_handlers_von_spread_with_regular_event() {
        // @click + v-on="handlers" → _mergeProps({onClick:...}, _toHandlers(handlers, true))
        let code = gen_vdom_template(
            "<template><div @click=\"onClick\" v-on=\"handlers\">hi</div></template>\n<script setup>\nconst onClick = () => {}\nconst handlers = {}\n</script>",
        );
        assert!(
            code.contains("_mergeProps("),
            "v-on spread + regular event should use _mergeProps, got:\n{code}"
        );
        assert!(
            code.contains("_toHandlers("),
            "v-on spread in mergeProps should use _toHandlers, got:\n{code}"
        );
    }

    #[test]
    fn to_handlers_vbind_and_von_spreads() {
        // v-bind="attrs" v-on="handlers" → _mergeProps(attrs, _toHandlers(handlers, true))
        let code = gen_vdom_template(
            "<template><div v-bind=\"attrs\" v-on=\"handlers\">hi</div></template>\n<script setup>\nconst attrs = {}\nconst handlers = {}\n</script>",
        );
        assert!(
            code.contains("_mergeProps("),
            "v-bind + v-on spreads should use _mergeProps, got:\n{code}"
        );
        assert!(
            code.contains("_toHandlers("),
            "v-on spread should be wrapped with _toHandlers, got:\n{code}"
        );
        assert!(
            !code.contains("_toHandlers($setup.attrs"),
            "v-bind spread should NOT use _toHandlers, got:\n{code}"
        );
    }

    // ==================== Literal prop optimization ====================

    #[test]
    fn literal_bind_value_not_in_dynamic_props() {
        // :value="200" :max="99" are pure literals — should NOT generate PROPS flag
        let code = gen_vdom_template(
            "<template><MyComp :value=\"200\" :max=\"99\" class=\"item\"><template #default>content</template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>",
        );
        assert!(
            !code.contains("8 /* PROPS */"),
            "Literal bind values should NOT add PROPS flag, got:\n{code}"
        );
        assert!(
            !code.contains("[\"value\""),
            "Literal bind values should NOT appear in dynamic props, got:\n{code}"
        );
    }

    #[test]
    fn dynamic_bind_value_in_dynamic_props() {
        // :value="count" uses a reactive variable — SHOULD generate PROPS flag
        let code = gen_vdom_template(
            "<template><MyComp :value=\"count\" class=\"item\"><template #default>content</template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>",
        );
        assert!(
            code.contains("8 /* PROPS */") || code.contains("PROPS"),
            "Dynamic bind values should add PROPS flag, got:\n{code}"
        );
    }

    // ==================== Static hoisting (_hoisted_N) ====================

    #[test]
    fn hoisted_dynamic_props_array() {
        // :id="x" should produce _hoisted_1 = ["id"] before render function
        let code = gen_vdom_template(
            "<template><div><span :id=\"x\">hello</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\n</script>",
        );
        assert!(
            code.contains("const _hoisted_1 = [\"id\"]"),
            "Dynamic props array should be hoisted as _hoisted_1, got:\n{code}"
        );
        assert!(
            code.contains("_hoisted_1)"),
            "Element should reference _hoisted_1 instead of inline array, got:\n{code}"
        );
        assert!(
            !code.contains(", [\"id\"])"),
            "Dynamic props array should NOT be inlined, got:\n{code}"
        );
    }

    #[test]
    fn hoisted_multiple_dynamic_props_arrays() {
        // Multiple elements with different dynamic props get separate hoisted constants
        let code = gen_vdom_template(
            "<template><div><span :id=\"x\">a</span><span :title=\"y\">b</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\nconst y = ref(2)\n</script>",
        );
        assert!(
            code.contains("const _hoisted_1 = [\"id\"]"),
            "First dynamic props array should be hoisted as _hoisted_1, got:\n{code}"
        );
        assert!(
            code.contains("const _hoisted_2 = [\"title\"]"),
            "Second dynamic props array should be hoisted as _hoisted_2, got:\n{code}"
        );
    }

    #[test]
    fn hoisted_dynamic_props_array_deduplication() {
        // Two elements with the same dynamic props array should share the hoisted constant
        let code = gen_vdom_template(
            "<template><div><span :id=\"x\">a</span><span :id=\"y\">b</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\nconst y = ref(2)\n</script>",
        );
        assert!(
            code.contains("const _hoisted_1 = [\"id\"]"),
            "Dynamic props array should be hoisted, got:\n{code}"
        );
        // Should not have _hoisted_2 since ["id"] is the same
        assert!(
            !code.contains("const _hoisted_2"),
            "Duplicate dynamic props arrays should be deduplicated, got:\n{code}"
        );
    }

    // ==================== Cache wrapping (_cache[N]) ====================

    #[test]
    fn cache_wraps_static_element() {
        // Static <p> child of a dynamic parent should use _cache[N] wrapping
        let code = gen_vdom_template(
            "<template><div><p id=\"static\">hello</p><span :class=\"cls\">world</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst cls = ref('foo')\n</script>",
        );
        assert!(
            code.contains("_cache[0] || (_cache[0] = _createElementVNode(\"p\""),
            "Static element should be wrapped with _cache[0], got:\n{code}"
        );
        assert!(
            code.contains("-1 /* CACHED */"),
            "Cached element should have -1 CACHED patch flag, got:\n{code}"
        );
        assert!(
            !code.contains("_createStaticVNode"),
            "Should NOT use createStaticVNode, got:\n{code}"
        );
    }

    #[test]
    fn cache_wraps_multiple_static_elements() {
        // Multiple static children each get their own _cache[N]
        let code = gen_vdom_template(
            "<template><div><p>a</p><p>b</p><span :class=\"cls\">c</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst cls = ref('foo')\n</script>",
        );
        assert!(
            code.contains("_cache[0]"),
            "First static child should use _cache[0], got:\n{code}"
        );
        assert!(
            code.contains("_cache[1]"),
            "Second static child should use _cache[1], got:\n{code}"
        );
        assert!(
            !code.contains("_createStaticVNode"),
            "Should NOT use createStaticVNode, got:\n{code}"
        );
    }
}
