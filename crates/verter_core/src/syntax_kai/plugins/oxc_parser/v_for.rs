
/// Parse a v-for directive.
pub fn parse_vfor(
    &self,
    prop: &Prop,
    element_id: u32,
    ctx: &SyntaxPluginContext<'alloc>,
) -> Option<OxcVFor<'alloc>> {
    let value_span = prop.value?;
    let source_slice = &ctx.input[value_span.start as usize..value_span.end as usize];

    let mut parsed = parse_vfor_with_bindings(self.alloc, source_slice, self.source_type);

    // Adjust spans to be relative to original source
    for s in &mut parsed.locals {
        s.start += value_span.start;
        s.end += value_span.start;
    }
    for s in &mut parsed.references {
        s.start += value_span.start;
        s.end += value_span.start;
    }

    Some(OxcVFor {
        element_id,
        start: prop.start,
        end: prop.end,
        parsed,
        event: ElementScopeFor {
            element_start: element_id,
            start: prop.start,
            end: prop.end,
            value: prop.value,
            iterator: None,
            iterable: None,
            is_of: false,
        },
    })
}
