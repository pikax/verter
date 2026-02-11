use oxc_allocator::Allocator;

use crate::{
    common::Span,
    utils::oxc::{
        extract_bindings_from_expression,
        vue::{adjust_diagnostics_spans, adjust_expression_spans, adjust_program_spans},
        BindingContext, BindingExtractionResult,
    },
};

/// Parse a single expression from a source span.
pub fn parse_expression<'alloc>(
    span: Span,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: oxc_span::SourceType,
    ignored: &'alloc Vec<&[u8]>,
) -> (
    Option<oxc_ast::ast::Expression<'alloc>>,
    Option<Vec<oxc_diagnostics::OxcDiagnostic>>,
    Option<BindingExtractionResult<'alloc>>,
) {
    if span.start >= span.end {
        return (None, None, None);
    }
    let source_slice = &input[span.start as usize..span.end as usize];

    let parser = oxc_parser::Parser::new(alloc, source_slice, source_type);
    let result = parser.parse_expression();

    match result {
        Ok(mut expr) => {
            // Adjust spans to be relative to original source
            // this is faster than creating a new padded string
            adjust_expression_spans(&mut expr, span.start);

            // Extract bindings
            let binding_ctx = BindingContext::with_ignored(span.start, ignored.iter().copied());
            let bindings = extract_bindings_from_expression(&expr, input, &binding_ctx);

            (Some(expr), None, Some(bindings))
        }
        Err(mut errors) => {
            adjust_diagnostics_spans(&mut errors, span.start);
            (None, Some(errors), None)
        }
    }
}

/// Parse a full program from a source span, adjusting all AST and diagnostic
/// spans to be relative to the original source.
///
/// Returns the `ParserReturn` with all spans offset by `span.start`.
pub fn parse_program<'alloc>(
    span: Span,
    input: &'alloc str,
    alloc: &'alloc Allocator,
    source_type: oxc_span::SourceType,
) -> oxc_parser::ParserReturn<'alloc> {
    if span.start >= span.end {
        // Parse empty string to get a valid empty ParserReturn
        return oxc_parser::Parser::new(alloc, "", source_type).parse();
    }
    let source_slice = &input[span.start as usize..span.end as usize];

    let mut result = oxc_parser::Parser::new(alloc, source_slice, source_type).parse();

    // Adjust all AST spans to be relative to original source
    adjust_program_spans(&mut result.program, span.start);
    // Adjust diagnostic label spans too
    adjust_diagnostics_spans(&mut result.errors, span.start);

    result
}
