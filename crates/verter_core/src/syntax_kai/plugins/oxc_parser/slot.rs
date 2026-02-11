use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::{syntax_kai::types::{OxcVSlotTemplate, Prop}, utils::oxc::vue::parse_vslot_with_bindings};

/// Parse a v-slot on a template element.
pub fn parse_vslot_template<'alloc>(
    event: Prop,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &'alloc Vec<&[u8]>,
) -> Option<OxcVSlotTemplate<'alloc>> {
    let value_span = event.value;
    let source_slice: Option<&_> = value_span.map(|s| &input[s.start as usize..s.end as usize]);

    let mut parsed = if let Some(slice) = source_slice {
        parse_vslot_with_bindings(alloc, slice, source_type)
    } else {
        parse_vslot_with_bindings(alloc, "", source_type)
    };

    // Adjust spans
    let offset = value_span.map_or(0, |s| s.start);
    for s in &mut parsed.locals {
        s.start += offset;
        s.end += offset;
    }
    for s in &mut parsed.references {
        s.start += offset;
        s.end += offset;
    }

    Some(OxcVSlotTemplate {
        element_id,
        start: prop.start,
        end: prop.end,
        parsed,
        event: ElementScopeSlotTemplate {
            element_start: element_id,
            start: prop.start,
            end: prop.end,
            name: prop.arg,
        },
    })
}

/// Parse a v-slot on a component element (not template).
pub fn parse_vslot_element(
    &self,
    prop: &Prop,
    element_id: u32,
    open_tag_end: &ElementOpenTagEnd,
    ctx: &SyntaxPluginContext<'alloc>,
) -> Option<OxcVSlotElement<'alloc>> {
    let value_span = prop.value;
    let source_slice = value_span.map(|s| &ctx.input[s.start as usize..s.end as usize]);

    let mut parsed = if let Some(slice) = source_slice {
        parse_vslot_with_bindings(self.alloc, slice, self.source_type)
    } else {
        parse_vslot_with_bindings(self.alloc, "", self.source_type)
    };

    let offset = value_span.map_or(0, |s| s.start);
    for s in &mut parsed.locals {
        s.start += offset;
        s.end += offset;
    }
    for s in &mut parsed.references {
        s.start += offset;
        s.end += offset;
    }

    Some(OxcVSlotElement {
        element_id,
        start: prop.start,
        end: prop.end,
        parsed,
        event: ElementScopeSlotElement {
            element_start: element_id,
            element_content_start: open_tag_end.end,
            start: prop.start,
            end: prop.end,
            name: prop.arg,
        },
    })
}
