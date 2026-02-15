//! Vue v-slot expression parsing.
//!
//! Parses Vue v-slot expressions which use function parameter syntax.
//! Examples: `{ data }`, `{ item, index = 0 }`, `{ rowData: role }: { rowData: ProjectRole }`
//!
//! The slot content is wrapped as arrow function parameters and parsed:
//! `{ foo, bar }` → `({ foo, bar })=>{}`

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, FormalParameters};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::span::{
    adjust_diagnostics_spans, adjust_formal_parameters_spans, subtract_formal_parameters_spans,
};
use crate::common::Span;
use crate::utils::oxc::bindings::{
    collect_expression_reference_spans, collect_pattern_local_spans,
    collect_pattern_reference_spans, collect_type_reference_spans,
};

/// Result of parsing a v-slot expression.
#[derive(Debug)]
pub struct VSlotParseResult<'a> {
    /// The byte offset added by wrapping (1 for "(").
    /// Subtract this from AST spans to get original source offsets.
    pub offset: u32,

    /// The parsed formal parameters from the slot expression.
    /// Contains the parameter patterns, type annotations, and default values.
    pub params: Option<FormalParameters<'a>>,

    /// Parse errors, if any.
    pub errors: Option<Vec<OxcDiagnostic>>,
}

impl<'a> VSlotParseResult<'a> {
    /// Returns true if parsing was successful (no errors and params are present).
    pub fn is_ok(&self) -> bool {
        self.errors.is_none() && self.params.is_some()
    }
}

/// Combined result of parsing a v-slot expression with extracted bindings.
///
/// This combines the parse result (AST) with the extracted bindings (locals/references)
/// in a single struct for convenience.
///
/// Bindings are stored as spans to avoid self-referential struct issues and save memory.
/// Use `span.slice(source)` to get the string value when needed.
#[derive(Debug)]
pub struct VSlotWithBindings<'a> {
    /// The parsed v-slot expression result containing the AST.
    pub result: VSlotParseResult<'a>,

    /// Spans of local bindings declared by the slot parameters.
    /// For `{ item, index }`, this would contain spans for "item" and "index".
    /// Use `span.slice(source)` to get the string value.
    pub locals: Vec<Span>,

    /// Spans of external references used in the slot expression (type annotations, defaults).
    /// For `{ data }: { data: MyType }`, this would contain a span for "MyType".
    /// Use `span.slice(source)` to get the string value.
    pub references: Vec<Span>,
}

impl<'a> VSlotWithBindings<'a> {
    /// Returns the parsed formal parameters from the slot expression.
    pub fn params(&self) -> Option<&FormalParameters<'a>> {
        self.result.params.as_ref()
    }

    /// Returns true if there are any parse errors.
    pub fn has_errors(&self) -> bool {
        self.result.errors.is_some()
    }

    /// Returns true if parsing was successful.
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }

    /// Returns the offset added by wrapping.
    pub fn offset(&self) -> u32 {
        self.result.offset
    }
}

/// Extract binding spans from FormalParameters.
///
/// This is an internal function used by `parse_vslot_with_bindings`.
/// Returns spans instead of string references to avoid self-referential struct issues.
fn extract_slot_bindings_internal(
    params: &FormalParameters<'_>,
    source: &str,
    ignored_extra: &[&str],
) -> (Vec<Span>, Vec<Span>) {
    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();

    // Extract local spans from parameters
    for param in &params.items {
        collect_pattern_local_spans(&param.pattern, &mut locals);
    }
    if let Some(rest) = &params.rest {
        collect_pattern_local_spans(&rest.rest.argument, &mut locals);
    }

    // Build ignored set from local names (need the actual strings to filter references)
    let mut ignored: FxHashSet<&[u8]> = locals
        .iter()
        .map(|span| span.slice(source).as_bytes())
        .collect();

    if !ignored_extra.is_empty() {
        for name in ignored_extra {
            ignored.insert(name.as_bytes());
        }
    }

    // Extract reference spans from type annotations and default values
    for param in &params.items {
        // Default value (initializer)
        if let Some(init) = &param.initializer {
            collect_expression_reference_spans(init, &ignored, &mut references_set);
        }
        // Type annotation on the parameter (on FormalParameter, not BindingPattern)
        if let Some(annotation) = &param.type_annotation {
            collect_type_reference_spans(&annotation.type_annotation, &mut references_set);
        }
        // References in default values within the pattern
        collect_pattern_reference_spans(&param.pattern, &ignored, &mut references_set);
    }
    if let Some(rest) = &params.rest {
        if let Some(annotation) = &rest.type_annotation {
            collect_type_reference_spans(&annotation.type_annotation, &mut references_set);
        }
    }

    let references: Vec<Span> = references_set.into_iter().collect();
    (locals, references)
}

/// Parse a Vue v-slot expression from a span within a larger source string.
///
/// This is the primary implementation. All AST spans in the result are
/// adjusted to be relative to the full `input`, not the extracted substring.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `span` - The byte range within `input` containing the v-slot expression
/// * `input` - The full source string (e.g., the entire SFC file)
/// * `source_type` - The source type (e.g., TSX, JavaScript)
/// * `ignored` - Identifiers to ignore when collecting references
/// * `ignored` - Identifiers to ignore when collecting references
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// //                0         1         2         3
/// //                012345678901234567890123456789012345
/// let input = r#"<template #default="{ data }"></template>"#;
/// // "{ data }" is at bytes 20..28
/// let result = parse_vslot_sliced(&allocator, Span::new(20, 28), input, SourceType::tsx());
/// assert!(result.is_ok());
/// // FormalParameters spans are file-relative (20..28 range)
/// ```
pub fn parse_vslot_sliced<'a>(
    allocator: &'a Allocator,
    span: Span,
    input: &str,
    source_type: SourceType,
) -> VSlotParseResult<'a> {
    if span.start >= span.end {
        return VSlotParseResult {
            params: None,
            offset: 0,
            errors: None,
        };
    }

    let source = &input[span.start as usize..span.end as usize];

    // Parse substring-relative, then adjust to file-relative
    let mut result = parse_vslot_internal(allocator, source, source_type);

    // Adjust all AST spans to be file-relative
    if span.start > 0 {
        if let Some(params) = &mut result.params {
            adjust_formal_parameters_spans(params, span.start);
        }
        if let Some(errors) = &mut result.errors {
            adjust_diagnostics_spans(errors, span.start);
        }
    }

    result
}

/// Parse a Vue v-slot expression from a raw string.
///
/// Convenience wrapper around the internal parse logic. All spans in the
/// result are relative to `source` (starting from 0).
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vslot(&allocator, "{ data, index = 0 }", SourceType::tsx());
/// assert!(result.is_ok());
/// ```
pub fn parse_vslot<'a>(
    allocator: &'a Allocator,
    source: &str,
    source_type: SourceType,
) -> VSlotParseResult<'a> {
    parse_vslot_internal(allocator, source, source_type)
}

/// Internal v-slot parsing logic. Returns substring-relative spans.
fn parse_vslot_internal<'a>(
    allocator: &'a Allocator,
    source: &str,
    source_type: SourceType,
) -> VSlotParseResult<'a> {
    // Handle empty/whitespace-only input
    if source.trim().is_empty() {
        return VSlotParseResult {
            params: None,
            offset: 0,
            errors: None,
        };
    }

    // Wrap as arrow function parameters: `({content})=>{}`
    // The opening `(` adds 1 byte offset
    const WRAPPER_OFFSET: u32 = 1;
    let wrapped_string = format!("({})=>{{}}", source);
    // Allocate the wrapped string in the allocator so it lives as long as the allocator
    let wrapped = allocator.alloc_str(&wrapped_string);

    // Parse as expression
    let parser = Parser::new(allocator, wrapped, source_type);
    let result = parser.parse_expression();

    match result {
        Ok(expr) => {
            // Extract FormalParameters from ArrowFunctionExpression
            if let Expression::ArrowFunctionExpression(arrow) = expr {
                let mut params = arrow.unbox().params.unbox();
                // Adjust spans to reflect original source positions (subtract wrapper offset)
                subtract_formal_parameters_spans(&mut params, WRAPPER_OFFSET);
                VSlotParseResult {
                    params: Some(params),
                    offset: WRAPPER_OFFSET,
                    errors: None,
                }
            } else {
                VSlotParseResult {
                    params: None,
                    offset: WRAPPER_OFFSET,
                    errors: Some(vec![OxcDiagnostic::error(
                        "Failed to parse slot expression as arrow function parameters",
                    )]),
                }
            }
        }
        Err(errors) => VSlotParseResult {
            params: None,
            offset: WRAPPER_OFFSET,
            errors: Some(errors),
        },
    }
}

/// Parse a Vue v-slot expression from a span and extract bindings in one pass.
///
/// This is the preferred function when you need both the parsed AST and the
/// extracted bindings with file-relative spans. Binding extraction happens on
/// the substring-relative AST, then all spans are adjusted to be input-relative.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `span` - The byte range within `input` containing the v-slot expression,
///   or `None` for a bare `v-slot` with no value
/// * `input` - The full source string (e.g., the entire SFC file)
/// * `source_type` - The source type (e.g., TSX, JavaScript)
///
/// # Returns
/// A `VSlotWithBindings` with all spans (AST, locals, references) file-relative.
pub fn parse_vslot_with_bindings_sliced<'a>(
    allocator: &'a Allocator,
    span: Option<Span>,
    input: &str,
    source_type: SourceType,
    ignored: &[&str],
) -> VSlotWithBindings<'a> {
    let (source, offset) = match span {
        Some(s) if s.start < s.end => (&input[s.start as usize..s.end as usize], s.start),
        _ => ("", 0),
    };

    // Parse with substring — spans are substring-relative
    let mut result = parse_vslot_internal(allocator, source, source_type);

    // Extract bindings while spans are still substring-relative
    let (mut locals, mut references) = if result.errors.is_some() {
        (Vec::new(), Vec::new())
    } else if let Some(params) = &result.params {
        extract_slot_bindings_internal(params, source, ignored)
    } else {
        (Vec::new(), Vec::new())
    };

    // Adjust everything to file-relative
    if offset > 0 {
        if let Some(params) = &mut result.params {
            adjust_formal_parameters_spans(params, offset);
        }
        if let Some(errors) = &mut result.errors {
            adjust_diagnostics_spans(errors, offset);
        }
        for s in &mut locals {
            s.start += offset;
            s.end += offset;
        }
        for s in &mut references {
            s.start += offset;
            s.end += offset;
        }
    }

    VSlotWithBindings {
        result,
        locals,
        references,
    }
}

/// Parse a Vue v-slot expression from a raw string and extract bindings.
///
/// Convenience wrapper around [`parse_vslot_with_bindings_sliced`] that treats
/// the entire `source` string as the expression.
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vslot_with_bindings(&allocator, "{ data }", SourceType::tsx(), &[]);
/// assert!(result.is_ok());
/// ```
pub fn parse_vslot_with_bindings<'a>(
    allocator: &'a Allocator,
    source: &str,
    source_type: SourceType,
    ignored: &[&str],
) -> VSlotWithBindings<'a> {
    parse_vslot_with_bindings_sliced(
        allocator,
        Some(Span::new(0, source.len() as u32)),
        source,
        source_type,
        ignored,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_ast::ast::BindingPattern;

    fn parse(source: &str) -> VSlotParseResult<'static> {
        let allocator = Box::leak(Box::new(Allocator::default()));
        parse_vslot(allocator, source, SourceType::tsx())
    }

    #[test]
    fn test_simple_identifier() {
        let result = parse("data");
        assert!(result.is_ok());
        assert_eq!(result.offset, 1);

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        // Check the parameter is an identifier with adjusted spans
        if let BindingPattern::BindingIdentifier(id) = &params.items[0].pattern {
            assert_eq!(id.name.as_str(), "data");
            // Spans are now adjusted to original positions (0-4 for "data")
            assert_eq!(id.span.start, 0);
            assert_eq!(id.span.end, 4);
        } else {
            panic!("Expected BindingIdentifier");
        }
    }

    #[test]
    fn test_object_destructuring() {
        let result = parse("{ item, index }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        // Check the parameter is an object pattern
        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            assert_eq!(obj.properties.len(), 2);
        } else {
            panic!("Expected ObjectPattern");
        }
    }

    #[test]
    fn test_renamed_destructuring() {
        let result = parse("{ rowData: role }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            assert_eq!(obj.properties.len(), 1);
            // The binding should be 'role'
            if let BindingPattern::BindingIdentifier(id) = &obj.properties[0].value {
                assert_eq!(id.name.as_str(), "role");
            } else {
                panic!("Expected BindingIdentifier for renamed property");
            }
        } else {
            panic!("Expected ObjectPattern");
        }
    }

    #[test]
    fn test_with_default_value() {
        let result = parse("{ item = defaultItem }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            assert_eq!(obj.properties.len(), 1);
            // The property should have a default value (AssignmentPattern)
            if let BindingPattern::AssignmentPattern(_) = &obj.properties[0].value {
                // OK - has default
            } else {
                panic!("Expected AssignmentPattern for default value");
            }
        } else {
            panic!("Expected ObjectPattern");
        }
    }

    #[test]
    fn test_with_type_annotation() {
        let result = parse("{ data }: { data: MyType }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        // Check type annotation is present
        assert!(params.items[0].type_annotation.is_some());
    }

    #[test]
    fn test_multiple_params() {
        let result = parse("item, index, extra");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 3);
    }

    #[test]
    fn test_rest_parameter() {
        let result = parse("first, ...rest");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1); // 'first' is a regular param
        assert!(params.rest.is_some()); // '...rest' is the rest element
    }

    #[test]
    fn test_nested_destructuring() {
        let result = parse("{ user: { name, id } }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);
    }

    #[test]
    fn test_array_destructuring() {
        let result = parse("[first, second]");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        if let BindingPattern::ArrayPattern(_) = &params.items[0].pattern {
            // OK
        } else {
            panic!("Expected ArrayPattern");
        }
    }

    #[test]
    fn test_empty_input() {
        let result = parse("");
        // Empty input returns None for params but no errors
        assert!(result.params.is_none());
        assert!(result.errors.is_none());
    }

    #[test]
    fn test_whitespace_only() {
        let result = parse("   ");
        assert!(result.params.is_none());
        assert!(result.errors.is_none());
    }

    #[test]
    fn test_invalid_syntax() {
        let result = parse("{ invalid: }");
        assert!(!result.is_ok());
        assert!(result.errors.is_some());
    }

    #[test]
    fn test_complex_type_annotation() {
        let result = parse("data: Array<Item>");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert!(params.items[0].type_annotation.is_some());
    }

    #[test]
    fn test_default_with_function_call() {
        let result = parse("data = getData()");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        // Parameter with default value at top level
        assert!(params.items[0].initializer.is_some());
    }

    #[test]
    fn test_span_offset() {
        let result = parse("data");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        if let BindingPattern::BindingIdentifier(id) = &params.items[0].pattern {
            // Spans are now pre-adjusted to original positions
            // "data" should be 0-4 in the original source
            assert_eq!(id.span.start, 0);
            assert_eq!(id.span.end, 4);
        }
    }

    #[test]
    fn test_complex_slot_expression() {
        // Real-world example from Vue
        let result = parse("{ rowData: role }: { rowData: ProjectRole }");
        assert!(result.is_ok());

        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        // Check destructuring
        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            if let BindingPattern::BindingIdentifier(id) = &obj.properties[0].value {
                assert_eq!(id.name.as_str(), "role");
            }
        }

        // Check type annotation
        assert!(params.items[0].type_annotation.is_some());
    }

    // ── parse_vslot_sliced ─────────────────────────────────────────

    /// @ai-generated - Sliced parse adjusts FormalParameters spans to file-relative.
    #[test]
    fn test_sliced_span_adjustment() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        //               0         1         2         3
        //               0123456789012345678901234567890123456
        let input = r#"<template #default="{ data }"></template>"#;
        // "{ data }" is at bytes 20..28
        let result = parse_vslot_sliced(allocator, Span::new(20, 28), input, SourceType::tsx());

        assert!(result.is_ok());
        let params = result.params.unwrap();
        assert_eq!(params.items.len(), 1);

        // "data" binding should be at file-relative position
        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            if let BindingPattern::BindingIdentifier(id) = &obj.properties[0].value {
                assert_eq!(id.name.as_str(), "data");
                // "data" is at position 22..26 in the full input
                assert!(
                    id.span.start >= 20,
                    "span.start {} should be >= 20",
                    id.span.start
                );
                assert!(
                    id.span.end <= 28,
                    "span.end {} should be <= 28",
                    id.span.end
                );
            }
        }
    }

    /// @ai-generated - Sliced parse with zero offset matches raw parse.
    #[test]
    fn test_sliced_zero_offset_matches_raw() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let source = "{ data }";
        let raw = parse_vslot(allocator, source, SourceType::tsx());
        let sliced = parse_vslot_sliced(
            allocator,
            Span::new(0, source.len() as u32),
            source,
            SourceType::tsx(),
        );

        assert_eq!(raw.is_ok(), sliced.is_ok());
        assert_eq!(raw.offset, sliced.offset);
    }

    /// @ai-generated - Sliced parse with empty span returns empty result.
    #[test]
    fn test_sliced_empty_span() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let result =
            parse_vslot_sliced(allocator, Span::new(5, 5), "some input", SourceType::tsx());
        assert!(result.params.is_none());
        assert!(result.errors.is_none());
    }

    // ── parse_vslot_with_bindings_sliced ───────────────────────────

    /// @ai-generated - Bindings sliced adjusts all spans to file-relative.
    #[test]
    fn test_bindings_sliced_span_adjustment() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        //               0         1         2         3
        //               0123456789012345678901234567890123456
        let input = r#"<template #default="{ data }"></template>"#;
        // "{ data }" at bytes 20..28
        let wb = parse_vslot_with_bindings_sliced(
            allocator,
            Some(Span::new(20, 28)),
            input,
            SourceType::tsx(),
            &[],
        );

        assert!(wb.is_ok());

        // Local "data" should be at file-relative position
        assert_eq!(wb.locals.len(), 1);
        assert!(wb.locals[0].start >= 20);
        assert!(wb.locals[0].end <= 28);
        assert_eq!(wb.locals[0].slice(input), "data");
    }

    /// @ai-generated - Bindings sliced with None span returns empty result.
    #[test]
    fn test_bindings_sliced_none_span() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb =
            parse_vslot_with_bindings_sliced(allocator, None, "some input", SourceType::tsx(), &[]);

        assert!(wb.locals.is_empty());
        assert!(wb.references.is_empty());
    }

    /// @ai-generated - Bindings sliced with zero offset matches raw.
    #[test]
    fn test_bindings_sliced_zero_offset() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let source = "{ item, index }";
        let raw = parse_vslot_with_bindings(allocator, source, SourceType::tsx(), &[]);
        let sliced = parse_vslot_with_bindings_sliced(
            allocator,
            Some(Span::new(0, source.len() as u32)),
            source,
            SourceType::tsx(),
            &[],
        );

        assert_eq!(raw.locals.len(), sliced.locals.len());
        assert_eq!(raw.references.len(), sliced.references.len());
        for (r, s) in raw.locals.iter().zip(sliced.locals.iter()) {
            assert_eq!(r.start, s.start);
            assert_eq!(r.end, s.end);
        }
    }

    /// @ai-generated - Bindings sliced with type annotations and offset.
    #[test]
    fn test_bindings_sliced_with_types() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let input = "prefix { data }: { data: MyType } suffix";
        //           0123456 = 7 bytes prefix
        let start = 7u32;
        let end = start + "{ data }: { data: MyType }".len() as u32;
        let wb = parse_vslot_with_bindings_sliced(
            allocator,
            Some(Span::new(start, end)),
            input,
            SourceType::tsx(),
            &[],
        );

        assert!(wb.is_ok());

        // Local "data" should be within [start, end)
        assert_eq!(wb.locals.len(), 1);
        assert!(wb.locals[0].start >= start);
        assert!(wb.locals[0].end <= end);
        assert_eq!(wb.locals[0].slice(input), "data");

        // Reference "MyType" should be within [start, end)
        assert!(!wb.references.is_empty());
        for s in &wb.references {
            assert!(s.start >= start);
            assert!(s.end <= end);
        }
    }

    /// @ai-generated - Ignored identifiers are excluded from references.
    #[test]
    fn test_bindings_ignored_identifiers() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let source = "{ item = ignoredRef }";
        let ignored: Vec<&str> = vec!["ignoredRef"];
        let wb = parse_vslot_with_bindings(allocator, source, SourceType::tsx(), &ignored);

        assert!(wb.is_ok());
        assert!(wb.references.is_empty());
    }
}
