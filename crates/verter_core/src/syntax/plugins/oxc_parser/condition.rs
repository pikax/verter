use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::syntax::{
    plugins::oxc_parser::helpers::parse_expression,
    types::{OxcElseCondition, OxcElseIfCondition, OxcIfCondition, Prop},
};

/// Parse a v-if condition.
pub fn parse_if_condition<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcIfCondition<'alloc> {
    let (expression, errors, bindings) = if let Some(value_span) = event.value {
        parse_expression(value_span, input, alloc, source_type, ignored)
    } else {
        (None, None, None)
    };

    OxcIfCondition {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        expression,
        errors,
        bindings,
        event,
    }
}

/// Parse a v-else-if condition.
pub fn parse_else_if_condition<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcElseIfCondition<'alloc> {
    let (expression, errors, bindings) = if let Some(value_span) = event.value {
        parse_expression(value_span, input, alloc, source_type, ignored)
    } else {
        (None, None, None)
    };

    OxcElseIfCondition {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        expression,
        errors,
        bindings,
        event,
    }
}

// Parse a v-else condition. Note that v-else has no expression, but we still want to create an OxcElseCondition for it to be consistent with the other conditions and to store the event info.
pub fn parse_else_condition(event: Prop) -> OxcElseCondition {
    OxcElseCondition {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        event,
    }
}
