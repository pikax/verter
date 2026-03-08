//! SSR (Server-Side Rendering) template code generation backend.
//!
//! Produces string-concatenation output using `_push()` + template literals,
//! matching Vue's `@vue/compiler-ssr` output format:
//!
//! ```js
//! function ssrRender(_ctx, _push, _parent, _attrs) {
//!   _push(`<div${_ssrRenderAttrs(_attrs)}>hello</div>`)
//! }
//! ```
//!
//! ## Push Buffering Strategy
//!
//! Vue's SSR compiler batches all static/interpolation content into a single
//! `_push()` template literal call. Structural directives (v-if, v-for) and
//! components cause the template literal to be "flushed" — closed with `` `) ``
//! — and a new literal is opened after the structural construct.
//!
//! This backend tracks an `in_push` flag to know whether we're inside an open
//! `_push(\`...\`)` template literal. Content that can be inlined (text,
//! interpolations, static elements) is appended to the current literal.
//! Structural breaks close the literal, emit their code, and subsequent
//! content opens a new literal.

use std::fmt::Write;

use crate::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeConditionKind, InterpolationNode, PropFlags,
    TagType, TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::template::oxc::types::{
    OxcNodeData, OxcParsedAst, OxcParsedElement, OxcParsedExpression,
};
use crate::types::{NodeId, NodeProp};
use crate::utils::vue::tag::is_void_tag;

use super::binding::{BindingResolver, ReactivityLevel};
use super::shared::helpers::{self, is_builtin_component, to_pascal_case, SsrHelper, VdomHelper};
use super::types::CodeGenOutput;
use super::vdom::element::resolve_expr;
use super::vdom::props::{camelize, format_event_handler_key_into, needs_quoted_key};
use super::{TemplateCodeGen, TemplateCodeGenOptions};

/// What each element pushed onto `elem_ctx` means for `leave_element`.
#[derive(Debug, Clone, PartialEq)]
enum ElemCtx {
    /// Element inside parent's push literal — emit `</tag>` only, no push management.
    InParentPush,
    /// Element opened its own push — emit `</tag>` + close push (`` `) ``).
    OwnPush,
    /// Element fully handled in enter_element (component, void, v-html/v-text).
    Complete,
    /// Component with slot children — `leave_element` closes the slots object.
    ComponentWithSlots,
    /// Dynamic component (`<component :is>`) with slot children —
    /// `leave_element` closes slots + `_createVNode` + `_ssrRenderVNode`.
    DynamicComponentWithSlots,
    /// `<template v-slot>` wrapper inside a component — marks a named slot.
    SlotTemplate,
    /// `<slot>` outlet with fallback children.
    SlotOutletFallback,
    /// `<Suspense>` with slot children — `leave_element` closes `_ssrRenderSuspense`.
    SuspenseSlots,
    /// `<template v-slot>` inside a `<Suspense>` — uses simple arrow functions, no `_withCtx`.
    SuspenseSlotTemplate,
    /// Transparent built-in (Transition, KeepAlive) — children render directly,
    /// `leave_element` removes the closing tag.
    TransparentBuiltin,
    /// `<TransitionGroup tag="X">` — renders as `<X attrs>...children...</X>`.
    /// Stores the resolved tag name for the closing tag.
    TransitionGroupTag(String),
    /// `<Teleport>` body — `leave_element` closes `_ssrRenderTeleport`.
    TeleportBody,
}

/// SSR code generation backend.
///
/// Produces `_push(\`...\`)` string-concatenation calls for server-side
/// rendering. Root elements merge `_attrs` via `_ssrRenderAttrs()`; nested
/// elements use plain HTML literals.
pub struct SsrCodeGen<'ast, 'alloc> {
    /// Reference to the template AST arena for O(1) node lookups.
    ast: &'ast TemplateAst,
    /// Reference to the OXC-parsed expressions overlay for binding resolution.
    /// Used by the VDOM fallback path to resolve compound expressions.
    oxc_ast: &'ast OxcParsedAst<'alloc>,
    resolver: BindingResolver<'alloc>,
    options: TemplateCodeGenOptions,
    /// Reusable buffer for building output strings.
    buf: String,
    /// Current nesting depth (0 = root). Root elements get `_ssrRenderAttrs(_attrs)`,
    /// nested elements use literal HTML.
    depth: u32,
    /// Whether we are currently inside an open `_push(\`...\` template literal.
    in_push: bool,
    /// Per-element context stack — tells `leave_element` what to do.
    elem_ctx: Vec<ElemCtx>,
    /// Collected `_resolveComponent()` declarations, hoisted to function preamble.
    component_resolves: Vec<String>,
    /// Collected `_resolveDirective()` declarations, hoisted to function preamble.
    directive_resolves: Vec<String>,
    /// Whether the template has multiple effective root nodes. Multi-root
    /// templates use `<!--[-->...<!--]-->` fragment markers and do NOT
    /// merge `_attrs` into individual root elements.
    is_multi_root: bool,
    /// Whether the template needs fragment markers `<!--[-->...<!--]-->`.
    /// This is true for multi-root templates AND single-root templates
    /// that have root-level comments (needed for hydration correctness).
    needs_fragment: bool,
    /// Depth counter for nested component-slot contexts. When > 0, we are
    /// inside a component's slot content and should not treat children as roots.
    in_component_slots: u32,
    /// Whether the SFC has `<style scoped>`. When true, the SSR output uses
    /// an 8-param `ssrRender` signature with `_scopeId`, appends `${_scopeId}`
    /// to element tags, and passes `_scopeId` to `_ssrRenderComponent` calls.
    has_scope_id: bool,
    /// Whether we've opened an implicit `default: _withCtx(...)` wrapper for
    /// non-template children inside a ComponentWithSlots that has named slots.
    default_slot_open: bool,
    /// Stack of saved `default_slot_open` values for nested component scopes.
    /// When entering a nested component, the current value is pushed here and
    /// `default_slot_open` is reset for the new component scope.
    saved_default_slot_open: Vec<bool>,
    /// Stack of closing-argument strings for nested `<Teleport>` elements.
    /// Each entry contains the args to append after the callback: `, "body", false, _parent)`.
    teleport_closing_args: Vec<String>,
    /// Whether we've opened an implicit `default: () => {` inside a Suspense
    /// with mixed content. Cleared when the first `<template v-slot>` closes it.
    suspense_implicit_default_open: bool,
    /// When inside a `<select v-model="expr">`, stores the resolved v-model
    /// expression so that child `<option>` elements can inject the `selected`
    /// attribute check (`_ssrIncludeBooleanAttr(... ? _ssrLooseContain : _ssrLooseEqual)`).
    select_v_model_expr: Option<String>,
    /// Start position of an implicit default slot that was opened before named
    /// slots. Used by `leave_element` to reorder default slot after named slots
    /// via `move_slice`.
    default_slot_move_start: Option<u32>,
    /// End position (exclusive) of the default slot range to move. Set when
    /// the default slot is closed because a `<template v-slot>` was encountered.
    default_slot_move_end: Option<u32>,
    /// Saved move tracking for nested component scopes.
    saved_default_slot_move: Vec<(Option<u32>, Option<u32>)>,
    /// Start of component children region (tag_open.end of ComponentWithSlots).
    /// Used as the move_start position to ensure all Inserted chunks at child
    /// positions are captured even when whitespace overwrites leave position gaps.
    comp_children_start: Option<u32>,
    /// Nesting depth of v-for loops. When > 0, a component's slots are
    /// considered DYNAMIC (matching Vue's behavior).
    /// Uses Cell for interior mutability since VDOM generation uses `&self`.
    v_for_depth: std::cell::Cell<u32>,
    /// Nesting depth of scoped slots (slots with user-defined parameters).
    /// When > 0, child component slots are considered DYNAMIC because the
    /// scoped slot context may re-render, requiring child slots to be dynamic.
    v_slot_scope_depth: u32,
    /// Stack tracking whether each entered slot context incremented v_slot_scope_depth.
    /// Pushed on ComponentWithSlots/DynamicComponentWithSlots/SlotTemplate enter,
    /// popped on leave. True means we incremented and need to decrement on leave.
    scoped_slot_entered: Vec<bool>,
    /// Whether the template uses the `_temp0` variable pattern for root v-model
    /// on native elements. When true, `let _temp0\n` is emitted in the preamble.
    temp_var_needed: bool,
}

impl<'ast, 'alloc> SsrCodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        oxc_ast: &'ast OxcParsedAst<'alloc>,
        resolver: BindingResolver<'alloc>,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            oxc_ast,
            resolver,
            options: options.clone(),
            buf: String::with_capacity(256),
            depth: 0,
            in_push: false,
            elem_ctx: Vec::with_capacity(16),
            component_resolves: Vec::new(),
            directive_resolves: Vec::new(),
            is_multi_root: false,
            needs_fragment: false,
            in_component_slots: 0,
            // Disabled for now: Vue inlines literal scope IDs, not runtime _scopeId params.
            // TODO: implement literal scope ID injection (e.g., `data-v-xxxxx`).
            has_scope_id: false,
            default_slot_open: false,
            saved_default_slot_open: Vec::new(),
            teleport_closing_args: Vec::new(),
            suspense_implicit_default_open: false,
            select_v_model_expr: None,
            default_slot_move_start: None,
            default_slot_move_end: None,
            saved_default_slot_move: Vec::new(),
            comp_children_start: None,
            v_for_depth: std::cell::Cell::new(0),
            v_slot_scope_depth: 0,
            scoped_slot_entered: Vec::new(),
            temp_var_needed: false,
        }
    }

    // ── Scope ID helpers ─────────────────────────────────────────

    /// Returns `${_scopeId}` when scoped styles are present, empty string otherwise.
    #[inline]
    fn scope_id_suffix(&self) -> &'static str {
        if self.has_scope_id {
            "${_scopeId}"
        } else {
            ""
        }
    }

    /// Returns `, _scopeId` when scoped styles are present, empty string otherwise.
    /// Used as extra argument to `_ssrRenderComponent`.
    #[inline]
    fn scope_id_arg(&self) -> &'static str {
        if self.has_scope_id {
            ", _scopeId"
        } else {
            ""
        }
    }

    // ── Directive resolution ──────────────────────────────────

    /// Resolve a global (non-setup) directive: emit `_resolveDirective("name")` declaration
    /// and return the variable name `_directive_name`.
    fn resolve_directive_global(
        &mut self,
        directive_name: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let var_name = format!("_directive_{}", directive_name.replace('-', "_"));
        let decl = format!(
            "const {} = _resolveDirective(\"{}\")",
            var_name, directive_name
        );
        if !self.directive_resolves.contains(&decl) {
            self.directive_resolves.push(decl);
            out.add_vdom_import(VdomHelper::ResolveDirective);
        }
        var_name
    }

    /// Build a `_ssrGetDirectiveProps(_ctx, ref, value?, arg?, modifiers?)` call
    /// for a custom directive prop on a component. Returns the call string and
    /// registers the SSR helper import + directive resolution.
    fn build_directive_props_call(
        &mut self,
        prop: &crate::types::NodeProp,
        prop_name: &str,
        source: &str,
        oxc: Option<&crate::template::oxc::types::OxcParsedElement>,
        i: usize,
        out: &mut CodeGenOutput<'alloc>,
    ) -> Option<String> {
        let directive_name = prop_name.strip_prefix("v-")?;
        let binding_name = directive_to_camel(directive_name);

        // Resolve directive reference: setup binding or _resolveDirective()
        let directive_ref = if let Some(bt) = self.resolver.get(&binding_name) {
            if bt.is_setup() {
                format!("$setup[\"{}\"]", binding_name)
            } else {
                self.resolve_directive_global(directive_name, out)
            }
        } else {
            self.resolve_directive_global(directive_name, out)
        };

        let mut dir_call = format!("_ssrGetDirectiveProps(_ctx, {}", directive_ref);

        // Value (optional)
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let expr = &source[vs as usize..ve as usize];
            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
            let resolved = self.resolve_expr(expr, vs, oxc_expr);
            dir_call.push_str(", ");
            dir_call.push_str(&resolved);
        }

        // Argument (optional)
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            if prop.value_start.is_none() {
                dir_call.push_str(", void 0");
            }
            if prop.is_dynamic == Some(true) {
                let raw_arg = &source[as_ as usize..ae as usize];
                let arg = raw_arg
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .unwrap_or(raw_arg);
                let arg_offset = if raw_arg.starts_with('[') {
                    as_ + 1
                } else {
                    as_
                };
                let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                let oxc_expr = oxc_prop.and_then(|p| p.arg.as_ref());
                let resolved = self.resolve_expr(arg, arg_offset, oxc_expr);
                dir_call.push_str(", ");
                dir_call.push_str(&resolved);
            } else {
                let arg = &source[as_ as usize..ae as usize];
                dir_call.push_str(", \"");
                dir_call.push_str(arg);
                dir_call.push('"');
            }
        }

        // Modifiers (optional)
        if !prop.modifiers.is_empty() {
            if prop.arg_start.is_none() {
                if prop.value_start.is_none() {
                    dir_call.push_str(", void 0");
                }
                dir_call.push_str(", void 0");
            }
            dir_call.push_str(", { ");
            for (j, modifier) in prop.modifiers.iter().enumerate() {
                let mod_name = &source[modifier.start as usize..modifier.end as usize];
                if j > 0 {
                    dir_call.push_str(", ");
                }
                dir_call.push_str(mod_name);
                dir_call.push_str(": true");
            }
            dir_call.push_str(" }");
        }

        dir_call.push(')');
        out.add_ssr_import(SsrHelper::GetDirectiveProps);
        Some(dir_call)
    }

    // ── Root detection ─────────────────────────────────────────

    /// Whether we're currently at root depth (the element is a direct child of `<template>`).
    fn is_root(&self) -> bool {
        self.depth == 0 && !self.is_multi_root && self.in_component_slots == 0
    }

    // ── Push management ────────────────────────────────────────

    /// Close the current `_push(\`...\`)` literal if one is open.
    fn close_push(&mut self, pos: u32, out: &mut CodeGenOutput<'alloc>) {
        if self.in_push {
            out.prepend_alloc(pos, "`)\n");
            self.in_push = false;
        }
    }

    /// Ensure a `_push(\`...\`)` literal is open. If not, open one.
    fn ensure_push(&mut self, pos: u32, out: &mut CodeGenOutput<'alloc>) {
        if !self.in_push {
            out.prepend_alloc(pos, "_push(`");
            self.in_push = true;
        }
    }

    // ── Tag and position helpers ───────────────────────────────

    /// Extract the tag name from the element's open tag.
    fn tag_name(&self, el: &ElementNode, source: &str) -> String {
        source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize].to_string()
    }

    /// Get the end position of the element (end of close tag, or end of self-closing tag).
    fn el_end(&self, el: &ElementNode) -> u32 {
        el.tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end)
    }

    /// Look up OXC parsed data for a given node ID.
    fn oxc_element(&self, id: NodeId) -> Option<&OxcParsedElement<'alloc>> {
        match &self.oxc_ast.data[id.0] {
            OxcNodeData::Element(e) => Some(e.as_ref()),
            _ => None,
        }
    }

    /// Look up OXC parsed interpolation for a given node ID.
    fn oxc_interpolation(&self, id: NodeId) -> Option<&OxcParsedExpression<'alloc>> {
        match &self.oxc_ast.data[id.0] {
            OxcNodeData::Interpolation(expr) => Some(expr),
            _ => None,
        }
    }

    /// Resolve an expression using the binding resolver.
    /// Note: SSR does NOT strip TypeScript syntax — Vue's SSR compiler preserves
    /// `as` casts, `!` assertions, etc. Bundler-level TS stripping handles these.
    fn resolve_expr(
        &self,
        expr: &str,
        offset: u32,
        oxc_expr: Option<&OxcParsedExpression<'alloc>>,
    ) -> String {
        resolve_expr(expr, offset, oxc_expr, &self.resolver, false)
    }

    /// Count effective root children (non-whitespace text, elements, interpolations).
    /// v-else-if and v-else branches don't count as separate roots since only one
    /// branch renders at a time.
    /// Count element-level roots, excluding text, interpolation, and comments.
    ///
    /// Used for fragment marker decisions: Vue SSR only emits `<!--[-->...<!--]-->`
    /// when there are 2+ element roots. Text/interpolation at root level is just
    /// inline content and doesn't trigger fragment wrapping.
    fn count_element_roots(&self, root_children: &[NodeId]) -> usize {
        let mut count = 0;
        for &child_id in root_children {
            let child = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref el) = child.kind {
                // v-else-if and v-else don't count as separate roots
                if let Some(ref cond) = el.v_condition {
                    if matches!(
                        cond.kind,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                    ) {
                        continue;
                    }
                }
                count += 1;
            }
        }
        count
    }

    fn count_effective_roots(&self, root_children: &[NodeId], source: &str) -> usize {
        let mut count = 0;
        for &child_id in root_children {
            let child = &self.ast.nodes[child_id.0];
            match &child.kind {
                AstNodeKind::Element(el) => {
                    // v-else-if and v-else don't count as separate roots
                    if let Some(ref cond) = el.v_condition {
                        if matches!(
                            cond.kind,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                        ) {
                            continue;
                        }
                    }
                    // v-for produces 0..N elements — treat as multi-root so
                    // _attrs is NOT applied to each iteration element.
                    if el.v_for.is_some() {
                        count += 2;
                    } else {
                        count += 1;
                    }
                }
                AstNodeKind::Interpolation(_) => count += 1,
                // Comments don't count as effective roots — a template with
                // `<!-- comment --><div>...</div>` is single-root in Vue SSR.
                AstNodeKind::Comment(_) => {}
                AstNodeKind::Text(t) => {
                    let text = &source[t.start as usize..t.end as usize];
                    if !text.trim().is_empty() {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    // ── VDOM fallback for slot else branches ─────────────────────

    /// Generate VDOM VNode array for a slot's else branch.
    ///
    /// In Vue's SSR output, slot callbacks have both an SSR `if (_push)` branch
    /// and a VDOM `else { return [...] }` fallback. This method generates the
    /// VNode array contents for the else branch.
    fn generate_vdom_fallback(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let children = match el.content {
            Some(ref content) => &content.children[..],
            None => return "[]".to_string(),
        };
        self.generate_vdom_children(children, source, out)
    }

    /// Generate VDOM fallback for only the non-template-v-slot children
    /// (the implicit default slot content).
    fn generate_vdom_fallback_default(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let children = match el.content {
            Some(ref content) => &content.children[..],
            None => return "[]".to_string(),
        };
        // Filter out template v-slot children — only include default slot content
        let default_children: Vec<NodeId> = children
            .iter()
            .filter(|&&cid| {
                if let AstNodeKind::Element(ref child_el) = self.ast.nodes[cid.0].kind {
                    !(child_el.tag_type == TagType::Template && child_el.v_slot.is_some())
                } else {
                    true
                }
            })
            .copied()
            .collect();
        self.generate_vdom_children(&default_children, source, out)
    }

    /// Generate a VDOM VNode array string from a list of child node IDs.
    /// Adjacent text + interpolation nodes are merged into a single `_createTextVNode`
    /// call with string concatenation (e.g., `"Hello " + _toDisplayString(name)`).
    fn generate_vdom_children(
        &self,
        children: &[NodeId],
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let mut vnodes: Vec<String> = Vec::new();
        // Shared key counter across all v-if chains in this parent scope.
        // Vue assigns globally unique keys within a parent's children.
        let mut vif_key_counter: u32 = 0;

        let mut i = 0;
        while i < children.len() {
            let child = &self.ast.nodes[children[i].0];
            match &child.kind {
                AstNodeKind::Text(_) | AstNodeKind::Interpolation(_) => {
                    // Collect adjacent text/interpolation run and merge into one _createTextVNode
                    let mut parts: Vec<String> = Vec::new();
                    let mut has_interp = false;
                    while i < children.len() {
                        let node = &self.ast.nodes[children[i].0];
                        match &node.kind {
                            AstNodeKind::Text(t) => {
                                let text = &source[t.start as usize..t.end as usize];
                                let condensed = condense_whitespace(text);
                                if !condensed.trim().is_empty() {
                                    // Decode HTML entities (e.g. &nbsp; → U+00A0)
                                    // then escape for JS string literal
                                    let decoded = decode_html_entities(&condensed);
                                    parts.push(format!("\"{}\"", Self::escape_vdom_text(&decoded)));
                                } else if has_interp && condensed == " " {
                                    // Whitespace-only text between interpolation and next node:
                                    // include as " " so `{{ a }} {{ b }}` produces
                                    // `_toDisplayString(a) + " " + _toDisplayString(b)`
                                    // Only add when preceded by interpolation AND followed by
                                    // another interpolation (not at end of text run).
                                    let next_is_interp = if i + 1 < children.len() {
                                        let n = &self.ast.nodes[children[i + 1].0];
                                        matches!(n.kind, AstNodeKind::Interpolation(_))
                                    } else {
                                        false
                                    };
                                    if next_is_interp {
                                        parts.push("\" \"".to_string());
                                    }
                                }
                            }
                            AstNodeKind::Interpolation(interp) => {
                                let expr =
                                    &source[interp.inner_start as usize..interp.inner_end as usize];
                                let oxc_interp = self.oxc_interpolation(children[i]);
                                let resolved =
                                    self.resolve_expr(expr, interp.inner_start, oxc_interp);
                                out.add_vdom_import(VdomHelper::ToDisplayString);
                                parts.push(format!("_toDisplayString({})", resolved));
                                has_interp = true;
                            }
                            _ => break,
                        }
                        i += 1;
                    }
                    if !parts.is_empty() {
                        let joined = parts.join(" + ");
                        out.add_vdom_import(VdomHelper::CreateTextVNode);
                        if has_interp {
                            vnodes.push(format!("_createTextVNode({}, 1 /* TEXT */)", joined));
                        } else {
                            vnodes.push(format!("_createTextVNode({})", joined));
                        }
                    }
                    continue; // i already advanced
                }
                AstNodeKind::Element(ref child_el) => {
                    // v-if chain: generate ternary with _openBlock/_createBlock + key
                    if child_el
                        .v_condition
                        .as_ref()
                        .is_some_and(|c| c.kind == ElementNodeConditionKind::If)
                    {
                        let vif_expr = self.generate_vdom_vif_chain(
                            children,
                            &mut i,
                            &mut vif_key_counter,
                            source,
                            out,
                        );
                        vnodes.push(vif_expr);
                        continue; // i already advanced past the chain
                    }
                    // v-else-if / v-else without a preceding v-if shouldn't happen,
                    // but skip gracefully if it does.
                    if child_el.v_condition.is_some() {
                        i += 1;
                        continue;
                    }
                    // v-for: wrap in (_openBlock(true/false), _createBlock(_Fragment, null, _renderList(...)))
                    if let Some(ref v_for) = child_el.v_for {
                        let vfor_expr =
                            self.generate_vdom_vfor(child_el, children[i], v_for, source, out);
                        vnodes.push(vfor_expr);
                        i += 1;
                        continue;
                    }
                    vnodes.push(self.generate_vdom_element(child_el, children[i], source, out));
                }
                AstNodeKind::Comment(c) => {
                    let text = &source[c.content_start as usize..c.content_end as usize];
                    out.add_vdom_import(VdomHelper::CreateCommentVNode);
                    let escaped = Self::escape_vdom_text(text);
                    vnodes.push(format!("_createCommentVNode(\"{}\")", escaped));
                }
            }
            i += 1;
        }

        if vnodes.is_empty() {
            "[]".to_string()
        } else {
            format!("[\n{}\n]", vnodes.join(",\n"))
        }
    }

    /// Generate a VDOM `_createVNode(...)` call for a single element.
    fn generate_vdom_element(
        &self,
        el: &ElementNode,
        node_id: NodeId,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let oxc = self.oxc_element(node_id);

        if el.tag_type.is_component() {
            return self.generate_vdom_component(el, tag_name, oxc, source, out);
        }

        // <slot> → _renderSlot(_ctx.$slots, "name", props, fallback)
        if el.tag_type == TagType::SlotOutlet {
            out.add_vdom_import(VdomHelper::RenderSlot);
            // Determine slot name from `name` attribute or "default"
            let (slot_name, is_dynamic) = self.extract_slot_outlet_name(el, oxc, source);
            if is_dynamic {
                return format!("_renderSlot(_ctx.$slots, {})", slot_name);
            }
            return format!("_renderSlot(_ctx.$slots, \"{}\")", slot_name);
        }

        // <template> (non-slot) is transparent in VDOM: unwrap its children as a Fragment.
        // Vue generates (_openBlock(), _createBlock(_Fragment, ...key..., [...children]))
        if el.tag_type == TagType::Template {
            let children = match el.content {
                Some(ref content) => &content.children[..],
                None => &[],
            };
            if children.is_empty() {
                out.add_vdom_import(VdomHelper::CreateCommentVNode);
                return "_createCommentVNode(\"v-if\", true)".to_string();
            }
            // For v-for on <template>, use Fragment wrapper
            out.add_vdom_import(VdomHelper::OpenBlock);
            out.add_vdom_import(VdomHelper::CreateBlock);
            out.add_vdom_import(VdomHelper::Fragment);
            let children_arr = self.generate_vdom_children(children, source, out);
            // Check for :key prop
            let key_prop = el.props.iter().enumerate().find(|(_, p)| {
                if !p.is_directive {
                    return false;
                }
                if let (Some(as_), Some(ae)) = (p.arg_start, p.arg_end) {
                    &source[as_ as usize..ae as usize] == "key"
                } else {
                    false
                }
            });
            if let Some((ki, kp)) = key_prop {
                if let (Some(vs), Some(ve)) = (kp.value_start, kp.value_end) {
                    let key_expr = &source[vs as usize..ve as usize];
                    let oxc_expr = oxc
                        .and_then(|o| find_oxc_prop(o, ki))
                        .and_then(|p| p.exp.as_ref());
                    let resolved = self.resolve_expr(key_expr, vs, oxc_expr);
                    return format!(
                        "(_openBlock(), _createBlock(_Fragment, {{ key: {} }}, {}, 64 /* STABLE_FRAGMENT */))",
                        resolved, children_arr
                    );
                }
            }
            return format!(
                "(_openBlock(), _createBlock(_Fragment, null, {}, 64 /* STABLE_FRAGMENT */))",
                children_arr
            );
        }

        out.add_vdom_import(VdomHelper::CreateVNode);

        // Build props
        let props_str = self.generate_vdom_props(el, oxc, source, out);

        // Build children
        let children_str = self.generate_vdom_element_children(el, source, out);

        // Compute patch flags for the element
        let (patch_flag, dynamic_props) =
            self.compute_element_patch_flag(el, tag_name, source, &children_str);

        // Assemble _createVNode("tag", props, children, patchFlag, dynamicProps)
        let mut result = format!("_createVNode(\"{}\"", tag_name);
        if props_str == "null" && children_str == "null" && patch_flag == 0 {
            // No props, no children, no flags: just the tag
            result.push(')');
        } else {
            result.push_str(", ");
            result.push_str(&props_str);
            if children_str != "null" || patch_flag != 0 {
                result.push_str(", ");
                result.push_str(if children_str == "null" {
                    "null"
                } else {
                    &children_str
                });
            }
            if patch_flag != 0 {
                result.push_str(", ");
                result.push_str(&Self::format_patch_flag(patch_flag));
                if !dynamic_props.is_empty() {
                    result.push_str(", [");
                    for (i, name) in dynamic_props.iter().enumerate() {
                        if i > 0 {
                            result.push_str(", ");
                        }
                        result.push('"');
                        result.push_str(name);
                        result.push('"');
                    }
                    result.push(']');
                }
            }
            result.push(')');
        }
        result
    }

    /// Generate a v-if/v-else-if/v-else chain as a VDOM ternary expression.
    ///
    /// Vue's SSR VDOM fallback generates:
    /// ```js
    /// (cond) ? (_openBlock(), _createBlock("tag", { key: 0 }))
    ///        : (cond2) ? (_openBlock(), _createBlock("tag2", { key: 1 }))
    ///                  : _createCommentVNode("v-if", true)
    /// ```
    /// `i` points to the v-if element. On return, `i` points past the last element in the chain.
    /// `vif_key_counter` is shared across all v-if chains in the same parent, so keys are
    /// globally unique (matching Vue's behavior).
    fn generate_vdom_vif_chain(
        &self,
        children: &[NodeId],
        i: &mut usize,
        vif_key_counter: &mut u32,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        out.add_vdom_import(VdomHelper::OpenBlock);
        out.add_vdom_import(VdomHelper::CreateBlock);

        let mut result = String::new();
        let key_start = *vif_key_counter;
        let mut key_index: u32 = key_start;
        let mut has_else = false;

        // Process the v-if element and all subsequent v-else-if / v-else elements
        loop {
            if *i >= children.len() {
                break;
            }
            let node = &self.ast.nodes[children[*i].0];
            let el = match &node.kind {
                AstNodeKind::Element(el) => el,
                _ => break,
            };
            let cond = match &el.v_condition {
                Some(c) => c,
                None => break,
            };

            match cond.kind {
                ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                    // Extract condition expression with OXC binding resolution
                    let oxc_cond = self
                        .oxc_element(children[*i])
                        .and_then(|o| o.condition.as_ref());
                    let cond_expr = if let (Some(vs), Some(ve)) =
                        (cond.prop.value_start, cond.prop.value_end)
                    {
                        let expr = &source[vs as usize..ve as usize];
                        self.resolve_expr(expr, vs, oxc_cond)
                    } else {
                        "true".to_string()
                    };

                    if key_index > key_start {
                        // v-else-if: chain with " : "
                        result.push_str("\n  : ");
                    }
                    result.push('(');
                    result.push_str(&cond_expr);
                    result.push_str(")\n  ? ");

                    // Generate the block for this branch
                    let block = self.generate_vdom_block(el, children[*i], key_index, source, out);
                    result.push_str(&block);
                }
                ElementNodeConditionKind::Else => {
                    result.push_str("\n  : ");
                    let block = self.generate_vdom_block(el, children[*i], key_index, source, out);
                    result.push_str(&block);
                    has_else = true;
                    *i += 1;
                    break;
                }
            }

            key_index += 1;
            *i += 1;

            // Skip whitespace text and comment nodes between v-if branches
            while *i < children.len() {
                let next = &self.ast.nodes[children[*i].0];
                match &next.kind {
                    AstNodeKind::Text(t) => {
                        let text = &source[t.start as usize..t.end as usize];
                        if text.trim().is_empty() {
                            *i += 1;
                        } else {
                            break;
                        }
                    }
                    AstNodeKind::Comment(_) => {
                        *i += 1; // Skip comments between v-if branches
                    }
                    AstNodeKind::Element(next_el) => {
                        if next_el.v_condition.as_ref().is_some_and(|c| {
                            matches!(
                                c.kind,
                                ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                            )
                        }) {
                            break; // next element is part of the chain — continue loop
                        }
                        break; // not part of chain — stop
                    }
                    _ => break,
                }
            }

            // Check if the next element continues the chain
            if *i < children.len() {
                let next = &self.ast.nodes[children[*i].0];
                if let AstNodeKind::Element(next_el) = &next.kind {
                    if next_el.v_condition.as_ref().is_some_and(|c| {
                        matches!(
                            c.kind,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                        )
                    }) {
                        continue; // process the next branch
                    }
                }
            }
            break;
        }

        // If no v-else, add _createCommentVNode("v-if", true)
        if !has_else {
            out.add_vdom_import(VdomHelper::CreateCommentVNode);
            result.push_str("\n  : _createCommentVNode(\"v-if\", true)");
        }

        // Update the shared key counter so subsequent v-if chains continue numbering
        *vif_key_counter = key_index;

        result
    }

    /// Generate `(_openBlock(true), _createBlock(_Fragment, null, _renderList(...), FLAG))`
    /// for a v-for element in the VDOM fallback path.
    fn generate_vdom_vfor(
        &self,
        el: &ElementNode,
        node_id: NodeId,
        v_for: &NodeProp,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        out.add_vdom_import(VdomHelper::OpenBlock);
        out.add_vdom_import(VdomHelper::CreateBlock);
        out.add_vdom_import(VdomHelper::Fragment);
        out.add_vdom_import(VdomHelper::RenderList);

        let full_expr = helpers::extract_directive_value(v_for, source);
        let (params, iterable) = helpers::parse_v_for_expression(full_expr);

        // Resolve bindings in the iterable expression
        let oxc = self.oxc_element(node_id);
        let resolved_iterable = if let Some(oxc_el) = oxc {
            if let Some(ref vfor_data) = oxc_el.v_for {
                let refs = &vfor_data.parsed.references;
                if refs.is_empty() {
                    iterable.to_string()
                } else {
                    super::vdom::directives::build_prefixed_iterable(
                        iterable,
                        source,
                        v_for,
                        &vfor_data.parsed,
                        &self.resolver,
                    )
                }
            } else {
                self.resolver.resolve_simple_expr(iterable)
            }
        } else {
            self.resolver.resolve_simple_expr(iterable)
        };

        // Determine if keyed (has :key prop)
        let has_key = el.props.iter().any(|p| {
            if !p.is_directive {
                return false;
            }
            if let (Some(as_), Some(ae)) = (p.arg_start, p.arg_end) {
                &source[as_ as usize..ae as usize] == "key"
            } else {
                false
            }
        });
        let frag_flag = if has_key {
            "128 /* KEYED_FRAGMENT */"
        } else {
            "256 /* UNKEYED_FRAGMENT */"
        };

        // Determine if numeric range (e.g., `v-for="n in 10"`)
        let is_numeric = iterable.trim().parse::<f64>().is_ok();

        // Generate the element inside the v-for callback.
        // For keyed v-for, each iteration's root element gets its own block:
        // (_openBlock(), _createBlock(...)) instead of _createVNode(...)
        // Track v-for depth so component slots inside v-for are marked DYNAMIC.
        self.v_for_depth.set(self.v_for_depth.get() + 1);
        let inner = self.generate_vdom_element(el, node_id, source, out);
        self.v_for_depth.set(self.v_for_depth.get() - 1);
        // For keyed v-for, each iteration's root element gets its own block:
        // (_openBlock(), _createBlock(...)) instead of _createVNode(...)
        let inner = if has_key && inner.starts_with("_createVNode(") {
            out.add_vdom_import(VdomHelper::CreateBlock);
            let rest = &inner["_createVNode(".len()..];
            format!("(_openBlock(), _createBlock({})", rest)
        } else {
            inner
        };

        format!(
            "(_openBlock({}), _createBlock(_Fragment, null, _renderList({}, ({}) => {{\n  return {}\n}}), {}))",
            if is_numeric { "" } else { "true" },
            resolved_iterable, params, inner, frag_flag
        )
    }

    /// Generate `(_openBlock(), _createBlock(..., { key: N, ...props }, children))` for a
    /// single v-if/v-else-if/v-else branch element.
    fn generate_vdom_block(
        &self,
        el: &ElementNode,
        node_id: NodeId,
        key_index: u32,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let oxc = self.oxc_element(node_id);

        if el.tag_type.is_component() {
            return self.generate_vdom_block_component(el, tag_name, oxc, key_index, source, out);
        }

        // <template v-if> — transparent wrapper. When there's a single child
        // element, Vue promotes it to be the block directly (with key injected).
        // Multiple children get Fragment wrapping.
        if el.tag_type == TagType::Template {
            let children = match el.content {
                Some(ref content) => &content.children[..],
                None => &[],
            };
            // Count effective children (skip whitespace-only text)
            let effective: Vec<NodeId> = children
                .iter()
                .filter(|&&cid| {
                    let node = &self.ast.nodes[cid.0];
                    match &node.kind {
                        AstNodeKind::Text(t) => {
                            let text = &source[t.start as usize..t.end as usize];
                            !text.trim().is_empty()
                        }
                        _ => true,
                    }
                })
                .copied()
                .collect();

            if effective.len() == 1 {
                // Single child: promote to block with key
                let child_node = &self.ast.nodes[effective[0].0];
                if let AstNodeKind::Element(ref child_el) = child_node.kind {
                    // Re-use the regular block generation but inject the key
                    let child_tag = &source
                        [child_el.tag_open.start as usize + 1..child_el.tag_open.name_end as usize];
                    let child_oxc = self.oxc_element(effective[0]);

                    if child_el.tag_type.is_component() {
                        return self.generate_vdom_block_component(
                            child_el, child_tag, child_oxc, key_index, source, out,
                        );
                    }

                    let props_str = self.generate_vdom_props(child_el, child_oxc, source, out);
                    let props_with_key = if props_str == "null" {
                        format!("{{ key: {} }}", key_index)
                    } else {
                        let inner = props_str
                            .strip_prefix('{')
                            .and_then(|s| s.strip_suffix('}'))
                            .unwrap_or(&props_str);
                        let inner = inner.trim();
                        if inner.is_empty() {
                            format!("{{ key: {} }}", key_index)
                        } else {
                            format!("{{ key: {}, {} }}", key_index, inner)
                        }
                    };

                    let children_str = self.generate_vdom_element_children(child_el, source, out);
                    let (patch_flag, dynamic_props) =
                        self.compute_element_patch_flag(child_el, child_tag, source, &children_str);

                    let mut result = format!("(_openBlock(), _createBlock(\"{}\"", child_tag);
                    result.push_str(", ");
                    result.push_str(&props_with_key);
                    if children_str != "null" || patch_flag != 0 {
                        result.push_str(", ");
                        result.push_str(if children_str == "null" {
                            "null"
                        } else {
                            &children_str
                        });
                    }
                    if patch_flag != 0 {
                        result.push_str(", ");
                        result.push_str(&Self::format_patch_flag(patch_flag));
                        if !dynamic_props.is_empty() {
                            result.push_str(", [");
                            for (i, name) in dynamic_props.iter().enumerate() {
                                if i > 0 {
                                    result.push_str(", ");
                                }
                                result.push('"');
                                result.push_str(name);
                                result.push('"');
                            }
                            result.push(']');
                        }
                    }
                    result.push_str("))");
                    return result;
                }
            }

            // Multiple children or non-element single child: wrap in Fragment
            out.add_vdom_import(VdomHelper::Fragment);
            let children_arr = self.generate_vdom_children(children, source, out);
            return format!(
                "(_openBlock(), _createBlock(_Fragment, {{ key: {} }}, {}, 64 /* STABLE_FRAGMENT */))",
                key_index, children_arr
            );
        }

        // Build props with key injected
        let props_str = self.generate_vdom_props(el, oxc, source, out);
        let props_with_key = if props_str == "null" {
            format!("{{ key: {} }}", key_index)
        } else {
            // Inject key at the start of the props object
            let inner = props_str
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or(&props_str);
            let inner = inner.trim();
            if inner.is_empty() {
                format!("{{ key: {} }}", key_index)
            } else {
                format!("{{ key: {}, {} }}", key_index, inner)
            }
        };

        // Build children
        let children_str = self.generate_vdom_element_children(el, source, out);
        let (patch_flag, dynamic_props) =
            self.compute_element_patch_flag(el, tag_name, source, &children_str);

        let mut result = format!("(_openBlock(), _createBlock(\"{}\"", tag_name);
        result.push_str(", ");
        result.push_str(&props_with_key);
        if children_str != "null" || patch_flag != 0 {
            result.push_str(", ");
            result.push_str(if children_str == "null" {
                "null"
            } else {
                &children_str
            });
        }
        if patch_flag != 0 {
            result.push_str(", ");
            result.push_str(&Self::format_patch_flag(patch_flag));
            if !dynamic_props.is_empty() {
                result.push_str(", [");
                for (i, name) in dynamic_props.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push('"');
                    result.push_str(name);
                    result.push('"');
                }
                result.push(']');
            }
        }
        result.push_str("))");
        result
    }

    /// Generate `(_openBlock(), _createBlock(CompRef, { key: N, ...props }, slots))` for a
    /// component v-if branch.
    fn generate_vdom_block_component(
        &self,
        el: &ElementNode,
        tag_name: &str,
        oxc: Option<&OxcParsedElement<'alloc>>,
        key_index: u32,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        // Resolve component reference (dynamic <component :is> or named)
        let (component_ref, is_prop_index) = if tag_name == "component" || tag_name == "Component" {
            if let Some((resolved, idx)) = self.resolve_dynamic_is(el, oxc, source, out) {
                out.add_vdom_import(VdomHelper::ResolveDynamicComponent);
                (resolved, Some(idx))
            } else {
                ("_component_component".to_string(), None)
            }
        } else {
            (self.resolve_vdom_component_ref(tag_name), None)
        };

        // Build props with key injected (skip :is prop for dynamic components)
        let props_str = self.generate_vdom_props_skip(el, oxc, source, out, is_prop_index);
        let props_with_key = if props_str == "null" {
            format!("{{ key: {} }}", key_index)
        } else {
            let inner = props_str
                .strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .unwrap_or(&props_str);
            let inner = inner.trim();
            if inner.is_empty() {
                format!("{{ key: {} }}", key_index)
            } else {
                format!("{{ key: {}, {} }}", key_index, inner)
            }
        };

        let has_children = self.has_effective_children(el, source);

        let mut result = format!("(_openBlock(), _createBlock({}", component_ref);
        result.push_str(", ");
        result.push_str(&props_with_key);

        if has_children {
            out.add_vdom_import(VdomHelper::WithCtx);
            let slot_flag = if self.has_dynamic_slots(el) {
                "_: 2 /* DYNAMIC */"
            } else if self.has_slot_outlet_in_descendants(el, source) {
                "_: 3 /* FORWARDED */"
            } else {
                "_: 1 /* STABLE */"
            };

            let slots = self.collect_vdom_named_slots(el, source, out);
            if slots.is_empty() {
                let children_vnodes = self.generate_vdom_fallback(el, source, out);
                let _ = write!(
                    result,
                    ", {{\ndefault: _withCtx(() => {}),\n{}\n}}",
                    children_vnodes, slot_flag
                );
            } else {
                result.push_str(", {\n");
                for (slot_name, slot_params, slot_vnodes) in &slots {
                    let _ = writeln!(
                        result,
                        "{}: _withCtx(({}) => {}),",
                        slot_name, slot_params, slot_vnodes
                    );
                }
                let default_vnodes = self.generate_vdom_fallback_default(el, source, out);
                if default_vnodes != "[]" {
                    let _ = writeln!(result, "default: _withCtx(() => {}),", default_vnodes);
                }
                result.push_str(slot_flag);
                result.push_str("\n}");
            }
            // Add PROPS/DYNAMIC_SLOTS patch flags after slot object
            let dynamic_props = self.collect_dynamic_prop_names(el, source);
            let has_dyn_slots = self.has_dynamic_slots(el);
            if !dynamic_props.is_empty() || has_dyn_slots {
                let mut flag: u32 = 0;
                if !dynamic_props.is_empty() {
                    flag |= 8; // PROPS
                }
                if has_dyn_slots {
                    flag |= 1024; // DYNAMIC_SLOTS
                }
                result.push_str(", ");
                result.push_str(&Self::format_patch_flag(flag));
                if !dynamic_props.is_empty() {
                    result.push_str(", [");
                    for (i, name) in dynamic_props.iter().enumerate() {
                        if i > 0 {
                            result.push_str(", ");
                        }
                        result.push('"');
                        result.push_str(name);
                        result.push('"');
                    }
                    result.push(']');
                }
            }
        } else {
            // No children: still add PROPS patch flag for dynamic bindings
            let dynamic_props = self.collect_dynamic_prop_names(el, source);
            if !dynamic_props.is_empty() {
                result.push_str(", null, ");
                result.push_str(&Self::format_patch_flag(8));
                result.push_str(", [");
                for (i, name) in dynamic_props.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push('"');
                    result.push_str(name);
                    result.push('"');
                }
                result.push(']');
            }
        }
        result.push_str("))");
        result
    }

    /// Generate a VDOM `_createVNode(comp, ...)` call for a component child.
    fn generate_vdom_component(
        &self,
        el: &ElementNode,
        tag_name: &str,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        out.add_vdom_import(VdomHelper::CreateVNode);

        // Dynamic component: <component :is="expr">
        let (component_ref, is_prop_index) = if tag_name == "component" || tag_name == "Component" {
            if let Some((resolved, idx)) = self.resolve_dynamic_is(el, oxc, source, out) {
                out.add_vdom_import(VdomHelper::ResolveDynamicComponent);
                (resolved, Some(idx))
            } else {
                ("_component_component".to_string(), None)
            }
        } else {
            (self.resolve_vdom_component_ref(tag_name), None)
        };

        // Build props (skip the :is prop for dynamic components)
        let props_str = self.generate_vdom_props_skip(el, oxc, source, out, is_prop_index);

        // Build slot children for components with content
        let has_children = self.has_effective_children(el, source);

        let mut result = format!("_createVNode({}", component_ref);
        if has_children {
            out.add_vdom_import(VdomHelper::WithCtx);
            let slot_flag = if self.has_dynamic_slots(el) {
                "_: 2 /* DYNAMIC */"
            } else if self.has_slot_outlet_in_descendants(el, source) {
                "_: 3 /* FORWARDED */"
            } else {
                "_: 1 /* STABLE */"
            };

            // Check if the component has named slots (template v-slot children)
            let slots = self.collect_vdom_named_slots(el, source, out);
            result.push_str(", ");
            result.push_str(&props_str);
            if slots.is_empty() {
                // No named slots — all children go to default slot
                let children_vnodes = self.generate_vdom_fallback(el, source, out);
                let _ = write!(
                    result,
                    ", {{\ndefault: _withCtx(() => {}),\n{}\n}}",
                    children_vnodes, slot_flag
                );
            } else {
                // Named slots — build slot object
                result.push_str(", {\n");
                for (slot_name, slot_params, slot_vnodes) in &slots {
                    let _ = writeln!(
                        result,
                        "{}: _withCtx(({}) => {}),",
                        slot_name, slot_params, slot_vnodes
                    );
                }
                // Add implicit default slot for non-template children
                let default_vnodes = self.generate_vdom_fallback_default(el, source, out);
                if default_vnodes != "[]" {
                    let _ = writeln!(result, "default: _withCtx(() => {}),", default_vnodes);
                }
                result.push_str(slot_flag);
                result.push_str("\n}");
            }
        } else if props_str != "null" {
            result.push_str(", ");
            result.push_str(&props_str);
        }
        // Add PROPS/DYNAMIC_SLOTS/NEED_PATCH patch flags for components
        let dynamic_props = self.collect_dynamic_prop_names(el, source);
        let has_dyn_slots = has_children && self.has_dynamic_slots(el);
        let has_ref = el.v_ref.is_some();
        if !dynamic_props.is_empty() || has_dyn_slots || has_ref {
            let mut flag: u32 = 0;
            if !dynamic_props.is_empty() {
                flag |= 8; // PROPS
            }
            if has_dyn_slots {
                flag |= 1024; // DYNAMIC_SLOTS
            }
            // NEED_PATCH (512): component has ref with no other dynamic flags.
            // Same rule as HTML elements — only when ref is the sole reason for patching.
            if has_ref && flag == 0 {
                flag |= 512; // NEED_PATCH
            }
            if !has_children {
                // Need null placeholder for children argument
                result.push_str(", null");
            }
            result.push_str(", ");
            result.push_str(&Self::format_patch_flag(flag));
            if !dynamic_props.is_empty() {
                result.push_str(", [");
                for (i, name) in dynamic_props.iter().enumerate() {
                    if i > 0 {
                        result.push_str(", ");
                    }
                    result.push('"');
                    result.push_str(name);
                    result.push('"');
                }
                result.push(']');
            }
        }
        result.push(')');
        result
    }

    /// Collect the names of dynamic props on an element (for VDOM PROPS patch flag).
    /// Returns a list of prop names that are dynamically bound (`:prop`, `v-model`, `@event`).
    fn collect_dynamic_prop_names(&self, el: &ElementNode, source: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for prop in &el.props {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive {
                continue; // static props are not dynamic
            }

            // Extract value expression for constness check
            let value_expr = if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                Some(&source[vs as usize..ve as usize])
            } else {
                None
            };
            let is_const = value_expr.is_some_and(|e| self.is_const_simple_expr(e));

            // :attr or v-bind:attr — skip const bindings
            if prop_name.starts_with(':') || prop_name.starts_with("v-bind") {
                if is_const {
                    continue;
                }
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    // Skip key — it's handled by runtime, not a patch prop
                    if attr == "key" {
                        continue;
                    }
                    names.push(attr.to_string());
                }
            }
            // @event → onEvent — skip const handlers
            else if prop_name.starts_with('@') || prop_name == "v-on" {
                if is_const {
                    continue;
                }
                let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                    if after_at.is_empty() {
                        if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                            &source[s as usize..e as usize]
                        } else {
                            continue;
                        }
                    } else {
                        after_at
                    }
                } else if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                    &source[s as usize..e as usize]
                } else {
                    continue;
                };
                let mut js_key = String::with_capacity(event_name.len() + 2);
                format_event_handler_key_into(&mut js_key, event_name);
                names.push(js_key);
            }
            // v-model → modelValue + onUpdate:modelValue (both always dynamic)
            else if prop_name.starts_with("v-model") {
                let model_prop = if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    source[as_ as usize..ae as usize].to_string()
                } else {
                    "modelValue".to_string()
                };
                names.push(model_prop.clone());
                // Vue camelizes the model prop name for the onUpdate handler
                let camelized = camelize(&model_prop);
                names.push(format!("onUpdate:{}", camelized));
            }
        }
        names
    }

    /// Check if a raw expression (from Vue template source) is a constant expression.
    /// Used to skip patch flags — Vue's VDOM fallback omits TEXT/PROPS/CLASS/STYLE
    /// flags when the bound expression is constant.
    ///
    /// Constant expressions include:
    /// - Simple identifiers resolving to `setup-const`, `setup-import`, or `literal-const`
    /// - Numeric literals: `8`, `3.14`
    /// - String literals: `'hello'`, `"world"`
    /// - Boolean literals: `true`, `false`
    /// - `null`, `undefined`
    fn is_const_simple_expr(&self, expr: &str) -> bool {
        let trimmed = expr.trim();
        if trimmed.is_empty() {
            return false;
        }

        // Check for literal constants first (no binding lookup needed)
        // Boolean literals
        if trimmed == "true" || trimmed == "false" {
            return true;
        }
        // null / undefined
        if trimmed == "null" || trimmed == "undefined" {
            return true;
        }
        // Numeric literals: digits, optional dot, optional leading minus
        if is_numeric_literal(trimmed) {
            return true;
        }
        // String literals: 'xxx' or "xxx"
        if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        {
            return true;
        }
        // Array/object literals with only constant values (e.g. [{type: 'email'}])
        // Quick heuristic: if the expression starts with [ or { and contains
        // no identifiers that look like bindings (no $, no letter-starting words
        // that aren't JS keywords), it's a constant literal.
        if (trimmed.starts_with('[') || trimmed.starts_with('{')) && Self::is_literal_only(trimmed)
        {
            return true;
        }

        // Simple identifier: only ascii alphanumeric, underscore, $
        if !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        {
            return false;
        }
        // Check binding type
        if let Some(bt) = self.resolver.get(trimmed) {
            bt.reactivity_level() == ReactivityLevel::Static
        } else {
            false
        }
    }

    /// Quick heuristic: check if a complex expression contains only literal values
    /// (strings, numbers, booleans, null, undefined, and JS keywords used in literals).
    /// Returns true for expressions like `[{type: 'email'}]`, `{a: 1, b: 'x'}`, `[1, 2]`.
    fn is_literal_only(expr: &str) -> bool {
        // Extract all "word" tokens from the expression, distinguishing
        // object property keys (before `:`) from values (after `:`).
        let mut i = 0;
        let bytes = expr.as_bytes();
        while i < bytes.len() {
            let b = bytes[i];
            // Skip delimiters, operators, whitespace
            if matches!(
                b,
                b'[' | b']'
                    | b'{'
                    | b'}'
                    | b'('
                    | b')'
                    | b':'
                    | b','
                    | b' '
                    | b'\n'
                    | b'\r'
                    | b'\t'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'?'
                    | b'!'
                    | b'='
                    | b';'
                    | b'/'
            ) {
                i += 1;
                continue;
            }
            // Skip string literals
            if b == b'\'' || b == b'"' || b == b'`' {
                i += 1;
                while i < bytes.len() && bytes[i] != b {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
                i += 1; // skip closing quote
                continue;
            }
            // Digit: skip number
            if b.is_ascii_digit() {
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                continue;
            }
            // Identifier: collect it
            if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                let word = &expr[start..i];
                // Allow JS keywords used in literals
                if matches!(
                    word,
                    "true" | "false" | "null" | "undefined" | "new" | "typeof"
                ) {
                    continue;
                }
                // Allow identifiers that are object property keys (followed by `:`)
                let mut j = i;
                while j < bytes.len() && bytes[j] == b' ' {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b':' {
                    // This is a property key — always constant
                    continue;
                }
                // This word is a variable reference — not a constant literal
                return false;
            }
            // Unknown character — not a constant
            return false;
        }
        true
    }

    /// Compute VDOM patch flags for an element's _createVNode call.
    ///
    /// Returns (patch_flag_bits, dynamic_prop_names).
    /// The patch flag is a bitmask combining:
    /// - 1 (TEXT): children include interpolation (dynamic text)
    /// - 2 (CLASS): has dynamic :class
    /// - 4 (STYLE): has dynamic :style
    /// - 8 (PROPS): has other dynamic props (returned in dynamic_prop_names)
    /// - 16 (FULL_PROPS): has v-bind spread
    /// - 32 (NEED_HYDRATION): has event handlers on form elements (SSR-specific)
    fn compute_element_patch_flag(
        &self,
        el: &ElementNode,
        _tag_name: &str,
        source: &str,
        children_str: &str,
    ) -> (u32, Vec<String>) {
        let mut flag: u32 = 0;
        let mut dynamic_props: Vec<String> = Vec::new();

        // TEXT (1): children are purely text/interpolation with non-const expressions.
        // Vue only sets TEXT when children are a simple text expression (no element children).
        // When children include both elements and text, Vue uses a VNode array without TEXT.
        if children_str.contains("_toDisplayString(") && self.has_all_text_children(el) {
            let has_dynamic_interp = self.has_dynamic_children_interp(el, source);
            if has_dynamic_interp {
                flag |= 1;
            }
        }

        let mut has_dynamic_class = false;
        let mut has_dynamic_style = false;
        let mut has_v_bind_spread = false;
        let mut has_need_hydration = false;

        for prop in &el.props {
            if !prop.is_directive {
                continue;
            }
            let prop_name = &source[prop.start as usize..prop.name_end as usize];

            // Extract value expression for constness check
            let value_expr = if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                Some(&source[vs as usize..ve as usize])
            } else {
                None
            };
            let is_const = value_expr.is_some_and(|e| self.is_const_simple_expr(e));

            if prop_name.starts_with(':') || prop_name.starts_with("v-bind") {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    // Skip const bindings — Vue omits them from patch flags
                    if is_const {
                        continue;
                    }
                    match attr {
                        "class" => has_dynamic_class = true,
                        "style" => has_dynamic_style = true,
                        "key" => {} // skip
                        _ => dynamic_props.push(attr.to_string()),
                    }
                } else {
                    // v-bind without arg = spread
                    has_v_bind_spread = true;
                }
            } else if prop_name.starts_with('@') || prop_name == "v-on" {
                let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                    if after_at.is_empty() {
                        if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                            &source[s as usize..e as usize]
                        } else {
                            continue;
                        }
                    } else {
                        after_at
                    }
                } else if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                    &source[s as usize..e as usize]
                } else {
                    continue;
                };
                // NEED_HYDRATION (32) for event handlers except:
                // - click (has dedicated fast path)
                // - update:modelValue (not a real event)
                // - lifecycle hooks (vue:mounted etc.)
                if !event_name.eq_ignore_ascii_case("click")
                    && !event_name.starts_with("update:")
                    && !event_name.starts_with("vue:")
                {
                    has_need_hydration = true;
                }
                if !is_const {
                    // Dynamic handler: also add to PROPS flag
                    let mut js_key = String::with_capacity(event_name.len() + 2);
                    format_event_handler_key_into(&mut js_key, event_name);
                    dynamic_props.push(js_key);
                }
            } else if prop_name.starts_with("v-model") {
                let model_prop = if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    source[as_ as usize..ae as usize].to_string()
                } else {
                    "modelValue".to_string()
                };
                dynamic_props.push(model_prop.clone());
                dynamic_props.push(format!("onUpdate:{}", model_prop));
            }
        }

        if has_dynamic_class {
            flag |= 2;
        }
        if has_dynamic_style {
            flag |= 4;
        }
        if has_v_bind_spread {
            flag |= 16;
        }
        if !dynamic_props.is_empty() {
            flag |= 8;
        }
        if has_need_hydration {
            flag |= 32;
        }
        // NEED_PATCH (512): element has ref with no other dynamic flags.
        // Vue's SSR compiler only includes ref in VDOM (and sets NEED_PATCH)
        // when the element has no other dynamic content. When other dynamic
        // flags exist, ref is handled separately and stripped from VDOM.
        if el.v_ref.is_some() && flag == 0 && dynamic_props.is_empty() {
            flag |= 512;
        }

        (flag, dynamic_props)
    }

    /// Check if any interpolation child of the element has a non-const expression.
    /// Check if all children of an element are text or interpolation nodes (no element children).
    /// Vue only applies the TEXT patch flag when children are purely text.
    fn has_all_text_children(&self, el: &ElementNode) -> bool {
        let children = match el.content {
            Some(ref c) => &c.children[..],
            None => return false,
        };
        children.iter().all(|&cid| {
            matches!(
                self.ast.nodes[cid.0].kind,
                AstNodeKind::Text(_) | AstNodeKind::Interpolation(_)
            )
        })
    }

    /// Used to decide whether TEXT patchflag should be set.
    fn has_dynamic_children_interp(&self, el: &ElementNode, source: &str) -> bool {
        let children = match el.content {
            Some(ref c) => &c.children[..],
            None => return false,
        };
        for &child_id in children {
            let child = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Interpolation(interp) = &child.kind {
                let expr = &source[interp.inner_start as usize..interp.inner_end as usize];
                if !self.is_const_simple_expr(expr) {
                    return true;
                }
            }
        }
        false
    }

    /// Format a patch flag bitmask as "N /* NAME */" string.
    fn format_patch_flag(flag: u32) -> String {
        let mut names = Vec::new();
        if flag & 1 != 0 {
            names.push("TEXT");
        }
        if flag & 2 != 0 {
            names.push("CLASS");
        }
        if flag & 4 != 0 {
            names.push("STYLE");
        }
        if flag & 8 != 0 {
            names.push("PROPS");
        }
        if flag & 16 != 0 {
            names.push("FULL_PROPS");
        }
        if flag & 32 != 0 {
            names.push("NEED_HYDRATION");
        }
        if flag & 64 != 0 {
            names.push("STABLE_FRAGMENT");
        }
        if flag & 128 != 0 {
            names.push("KEYED_FRAGMENT");
        }
        if flag & 256 != 0 {
            names.push("UNKEYED_FRAGMENT");
        }
        if flag & 512 != 0 {
            names.push("NEED_PATCH");
        }
        if flag & 1024 != 0 {
            names.push("DYNAMIC_SLOTS");
        }
        if names.is_empty() {
            flag.to_string()
        } else {
            format!("{} /* {} */", flag, names.join(", "))
        }
    }

    /// Collect named slots from a component's children.
    /// Returns Vec of (slot_name, params_string, vnodes_string).
    fn collect_vdom_named_slots(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> Vec<(String, String, String)> {
        let children = match el.content {
            Some(ref content) => &content.children[..],
            None => return Vec::new(),
        };

        let mut slots: Vec<(String, String, String)> = Vec::new();
        for &child_id in children {
            let child = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = child.kind {
                if child_el.tag_type == TagType::Template {
                    if let Some(ref v_slot) = child_el.v_slot {
                        // Extract slot name from arg (including modifiers for dot-notation names)
                        let slot_name = Self::build_slot_name(v_slot, source);

                        // Extract slot params from value
                        let params =
                            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                                source[vs as usize..ve as usize].to_string()
                            } else {
                                String::new()
                            };

                        // Generate children VNodes
                        let children = match child_el.content {
                            Some(ref content) => &content.children[..],
                            None => &[],
                        };
                        let vnodes = self.generate_vdom_children(children, source, out);

                        slots.push((slot_name, params, vnodes));
                    }
                }
            }
        }
        slots
    }

    /// Generate VDOM props object string for an element.
    /// Returns `"null"` for no props, or `"{ key: value, ... }"`.
    fn generate_vdom_props(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        self.generate_vdom_props_skip(el, oxc, source, out, None)
    }

    /// Generate VDOM props, optionally skipping a prop at `skip_index` (used for `:is`
    /// on dynamic components where the prop becomes the `_resolveDynamicComponent` arg).
    fn generate_vdom_props_skip(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
        skip_index: Option<usize>,
    ) -> String {
        if el.props.is_empty() && el.v_ref.is_none() {
            return "null".to_string();
        }

        // Pre-scan for class/style merge: static `class` + `:class` → merged `class: [static, dynamic]`
        let mut static_class: Option<String> = None;
        let mut dynamic_class: Option<String> = None;
        let mut static_class_idx: Option<usize> = None;
        let mut dynamic_class_idx: Option<usize> = None;

        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive && prop_name == "class" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    static_class = Some(source[vs as usize..ve as usize].to_string());
                    static_class_idx = Some(i);
                }
            } else if prop.is_directive
                && (prop_name.starts_with(':') || prop_name.starts_with("v-bind"))
            {
                // Check if arg is "class" via arg_start..arg_end
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let arg_name = &source[as_ as usize..ae as usize];
                    if arg_name == "class" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_expr = oxc
                                .and_then(|o| find_oxc_prop(o, i))
                                .and_then(|p| p.exp.as_ref());
                            dynamic_class = Some(self.resolve_expr(expr, vs, oxc_expr));
                            dynamic_class_idx = Some(i);
                        }
                    }
                }
            }
        }

        let merge_class = static_class.is_some() && dynamic_class.is_some();

        let mut parts: Vec<String> = Vec::new();
        let mut part_positions: Vec<u32> = Vec::new();
        for (i, prop) in el.props.iter().enumerate() {
            // Skip the :is prop on dynamic <component> (consumed by _resolveDynamicComponent)
            if skip_index == Some(i) {
                continue;
            }
            // Skip props that were merged into the combined class
            if merge_class && (Some(i) == static_class_idx || Some(i) == dynamic_class_idx) {
                // Emit the merged class on the static class index
                if Some(i) == static_class_idx {
                    parts.push(format!(
                        "class: [\"{}\", {}]",
                        static_class.as_ref().unwrap(),
                        dynamic_class.as_ref().unwrap()
                    ));
                    part_positions.push(prop.start);
                }
                continue;
            }

            let prop_name = &source[prop.start as usize..prop.name_end as usize];

            if prop.is_directive {
                // Handle v-bind (:attr or v-bind:attr) — use arg_start..arg_end for attr name
                if prop_name.starts_with(':') || prop_name.starts_with("v-bind") {
                    if let (Some(as_), Some(ae), Some(vs), Some(ve)) = (
                        prop.arg_start,
                        prop.arg_end,
                        prop.value_start,
                        prop.value_end,
                    ) {
                        let attr = &source[as_ as usize..ae as usize];
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_expr = oxc
                            .and_then(|o| find_oxc_prop(o, i))
                            .and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        let key = if needs_quoted_key(attr) {
                            format!("\"{}\"", attr)
                        } else {
                            attr.to_string()
                        };
                        parts.push(format!("{}: {}", key, resolved));
                        part_positions.push(prop.start);
                    }
                }
                // Event handlers: @click → onClick, v-on:input → onInput
                // Modifiers: .capture/.once/.passive append to key; .enter etc. use _withKeys;
                // .stop/.prevent/etc. use _withModifiers.
                else if prop_name.starts_with('@') || prop_name == "v-on" {
                    let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                        if after_at.is_empty() {
                            match (prop.arg_start, prop.arg_end) {
                                (Some(s), Some(e)) => &source[s as usize..e as usize],
                                _ => {
                                    continue;
                                }
                            }
                        } else {
                            after_at
                        }
                    } else {
                        match (prop.arg_start, prop.arg_end) {
                            (Some(s), Some(e)) => &source[s as usize..e as usize],
                            _ => {
                                continue;
                            }
                        }
                    };

                    // Build key name
                    let mut js_key = String::with_capacity(event_name.len() + 2);
                    format_event_handler_key_into(&mut js_key, event_name);

                    // Classify modifiers
                    let mut key_modifiers: Vec<&str> = Vec::new();
                    let mut runtime_modifiers: Vec<&str> = Vec::new();
                    for modifier in &prop.modifiers {
                        let mod_name = &source[modifier.start as usize..modifier.end as usize];
                        match mod_name {
                            // Option modifiers: append to key name
                            "capture" | "once" | "passive" => {
                                let first = mod_name.as_bytes()[0].to_ascii_uppercase() as char;
                                js_key.push(first);
                                js_key.push_str(&mod_name[1..]);
                            }
                            // Key modifiers: wrapped with _withKeys
                            "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down"
                            | "left" | "right" => {
                                // "left"/"right" are key modifiers on key events,
                                // runtime modifiers on mouse events
                                if (mod_name == "left" || mod_name == "right")
                                    && !event_name.starts_with("key")
                                {
                                    runtime_modifiers.push(mod_name);
                                } else {
                                    key_modifiers.push(mod_name);
                                }
                            }
                            // Runtime modifiers: wrapped with _withModifiers
                            _ => {
                                runtime_modifiers.push(mod_name);
                            }
                        }
                    }

                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_expr = oxc
                            .and_then(|o| find_oxc_prop(o, i))
                            .and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        let handler = if is_inline_handler(expr) {
                            format!("$event => ({})", resolved)
                        } else {
                            resolved
                        };

                        // Wrap with _withKeys (innermost) then _withModifiers (outermost)
                        let mut value = handler;
                        if !key_modifiers.is_empty() {
                            let mods: Vec<String> =
                                key_modifiers.iter().map(|m| format!("\"{}\"", m)).collect();
                            value = format!("_withKeys({}, [{}])", value, mods.join(", "));
                            out.add_vdom_import(VdomHelper::WithKeys);
                        }
                        if !runtime_modifiers.is_empty() {
                            let mods: Vec<String> = runtime_modifiers
                                .iter()
                                .map(|m| format!("\"{}\"", m))
                                .collect();
                            value = format!("_withModifiers({}, [{}])", value, mods.join(", "));
                            out.add_vdom_import(VdomHelper::WithModifiers);
                        }

                        if needs_quoted_key(&js_key) {
                            parts.push(format!("\"{}\": {}", js_key, value));
                        } else {
                            parts.push(format!("{}: {}", js_key, value));
                        }
                        part_positions.push(prop.start);
                    }
                }
                // v-model: decompose into value prop + onUpdate handler
                else if prop_name.starts_with("v-model") {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_expr = oxc
                            .and_then(|o| find_oxc_prop(o, i))
                            .and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        let model_prop =
                            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                source[as_ as usize..ae as usize].to_string()
                            } else {
                                "modelValue".to_string()
                            };
                        parts.push(format!("{}: {}", model_prop, resolved));
                        part_positions.push(prop.start);
                        let camelized_prop = camelize(&model_prop);
                        parts.push(format!(
                            "\"onUpdate:{}\": $event => (({}) = $event)",
                            camelized_prop, resolved
                        ));
                        part_positions.push(prop.start);
                        if !prop.modifiers.is_empty() {
                            let mod_key = if model_prop == "modelValue" {
                                "modelModifiers".to_string()
                            } else {
                                format!("{}Modifiers", model_prop)
                            };
                            let mods: Vec<String> = prop
                                .modifiers
                                .iter()
                                .map(|m| {
                                    let name = &source[m.start as usize..m.end as usize];
                                    format!("{}: true", name)
                                })
                                .collect();
                            parts.push(format!("{}: {{ {} }}", mod_key, mods.join(", ")));
                            part_positions.push(prop.start);
                        }
                    }
                }
                continue;
            }

            // Static attribute
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                let key = if needs_quoted_key(prop_name) {
                    format!("\"{}\"", prop_name)
                } else {
                    prop_name.to_string()
                };
                if prop_name == "style" {
                    // Vue SSR keeps kebab-case for style property names in VDOM too
                    parts.push(format!("{}: {}", key, css_to_js_object(value)));
                } else {
                    parts.push(format!("{}: \"{}\"", key, value));
                }
                part_positions.push(prop.start);
            } else {
                // Boolean attribute (no value)
                let key = if needs_quoted_key(prop_name) {
                    format!("\"{}\"", prop_name)
                } else {
                    prop_name.to_string()
                };
                parts.push(format!("{}: \"\"", key));
                part_positions.push(prop.start);
            }
        }

        // Add ref prop if present (cached on el.v_ref, not in el.props)
        // Insert at correct source order position (not always first)
        if let Some(ref v_ref) = el.v_ref {
            if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
                let ref_val = &source[vs as usize..ve as usize];
                let ref_str = if v_ref.is_directive {
                    let resolved = self.resolve_expr(ref_val, vs, None);
                    format!("ref: {}", resolved)
                } else {
                    format!("ref: \"{}\"", escape_js_string(ref_val))
                };
                let insert_idx = part_positions.partition_point(|&pos| pos < v_ref.start);
                parts.insert(insert_idx, ref_str);
            }
        }

        if parts.is_empty() {
            "null".to_string()
        } else {
            format!("{{ {} }}", parts.join(", "))
        }
    }

    /// Generate VDOM children for an element (string text, or recursive VNode array).
    /// Returns `"null"` for no children, `"text"` for text-only, or `[...]` array.
    fn generate_vdom_element_children(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let children = match el.content {
            Some(ref content) => &content.children[..],
            None => return "null".to_string(),
        };

        if children.is_empty() {
            return "null".to_string();
        }

        // Check if all children are text/interpolation (can be a simple string)
        let all_text = children.iter().all(|&cid| {
            matches!(
                self.ast.nodes[cid.0].kind,
                AstNodeKind::Text(_) | AstNodeKind::Interpolation(_)
            )
        });

        if all_text {
            // Check if it's purely static text
            let all_static = children
                .iter()
                .all(|&cid| matches!(self.ast.nodes[cid.0].kind, AstNodeKind::Text(_)));

            if all_static {
                // Concatenate all text into a single string
                let mut text = String::new();
                for &cid in children {
                    if let AstNodeKind::Text(ref t) = self.ast.nodes[cid.0].kind {
                        text.push_str(&source[t.start as usize..t.end as usize]);
                    }
                }
                let condensed = condense_whitespace(&text);
                if condensed.trim().is_empty() {
                    "null".to_string()
                } else {
                    format!("\"{}\"", Self::escape_vdom_text(&condensed))
                }
            } else {
                // Mixed text + interpolation: use _toDisplayString
                // Vue drops whitespace-only text nodes at start/end of children.
                // Find the effective range: skip leading/trailing ws-only text.
                let mut start = 0;
                let mut end = children.len();
                while start < end {
                    if let AstNodeKind::Text(t) = &self.ast.nodes[children[start].0].kind {
                        let text = &source[t.start as usize..t.end as usize];
                        if condense_whitespace(text).trim().is_empty() {
                            start += 1;
                            continue;
                        }
                    }
                    break;
                }
                while end > start {
                    if let AstNodeKind::Text(t) = &self.ast.nodes[children[end - 1].0].kind {
                        let text = &source[t.start as usize..t.end as usize];
                        if condense_whitespace(text).trim().is_empty() {
                            end -= 1;
                            continue;
                        }
                    }
                    break;
                }

                let mut parts: Vec<String> = Vec::new();
                for &cid in &children[start..end] {
                    match &self.ast.nodes[cid.0].kind {
                        AstNodeKind::Text(t) => {
                            let text = &source[t.start as usize..t.end as usize];
                            let condensed = condense_whitespace(text);
                            if !condensed.is_empty() {
                                parts.push(format!("\"{}\"", Self::escape_vdom_text(&condensed)));
                            }
                        }
                        AstNodeKind::Interpolation(interp) => {
                            let expr =
                                &source[interp.inner_start as usize..interp.inner_end as usize];
                            let oxc_interp = self.oxc_interpolation(cid);
                            let resolved = self.resolve_expr(expr, interp.inner_start, oxc_interp);
                            out.add_vdom_import(VdomHelper::ToDisplayString);
                            parts.push(format!("_toDisplayString({})", resolved));
                        }
                        _ => {}
                    }
                }
                parts.join(" + ")
            }
        } else {
            // Mixed children: generate array of VNodes
            self.generate_vdom_children(children, source, out)
        }
    }

    /// Escape text for VDOM string literals.
    fn escape_vdom_text(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }

    // ── Attribute rendering ─────────────────────────────────────

    /// Build SSR attributes string for an element.
    /// For root elements, wraps attrs in `_ssrRenderAttrs()` with `_attrs` merging.
    /// For nested elements without v-bind spread, emits inline per-attribute helpers.
    fn build_attrs_string(
        &mut self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
        is_root: bool,
    ) -> String {
        let tag_name_str = self.tag_name(el, source);
        // When custom directives are present, force the attrs_obj/_mergeProps path
        // even for nested elements (don't use inline per-attr helpers).
        let has_custom_directives = el.prop_flag.has(PropFlags::HasCustomDirective);

        let mut attrs_obj = String::new();
        let mut has_dynamic_attrs = false;
        let mut has_v_show = false;
        let mut v_show_expr = String::new();
        let mut v_bind_spread_expr = String::new();
        let mut directive_calls: Vec<String> = Vec::new();

        // Pre-scan for v-bind spread and v-show so that static class/style processing
        // knows upfront whether to use the mergeProps path (source order matters).
        let mut has_v_bind_spread = false;
        let mut has_v_show_prescan = false;
        for p in el.props.iter() {
            if p.is_directive {
                let pn = &source[p.start as usize..p.name_end as usize];
                if pn == "v-bind" && p.arg_start.is_none() {
                    has_v_bind_spread = true;
                } else if pn == "v-show" {
                    has_v_show_prescan = true;
                }
            }
        }

        // Collect v-model info for this element
        let mut v_model_expr: Option<String> = None;
        let mut v_model_arg: Option<String> = None;
        let mut v_model_prop_idx: usize = 0;
        let mut v_model_is_real = false; // true if from v-model, false if from :value on textarea

        // Per-attribute inline parts (for nested elements without v-bind spread).
        // Stores (prop_index, rendered_attr_string) for source-order interleaving.
        let mut inline_parts: Vec<(usize, String)> = Vec::new();

        // Class merge tracking: when both static `class` and `:class` exist,
        // they are merged into a single `_ssrRenderClass([dynamic, "static"])`.
        let mut static_class_value: Option<String> = None;
        let mut static_class_prop_idx: Option<usize> = None;
        let mut dynamic_class_resolved: Option<String> = None;
        let mut dynamic_class_prop_idx: Option<usize> = None;

        // Style merge tracking: when v-show coexists with :style or static style,
        // they are merged into a single `_ssrRenderStyle([...])` array call.
        let mut dynamic_style_resolved: Option<String> = None;
        let mut static_style_value: Option<String> = None;
        let mut static_style_prop_idx: Option<usize> = None;

        // Prepare ref entry for root elements (ref needs source-order insertion)
        let ref_entry: Option<(u32, String)> = if is_root {
            el.v_ref.as_ref().and_then(|v_ref| {
                let (vs, ve) = (v_ref.value_start?, v_ref.value_end?);
                let ref_val = &source[vs as usize..ve as usize];
                let entry = if v_ref.is_directive {
                    let resolved = self.resolve_expr(ref_val, vs, None);
                    format!("ref: {}", resolved)
                } else {
                    format!("ref: \"{}\"", escape_js_string(ref_val))
                };
                Some((v_ref.start, entry))
            })
        } else {
            None
        };
        let mut ref_emitted = false;

        for (i, prop) in el.props.iter().enumerate() {
            // Emit ref at the correct source position among other props
            if let Some((ref_pos, ref entry_str)) = &ref_entry {
                if !ref_emitted && *ref_pos < prop.start {
                    if !attrs_obj.is_empty() {
                        attrs_obj.push_str(", ");
                    }
                    attrs_obj.push_str(entry_str);
                    has_dynamic_attrs = true;
                    ref_emitted = true;
                }
            }

            let prop_name = &source[prop.start as usize..prop.name_end as usize];

            if prop.is_directive {
                // Skip events entirely in SSR
                if prop_name.starts_with('@') || prop_name.starts_with("v-on") {
                    continue;
                }

                // v-show
                if prop_name == "v-show" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        v_show_expr = self.resolve_expr(expr, vs, oxc_expr);
                        has_v_show = true;
                    }
                    continue;
                }

                // v-bind spread (no argument) — already detected in pre-scan
                if prop_name == "v-bind" && prop.arg_start.is_none() {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        v_bind_spread_expr = self.resolve_expr(expr, vs, oxc_expr);
                    }
                    continue;
                }

                // v-html and v-text are handled at the content level
                if prop_name == "v-html" || prop_name == "v-text" {
                    continue;
                }

                // v-model: handle based on element type
                if prop_name.starts_with("v-model") {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        v_model_expr = Some(resolved);
                        v_model_prop_idx = i;
                        v_model_is_real = true;

                        // Check for v-model argument (v-model:title)
                        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                            v_model_arg = Some(source[as_ as usize..ae as usize].to_string());
                        }
                    }
                    continue;
                }

                // v-memo, v-cloak — skip in SSR
                if prop_name == "v-memo" || prop_name == "v-cloak" {
                    continue;
                }

                // Dynamic bindings (:attr or v-bind:attr)
                if prop_name.starts_with(':') || (prop_name == "v-bind" && prop.arg_start.is_some())
                {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let attr_name = &source[as_ as usize..ae as usize];
                        let is_dynamic_name = prop.is_dynamic == Some(true);
                        // :key and :ref are client-only, skip in SSR
                        if attr_name == "key" || attr_name == "ref" {
                            continue;
                        }
                        // :value on <textarea>: skip from attrs, rendered as content
                        if attr_name == "value" && tag_name_str == "textarea" {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                v_model_expr = Some(self.resolve_expr(expr, vs, oxc_expr));
                                v_model_prop_idx = i;
                            }
                            continue;
                        }
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);

                            // Dynamic attribute names (:[expr]) must go through _ssrRenderAttrs
                            // with a computed property key: { [resolvedExpr || ""]: value }
                            if is_dynamic_name {
                                // Strip brackets from arg span to get raw expression
                                let raw_arg = attr_name
                                    .strip_prefix('[')
                                    .unwrap_or(attr_name)
                                    .strip_suffix(']')
                                    .unwrap_or(attr_name);
                                // Resolve the dynamic name expression
                                // as_ points to '[', but raw_arg starts after '[', so offset + 1
                                let oxc_arg = oxc_prop.and_then(|p| p.arg.as_ref());
                                let resolved_name = self.resolve_expr(raw_arg, as_ + 1, oxc_arg);
                                // Build computed key: [resolvedExpr || ""]
                                let computed_key = format!("[{} || \"\"]", resolved_name);
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                attrs_obj.push_str(&computed_key);
                                attrs_obj.push_str(": ");
                                attrs_obj.push_str(&resolved);
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // For nested elements without v-bind spread or custom directives,
                            // use inline helpers. Custom directives force the _mergeProps path.
                            if !is_root && !has_v_bind_spread && !has_custom_directives {
                                if attr_name == "class" {
                                    // Save for potential merge with static class
                                    dynamic_class_resolved = Some(resolved);
                                    dynamic_class_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if attr_name == "style" {
                                    if has_v_show_prescan {
                                        // Save for merge with v-show
                                        dynamic_style_resolved = Some(resolved);
                                        has_dynamic_attrs = true;
                                        continue;
                                    }
                                    out.add_ssr_import(SsrHelper::RenderStyle);
                                    inline_parts.push((
                                        i,
                                        format!(" style=\"${{_ssrRenderStyle({})}}\"", resolved),
                                    ));
                                } else if is_boolean_html_attr(attr_name) {
                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{(_ssrIncludeBooleanAttr({})) ? \" {}\" : \"\"}}",
                                            resolved, attr_name
                                        ),
                                    ));
                                } else {
                                    out.add_ssr_import(SsrHelper::RenderAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{_ssrRenderAttr(\"{}\", {})}}",
                                            attr_name, resolved
                                        ),
                                    ));
                                }
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // Track dynamic class for root-path merge
                            if attr_name == "class" {
                                dynamic_class_resolved = Some(resolved);
                                dynamic_class_prop_idx = Some(i);
                                has_dynamic_attrs = true;
                                // Insert placeholder for source-order preservation
                                if !attrs_obj.contains("__CLASS_PLACEHOLDER__") {
                                    if !attrs_obj.is_empty() {
                                        attrs_obj.push_str(", ");
                                    }
                                    attrs_obj.push_str("__CLASS_PLACEHOLDER__");
                                }
                            } else {
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                let js_attr = html_attr_to_js_key(attr_name);
                                attrs_obj.push_str(&js_attr);
                                attrs_obj.push_str(": ");
                                attrs_obj.push_str(&resolved);
                                has_dynamic_attrs = true;
                            }
                        } else {
                            // Same-name shorthand (Vue 3.4+): `:class` ≡ `:class="class"`
                            // The value expression is the arg name itself.
                            // Vue always uses dot notation (_ctx.class) even for keywords.
                            // For hyphenated names (:heading-value), camelize to headingValue
                            // before binding lookup — bindings use camelCase keys and the
                            // raw hyphenated form would produce subtraction in JS output.
                            let camelized = camelize(attr_name);
                            let prefix = self.resolver.resolve_prefix(&camelized);
                            let resolved = if prefix.is_empty() {
                                format!("_ctx.{}", camelized)
                            } else {
                                format!("{}{}", prefix, camelized)
                            };

                            if !is_root && !has_v_bind_spread && !has_custom_directives {
                                if attr_name == "class" {
                                    dynamic_class_resolved = Some(resolved);
                                    dynamic_class_prop_idx = Some(i);
                                    has_dynamic_attrs = true;
                                    continue;
                                } else if attr_name == "style" {
                                    out.add_ssr_import(SsrHelper::RenderStyle);
                                    inline_parts.push((
                                        i,
                                        format!(" style=\"${{_ssrRenderStyle({})}}\"", resolved),
                                    ));
                                } else if is_boolean_html_attr(attr_name) {
                                    out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{(_ssrIncludeBooleanAttr({})) ? \" {}\" : \"\"}}",
                                            resolved, attr_name
                                        ),
                                    ));
                                } else {
                                    out.add_ssr_import(SsrHelper::RenderAttr);
                                    inline_parts.push((
                                        i,
                                        format!(
                                            "${{_ssrRenderAttr(\"{}\", {})}}",
                                            attr_name, resolved
                                        ),
                                    ));
                                }
                                has_dynamic_attrs = true;
                                continue;
                            }

                            // Root/spread path
                            if attr_name == "class" {
                                dynamic_class_resolved = Some(resolved);
                                dynamic_class_prop_idx = Some(i);
                                has_dynamic_attrs = true;
                                if !attrs_obj.contains("__CLASS_PLACEHOLDER__") {
                                    if !attrs_obj.is_empty() {
                                        attrs_obj.push_str(", ");
                                    }
                                    attrs_obj.push_str("__CLASS_PLACEHOLDER__");
                                }
                            } else {
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                let js_attr = html_attr_to_js_key(attr_name);
                                attrs_obj.push_str(&js_attr);
                                attrs_obj.push_str(": ");
                                attrs_obj.push_str(&resolved);
                                has_dynamic_attrs = true;
                            }
                        }
                    }
                    continue;
                }

                // Other custom directives — resolve and merge via _ssrGetDirectiveProps
                if prop_name.starts_with("v-") {
                    if let Some(call) =
                        self.build_directive_props_call(prop, prop_name, source, oxc, i, out)
                    {
                        directive_calls.push(call);
                        has_dynamic_attrs = true;
                    }
                }
                continue;
            }

            // Static attributes
            let attr_name = prop_name;
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];

                // Track static class for potential merge with :class
                // Trim trailing whitespace (Vue does this)
                if attr_name == "class" {
                    static_class_value = Some(value.trim_end().to_string());
                    static_class_prop_idx = Some(i);
                    if is_root || has_v_bind_spread || has_custom_directives {
                        // Insert a placeholder in attrs_obj at source order position.
                        // The class merge section below will replace it with the
                        // final merged value, preserving source order.
                        if !attrs_obj.is_empty() {
                            attrs_obj.push_str(", ");
                        }
                        attrs_obj.push_str("__CLASS_PLACEHOLDER__");
                        continue;
                    }
                    // For inline path: let it continue to attrs_obj (harmless, attrs_obj
                    // isn't used when inline path returns early).
                }

                if !attrs_obj.is_empty() {
                    attrs_obj.push_str(", ");
                }
                let js_attr = html_attr_to_js_key(attr_name);
                attrs_obj.push_str(&js_attr);
                if attr_name == "style" {
                    // Vue SSR converts static style strings to JS objects in mergeProps
                    attrs_obj.push_str(": ");
                    attrs_obj.push_str(&css_to_js_object(value));
                } else {
                    attrs_obj.push_str(": \"");
                    attrs_obj.push_str(&escape_js_string(value));
                    attrs_obj.push('"');
                }
            } else {
                // Boolean attribute (e.g., `disabled`)
                if !attrs_obj.is_empty() {
                    attrs_obj.push_str(", ");
                }
                let js_attr = html_attr_to_js_key(attr_name);
                attrs_obj.push_str(&js_attr);
                attrs_obj.push_str(": \"\"");
            }
        }

        // Merge static + dynamic class into a single expression.
        // For the root path, a __CLASS_PLACEHOLDER__ was inserted in source-order
        // position. Replace it with the final class value to preserve ordering.
        match (&dynamic_class_resolved, &static_class_value) {
            (Some(dyn_expr), Some(static_cls)) => {
                if !is_root && !has_v_bind_spread && !has_custom_directives {
                    // Inline path: merged _ssrRenderClass call (Vue order: dynamic first)
                    out.add_ssr_import(SsrHelper::RenderClass);
                    let prop_idx =
                        dynamic_class_prop_idx.unwrap_or(static_class_prop_idx.unwrap_or(0));
                    inline_parts.push((
                        prop_idx,
                        format!(
                            " class=\"${{_ssrRenderClass([{}, \"{}\"])}}\"",
                            dyn_expr,
                            escape_js_string(static_cls),
                        ),
                    ));
                    has_dynamic_attrs = true;
                } else {
                    // Root/attrs_obj path: class: ["static", dynamic] (Vue order: static first)
                    let class_entry = format!(
                        "class: [\"{}\", {}]",
                        escape_js_string(static_cls),
                        dyn_expr,
                    );
                    if attrs_obj.contains("__CLASS_PLACEHOLDER__") {
                        attrs_obj = attrs_obj.replace("__CLASS_PLACEHOLDER__", &class_entry);
                    } else {
                        if !attrs_obj.is_empty() {
                            attrs_obj.push_str(", ");
                        }
                        attrs_obj.push_str(&class_entry);
                    }
                    has_dynamic_attrs = true;
                }
            }
            (Some(dyn_expr), None) => {
                if !is_root && !has_v_bind_spread && !has_custom_directives {
                    // Inline path: dynamic only
                    out.add_ssr_import(SsrHelper::RenderClass);
                    let prop_idx = dynamic_class_prop_idx.unwrap_or(0);
                    inline_parts.push((
                        prop_idx,
                        format!(" class=\"${{_ssrRenderClass({})}}\"", dyn_expr),
                    ));
                    has_dynamic_attrs = true;
                } else {
                    // Root/attrs_obj path: dynamic class only
                    let class_entry = format!("class: {}", dyn_expr);
                    if attrs_obj.contains("__CLASS_PLACEHOLDER__") {
                        attrs_obj = attrs_obj.replace("__CLASS_PLACEHOLDER__", &class_entry);
                    } else {
                        if !attrs_obj.is_empty() {
                            attrs_obj.push_str(", ");
                        }
                        attrs_obj.push_str(&class_entry);
                    }
                    has_dynamic_attrs = true;
                }
            }
            (None, Some(static_cls)) => {
                if is_root || has_v_bind_spread || has_custom_directives {
                    // Root/mergeProps path: put static class into attrs_obj
                    let class_entry = format!("class: \"{}\"", escape_js_string(static_cls));
                    if attrs_obj.contains("__CLASS_PLACEHOLDER__") {
                        attrs_obj = attrs_obj.replace("__CLASS_PLACEHOLDER__", &class_entry);
                    } else {
                        if !attrs_obj.is_empty() {
                            attrs_obj.push_str(", ");
                        }
                        attrs_obj.push_str(&class_entry);
                    }
                }
                // Inline path: static class will be rendered as literal HTML
                // in the source-order loop (it wasn't skipped from props).
            }
            (None, None) => {}
        }

        // Handle v-model attrs injection
        if let Some(ref model_expr) = v_model_expr {
            let prop_name = v_model_arg.as_deref().unwrap_or("value");
            let input_type = self.get_input_type(el, source);

            match tag_name_str.as_str() {
                "textarea" => {
                    if v_model_is_real {
                        // Real v-model on textarea: content is always interpolated
                        // (both root and non-root). Do NOT add value to attrs.
                    } else {
                        // :value on textarea: for root path, add value to attrs_obj.
                        // For non-root, content interpolation handles it.
                        if is_root || has_v_bind_spread || has_custom_directives {
                            if !attrs_obj.is_empty() {
                                attrs_obj.push_str(", ");
                            }
                            attrs_obj.push_str(&format!("{}: {}", prop_name, model_expr));
                            has_dynamic_attrs = true;
                        }
                    }
                }
                "select" => {
                    // select: no attr changes needed on <select> itself;
                    // store v-model expr so child <option> elements can inject `selected`
                    self.select_v_model_expr = Some(model_expr.clone());
                }
                "input" => {
                    // For root input elements, skip adding explicit value/checked to
                    // attrs_obj — _ssrGetDynamicModelProps determines the correct prop
                    // at runtime based on the merged type (which may come from _attrs).
                    if is_root {
                        has_dynamic_attrs = true;
                    } else if !has_v_bind_spread && !has_custom_directives {
                        // Non-root inline path
                        match input_type {
                            Some("checkbox") => {
                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                out.add_ssr_import(SsrHelper::LooseContain);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{(_ssrIncludeBooleanAttr(\
                                         (Array.isArray({expr})) \
                                         ? _ssrLooseContain({expr}, null) \
                                         : {expr})) \
                                         ? \" checked\" : \"\"}}",
                                        expr = model_expr
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                            Some("radio") => {
                                let radio_value = self.get_option_value(el, oxc, source);
                                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                                out.add_ssr_import(SsrHelper::LooseEqual);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{(_ssrIncludeBooleanAttr(\
                                         _ssrLooseEqual({}, {}))) \
                                         ? \" checked\" : \"\"}}",
                                        model_expr, radio_value
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                            _ => {
                                out.add_ssr_import(SsrHelper::RenderAttr);
                                inline_parts.push((
                                    v_model_prop_idx,
                                    format!(
                                        "${{_ssrRenderAttr(\"{}\", {})}}",
                                        prop_name, model_expr
                                    ),
                                ));
                                has_dynamic_attrs = true;
                            }
                        }
                    } else {
                        // Non-root with v-bind spread or custom directives:
                        // add to attrs_obj (mergeProps path)
                        match input_type {
                            Some("checkbox") => {
                                out.add_ssr_import(SsrHelper::LooseContain);
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                attrs_obj.push_str(&format!(
                                    "checked: (Array.isArray({expr}) \
                                     ? _ssrLooseContain({expr}, null) \
                                     : {expr})",
                                    expr = model_expr
                                ));
                                has_dynamic_attrs = true;
                            }
                            Some("radio") => {
                                let radio_value = self.get_option_value(el, oxc, source);
                                out.add_ssr_import(SsrHelper::LooseEqual);
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                attrs_obj.push_str(&format!(
                                    "checked: _ssrLooseEqual({}, {})",
                                    model_expr, radio_value
                                ));
                                has_dynamic_attrs = true;
                            }
                            _ => {
                                if !attrs_obj.is_empty() {
                                    attrs_obj.push_str(", ");
                                }
                                attrs_obj.push_str(&format!("{}: {}", prop_name, model_expr));
                                has_dynamic_attrs = true;
                            }
                        }
                    }
                }
                _ => {
                    // Component v-model or other elements
                    if !attrs_obj.is_empty() {
                        attrs_obj.push_str(", ");
                    }
                    attrs_obj.push_str(&format!("{}: {}", prop_name, model_expr));
                    has_dynamic_attrs = true;
                }
            }
        }

        // <option> inside <select v-model>: inject `selected` attribute check
        let option_selected_suffix = if tag_name_str == "option" {
            if let Some(ref model_expr) = self.select_v_model_expr {
                // Determine the option's value expression
                let option_value = self.get_option_value(el, oxc, source);
                out.add_ssr_import(SsrHelper::IncludeBooleanAttr);
                out.add_ssr_import(SsrHelper::LooseContain);
                out.add_ssr_import(SsrHelper::LooseEqual);
                format!(
                    "${{(_ssrIncludeBooleanAttr((Array.isArray({model})) \
                     ? _ssrLooseContain({model}, {val}) \
                     : _ssrLooseEqual({model}, {val}))) ? \" selected\" : \"\"}}",
                    model = model_expr,
                    val = option_value
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // For nested elements using inline mode (has inline_parts or v-show, and no v-bind spread).
        // When directive_calls are present, skip inline mode and use the _mergeProps path instead.
        if !is_root
            && !has_v_bind_spread
            && directive_calls.is_empty()
            && (!inline_parts.is_empty() || has_v_show)
        {
            // Build attrs in source order: interleave static and dynamic parts
            let mut dynamic_map: std::collections::HashMap<usize, &str> =
                inline_parts.iter().map(|(i, s)| (*i, s.as_str())).collect();
            // Track which prop indices were consumed by the class merge
            let class_merged_static_idx = if dynamic_class_resolved.is_some() {
                static_class_prop_idx
            } else {
                None
            };
            let mut result = String::new();
            for (i, prop) in el.props.iter().enumerate() {
                if let Some(dynamic_part) = dynamic_map.remove(&i) {
                    result.push_str(dynamic_part);
                } else if Some(i) == class_merged_static_idx {
                    // Static class was merged into the _ssrRenderClass call — skip
                    continue;
                } else if Some(i) == static_style_prop_idx {
                    // Static style was merged into the _ssrRenderStyle call — skip
                    continue;
                } else if !prop.is_directive {
                    // Static attr: render as literal HTML (same as build_literal_attrs)
                    let name = &source[prop.start as usize..prop.name_end as usize];
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let value = &source[vs as usize..ve as usize];
                        if name == "style" {
                            if has_v_show_prescan {
                                // Will be merged into a combined _ssrRenderStyle call
                                static_style_value = Some(css_to_js_object(value));
                                static_style_prop_idx = Some(i);
                                continue;
                            }
                            out.add_ssr_import(SsrHelper::RenderStyle);
                            result.push_str(&format!(
                                " style=\"${{_ssrRenderStyle({})}}\"",
                                css_to_js_object(value)
                            ));
                        } else {
                            // Vue trims class attribute values (trailing whitespace)
                            let emit_value = if name == "class" {
                                value.trim_end()
                            } else {
                                value
                            };
                            result.push(' ');
                            result.push_str(name);
                            result.push_str("=\"");
                            result.push_str(&escape_template_literal(emit_value));
                            result.push('"');
                        }
                    } else {
                        result.push(' ');
                        result.push_str(name);
                    }
                }
            }
            // Merge all style parts (v-show, :style, static style) into a single attribute
            let v_show_style = if has_v_show {
                Some(format!(
                    "({}) ? null : {{ display: \"none\" }}",
                    v_show_expr
                ))
            } else {
                None
            };
            let has_any_style = dynamic_style_resolved.is_some()
                || static_style_value.is_some()
                || v_show_style.is_some();
            if has_any_style {
                out.add_ssr_import(SsrHelper::RenderStyle);
                let mut style_parts: Vec<String> = Vec::new();
                if let Some(ref dyn_style) = dynamic_style_resolved {
                    style_parts.push(dyn_style.clone());
                }
                if let Some(ref stat_style) = static_style_value {
                    style_parts.push(stat_style.clone());
                }
                if let Some(vshow) = v_show_style {
                    style_parts.push(vshow);
                }
                if style_parts.len() == 1 {
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle({})}}\"",
                        style_parts[0]
                    ));
                } else {
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle([{}])}}\"",
                        style_parts.join(", ")
                    ));
                }
            }
            result.push_str(&option_selected_suffix);
            return result;
        }

        // Add ref to attrs for root _mergeProps path if not yet emitted during prop loop
        // (ref appears after all other props in source order)
        if let Some((_, ref entry_str)) = &ref_entry {
            if !ref_emitted {
                if !attrs_obj.is_empty() {
                    attrs_obj.push_str(", ");
                }
                attrs_obj.push_str(entry_str);
                has_dynamic_attrs = true;
            }
        }

        // Build the final expression (existing path for root/v-bind-spread)
        if !is_root && !has_dynamic_attrs && !has_v_show && !has_v_bind_spread {
            let mut attrs = self.build_literal_attrs(el, source, out);
            attrs.push_str(&option_selected_suffix);
            return attrs;
        }

        let mut parts: Vec<String> = Vec::new();

        if has_v_bind_spread {
            parts.push(v_bind_spread_expr);
        }

        if !attrs_obj.is_empty() {
            parts.push(format!("{{ {} }}", attrs_obj));
        }

        if is_root
            && (has_v_show || !parts.is_empty() || has_v_bind_spread || !directive_calls.is_empty())
        {
            parts.push("_attrs".to_string());
        }

        if has_v_show {
            parts.push(format!(
                "{{ style: ({}) ? null : {{ display: \"none\" }} }}",
                v_show_expr
            ));
        }

        // Custom directive props
        for dir_call in &directive_calls {
            parts.push(dir_call.clone());
        }

        // v-model on root input: use _temp0 pattern for _ssrGetDynamicModelProps.
        // Vue first merges all props + _attrs into _temp0, then passes _temp0 to
        // _ssrGetDynamicModelProps which determines the correct prop at runtime.
        let use_temp0 = is_root && v_model_is_real && tag_name_str == "input";

        // Vue passes the tag name as second arg to _ssrRenderAttrs for textarea
        // so the renderer knows to skip the `value` attribute (rendered as content).
        let tag_arg = if tag_name_str == "textarea" {
            r#", "textarea""#
        } else {
            ""
        };

        let base = if use_temp0 {
            if let Some(ref model_expr) = v_model_expr {
                self.temp_var_needed = true;
                out.add_ssr_import(SsrHelper::RenderAttrs);
                out.add_ssr_import(SsrHelper::GetDynamicModelProps);
                out.add_vdom_import(VdomHelper::MergeProps);
                // Build: (_temp0 = _mergeProps(parts..., _attrs), _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, expr)))
                let first_merge = format!("_mergeProps({})", parts.join(", "));
                format!(
                    "${{_ssrRenderAttrs((_temp0 = {}, _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, {}))))}}",
                    first_merge, model_expr
                )
            } else {
                // Shouldn't happen — v_model_is_real implies v_model_expr is Some
                String::new()
            }
        } else if is_root {
            out.add_ssr_import(SsrHelper::RenderAttrs);

            if parts.is_empty() {
                format!("${{_ssrRenderAttrs(_attrs{})}}", tag_arg)
            } else {
                out.add_vdom_import(VdomHelper::MergeProps);
                format!(
                    "${{_ssrRenderAttrs(_mergeProps({}){})}}",
                    parts.join(", "),
                    tag_arg,
                )
            }
        } else if parts.is_empty() {
            String::new()
        } else if parts.len() == 1 && !has_v_show {
            out.add_ssr_import(SsrHelper::RenderAttrs);
            format!("${{_ssrRenderAttrs({}{})}}", parts[0], tag_arg)
        } else {
            out.add_ssr_import(SsrHelper::RenderAttrs);
            out.add_vdom_import(VdomHelper::MergeProps);
            format!(
                "${{_ssrRenderAttrs(_mergeProps({}){})}}",
                parts.join(", "),
                tag_arg,
            )
        };
        if option_selected_suffix.is_empty() {
            base
        } else {
            format!("{}{}", base, option_selected_suffix)
        }
    }

    /// Build literal HTML attributes for a nested (non-root) static element.
    /// Static `style` attributes are wrapped in `_ssrRenderStyle()` to match Vue.
    fn build_literal_attrs(
        &self,
        el: &ElementNode,
        source: &str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> String {
        let mut result = String::new();
        for prop in &el.props {
            if prop.is_directive {
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                if name == "style" {
                    // Vue wraps static styles in _ssrRenderStyle() with JS object
                    out.add_ssr_import(SsrHelper::RenderStyle);
                    result.push_str(&format!(
                        " style=\"${{_ssrRenderStyle({})}}\"",
                        css_to_js_object(value)
                    ));
                } else {
                    // Vue trims class attribute values (trailing whitespace)
                    let emit_value = if name == "class" {
                        value.trim_end()
                    } else {
                        value
                    };
                    result.push(' ');
                    result.push_str(name);
                    result.push_str("=\"");
                    result.push_str(&escape_template_literal(emit_value));
                    result.push('"');
                }
            } else {
                result.push(' ');
                result.push_str(name);
            }
        }
        result
    }

    // ── Directive value extraction ──────────────────────────────

    /// Check if an element has v-html directive.
    fn get_v_html_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Option<String> {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-html" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
        }
        None
    }

    /// Check if an element has v-text directive.
    fn get_v_text_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Option<String> {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-text" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
        }
        None
    }

    /// Extract v-model or :value expression from an element (for textarea content injection).
    fn get_v_model_expr(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Option<String> {
        let mut value_binding: Option<String> = None;
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name.starts_with("v-model") {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    return Some(self.resolve_expr(expr, vs, oxc_expr));
                }
            }
            // Also check for :value binding (rendered as content for textarea)
            if prop.is_directive {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr_name = &source[as_ as usize..ae as usize];
                    if attr_name == "value" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            value_binding = Some(self.resolve_expr(expr, vs, oxc_expr));
                        }
                    }
                }
            }
        }
        value_binding
    }

    // ── Slot helpers ───────────────────────────────────────────

    /// Check if a component has any non-template-v-slot children (default slot content).
    /// These children need to be wrapped in `default: _withCtx(...)` when named slots exist.
    #[allow(dead_code)]
    fn has_default_slot_children(&self, el: &ElementNode, source: &str) -> bool {
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                match &child.kind {
                    AstNodeKind::Element(ref child_el) => {
                        if !(child_el.tag_type == TagType::Template && child_el.v_slot.is_some()) {
                            return true;
                        }
                    }
                    AstNodeKind::Interpolation(_) | AstNodeKind::Comment(_) => return true,
                    AstNodeKind::Text(t) => {
                        let text = &source[t.start as usize..t.end as usize];
                        if !text.trim().is_empty() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Build a slot name from a `v-slot` directive prop, including modifiers for
    /// dot-notation names. `#header.id` → `"header.id"`, `#default` → `default`.
    fn build_slot_name(v_slot: &NodeProp, source: &str) -> String {
        if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
            let raw = &source[as_ as usize..ae as usize];
            if v_slot.modifiers.is_empty() {
                if needs_quoted_key(raw) {
                    format!("\"{}\"", raw)
                } else {
                    raw.to_string()
                }
            } else {
                // Dot-notation slot names: #header.id → "header.id"
                let mut name = raw.to_string();
                for modifier in &v_slot.modifiers {
                    name.push('.');
                    name.push_str(modifier.slice(source));
                }
                // Names with dots always need quoting
                format!("\"{}\"", name)
            }
        } else {
            "default".to_string()
        }
    }

    /// Open an implicit `default: _withCtx(...)` wrapper at the given position.
    /// Called when the first non-template child of a ComponentWithSlots is encountered.
    fn open_default_slot(&mut self, pos: u32, out: &mut CodeGenOutput<'alloc>) {
        self.close_push(pos, out);
        out.add_vdom_import(VdomHelper::WithCtx);
        self.buf.clear();
        let _ = write!(
            self.buf,
            "\ndefault: _withCtx((_, _push, _parent, _scopeId) => {{\nif (_push) {{\n"
        );
        out.prepend_alloc(pos, &self.buf);
        self.default_slot_open = true;
        self.in_push = false;
        // Record start for potential reordering (default before named slots).
        // Use comp_children_start (component tag_open.end) to ensure Inserted
        // chunks are captured even when whitespace overwrites leave position gaps.
        self.default_slot_move_start = self.comp_children_start;
    }

    /// Check if a component's children contain `<template v-slot>` wrappers.
    fn has_template_slot_children(&self, el: &ElementNode) -> bool {
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                if let AstNodeKind::Element(ref child_el) = child.kind {
                    if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a Suspense/component has bare (non-template-slot) children.
    /// Returns true if there are elements, text, or expressions that are NOT
    /// wrapped in `<template v-slot>`.
    fn has_bare_children(&self, el: &ElementNode, source: &str) -> bool {
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                match &child.kind {
                    AstNodeKind::Element(ref child_el) => {
                        if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                            continue; // skip named slot templates
                        }
                        return true;
                    }
                    AstNodeKind::Text(t) => {
                        // Skip whitespace-only text nodes
                        let text = &source[t.start as usize..t.end as usize];
                        if !text.trim().is_empty() {
                            return true;
                        }
                    }
                    AstNodeKind::Interpolation(_) => return true,
                    AstNodeKind::Comment(_) => {} // skip
                }
            }
        }
        false
    }

    /// Check if a component has any dynamic slot children.
    ///
    /// A slot is dynamic when:
    /// - Any `<template v-slot>` child has `v-if`/`v-else-if`/`v-else`
    /// - Any `<template v-slot>` child has `v-for`
    /// - Any `<template v-slot>` child has a dynamic slot name (`#[expr]`)
    /// - The component itself is inside a `v-for` loop
    fn has_dynamic_slots(&self, el: &ElementNode) -> bool {
        if self.v_for_depth.get() > 0 {
            return true;
        }
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                if let AstNodeKind::Element(ref child_el) = child.kind {
                    if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                        // v-if/v-else-if/v-else on a slot template
                        if child_el.v_condition.is_some() {
                            return true;
                        }
                        // v-for on a slot template
                        if child_el.v_for.is_some() {
                            return true;
                        }
                        // Dynamic slot name: #[expr]
                        if let Some(ref slot) = child_el.v_slot {
                            if slot.is_dynamic == Some(true) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Check if an element has exactly one effective child and that child is an element.
    ///
    /// Vue SSR skips fragment markers for `<template v-if>` that wraps exactly
    /// one element child (no text/interpolation siblings). The child is rendered
    /// directly without `<!--[-->...<!--]-->` wrapping.
    fn has_single_element_child(&self, el: &ElementNode, source: &str) -> bool {
        if let Some(ref content) = el.content {
            let mut element_count = 0;
            let mut has_non_ws_text_or_interp = false;
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                match &child.kind {
                    AstNodeKind::Element(child_el) => {
                        // v-else-if and v-else are part of the same branch chain
                        if let Some(ref cond) = child_el.v_condition {
                            if matches!(
                                cond.kind,
                                ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                            ) {
                                continue;
                            }
                        }
                        element_count += 1;
                    }
                    AstNodeKind::Interpolation(_) => {
                        has_non_ws_text_or_interp = true;
                    }
                    AstNodeKind::Comment(_) => {}
                    AstNodeKind::Text(t) => {
                        let text = &source[t.start as usize..t.end as usize];
                        if !text.trim().is_empty() {
                            has_non_ws_text_or_interp = true;
                        }
                    }
                }
            }
            return element_count == 1 && !has_non_ws_text_or_interp;
        }
        false
    }

    /// Check if a component has any effective children (non-whitespace).
    fn has_effective_children(&self, el: &ElementNode, source: &str) -> bool {
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                let child = &self.ast.nodes[child_id.0];
                match &child.kind {
                    AstNodeKind::Element(_) => return true,
                    AstNodeKind::Interpolation(_) => return true,
                    AstNodeKind::Comment(_) => return true,
                    AstNodeKind::Text(t) => {
                        let text = &source[t.start as usize..t.end as usize];
                        if !text.trim().is_empty() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Extract slot name from a `<slot>` outlet element.
    /// Returns `("default", false)` if no `name` attribute is found.
    /// For dynamic `:name="expr"`, returns `(resolved_expr, true)`.
    fn extract_slot_outlet_name(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> (String, bool) {
        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            // Static name="header"
            if !prop.is_directive && prop_name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    return (source[vs as usize..ve as usize].to_string(), false);
                }
            }
            // Dynamic :name="expr" or v-bind:name="expr"
            if prop.is_directive
                && (prop_name.starts_with(':')
                    || (prop_name == "v-bind" && prop.arg_start.is_some()))
            {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    if attr == "name" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);
                            return (resolved, true);
                        }
                    }
                }
            }
        }
        ("default".to_string(), false)
    }

    /// Build slot outlet props (non-name static/dynamic attrs).
    /// Build the props argument for `_ssrRenderSlot`.
    /// Returns `(props_string, needs_merge_props)`.
    fn build_slot_outlet_props(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> (String, bool) {
        let mut parts: Vec<String> = Vec::new();
        let mut spreads: Vec<String> = Vec::new();
        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive {
                if prop_name == "name" {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let value = &source[vs as usize..ve as usize];
                    // Slot outlet props are always camelized (like component props)
                    let key = camelize(prop_name);
                    parts.push(format!("{}: \"{}\"", key, escape_js_string(value)));
                }
            } else if prop_name == "v-bind" && prop.arg_start.is_none() {
                // v-bind="obj" spread (no argument)
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                    spreads.push(resolved);
                }
            } else if prop_name.starts_with(':')
                || (prop_name == "v-bind" && prop.arg_start.is_some())
            {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let attr = &source[as_ as usize..ae as usize];
                    // Skip :name — handled by extract_slot_outlet_name
                    if attr == "name" {
                        continue;
                    }
                    let key = camelize(attr);
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        parts.push(format!("{}: {}", key, resolved));
                    } else {
                        // Same-name shorthand: `:items` ≡ `:items="items"`
                        // Camelize the attr for binding lookup — hyphenated
                        // names like `heading-value` must resolve to
                        // `headingValue`, not `heading - value` (subtraction).
                        let prefix = self.resolver.resolve_prefix(&key);
                        let resolved = if prefix.is_empty() {
                            format!("_ctx.{}", key)
                        } else {
                            format!("{}{}", prefix, key)
                        };
                        parts.push(format!("{}: {}", key, resolved));
                    }
                }
            }
        }

        if spreads.is_empty() {
            // No v-bind spread — same as before
            if parts.is_empty() {
                ("{}".to_string(), false)
            } else {
                (format!("{{ {} }}", parts.join(", ")), false)
            }
        } else if parts.is_empty() {
            // Only v-bind spread(s), no individual props
            if spreads.len() == 1 {
                // Single spread — use directly, no _mergeProps needed
                (spreads.remove(0), false)
            } else {
                (format!("_mergeProps({})", spreads.join(", ")), true)
            }
        } else {
            // v-bind spread(s) + individual props → _mergeProps(spread, { props })
            let obj = format!("{{ {} }}", parts.join(", "));
            let mut args: Vec<String> = spreads;
            args.push(obj);
            (format!("_mergeProps({})", args.join(", ")), true)
        }
    }

    /// Detect the input type from element props (for v-model handling).
    fn get_input_type<'s>(&self, el: &ElementNode, source: &'s str) -> Option<&'s str> {
        for prop in &el.props {
            if !prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "type" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        return Some(&source[vs as usize..ve as usize]);
                    }
                }
            }
        }
        None
    }

    /// Extract the `value` expression for an `<option>` element.
    ///
    /// - Static `value="X"` → `"X"` (string literal)
    /// - Dynamic `:value="expr"` → resolved expression
    /// - No value attribute → `""` (empty string)
    fn get_option_value(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> String {
        for (i, prop) in el.props.iter().enumerate() {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive && name == "value" {
                // Static value="X"
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let val = &source[vs as usize..ve as usize];
                    return format!("\"{}\"", escape_js_string(val));
                }
                return "\"\"".to_string();
            }
            // Dynamic :value="expr" or v-bind:value="expr"
            if prop.is_directive && (name == ":" || name == "v-bind") {
                if let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) {
                    let arg = &source[arg_s as usize..arg_e as usize];
                    if arg == "value" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            return self.resolve_expr(expr, vs, oxc_expr);
                        }
                        return "\"\"".to_string();
                    }
                }
            }
        }
        // No value attribute
        "\"\"".to_string()
    }

    // ── Whitespace condensation helpers ────────────────────────────

    /// Determine whether a whitespace-only text node should be removed entirely,
    /// matching Vue's `condense` whitespace mode.
    ///
    /// Returns `true` (remove) when:
    /// 1. No previous sibling (first child) OR no next sibling (last child)
    /// 2. Between two comments
    /// 3. Between a comment and an element (either direction)
    /// 4. Between two elements AND text contains `\r` or `\n`
    ///
    /// Returns `false` (keep as single space) for all other cases, notably
    /// when adjacent to an interpolation — e.g. `{{ msg }}\n<span>` preserves
    /// the space between the interpolation and the element.
    fn should_remove_whitespace_node(&self, id: NodeId, content: &str) -> bool {
        let node = &self.ast.nodes[id.0];
        let siblings = if let Some(parent_id) = node.parent {
            let parent = &self.ast.nodes[parent_id.0];
            if let AstNodeKind::Element(ref el) = parent.kind {
                el.content.as_ref().map(|c| c.children.as_slice())
            } else {
                None
            }
        } else {
            self.ast
                .root
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
        };

        let Some(siblings) = siblings else {
            return true; // No siblings info → remove to be safe
        };
        let idx = node.index_in_parent;

        // Rule 1: boundary — first or last child.
        // Exception: if adjacent sibling is also a Text node, this whitespace
        // is part of a continuous text run (split by entity boundaries like &gt;)
        // and should be condensed to a space, not removed.
        if idx == 0 && idx == siblings.len() - 1 {
            // Only child — remove
            return true;
        }
        if idx == 0 {
            let next_kind = &self.ast.nodes[siblings[1].0].kind;
            if !matches!(next_kind, AstNodeKind::Text(_)) {
                return true;
            }
            // Next sibling is text — part of a text run, keep as space
            return false;
        }
        if idx == siblings.len() - 1 {
            let prev_kind = &self.ast.nodes[siblings[idx - 1].0].kind;
            if !matches!(prev_kind, AstNodeKind::Text(_)) {
                return true;
            }
            // Prev sibling is text — part of a text run, keep as space
            return false;
        }

        // Get prev and next sibling kinds
        let prev = &self.ast.nodes[siblings[idx - 1].0].kind;
        let next = &self.ast.nodes[siblings[idx + 1].0].kind;

        let prev_is_element = matches!(prev, AstNodeKind::Element(_));
        let prev_is_comment = matches!(prev, AstNodeKind::Comment(_));
        let next_is_element = matches!(next, AstNodeKind::Element(_));
        let next_is_comment = matches!(next, AstNodeKind::Comment(_));

        // Rule 2: between two comments
        if prev_is_comment && next_is_comment {
            return true;
        }

        // Rule 3: between a comment and an element
        if (prev_is_comment && next_is_element) || (prev_is_element && next_is_comment) {
            return true;
        }

        // Rule 4: between two elements AND has newline
        let has_newline = content.contains('\n') || content.contains('\r');
        if prev_is_element && next_is_element && has_newline {
            return true;
        }

        // All other cases: keep as single space
        false
    }

    /// Check if an element's descendants contain `<slot>` elements (slot outlets).
    /// When a component's slot content forwards parent slots via `<slot>`, the
    /// slot stability flag should be `_: 3 /* FORWARDED */` instead of `_: 1`.
    fn has_slot_outlet_in_descendants(&self, el: &ElementNode, source: &str) -> bool {
        if let Some(ref content) = el.content {
            for &child_id in &content.children {
                if self.node_is_or_contains_slot_outlet(child_id, source) {
                    return true;
                }
            }
        }
        false
    }

    fn node_is_or_contains_slot_outlet(&self, id: NodeId, source: &str) -> bool {
        let node = &self.ast.nodes[id.0];
        if let AstNodeKind::Element(ref el) = node.kind {
            let tag = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
            if tag.eq_ignore_ascii_case("slot") {
                return true;
            }
            if let Some(ref content) = el.content {
                for &child_id in &content.children {
                    if self.node_is_or_contains_slot_outlet(child_id, source) {
                        return true;
                    }
                }
            }
        }
        false
    }

    // ── Dynamic component detection ──────────────────────────────

    /// Detect `<component :is="expr">` and return the resolved expression + prop index.
    fn resolve_dynamic_is(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
        _out: &mut CodeGenOutput<'alloc>,
    ) -> Option<(String, usize)> {
        // Find :is or v-bind:is directive
        for (i, prop) in el.props.iter().enumerate() {
            if !prop.is_directive {
                // Check for static is="value"
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "is" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let value = &source[vs as usize..ve as usize];
                        return Some((format!("_resolveDynamicComponent(\"{}\")", value), i));
                    }
                }
                continue;
            }
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            if !(directive_name.starts_with(':') || directive_name.starts_with("v-bind")) {
                continue;
            }
            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                let arg_name = &source[as_ as usize..ae as usize];
                if arg_name != "is" {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                    return Some((format!("_resolveDynamicComponent({})", resolved), i));
                }
            }
        }
        None
    }

    /// Resolve a component reference for VDOM output: try setup bindings (as-is
    /// then camelCase then PascalCase), falling back to `_component_xxx`.
    fn resolve_vdom_component_ref(&self, tag_name: &str) -> String {
        if self.resolver.get(tag_name).is_some() {
            let prefix = self.resolver.resolve_prefix(tag_name);
            let suffix = self.resolver.resolve_suffix(tag_name);
            let raw = format!("{}{}{}", prefix, tag_name, suffix);
            setup_dot_to_bracket(&raw)
        } else {
            let camel = camelize(tag_name);
            let pascal = to_pascal_case(tag_name);
            if self.resolver.get(camel.as_ref()).is_some() {
                let prefix = self.resolver.resolve_prefix(camel.as_ref());
                let suffix = self.resolver.resolve_suffix(camel.as_ref());
                let raw = format!("{}{}{}", prefix, camel, suffix);
                setup_dot_to_bracket(&raw)
            } else if self.resolver.get(&pascal).is_some() {
                let prefix = self.resolver.resolve_prefix(&pascal);
                let suffix = self.resolver.resolve_suffix(&pascal);
                let raw = format!("{}{}{}", prefix, pascal, suffix);
                setup_dot_to_bracket(&raw)
            } else {
                let var_name = tag_name.replace('-', "_");
                format!("_component_{}", var_name)
            }
        }
    }

    // ── Condition continuation detection ────────────────────────

    /// Check if this element (which has v-if or v-else-if) has a continuation
    /// sibling with v-else-if or v-else.
    fn has_condition_continuation(&self, el: &ElementNode) -> bool {
        if let Some(ref root_content) = self.ast.root.content {
            if let Some(result) = self.find_continuation_in(&root_content.children, el) {
                return result;
            }
        }
        for node in &self.ast.nodes {
            if let AstNodeKind::Element(ref parent_el) = node.kind {
                if let Some(ref content) = parent_el.content {
                    if let Some(result) = self.find_continuation_in(&content.children, el) {
                        return result;
                    }
                }
            }
        }
        false
    }

    /// Search a children list for `el` (by tag_open.start) and check if
    /// the next element sibling has v-else-if or v-else.
    fn find_continuation_in(&self, children: &[NodeId], el: &ElementNode) -> Option<bool> {
        let mut found_self = false;
        for &child_id in children {
            let child = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = child.kind {
                if child_el.tag_open.start == el.tag_open.start {
                    found_self = true;
                    continue;
                }
                if found_self {
                    return Some(child_el.v_condition.as_ref().is_some_and(|c| {
                        matches!(
                            c.kind,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                        )
                    }));
                }
            } else if found_self {
                // Skip whitespace text and comments between v-if branches
                if matches!(child.kind, AstNodeKind::Text(_) | AstNodeKind::Comment(_)) {
                    continue;
                }
                return Some(false);
            }
        }
        if found_self {
            Some(false)
        } else {
            None
        }
    }
}

impl<'ast, 'alloc> TemplateCodeGen<'alloc> for SsrCodeGen<'ast, 'alloc> {
    fn enter_template(
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
        let effective_count = self.count_effective_roots(root_children, source);
        // Count element-level roots only (excluding text/interpolation) for fragment
        // marker decisions. Vue SSR only adds fragment markers when there are
        // multiple element roots, not for text/interpolation content.
        let element_root_count = self.count_element_roots(root_children);
        self.is_multi_root = effective_count > 1;

        // Vue's SSR compiler treats templates with root-level comments as
        // needing fragment markers for hydration, even if there's only one
        // effective element root. The single element still receives _attrs.
        let has_root_comments = root_children
            .iter()
            .any(|&id| matches!(self.ast.nodes[id.0].kind, AstNodeKind::Comment(_)));
        self.needs_fragment = element_root_count > 1 || has_root_comments;

        if self.needs_fragment {
            // Open a push with the fragment open marker.
            out.prepend_alloc(root.tag_open.end, "_push(`<!--[-->");
            self.in_push = true;
        }
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

        let effective_count = self.count_effective_roots(root_children, source);

        // Build function signature with hoisted component resolves
        self.buf.clear();
        if self.has_scope_id {
            self.buf.push_str("function ssrRender(_ctx, _push, _parent, _attrs, $setup, $data, $options, _scopeId) {\n");
        } else {
            self.buf
                .push_str("function ssrRender(_ctx, _push, _parent, _attrs) {\n");
        }
        for resolve in &self.component_resolves {
            self.buf.push_str(resolve);
            self.buf.push('\n');
        }
        for resolve in &self.directive_resolves {
            self.buf.push_str(resolve);
            self.buf.push('\n');
        }
        if self.temp_var_needed {
            self.buf.push_str("let _temp0\n");
        }
        let fn_sig = self.buf.clone();

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

        // Close any open push before the function close.
        // For templates needing fragment markers (multi-root or root comments),
        // append the fragment close marker before closing.
        if self.needs_fragment {
            if self.in_push {
                out.prepend_alloc(close_start, "<!--]-->`)\n");
                self.in_push = false;
            } else {
                out.prepend_alloc(close_start, "_push(`<!--]-->`)\n");
            }
        } else if self.in_push {
            self.close_push(close_start, out);
        }

        if effective_count == 0 {
            // Empty template
            out.overwrite(root.tag_open.start, close_end, &fn_sig);
            out.prepend_static(close_end, "}");
        } else {
            out.overwrite(root.tag_open.start, root.tag_open.end, &fn_sig);
            out.overwrite(close_start, close_end, "}");
        }
    }

    fn enter_element(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> super::WalkAction {
        let tag_name = self.tag_name(el, source);
        let is_root = self.is_root();

        // ── 0. Implicit default slot wrapping ───────────────────
        // When inside a ComponentWithSlots with named slots, non-template
        // children need to be wrapped in `default: _withCtx(...)`.
        if matches!(
            self.elem_ctx.last(),
            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
        ) && !(el.tag_type == TagType::Template && el.v_slot.is_some())
            && !self.default_slot_open
        {
            self.open_default_slot(el.tag_open.start, out);
        }

        // ── 1. Structural directive preambles ───────────────────

        // v-if/v-else-if/v-else: close parent push, emit conditional
        if let Some(ref cond) = el.v_condition {
            // Close parent's push before the conditional statement
            self.close_push(el.tag_open.start, out);

            match cond.kind {
                ElementNodeConditionKind::If => {
                    let oxc_cond = oxc.and_then(|o| o.condition.as_ref());
                    let expr_str = if let (Some(vs), Some(ve)) =
                        (cond.prop.value_start, cond.prop.value_end)
                    {
                        let raw = &source[vs as usize..ve as usize];
                        self.resolve_expr(raw, vs, oxc_cond)
                    } else {
                        "true".to_string()
                    };
                    self.buf.clear();
                    let _ = writeln!(self.buf, "if ({}) {{", expr_str);
                    out.prepend_alloc(el.tag_open.start, &self.buf);
                }
                ElementNodeConditionKind::ElseIf => {
                    let oxc_cond = oxc.and_then(|o| o.condition.as_ref());
                    let expr_str = if let (Some(vs), Some(ve)) =
                        (cond.prop.value_start, cond.prop.value_end)
                    {
                        let raw = &source[vs as usize..ve as usize];
                        self.resolve_expr(raw, vs, oxc_cond)
                    } else {
                        "true".to_string()
                    };
                    self.buf.clear();
                    let _ = writeln!(self.buf, "}} else if ({}) {{", expr_str);
                    out.prepend_alloc(el.tag_open.start, &self.buf);
                }
                ElementNodeConditionKind::Else => {
                    out.prepend_alloc(el.tag_open.start, "} else {\n");
                }
            }
        }

        // v-for: merge fragment open marker into parent push, then close & emit _ssrRenderList
        if let Some(ref v_for) = el.v_for {
            self.v_for_depth.set(self.v_for_depth.get() + 1);

            // Merge fragment open marker into current push if one is open,
            // otherwise emit it as a separate push.
            if self.in_push {
                out.prepend_alloc(el.tag_open.start, "<!--[-->");
                self.close_push(el.tag_open.start, out);
            } else {
                out.prepend_alloc(el.tag_open.start, "_push(`<!--[-->`)\n");
            }

            out.add_ssr_import(SsrHelper::RenderList);

            let full_expr = helpers::extract_directive_value(v_for, source);
            let (params, iterable) = helpers::parse_v_for_expression(full_expr);

            let resolved_iterable = if let Some(oxc_el) = oxc {
                if let Some(ref vfor_data) = oxc_el.v_for {
                    let refs = &vfor_data.parsed.references;
                    if refs.is_empty() {
                        iterable.to_string()
                    } else {
                        super::vdom::directives::build_prefixed_iterable(
                            iterable,
                            source,
                            v_for,
                            &vfor_data.parsed,
                            &self.resolver,
                        )
                    }
                } else {
                    self.resolver.resolve_simple_expr(iterable)
                }
            } else {
                self.resolver.resolve_simple_expr(iterable)
            };

            self.buf.clear();
            let _ = writeln!(
                self.buf,
                "_ssrRenderList({}, ({}) => {{",
                resolved_iterable, params
            );
            out.prepend_alloc(el.tag_open.start, &self.buf);
        }

        // ── 2. Component handling ───────────────────────────────

        if el.tag_type == TagType::Component {
            // ── 2a. Dynamic component: <component :is="expr"> ───────
            if tag_name == "component" {
                if let Some((dynamic_expr, is_prop_idx)) =
                    self.resolve_dynamic_is(el, oxc, source, out)
                {
                    self.close_push(el.tag_open.start, out);

                    out.add_ssr_import(SsrHelper::RenderVNode);
                    out.add_vdom_import(VdomHelper::CreateVNode);
                    out.add_vdom_import(VdomHelper::ResolveDynamicComponent);

                    // Build props excluding the :is prop
                    // Pre-scan for class merge: static `class` + `:class` → merged array
                    let mut dyn_static_class: Option<String> = None;
                    let mut dyn_static_class_idx: Option<usize> = None;
                    let mut dyn_dynamic_class: Option<String> = None;
                    let mut dyn_dynamic_class_idx: Option<usize> = None;
                    for (i, prop) in el.props.iter().enumerate() {
                        if i == is_prop_idx {
                            continue;
                        }
                        let pn = &source[prop.start as usize..prop.name_end as usize];
                        if !prop.is_directive && pn == "class" {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                dyn_static_class =
                                    Some(source[vs as usize..ve as usize].to_string());
                                dyn_static_class_idx = Some(i);
                            }
                        } else if prop.is_directive
                            && (pn.starts_with(':') || pn.starts_with("v-bind"))
                        {
                            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                if &source[as_ as usize..ae as usize] == "class" {
                                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end)
                                    {
                                        let expr = &source[vs as usize..ve as usize];
                                        let oxc_expr = oxc
                                            .and_then(|o| find_oxc_prop(o, i))
                                            .and_then(|p| p.exp.as_ref());
                                        dyn_dynamic_class =
                                            Some(self.resolve_expr(expr, vs, oxc_expr));
                                        dyn_dynamic_class_idx = Some(i);
                                    }
                                }
                            }
                        }
                    }
                    let dyn_merge_class = dyn_static_class.is_some() && dyn_dynamic_class.is_some();

                    let mut props_parts: Vec<String> = Vec::new();
                    let mut dyn_directive_calls: Vec<String> = Vec::new();
                    let mut has_v_bind_spread = false;
                    let mut v_bind_spread_expr = String::new();
                    for (i, prop) in el.props.iter().enumerate() {
                        if i == is_prop_idx {
                            continue;
                        }
                        // Handle class merge
                        if dyn_merge_class
                            && (Some(i) == dyn_static_class_idx || Some(i) == dyn_dynamic_class_idx)
                        {
                            if Some(i) == dyn_static_class_idx {
                                props_parts.push(format!(
                                    "class: [\"{}\", {}]",
                                    escape_js_string(dyn_static_class.as_ref().unwrap()),
                                    dyn_dynamic_class.as_ref().unwrap()
                                ));
                            }
                            continue;
                        }
                        let prop_name = &source[prop.start as usize..prop.name_end as usize];
                        if prop.is_directive {
                            // Event handlers on dynamic component
                            if prop_name.starts_with('@') || prop_name == "v-on" {
                                let event_name = if let Some(after_at) = prop_name.strip_prefix('@')
                                {
                                    if after_at.is_empty() {
                                        match (prop.arg_start, prop.arg_end) {
                                            (Some(s), Some(e)) => &source[s as usize..e as usize],
                                            _ => {
                                                continue;
                                            }
                                        }
                                    } else {
                                        after_at
                                    }
                                } else {
                                    match (prop.arg_start, prop.arg_end) {
                                        (Some(s), Some(e)) => &source[s as usize..e as usize],
                                        _ => {
                                            continue;
                                        }
                                    }
                                };
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    let expr = &source[vs as usize..ve as usize];
                                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                    let mut js_key = String::with_capacity(event_name.len() + 2);
                                    format_event_handler_key_into(&mut js_key, event_name);
                                    if needs_quoted_key(&js_key) {
                                        props_parts.push(format!("\"{}\": {}", js_key, resolved));
                                    } else {
                                        props_parts.push(format!("{}: {}", js_key, resolved));
                                    }
                                }
                                continue;
                            }
                            // v-bind spread (no argument): v-bind="obj"
                            if (prop_name == "v-bind" || prop_name == ":")
                                && prop.arg_start.is_none()
                            {
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    let expr = &source[vs as usize..ve as usize];
                                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                    has_v_bind_spread = true;
                                    v_bind_spread_expr = resolved;
                                }
                                continue;
                            }
                            // v-model on dynamic component
                            if prop_name.starts_with("v-model") {
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    let expr = &source[vs as usize..ve as usize];
                                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                    // Determine model prop name (v-model:title -> "title")
                                    let model_prop = if let (Some(as_), Some(ae)) =
                                        (prop.arg_start, prop.arg_end)
                                    {
                                        source[as_ as usize..ae as usize].to_string()
                                    } else {
                                        "modelValue".to_string()
                                    };
                                    props_parts.push(format!("{}: {}", model_prop, resolved));
                                    props_parts.push(format!(
                                        "\"onUpdate:{}\": $event => (({}) = $event)",
                                        model_prop, resolved
                                    ));
                                }
                                continue;
                            }
                            if prop_name.starts_with(':') || prop_name == "v-bind" {
                                if let (Some(as_), Some(ae), Some(vs), Some(ve)) = (
                                    prop.arg_start,
                                    prop.arg_end,
                                    prop.value_start,
                                    prop.value_end,
                                ) {
                                    let attr = &source[as_ as usize..ae as usize];
                                    // :key is passed through on components (unlike HTML elements)
                                    let expr = &source[vs as usize..ve as usize];
                                    let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                    props_parts.push(format!(
                                        "{}: {}",
                                        html_attr_to_js_key(attr),
                                        resolved
                                    ));
                                }
                                continue;
                            }
                            // Custom directives on dynamic components.
                            // Exclude built-in directives handled elsewhere or
                            // that are structural (removed from el.props by the
                            // parser).
                            if prop_name.starts_with("v-")
                                && !matches!(
                                    prop_name,
                                    "v-bind"
                                        | "v-on"
                                        | "v-model"
                                        | "v-show"
                                        | "v-html"
                                        | "v-text"
                                        | "v-memo"
                                        | "v-cloak"
                                )
                            {
                                if let Some(call) = self.build_directive_props_call(
                                    prop, prop_name, source, oxc, i, out,
                                ) {
                                    dyn_directive_calls.push(call);
                                }
                            }
                        } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let value = &source[vs as usize..ve as usize];
                            props_parts.push(format!(
                                "{}: \"{}\"",
                                html_attr_to_js_key(prop_name),
                                escape_js_string(value)
                            ));
                        }
                    }

                    // For root-level dynamic components, merge _attrs into props
                    let dir_suffix = if dyn_directive_calls.is_empty() {
                        String::new()
                    } else {
                        format!(", {}", dyn_directive_calls.join(", "))
                    };
                    let props_str = if has_v_bind_spread {
                        // v-bind spread: merge spread expr with any other props + _attrs
                        out.add_vdom_import(VdomHelper::MergeProps);
                        if is_root {
                            if props_parts.is_empty() {
                                format!("_mergeProps({}, _attrs{})", v_bind_spread_expr, dir_suffix)
                            } else {
                                format!(
                                    "_mergeProps({{ {} }}, {}, _attrs{})",
                                    props_parts.join(", "),
                                    v_bind_spread_expr,
                                    dir_suffix
                                )
                            }
                        } else if props_parts.is_empty() && dyn_directive_calls.is_empty() {
                            v_bind_spread_expr
                        } else {
                            out.add_vdom_import(VdomHelper::MergeProps);
                            if props_parts.is_empty() {
                                format!("_mergeProps({}{})", v_bind_spread_expr, dir_suffix)
                            } else {
                                format!(
                                    "_mergeProps({{ {} }}, {}{})",
                                    props_parts.join(", "),
                                    v_bind_spread_expr,
                                    dir_suffix
                                )
                            }
                        }
                    } else if !dyn_directive_calls.is_empty() {
                        out.add_vdom_import(VdomHelper::MergeProps);
                        let dir_args = dyn_directive_calls.join(", ");
                        if is_root {
                            if props_parts.is_empty() {
                                format!("_mergeProps(_attrs, {})", dir_args)
                            } else {
                                format!(
                                    "_mergeProps({{ {} }}, _attrs, {})",
                                    props_parts.join(", "),
                                    dir_args
                                )
                            }
                        } else if props_parts.is_empty() {
                            dir_args
                        } else {
                            format!(
                                "_mergeProps({{ {} }}, {})",
                                props_parts.join(", "),
                                dir_args
                            )
                        }
                    } else if is_root {
                        if props_parts.is_empty() {
                            "_attrs".to_string()
                        } else {
                            out.add_vdom_import(VdomHelper::MergeProps);
                            format!("_mergeProps({{ {} }}, _attrs)", props_parts.join(", "))
                        }
                    } else if props_parts.is_empty() {
                        "null".to_string()
                    } else {
                        format!("{{ {} }}", props_parts.join(", "))
                    };

                    let has_children = self.has_effective_children(el, source);

                    if !has_children {
                        let sid_arg = self.scope_id_arg();
                        self.buf.clear();
                        let _ = writeln!(
                            self.buf,
                            "_ssrRenderVNode(_push, _createVNode({}, {}, null), _parent{})",
                            dynamic_expr, props_str, sid_arg
                        );
                        let el_end = self.el_end(el);
                        out.overwrite(el.tag_open.start, el_end, &self.buf);
                        self.elem_ctx.push(ElemCtx::Complete);
                    } else {
                        // Has children — render as slot content like regular components
                        out.add_vdom_import(VdomHelper::WithCtx);
                        let has_named_slots = self.has_template_slot_children(el);

                        self.saved_default_slot_open.push(self.default_slot_open);
                        self.saved_default_slot_move.push((
                            self.default_slot_move_start.take(),
                            self.default_slot_move_end.take(),
                        ));

                        self.buf.clear();
                        let _ = write!(
                            self.buf,
                            "_ssrRenderVNode(_push, _createVNode({}, {}, {{",
                            dynamic_expr, props_str
                        );

                        if !has_named_slots {
                            let params = if let Some(ref v_slot) = el.v_slot {
                                if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end)
                                {
                                    let p = source[vs as usize..ve as usize].trim();
                                    if p.is_empty() {
                                        "_"
                                    } else {
                                        p
                                    }
                                } else {
                                    "_"
                                }
                            } else {
                                "_"
                            };
                            let _ = write!(
                                self.buf,
                                "\ndefault: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                                params
                            );
                            self.default_slot_open = true;
                            // Track scoped slot depth for child component DYNAMIC flag
                            let is_scoped = params != "_";
                            if is_scoped {
                                self.v_slot_scope_depth += 1;
                            }
                            self.scoped_slot_entered.push(is_scoped);
                        } else {
                            self.default_slot_open = false;
                            self.comp_children_start = Some(el.tag_open.end);
                            self.scoped_slot_entered.push(false);
                        }

                        out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                        self.in_push = false;
                        self.in_component_slots += 1;
                        self.elem_ctx.push(ElemCtx::DynamicComponentWithSlots);
                    }
                    return super::WalkAction::Continue;
                }
            }

            // ── 2b. Built-in component: <Suspense> ───────────────────
            if let Some((_flag_bit, _helper_name)) = helpers::is_builtin_component(&tag_name) {
                if tag_name == "Suspense" || tag_name == "suspense" {
                    self.close_push(el.tag_open.start, out);
                    out.add_ssr_import(SsrHelper::RenderSuspense);

                    let has_children = self.has_effective_children(el, source);
                    if !has_children {
                        self.buf.clear();
                        let _ = writeln!(self.buf, "_ssrRenderSuspense(_push, {{}})");
                        let el_end = self.el_end(el);
                        out.overwrite(el.tag_open.start, el_end, &self.buf);
                        self.elem_ctx.push(ElemCtx::Complete);
                    } else {
                        let has_named_slots = self.has_template_slot_children(el);
                        let has_bare = self.has_bare_children(el, source);
                        self.buf.clear();
                        if has_named_slots && has_bare {
                            // Mixed content: bare children become implicit default slot,
                            // named <template v-slot> children are separate entries.
                            // `default: () => {` opened here; closed when first
                            // <template v-slot> is encountered.
                            self.buf
                                .push_str("_ssrRenderSuspense(_push, {\ndefault: () => {\n");
                        } else if has_named_slots {
                            // Pure named slots: just open the slots object; each <template v-slot>
                            // will emit its own `slotName: () => { ... }` entry.
                            self.buf.push_str("_ssrRenderSuspense(_push, {\n");
                        } else {
                            // Implicit default slot
                            self.buf
                                .push_str("_ssrRenderSuspense(_push, {\ndefault: () => {\n");
                        }
                        out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                        self.in_push = false;
                        self.in_component_slots += 1;
                        self.suspense_implicit_default_open = has_named_slots && has_bare;
                        self.elem_ctx.push(ElemCtx::SuspenseSlots);
                    }
                    return super::WalkAction::Continue;
                }

                // TransitionGroup — renders its `tag` prop as a real HTML element
                if matches!(tag_name.as_str(), "TransitionGroup" | "transition-group") {
                    // Extract tag prop (default: "span")
                    let mut tg_tag = "span".to_string();
                    let mut tg_attrs: Vec<String> = Vec::new();
                    for prop in el.props.iter() {
                        let prop_name = &source[prop.start as usize..prop.name_end as usize];
                        if !prop.is_directive {
                            if prop_name == "tag" {
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    tg_tag = source[vs as usize..ve as usize].to_string();
                                }
                            } else if prop_name == "name"
                                || prop_name == "appear"
                                || prop_name == "css"
                                || prop_name == "duration"
                                || prop_name == "mode"
                                || prop_name == "moveClass"
                                || prop_name == "move-class"
                                || prop_name == "enterFromClass"
                                || prop_name == "enter-from-class"
                                || prop_name == "enterActiveClass"
                                || prop_name == "enter-active-class"
                                || prop_name == "enterToClass"
                                || prop_name == "enter-to-class"
                                || prop_name == "leaveFromClass"
                                || prop_name == "leave-from-class"
                                || prop_name == "leaveActiveClass"
                                || prop_name == "leave-active-class"
                                || prop_name == "leaveToClass"
                                || prop_name == "leave-to-class"
                            {
                                // TransitionGroup-specific props — skip in HTML
                            } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end)
                            {
                                let value = &source[vs as usize..ve as usize];
                                tg_attrs.push(format!(
                                    " {}=\"{}\"",
                                    prop_name,
                                    escape_js_string(value)
                                ));
                            }
                        }
                        // Dynamic props are skipped for now (rarely used on TransitionGroup)
                    }

                    // Emit opening tag: `<ul class="list">`
                    self.buf.clear();
                    let _ = write!(self.buf, "<{}", tg_tag);
                    for attr in &tg_attrs {
                        self.buf.push_str(attr);
                    }
                    self.buf.push('>');

                    // Insert into push stream
                    if self.in_push {
                        out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                    } else {
                        let s = format!("_push(`{}`)\n", self.buf);
                        out.overwrite(el.tag_open.start, el.tag_open.end, &s);
                    }
                    self.elem_ctx.push(ElemCtx::TransitionGroupTag(tg_tag));
                    return super::WalkAction::Continue;
                }

                // Transition / KeepAlive / BaseTransition — transparent in SSR
                if matches!(
                    tag_name.as_str(),
                    "Transition"
                        | "transition"
                        | "KeepAlive"
                        | "keep-alive"
                        | "BaseTransition"
                        | "base-transition"
                ) {
                    // Remove the opening tag — children render directly.
                    out.overwrite(el.tag_open.start, el.tag_open.end, "");
                    self.elem_ctx.push(ElemCtx::TransparentBuiltin);
                    return super::WalkAction::Continue;
                }

                // Teleport — uses _ssrRenderTeleport helper
                if tag_name == "Teleport" || tag_name == "teleport" {
                    self.close_push(el.tag_open.start, out);
                    out.add_ssr_import(SsrHelper::RenderTeleport);

                    // Extract `to` and `disabled` props
                    let mut target = "\"body\"".to_string();
                    let mut disabled = "false".to_string();
                    let oxc = self.oxc_element(_id);
                    for (i, prop) in el.props.iter().enumerate() {
                        let prop_name = &source[prop.start as usize..prop.name_end as usize];

                        if prop.is_directive && (prop_name == ":" || prop_name == "v-bind") {
                            // Dynamic binding: :to or v-bind:to
                            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                let arg = &source[as_ as usize..ae as usize];
                                match arg {
                                    "to" => {
                                        if let (Some(vs), Some(ve)) =
                                            (prop.value_start, prop.value_end)
                                        {
                                            let expr = &source[vs as usize..ve as usize];
                                            let oxc_expr = oxc
                                                .and_then(|o| find_oxc_prop(o, i))
                                                .and_then(|p| p.exp.as_ref());
                                            target = self.resolve_expr(expr, vs, oxc_expr);
                                        }
                                    }
                                    "disabled" => {
                                        if let (Some(vs), Some(ve)) =
                                            (prop.value_start, prop.value_end)
                                        {
                                            let expr = &source[vs as usize..ve as usize];
                                            let oxc_expr = oxc
                                                .and_then(|o| find_oxc_prop(o, i))
                                                .and_then(|p| p.exp.as_ref());
                                            disabled = self.resolve_expr(expr, vs, oxc_expr);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else if !prop.is_directive {
                            // Static attribute: to="body" or disabled
                            match prop_name {
                                "to" => {
                                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end)
                                    {
                                        let value = &source[vs as usize..ve as usize];
                                        target = format!("\"{}\"", value);
                                    }
                                }
                                "disabled" => {
                                    disabled = "true".to_string();
                                }
                                _ => {}
                            }
                        }
                    }

                    // Store closing args for leave_element
                    self.teleport_closing_args
                        .push(format!(", {}, {}, _parent)", target, disabled));

                    self.buf.clear();
                    self.buf
                        .push_str("_ssrRenderTeleport(_push, (_push) => {\n");
                    out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                    self.in_push = false;
                    self.in_component_slots += 1;
                    self.elem_ctx.push(ElemCtx::TeleportBody);
                    return super::WalkAction::Continue;
                }
            }

            // ── 2c. Normal component ─────────────────────────────────
            self.close_push(el.tag_open.start, out);

            out.add_ssr_import(SsrHelper::RenderComponent);

            // Resolve component reference: check bindings first (like VDOM),
            // then fall back to _resolveComponent().
            // Vue's resolution order: exact → camelCase → PascalCase → _resolveComponent
            let component_ref = if self.resolver.get(&tag_name).is_some() {
                // Exact match (e.g. `MyComp` imported as `MyComp`)
                let prefix = self.resolver.resolve_prefix(&tag_name);
                let suffix = self.resolver.resolve_suffix(&tag_name);
                let raw = format!("{}{}{}", prefix, tag_name, suffix);
                // Vue SSR uses bracket notation for $setup component refs: $setup["MyComp"]
                setup_dot_to_bracket(&raw)
            } else {
                let camel = camelize(&tag_name);
                let pascal = to_pascal_case(&tag_name);
                if self.resolver.get(camel.as_ref()).is_some() {
                    // camelCase match (e.g. `<el-icon>` with `elIcon` imported)
                    let prefix = self.resolver.resolve_prefix(camel.as_ref());
                    let suffix = self.resolver.resolve_suffix(camel.as_ref());
                    let raw = format!("{}{}{}", prefix, camel, suffix);
                    setup_dot_to_bracket(&raw)
                } else if self.resolver.get(&pascal).is_some() {
                    // PascalCase match (e.g. `<my-comp>` with `MyComp` imported)
                    let prefix = self.resolver.resolve_prefix(&pascal);
                    let suffix = self.resolver.resolve_suffix(&pascal);
                    let raw = format!("{}{}{}", prefix, pascal, suffix);
                    setup_dot_to_bracket(&raw)
                } else if let Some((_flag, helper_name)) =
                    is_builtin_component(&tag_name).or_else(|| is_builtin_component(&pascal))
                {
                    // Built-in component (KeepAlive, Teleport, etc.)
                    out.add_builtin_component(_flag);
                    helper_name.to_string()
                } else {
                    // Fallback: _resolveComponent (deduplicate)
                    out.add_vdom_import(VdomHelper::ResolveComponent);
                    let var_name = tag_name.replace('-', "_");
                    let resolve_decl = format!(
                        "const _component_{} = _resolveComponent(\"{}\")",
                        var_name, tag_name
                    );
                    if !self.component_resolves.contains(&resolve_decl) {
                        self.component_resolves.push(resolve_decl);
                    }
                    format!("_component_{}", var_name)
                }
            };

            // Build component props
            // Pre-scan for class merge: static `class` + `:class` → merged array
            let mut comp_static_class: Option<String> = None;
            let mut comp_static_class_idx: Option<usize> = None;
            let mut comp_dynamic_class: Option<String> = None;
            let mut comp_dynamic_class_idx: Option<usize> = None;
            for (i, prop) in el.props.iter().enumerate() {
                let prop_name = &source[prop.start as usize..prop.name_end as usize];
                if !prop.is_directive && prop_name == "class" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        comp_static_class = Some(source[vs as usize..ve as usize].to_string());
                        comp_static_class_idx = Some(i);
                    }
                } else if prop.is_directive
                    && (prop_name.starts_with(':') || prop_name.starts_with("v-bind"))
                {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let arg_name = &source[as_ as usize..ae as usize];
                        if arg_name == "class" {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_expr = oxc
                                    .and_then(|o| find_oxc_prop(o, i))
                                    .and_then(|p| p.exp.as_ref());
                                comp_dynamic_class = Some(self.resolve_expr(expr, vs, oxc_expr));
                                comp_dynamic_class_idx = Some(i);
                            }
                        }
                    }
                }
            }
            let comp_merge_class = comp_static_class.is_some() && comp_dynamic_class.is_some();

            let mut props_parts: Vec<String> = Vec::new();
            let mut props_part_positions: Vec<u32> = Vec::new();
            let mut comp_directive_calls: Vec<String> = Vec::new();
            let mut comp_v_bind_spread: Option<String> = None;
            // Track props collected before the v-bind spread for position-aware merging.
            // Vue splits props into groups around v-bind spreads:
            //   <Comp :a="1" v-bind="obj" :b="2" /> → _mergeProps({a: 1}, obj, {b: 2})
            let mut props_before_spread: Option<Vec<String>> = None;
            for (i, prop) in el.props.iter().enumerate() {
                // Handle class merge: skip individual class entries, emit merged on first
                if comp_merge_class
                    && (Some(i) == comp_static_class_idx || Some(i) == comp_dynamic_class_idx)
                {
                    if Some(i) == comp_static_class_idx {
                        props_parts.push(format!(
                            "class: [\"{}\", {}]",
                            escape_js_string(comp_static_class.as_ref().unwrap()),
                            comp_dynamic_class.as_ref().unwrap()
                        ));
                        props_part_positions.push(prop.start);
                    }
                    continue;
                }

                let prop_name = &source[prop.start as usize..prop.name_end as usize];
                if prop.is_directive {
                    // Event handlers: @click → onClick, v-on:input → onInput
                    if prop_name.starts_with('@') || prop_name == "v-on" {
                        let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                            if after_at.is_empty() {
                                // @ shorthand with arg in arg_start/arg_end
                                match (prop.arg_start, prop.arg_end) {
                                    (Some(s), Some(e)) => &source[s as usize..e as usize],
                                    _ => {
                                        continue;
                                    }
                                }
                            } else {
                                after_at
                            }
                        } else {
                            // v-on with arg
                            match (prop.arg_start, prop.arg_end) {
                                (Some(s), Some(e)) => &source[s as usize..e as usize],
                                _ => {
                                    continue;
                                }
                            }
                        };
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);

                            // Wrap inline handlers (calls, assignments) in $event => (...)
                            let value = if is_inline_handler(expr) {
                                format!("$event => ({})", resolved)
                            } else {
                                resolved
                            };

                            let mut js_key = String::with_capacity(event_name.len() + 2);
                            format_event_handler_key_into(&mut js_key, event_name);
                            if needs_quoted_key(&js_key) {
                                props_parts.push(format!("\"{}\": {}", js_key, value));
                            } else {
                                props_parts.push(format!("{}: {}", js_key, value));
                            }
                            props_part_positions.push(prop.start);
                        }
                        continue;
                    }
                    // v-model on component: decompose into modelValue + onUpdate:modelValue
                    if prop_name.starts_with("v-model") {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);

                            // Determine model prop name: v-model:title → "title", v-model → "modelValue"
                            let model_prop =
                                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                    source[as_ as usize..ae as usize].to_string()
                                } else {
                                    "modelValue".to_string()
                                };

                            // Emit value prop: modelValue: <expr>
                            // Quote kebab-case model prop names (e.g., "page-size")
                            let model_key = if model_prop.contains('-') {
                                format!("\"{}\"", model_prop)
                            } else {
                                model_prop.clone()
                            };
                            props_parts.push(format!("{}: {}", model_key, resolved));
                            props_part_positions.push(prop.start);

                            // Emit update handler: "onUpdate:modelValue": $event => ((<expr>) = $event)
                            // Vue camelCases kebab-case model names in the handler key
                            let update_name = if model_prop.contains('-') {
                                kebab_to_camel(&model_prop)
                            } else {
                                model_prop.clone()
                            };
                            props_parts.push(format!(
                                "\"onUpdate:{}\": $event => (({}) = $event)",
                                update_name, resolved
                            ));
                            props_part_positions.push(prop.start);

                            // Emit modifiers if any: modelModifiers: { trim: true, ... }
                            if !prop.modifiers.is_empty() {
                                let mod_key = if model_prop == "modelValue" {
                                    "modelModifiers".to_string()
                                } else {
                                    format!("{}Modifiers", model_prop)
                                };
                                let mods: Vec<String> = prop
                                    .modifiers
                                    .iter()
                                    .map(|m| {
                                        let name = &source[m.start as usize..m.end as usize];
                                        format!("{}: true", name)
                                    })
                                    .collect();
                                props_parts.push(format!("{}: {{ {} }}", mod_key, mods.join(", ")));
                                props_part_positions.push(prop.start);
                            }
                        }
                        continue;
                    }
                    if prop_name.starts_with(':')
                        || (prop_name == "v-bind" && prop.arg_start.is_some())
                    {
                        if let (Some(as_), Some(ae), Some(vs), Some(ve)) = (
                            prop.arg_start,
                            prop.arg_end,
                            prop.value_start,
                            prop.value_end,
                        ) {
                            let attr = &source[as_ as usize..ae as usize];
                            // :key is passed through on components (unlike HTML elements)
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);
                            props_parts.push(format!(
                                "{}: {}",
                                html_attr_to_js_key(attr),
                                resolved
                            ));
                            props_part_positions.push(prop.start);
                        }
                        continue;
                    }
                    // v-bind spread (no argument): v-bind="obj" — merge into props
                    if (prop_name == "v-bind" || prop_name == ":") && prop.arg_start.is_none() {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_prop = oxc.and_then(|o| find_oxc_prop(o, i));
                            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                            let resolved = self.resolve_expr(expr, vs, oxc_expr);
                            comp_v_bind_spread = Some(resolved);
                            // Snapshot props collected so far as "before spread" group.
                            // Props added after this point go into the "after spread" group.
                            props_before_spread = Some(std::mem::take(&mut props_parts));
                            props_part_positions.clear();
                        }
                        continue;
                    }
                    // Custom directives on components: v-foo, v-tooltip, etc.
                    // Exclude built-in directives that are handled elsewhere or
                    // are structural (removed from el.props by the parser).
                    if prop_name.starts_with("v-")
                        && !matches!(
                            prop_name,
                            "v-bind" | "v-on" | "v-model" | "v-show" | "v-html" | "v-text"
                        )
                    {
                        if let Some(call) =
                            self.build_directive_props_call(prop, prop_name, source, oxc, i, out)
                        {
                            comp_directive_calls.push(call);
                        }
                    }
                } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let value = &source[vs as usize..ve as usize];
                    if prop_name == "style" {
                        // Vue SSR converts static style to JS object for component props
                        props_parts.push(format!("style: {}", css_to_js_object(value)));
                    } else {
                        props_parts.push(format!(
                            "{}: \"{}\"",
                            html_attr_to_js_key(prop_name),
                            escape_js_string(value)
                        ));
                    }
                    props_part_positions.push(prop.start);
                } else {
                    // Boolean attribute with no value: <Comp rounded /> → rounded: ""
                    props_parts.push(format!("{}: \"\"", html_attr_to_js_key(prop_name)));
                    props_part_positions.push(prop.start);
                }
            }

            // Add ref to component props in source order (cached in el.v_ref, removed from el.props)
            // Note: unlike HTML elements, components DO get ref in SSR props
            if let Some(ref v_ref) = el.v_ref {
                if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
                    let ref_val = &source[vs as usize..ve as usize];
                    let ref_str = if v_ref.is_directive {
                        let resolved = self.resolve_expr(ref_val, vs, None);
                        format!("ref: {}", resolved)
                    } else {
                        format!("ref: \"{}\"", escape_js_string(ref_val))
                    };
                    let insert_idx = props_part_positions.partition_point(|&pos| pos < v_ref.start);
                    props_parts.insert(insert_idx, ref_str);
                }
            }

            // Merge duplicate event handler keys into arrays.
            // When v-model and an explicit @update:model-value handler coexist,
            // Vue merges them into an array: "onUpdate:modelValue": [handler1, handler2].
            merge_duplicate_event_handlers(&mut props_parts);

            // Also merge in the before-spread group if applicable
            if let Some(ref mut before) = props_before_spread {
                merge_duplicate_event_handlers(before);
            }

            // Build the props expression, handling combinations of:
            // - static props (props_parts)
            // - v-bind spread (comp_v_bind_spread)
            // - custom directives (comp_directive_calls)
            // - root element _attrs merge
            let has_spread = comp_v_bind_spread.is_some();
            let has_directives = !comp_directive_calls.is_empty();

            let props_expr = if has_spread || has_directives {
                // Multiple sources to merge — use _mergeProps.
                // Vue preserves the position of v-bind spreads relative to other props:
                //   <Comp :a="1" v-bind="obj" :b="2" /> → _mergeProps({a:1}, obj, {b:2})
                // The `props_before_spread` snapshot contains props before the spread,
                // `props_parts` contains props after the spread.
                let mut merge_args: Vec<String> = Vec::new();
                if let Some(ref before) = props_before_spread {
                    if !before.is_empty() {
                        merge_args.push(format!("{{ {} }}", before.join(", ")));
                    }
                }
                if let Some(spread) = &comp_v_bind_spread {
                    merge_args.push(spread.clone());
                }
                // Props after the spread (or all props if no spread was present)
                if !props_parts.is_empty() {
                    merge_args.push(format!("{{ {} }}", props_parts.join(", ")));
                }
                if is_root {
                    merge_args.push("_attrs".to_string());
                }
                for call in &comp_directive_calls {
                    merge_args.push(call.clone());
                }
                // If only a single source (spread-only, no root, no directives),
                // use it directly without _mergeProps wrapping
                if merge_args.len() == 1 && !is_root && !has_directives {
                    merge_args.into_iter().next().unwrap()
                } else {
                    out.add_vdom_import(VdomHelper::MergeProps);
                    format!("_mergeProps({})", merge_args.join(", "))
                }
            } else if is_root {
                // Root component: merge static props with _attrs
                if props_parts.is_empty() {
                    "_attrs".to_string()
                } else {
                    out.add_vdom_import(VdomHelper::MergeProps);
                    format!("_mergeProps({{ {} }}, _attrs)", props_parts.join(", "))
                }
            } else if props_parts.is_empty() {
                "null".to_string()
            } else {
                format!("{{ {} }}", props_parts.join(", "))
            };

            let has_children = self.has_effective_children(el, source);

            if !has_children {
                self.buf.clear();
                let sid_arg = self.scope_id_arg();
                let _ = writeln!(
                    self.buf,
                    "_push(_ssrRenderComponent({}, {}, null, _parent{}))",
                    component_ref, props_expr, sid_arg
                );
                let el_end = self.el_end(el);
                out.overwrite(el.tag_open.start, el_end, &self.buf);
                self.elem_ctx.push(ElemCtx::Complete);
            } else {
                // Has children — emit slot wrappers via _withCtx.
                out.add_vdom_import(VdomHelper::WithCtx);
                let has_named_slots = self.has_template_slot_children(el);

                // Save outer default_slot_open state before entering this component's
                // slot scope. Nested components must start with a fresh state.
                self.saved_default_slot_open.push(self.default_slot_open);
                self.saved_default_slot_move.push((
                    self.default_slot_move_start.take(),
                    self.default_slot_move_end.take(),
                ));

                self.buf.clear();
                let _ = write!(
                    self.buf,
                    "_push(_ssrRenderComponent({}, {}, {{",
                    component_ref, props_expr
                );

                if !has_named_slots {
                    // Extract v-slot params from the component itself (e.g., <Comp v-slot="{ item }">)
                    let params = if let Some(ref v_slot) = el.v_slot {
                        if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                            let p = source[vs as usize..ve as usize].trim();
                            if p.is_empty() {
                                "_"
                            } else {
                                p
                            }
                        } else {
                            "_"
                        }
                    } else {
                        "_"
                    };
                    let _ = write!(
                        self.buf,
                        "\ndefault: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                        params
                    );
                    // Mark default slot as already opened so the implicit
                    // default-slot check doesn't open a second one.
                    self.default_slot_open = true;
                    // Track scoped slot depth for child component DYNAMIC flag
                    let is_scoped = params != "_";
                    if is_scoped {
                        self.v_slot_scope_depth += 1;
                    }
                    self.scoped_slot_entered.push(is_scoped);
                } else {
                    // Named slots present — reset for nested scope; children will
                    // open the default slot implicitly when needed.
                    self.default_slot_open = false;
                    self.comp_children_start = Some(el.tag_open.end);
                    self.scoped_slot_entered.push(false);
                }

                out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                self.in_push = false;
                self.in_component_slots += 1;
                self.elem_ctx.push(ElemCtx::ComponentWithSlots);
            }
            return super::WalkAction::Continue;
        }

        // ── 2b. <template v-slot> inside a component ─────────────

        if el.tag_type == TagType::Template && el.v_slot.is_some() && self.in_component_slots > 0 {
            // Check if we're inside a Suspense FIRST — Suspense slots use simple
            // arrow functions, not _withCtx with VDOM fallback. This must happen
            // before the default_slot_open check, because when Suspense is nested
            // inside a ComponentWithSlots, `default_slot_open` refers to the OUTER
            // component's slot, not the Suspense's implicit default.
            let in_suspense = self
                .elem_ctx
                .iter()
                .rev()
                .any(|ctx| matches!(ctx, ElemCtx::SuspenseSlots));

            // If a default slot was opened for preceding non-template children,
            // close it before starting the named slot.
            // Skip this when inside Suspense — the outer component's default slot
            // is NOT being closed here; the Suspense implicit default is handled below.
            if !in_suspense && self.default_slot_open {
                self.close_push(el.tag_open.start, out);
                // Walk up to parent component to generate VDOM fallback for default children
                let parent_node = &self.ast.nodes[_id.0];
                let vdom_fallback = if let Some(parent_id) = parent_node.parent {
                    if let AstNodeKind::Element(ref parent_el) = self.ast.nodes[parent_id.0].kind {
                        self.generate_vdom_fallback_default(parent_el, source, out)
                    } else {
                        "[]".to_string()
                    }
                } else {
                    "[]".to_string()
                };
                self.buf.clear();
                let _ = write!(self.buf, "}} else {{\nreturn {}\n}}\n}}),", vdom_fallback);
                out.prepend_alloc(el.tag_open.start, &self.buf);
                self.default_slot_open = false;
                // Record end for reordering: +1 to include the Inserted chunk at this pos
                self.default_slot_move_end = Some(el.tag_open.start + 1);
            }

            let slot_name = if let Some(ref v_slot) = el.v_slot {
                Self::build_slot_name(v_slot, source)
            } else {
                "default".to_string()
            };

            let params = if let Some(ref v_slot) = el.v_slot {
                if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                    let p = source[vs as usize..ve as usize].trim();
                    if p.is_empty() {
                        "_"
                    } else {
                        p
                    }
                } else {
                    "_"
                }
            } else {
                "_"
            };

            self.buf.clear();
            if in_suspense {
                // If there's an open implicit default slot (mixed content Suspense),
                // close it before emitting the named slot entry.
                if self.suspense_implicit_default_open {
                    self.close_push(el.tag_open.start, out);
                    self.buf.push_str("},\n");
                    self.suspense_implicit_default_open = false;
                }
                let _ = write!(self.buf, "\n{}: () => {{\n", slot_name);
                out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                self.in_push = false;
                self.elem_ctx.push(ElemCtx::SuspenseSlotTemplate);
            } else {
                let _ = write!(
                    self.buf,
                    "\n{}: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                    slot_name, params
                );
                out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                self.in_push = false;
                // Track scoped slot depth for child component DYNAMIC flag
                let is_scoped = params != "_";
                if is_scoped {
                    self.v_slot_scope_depth += 1;
                }
                self.scoped_slot_entered.push(is_scoped);
                self.elem_ctx.push(ElemCtx::SlotTemplate);
                self.depth += 1;
            }
            return super::WalkAction::Continue;
        }

        // ── 2c. <slot> outlet rendering ──────────────────────────

        if el.tag_type == TagType::SlotOutlet {
            self.close_push(el.tag_open.start, out);
            out.add_ssr_import(SsrHelper::RenderSlot);

            let (slot_name, is_dynamic_name) = self.extract_slot_outlet_name(el, oxc, source);
            let (props, needs_merge_props) = self.build_slot_outlet_props(el, oxc, source);
            if needs_merge_props {
                out.add_vdom_import(VdomHelper::MergeProps);
            }

            let has_children = el.content.as_ref().is_some_and(|c| !c.children.is_empty());

            // Format slot name: quoted for static, expression for dynamic
            let name_arg = if is_dynamic_name {
                slot_name.clone()
            } else {
                format!("\"{}\"", slot_name)
            };

            let sid_arg = self.scope_id_arg();
            self.buf.clear();
            if has_children {
                let _ = writeln!(
                    self.buf,
                    "_ssrRenderSlot(_ctx.$slots, {}, {}, () => {{",
                    name_arg, props
                );
                out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
                self.in_push = false;
                self.elem_ctx.push(ElemCtx::SlotOutletFallback);
                self.depth += 1;
            } else {
                let _ = writeln!(
                    self.buf,
                    "_ssrRenderSlot(_ctx.$slots, {}, {}, null, _push, _parent{})",
                    name_arg, props, sid_arg
                );
                let el_end = self.el_end(el);
                out.overwrite(el.tag_open.start, el_end, &self.buf);
                self.elem_ctx.push(ElemCtx::Complete);
            }
            return super::WalkAction::Continue;
        }

        // ── 3. Normal element ───────────────────────────────────

        // ── 3. <template> elements (non-slot) → transparent fragment ──
        // Vue SSR renders <template v-if/v-for> as fragment markers <!--[-->...<!--]-->
        // rather than <template>...</template> tags.
        // Exception: <template v-if> with exactly one element child (no text/interp)
        // is transparent without fragment markers (matching Vue's SSR behavior).
        // Templates with v-for always need fragment markers for each iteration boundary.
        if el.tag_type == TagType::Template {
            let skip_frag = el.v_for.is_none() && self.has_single_element_child(el, source);
            self.ensure_push(el.tag_open.start, out);
            if skip_frag {
                out.overwrite(el.tag_open.start, el.tag_open.end, "");
            } else {
                out.overwrite(el.tag_open.start, el.tag_open.end, "<!--[-->");
            }
            self.depth += 1;
            self.elem_ctx.push(ElemCtx::InParentPush);
            return super::WalkAction::Continue;
        }

        let is_void = is_void_tag(tag_name.as_bytes());
        let attrs_str = self.build_attrs_string(el, oxc, source, out, is_root);

        let sid = self.scope_id_suffix();

        // v-html: entire element is one _push
        if let Some(html_expr) = self.get_v_html_expr(el, oxc, source) {
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let _ = write!(self.buf, "${{({}) ?? ''}}", html_expr);
            if !is_void {
                let _ = write!(self.buf, "</{}>", tag_name);
            }
            let el_end = self.el_end(el);
            out.overwrite(el.tag_open.start, el_end, &self.buf);
            self.elem_ctx.push(ElemCtx::Complete);
            return super::WalkAction::Continue;
        }

        // v-text: entire element is one _push
        if let Some(text_expr) = self.get_v_text_expr(el, oxc, source) {
            out.add_ssr_import(SsrHelper::Interpolate);
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let _ = write!(self.buf, "${{_ssrInterpolate({})}}", text_expr);
            if !is_void {
                let _ = write!(self.buf, "</{}>", tag_name);
            }
            let el_end = self.el_end(el);
            out.overwrite(el.tag_open.start, el_end, &self.buf);
            self.elem_ctx.push(ElemCtx::Complete);
            return super::WalkAction::Continue;
        }

        // Textarea with v-model or :value: content handling.
        if tag_name == "textarea" {
            // Check if the element has a real v-model (not just :value)
            let has_real_v_model = el.props.iter().any(|p| {
                if !p.is_directive {
                    return false;
                }
                let name = &source[p.start as usize..p.name_end as usize];
                name.starts_with("v-model")
            });

            if let Some(model_expr) = self.get_v_model_expr(el, oxc, source) {
                if has_real_v_model {
                    // v-model on textarea: always render content via _ssrInterpolate
                    // (both root and non-root — value is NOT in attrs).
                    self.ensure_push(el.tag_open.start, out);
                    self.buf.clear();
                    let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
                    out.add_ssr_import(SsrHelper::Interpolate);
                    let _ = write!(self.buf, "${{_ssrInterpolate({})}}", model_expr);
                    let _ = write!(self.buf, "</{}>", tag_name);
                    let el_end = self.el_end(el);
                    out.overwrite(el.tag_open.start, el_end, &self.buf);
                    self.elem_ctx.push(ElemCtx::Complete);
                    return super::WalkAction::Continue;
                } else {
                    // :value on textarea: for root path, value is in attrs (via _ssrRenderAttrs).
                    // For non-root, content is interpolated.
                    let value_in_attrs = attrs_str.contains("_ssrRenderAttrs");
                    self.ensure_push(el.tag_open.start, out);
                    self.buf.clear();
                    let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
                    if !value_in_attrs {
                        out.add_ssr_import(SsrHelper::Interpolate);
                        let _ = write!(self.buf, "${{_ssrInterpolate({})}}", model_expr);
                    }
                    let _ = write!(self.buf, "</{}>", tag_name);
                    let el_end = self.el_end(el);
                    out.overwrite(el.tag_open.start, el_end, &self.buf);
                    self.elem_ctx.push(ElemCtx::Complete);
                    return super::WalkAction::Continue;
                }
            }
        }

        // Void element
        if is_void {
            self.ensure_push(el.tag_open.start, out);
            self.buf.clear();
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            let el_end = self.el_end(el);
            out.overwrite(el.tag_open.start, el_end, &self.buf);
            self.elem_ctx.push(ElemCtx::Complete);
            return super::WalkAction::Continue;
        }

        // Non-void element with children.
        self.buf.clear();
        if !self.in_push {
            // Not inside a push — open a new push (for root or standalone)
            let _ = write!(self.buf, "_push(`<{}{}{}>", tag_name, attrs_str, sid);
            self.in_push = true;
            self.elem_ctx.push(ElemCtx::OwnPush);
        } else {
            // Already in a push (nested element, or root after preceding comment).
            // attrs_str still has _ssrRenderAttrs(_attrs) when is_root.
            let _ = write!(self.buf, "<{}{}{}>", tag_name, attrs_str, sid);
            self.elem_ctx.push(ElemCtx::InParentPush);
        }
        out.overwrite(el.tag_open.start, el.tag_open.end, &self.buf);
        self.depth += 1;
        super::WalkAction::Continue
    }

    fn leave_element(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        _oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Clear select v-model tracking when leaving a <select>
        if self.select_v_model_expr.is_some() {
            let tn = self.tag_name(el, source);
            if tn == "select" {
                self.select_v_model_expr = None;
            }
        }

        let ctx = self.elem_ctx.pop().unwrap_or(ElemCtx::Complete);

        match ctx {
            ElemCtx::Complete => {
                // Fully handled in enter_element — nothing to do
            }
            ElemCtx::InParentPush => {
                self.depth -= 1;
                let tag_name = self.tag_name(el, source);
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(el.tag_open.end);
                self.ensure_push(close_pos, out);

                self.buf.clear();
                if el.tag_type == TagType::Template {
                    // <template> → fragment close marker unless single-element child (no v-for)
                    let skip_frag = el.v_for.is_none() && self.has_single_element_child(el, source);
                    if !skip_frag {
                        self.buf.push_str("<!--]-->");
                    }
                } else {
                    let _ = write!(self.buf, "</{}>", tag_name);
                }
                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    out.prepend_alloc(el.tag_open.end, &self.buf);
                }
            }
            ElemCtx::OwnPush => {
                self.depth -= 1;
                let tag_name = self.tag_name(el, source);
                let is_now_root = self.depth == 0 && self.in_component_slots == 0;

                if self.in_push {
                    self.buf.clear();
                    let _ = write!(self.buf, "</{}>", tag_name);

                    if is_now_root && !self.is_multi_root {
                        // Single-root: close the push after the closing tag.
                        // Multi-root: leave the push open so leave_template
                        // can merge the fragment close marker <!--]--> into it.
                        self.buf.push_str("`)\n");
                        self.in_push = false;
                    }

                    if let Some(ref tc) = el.tag_close {
                        out.overwrite(tc.start, tc.end, &self.buf);
                    } else {
                        out.prepend_alloc(el.tag_open.end, &self.buf);
                    }
                } else {
                    // Push was closed by a child (e.g., v-if/v-for).
                    // For non-root elements, re-open the push for the closing tag.
                    if is_now_root && !self.is_multi_root {
                        self.buf.clear();
                        let _ = write!(self.buf, "_push(`</{}>", tag_name);
                        self.buf.push_str("`)\n");
                    } else if is_now_root && self.is_multi_root {
                        // Multi-root: re-open push for the closing tag but
                        // leave it open so leave_template merges <!--]-->.
                        let close_pos = el
                            .tag_close
                            .as_ref()
                            .map(|tc| tc.start)
                            .unwrap_or(el.tag_open.end);
                        self.ensure_push(close_pos, out);
                        self.buf.clear();
                        let _ = write!(self.buf, "</{}>", tag_name);
                    } else {
                        let close_pos = el
                            .tag_close
                            .as_ref()
                            .map(|tc| tc.start)
                            .unwrap_or(el.tag_open.end);
                        self.ensure_push(close_pos, out);
                        self.buf.clear();
                        let _ = write!(self.buf, "</{}>", tag_name);
                    }

                    if let Some(ref tc) = el.tag_close {
                        out.overwrite(tc.start, tc.end, &self.buf);
                    } else {
                        out.prepend_alloc(el.tag_open.end, &self.buf);
                    }
                }
            }
            ElemCtx::ComponentWithSlots => {
                // Close the slots object and _ssrRenderComponent call
                self.in_component_slots -= 1;
                // Restore scoped slot depth for SSR push path slot flag.
                let was_scoped = self.scoped_slot_entered.pop() == Some(true);
                if was_scoped {
                    self.v_slot_scope_depth -= 1;
                }
                let has_named_slots = self.has_template_slot_children(el);

                // Determine slot stability:
                // _: 2 /* DYNAMIC */ when slots have v-if/v-for/dynamic names
                // _: 3 /* FORWARDED */ when slot content contains <slot> outlets
                // _: 1 /* STABLE */ otherwise
                // SSR push path: slots are DYNAMIC when inside a scoped slot context
                // (v_slot_scope_depth > 0), even if the slots themselves are static.
                // This differs from the VDOM fallback where static content is always STABLE.
                let slot_flag = if self.has_dynamic_slots(el) || self.v_slot_scope_depth > 0 {
                    "_: 2 /* DYNAMIC */"
                } else if self.has_slot_outlet_in_descendants(el, source) {
                    "_: 3 /* FORWARDED */"
                } else {
                    "_: 1 /* STABLE */"
                };

                // Close push BEFORE the closing tag so the backtick ends before
                // the slot closure code that overwrites the closing tag.
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let sid_arg = self.scope_id_arg();
                self.buf.clear();
                if !has_named_slots {
                    // Close default slot: VDOM fallback + _withCtx + slots obj + _ssrRenderComponent
                    let vdom_fallback = self.generate_vdom_fallback(el, source, out);
                    let _ = write!(
                        self.buf,
                        "}} else {{\nreturn {}\n}}\n}}),\n{}\n}}, _parent{}))\n",
                        vdom_fallback, slot_flag, sid_arg
                    );
                } else if self.default_slot_open {
                    // Close the implicit default slot, then slots object + _ssrRenderComponent
                    let vdom_fallback = self.generate_vdom_fallback_default(el, source, out);
                    let _ = write!(
                        self.buf,
                        "}} else {{\nreturn {}\n}}\n}}),\n{}\n}}, _parent{}))\n",
                        vdom_fallback, slot_flag, sid_arg
                    );
                    self.default_slot_open = false;
                } else {
                    // Close slots object + _ssrRenderComponent (named slots only, no default)
                    let _ = write!(self.buf, "\n{}\n}}, _parent{}))\n", slot_flag, sid_arg);
                }

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
                // Reorder: move default slot after named slots if it was opened first
                if let (Some(move_start), Some(move_end)) = (
                    self.default_slot_move_start.take(),
                    self.default_slot_move_end.take(),
                ) {
                    out.move_slice(move_start, move_end, close_pos);
                }
                // Restore outer default_slot_open state
                self.default_slot_open = self.saved_default_slot_open.pop().unwrap_or(false);
                if let Some((ms, me)) = self.saved_default_slot_move.pop() {
                    self.default_slot_move_start = ms;
                    self.default_slot_move_end = me;
                }
            }
            ElemCtx::DynamicComponentWithSlots => {
                // Close the slots object and _createVNode + _ssrRenderVNode call
                self.in_component_slots -= 1;
                // Restore scoped slot depth for SSR push path slot flag.
                let was_scoped = self.scoped_slot_entered.pop() == Some(true);
                if was_scoped {
                    self.v_slot_scope_depth -= 1;
                }
                let has_named_slots = self.has_template_slot_children(el);

                // SSR push path: slots are DYNAMIC when inside a scoped slot context
                let slot_flag = if self.has_dynamic_slots(el) || self.v_slot_scope_depth > 0 {
                    "_: 2 /* DYNAMIC */"
                } else if self.has_slot_outlet_in_descendants(el, source) {
                    "_: 3 /* FORWARDED */"
                } else {
                    "_: 1 /* STABLE */"
                };

                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let sid_arg = self.scope_id_arg();
                self.buf.clear();
                if !has_named_slots {
                    let vdom_fallback = self.generate_vdom_fallback(el, source, out);
                    let _ = write!(
                        self.buf,
                        "}} else {{\nreturn {}\n}}\n}}),\n{}\n}}), _parent{})\n",
                        vdom_fallback, slot_flag, sid_arg
                    );
                } else if self.default_slot_open {
                    let vdom_fallback = self.generate_vdom_fallback_default(el, source, out);
                    let _ = write!(
                        self.buf,
                        "}} else {{\nreturn {}\n}}\n}}),\n{}\n}}), _parent{})\n",
                        vdom_fallback, slot_flag, sid_arg
                    );
                    self.default_slot_open = false;
                } else {
                    let _ = write!(self.buf, "\n{}\n}}), _parent{})\n", slot_flag, sid_arg);
                }

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
                // Reorder: move default slot after named slots if it was opened first
                if let (Some(move_start), Some(move_end)) = (
                    self.default_slot_move_start.take(),
                    self.default_slot_move_end.take(),
                ) {
                    out.move_slice(move_start, move_end, close_pos);
                }
                self.default_slot_open = self.saved_default_slot_open.pop().unwrap_or(false);
                if let Some((ms, me)) = self.saved_default_slot_move.pop() {
                    self.default_slot_move_start = ms;
                    self.default_slot_move_end = me;
                }
            }
            ElemCtx::SlotTemplate => {
                // Close the named slot's _withCtx wrapper
                self.depth -= 1;
                // Restore scoped slot depth for SSR push path.
                let was_scoped = self.scoped_slot_entered.pop() == Some(true);
                if was_scoped {
                    self.v_slot_scope_depth -= 1;
                }
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let vdom_fallback = self.generate_vdom_fallback(el, source, out);
                self.buf.clear();
                let _ = write!(self.buf, "}} else {{\nreturn {}\n}}\n}}),", vdom_fallback);

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
            }
            ElemCtx::SuspenseSlotTemplate => {
                // Close a Suspense slot's simple arrow function: `},`
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                self.buf.clear();
                self.buf.push_str("},");

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
            }
            ElemCtx::SlotOutletFallback => {
                // Close the fallback function of _ssrRenderSlot
                self.depth -= 1;
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let sid_arg = self.scope_id_arg();
                self.buf.clear();
                let _ = writeln!(self.buf, "}}, _push, _parent{})", sid_arg);

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
            }
            ElemCtx::SuspenseSlots => {
                // Close the _ssrRenderSuspense slots object
                self.in_component_slots -= 1;
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let has_named_slots = self.has_template_slot_children(el);
                self.buf.clear();
                if has_named_slots {
                    // Named slots already closed by SuspenseSlotTemplate leave handlers
                    self.buf.push_str("\n_: 1 /* STABLE */\n})\n");
                } else {
                    // Implicit default: close the arrow function body + object + call
                    self.buf.push_str("},\n_: 1 /* STABLE */\n})\n");
                }

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
            }
            ElemCtx::TransparentBuiltin => {
                // Remove closing tag — children already rendered directly.
                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, "");
                }
            }
            ElemCtx::TransitionGroupTag(ref tg_tag) => {
                // Emit closing tag for TransitionGroup: `</ul>`
                let closing = format!("</{}>", tg_tag);
                if let Some(ref tc) = el.tag_close {
                    if self.in_push {
                        out.overwrite(tc.start, tc.end, &closing);
                    } else {
                        let s = format!("_push(`{}`)\n", closing);
                        out.overwrite(tc.start, tc.end, &s);
                    }
                }
            }
            ElemCtx::TeleportBody => {
                // Close the _ssrRenderTeleport callback and append target/disabled args.
                self.in_component_slots -= 1;
                let close_pos = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.start)
                    .unwrap_or(self.el_end(el));
                self.close_push(close_pos, out);

                let closing_args = self
                    .teleport_closing_args
                    .pop()
                    .unwrap_or_else(|| ", \"body\", false, _parent)".to_string());

                self.buf.clear();
                let _ = writeln!(self.buf, "}}{}", closing_args);

                if let Some(ref tc) = el.tag_close {
                    out.overwrite(tc.start, tc.end, &self.buf);
                } else {
                    let el_end = self.el_end(el);
                    out.prepend_alloc(el_end, &self.buf);
                }
            }
        }

        // Close v-for
        if el.v_for.is_some() {
            self.v_for_depth.set(self.v_for_depth.get() - 1);
            let el_end = self.el_end(el);
            self.close_push(el_end, out);
            out.prepend_alloc(el_end, "})\n_push(`<!--]-->");
            self.in_push = true;
        }

        // Close v-if/v-else-if/v-else
        if let Some(ref cond) = el.v_condition {
            let el_end = self.el_end(el);
            self.close_push(el_end, out);

            match cond.kind {
                ElementNodeConditionKind::If => {
                    let has_continuation = self.has_condition_continuation(el);
                    if !has_continuation {
                        out.prepend_alloc(el_end, "} else {\n_push(`<!---->`)\n}\n");
                    } else {
                        out.prepend_alloc(el_end, "\n");
                    }
                }
                ElementNodeConditionKind::ElseIf => {
                    let has_continuation = self.has_condition_continuation(el);
                    if !has_continuation {
                        out.prepend_alloc(el_end, "} else {\n_push(`<!---->`)\n}\n");
                    } else {
                        out.prepend_alloc(el_end, "\n");
                    }
                }
                ElementNodeConditionKind::Else => {
                    out.prepend_alloc(el_end, "}\n");
                }
            }
        }
    }

    fn visit_text(
        &mut self,
        id: NodeId,
        text: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let content = &source[text.start as usize..text.end as usize];

        // Implicit default slot wrapping for text children of component with slots
        if matches!(
            self.elem_ctx.last(),
            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
        ) && !self.default_slot_open
            && !content.trim().is_empty()
        {
            self.open_default_slot(text.start, out);
        }

        let is_ws_only = content.chars().all(|c| c.is_ascii_whitespace());

        if is_ws_only {
            // Vue's whitespace condensation rules for whitespace-only text:
            // Remove entirely when:
            //   1. No previous OR no next sibling (boundary of parent)
            //   2. Between two comments
            //   3. Between a comment and an element (either direction)
            //   4. Between two elements AND text contains \r or \n
            // Otherwise: condense to a single space.
            if self.should_remove_whitespace_node(id, content) {
                out.overwrite(text.start, text.end, "");
            } else {
                self.ensure_push(text.start, out);
                out.overwrite(text.start, text.end, " ");
            }
        } else {
            self.ensure_push(text.start, out);
            // Vue condenses consecutive whitespace (\n, \t, spaces) to a single space
            // within text nodes that contain non-whitespace content.
            let condensed = condense_whitespace(content);
            // Vue decodes all HTML entities then re-encodes HTML special chars.
            // Apply full SSR text escaping: decode entities → escape HTML → escape template literal.
            let escaped = escape_ssr_text(&condensed);
            if escaped != content {
                out.overwrite(text.start, text.end, &escaped);
            }
        }
    }

    fn visit_interpolation(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Implicit default slot wrapping for interpolation children of component with slots
        if matches!(
            self.elem_ctx.last(),
            Some(&ElemCtx::ComponentWithSlots) | Some(&ElemCtx::DynamicComponentWithSlots)
        ) && !self.default_slot_open
        {
            self.open_default_slot(interp.start, out);
        }

        out.add_ssr_import(SsrHelper::Interpolate);

        let expr = &source[interp.inner_start as usize..interp.inner_end as usize];
        let resolved = self.resolve_expr(expr, interp.inner_start, Some(oxc));

        self.buf.clear();
        let _ = write!(self.buf, "${{_ssrInterpolate({})}}", resolved);

        // Use ensure_push to coalesce with adjacent nodes
        self.ensure_push(interp.start, out);
        out.overwrite(interp.start, interp.end, &self.buf);
    }

    fn visit_comment(
        &mut self,
        _id: NodeId,
        comment: &CommentNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        if !self.options.comments {
            out.overwrite(comment.start, comment.end, "");
            return;
        }

        let content = &source[comment.start as usize..comment.end as usize];
        // Use ensure_push to coalesce with adjacent nodes
        self.ensure_push(comment.start, out);
        let escaped = escape_template_literal(content);
        if escaped != content {
            out.overwrite(comment.start, comment.end, &escaped);
        }
    }
}

// ======================== OXC lookup helper ========================

/// Find the OXC parsed prop data for a given element prop index.
fn find_oxc_prop<'a, 'alloc>(
    oxc: &'a OxcParsedElement<'alloc>,
    prop_index: usize,
) -> Option<&'a crate::template::oxc::types::OxcParsedProp<'alloc>> {
    oxc.props.iter().find(|p| p.prop_index == prop_index)
}

// ======================== Utility functions ========================

/// Convert `$setup.Foo` to `$setup["Foo"]` for SSR component references.
/// Vue's SSR compiler uses bracket notation for $setup component references.
/// Non-$setup references (e.g. `_ctx.Foo`, `_component_Foo`) pass through unchanged.
fn setup_dot_to_bracket(s: &str) -> String {
    if let Some(name) = s.strip_prefix("$setup.") {
        format!("$setup[\"{}\"]", name)
    } else {
        s.to_string()
    }
}

/// Convert a directive name (after `v-` prefix) to a camelCase binding name.
///
/// Vue convention: `v-focus` → `vFocus`, `v-click-outside` → `vClickOutside`.
/// Strips `v-` prefix, capitalizes first letter of each `-`-separated segment,
/// and prepends `v`.
fn directive_to_camel(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 1);
    result.push('v');
    let mut capitalize_next = true;
    for ch in name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            for c in ch.to_uppercase() {
                result.push(c);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert a kebab-case string to camelCase.
/// `"page-size"` → `"pageSize"`, `"target-keys"` → `"targetKeys"`
fn kebab_to_camel(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut capitalize_next = false;
    for ch in name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            for c in ch.to_uppercase() {
                result.push(c);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Escape a string for use inside a JavaScript template literal.
/// Decode HTML entities in text content to their Unicode characters.
///
/// Handles the 5 XML entities, common named entities, and numeric/hex entities.
/// Unknown named entities are kept as-is.
fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '&' {
            result.push(ch);
            continue;
        }
        // Collect entity: &...;
        let mut entity = String::new();
        let mut found_semi = false;
        while let Some(&next) = chars.peek() {
            if next == ';' {
                chars.next();
                found_semi = true;
                break;
            }
            if next.is_alphanumeric() || next == '#' {
                entity.push(next);
                chars.next();
            } else {
                break;
            }
            if entity.len() > 10 {
                break;
            }
        }
        if !found_semi || entity.is_empty() {
            result.push('&');
            result.push_str(&entity);
            continue;
        }
        // Decode the entity
        match entity.as_str() {
            // XML5 entities
            "amp" => result.push('&'),
            "lt" => result.push('<'),
            "gt" => result.push('>'),
            "quot" => result.push('"'),
            "apos" => result.push('\''),
            // Common named entities
            "copy" => result.push('\u{00A9}'),
            "reg" => result.push('\u{00AE}'),
            "trade" => result.push('\u{2122}'),
            "nbsp" => result.push('\u{00A0}'),
            "mdash" => result.push('\u{2014}'),
            "ndash" => result.push('\u{2013}'),
            "laquo" => result.push('\u{00AB}'),
            "raquo" => result.push('\u{00BB}'),
            "hellip" => result.push('\u{2026}'),
            "bull" => result.push('\u{2022}'),
            "times" => result.push('\u{00D7}'),
            "divide" => result.push('\u{00F7}'),
            "euro" => result.push('\u{20AC}'),
            "pound" => result.push('\u{00A3}'),
            "yen" => result.push('\u{00A5}'),
            "cent" => result.push('\u{00A2}'),
            "lsquo" => result.push('\u{2018}'),
            "rsquo" => result.push('\u{2019}'),
            "ldquo" => result.push('\u{201C}'),
            "rdquo" => result.push('\u{201D}'),
            "shy" => result.push('\u{00AD}'),
            "macr" => result.push('\u{00AF}'),
            "deg" => result.push('\u{00B0}'),
            "plusmn" => result.push('\u{00B1}'),
            "micro" => result.push('\u{00B5}'),
            "para" => result.push('\u{00B6}'),
            "middot" => result.push('\u{00B7}'),
            "frac14" => result.push('\u{00BC}'),
            "frac12" => result.push('\u{00BD}'),
            "frac34" => result.push('\u{00BE}'),
            "iquest" => result.push('\u{00BF}'),
            "larr" => result.push('\u{2190}'),
            "uarr" => result.push('\u{2191}'),
            "rarr" => result.push('\u{2192}'),
            "darr" => result.push('\u{2193}'),
            other => {
                // Numeric entity: &#123; or &#x1F;
                if let Some(rest) = other.strip_prefix('#') {
                    if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
                        if let Ok(code) = u32::from_str_radix(hex, 16) {
                            if let Some(c) = char::from_u32(code) {
                                result.push(c);
                                continue;
                            }
                        }
                    } else if let Ok(code) = rest.parse::<u32>() {
                        if let Some(c) = char::from_u32(code) {
                            result.push(c);
                            continue;
                        }
                    }
                }
                // Unknown entity — keep as-is
                result.push('&');
                result.push_str(other);
                result.push(';');
            }
        }
    }
    result
}

/// Check if an event handler expression is an "inline handler" that needs
/// `$event => (...)` wrapping. Method references (identifiers, member expressions)
/// and function expressions don't need wrapping.
fn is_inline_handler(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Already a function expression — no wrapping needed
    if trimmed.starts_with("function") || trimmed.contains("=>") {
        return false;
    }
    // Contains call expression, assignment, or update — needs wrapping
    trimmed.contains('(')
        || trimmed.contains('=')
        || trimmed.contains("++")
        || trimmed.contains("--")
}

/// Escape text content for HTML: encode `&`, `<`, `>`, `"`, `'`.
/// This matches Vue's `escapeHtml()` from `@vue/shared`.
fn escape_html(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(ch),
        }
    }
    result
}

/// Process SSR text content: decode entities, escape for HTML, escape for template literal.
fn escape_ssr_text(s: &str) -> String {
    let decoded = decode_html_entities(s);
    let html_escaped = escape_html(&decoded);
    escape_template_literal(&html_escaped)
}

/// Condense consecutive whitespace characters (spaces, tabs, newlines) to a
/// single space, matching Vue's `condense` whitespace mode for text nodes.
fn condense_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !in_ws {
                result.push(' ');
                in_ws = true;
            }
        } else {
            in_ws = false;
            result.push(ch);
        }
    }
    result
}

fn escape_template_literal(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '`' => result.push_str("\\`"),
            '$' => result.push_str("\\$"),
            '\\' => result.push_str("\\\\"),
            _ => result.push(ch),
        }
    }
    result
}

/// Escape a string for use inside a JavaScript string literal.
fn escape_js_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            _ => result.push(ch),
        }
    }
    result
}

/// Convert an HTML attribute name to a JS property key.
/// Handles reserved words and hyphenated names.
fn html_attr_to_js_key(attr: &str) -> String {
    if attr == "class" {
        "class".to_string()
    } else if attr == "for" {
        "for".to_string()
    } else if attr.contains('-') {
        format!("\"{}\"", attr)
    } else {
        attr.to_string()
    }
}

/// Convert a CSS inline style string to a JS object literal.
///
/// `"color: red; font-size: 14px"` → `{"color":"red","fontSize":"14px"}`
fn css_to_js_object(css: &str) -> String {
    let mut result = String::from("{");
    let mut first = true;
    for decl in css.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon_pos) = decl.find(':') {
            let prop = decl[..colon_pos].trim();
            let value = decl[colon_pos + 1..].trim();
            if prop.is_empty() || value.is_empty() {
                continue;
            }
            if !first {
                result.push(',');
            }
            first = false;
            // Keep CSS property names in original kebab-case for SSR
            // (Vue SSR emits {"margin-left":"20px"}, not {"marginLeft":"20px"})
            result.push('"');
            result.push_str(prop);
            result.push_str("\":\"");
            result.push_str(&escape_js_string(value));
            result.push('"');
        }
    }
    result.push('}');
    result
}

/// Check if an HTML attribute is a boolean attribute.
/// Boolean attributes only have meaning by their presence (no value needed).
fn is_boolean_html_attr(attr: &str) -> bool {
    matches!(
        attr,
        "disabled"
            | "checked"
            | "selected"
            | "readonly"
            | "required"
            | "multiple"
            | "autofocus"
            | "autoplay"
            | "controls"
            | "default"
            | "defer"
            | "formnovalidate"
            | "hidden"
            | "ismap"
            | "loop"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "reversed"
            | "scoped"
            | "seamless"
            | "allowfullscreen"
            | "async"
            | "itemscope"
            | "inert"
    )
}

/// Check if a string is a numeric literal (integer or float, optionally negative).
fn is_numeric_literal(s: &str) -> bool {
    let s = if let Some(rest) = s.strip_prefix('-') {
        rest
    } else {
        s
    };
    if s.is_empty() {
        return false;
    }
    let mut has_dot = false;
    for b in s.bytes() {
        if b == b'.' {
            if has_dot {
                return false;
            }
            has_dot = true;
        } else if !b.is_ascii_digit() {
            return false;
        }
    }
    // Must have at least one digit (not just "." or "-.")
    s.bytes().any(|b| b.is_ascii_digit())
}

/// Merge duplicate event handler keys in `props_parts` into arrays.
///
/// When v-model and an explicit `@update:model-value` handler coexist on the same component,
/// both emit a `"onUpdate:modelValue": <handler>` entry. Vue merges them into a single entry
/// with an array value: `"onUpdate:modelValue": [handler1, handler2]`.
///
/// Each entry in `props_parts` is a string like `"onUpdate:modelValue": $event => ...`
/// or `key: value`. This function finds entries with the same key, removes the duplicates,
/// and replaces the first occurrence with the merged array form.
fn merge_duplicate_event_handlers(parts: &mut Vec<String>) {
    if parts.len() < 2 {
        return;
    }

    // Extract key from a "key: value" or `"key": value` entry.
    // Returns (key_with_quotes, value) where key_with_quotes includes any surrounding quotes.
    fn extract_key_value(entry: &str) -> Option<(&str, &str)> {
        // Handle quoted keys: "onUpdate:modelValue": value
        if let Some(stripped) = entry.strip_prefix('"') {
            if let Some(close_quote) = stripped.find('"') {
                let key_end = close_quote + 2; // past the closing quote
                let rest = &entry[key_end..];
                // Skip ": "
                let value_start = rest.find(": ").map(|i| i + 2)?;
                let value = &rest[value_start..];
                return Some((&entry[..key_end], value));
            }
        }
        // Handle unquoted keys: key: value
        let colon_pos = entry.find(": ")?;
        Some((&entry[..colon_pos], &entry[colon_pos + 2..]))
    }

    // Find duplicate keys: collect (key, indices) pairs.
    // Use owned Strings to avoid borrowing from parts.
    let mut seen: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if let Some((key, _)) = extract_key_value(part) {
            let key_owned = key.to_string();
            if let Some(entry) = seen.iter_mut().find(|(k, _)| *k == key_owned) {
                entry.1.push(i);
            } else {
                seen.push((key_owned, vec![i]));
            }
        }
    }

    // Collect all merge operations first, then apply all at once.
    // We must not modify `parts` during the collection phase because removing
    // entries for one key group would shift indices for subsequent groups.
    let mut merges: Vec<(usize, String)> = Vec::new(); // (first_index, merged_entry)
    let mut removals: Vec<usize> = Vec::new(); // indices to remove

    for (_key, indices) in &seen {
        if indices.len() < 2 {
            continue;
        }
        // Collect all values from original (unmodified) parts
        let values: Vec<String> = indices
            .iter()
            .filter_map(|&i| extract_key_value(&parts[i]).map(|(_, v)| v.to_string()))
            .collect();

        // Build merged entry: "key": [value1, value2]
        let key_str = extract_key_value(&parts[indices[0]])
            .map(|(k, _)| k.to_string())
            .unwrap_or_default();
        let merged = format!("{}: [{}]", key_str, values.join(", "));

        merges.push((indices[0], merged));
        // Mark all but first for removal
        removals.extend_from_slice(&indices[1..]);
    }

    // Apply merges (replacements) first — indices are still valid
    for (idx, merged) in &merges {
        parts[*idx] = merged.clone();
    }

    // Sort removals in descending order, then remove from the end
    removals.sort_unstable();
    removals.dedup();
    for &i in removals.iter().rev() {
        parts.remove(i);
    }
}

#[cfg(test)]
mod tests;
