//! Native model and template-ref operations share recorded expression runs.
use super::events::record_options;
use super::ops::{NameKind, Part, RunId, Stmt};
use super::scope::VaporScope;
use super::VaporCompiler;
use crate::framework::vue::structure::{VueAttr, VueAttrKind, VueCompileNodeId};
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::code_gen::shared::helpers::VaporHelper;

impl<'c> VaporCompiler<'c> {
    pub(super) fn process_model(
        &mut self,
        node: VueCompileNodeId,
        attr: &VueAttr,
        scope: &mut VaporScope<'c>,
    ) {
        let (element, oxc) = self.element_of(node);
        let source = self.inputs.source.text;
        let tag = self.inputs.structure.tag_name(node).expect("model element");
        let input_type = self
            .inputs
            .structure
            .attrs(node)
            .iter()
            .find(|attr| {
                attr.kind == VueAttrKind::Static
                    && &source[attr.name_span.start as usize..attr.name_span.end as usize] == "type"
            })
            .and_then(|attr| attr.value_span)
            .map(|span| &source[span.start as usize..span.end as usize]);
        let (helper, call) = match (tag, input_type) {
            ("select", _) => (VaporHelper::ApplySelectModel, "_applySelectModel("),
            ("input", Some("checkbox")) => {
                (VaporHelper::ApplyCheckboxModel, "_applyCheckboxModel(")
            }
            ("input", Some("radio")) => (VaporHelper::ApplyRadioModel, "_applyRadioModel("),
            _ => (VaporHelper::ApplyTextModel, "_applyTextModel("),
        };
        let value = attr.value_span;
        let expr = self.record_expression(
            value.map_or("", |span| &source[span.start as usize..span.end as usize]),
            value.map_or(0, |span| span.start),
            find_prop_oxc_exp(oxc, attr.prop_index as usize),
        );
        let node_ref = scope.ensure_node_ref(&mut self.state.counters);
        let mut parts = vec![
            Part::Text(call),
            Part::Name(NameKind::Node, node_ref),
            Part::Text(", () => ("),
            Part::Run(expr),
            Part::Text("), _value => ("),
            Part::Run(expr),
            Part::Text(" = _value)"),
        ];
        let modifiers: Vec<_> = element.props[attr.prop_index as usize]
            .modifiers
            .iter()
            .map(|modifier| &source[modifier.start as usize..modifier.end as usize])
            .filter(|name| matches!(*name, "trim" | "number" | "lazy"))
            .collect();
        record_options(&mut parts, &modifiers);
        parts.push(Part::Text(")"));
        scope
            .child_statements
            .push(Stmt::Run(self.state.runs.commit(&mut parts)));
        self.state.use_helper(helper);
    }

    pub(super) fn process_template_ref(
        &mut self,
        node: VueCompileNodeId,
        scope: &mut VaporScope<'c>,
    ) {
        if self.static_template_ref(node).is_some() {
            let node_ref = scope.ensure_node_ref(&mut self.state.counters);
            let run = self
                .template_ref_statement(node, node_ref)
                .expect("static template ref");
            scope.child_statements.push(Stmt::Run(run));
        }
    }

    /// The name a static `ref="name"` attribute gives the element or component.
    fn static_template_ref(&self, node: VueCompileNodeId) -> Option<&'c str> {
        let (element, _) = self.element_of(node);
        let prop = element.v_ref.as_ref()?;
        let (start, end) = prop.value_start.zip(prop.value_end)?;
        Some(&self.inputs.source.text[start as usize..end as usize])
    }

    /// The statement that registers a static template ref on `node_ref`, as
    /// Vue's `genSetTemplateRef` writes it: outside a loop the imported
    /// `setStaticTemplateRef`; inside one, the render's own setter in its
    /// `ref_for` form, which collects every row's node into an array.
    pub(super) fn template_ref_statement(
        &mut self,
        node: VueCompileNodeId,
        node_ref: u32,
    ) -> Option<RunId> {
        let name = self.static_template_ref(node)?;
        let (element, _) = self.element_of(node);
        let in_loop = element.v_for.is_some()
            || self
                .construct_refs
                .iter()
                .any(|refs| refs.for_loop.is_some() && !refs.loops_slots);
        let quoted = self.quoted(name);
        let mut parts = vec![
            Part::Text(if in_loop {
                "_setTemplateRef("
            } else {
                "_setStaticTemplateRef("
            }),
            Part::Name(NameKind::Node, node_ref),
            Part::Text(", "),
            Part::Text(quoted),
            Part::Text(if in_loop { ", true)" } else { ")" }),
        ];
        if in_loop {
            self.state.declare_template_ref_setter();
        } else {
            self.state.use_helper(VaporHelper::SetStaticTemplateRef);
        }
        Some(self.state.runs.commit(&mut parts))
    }
}
