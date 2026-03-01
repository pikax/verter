//! Vapor2 — experimental second-generation vapor code generation backend.
//!
//! **Status: Experimental.** This backend explores alternative codegen approaches
//! (NodeId-based naming, scope-stack with buffer swapping). It is kept for
//! comparison and experimentation, not currently used in production builds.
//!
//! Produces the same Vapor runtime API calls as the first-generation
//! [`super::vapor`] backend but with a refined internal architecture:
//! NodeId-based variable naming (no counters), a scope-stack design with
//! buffer swapping, and on-demand metadata derivation from the AST.
//!
//! ## Output shape
//!
//! ```js
//! const t0 = _template("<div> </div>", true)
//!
//! function render(_ctx, _cache, $props, $setup, $data, $options) {
//!   const n0 = t0()
//!   const x0 = _txt(n0)
//!   _renderEffect(() => {
//!     _setText(x0, _toDisplayString(_ctx.msg))
//!   })
//!   return n0
//! }
//! ```
//!
//! ## Codegen strategy
//!
//! Like Vapor v1, the entire `<template>` block is replaced with a single
//! overwrite containing the assembled output. The output has the same three
//! sections (template declarations, delegated events, render function body).
//!
//! Key differences from Vapor v1:
//!
//! - **NodeId-based variable naming** — variable suffixes are the AST
//!   `NodeId.0` value (e.g. `n3`, `n7`) rather than sequential counters.
//!   This eliminates the `VaporCounters` allocator entirely.
//! - **Scope-stack with buffer swapping** — structural directives (`v-if`,
//!   `v-for`), components, and slots push a `StructuralScope` that swaps
//!   in fresh accumulators (`body_lines`, `effect_lines`, `html_buf`,
//!   `root_ids`, `text_parts`). On leave, the parent's buffers are restored
//!   and the scope's output is assembled into a closure body. This replaces
//!   Vapor v1's per-element `VaporElementState` stack.
//! - **Flat accumulator model** — output is written directly into
//!   `body_lines` and `effect_lines` vectors (bump-allocated strings)
//!   rather than building intermediate `VaporRootElement` records.
//! - **Component and slot support** — dedicated `ScopeKind::Component`,
//!   `ScopeKind::SlotOutlet`, and `ScopeKind::NamedSlot` handle component
//!   children as slot content via scope isolation.
//! - **v-memo support** — `_withMemo()` wrapping with a `memo_cache_idx`
//!   counter for cache slot allocation.
//!
//! ## Shared vs unique logic
//!
//! Binding resolution, runtime helper constants, and the DFS walker are shared
//! with the VDOM and Vapor v1 backends (see [`super::binding`], [`super::shared`],
//! [`super::walker`]). This backend's unique elements are its scope-stack
//! architecture, NodeId-based naming, and the `component`/`structural`/`events`
//! submodules that factor out component codegen, structural directive codegen,
//! and event handling respectively.

pub mod component;
pub mod directives;
pub mod element;
pub mod events;
pub mod props;
pub mod structural;
pub mod text;

use crate::ast::types::{
    AstNodeKind, ChildrenFlags, CommentNode, ElementNode, ElementNodeConditionKind,
    InterpolationNode, TagType, TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::template::code_gen::shared::helpers::{self, push_u32, VaporHelper};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::types::NodeId;

use super::binding::BindingResolver;
use super::types::{CodeGenOutput, VaporTextPart};
use super::{TemplateCodeGen, TemplateCodeGenOptions};

use rustc_hash::FxHashSet;

/// The kind of structural scope.
enum ScopeKind {
    /// v-if/v-else-if/v-else or v-for scope.
    Structural,
    /// Component scope — captures children as slot content.
    Component,
    /// Slot outlet scope — captures children as fallback content.
    SlotOutlet,
    /// Individual named slot scope inside a component.
    /// The string is the slot name (e.g. "default", "header").
    NamedSlot(String),
}

/// Saved parent state during a structural scope (v-if/v-for/component/slot).
///
/// When entering an element with a structural directive, we push a scope
/// that swaps in fresh accumulators. All output during the DFS walk goes
/// into `self.*` (which are the scope's buffers after the swap). On leave,
/// we pop the scope, restore the parent's buffers, and assemble the scope's
/// output into a closure body.
struct StructuralScope<'alloc> {
    /// The NodeId of the element that created this scope.
    element_id: NodeId,
    /// What kind of scope this is.
    kind: ScopeKind,
    /// Saved parent depth.
    saved_depth: u32,
    /// Saved parent body_lines.
    saved_body_lines: Vec<&'alloc str>,
    /// Saved parent effect_lines.
    saved_effect_lines: Vec<&'alloc str>,
    /// Saved parent html_buf.
    saved_html_buf: String,
    /// Saved parent root_ids.
    saved_root_ids: Vec<NodeId>,
    /// Saved parent text_parts.
    saved_text_parts: Vec<VaporTextPart<'alloc>>,
    /// Saved parent has_render_effect.
    saved_has_render_effect: bool,
    /// Saved parent root_body_start_idx.
    saved_root_body_start_idx: usize,
}

/// A completed slot body (captured output from a named slot or default slot).
struct SlotBody<'alloc> {
    /// Slot name (e.g. "default", "header").
    name: String,
    /// Body lines of the slot closure.
    body_lines: Vec<&'alloc str>,
    /// Root NodeIds within this slot (for the return statement).
    root_ids: Vec<NodeId>,
}

/// A completed branch of a v-if chain.
struct VIfBranch<'alloc> {
    kind: ElementNodeConditionKind,
    /// The assembled body lines for this branch.
    body_lines: Vec<&'alloc str>,
    /// Root NodeIds within this branch.
    root_ids: Vec<NodeId>,
    /// The condition expression (None for v-else).
    condition_expr: Option<&'alloc str>,
}

/// Pending v-if chain accumulator.
///
/// A v-if chain starts when a v-if element completes and accumulates
/// v-else-if / v-else branches until the chain ends (non-continuation
/// sibling or template end).
struct VIfChain<'alloc> {
    /// The NodeId of the first v-if element (used for the outer variable).
    vif_id: NodeId,
    /// Whether this chain is at root level in its parent context.
    is_root: bool,
    /// Accumulated branches.
    branches: Vec<VIfBranch<'alloc>>,
}

/// Vapor2 stateless code generation backend.
///
/// Uses NodeId as variable name suffix — no separate counters.
/// Derives metadata from the AST on-demand — no per-element state caching.
pub struct Vapor2CodeGen<'ast, 'alloc> {
    /// Reference to the template AST arena for O(1) node lookups.
    ast: &'ast TemplateAst,
    resolver: BindingResolver<'alloc>,
    options: TemplateCodeGenOptions,

    // Minimal mutable state:
    /// Current text group (cleared per element).
    text_parts: Vec<VaporTextPart<'alloc>>,

    // Output accumulators:
    /// Shared HTML buffer, cleared per root element.
    html_buf: String,
    /// Render function body lines (bump-allocated).
    body_lines: Vec<&'alloc str>,
    /// Hoisted template declarations.
    template_decls: Vec<&'alloc str>,
    /// Lines inside the current render effect block.
    effect_lines: Vec<&'alloc str>,
    /// Delegated event names.
    delegated_events: Vec<&'alloc str>,
    /// Set of delegated event names (for dedup).
    delegated_events_set: FxHashSet<&'alloc str>,
    /// Root element NodeIds for return statement.
    root_ids: Vec<NodeId>,
    /// DFS depth (0 = root-level children of <template>).
    depth: u32,
    /// Track if we need render effect for current root subtree.
    has_render_effect: bool,
    /// Index into body_lines where current root element's subtree starts.
    /// Used to insert template instantiation BEFORE child navigation.
    root_body_start_idx: usize,
    /// Stack of structural scopes for v-if/v-for/component/slot.
    scope_stack: Vec<StructuralScope<'alloc>>,
    /// Pending v-if chain being accumulated.
    pending_vif_chain: Option<VIfChain<'alloc>>,
    /// Accumulated slot bodies for the current component scope.
    /// Populated as named slot scopes complete; consumed when the component scope completes.
    slot_bodies: Vec<SlotBody<'alloc>>,
    /// Cache index counter for v-memo.
    memo_cache_idx: u32,
}

impl<'ast, 'alloc> Vapor2CodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        resolver: BindingResolver<'alloc>,
        _source: &'alloc str,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            resolver,
            options: options.clone(),
            text_parts: Vec::new(),
            html_buf: String::with_capacity(256),
            body_lines: Vec::new(),
            template_decls: Vec::new(),
            effect_lines: Vec::new(),
            delegated_events: Vec::new(),
            delegated_events_set: FxHashSet::default(),
            root_ids: Vec::new(),
            depth: 0,
            has_render_effect: false,
            root_body_start_idx: 0,
            scope_stack: Vec::new(),
            pending_vif_chain: None,
            slot_bodies: Vec::new(),
            memo_cache_idx: 0,
        }
    }

    /// Find the nearest ancestor element's NodeId for a given node.
    fn find_parent_element_id(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.ast.nodes[id.0];
        node.parent
    }

    /// Assemble the final Vapor output.
    fn assemble_output(&mut self, _out: &mut CodeGenOutput<'alloc>) -> String {
        let mut buf = String::with_capacity(512);

        // 1. Template declarations
        for decl in &self.template_decls {
            buf.push_str(decl);
            buf.push('\n');
        }

        // 2. Render function signature
        if self.options.is_inline {
            buf.push_str("return (_ctx,_cache) => {\n");
        } else {
            buf.push_str("function render(_ctx, _cache, $props, $setup, $data, $options) {\n");
        }

        // 3. Delegated events
        if !self.delegated_events.is_empty() {
            buf.push_str("  _delegateEvents(");
            for (i, evt) in self.delegated_events.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push('"');
                helpers::escape_js_string_into(&mut buf, evt);
                buf.push('"');
            }
            buf.push_str(")\n");
        }

        // 4. Body lines
        for line in &self.body_lines {
            buf.push_str(line);
            buf.push('\n');
        }

        // 5. Return statement
        match self.root_ids.len() {
            0 => buf.push_str("  return null\n"),
            1 => {
                buf.push_str("  return n");
                push_u32(&mut buf, self.root_ids[0].0 as u32);
                buf.push('\n');
            }
            _ => {
                buf.push_str("  return [");
                for (i, root_id) in self.root_ids.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push('n');
                    push_u32(&mut buf, root_id.0 as u32);
                }
                buf.push_str("]\n");
            }
        }

        buf.push('}');
        buf
    }

    /// Close and flush any open render effect.
    ///
    /// When `v_once` is true, effect lines are emitted as direct statements
    /// without the `_renderEffect(() => { ... })` wrapper.
    fn close_render_effect(
        &mut self,
        v_once: bool,
        v_memo_expr: Option<&str>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        if !self.effect_lines.is_empty() {
            if v_once {
                // v-once: emit effects as direct statements (no reactive wrapper)
                for line in self.effect_lines.drain(..) {
                    self.body_lines.push(line);
                }
            } else if let Some(memo_deps) = v_memo_expr {
                // v-memo: wrap render effect with _withMemo
                let mut open = String::with_capacity(64);
                open.push_str("  _renderEffect(() => _withMemo(");
                open.push_str(memo_deps);
                open.push_str(", () => {");
                self.body_lines.push(out.alloc_str(&open));
                for line in self.effect_lines.drain(..) {
                    self.body_lines.push(line);
                }
                let mut close = String::with_capacity(32);
                close.push_str("  }, _cache, ");
                push_u32(&mut close, self.memo_cache_idx);
                close.push_str("))");
                self.body_lines.push(out.alloc_str(&close));
                self.memo_cache_idx += 1;
                out.add_vapor_import(VaporHelper::RenderEffect);
                out.add_vapor_import(VaporHelper::WithMemo);
            } else {
                self.body_lines
                    .push(out.alloc_str("  _renderEffect(() => {"));
                for line in self.effect_lines.drain(..) {
                    self.body_lines.push(line);
                }
                self.body_lines.push(out.alloc_str("  })"));
                out.add_vapor_import(VaporHelper::RenderEffect);
            }
        }
        self.has_render_effect = false;
    }

    /// Push a structural scope for v-if/v-for/component/slot.
    ///
    /// Swaps in fresh accumulators for `body_lines`, `effect_lines`, `html_buf`,
    /// `root_ids`, and `text_parts`. The parent's buffers are saved in the scope
    /// and restored when the scope is popped.
    fn push_structural_scope(&mut self, id: NodeId, kind: ScopeKind) {
        let scope = StructuralScope {
            element_id: id,
            kind,
            saved_depth: self.depth,
            saved_body_lines: std::mem::take(&mut self.body_lines),
            saved_effect_lines: std::mem::take(&mut self.effect_lines),
            saved_html_buf: std::mem::replace(&mut self.html_buf, String::with_capacity(256)),
            saved_root_ids: std::mem::take(&mut self.root_ids),
            saved_text_parts: std::mem::take(&mut self.text_parts),
            saved_has_render_effect: self.has_render_effect,
            saved_root_body_start_idx: self.root_body_start_idx,
        };
        self.scope_stack.push(scope);
        self.depth = 0;
        self.has_render_effect = false;
        self.root_body_start_idx = 0;
    }

    /// Pop a scope: captures current output, restores parent buffers.
    /// Returns `(scope, body_lines, root_ids)`.
    fn pop_scope(&mut self) -> (StructuralScope<'alloc>, Vec<&'alloc str>, Vec<NodeId>) {
        let mut scope = self.scope_stack.pop().unwrap();
        let scope_body_lines = std::mem::replace(
            &mut self.body_lines,
            std::mem::take(&mut scope.saved_body_lines),
        );
        let scope_root_ids = std::mem::replace(
            &mut self.root_ids,
            std::mem::take(&mut scope.saved_root_ids),
        );
        let _ = std::mem::replace(
            &mut self.effect_lines,
            std::mem::take(&mut scope.saved_effect_lines),
        );
        let _ = std::mem::replace(
            &mut self.html_buf,
            std::mem::take(&mut scope.saved_html_buf),
        );
        let _ = std::mem::replace(
            &mut self.text_parts,
            std::mem::take(&mut scope.saved_text_parts),
        );
        (scope, scope_body_lines, scope_root_ids)
    }

    /// Restore saved depth/flags from a popped scope.
    fn restore_scope_state(&mut self, scope: &StructuralScope<'alloc>) {
        self.depth = scope.saved_depth;
        self.has_render_effect = scope.saved_has_render_effect;
        self.root_body_start_idx = scope.saved_root_body_start_idx;
    }

    /// Complete a structural scope on leave_element.
    ///
    /// Pops the scope, captures the scope's output, restores parent's buffers,
    /// and dispatches based on scope kind.
    fn complete_structural_scope(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let (scope, scope_body_lines, scope_root_ids) = self.pop_scope();
        self.restore_scope_state(&scope);

        match scope.kind {
            ScopeKind::Structural => {
                self.complete_structural_directive(
                    id,
                    el,
                    source,
                    scope_body_lines,
                    scope_root_ids,
                    out,
                );
            }
            ScopeKind::Component => {
                // Component scope completed — slot_bodies has been populated
                // by completed NamedSlot scopes. Any remaining body_lines in
                // the scope (from non-slotted children) become the default slot.
                self.complete_component_scope(
                    id,
                    el,
                    source,
                    scope_body_lines,
                    scope_root_ids,
                    out,
                );
            }
            ScopeKind::SlotOutlet => {
                self.complete_slot_outlet_scope(
                    id,
                    el,
                    source,
                    scope_body_lines,
                    scope_root_ids,
                    out,
                );
            }
            ScopeKind::NamedSlot(slot_name) => {
                // Named slot completed — assemble its body into a SlotBody
                // and push it into slot_bodies for the parent component scope.
                self.slot_bodies.push(SlotBody {
                    name: slot_name,
                    body_lines: scope_body_lines,
                    root_ids: scope_root_ids,
                });
            }
        }
    }

    /// Complete a structural directive scope (v-if/v-for).
    fn complete_structural_directive(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        scope_body_lines: Vec<&'alloc str>,
        scope_root_ids: Vec<NodeId>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        if let Some(ref condition) = el.v_condition {
            let condition_expr = match (condition.prop.value_start, condition.prop.value_end) {
                (Some(s), Some(e)) => {
                    let raw = &source[s as usize..e as usize];
                    let prefixed = apply_simple_prefix(raw.trim(), &self.resolver);
                    Some(out.alloc_str(&prefixed))
                }
                _ => None,
            };

            let branch = VIfBranch {
                kind: condition.kind.clone(),
                body_lines: scope_body_lines,
                root_ids: scope_root_ids,
                condition_expr,
            };

            match condition.kind {
                ElementNodeConditionKind::If => {
                    self.pending_vif_chain = Some(VIfChain {
                        vif_id: id,
                        is_root: self.depth == 0,
                        branches: vec![branch],
                    });
                }
                ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                    if let Some(ref mut chain) = self.pending_vif_chain {
                        chain.branches.push(branch);
                    }
                    if matches!(condition.kind, ElementNodeConditionKind::Else) {
                        self.flush_vif_chain(out);
                    }
                }
            }

            structural::collect_condition_imports(out);
        } else if el.v_for.is_some() {
            self.emit_v_for(id, el, source, scope_body_lines, scope_root_ids, out);
            structural::collect_for_imports(out);
        }
    }

    /// Complete a component scope — assemble slot closures and emit _createComponent.
    fn complete_component_scope(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        default_body_lines: Vec<&'alloc str>,
        default_root_ids: Vec<NodeId>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Collect all slot bodies. Named slots were accumulated in self.slot_bodies.
        // Any remaining output in the scope (non-slotted children) becomes the default slot.
        let mut slots = std::mem::take(&mut self.slot_bodies);

        // If there are default body lines and no explicit default slot, create one
        let has_explicit_default = slots.iter().any(|s| s.name == "default");
        if !has_explicit_default && (!default_body_lines.is_empty() || !default_root_ids.is_empty())
        {
            slots.push(SlotBody {
                name: "default".to_string(),
                body_lines: default_body_lines,
                root_ids: default_root_ids,
            });
        }

        // Build the slots object string
        let slots_str = if slots.is_empty() {
            None
        } else {
            let mut s = String::with_capacity(256);
            s.push_str("{\n");
            for (i, slot) in slots.iter().enumerate() {
                if i > 0 {
                    s.push_str(",\n");
                }
                s.push_str("    ");
                s.push_str(&slot.name);
                s.push_str(": () => {\n");
                for line in &slot.body_lines {
                    s.push_str("    ");
                    s.push_str(line);
                    s.push('\n');
                }
                // Return statement
                match slot.root_ids.len() {
                    0 => s.push_str("      return null\n"),
                    1 => {
                        s.push_str("      return n");
                        push_u32(&mut s, slot.root_ids[0].0 as u32);
                        s.push('\n');
                    }
                    _ => {
                        s.push_str("      return [");
                        for (j, rid) in slot.root_ids.iter().enumerate() {
                            if j > 0 {
                                s.push_str(", ");
                            }
                            s.push('n');
                            push_u32(&mut s, rid.0 as u32);
                        }
                        s.push_str("]\n");
                    }
                }
                s.push_str("    }");
            }
            s.push_str("\n  }");
            Some(s)
        };

        // Emit the _createComponent call with slots
        component::process_component_with_slots(
            id,
            el,
            source,
            &self.resolver,
            slots_str.as_deref(),
            &mut self.body_lines,
            out,
        );
        if self.depth == 0 {
            self.root_ids.push(id);
        }
    }

    /// Complete a slot outlet scope — assemble fallback closure and emit _createSlot.
    fn complete_slot_outlet_scope(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        fallback_body_lines: Vec<&'alloc str>,
        fallback_root_ids: Vec<NodeId>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let fallback_str = if fallback_body_lines.is_empty() && fallback_root_ids.is_empty() {
            None
        } else {
            let mut s = String::with_capacity(128);
            s.push_str("() => {\n");
            for line in &fallback_body_lines {
                s.push_str("    ");
                s.push_str(line);
                s.push('\n');
            }
            match fallback_root_ids.len() {
                0 => s.push_str("    return null\n"),
                1 => {
                    s.push_str("    return n");
                    push_u32(&mut s, fallback_root_ids[0].0 as u32);
                    s.push('\n');
                }
                _ => {
                    s.push_str("    return [");
                    for (j, rid) in fallback_root_ids.iter().enumerate() {
                        if j > 0 {
                            s.push_str(", ");
                        }
                        s.push('n');
                        push_u32(&mut s, rid.0 as u32);
                    }
                    s.push_str("]\n");
                }
            }
            s.push_str("  }");
            Some(s)
        };

        component::process_slot_outlet_with_fallback(
            id,
            el,
            source,
            fallback_str.as_deref(),
            &mut self.body_lines,
            out,
        );
        if self.depth == 0 {
            self.root_ids.push(id);
        }
    }

    /// Flush the pending v-if chain, assembling all branches into a
    /// `_createIf(...)` expression and pushing it to body_lines.
    fn flush_vif_chain(&mut self, out: &mut CodeGenOutput<'alloc>) {
        let Some(chain) = self.pending_vif_chain.take() else {
            return;
        };

        let mut code = String::with_capacity(256);

        // Variable declaration
        code.push_str("  const n");
        push_u32(&mut code, chain.vif_id.0 as u32);
        code.push_str(" = ");

        for branch in &chain.branches {
            match &branch.kind {
                ElementNodeConditionKind::If => {
                    code.push_str("_createIf(() => (");
                    code.push_str(branch.condition_expr.unwrap_or("true"));
                    code.push_str("), () => {\n");
                }
                ElementNodeConditionKind::ElseIf => {
                    code.push_str(", () => _createIf(() => (");
                    code.push_str(branch.condition_expr.unwrap_or("true"));
                    code.push_str("), () => {\n");
                }
                ElementNodeConditionKind::Else => {
                    code.push_str(", () => {\n");
                }
            }

            // Branch body lines
            for line in &branch.body_lines {
                code.push_str(line);
                code.push('\n');
            }

            // Return statement within closure
            match branch.root_ids.len() {
                0 => code.push_str("  return null\n"),
                1 => {
                    code.push_str("  return n");
                    push_u32(&mut code, branch.root_ids[0].0 as u32);
                    code.push('\n');
                }
                _ => {
                    code.push_str("  return [");
                    for (j, rid) in branch.root_ids.iter().enumerate() {
                        if j > 0 {
                            code.push_str(", ");
                        }
                        code.push('n');
                        push_u32(&mut code, rid.0 as u32);
                    }
                    code.push_str("]\n");
                }
            }

            // Close branch closure
            code.push('}');
        }

        // Close all _createIf calls (one `)` per If/ElseIf branch)
        let create_if_count = chain
            .branches
            .iter()
            .filter(|b| {
                matches!(
                    b.kind,
                    ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf
                )
            })
            .count();
        for _ in 0..create_if_count {
            code.push(')');
        }

        self.body_lines.push(out.alloc_str(&code));

        // If the chain was at root level, add to root_ids
        if chain.is_root {
            self.root_ids.push(chain.vif_id);
        }
    }

    /// Emit a v-for wrapper around the scope's output.
    fn emit_v_for(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        source: &'alloc str,
        scope_body_lines: Vec<&'alloc str>,
        scope_root_ids: Vec<NodeId>,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let v_for = el.v_for.as_ref().unwrap();
        let expr = match (v_for.value_start, v_for.value_end) {
            (Some(s), Some(e)) => &source[s as usize..e as usize],
            _ => return,
        };

        let (params, iterable) = helpers::parse_v_for_expression(expr);
        let iterable_prefixed = apply_simple_prefix(iterable.trim(), &self.resolver);

        // Extract :key expression if present
        let key_expr = el.props.iter().find_map(|p| {
            if !p.is_directive {
                return None;
            }
            let name = &source[p.start as usize..p.name_end as usize];
            let arg = match (p.arg_start, p.arg_end) {
                (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
                _ => None,
            };
            if (name.starts_with(':') || name == "v-bind") && arg == Some("key") {
                p.value_start
                    .zip(p.value_end)
                    .map(|(vs, ve)| &source[vs as usize..ve as usize])
            } else {
                None
            }
        });

        let mut code = String::with_capacity(256);
        code.push_str("  const n");
        push_u32(&mut code, id.0 as u32);
        code.push_str(" = _createFor(() => (");
        code.push_str(&iterable_prefixed);
        code.push_str("), (");
        code.push_str(params);
        code.push_str(") => {\n");

        // Body
        for line in &scope_body_lines {
            code.push_str(line);
            code.push('\n');
        }

        // Return
        match scope_root_ids.len() {
            0 => code.push_str("  return null\n"),
            1 => {
                code.push_str("  return n");
                push_u32(&mut code, scope_root_ids[0].0 as u32);
                code.push('\n');
            }
            _ => {
                code.push_str("  return [");
                for (j, rid) in scope_root_ids.iter().enumerate() {
                    if j > 0 {
                        code.push_str(", ");
                    }
                    code.push('n');
                    push_u32(&mut code, rid.0 as u32);
                }
                code.push_str("]\n");
            }
        }

        code.push_str("  }");

        // Key argument
        if let Some(key) = key_expr {
            code.push_str(", (");
            code.push_str(params);
            code.push_str(") => (");
            code.push_str(key);
            code.push(')');
        }

        code.push(')');

        self.body_lines.push(out.alloc_str(&code));

        if self.depth == 0 {
            self.root_ids.push(id);
        }
    }

    /// Extract the slot name from a `v-slot:name` directive.
    /// Returns "default" if no arg is specified.
    fn extract_slot_name(&self, el: &ElementNode, source: &str) -> String {
        if let Some(ref v_slot) = el.v_slot {
            match (v_slot.arg_start, v_slot.arg_end) {
                (Some(s), Some(e)) => source[s as usize..e as usize].to_string(),
                _ => "default".to_string(),
            }
        } else {
            "default".to_string()
        }
    }

    /// Process a static text node.
    fn do_visit_text(
        &mut self,
        id: NodeId,
        text: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let content = &source[text.start as usize..text.end as usize];
        self.html_buf.push_str(content);

        // Check if parent has interpolation → need text parts
        let has_interpolation = self
            .ast
            .nodes
            .get(id.0)
            .and_then(|node| node.parent)
            .and_then(|pid| self.ast.nodes.get(pid.0))
            .map(|parent_node| match &parent_node.kind {
                AstNodeKind::Element(el) => el.children_flag.has(ChildrenFlags::HasInterpolation),
                _ => false,
            })
            .unwrap_or(false);

        if has_interpolation && !content.is_empty() {
            let mut buf = String::with_capacity(content.len() + 4);
            buf.push('"');
            if helpers::needs_js_escaping(content) {
                helpers::escape_js_string_into(&mut buf, content);
            } else {
                buf.push_str(content);
            }
            buf.push('"');
            self.text_parts
                .push(VaporTextPart::Static(out.alloc_str(&buf)));
        }
    }

    /// Process an interpolation node.
    fn do_visit_interpolation(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        _oxc: &OxcParsedExpression<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Space placeholder in HTML
        self.html_buf.push(' ');

        // Extract and prefix expression
        let expr = &source[interp.inner_start as usize..interp.inner_end as usize];
        let expr_trimmed = expr.trim();

        let prefixed = apply_simple_prefix(expr_trimmed, &self.resolver);

        // Wrap in _toDisplayString
        let mut tds_buf =
            String::with_capacity(helpers::VAPOR_TO_DISPLAY_STRING.len() + prefixed.len() + 2);
        tds_buf.push_str(helpers::VAPOR_TO_DISPLAY_STRING);
        tds_buf.push('(');
        tds_buf.push_str(&prefixed);
        tds_buf.push(')');
        self.text_parts
            .push(VaporTextPart::Dynamic(out.alloc_str(&tds_buf)));

        out.add_vapor_import(VaporHelper::ToDisplayString);
    }
}

/// Apply binding prefix/suffix for simple identifiers.
fn apply_simple_prefix(expr: &str, resolver: &BindingResolver<'_>) -> String {
    if is_simple_ident(expr) {
        let prefix = resolver.resolve_prefix(expr);
        let suffix = resolver.resolve_suffix(expr);
        let mut buf = String::with_capacity(prefix.len() + expr.len() + suffix.len());
        buf.push_str(prefix);
        buf.push_str(expr);
        buf.push_str(suffix);
        buf
    } else {
        expr.to_string()
    }
}

/// Extract the v-memo deps expression from an element's props.
/// Returns `Some("[dep1, dep2]")` if the element has `v-memo="[dep1, dep2]"`.
fn extract_v_memo_expr(el: &ElementNode, source: &str) -> Option<String> {
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name == "v-memo" {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                return Some(source[vs as usize..ve as usize].to_string());
            }
        }
    }
    None
}

/// Check if a string is a valid simple JavaScript identifier.
fn is_simple_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

impl<'ast, 'alloc> TemplateCodeGen<'alloc> for Vapor2CodeGen<'ast, 'alloc> {
    fn enter_template(
        &mut self,
        _root: &RootNodeTemplate,
        _source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        self.depth = 0;
    }

    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        _source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Flush any pending v-if chain at template end
        self.flush_vif_chain(out);

        let output = self.assemble_output(out);

        let start = root.tag_open.start;
        let end = match root.tag_close.as_ref() {
            Some(tc) => tc.end,
            None => root
                .content
                .as_ref()
                .map(|c| c.end)
                .unwrap_or(root.tag_open.end),
        };
        out.overwrite(start, end, &output);
    }

    fn enter_element(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        _oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> super::WalkAction {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        // Flush pending v-if chain if this element is NOT a continuation
        if self.pending_vif_chain.is_some() {
            let is_continuation = el.v_condition.as_ref().is_some_and(|c| {
                matches!(
                    c.kind,
                    ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                )
            });
            if !is_continuation {
                self.flush_vif_chain(out);
            }
        }

        // Push structural scope for v-if/v-else-if/v-else or v-for
        let has_structural = el.v_condition.is_some() || el.v_for.is_some();
        if has_structural {
            // For nested structural directives (depth > 0), insert a comment
            // placeholder into the parent's HTML before saving it.
            if self.depth > 0
                && (el.tag_type == TagType::Element || el.tag_type == TagType::Template)
            {
                self.html_buf.push_str("<!---->");
            }
            self.push_structural_scope(id, ScopeKind::Structural);
        }

        // Push component scope to capture children as slot content
        if !has_structural && component::is_component(el) && el.content.is_some() {
            self.push_structural_scope(id, ScopeKind::Component);
        }

        // Push slot outlet scope to capture children as fallback content
        if !has_structural && component::is_slot_outlet(el) && el.content.is_some() {
            // Check if slot has any children (fallback content)
            let has_children = el
                .content
                .as_ref()
                .map(|c| !c.children.is_empty())
                .unwrap_or(false);
            if has_children {
                self.push_structural_scope(id, ScopeKind::SlotOutlet);
            }
        }

        // Push named slot scope for <template v-slot:name> inside component
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            let slot_name = self.extract_slot_name(el, source);
            self.push_structural_scope(id, ScopeKind::NamedSlot(slot_name));
        }

        if self.depth == 0 {
            self.html_buf.clear();
            self.has_render_effect = false;
            // Record where this root's body_lines start so we can insert
            // template instantiation before child navigation.
            self.root_body_start_idx = self.body_lines.len();
        }

        // Skip HTML building for components, slot outlets, and <template v-slot>
        let is_slot_template = el.tag_type == TagType::Template && el.v_slot.is_some();
        if (el.tag_type == TagType::Element || el.tag_type == TagType::Template)
            && !is_slot_template
        {
            element::build_open_tag_html(el, source, &mut self.html_buf);
        }

        // For component/slot-outlet/named-slot scopes, the element itself is virtual
        // (doesn't produce HTML). Don't increment depth so children are at depth 0
        // (root level) within the scope.
        let is_scope_container = self.scope_stack.last().is_some_and(|s| {
            s.element_id == id
                && matches!(
                    s.kind,
                    ScopeKind::Component | ScopeKind::SlotOutlet | ScopeKind::NamedSlot(_)
                )
        });
        if !is_scope_container {
            self.depth += 1;
        }
        super::WalkAction::Continue
    }

    fn leave_element(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        _oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        // For scope container elements (component/slot-outlet/named-slot) we
        // didn't increment depth in enter_element, so don't decrement here.
        let is_scope_container = self.scope_stack.last().is_some_and(|s| {
            s.element_id == id
                && matches!(
                    s.kind,
                    ScopeKind::Component | ScopeKind::SlotOutlet | ScopeKind::NamedSlot(_)
                )
        });
        if !is_scope_container {
            self.depth -= 1;
        }

        let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let is_void = el.is_self_closing || el.content.is_none();

        // Handle components — if we have a scope for this component, let scope
        // completion handle the _createComponent emission (with slot closures).
        // Otherwise, process directly (component with no children).
        if component::is_component(el) {
            if self.scope_stack.last().is_some_and(|s| s.element_id == id) {
                // Scope completion handles everything
                self.complete_structural_scope(id, el, source, out);
            } else {
                // No scope (no children) — emit directly
                component::process_component(
                    id,
                    el,
                    source,
                    &self.resolver,
                    &mut self.body_lines,
                    out,
                );
                if self.depth == 0 {
                    self.root_ids.push(id);
                }
            }
            return;
        }

        // Handle slot outlets — if we have a scope (fallback content), let scope
        // completion handle it. Otherwise, process directly.
        if component::is_slot_outlet(el) {
            if self.scope_stack.last().is_some_and(|s| s.element_id == id) {
                self.complete_structural_scope(id, el, source, out);
            } else {
                component::process_slot_outlet(id, el, source, &mut self.body_lines, out);
                if self.depth == 0 {
                    self.root_ids.push(id);
                }
            }
            return;
        }

        // Handle <template v-slot:name> — scope completion on the NamedSlot scope
        if el.tag_type == TagType::Template && el.v_slot.is_some() {
            if self.scope_stack.last().is_some_and(|s| s.element_id == id) {
                self.complete_structural_scope(id, el, source, out);
            }
            // If the element also had a structural directive (v-for/v-if),
            // there's a second scope to complete (pushed first in enter_element).
            if self.scope_stack.last().is_some_and(|s| s.element_id == id) {
                self.complete_structural_scope(id, el, source, out);
            }
            return;
        }

        // Close HTML tag
        element::close_html_tag(&mut self.html_buf, tag_name, is_void);

        // Process text parts if element has dynamic text
        let has_dynamic_text = el.children_flag.has(ChildrenFlags::HasInterpolation);
        if has_dynamic_text && !self.text_parts.is_empty() {
            // We need navigation to the element first if non-root
            if self.depth > 0 {
                let parent_id = self.find_parent_element_id(id).unwrap_or(id);
                let dom_child_index =
                    element::compute_dom_child_index(self.ast, id, self.options.comments);
                element::emit_navigation(id, parent_id, dom_child_index, &mut self.body_lines, out);
            }
            text::process_text_parts(
                id,
                &mut self.text_parts,
                &mut self.body_lines,
                &mut self.effect_lines,
                out,
            );
        } else {
            self.text_parts.clear();
        }

        // Process dynamic props
        if self.depth > 0 {
            // Emit navigation for non-root elements that need dynamic processing
            let needs_dynamic = el.prop_flag.needs_oxc_parsing()
                || el
                    .prop_flag
                    .has(crate::ast::types::PropFlags::HasEventListener)
                || el.prop_flag.has(crate::ast::types::PropFlags::HasShow)
                || el.prop_flag.has(crate::ast::types::PropFlags::HasModel)
                || el.prop_flag.has(crate::ast::types::PropFlags::HasRef)
                || el
                    .prop_flag
                    .has(crate::ast::types::PropFlags::HasCustomDirective);

            if needs_dynamic && !has_dynamic_text {
                // Only emit navigation if we didn't already emit it for text
                let parent_id = self.find_parent_element_id(id).unwrap_or(id);
                let dom_child_index =
                    element::compute_dom_child_index(self.ast, id, self.options.comments);
                element::emit_navigation(id, parent_id, dom_child_index, &mut self.body_lines, out);
            }
        }

        props::process_dynamic_props(id, el, source, &mut self.effect_lines, out);

        // Process events (statements, NOT in renderEffect)
        events::process_events(
            id,
            el,
            source,
            &mut self.body_lines,
            &mut self.delegated_events,
            &mut self.delegated_events_set,
            out,
        );

        // Process v-show (statement)
        directives::process_v_show(id, el, source, &mut self.body_lines, out);

        // Process v-model (statement)
        directives::process_v_model(id, el, source, &mut self.body_lines, out);

        // Process template ref (statement)
        directives::process_template_ref(id, el, source, &mut self.body_lines, out);

        // Process custom directives (statement)
        directives::process_custom_directives(id, el, source, &mut self.body_lines, out);

        // Root element finalization
        if self.depth == 0 {
            // Register template declaration (hoisted above render function)
            element::write_template_decl(
                id,
                &self.html_buf,
                true, // single-root for each template
                &mut self.template_decls,
                out,
            );
            // Insert template instantiation at the START of this root's body_lines,
            // before any child navigation that was accumulated during children processing.
            let instantiation = element::make_template_instantiation(id, out);
            self.body_lines
                .insert(self.root_body_start_idx, instantiation);

            // Close render effect for this root's subtree
            // v-once suppresses the _renderEffect wrapper
            // v-memo wraps with _withMemo
            let v_once = el.v_once.is_some();
            let v_memo_expr = extract_v_memo_expr(el, source);
            self.close_render_effect(v_once, v_memo_expr.as_deref(), out);

            self.root_ids.push(id);
        }

        // Complete structural scope if this element owns the top scope
        if self.scope_stack.last().is_some_and(|s| s.element_id == id) {
            self.complete_structural_scope(id, el, source, out);
        }
    }

    fn visit_text(
        &mut self,
        id: NodeId,
        text: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(source, text.start, text.end, "visit_text");
        self.do_visit_text(id, text, source, out);
    }

    fn visit_interpolation(
        &mut self,
        id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            interp.inner_start,
            interp.inner_end,
            "visit_interpolation",
        );
        self.do_visit_interpolation(id, interp, oxc, source, out);
    }

    fn visit_comment(
        &mut self,
        _id: NodeId,
        comment: &CommentNode,
        source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(source, comment.start, comment.end, "visit_comment");
        if self.options.comments {
            let raw = &source[comment.start as usize..comment.end as usize];
            self.html_buf.push_str(raw);
        }
    }
}

#[cfg(test)]
mod tests;
