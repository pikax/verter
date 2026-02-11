use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::{
    syntax_kai::types::{OxcVSlotElement, OxcVSlotTemplate, Prop},
    utils::oxc::vue::{parse_vslot_with_bindings, parse_vslot_with_bindings_sliced},
};

/// Parse a v-slot on a template element.
pub fn parse_vslot_template<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> Option<OxcVSlotTemplate<'alloc>> {
    let value_span = event.value;

    let parsed = if value_span.is_some() {
        parse_vslot_with_bindings_sliced(alloc, value_span, input, source_type, ignored)
    } else {
        parse_vslot_with_bindings(alloc, "", source_type, ignored)
    };

    Some(OxcVSlotTemplate {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        parsed,
        event,
    })
}

/// Parse a v-slot on a component element (not template).
pub fn parse_vslot_element<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> Option<OxcVSlotElement<'alloc>> {
    let value_span = event.value;

    let parsed = if value_span.is_some() {
        parse_vslot_with_bindings_sliced(alloc, value_span, input, source_type, ignored)
    } else {
        parse_vslot_with_bindings(alloc, "", source_type, ignored)
    };

    Some(OxcVSlotElement {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        parsed,
        event,
    })
}
