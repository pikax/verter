//! The Vue server render target.
//!
//! Produces string concatenation through `_push()` and template literals,
//! as `@vue/compiler-ssr` does:
//!
//! ```js
//! function ssrRender(_ctx, _push, _parent, _attrs) {
//!   _push(`<div${_ssrRenderAttrs(_attrs)}>hello</div>`)
//! }
//! ```
//!
//! Static and interpolated content is batched into one `` _push(`…`) ``
//! literal; structural constructs (`v-if`, `v-for`, components) close it and
//! the next content opens a new one. The emitter records source-position
//! edits; [`VueSsrExecutor`] applies them to a transform over the template,
//! freezes it and fills its client slot branches with fragments. The
//! server target plans nothing for the client: it builds no patch flags, no
//! hoists, no caches and no effect graph.

use std::fmt::Write;

use verter_audit::observe::ObserveExec;
use verter_span::Span;

use crate::assembly::emit_plan::EmitPlanBuilder;
use crate::ast::types::{ElementNodeConditionKind, TagType};
use crate::code_transform::{CodeTransform, EditRecorder};
use crate::compile_request::demand::TargetKind;
use crate::framework::vue::plan::VueTarget;
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::{
    walk_template, VueChainRole, VueChildKind, VueChildRecord, VueCompileNodeId,
    VueTemplateVisitor, VueWalkControl,
};
use crate::framework::vue::target::{VueTargetInputs, VueTargetOutput, VueTargetRefusal};
use crate::framework_common::TemplateRenderExport;
use crate::target_plan::TargetExecutor;
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::work_ledger::CompilerWorkLedger;

mod attrs;
mod component;
mod control_flow;
mod element;
mod escape;
mod fallback;
mod model;
pub(crate) mod props_object;
mod push;
mod raw;
mod slots;
mod state;
mod text;

use state::{ElemCtx, SsrEmitter, SsrOut};

/// Compiles a template to a function that pushes HTML strings.
pub struct VueSsrExecutor;

impl TargetExecutor for VueSsrExecutor {
    const TARGET: TargetKind = TargetKind::Server;
    type Inputs<'a> = &'a VueTargetInputs<'a>;
    type Output<'a> = VueTargetOutput<'a>;
    type Refusal = VueTargetRefusal;

    fn execute<'a, M: ObserveExec>(
        inputs: Self::Inputs<'a>,
        _ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<Self::Output<'a>, Self::Refusal> {
        let options = inputs.options;
        let bindings = VueBindingView::new(
            inputs.semantic.bindings(),
            options.inline,
            false,
            options
                .const_props
                .as_ref()
                .map(|props| props.iter().map(String::as_str).collect()),
            options.ref_bindable_imports.clone(),
        )
        .with_template_scopes(&inputs.semantic.expressions().scopes);
        let mut state = SsrState::<M> {
            emitter: SsrEmitter::new(inputs, bindings),
            out: SsrOut::new(EditRecorder::with_mode(inputs.allocator, options.map_mode)),
        };
        walk_template(inputs.topology, inputs.structure, &mut state);
        state.finish()
    }
}

/// The emitter and the edits it records, walked as one visitor.
struct SsrState<'c, M: ObserveExec> {
    emitter: SsrEmitter<'c, M>,
    out: SsrOut<'c>,
}

impl<'c, M: ObserveExec> SsrState<'c, M> {
    /// Apply the recorded edits and fill the declared slot-content holes.
    fn finish(self) -> Result<VueTargetOutput<'c>, VueTargetRefusal> {
        if let Some(refusal) = self.emitter.refusal {
            return Err(refusal);
        }
        let inputs = self.emitter.inputs;
        let options = inputs.options;
        let source = inputs.source.text;
        let (start, end) = self.emitter.template_window();
        let helpers = self.out.helpers();
        let mut edits = self.out.edits;
        if options.force_js {
            // Removals share the recorder's replacement law and travel with
            // slot content when its source range is relocated.
            for expr in inputs.semantic.expressions().iter_expressions() {
                if let Some(expression) = &expr.expression {
                    for (start, end) in
                        crate::strip_types::typescript::collect_ts_removal_spans(expression)
                    {
                        edits.overwrite(expr.offset + start, expr.offset + end, "");
                    }
                }
            }
        }

        let mut transform = CodeTransform::<M>::for_window_in(
            source,
            Span::new(start, end),
            inputs.topology.len(),
            inputs.allocator,
            options.map_mode,
        )
        .map_err(VueTargetRefusal::Edit)?;
        edits
            .apply(&mut transform)
            .map_err(VueTargetRefusal::Edit)?;
        let mut marks = Vec::with_capacity(self.emitter.slot_fills.len());
        let mut fills = Vec::with_capacity(self.emitter.slot_fills.len());
        for (content, start, fill) in self.emitter.slot_fills {
            let marker = transform
                .mark_generated_sub_range(content, start, start + 2)
                .ok_or(VueTargetRefusal::Emit(
                    crate::assembly::emit_plan::EmitPlanError::UnknownSlot,
                ))?;
            let id = crate::emission::MarkerId::fresh();
            marks.push((marker, id));
            fills.push((id, fill));
        }
        let fragment = transform
            .freeze_marked(inputs.source.clone(), &marks)
            .map_err(|error| match error {
                crate::code_transform::MarkedFreezeError::Transform(error) => {
                    VueTargetRefusal::Edit(error)
                }
                _ => VueTargetRefusal::Emit(crate::assembly::emit_plan::EmitPlanError::UnknownSlot),
            })?;
        let fills = fills
            .into_iter()
            .map(|(id, fill)| {
                fragment
                    .slot(id)
                    .map(|slot| (slot, fill))
                    .ok_or(VueTargetRefusal::Emit(
                        crate::assembly::emit_plan::EmitPlanError::UnknownSlot,
                    ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut plan = EmitPlanBuilder::new(options.map_mode);
        plan.push_fragment(fragment)
            .map_err(VueTargetRefusal::Emit)?;
        for (slot, fill) in fills {
            plan.replace_marked(slot, fill)
                .map_err(VueTargetRefusal::Emit)?;
        }
        let render = plan.finish().map_err(VueTargetRefusal::Emit)?;

        Ok(VueTargetOutput {
            target: VueTarget::Ssr,
            render,
            preamble: None,
            helpers,
            render_export: TemplateRenderExport::SsrRender,
            diagnostics: Vec::new(),
        })
    }
}

impl<'c, M: ObserveExec> VueTemplateVisitor for SsrState<'c, M> {
    fn enter_template(&mut self) {
        self.emitter.enter_template(&mut self.out);
    }

    fn leave_template(&mut self) {
        self.emitter.leave_template(&mut self.out);
    }

    fn enter_element(
        &mut self,
        node: VueCompileNodeId,
        _kind: VueTemplateNodeKind,
    ) -> VueWalkControl {
        self.emitter.enter_scope_of(node);
        let control = self.emitter.enter_element(node, &mut self.out);
        self.emitter.bindings.pop_local_scope();
        control
    }

    fn leave_element(&mut self, node: VueCompileNodeId, _kind: VueTemplateNodeKind) {
        self.emitter.enter_scope_of(node);
        self.emitter.leave_element(node, &mut self.out);
        self.emitter.bindings.pop_local_scope();
    }

    fn visit_text(&mut self, node: VueCompileNodeId) {
        self.emitter.text_node(node, &mut self.out);
    }

    fn visit_interpolation(&mut self, node: VueCompileNodeId) {
        self.emitter.enter_scope_of(node);
        self.emitter.interpolation_node(node, &mut self.out);
        self.emitter.bindings.pop_local_scope();
    }

    fn visit_comment(&mut self, node: VueCompileNodeId) {
        self.emitter.comment_node(node, &mut self.out);
    }
}

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// The root `<template>` tag's range: its open tag through its close
    /// tag, which the render function replaces.
    fn template_window(&self) -> (u32, u32) {
        let root = &self.inputs.topology.ast().root;
        (root.tag_open.start, self.root_close().1)
    }

    /// The root close tag's range; empty at the content's end when absent.
    fn root_close(&self) -> (u32, u32) {
        let root = &self.inputs.topology.ast().root;
        match root.tag_close.as_ref() {
            Some(tc) => (tc.start, tc.end),
            None => {
                let pos = root
                    .content
                    .as_ref()
                    .map(|c| c.end)
                    .unwrap_or(root.tag_open.end);
                (pos, pos)
            }
        }
    }

    fn root_children(&self) -> impl Iterator<Item = VueTemplateNodeId> + '_ {
        let topology = self.inputs.topology;
        topology.children(topology.root())
    }

    /// The branch continues an earlier one of its chain.
    fn continues_chain(&self, tid: VueTemplateNodeId) -> bool {
        self.inputs
            .topology
            .element(tid)
            .and_then(|el| el.v_condition.as_ref())
            .is_some_and(|cond| {
                matches!(
                    cond.kind,
                    ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                )
            })
    }

    /// The template's rendered root children after whitespace resolution,
    /// one per `v-if` chain. Comments are among them unless comments are
    /// off.
    fn rendered_roots(&self) -> impl Iterator<Item = VueChildRecord> + '_ {
        let structure = self.inputs.structure;
        let root = structure.compile_id(self.inputs.topology.root());
        structure
            .children(root)
            .iter()
            .copied()
            .filter(|record| record.chain != VueChainRole::Continuation)
    }

    /// Roots that render content: elements (a `v-for` counts as several),
    /// interpolations and non-blank text; comments do not count.
    fn count_effective_roots(&self) -> usize {
        let mut count = 0;
        for child in self.root_children() {
            match self.kind(child) {
                VueTemplateNodeKind::Interpolation => count += 1,
                VueTemplateNodeKind::Text => {
                    if !self.text(child).trim().is_empty() {
                        count += 1;
                    }
                }
                VueTemplateNodeKind::Comment | VueTemplateNodeKind::Root => {}
                _ => {
                    if self.continues_chain(child) {
                        continue;
                    }
                    count += if self.element(child).v_for.is_some() {
                        2
                    } else {
                        1
                    };
                }
            }
        }
        count
    }

    pub(super) fn is_element_like(&self, tid: VueTemplateNodeId) -> bool {
        matches!(
            self.kind(tid),
            VueTemplateNodeKind::Element
                | VueTemplateNodeKind::Component
                | VueTemplateNodeKind::SlotOutlet
                | VueTemplateNodeKind::Template
        )
    }

    /// Enter the template scope a node's own expressions see, so names a
    /// `v-for` or `v-slot` declares there shadow script bindings. An
    /// element's own props see the names its own `v-for` declares.
    fn enter_scope_of(&mut self, node: VueCompileNodeId) {
        let tid = self.tid(node);
        let topology = self.inputs.topology;
        let has_own_loop = topology.element(tid).is_some_and(|el| el.v_for.is_some());
        let scope = match topology.syntax(tid) {
            Some(syntax) if has_own_loop => {
                self.inputs.semantic.expressions().children_scope(syntax)
            }
            _ => self.inputs.semantic.scope_of(tid),
        };
        self.bindings.push_local_scope(scope);
    }

    fn enter_template(&mut self, out: &mut SsrOut<'c>) {
        // `_attrs` falls through to a single rendering root only.
        self.is_multi_root = self.count_effective_roots() > 1;
        // Several rendered roots are wrapped in fragment markers unless all
        // of them are text: a lone comment renders bare, a comment beside an
        // element root is wrapped for hydration.
        let mut roots = 0;
        let mut all_text = true;
        for record in self.rendered_roots() {
            roots += 1;
            all_text &= matches!(
                record.kind,
                VueChildKind::Text
                    | VueChildKind::Interpolation
                    | VueChildKind::WhitespaceSpace
                    | VueChildKind::WhitespaceNewline
            );
        }
        self.needs_fragment = roots > 1 && !all_text;

        if self.needs_fragment {
            let open_end = self.inputs.topology.ast().root.tag_open.end;
            out.prepend_alloc(open_end, "_push(`<!--[-->");
            self.in_push = true;
        }
    }

    fn leave_template(&mut self, out: &mut SsrOut<'c>) {
        let renders_nothing = self.rendered_roots().next().is_none();
        let root_open = &self.inputs.topology.ast().root.tag_open;

        // `$props, $setup, $data, $options` follow whenever binding metadata
        // exists (a script) and the template is not inlined, as in the
        // client render signature.
        self.buf.clear();
        if !self.options.inline && self.options.has_script {
            self.buf.push_str(
                "function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {\n",
            );
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
        // CSS `v-bind()` values reach the HTML through `_cssVars` on root
        // attributes.
        if self.has_css_vars() {
            self.buf.push_str("const _cssVars = { style: {\n");
            let mut written = Vec::new();
            for var in &self.inputs.scope.css_vars {
                if written.contains(&var.var_name.as_str()) {
                    continue;
                }
                if !written.is_empty() {
                    self.buf.push_str(",\n");
                }
                written.push(var.var_name.as_str());
                let resolved = self.bindings.resolve_simple_expr(&var.expression);
                let _ = write!(self.buf, "  \"{}\": ({})", var.var_name, resolved);
            }
            self.buf.push_str("\n} }\n");
        }
        let fn_sig = std::mem::take(&mut self.buf);

        let (close_start, close_end) = self.root_close();
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

        if renders_nothing {
            self.absorb_pending_comment_removals(root_open.start, close_end);
            out.overwrite_segmented(root_open.start, close_end, &fn_sig, &[]);
            out.prepend_static(close_end, "}");
        } else {
            self.absorb_pending_comment_removals(root_open.start, root_open.end);
            out.overwrite_segmented(root_open.start, root_open.end, &fn_sig, &[]);
            self.absorb_pending_comment_removals(close_start, close_end);
            out.overwrite_segmented(close_start, close_end, "}", &[]);
        }
        self.buf = fn_sig;

        // A removal no header range claimed is an ordinary deletion.
        for (start, end) in self.pending_disabled_comment_removals.drain(..) {
            out.overwrite(start, end, "");
        }
    }

    fn enter_element(&mut self, node: VueCompileNodeId, out: &mut SsrOut<'c>) -> VueWalkControl {
        let v = self.visit(node);
        let el = v.el;

        // Non-slot-template children of a component with named slots open
        // the implicit default slot.
        self.open_implicit_default_slot(&v, out);
        // `v-if` chains and `v-for` open their statements first.
        self.enter_control_flow(&v, out);

        if el.tag_type == TagType::Component {
            return self.enter_component(&v, out);
        }
        if el.tag_type == TagType::Template && el.v_slot.is_some() && self.in_component_slots > 0 {
            return self.enter_slot_template(&v, out);
        }
        if el.tag_type == TagType::SlotOutlet {
            return self.enter_slot_outlet(&v, out);
        }
        if el.tag_type == TagType::Template {
            return self.enter_template_fragment(&v, out);
        }
        self.enter_plain_element(&v, out)
    }

    fn leave_element(&mut self, node: VueCompileNodeId, out: &mut SsrOut<'c>) {
        let v = self.visit(node);

        if self.select_v_model_expr.is_some() && v.tag_name == "select" {
            self.select_v_model_expr = None;
        }

        match self.elem_ctx.pop().unwrap_or(ElemCtx::Complete) {
            ElemCtx::Complete => {}
            ElemCtx::InParentPush => self.leave_in_parent_push(&v, out),
            ElemCtx::OwnPush => self.leave_own_push(&v, out),
            ElemCtx::ComponentWithSlots => self.leave_component_with_slots(&v, out),
            ElemCtx::DynamicComponentWithSlots => self.leave_dynamic_component_with_slots(&v, out),
            ElemCtx::SlotTemplate => self.leave_slot_template(&v, out),
            ElemCtx::SuspenseSlotTemplate => self.leave_suspense_slot_template(&v, out),
            ElemCtx::SlotOutletFallback => self.leave_slot_outlet_fallback(&v, out),
            ElemCtx::SuspenseSlots => self.leave_suspense_slots(&v, out),
            ElemCtx::TransparentBuiltin => self.leave_transparent_builtin(&v, out),
            ElemCtx::TransitionGroupTag(tag) => self.leave_transition_group_tag(&v, &tag, out),
            ElemCtx::TeleportBody => self.leave_teleport_body(&v, out),
        }

        self.leave_control_flow(&v, out);
    }
}

#[cfg(test)]
mod tests;
