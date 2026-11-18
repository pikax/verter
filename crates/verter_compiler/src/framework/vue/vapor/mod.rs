//! The Vue fine-grained render target.
mod comment;
#[cfg(test)]
mod comment_tests;
mod component;
#[cfg(test)]
mod component_tests;
mod control_flow;
#[cfg(test)]
mod control_flow_tests;
mod counters;
mod effect;
mod element;
#[cfg(test)]
mod element_tests;
mod events;
#[cfg(test)]
mod events_tests;
mod expr;
#[cfg(test)]
mod expr_tests;
mod graph;
mod interpolation;
#[cfg(test)]
mod interpolation_tests;
mod model;
mod nav;
mod ops;
#[cfg(test)]
mod ops_tests;
mod output;
#[cfg(test)]
mod ported_tests;
mod props;
#[cfg(test)]
mod props_tests;
mod repeated_reads;
#[cfg(test)]
mod repeated_reads_tests;
mod scope;
mod slots;
mod state;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod text;
#[cfg(test)]
mod text_tests;

use rustc_hash::FxHashMap;
use verter_audit::observe::ObserveExec;
use verter_span::Span;

use crate::ast::types::{AstNodeKind, ChildrenFlags, ElementNode};
use crate::compile_request::demand::TargetKind;
use crate::framework::vue::plan::VueTarget;
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::{
    walk_template, VueAttrKind, VueBuiltinDirective, VueChainRole, VueChildKind, VueCompileNodeId,
    VueNodeFlags, VueTemplateVisitor, VueWalkControl,
};
use crate::framework::vue::target::{
    VueHelperSet, VueTargetInputs, VueTargetOutput, VueTargetRefusal,
};
use crate::framework_common::TemplateRenderExport;
use crate::target_plan::TargetExecutor;
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::template::topology::VueTemplateNodeKind;
use crate::work_ledger::CompilerWorkLedger;

use effect::TextPart;
use ops::{Nav, Part, RunId, Stmt};
use scope::{PendingNavRequest, VaporScope};
use state::{VaporFinishError, VaporState};

/// Compiles a template to direct DOM operations and render effects.
pub struct VueVaporExecutor;

impl TargetExecutor for VueVaporExecutor {
    const TARGET: TargetKind = TargetKind::ClientFineGrained;
    type Inputs<'a> = &'a VueTargetInputs<'a>;
    type Output<'a> = VueTargetOutput<'a>;
    type Refusal = VueTargetRefusal;

    fn execute<'a, M: ObserveExec>(
        inputs: Self::Inputs<'a>,
        ledger: &mut CompilerWorkLedger<M>,
    ) -> Result<Self::Output<'a>, Self::Refusal> {
        if inputs.options.inline {
            return Err(VueTargetRefusal::Unsupported {
                code: "inline-fine-grained-render",
                span: inputs.topology.span(inputs.topology.root()),
            });
        }
        let mut compiler = VaporCompiler::new(inputs);
        walk_template(inputs.topology, inputs.structure, &mut compiler);
        compiler.state.effect_graph.record_work(ledger);
        compiler.finish()
    }
}

/// The shared walk owns open scopes; output state owns immutable runs.
struct VaporCompiler<'c> {
    inputs: &'c VueTargetInputs<'c>,
    state: VaporState<'c>,
    resolver: VueBindingView<'c>,
    renames: Vec<FxHashMap<String, String>>,
    element_stack: Vec<VaporScope<'c>>,
    state_pool: Vec<VaporScope<'c>>,
    html: String,
    html_scope_stack: Vec<String>,
    depth: usize,
    block_depth: usize,
    for_scope_depth: u32,
    if_index_counter: u32,
    pending_if: Option<control_flow::IfChain>,
    enclosing_if_chains: Vec<Option<control_flow::IfChain>>,
    construct_refs: Vec<control_flow::ConstructRefs>,
    slot_frames: Vec<bool>,
    template_single_root: bool,
    child_info: Vec<nav::ChildInfo>,
    refusal: Option<VueTargetRefusal>,
}

impl<'c> VaporCompiler<'c> {
    fn new(inputs: &'c VueTargetInputs<'c>) -> Self {
        Self {
            inputs,
            state: VaporState::new(inputs.source.clone(), inputs.options.map_mode),
            resolver: VueBindingView::new(
                inputs.semantic.bindings(),
                false,
                true,
                inputs
                    .options
                    .const_props
                    .as_ref()
                    .map(|props| props.iter().map(String::as_str).collect()),
                inputs.options.ref_bindable_imports.clone(),
            )
            .with_template_scopes(&inputs.semantic.expressions().scopes),
            renames: Vec::new(),
            element_stack: Vec::new(),
            state_pool: Vec::new(),
            html: String::new(),
            html_scope_stack: Vec::new(),
            depth: 0,
            block_depth: 0,
            for_scope_depth: 0,
            if_index_counter: 0,
            pending_if: None,
            enclosing_if_chains: Vec::new(),
            construct_refs: Vec::new(),
            slot_frames: Vec::new(),
            template_single_root: false,
            child_info: nav::child_info(inputs),
            refusal: None,
        }
    }

    fn finish(self) -> Result<VueTargetOutput<'c>, VueTargetRefusal> {
        if let Some(error) = self.refusal {
            return Err(error);
        }
        let mut runtime = self.state.builtins.to_imports();
        runtime.extend(self.state.helpers().to_imports());
        let (render, preamble) = self.state.finish().map_err(|error| match error {
            VaporFinishError::Emission(error) => VueTargetRefusal::Emit(error.into()),
            VaporFinishError::Plan(error) => VueTargetRefusal::Emit(error),
        })?;
        Ok(VueTargetOutput {
            target: VueTarget::Vapor,
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

    fn element_of(
        &self,
        node: VueCompileNodeId,
    ) -> (&'c ElementNode, Option<&'c OxcParsedElement<'c>>) {
        let template = self.inputs.structure.template_id(node);
        (
            self.inputs
                .topology
                .element(template)
                .expect("element payload"),
            self.inputs.semantic.element_exprs(template),
        )
    }

    fn span_of(&self, node: VueCompileNodeId) -> Span {
        self.inputs
            .topology
            .span(self.inputs.structure.template_id(node))
    }

    fn record_expression(
        &mut self,
        text: &str,
        start: u32,
        oxc: Option<&OxcParsedExpression<'c>>,
    ) -> RunId {
        self.record_expression_with(text, start, oxc, self.inputs.options.force_js.into())
    }

    fn record_expression_with(
        &mut self,
        text: &str,
        start: u32,
        oxc: Option<&OxcParsedExpression<'c>>,
        options: expr::RecordOptions,
    ) -> RunId {
        self.record_syntax(text, start, expr::Syntax::Parsed(oxc), options)
    }

    fn record_loop_source(
        &mut self,
        text: &str,
        start: u32,
        parsed: Option<&crate::template::oxc::types::OxcParsedVFor<'c>>,
    ) -> RunId {
        self.record_syntax(
            text,
            start,
            expr::Syntax::LoopSource(parsed),
            self.inputs.options.force_js.into(),
        )
    }

    fn record_syntax(
        &mut self,
        text: &str,
        start: u32,
        syntax: expr::Syntax<'_, 'c>,
        options: expr::RecordOptions,
    ) -> RunId {
        let mut parts = Vec::new();
        expr::record_expr(
            &mut parts,
            self.inputs.allocator,
            text,
            start,
            syntax,
            &expr::VaporBindings {
                view: &self.resolver,
                renames: &self.renames,
            },
            options,
        );
        self.state.runs.commit(&mut parts)
    }

    fn syntax(&self, node: VueCompileNodeId) -> &'c AstNodeKind {
        let template = self.inputs.structure.template_id(node);
        let syntax = self
            .inputs
            .topology
            .syntax(template)
            .expect("visited syntax node");
        &self.inputs.topology.ast().nodes[syntax.0].kind
    }

    fn recycle(&mut self, mut scope: VaporScope<'c>) {
        scope.reset();
        self.state_pool.push(scope);
    }
}

impl<'c> VueTemplateVisitor for VaporCompiler<'c> {
    fn enter_template(&mut self) {
        let root = self
            .inputs
            .structure
            .compile_id(self.inputs.topology.root());
        self.template_single_root = self
            .inputs
            .structure
            .children(root)
            .iter()
            .filter(|child| {
                child.chain != VueChainRole::Continuation
                    && !matches!(
                        child.kind,
                        VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
                    )
            })
            .count()
            == 1;
    }

    fn leave_template(&mut self) {
        if let Err(error) = self.flush_if_chain() {
            self.refusal.get_or_insert(error);
        }
        self.state.effect_graph.finish();
        if self.refusal.is_some() {
            return;
        }
        if let Err(error) = self.state.write_render(self.inputs.allocator) {
            self.refusal = Some(error);
            return;
        }
        self.state.write_preamble();
    }

    fn enter_element(
        &mut self,
        node: VueCompileNodeId,
        kind: VueTemplateNodeKind,
    ) -> VueWalkControl {
        if let Err(error) = self.begin_child(node, false) {
            self.refusal.get_or_insert(error);
        }
        let (element, oxc) = self.element_of(node);
        let block_root = self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_transparent_wrapper);
        self.enclosing_if_chains.push(self.pending_if.take());
        let mut scope = self.state_pool.pop().unwrap_or_default();
        scope.children_all_text_like = self.child_info[node.index()].children_all_text_like;
        // A slot outlet's fallback, like a structural `<template>`, has no
        // DOM container: each of its children is a root of the fallback block.
        let fallback = kind == VueTemplateNodeKind::SlotOutlet
            && element
                .content
                .as_ref()
                .is_some_and(|content| !content.children.is_empty());
        scope.is_transparent_wrapper = fallback
            || (kind == VueTemplateNodeKind::Template
                && element.v_slot.is_none()
                && (element.v_condition.is_some() || element.v_for.is_some()));
        if scope.is_transparent_wrapper && !fallback {
            let roots = self
                .inputs
                .structure
                .children(node)
                .iter()
                .filter(|child| child.chain != VueChainRole::Continuation)
                .count();
            scope.root_template = roots == 1
                && self.template_single_root
                && self.depth == 0
                && element.v_for.is_none();
        }
        scope.is_component_scope = kind == VueTemplateNodeKind::Component
            || (kind == VueTemplateNodeKind::Template && element.v_slot.is_some());
        let builds_open_tag = !scope.is_transparent_wrapper
            && !matches!(
                kind,
                VueTemplateNodeKind::Component | VueTemplateNodeKind::SlotOutlet
            )
            && element.v_slot.is_none();
        let structural = element.v_condition.is_some() || element.v_for.is_some();
        if self.depth == 0 || structural || !builds_open_tag || block_root {
            self.html_scope_stack.push(std::mem::replace(
                &mut self.html,
                String::with_capacity(128),
            ));
        }
        if structural || !builds_open_tag {
            self.block_depth += 1;
        }
        if builds_open_tag {
            element::build_open_tag(element, self.inputs.source.text, &mut self.html);
        }
        let condition_ref = if let Some(condition) = &element.v_condition {
            let outer = matches!(
                condition.kind,
                crate::ast::types::ElementNodeConditionKind::If
            )
            .then(|| self.state.counters.next_node());
            self.state.counters.next_node();
            outer
        } else {
            None
        };
        let loop_ref = if let Some(prop) = &element.v_for {
            let outer = self.state.counters.next_node();
            self.state.counters.next_node();
            let full = match (prop.value_start, prop.value_end) {
                (Some(start), Some(end)) => &self.inputs.source.text[start as usize..end as usize],
                _ => "",
            };
            let (params, _) =
                crate::template::code_gen::shared::helpers::parse_v_for_expression(full);
            let map = control_flow::build_for_scope_map(
                params,
                self.for_scope_depth,
                control_flow::v_for_value_pattern(oxc),
                self.inputs.source.text,
                &self.resolver,
                self.inputs.options.force_js,
                &mut self.state,
            );
            self.renames.push(map);
            self.for_scope_depth += 1;
            Some(outer)
        } else {
            None
        };
        let slot_ref = if kind == VueTemplateNodeKind::SlotOutlet {
            let outer = self.state.counters.next_node();
            if element
                .content
                .as_ref()
                .is_some_and(|content| !content.children.is_empty())
            {
                self.state.counters.next_node();
            }
            Some(outer)
        } else {
            None
        };
        self.construct_refs.push(control_flow::ConstructRefs {
            condition: condition_ref,
            for_loop: loop_ref,
            slot: slot_ref,
            loops_slots: kind == VueTemplateNodeKind::Template && element.v_slot.is_some(),
        });
        let slot_host = matches!(
            kind,
            VueTemplateNodeKind::Template | VueTemplateNodeKind::Component
        );
        let leaves = if slot_host && element.v_slot.is_some() {
            control_flow::v_slot_destructure_leaves(
                oxc,
                self.inputs.source.text,
                &self.resolver,
                self.inputs.options.force_js,
            )
        } else {
            None
        };
        let destructured = leaves.is_some();
        if let Some(leaves) = leaves {
            let base = format!("_slotProps{}", self.for_scope_depth);
            self.for_scope_depth += 1;
            let mut map = FxHashMap::default();
            for (name, accessor) in leaves {
                control_flow::register_destructure_import(&accessor, &mut self.state);
                map.insert(name, accessor.render(&base));
            }
            self.renames.push(map);
        }
        self.slot_frames.push(destructured);
        self.element_stack.push(scope);
        self.depth += 1;
        VueWalkControl::Continue
    }

    fn leave_element(&mut self, node: VueCompileNodeId, kind: VueTemplateNodeKind) {
        if let Err(error) = self.close_text_root(self.span_of(node)) {
            self.refusal.get_or_insert(error);
        }
        let own_index = self.element_stack.len().checked_sub(1);
        if self
            .pending_if
            .as_ref()
            .is_some_and(|chain| chain.target_stack_index == own_index)
        {
            if let Err(error) = self.flush_if_chain() {
                self.refusal.get_or_insert(error);
            }
        }
        self.depth -= 1;
        let (element, _) = self.element_of(node);
        let no_tag = matches!(
            kind,
            VueTemplateNodeKind::Component | VueTemplateNodeKind::SlotOutlet
        ) || (kind == VueTemplateNodeKind::Template && element.v_slot.is_some());
        if no_tag || element.v_condition.is_some() || element.v_for.is_some() {
            self.block_depth -= 1;
        }
        let destructured = self.slot_frames.pop().expect("balanced slot frame stack");
        if destructured {
            self.renames.pop();
            self.for_scope_depth -= 1;
        }
        let construct_refs = self.construct_refs.pop().expect("balanced construct stack");
        let Some(mut scope) = self.element_stack.pop() else {
            return;
        };
        self.pending_if = self
            .enclosing_if_chains
            .pop()
            .expect("balanced condition frames");
        let (element, oxc) = self.element_of(node);
        let source = self.inputs.source.text;
        let template = self.inputs.structure.template_id(node);
        if no_tag {
            self.take_scope_html(&mut scope);
            let result = match kind {
                VueTemplateNodeKind::Component => self
                    .build_component(node, scope, destructured)
                    .and_then(|(root, dynamic)| {
                        self.finish_construct(
                            node,
                            root,
                            if dynamic {
                                scope::MergedConstructKind::DynamicComponent
                            } else {
                                scope::MergedConstructKind::Component
                            },
                            construct_refs,
                        )
                    }),
                VueTemplateNodeKind::SlotOutlet => self
                    .build_slot_outlet(node, scope, construct_refs.slot.expect("slot construct id"))
                    .and_then(|root| {
                        self.finish_construct(
                            node,
                            root,
                            scope::MergedConstructKind::SlotOutlet,
                            construct_refs,
                        )
                    }),
                _ => self.close_named_slot(node, scope, destructured, construct_refs),
            };
            if let Err(error) = result {
                self.refusal.get_or_insert(error);
            }
            return;
        }
        let text_attr = self
            .inputs
            .structure
            .attrs(node)
            .iter()
            .find(|attr| attr.kind == VueAttrKind::Builtin(VueBuiltinDirective::Text))
            .copied();
        let mut has_v_text = false;
        if let Some(attr) = text_attr {
            if let Some(span) = attr.value_span {
                let expression = find_prop_oxc_exp(oxc, attr.prop_index as usize);
                let expr = self.record_expression(
                    &source[span.start as usize..span.end as usize],
                    span.start,
                    expression,
                );
                let wrapped = self.state.runs.commit(&mut vec![
                    Part::Text("_toDisplayString("),
                    Part::Run(expr),
                    Part::Text(")"),
                ]);
                self.html.push(' ');
                scope.text_parts.push(TextPart::Dynamic(wrapped));
                scope.ensure_text_ref(&mut self.state.counters);
                self.state.use_helper(VaporHelper::ToDisplayString);
                has_v_text = true;
            }
        }
        if scope.is_transparent_wrapper {
            self.take_scope_html(&mut scope);
            let dynamic = element.children_flag.has(ChildrenFlags::HasInterpolation);
            let result = self.close_structural_scope(node, scope, dynamic, construct_refs);
            if let Err(error) = result {
                self.refusal.get_or_insert(error);
            }
            return;
        }
        let tag_name = self.inputs.structure.tag_name(node).expect("element tag");
        element::close_html_tag(
            &mut self.html,
            tag_name,
            element.is_self_closing || element.content.is_none(),
        );
        self.process_dynamic_props(node, &mut scope);
        let has_dynamic_text =
            element.children_flag.has(ChildrenFlags::HasInterpolation) || has_v_text;
        if element.v_condition.is_some() || element.v_for.is_some() {
            self.take_scope_html(&mut scope);
            let result = self.close_structural_scope(node, scope, has_dynamic_text, construct_refs);
            if let Err(error) = result {
                self.refusal.get_or_insert(error);
            }
            return;
        }
        element::finalize_text_parts(&mut scope, has_dynamic_text, &mut self.state);
        if let Err(error) = self.resolve_pending_nav_requests(&mut scope) {
            self.refusal.get_or_insert(error);
            return;
        }
        if self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_transparent_wrapper)
        {
            // A wrapper has no DOM container: the element is one of its roots.
            self.take_scope_html(&mut scope);
            let span = self.inputs.topology.span(template);
            let block = self.state.close_scope(
                scope,
                "    ",
                self.element_stack
                    .last()
                    .is_some_and(|parent| parent.root_template),
                false,
                span,
            );
            match block {
                Ok(root) => self
                    .element_stack
                    .last_mut()
                    .expect("enclosing wrapper")
                    .roots
                    .push(root),
                Err(error) => {
                    self.refusal.get_or_insert(error);
                }
            }
        } else if self.depth == 0 {
            scope.once = element.v_once.is_some();
            if let Some(attr) = self
                .inputs
                .structure
                .attrs(node)
                .iter()
                .find(|attr| attr.kind == VueAttrKind::Builtin(VueBuiltinDirective::Memo))
            {
                if let Some(span) = attr.value_span {
                    scope.memo = Some(self.record_expression_with(
                        &source[span.start as usize..span.end as usize],
                        span.start,
                        find_prop_oxc_exp(oxc, attr.prop_index as usize),
                        expr::RecordOptions {
                            force_js: self.inputs.options.force_js,
                            resolution: expr::Resolution::Verbatim,
                            provenance: expr::ExprProvenance::Authored,
                        },
                    ));
                }
            }
            self.take_scope_html(&mut scope);
            match self.close_block(
                scope,
                has_dynamic_text,
                "  ",
                false,
                false,
                self.inputs.topology.span(template),
            ) {
                Ok(root) => self.state.roots.push(root),
                Err(error) => {
                    self.refusal.get_or_insert(error);
                }
            }
        } else if self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_component_scope)
            && self.is_sole_meaningful_child(node)
        {
            let node_ref = scope.ensure_node_ref(&mut self.state.counters);
            let parent = self.element_stack.last_mut().expect("component scope");
            parent.node_ref = Some(node_ref);
            parent.own_effects.append(&mut scope.own_effects);
            parent.child_effects.append(&mut scope.child_effects);
            parent.child_nav.append(&mut scope.child_nav);
            parent
                .child_text_creations
                .append(&mut scope.child_text_creations);
            parent.text_node_ref = scope.text_node_ref;
            parent.text_ref_generated = scope.text_ref_generated;
            parent.child_statements.append(&mut scope.child_statements);
            self.recycle(scope);
        } else if let Some(parent) = self.element_stack.last_mut() {
            let index = parent.observe_dom_element();
            let navigated = has_dynamic_text
                || !scope.own_effects.is_empty()
                || !scope.child_effects.is_empty();
            let scope = element::merge_into_parent(
                scope,
                parent,
                &mut self.state.counters,
                index,
                has_dynamic_text,
            );
            let scope = self.bubble_structural_content_into_parent(scope, index, navigated);
            self.recycle(scope);
        }
    }

    fn visit_text(&mut self, node: VueCompileNodeId) {
        let Some(kind) = self.child_info[node.index()].kind else {
            return;
        };
        let AstNodeKind::Text(text_node) = self.syntax(node) else {
            return;
        };
        if let Err(error) = self.begin_child(node, true) {
            self.refusal.get_or_insert(error);
        }
        let content = &self.inputs.source.text[text_node.start as usize..text_node.end as usize];
        let Some(parent) = self.element_stack.last_mut() else {
            return;
        };
        if parent.observe_dom_text_run() {
            parent.text_run_html_start = Some(self.html.len());
            parent.text_run_has_dynamic = false;
        }
        let template = self.inputs.structure.template_id(node);
        let has_interpolation = self
            .inputs
            .topology
            .parent(template)
            .and_then(|parent| self.inputs.topology.element(parent))
            .is_some_and(|parent| parent.children_flag.has(ChildrenFlags::HasInterpolation));
        let content = if matches!(
            kind,
            VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
        ) {
            " "
        } else {
            content
        };
        let write_html = !parent.text_run_has_dynamic;
        text::process_text(
            content,
            &mut self.html,
            parent,
            has_interpolation,
            write_html,
            self.inputs.allocator,
            self.inputs
                .structure
                .facts(node)
                .flags
                .has(VueNodeFlags::PRESERVE_WHITESPACE),
        );
    }

    fn visit_interpolation(&mut self, node: VueCompileNodeId) {
        if self.child_info[node.index()].kind.is_none() {
            return;
        }
        let AstNodeKind::Interpolation(interp) = self.syntax(node) else {
            return;
        };
        let template = self.inputs.structure.template_id(node);
        let Some(expression) = self.inputs.semantic.interpolation(template) else {
            return;
        };
        if let Err(error) = self.begin_child(node, true) {
            self.refusal.get_or_insert(error);
        }
        let Some(parent) = self.element_stack.last_mut() else {
            return;
        };
        if parent.observe_dom_text_run() {
            parent.text_run_html_start = Some(self.html.len());
            parent.text_run_has_dynamic = false;
        }
        if !parent.text_run_has_dynamic {
            if let Some(start) = parent.text_run_html_start {
                self.html.truncate(start);
            }
            self.html.push(' ');
            parent.text_run_has_dynamic = true;
        }
        if !parent.children_all_text_like {
            // The run is the child the cursor has just passed.
            let dom_child_index = parent.dom_child_cursor - 1;
            if let Some(own_ref) = parent.reserve_nav_text_ref(&mut self.state.counters) {
                let nav_slot = parent.child_nav.len();
                parent.child_nav.push(Nav::Reserved);
                let stmt_slot = parent.child_statements.len();
                parent.child_statements.push(Stmt::Reserved);
                parent
                    .pending_nav_requests
                    .push(PendingNavRequest::TextRef {
                        own_ref,
                        dom_child_index,
                        nav_slot,
                        stmt_slot,
                    });
            }
        }
        interpolation::process_interpolation(self, interp, expression);
    }

    fn visit_comment(&mut self, node: VueCompileNodeId) {
        if self.child_info[node.index()].kind.is_none() {
            return;
        }
        let AstNodeKind::Comment(comment_node) = self.syntax(node) else {
            return;
        };
        if let Err(error) = self.begin_child(node, false) {
            self.refusal.get_or_insert(error);
        }
        if self
            .element_stack
            .last()
            .is_some_and(|parent| parent.is_transparent_wrapper)
        {
            if self.inputs.options.comments {
                if let Err(error) = self.push_comment_root(comment_node, self.span_of(node)) {
                    self.refusal.get_or_insert(error);
                }
            }
            return;
        }
        if let Some(parent) = self.element_stack.last_mut() {
            if self.inputs.options.comments {
                parent.observe_dom_comment();
            }
            comment::process_comment(
                comment_node,
                self.inputs.source.text,
                self.inputs.options.comments,
                &mut self.html,
            );
        }
    }
}
