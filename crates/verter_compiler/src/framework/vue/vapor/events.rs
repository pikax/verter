//! Event statements retain authored handler classification and expression provenance.
use super::ops::{NameKind, Part, RunBuf, RunId, Stmt};
use super::scope::VaporScope;
use super::VaporCompiler;
use crate::framework::vue::structure::{VueAttr, VueCompileNodeId};
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::code_gen::shared::helpers::{VaporHelper, DELEGATABLE_EVENTS};
use crate::template::oxc::types::OxcParsedExpression;
use crate::template::vue_syntax::{
    escape_js_string_into, is_function_expression_value, is_handler_reference,
    is_multi_statement_handler, references_event_param, trim_handler_body,
};
use crate::types::BindingType;

const RUNTIME_MODIFIERS: &[&str] = &[
    "stop", "prevent", "self", "ctrl", "shift", "alt", "meta", "left", "middle", "right", "exact",
];
const KEY_MODIFIERS: &[&str] = &[
    "enter", "tab", "delete", "esc", "space", "up", "down", "left", "right",
];

impl<'c> VaporCompiler<'c> {
    pub(super) fn quoted(&self, value: &str) -> &'c str {
        let mut text = String::from("\"");
        escape_js_string_into(&mut text, value);
        text.push('"');
        self.inputs.allocator.alloc_str(&text)
    }

    pub(super) fn process_event(
        &mut self,
        node: VueCompileNodeId,
        attr: &VueAttr,
        scope: &mut VaporScope<'c>,
    ) {
        let (Some(arg), Some(value)) = (attr.arg_span, attr.value_span) else {
            return;
        };
        let source = self.inputs.source.text;
        let event = &source[arg.start as usize..arg.end as usize];
        let (element, oxc) = self.element_of(node);
        let prop = &element.props[attr.prop_index as usize];
        let parsed = find_prop_oxc_exp(oxc, attr.prop_index as usize);
        let mut runtime = Vec::new();
        let mut keys = Vec::new();
        let mut options = Vec::new();
        let mut delegate = false;
        for modifier in &prop.modifiers {
            let name = &source[modifier.start as usize..modifier.end as usize];
            match name {
                "capture" | "passive" | "once" => options.push(name),
                "delegate" => delegate = true,
                m if RUNTIME_MODIFIERS.contains(&m) => runtime.push(m),
                m if KEY_MODIFIERS.contains(&m) => keys.push(m),
                _ => {}
            }
        }
        let handler = self.record_handler(
            &source[value.start as usize..value.end as usize],
            value.start,
            parsed,
            &runtime,
            &keys,
        );
        let node_ref = scope.ensure_node_ref(&mut self.state.counters);
        let mut parts = Vec::new();
        if delegate
            && options.is_empty()
            && !attr.dynamic_arg
            && DELEGATABLE_EVENTS.contains(&event)
        {
            parts.extend([
                Part::Name(NameKind::Node, node_ref),
                Part::Text(".$evt"),
                Part::Text(event),
                Part::Text(" = _createInvoker("),
                Part::Run(handler),
                Part::Text(")"),
            ]);
            self.state.delegate_event(event);
            self.state.use_helper(VaporHelper::DelegateEvents);
            self.state.use_helper(VaporHelper::CreateInvoker);
        } else {
            parts.extend([
                Part::Text("_on("),
                Part::Name(NameKind::Node, node_ref),
                Part::Text(", "),
                Part::Text(self.quoted(event)),
                Part::Text(", "),
                Part::Run(handler),
            ]);
            record_options(&mut parts, &options);
            parts.push(Part::Text(")"));
            self.state.use_helper(VaporHelper::On);
        }
        scope
            .child_statements
            .push(Stmt::Run(self.state.runs.commit(&mut parts)));
    }

    pub(super) fn record_handler(
        &mut self,
        handler: &str,
        start: u32,
        parsed: Option<&OxcParsedExpression<'c>>,
        runtime: &[&'c str],
        keys: &[&'c str],
    ) -> RunId {
        let mut parts = Vec::new();
        if !keys.is_empty() {
            parts.push(Part::Text("_withKeys("));
            self.state.use_helper(VaporHelper::WithKeys);
        }
        if !runtime.is_empty() {
            parts.push(Part::Text("_withModifiers("));
            self.state.use_helper(VaporHelper::WithModifiers);
        }
        if is_handler_reference(handler, parsed) {
            let constant = !handler.contains('.')
                && self.resolver.get(handler) == Some(BindingType::SetupConst);
            if !constant {
                parts.push(Part::Text("e => "));
            }
            parts.push(Part::Run(self.record_expression(handler, start, parsed)));
            if !constant {
                parts.push(Part::Text("(e)"));
            }
        } else if is_function_expression_value(handler, parsed) {
            // A function value is already the handler.
            parts.push(Part::Run(self.record_expression(
                trim_handler_body(handler),
                start,
                parsed,
            )));
        } else {
            let trimmed = trim_handler_body(handler);
            if trimmed.is_empty() {
                parts.push(Part::Text("$event => {}"));
            } else {
                parts.push(Part::Text(if references_event_param(trimmed) {
                    "$event"
                } else {
                    "()"
                }));
                let statements = is_multi_statement_handler(parsed);
                parts.push(Part::Text(if statements { " => { " } else { " => (" }));
                parts.push(Part::Run(self.record_expression(trimmed, start, parsed)));
                parts.push(Part::Text(if statements { " }" } else { ")" }));
            }
        }
        for modifiers in [runtime, keys] {
            if !modifiers.is_empty() {
                parts.push(Part::Text(", ["));
                for (index, modifier) in modifiers.iter().enumerate() {
                    if index > 0 {
                        parts.push(Part::Text(", "));
                    }
                    parts.push(Part::Text(self.quoted(modifier)));
                }
                parts.push(Part::Text("])"));
            }
        }
        self.state.runs.commit(&mut parts)
    }
}

pub(super) fn record_options<'c>(parts: &mut RunBuf<'c>, options: &[&'c str]) {
    if options.is_empty() {
        return;
    }
    parts.push(Part::Text(", { "));
    for (index, option) in options.iter().enumerate() {
        if index > 0 {
            parts.push(Part::Text(", "));
        }
        parts.extend([Part::Text(option), Part::Text(": true")]);
    }
    parts.push(Part::Text(" }"));
}
