use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::syntax_kai::{
    plugins::oxc_parser::{
        condition::{parse_else_condition, parse_else_if_condition, parse_if_condition},
        helpers::parse_expression,
        slot::{parse_vslot_element, parse_vslot_template},
        v_for::parse_vfor,
    },
    types::{
        CompiledElementStart, ElementKind, ElementScope, OxcCompiledElementStart, OxcProp,
        OxcPropProcessed, Prop, PropKind,
    },
};

/// Parse props from a CompiledElementStart into OxcProp and ElementScope vectors.
pub fn parse_element_props<'alloc>(
    event: CompiledElementStart,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcCompiledElementStart<'alloc> {
    let mut condition_if_scope = None;
    let mut condition_else_if_scope = None;
    let mut condition_else_scope = None;
    let mut for_scope = None;
    let mut slot_scope = None;
    let mut template_scope = None;

    let mut provided_bindings = Vec::with_capacity(5);

    let is_template = event.event_open_tag.kind == ElementKind::Template;
    // borrow props
    let props = &event.props;

    let oxc_props = props
        .into_iter()
        .filter_map(|prop| match prop.kind {
            PropKind::If => {
                let scope = parse_if_condition(prop.clone(), input, alloc, source_type, ignored);
                // TODO add a check if we have already a condition, if we do should log an error
                condition_if_scope = Some(scope);

                None
            }
            PropKind::ElseIf => {
                let scope =
                    parse_else_if_condition(prop.clone(), input, alloc, source_type, ignored);
                // TODO add a check if we have already a condition, if we do should log an error
                condition_else_if_scope = Some(scope);
                None
            }
            PropKind::Else => {
                let scope = parse_else_condition(prop.clone());
                // TODO add a check if we have already a condition, if we do should log an error
                condition_else_scope = Some(scope);
                None
            }
            PropKind::For => {
                // TODO add a check if we have already a for, if we do should log an error
                let scope = parse_vfor(prop.clone(), input, alloc, source_type, ignored);

                if let Some(scope) = &scope {
                    provided_bindings.extend(scope.parsed.locals.iter());
                }

                for_scope = scope;

                None
            }

            PropKind::Slot => {
                // TODO add a check if we have already a slot, if we do should log an error
                if is_template {
                    let scope =
                        parse_vslot_template(prop.clone(), input, alloc, source_type, ignored);

                    if let Some(scope) = &scope {
                        provided_bindings.extend(scope.parsed.locals.iter());
                    }
                    template_scope = scope;
                } else {
                    let scope =
                        parse_vslot_element(prop.clone(), input, alloc, source_type, ignored);

                    if let Some(scope) = &scope {
                        provided_bindings.extend(scope.parsed.locals.iter());
                    }
                    slot_scope = scope;
                }
                None
            }
            _ => Some(parse_prop(prop.clone(), input, alloc, source_type, ignored)),
        })
        .collect();

    let mut scopes: Vec<ElementScope<'_>> = Vec::with_capacity(3);
    if let Some(condition_if_scope) = condition_if_scope {
        scopes.push(ElementScope::If(condition_if_scope));
    }
    if let Some(condition_else_if_scope) = condition_else_if_scope {
        scopes.push(ElementScope::ElseIf(condition_else_if_scope));
    }
    if let Some(condition_else_scope) = condition_else_scope {
        scopes.push(ElementScope::Else(condition_else_scope));
    }
    if let Some(for_scope) = for_scope {
        scopes.push(ElementScope::For(for_scope));
    }
    if let Some(slot_scope) = slot_scope {
        scopes.push(ElementScope::SlotElement(slot_scope));
    }
    if let Some(template_scope) = template_scope {
        scopes.push(ElementScope::SlotTemplate(template_scope));
    }

    OxcCompiledElementStart {
        props: oxc_props,
        scopes,
        event,
        provided_bindings,
    }
}

/// Parse a single prop's value and arg expressions.
fn parse_prop<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcProp<'alloc> {
    let arg = if let Some(arg_span) = event.arg {
        if event.has_dynamic_arg {
            // Dynamic arg: :[key]="value" — parse the arg expression
            let (expression, errors, bindings) =
                parse_expression(arg_span, input, alloc, source_type, ignored);
            Some(OxcPropProcessed {
                start: arg_span.start,
                end: arg_span.end,
                expression,
                errors,
                bindings,
            })
        } else {
            // Static arg: :prop="value" — no parsing needed, just a span
            None
        }
    } else {
        None
    };

    let exp = if let Some(value_span) = event.value {
        if event.is_directive {
            // Directive value is an expression — parse it
            let (expression, errors, bindings) =
                parse_expression(value_span, input, alloc, source_type, ignored);
            Some(OxcPropProcessed {
                start: value_span.start,
                end: value_span.end,
                expression,
                errors,
                bindings,
            })
        } else {
            // Static attribute value — no parsing needed
            None
        }
    } else {
        None
    };

    OxcProp {
        element_id: event.element_id,
        start: event.start,
        name_end: event.name_end,
        arg,
        exp,
        // note maybe we could access straight from the event to avoid cloning, but this is simpler for now and we can optimize later if needed
        modifiers: event.modifiers.clone(),
        event,
    }
}
