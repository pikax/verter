//! Slot content written as virtual nodes, for the client branch of a server
//! render function's slot functions.
//!
//! The same state and walk as the render function, over the content's
//! subtrees only, with hoisting and caching off: a server render function
//! has no module preamble and no `_cache`.

use verter_audit::observe::ObserveExec;
use verter_span::Span;

use super::children;
use super::state::VdomState;
use super::VueVdomExecutor;
use crate::ast::types::TagType;
use crate::code_transform::CodeTransform;
use crate::emission::{EmissionBuilder, EmissionFragment};
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::{
    walk_template, VueCompileNodeId, VueTemplateVisitor, VueWalkControl,
};
use crate::framework::vue::target::{VueTargetInputs, VueTargetRefusal};
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::types::NodeId;

/// One slot's content as virtual nodes.
#[derive(Debug)]
pub(crate) struct VdomSlotContent<'c> {
    /// The array expression, `[…]`.
    pub array: EmissionFragment<'c>,
    /// Runtime helpers the array calls, in import order.
    pub helpers: Vec<&'static str>,
    /// Components the array resolves by name, including the snapshot's
    /// self-reference flag; the caller emits their declarations.
    pub resolved_components: Vec<super::component::ResolvedComponent>,
}

impl VueVdomExecutor {
    /// The content of the slot `owner` provides, written as virtual nodes:
    /// the children of a `<template v-slot>`, or a component's children
    /// outside its `<template v-slot>`s (its implicit default slot).
    pub(crate) fn render_slot_content<'c, M: ObserveExec>(
        inputs: &'c VueTargetInputs<'c>,
        owner: VueTemplateNodeId,
    ) -> Result<VdomSlotContent<'c>, VueTargetRefusal> {
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
        state.options.hoist_static = false;
        state.options.cache_handlers = false;
        state.options.element_vnodes = false;
        state.options.array_separator = ",\n";
        state.options.trim_interpolations = true;
        state.options.branches_on_lines = true;
        state.hoisting = false;
        let topology = inputs.topology;
        let structure = inputs.structure;
        let source = inputs.source.text;

        let is_slot_template = |node: VueTemplateNodeId| {
            topology
                .element(node)
                .is_some_and(|el| el.tag_type == TagType::Template && el.v_slot.is_some())
        };
        let default_of_component = topology.kind(owner) == VueTemplateNodeKind::Component;
        let in_content =
            |node: VueTemplateNodeId| !(default_of_component && is_slot_template(node));

        let resolved: Vec<_> = structure
            .children(structure.compile_id(owner))
            .iter()
            .filter(|child| in_content(structure.template_id(child.node)))
            .copied()
            .collect();
        let (Some(first), Some(last)) = (resolved.first(), resolved.last()) else {
            let mut array =
                EmissionBuilder::new(options.map_mode).with_source(inputs.source.clone());
            array.push_borrowed("[]");
            return Ok(VdomSlotContent {
                array: array
                    .finish()
                    .map_err(|error| VueTargetRefusal::Emit(error.into()))?,
                helpers: Vec::new(),
                resolved_components: Vec::new(),
            });
        };
        let window = Span::new(
            topology.span(structure.template_id(first.node)).start,
            topology.span(structure.template_id(last.node)).end,
        );

        let mut scope = vec![false; topology.len()];
        for child in topology.children(owner).filter(|&child| in_content(child)) {
            for node in topology.subtree(child) {
                scope[node.index()] = true;
            }
        }

        let mut out = state.take_out();
        state.settle_children(&mut out, Some((owner, window)));
        // A named slot between the default content's children is not part
        // of it.
        for child in topology.children(owner).filter(|&child| !in_content(child)) {
            let span = topology.span(child);
            if span.start >= window.start && span.end <= window.end {
                out.overwrite(span.start, span.end, "");
            }
        }
        state.restore_out(out);

        walk_template(
            topology,
            structure,
            &mut ContentVisitor {
                state: &mut state,
                scope: &scope,
            },
        );

        let ast = state.ast;
        let named_slot_starts: Vec<u32> = topology
            .children(owner)
            .filter(|&child| !in_content(child))
            .map(|child| topology.span(child).start)
            .collect();
        let ast_children: Vec<NodeId> = topology
            .children(owner)
            .filter(|&child| in_content(child))
            .filter_map(|child| topology.syntax(child))
            .collect();
        let records: Vec<_> = state
            .build_child_records(&ast_children, source)
            .into_iter()
            .filter(|record| record.start >= window.start && record.end <= window.end)
            .filter(|record| !named_slot_starts.contains(&record.start))
            .collect();
        let mut out = state.take_out();
        children::add_children_separators_array(
            &records,
            &mut out,
            &state.options,
            source,
            ast,
            &ast_children,
        );
        let (mut edits, helpers) = state.finish_out(out);
        if options.force_js {
            // Slot arrays freeze independently of the main render function.
            // Record type removals on this fragment's own source window.
            for expr in inputs.semantic.expressions().iter_expressions() {
                if expr.offset < window.start || expr.offset >= window.end {
                    continue;
                }
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
            window,
            ast_children.len(),
            inputs.allocator,
            options.map_mode,
        )
        .map_err(VueTargetRefusal::Edit)?;
        edits
            .apply(&mut transform)
            .map_err(VueTargetRefusal::Edit)?;
        transform.prepend("[\n");
        transform.append("\n]");
        let array = transform
            .freeze(inputs.source.clone())
            .map_err(VueTargetRefusal::Edit)?;
        Ok(VdomSlotContent {
            array,
            helpers,
            resolved_components: std::mem::take(&mut state.resolved_components),
        })
    }
}

/// The walk of the template, passed on for the nodes of the slot content
/// only.
struct ContentVisitor<'s, 'c> {
    state: &'s mut VdomState<'c>,
    /// Topology nodes inside the content, by index.
    scope: &'s [bool],
}

impl ContentVisitor<'_, '_> {
    fn in_scope(&self, node: VueCompileNodeId) -> bool {
        let id = self.state.inputs.structure.template_id(node);
        self.scope.get(id.index()).copied().unwrap_or(false)
    }
}

impl VueTemplateVisitor for ContentVisitor<'_, '_> {
    fn enter_template(&mut self) {}

    fn leave_template(&mut self) {}

    fn enter_element(
        &mut self,
        node: VueCompileNodeId,
        kind: VueTemplateNodeKind,
    ) -> VueWalkControl {
        if self.in_scope(node) {
            self.state.enter_element(node, kind)
        } else {
            VueWalkControl::Continue
        }
    }

    fn leave_element(&mut self, node: VueCompileNodeId, kind: VueTemplateNodeKind) {
        if self.in_scope(node) {
            self.state.leave_element(node, kind);
        }
    }

    fn visit_text(&mut self, node: VueCompileNodeId) {
        if self.in_scope(node) {
            self.state.visit_text(node);
        }
    }

    fn visit_interpolation(&mut self, node: VueCompileNodeId) {
        if self.in_scope(node) {
            self.state.visit_interpolation(node);
        }
    }

    fn visit_comment(&mut self, node: VueCompileNodeId) {
        if self.in_scope(node) {
            self.state.visit_comment(node);
        }
    }
}
