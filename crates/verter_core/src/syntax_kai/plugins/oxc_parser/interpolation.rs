use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::syntax_kai::{
    plugins::oxc_parser::helpers::parse_expression,
    types::{Interpolation, OxcInterpolation},
};

/// Parse an interpolation expression.
pub fn parse_interpolation<'alloc>(
    event: Interpolation,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: SourceType,
    ignored: &[&'alloc str],
) -> OxcInterpolation<'alloc> {
    let (expression, errors, bindings) =
        parse_expression(event.content, input, alloc, source_type, ignored);

    OxcInterpolation {
        parent_id: event.parent_id,
        start: event.start,
        end: event.end,
        content: event.content,
        expression,
        errors,
        bindings,
        event,
    }
}
