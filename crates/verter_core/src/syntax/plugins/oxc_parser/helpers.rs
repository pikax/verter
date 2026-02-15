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
    ignored: &[&'alloc str],
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

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_span::{GetSpan, SourceType};

    // ── parse_expression ─────────────────────────────────────────────

    /// @ai-generated - Empty span returns all None.
    #[test]
    fn parse_expression_empty_span() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let (expr, errors, bindings) =
            parse_expression(Span::new(0, 0), "", &alloc, SourceType::tsx(), &ignored);
        assert!(expr.is_none());
        assert!(errors.is_none());
        assert!(bindings.is_none());
    }

    /// @ai-generated - Inverted span (start > end) returns all None.
    #[test]
    fn parse_expression_inverted_span() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let (expr, errors, bindings) = parse_expression(
            Span::new(5, 2),
            "hello world",
            &alloc,
            SourceType::tsx(),
            &ignored,
        );
        assert!(expr.is_none());
        assert!(errors.is_none());
        assert!(bindings.is_none());
    }

    /// @ai-generated - Simple identifier expression parses correctly.
    #[test]
    fn parse_expression_simple_identifier() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let input = "foo";
        let (expr, errors, bindings) =
            parse_expression(Span::new(0, 3), input, &alloc, SourceType::tsx(), &ignored);
        assert!(expr.is_some(), "Expected expression to parse");
        assert!(errors.is_none(), "Expected no errors");
        let bindings = bindings.expect("Expected bindings");
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(bindings.bindings[0].name, "foo");
    }

    /// @ai-generated - Expression at an offset has adjusted spans.
    #[test]
    fn parse_expression_with_offset() {
        let alloc = Allocator::default();
        let ignored = vec![];
        //                   0123456789...
        let input = "prefix foo + bar suffix";
        // Expression "foo + bar" starts at 7, ends at 16
        let (expr, errors, bindings) =
            parse_expression(Span::new(7, 16), input, &alloc, SourceType::tsx(), &ignored);
        assert!(expr.is_some(), "Expected expression to parse");
        assert!(errors.is_none());
        let bindings = bindings.expect("Expected bindings");
        assert_eq!(bindings.bindings.len(), 2);

        let names: Vec<&str> = bindings.bindings.iter().map(|b| b.name).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar"));

        // Verify spans are adjusted to original source positions
        for b in &bindings.bindings {
            assert!(b.span.start >= 7, "Span start should be offset: {:?}", b);
            assert!(b.span.end <= 16, "Span end should be within range: {:?}", b);
        }
    }

    /// @ai-generated - Ignored identifiers are marked as ignored in bindings.
    #[test]
    fn parse_expression_with_ignored_identifiers() {
        let alloc = Allocator::default();
        let ignored: Vec<&str> = vec!["item"];
        let input = "item.name";
        let (expr, errors, bindings) =
            parse_expression(Span::new(0, 9), input, &alloc, SourceType::tsx(), &ignored);
        assert!(expr.is_some());
        assert!(errors.is_none());
        let bindings = bindings.expect("Expected bindings");
        // "item" is the root identifier in member expression
        let item_binding = bindings.bindings.iter().find(|b| b.name == "item");
        assert!(item_binding.is_some(), "Expected 'item' binding");
        assert!(
            item_binding.unwrap().ignore,
            "'item' should be marked as ignored"
        );
    }

    /// @ai-generated - Invalid expression returns errors.
    #[test]
    fn parse_expression_invalid_syntax() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let input = "if (";
        let (expr, errors, _bindings) =
            parse_expression(Span::new(0, 4), input, &alloc, SourceType::tsx(), &ignored);
        assert!(expr.is_none(), "Expected no expression for invalid syntax");
        assert!(errors.is_some(), "Expected parse errors");
    }

    /// @ai-generated - Error spans are adjusted by offset.
    #[test]
    fn parse_expression_error_spans_adjusted() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let input = "prefix if ( suffix";
        // "if (" at offset 7..11
        let (_expr, errors, _bindings) =
            parse_expression(Span::new(7, 11), input, &alloc, SourceType::tsx(), &ignored);
        let errors = errors.expect("Expected parse errors");
        assert!(!errors.is_empty());
        // Error label spans should be adjusted by offset 7
        for err in &errors {
            for label in err.labels.iter().flatten() {
                assert!(
                    label.offset() >= 7,
                    "Error label offset should be >= 7, got {}",
                    label.offset()
                );
            }
        }
    }

    /// @ai-generated - Complex expression: ternary with function call.
    #[test]
    fn parse_expression_ternary() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let input = "a ? fn(b) : c";
        let (expr, errors, bindings) = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
            &ignored,
        );
        assert!(expr.is_some());
        assert!(errors.is_none());
        let bindings = bindings.expect("Expected bindings");
        let names: Vec<&str> = bindings.bindings.iter().map(|b| b.name).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"fn"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    /// @ai-generated - Arrow function expression parses correctly.
    #[test]
    fn parse_expression_arrow_function() {
        let alloc = Allocator::default();
        let ignored = vec![];
        let input = "() => foo";
        let (expr, errors, bindings) = parse_expression(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
            &ignored,
        );
        assert!(expr.is_some());
        assert!(errors.is_none());
        let bindings = bindings.expect("Expected bindings");
        let names: Vec<&str> = bindings.bindings.iter().map(|b| b.name).collect();
        assert!(names.contains(&"foo"));
    }

    // ── parse_program ────────────────────────────────────────────────

    /// @ai-generated - Empty span returns valid empty program.
    #[test]
    fn parse_program_empty_span() {
        let alloc = Allocator::default();
        let result = parse_program(Span::new(0, 0), "", &alloc, SourceType::tsx());
        assert!(result.program.body.is_empty());
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Inverted span returns valid empty program.
    #[test]
    fn parse_program_inverted_span() {
        let alloc = Allocator::default();
        let result = parse_program(
            Span::new(10, 5),
            "some input text",
            &alloc,
            SourceType::tsx(),
        );
        assert!(result.program.body.is_empty());
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Simple variable declaration parses correctly.
    #[test]
    fn parse_program_simple_declaration() {
        let alloc = Allocator::default();
        let input = "const x = 1;";
        let result = parse_program(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
        );
        assert_eq!(result.program.body.len(), 1, "Expected 1 statement");
        assert!(result.errors.is_empty(), "Expected no errors");
    }

    /// @ai-generated - Program at an offset has adjusted spans.
    #[test]
    fn parse_program_with_offset() {
        let alloc = Allocator::default();
        //                   0123456789012345678
        let input = "<script>const x = 1;</script>";
        // "const x = 1;" starts at 8, ends at 20
        let result = parse_program(Span::new(8, 20), input, &alloc, SourceType::tsx());
        assert_eq!(result.program.body.len(), 1);
        assert!(result.errors.is_empty());
        // Verify spans are offset
        let stmt = &result.program.body[0];
        assert!(
            stmt.span().start >= 8,
            "Statement span start should be >= 8, got {}",
            stmt.span().start
        );
    }

    /// @ai-generated - Multiple statements parse correctly.
    #[test]
    fn parse_program_multiple_statements() {
        let alloc = Allocator::default();
        let input = "const a = 1;\nconst b = 2;\nfunction foo() {}";
        let result = parse_program(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
        );
        assert_eq!(result.program.body.len(), 3, "Expected 3 statements");
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Import statement parses correctly.
    #[test]
    fn parse_program_import_statement() {
        let alloc = Allocator::default();
        let input = "import { ref } from 'vue';";
        let result = parse_program(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
        );
        assert_eq!(result.program.body.len(), 1);
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - TypeScript type annotation parses in tsx mode.
    #[test]
    fn parse_program_typescript() {
        let alloc = Allocator::default();
        let input = "const x: number = 1;";
        let result = parse_program(
            Span::new(0, input.len() as u32),
            input,
            &alloc,
            SourceType::tsx(),
        );
        assert_eq!(result.program.body.len(), 1);
        assert!(result.errors.is_empty());
    }

    /// @ai-generated - Invalid program produces errors with adjusted spans.
    #[test]
    fn parse_program_invalid_syntax() {
        let alloc = Allocator::default();
        let input = "prefix const = ;; suffix";
        // "const = ;;" at offset 7..17
        let result = parse_program(Span::new(7, 17), input, &alloc, SourceType::tsx());
        assert!(
            !result.errors.is_empty(),
            "Expected parse errors for invalid syntax"
        );
        // Error label spans should be adjusted by offset 7
        for err in &result.errors {
            for label in err.labels.iter().flatten() {
                assert!(
                    label.offset() >= 7,
                    "Error label offset should be >= 7, got {}",
                    label.offset()
                );
            }
        }
    }

    /// @ai-generated - Embedded script content with surrounding SFC tags.
    #[test]
    fn parse_program_embedded_in_sfc() {
        let alloc = Allocator::default();
        let input = "<script setup>\nimport { ref } from 'vue';\nconst count = ref(0);\n</script>";
        // Content between <script setup>\n and \n</script>
        let content_start = "<script setup>\n".len() as u32;
        let content_end = input.len() as u32 - "\n</script>".len() as u32;
        let result = parse_program(
            Span::new(content_start, content_end),
            input,
            &alloc,
            SourceType::tsx(),
        );
        assert_eq!(result.program.body.len(), 2, "Expected import + const");
        assert!(result.errors.is_empty());
        // All spans should be >= content_start
        for stmt in &result.program.body {
            assert!(
                stmt.span().start >= content_start,
                "Statement span start {} should be >= content_start {}",
                stmt.span().start,
                content_start
            );
        }
    }
}
