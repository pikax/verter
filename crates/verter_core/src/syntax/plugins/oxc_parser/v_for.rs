use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::{
    syntax::types::{OxcVFor, Prop},
    utils::oxc::vue::{parse_vfor_with_bindings, parse_vfor_with_bindings_sliced},
};

/// Parse a v-for directive.
pub fn parse_vfor<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> Option<OxcVFor<'alloc>> {
    let value_span = event.value;

    let parsed = if let Some(value_span) = value_span {
        parse_vfor_with_bindings_sliced(alloc, value_span, input, source_type, ignored)
    } else {
        parse_vfor_with_bindings(alloc, "", source_type, ignored)
    };

    Some(OxcVFor {
        element_id: event.element_id,
        start: event.start,
        end: event.end,
        parsed,
        event,
    })
}
