//! Vue v-for expression parsing.
//!
//! Parses Vue v-for expressions like `item of items` or `(item, key, index) in obj`.
//!
//! The parser splits on ` of ` or ` in ` and parses left and right sides separately,
//! which properly handles Vue's multi-variable syntax.

use memchr::memmem::{self, find};
use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::span::adjust_expression_spans;
use crate::common::Span;
use crate::utils::oxc::bindings::{
    collect_expression_reference_spans, collect_ts_type_reference_spans_from_expression,
};

/// Result of parsing a v-for expression.
#[derive(Debug)]
pub struct VForParseResult<'a> {
    /// The left side of the for expression (iteration variable/pattern).
    /// This is parsed as an Expression, which can be:
    /// - Identifier (for `item in items`)
    /// - ObjectExpression (for `{ a, b } in items`)
    /// - ArrayExpression (for `[a, b] in items`)
    /// - ParenthesizedExpression containing SequenceExpression (for `(item, index) in items`)
    pub left: Option<Expression<'a>>,

    /// The right side of the for expression (the iterable).
    pub right: Option<Expression<'a>>,

    /// Whether the original expression used `of` instead of `in`.
    pub is_of: bool,

    /// The byte offset of the left expression (always 0 since we parse it directly).
    pub left_offset: u32,

    /// The byte offset of the right expression (position after ` in ` or ` of `).
    pub right_offset: u32,

    /// Parse errors for left side, if any.
    pub left_errors: Vec<OxcDiagnostic>,

    /// Parse errors for right side, if any.
    pub right_errors: Vec<OxcDiagnostic>,
}

impl<'a> VForParseResult<'a> {
    /// Returns true if parsing was successful (no errors and both left/right are present).
    pub fn is_ok(&self) -> bool {
        self.left_errors.is_empty()
            && self.right_errors.is_empty()
            && self.left.is_some()
            && self.right.is_some()
    }

    /// Returns true if left side has errors.
    pub fn has_left_errors(&self) -> bool {
        !self.left_errors.is_empty()
    }

    /// Returns true if right side has errors.
    pub fn has_right_errors(&self) -> bool {
        !self.right_errors.is_empty()
    }
}

/// Combined result of parsing a v-for expression with extracted bindings.
///
/// This combines the parse result (AST) with the extracted bindings (locals/references)
/// in a single struct for convenience.
///
/// Bindings are stored as spans to avoid self-referential struct issues and save memory.
/// Use `span.slice(source)` to get the string value when needed.
#[derive(Debug)]
pub struct VForWithBindings<'a> {
    /// The parsed v-for expression result containing the AST.
    pub result: VForParseResult<'a>,

    /// Spans of local bindings declared by the v-for (iteration variables).
    /// For `(item, index) of items`, this would contain spans for "item" and "index".
    /// Use `span.slice(source)` to get the string value.
    pub locals: Vec<Span>,

    /// Spans of external references used in the v-for expression.
    /// For `item of data.items`, this would contain a span for "data".
    /// Use `span.slice(source)` to get the string value.
    pub references: Vec<Span>,
}

impl<'a> VForWithBindings<'a> {
    /// Returns the left side of the v-for expression (iteration variable/pattern).
    pub fn left(&self) -> Option<&Expression<'a>> {
        self.result.left.as_ref()
    }

    /// Returns the right side of the v-for expression (the iterable).
    pub fn right(&self) -> Option<&Expression<'a>> {
        self.result.right.as_ref()
    }

    /// Returns whether the expression uses 'of' instead of 'in'.
    pub fn is_of(&self) -> bool {
        self.result.is_of
    }

    /// Returns true if there are any parse errors.
    pub fn has_errors(&self) -> bool {
        self.result.has_left_errors() || self.result.has_right_errors()
    }

    /// Returns true if parsing was successful.
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }

    /// Returns the left offset.
    pub fn left_offset(&self) -> u32 {
        self.result.left_offset
    }

    /// Returns the right offset.
    pub fn right_offset(&self) -> u32 {
        self.result.right_offset
    }
}

/// Extract local spans (iteration variables) from a v-for left expression.
///
/// Handles various patterns:
/// - `item` → Identifier → [span of "item"]
/// - `{ id, name }` → ObjectExpression → [span of "id", span of "name"]
/// - `[a, b]` → ArrayExpression → [span of "a", span of "b"]
/// - `(item, index)` → ParenthesizedExpression(SequenceExpression) → [span of "item", span of "index"]
fn collect_vfor_left_local_spans(expr: &Expression<'_>, locals: &mut Vec<Span>) {
    match expr {
        Expression::Identifier(ident) => {
            locals.push(ident.span.into());
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        // For shorthand: { foo } → foo is both key and value
                        // For non-shorthand: { foo: bar } → bar is the binding
                        if p.shorthand {
                            if let PropertyKey::StaticIdentifier(ident) = &p.key {
                                locals.push(ident.span.into());
                            }
                        } else {
                            collect_vfor_left_local_spans(&p.value, locals);
                        }
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        collect_vfor_left_local_spans(&spread.argument, locals);
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_vfor_left_local_spans(&spread.argument, locals);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = elem.as_expression() {
                            collect_vfor_left_local_spans(e, locals);
                        }
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_vfor_left_local_spans(&paren.expression, locals);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_vfor_left_local_spans(e, locals);
            }
        }
        Expression::AssignmentExpression(assign) => {
            // Handle default values like `item = defaultValue`
            use oxc_ast::ast::{AssignmentTarget, SimpleAssignmentTarget};
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &assign.left {
                locals.push(id.span.into());
            } else if let Some(SimpleAssignmentTarget::AssignmentTargetIdentifier(id)) =
                assign.left.as_simple_assignment_target()
            {
                locals.push(id.span.into());
            }
        }
        _ => {}
    }
}

/// Extract binding spans from a VForParseResult.
///
/// This is an internal function used by `parse_vfor_with_bindings`.
/// Returns spans instead of string references to avoid self-referential struct issues.
fn extract_vfor_bindings_internal(
    result: &VForParseResult<'_>,
    source: &str,
) -> (Vec<Span>, Vec<Span>) {
    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();

    // Extract local spans from the left side (iteration variables)
    if let Some(left) = &result.left {
        collect_vfor_left_local_spans(left, &mut locals);
    }

    // Build ignored set from local names (need the actual strings to filter references)
    let ignored: FxHashSet<&[u8]> = locals
        .iter()
        .map(|span| span.slice(source).as_bytes())
        .collect();

    // Extract reference spans from the right side (the iterable)
    if let Some(right) = &result.right {
        collect_expression_reference_spans(right, &ignored, &mut references_set);
        // Also extract TypeScript type references from type assertions
        collect_ts_type_reference_spans_from_expression(right, &mut references_set);
    }

    let references: Vec<Span> = references_set.into_iter().collect();
    (locals, references)
}

/// Parse a Vue v-for expression.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `source` - The v-for expression content (e.g., "item of items")
/// * `source_type` - The source type (e.g., TSX, JavaScript)
///
/// # Returns
/// A `VForParseResult` containing the parsed left/right expressions,
/// metadata about the parse, and any errors.
///
/// # How it works
/// 1. Finds the ` of ` or ` in ` separator
/// 2. Splits the expression into left and right parts
/// 3. Parses each part separately as an expression
/// 4. Returns both with their respective offsets
///
/// # Offset Information
/// - `left_offset`: Always 0 (left expression starts at position 0)
/// - `right_offset`: Position after the separator (e.g., 8 for "item of items" → "items" starts at 8)
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vfor(&allocator, "item of items", SourceType::tsx());
/// assert!(result.is_ok());
/// assert!(result.is_of);
/// // result.left contains "item" parsed as Identifier
/// // result.right contains "items" parsed as Identifier
/// // result.right_offset == 8 (position of "items" in original)
/// ```
pub fn parse_vfor<'a>(
    allocator: &'a Allocator,
    source: &str,
    source_type: SourceType,
) -> VForParseResult<'a> {
    if source.is_empty() {
        return VForParseResult {
            left: None,
            right: None,
            is_of: false,
            left_offset: 0,
            right_offset: 0,
            left_errors: vec![],
            right_errors: vec![],
        };
    }

    let source_bytes = source.as_bytes();

    // Find ` of ` or ` in ` separator
    // Note: Both are 4 bytes (space + 2 chars + space)
    let of_pos = find(source_bytes, b" of ");
    let in_pos = find(source_bytes, b" in ");

    let (is_of, separator_pos) = match (of_pos, in_pos) {
        (Some(of), Some(r#in)) => {
            // Both found - use whichever comes first
            if of < r#in {
                (true, of)
            } else {
                (false, r#in)
            }
        }
        (Some(of), None) => (true, of),
        (None, Some(r#in)) => (false, r#in),
        (None, None) => {
            // Neither found - invalid v-for syntax
            return VForParseResult {
                left: None,
                right: None,
                is_of: false,
                left_offset: 0,
                right_offset: 0,
                left_errors: vec![OxcDiagnostic::error(
                    "Invalid v-for expression: missing 'in' or 'of' keyword",
                )],
                right_errors: vec![],
            };
        }
    };

    // Split into left and right parts
    let left_str = &source[..separator_pos];
    let right_start = separator_pos + 4; // " of " or " in " is 4 bytes
    let right_str = &source[right_start..];

    // Allocate strings in the allocator for lifetime safety
    let left_alloc = allocator.alloc_str(left_str);
    let right_alloc = allocator.alloc_str(right_str);

    // Parse left side as expression
    let left_parser = Parser::new(allocator, left_alloc, source_type);
    let left_result = left_parser.parse_expression();

    // Parse right side as expression
    let right_parser = Parser::new(allocator, right_alloc, source_type);
    let right_result = right_parser.parse_expression();

    // Extract left and errors
    let (left, left_errors) = match left_result {
        Ok(expr) => (Some(expr), vec![]),
        Err(errors) => (None, errors),
    };

    // Extract right and errors, adjusting spans to reflect original positions
    let right_offset = right_start as u32;
    let (right, right_errors) = match right_result {
        Ok(mut expr) => {
            // Adjust all spans in the right expression to reflect original source positions
            adjust_expression_spans(&mut expr, right_offset);
            (Some(expr), vec![])
        }
        Err(errors) => (None, errors),
    };

    VForParseResult {
        left,
        right,
        is_of,
        left_offset: 0,
        right_offset,
        left_errors,
        right_errors,
    }
}

pub fn extract_vfor_positions(bytes: &[u8], start: u32, end: u32) -> Option<(u32, u32, u32, bool)> {
    let source_bytes = &bytes[start as usize..end as usize];

    if let Some(pos) = find(source_bytes, b" in ") {
        Some((start, start + pos as u32, start + pos as u32 + 4, false))
    } else {
        find(source_bytes, b" of ").map(|pos| (start, start + pos as u32, start + pos as u32 + 4, true))
    }
}

/// Parse a Vue v-for expression and extract bindings in one pass.
///
/// This is the preferred function when you need both the parsed AST and the
/// extracted bindings, as it avoids having to call separate functions.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `source` - The v-for expression content (e.g., "item of items")
/// * `source_type` - The source type (e.g., TSX, JavaScript)
///
/// # Returns
/// A `VForWithBindings` containing:
/// - `result`: The parsed VForParseResult with AST
/// - `locals`: Declared iteration variables (e.g., `["item", "index"]`)
/// - `references`: External identifiers referenced (e.g., `["items"]`)
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vfor_with_bindings(&allocator, "(item, index) of items", SourceType::tsx());
/// assert!(result.is_ok());
/// assert_eq!(result.locals, vec!["item", "index"]);
/// assert_eq!(result.references, vec!["items"]);
/// ```
pub fn parse_vfor_with_bindings<'a>(
    allocator: &'a Allocator,
    source: &str,
    source_type: SourceType,
) -> VForWithBindings<'a> {
    let result = parse_vfor(allocator, source, source_type);

    // Extract bindings if parsing succeeded
    let (locals, references) = if result.has_left_errors() || result.has_right_errors() {
        (Vec::new(), Vec::new())
    } else {
        extract_vfor_bindings_internal(&result, source)
    };

    VForWithBindings {
        result,
        locals,
        references,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> VForParseResult<'static> {
        let allocator = Box::leak(Box::new(Allocator::default()));
        parse_vfor(allocator, source, SourceType::tsx())
    }

    #[test]
    fn test_simple_of() {
        let result = parse("item of items");
        assert!(result.is_ok());
        assert!(result.is_of);
        assert_eq!(result.left_offset, 0);
        assert_eq!(result.right_offset, 8); // "item of " = 8 chars

        // Check left is an identifier
        if let Some(Expression::Identifier(id)) = &result.left {
            assert_eq!(id.name.as_str(), "item");
            // Span should be 0-4 in the left string
            assert_eq!(id.span.start, 0);
            assert_eq!(id.span.end, 4);
        } else {
            panic!("Expected Identifier, got {:?}", result.left);
        }

        // Check right is an identifier with adjusted spans
        if let Some(Expression::Identifier(id)) = &result.right {
            assert_eq!(id.name.as_str(), "items");
            // Spans are now adjusted to reflect original source positions
            assert_eq!(id.span.start, 8); // "item of " = 8 chars
            assert_eq!(id.span.end, 13); // "item of items" = 13 chars
        } else {
            panic!("Expected Identifier expression");
        }
    }

    #[test]
    fn test_simple_in() {
        let result = parse("item in items");
        assert!(result.is_ok());
        assert!(!result.is_of);
        assert_eq!(result.right_offset, 8); // "item in " = 8 chars
    }

    #[test]
    fn test_destructuring_object() {
        let result = parse("{ id, name } of items");
        assert!(result.is_ok());
        assert!(result.is_of);

        // Check left is an object expression
        if let Some(Expression::ObjectExpression(_)) = &result.left {
            // OK
        } else {
            panic!("Expected ObjectExpression, got {:?}", result.left);
        }
    }

    #[test]
    fn test_destructuring_array() {
        let result = parse("[first, second] of items");
        assert!(result.is_ok());
        assert!(result.is_of);

        // Check left is an array expression
        if let Some(Expression::ArrayExpression(_)) = &result.left {
            // OK
        } else {
            panic!("Expected ArrayExpression, got {:?}", result.left);
        }
    }

    #[test]
    fn test_with_parentheses() {
        // Vue's multi-variable syntax - now properly handled!
        let result = parse("(item, index) of items");
        assert!(result.is_ok());
        assert!(result.is_of);

        // Check left is a parenthesized sequence expression
        if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
            if let Expression::SequenceExpression(seq) = &paren.expression {
                assert_eq!(seq.expressions.len(), 2);
                // First should be "item"
                if let Expression::Identifier(id) = &seq.expressions[0] {
                    assert_eq!(id.name.as_str(), "item");
                }
                // Second should be "index"
                if let Expression::Identifier(id) = &seq.expressions[1] {
                    assert_eq!(id.name.as_str(), "index");
                }
            } else {
                panic!("Expected SequenceExpression inside parentheses");
            }
        } else {
            panic!("Expected ParenthesizedExpression, got {:?}", result.left);
        }
    }

    #[test]
    fn test_index_key_value() {
        // Vue's multi-variable syntax with three values
        let result = parse("(value, key, index) in obj");
        assert!(result.is_ok());
        assert!(!result.is_of);

        // Check left is a parenthesized sequence expression with 3 items
        if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
            if let Expression::SequenceExpression(seq) = &paren.expression {
                assert_eq!(seq.expressions.len(), 3);
            } else {
                panic!("Expected SequenceExpression");
            }
        } else {
            panic!("Expected ParenthesizedExpression");
        }
    }

    #[test]
    fn test_member_expression_iterable() {
        let result = parse("item of data.items");
        assert!(result.is_ok());
        assert!(result.is_of);

        // Check right is a member expression
        if let Some(Expression::StaticMemberExpression(_)) = &result.right {
            // OK
        } else {
            panic!("Expected StaticMemberExpression, got {:?}", result.right);
        }
    }

    #[test]
    fn test_function_call_iterable() {
        let result = parse("item of getItems()");
        assert!(result.is_ok());

        // Check right is a call expression
        if let Some(Expression::CallExpression(_)) = &result.right {
            // OK
        } else {
            panic!("Expected CallExpression");
        }
    }

    #[test]
    fn test_empty_input() {
        let result = parse("");
        assert!(!result.is_ok());
        assert!(result.left.is_none());
        assert!(result.right.is_none());
    }

    #[test]
    fn test_missing_separator() {
        let result = parse("item items");
        assert!(!result.is_ok());
        assert!(!result.left_errors.is_empty());
    }

    #[test]
    fn test_typescript_assertion() {
        let result = parse("item of (items as Item[])");
        assert!(result.is_ok());
        assert!(result.is_of);

        // Check right contains type assertion
        if let Some(Expression::ParenthesizedExpression(paren)) = &result.right {
            if let Expression::TSAsExpression(_) = &paren.expression {
                // OK
            } else {
                panic!("Expected TSAsExpression inside parentheses");
            }
        } else {
            panic!("Expected ParenthesizedExpression");
        }
    }

    #[test]
    fn test_span_offset_calculation() {
        // Test that spans correctly reflect original positions
        let result = parse("item of items");
        assert!(result.is_ok());

        // Left side "item" - spans are relative to substring (offset 0)
        if let Some(Expression::Identifier(id)) = &result.left {
            assert_eq!(id.span.start, 0);
            assert_eq!(id.span.end, 4);
            // For absolute position: add left_offset (always 0)
            assert_eq!(id.span.start + result.left_offset, 0);
            assert_eq!(id.span.end + result.left_offset, 4);
        }

        // Right side "items" - spans are now pre-adjusted to original positions
        if let Some(Expression::Identifier(id)) = &result.right {
            // Spans already reflect original source positions
            assert_eq!(id.span.start, 8);
            assert_eq!(id.span.end, 13);
        }
    }

    #[test]
    fn test_complex_destructuring_with_index() {
        let result = parse("({ id, name }, index) of items");
        assert!(result.is_ok());

        // Left should be a parenthesized sequence with object destructuring
        if let Some(Expression::ParenthesizedExpression(paren)) = &result.left {
            if let Expression::SequenceExpression(seq) = &paren.expression {
                assert_eq!(seq.expressions.len(), 2);
                // First should be object expression
                assert!(matches!(
                    &seq.expressions[0],
                    Expression::ObjectExpression(_)
                ));
                // Second should be identifier "index"
                if let Expression::Identifier(id) = &seq.expressions[1] {
                    assert_eq!(id.name.as_str(), "index");
                }
            }
        }
    }

    #[test]
    fn test_array_iterable() {
        let result = parse("item of [1, 2, 3]");
        assert!(result.is_ok());

        // Right should be an array expression
        if let Some(Expression::ArrayExpression(arr)) = &result.right {
            assert_eq!(arr.elements.len(), 3);
        } else {
            panic!("Expected ArrayExpression");
        }
    }

    #[test]
    fn test_range_expression() {
        // Common Vue pattern with computed range
        let result = parse("n of Array(10).keys()");
        assert!(result.is_ok());

        // Right should be a call expression chain
        if let Some(Expression::CallExpression(_)) = &result.right {
            // OK
        } else {
            panic!("Expected CallExpression");
        }
    }

    #[test]
    fn test_object_literal_with_shorthand() {
        // Object literal with shorthand properties on the right side
        // This tests the span adjustment for shorthand object properties
        let result = parse("item of [{ foo }, { bar }]");
        assert!(result.is_ok());

        // Right should be an array of objects
        if let Some(Expression::ArrayExpression(arr)) = &result.right {
            assert_eq!(arr.elements.len(), 2);
        } else {
            panic!("Expected ArrayExpression, got {:?}", result.right);
        }
    }

    #[test]
    fn test_object_literal_iterable() {
        // Object expression as the iterable
        let result = parse("item of { a: 1, b: 2 }");
        assert!(result.is_ok());

        // Right should be an object expression
        if let Some(Expression::ObjectExpression(obj)) = &result.right {
            assert_eq!(obj.properties.len(), 2);
        } else {
            panic!("Expected ObjectExpression, got {:?}", result.right);
        }
    }

    #[test]
    fn test_mixed_object_properties() {
        // Object with both shorthand and regular properties
        let result = parse("key of { foo, bar: baz, qux }");
        assert!(result.is_ok());

        if let Some(Expression::ObjectExpression(obj)) = &result.right {
            assert_eq!(obj.properties.len(), 3);
        } else {
            panic!("Expected ObjectExpression, got {:?}", result.right);
        }
    }
}
