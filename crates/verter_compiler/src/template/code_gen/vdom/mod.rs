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
pub mod text;

use rustc_hash::FxHashMap;

use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeCondition, ElementNodeConditionKind,
    InterpolationNode, TagType, TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use super::binding::BindingResolver;
use super::expression::{build_prefixed_expr_segments, resolve_simple_expr_segments};
use super::shared::helpers::{self, VdomHelper};
use super::types::{
    ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, MappedGeneratedText, ScopeClose,
};
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
    /// NodeId-aligned OXC parse data — used for the official-parity
    /// `hasScopeRef` slot-flag decision (scanning a component's slot
    /// subtree for references to outer template-scope variables).
    oxc_ast: &'ast crate::template::oxc::types::OxcParsedAst<'alloc>,
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
    /// Pre-computed condition expressions with binding resolution, carried as
    /// segment plans so the ternary head maps authored identifiers to source
    /// while leaving synthetic scaffolding unmapped.
    /// Populated during `enter_element` (where OXC data is available) and
    /// consumed by `build_child_records` (which only sees AST data).
    /// Keyed by AST node index. Holds the bare resolved expression (no `(` …
    /// `) ? ` wrapper); `build_child_records` wraps it per element.
    resolved_condition_prefixes: FxHashMap<usize, MappedGeneratedText>,
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
    /// Whether we are currently inside a slot function body.
    /// When true, `leave_element` skips individual `_cache[N]` wrapping
    /// because slot-level cache grouping handles it instead.
    /// Stored as a stack to handle nested slot contexts.
    in_slot_context_stack: Vec<bool>,
    /// Hoisted _resolveComponent() calls: Vec of (tag_name, variable_name).
    /// Emitted as `const _component_x = _resolveComponent("x")` at the top
    /// of the render function body. Insertion-ordered.
    resolved_components: Vec<(String, String)>,
}

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        oxc_ast: &'ast crate::template::oxc::types::OxcParsedAst<'alloc>,
        resolver: BindingResolver<'alloc>,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            oxc_ast,
            resolver,
            options: options.clone(),
            buf: String::with_capacity(128),
            scope_closes: Vec::new(),
            v_for_prefixes: Vec::new(),
            resolved_condition_prefixes: FxHashMap::default(),
            single_root: false,
            hoisted_constants: Vec::new(),
            cache_index: 0,
            in_slot_context_stack: Vec::new(),
            resolved_components: Vec::new(),
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
                    });
                }
                AstNodeKind::Element(el) => {
                    let end = el
                        .tag_close
                        .as_ref()
                        .map(|tc| tc.end)
                        .unwrap_or(el.tag_open.end);

                    let (condition, condition_prefix) = match el.v_condition.as_ref() {
                        Some(c) => {
                            let role = match c.kind {
                                ElementNodeConditionKind::If => ConditionChainRole::Start,
                                ElementNodeConditionKind::ElseIf
                                | ElementNodeConditionKind::Else => {
                                    ConditionChainRole::Continuation
                                }
                            };
                            // Build the ternary head for v-if/v-else-if (not v-else).
                            // Wrap the pre-resolved expression plan from
                            // `enter_element` (the only place with OXC binding data
                            // for correct $setup./$props. prefixes) in the synthetic
                            // `(` … `) ? ` so only authored identifiers map to source.
                            let prefix = match c.kind {
                                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                                    Some(self.condition_prefix_segments(child_id.0, c, source))
                                }
                                ElementNodeConditionKind::Else => None,
                            };
                            (Some(role), prefix)
                        }
                        None => (None, None),
                    };
                    records.push(ChildRecord {
                        start: el.tag_open.start,
                        end,
                        kind: ChildKind::Element,
                        condition,
                        condition_prefix,
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
                        });
                    }
                }
            }
        }

        records
    }

    /// Build the `(` … `) ? ` ternary head plan for a v-if/v-else-if element.
    ///
    /// Wraps the pre-resolved expression plan stored by `enter_element` (keyed
    /// by AST node index) in the synthetic ternary head, so only authored
    /// identifiers carry source-map tokens. `enter_element` always populates the
    /// map for If/ElseIf; the raw-expression branch is a defensive fallback.
    fn condition_prefix_segments(
        &self,
        node_idx: usize,
        c: &ElementNodeCondition,
        source: &str,
    ) -> MappedGeneratedText {
        match self.resolved_condition_prefixes.get(&node_idx) {
            Some(expr) => expr.wrapped("(", ") ? "),
            None => {
                let raw_expr = helpers::extract_directive_value(&c.prop, source);
                let value_start = c.prop.value_start.unwrap_or(0);
                let expr = if raw_expr.is_empty() {
                    MappedGeneratedText::synthetic("true")
                } else {
                    MappedGeneratedText::source(raw_expr, value_start)
                };
                expr.wrapped("(", ") ? ")
            }
        }
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

        // Build resolved component declarations (inside the function body)
        // e.g., `const _component_el_button = _resolveComponent("el-button")\n`
        let resolved_comp_preamble = if self.resolved_components.is_empty() {
            String::new()
        } else {
            let mut s = String::with_capacity(self.resolved_components.len() * 60);
            for (tag, var) in &self.resolved_components {
                // Check if this is a self-reference
                let is_self_ref = !self.options.self_name.is_empty() && {
                    let pascal = component::to_pascal_case(tag);
                    pascal == self.options.self_name
                };
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

        // Combined preamble: hoisted constants + function signature + resolved components
        let full_prefix = {
            let mut s = if hoisted_preamble.is_empty() {
                fn_sig.to_string()
            } else {
                let mut p = hoisted_preamble;
                p.push_str(fn_sig);
                p
            };
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
                let value_start = condition.prop.value_start.unwrap_or(0);
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
                        build_prefixed_expr_segments(
                            raw_expr,
                            value_start,
                            oxc_cond,
                            &self.resolver,
                            &ts_skip,
                        )
                    } else {
                        resolve_simple_expr_segments(&self.resolver, raw_expr, value_start)
                    }
                } else {
                    resolve_simple_expr_segments(&self.resolver, raw_expr, value_start)
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
                let (prefix, _for_close, iterable_src) =
                    directives::build_for_prefix(v_for, source, is_keyed, oxc, &self.resolver);
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
                directives::collect_scope_imports(&combined, out);
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

        // Track slot context: components and <template v-slot> create slot
        // contexts where children should use grouped caching instead of
        // individual _cache[N] wrapping. Teleport/KeepAlive take raw VNode-array
        // children (not slot objects), so they stay OUT of slot context.
        let tag_name =
            &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];
        let is_slot_parent = (element.tag_type.is_component()
            && !helpers::is_raw_children_builtin(tag_name))
            || (element.tag_type == TagType::Template && element.v_slot.is_some());
        self.in_slot_context_stack.push(is_slot_parent);

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
        //
        // Teleport/KeepAlive are ALWAYS block roots — official Vue emits
        // `(_openBlock(), _createBlock(_Teleport, …))` at ANY nesting depth,
        // not just at a single-root/v-if/v-for position.
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let raw_children_builtin = helpers::is_raw_children_builtin(tag_name);
        let is_root_child = self.ast.nodes[_id.0].parent.is_none();
        let is_single_template_root = is_root_child && self.single_root;
        let is_block_root = el.v_condition.is_some()
            || el.v_for.is_some()
            || is_single_template_root
            || raw_children_builtin;
        // Local `_openBlock()`: v-if/v-for branches always; a raw-children
        // built-in whenever it is NOT the sole single template root (the
        // single-root open block is provided once by leave_template).
        let force_open_block = raw_children_builtin && !is_single_template_root;

        // Handle component with slot children: wrap in slot object instead of array.
        // Teleport/KeepAlive are excluded — they take raw array children below.
        if el.tag_type.is_component()
            && !raw_children_builtin
            && self.has_slot_children(el_children)
        {
            self.leave_component_with_slots(_id, el, oxc, el_children, source, out, is_block_root);
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
        let cache_idx = if self.options.hoist_static
            && el.is_fully_static
            && !is_block_root
            && el.v_condition.is_none()
            && el.v_for.is_none()
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
            && self.in_slot_context_stack.last().copied().unwrap_or(false)
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
                .unwrap_or(false);

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
            force_open_block,
            Some(&mut self.hoisted_constants),
            cache_idx,
            Some(&mut self.resolved_components),
            slot_cached,
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
mod tests;
