//! Vue script generic parsing.
//!
//! Parses Vue `<script setup>` generic attributes like:
//! ```vue
//! <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
//! ```
//!
//! The parser wraps the generic string in an arrow function expression to parse it
//! as valid TypeScript, then extracts the type parameters.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, TSTypeParameterDeclaration};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use smallvec::SmallVec;

use crate::common::Span;

/// The prefix used when wrapping the generic string for parsing.
/// Creates an arrow function: `<{generic}>()=>{}`
pub const GENERIC_WRAPPER_PREFIX: &[u8] = b"<";
/// The suffix used when wrapping the generic string for parsing.
pub const GENERIC_WRAPPER_SUFFIX: &[u8] = b">()=>{}";

/// Information about a single generic type parameter.
#[derive(Debug, Clone, Copy)]
pub struct GenericParam {
    /// Span of the parameter name within the generic string (relative to generic start).
    pub name_span: Span,
    /// Span of the full parameter content within the generic string.
    pub span: Span,
    /// Span of the constraint within the generic string (if any).
    /// For `T extends Foo`, this would be the span of `Foo`.
    pub constraint_span: Option<Span>,
    /// Span of the default type within the generic string (if any).
    /// For `T = DefaultType`, this would be the span of `DefaultType`.
    pub default_span: Option<Span>,
}

impl GenericParam {
    /// Get the parameter name as bytes from the source.
    #[inline]
    pub fn name<'a>(&self, source: &'a [u8]) -> &'a [u8] {
        &source[self.name_span.start as usize..self.name_span.end as usize]
    }

    /// Get the full parameter content as bytes from the source.
    #[inline]
    pub fn content<'a>(&self, source: &'a [u8]) -> &'a [u8] {
        &source[self.span.start as usize..self.span.end as usize]
    }

    /// Get the constraint as bytes from the source (if any).
    #[inline]
    pub fn constraint<'a>(&self, source: &'a [u8]) -> Option<&'a [u8]> {
        self.constraint_span
            .map(|span| &source[span.start as usize..span.end as usize])
    }

    /// Get the default type as bytes from the source (if any).
    #[inline]
    pub fn default_type<'a>(&self, source: &'a [u8]) -> Option<&'a [u8]> {
        self.default_span
            .map(|span| &source[span.start as usize..span.end as usize])
    }
}

/// Result of parsing a generic attribute.
#[derive(Debug)]
pub struct GenericParseResult<'a> {
    /// The parsed expression (arrow function) containing type parameters.
    /// Use `type_parameters()` to access the type parameter declaration.
    /// `None` if parsing failed.
    expression: Option<Expression<'a>>,
    /// Span of the entire generic in the original file (file-based offset).
    pub position: Span,
    /// The offset added by the wrapper prefix.
    /// Use this to convert AST spans to generic-relative spans:
    /// `ast_span.start - ast_offset` gives the position within the generic string.
    pub ast_offset: u32,
    /// Information about each parameter (spans are relative to the generic string start).
    /// Empty if parsing failed.
    pub params: SmallVec<[GenericParam; 4]>,
    /// Parse errors, if any.
    pub errors: Vec<OxcDiagnostic>,
}

impl<'a> GenericParseResult<'a> {
    /// Returns true if parsing was successful (no errors and expression is present).
    #[inline]
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.expression.is_some()
    }

    /// Returns true if there are parse errors.
    #[inline]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the type parameters declaration if parsing succeeded.
    #[inline]
    pub fn type_parameters(&self) -> Option<&TSTypeParameterDeclaration<'a>> {
        let Expression::ArrowFunctionExpression(arrow_fn) = self.expression.as_ref()? else {
            return None;
        };
        arrow_fn.type_parameters.as_deref()
    }

    /// Get the number of generic parameters.
    #[inline]
    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    /// Get a parameter name as bytes from the generic source.
    #[inline]
    pub fn get_param_name<'s>(&self, index: usize, generic_source: &'s [u8]) -> Option<&'s [u8]> {
        self.params.get(index).map(|p| p.name(generic_source))
    }

    /// Get all parameter names as bytes from the generic source.
    pub fn get_all_param_names<'s>(&self, generic_source: &'s [u8]) -> SmallVec<[&'s [u8]; 4]> {
        self.params.iter().map(|p| p.name(generic_source)).collect()
    }
}

/// Backwards-compatible type alias.
pub type GenericInfo<'a> = GenericParseResult<'a>;

/// Parse a Vue script generic attribute.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `generic_str` - The generic string content (e.g., "T extends 'foo' | 'bar', Comp")
/// * `file_offset` - The byte offset where this generic starts in the original file
///
/// # Returns
/// A `GenericParseResult` containing the parsed AST (if successful), extracted parameters,
/// position information, and any parse errors.
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_generic(&allocator, "T extends Foo, U = Bar", 100);
/// if result.is_ok() {
///     assert_eq!(result.param_count(), 2);
///     // AST spans are offset by ast_offset (1 for "<")
///     // File position is 100-122 (100 + generic length)
/// }
/// ```
pub fn parse_generic<'a>(
    allocator: &'a Allocator,
    generic_str: &str,
    file_offset: u32,
) -> GenericParseResult<'a> {
    let ast_offset = GENERIC_WRAPPER_PREFIX.len() as u32;
    let position = Span::new(file_offset, file_offset + generic_str.len() as u32);

    // Handle empty input
    if generic_str.is_empty() {
        return GenericParseResult {
            expression: None,
            position,
            ast_offset,
            params: SmallVec::new(),
            errors: vec![OxcDiagnostic::error("Empty generic string")],
        };
    }

    let generic_bytes = generic_str.as_bytes();

    // Build the wrapped code: "<{generic_str}>() => {}"
    let wrapped_len =
        GENERIC_WRAPPER_PREFIX.len() + generic_str.len() + GENERIC_WRAPPER_SUFFIX.len();
    let mut wrapped_code = Vec::with_capacity(wrapped_len);
    wrapped_code.extend_from_slice(GENERIC_WRAPPER_PREFIX);
    wrapped_code.extend_from_slice(generic_bytes);
    wrapped_code.extend_from_slice(GENERIC_WRAPPER_SUFFIX);

    // SAFETY: We're constructing valid UTF-8 from known ASCII prefix/suffix and user's generic string
    let wrapped_str = unsafe { std::str::from_utf8_unchecked(&wrapped_code) };

    // Allocate in the allocator for lifetime safety
    let wrapped_alloc = allocator.alloc_str(wrapped_str);

    let parser = Parser::new(allocator, wrapped_alloc, SourceType::ts());
    let parsed = parser.parse_expression();

    // Handle parse result
    let expression = match parsed {
        Ok(expr) => expr,
        Err(errors) => {
            return GenericParseResult {
                expression: None,
                position,
                ast_offset,
                params: SmallVec::new(),
                errors,
            };
        }
    };

    // Get the arrow function expression
    let Expression::ArrowFunctionExpression(arrow_fn) = &expression else {
        return GenericParseResult {
            expression: None,
            position,
            ast_offset,
            params: SmallVec::new(),
            errors: vec![OxcDiagnostic::error("Expected arrow function expression")],
        };
    };

    let Some(type_params) = arrow_fn.type_parameters.as_ref() else {
        return GenericParseResult {
            expression: None,
            position,
            ast_offset,
            params: SmallVec::new(),
            errors: vec![OxcDiagnostic::error("No type parameters found")],
        };
    };

    // Extract parameter information
    let mut params: SmallVec<[GenericParam; 4]> = SmallVec::new();

    for param in &type_params.params {
        // Calculate spans relative to the original generic string
        let param_start = param.span.start.saturating_sub(ast_offset);
        let param_end = param.span.end.saturating_sub(ast_offset);

        let name_start = param.name.span.start.saturating_sub(ast_offset);
        let name_end = param.name.span.end.saturating_sub(ast_offset);

        let constraint_span = param.constraint.as_ref().map(|c| {
            let start = c.span().start.saturating_sub(ast_offset);
            let end = c.span().end.saturating_sub(ast_offset);
            Span::new(start, end)
        });

        let default_span = param.default.as_ref().map(|d| {
            let start = d.span().start.saturating_sub(ast_offset);
            let end = d.span().end.saturating_sub(ast_offset);
            Span::new(start, end)
        });

        params.push(GenericParam {
            name_span: Span::new(name_start, name_end),
            span: Span::new(param_start, param_end),
            constraint_span,
            default_span,
        });
    }

    GenericParseResult {
        expression: Some(expression),
        position,
        ast_offset,
        params,
        errors: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> GenericParseResult<'static> {
        let allocator = Box::leak(Box::new(Allocator::default()));
        parse_generic(allocator, source, 0)
    }

    fn parse_with_offset(source: &str, offset: u32) -> GenericParseResult<'static> {
        let allocator = Box::leak(Box::new(Allocator::default()));
        parse_generic(allocator, source, offset)
    }

    #[test]
    fn test_empty_generic() {
        let result = parse("");
        assert!(result.has_errors());
        assert!(!result.is_ok());
        assert!(result.type_parameters().is_none());
    }

    #[test]
    fn test_single_param() {
        let source = "T";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);
        assert_eq!(result.ast_offset, GENERIC_WRAPPER_PREFIX.len() as u32);
        assert_eq!(result.position.start, 0);
        assert_eq!(result.position.end, 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"T");
        assert!(param.constraint_span.is_none());
        assert!(param.default_span.is_none());
    }

    #[test]
    fn test_multiple_params() {
        let source = "T, U, V";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 3);

        let names = result.get_all_param_names(source.as_bytes());
        assert_eq!(names.as_slice(), &[b"T" as &[u8], b"U", b"V"]);
    }

    #[test]
    fn test_param_with_constraint() {
        let source = "T extends string";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"T");
        assert_eq!(
            param.constraint(source.as_bytes()),
            Some(b"string" as &[u8])
        );
        assert!(param.default_span.is_none());
    }

    #[test]
    fn test_param_with_default() {
        let source = "T = string";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"T");
        assert!(param.constraint_span.is_none());
        assert_eq!(
            param.default_type(source.as_bytes()),
            Some(b"string" as &[u8])
        );
    }

    #[test]
    fn test_param_with_constraint_and_default() {
        let source = "T extends object = {}";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"T");
        assert_eq!(
            param.constraint(source.as_bytes()),
            Some(b"object" as &[u8])
        );
        assert_eq!(param.default_type(source.as_bytes()), Some(b"{}" as &[u8]));
    }

    #[test]
    fn test_complex_constraint() {
        let source = "T extends 'foo' | 'bar'";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"T");
        assert_eq!(
            param.constraint(source.as_bytes()),
            Some(b"'foo' | 'bar'" as &[u8])
        );
    }

    #[test]
    fn test_multiple_params_with_constraints() {
        let source = "T extends string, U extends number, V";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 3);

        let names = result.get_all_param_names(source.as_bytes());
        assert_eq!(names.as_slice(), &[b"T" as &[u8], b"U", b"V"]);

        assert_eq!(
            result.params[0].constraint(source.as_bytes()),
            Some(b"string" as &[u8])
        );
        assert_eq!(
            result.params[1].constraint(source.as_bytes()),
            Some(b"number" as &[u8])
        );
        assert!(result.params[2].constraint_span.is_none());
    }

    #[test]
    fn test_file_offset() {
        let source = "T extends Foo";
        let file_offset = 100;
        let result = parse_with_offset(source, file_offset);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.position.start, 100);
        assert_eq!(result.position.end, 100 + source.len() as u32);
    }

    #[test]
    fn test_generic_with_keyof() {
        let source = "K extends keyof T";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(param.name(source.as_bytes()), b"K");
        assert_eq!(
            param.constraint(source.as_bytes()),
            Some(b"keyof T" as &[u8])
        );
    }

    #[test]
    fn test_generic_with_array_type() {
        let source = "T extends Array<string>";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(
            param.constraint(source.as_bytes()),
            Some(b"Array<string>" as &[u8])
        );
    }

    #[test]
    fn test_generic_with_union_default() {
        let source = "T = string | number";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 1);

        let param = &result.params[0];
        assert_eq!(
            param.default_type(source.as_bytes()),
            Some(b"string | number" as &[u8])
        );
    }

    #[test]
    fn test_invalid_generic_syntax() {
        let result = parse("T extends");
        assert!(result.has_errors());
        assert!(!result.is_ok());
    }

    #[test]
    fn test_invalid_generic_missing_comma() {
        let result = parse("T U");
        assert!(result.has_errors());
        assert!(!result.is_ok());
    }

    #[test]
    fn test_ast_offset() {
        let source = "T";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        // The AST spans are offset by the wrapper prefix length
        assert_eq!(result.ast_offset, 1); // "<" is 1 byte

        // Verify we can convert AST span to generic-relative span
        let type_params = result
            .type_parameters()
            .expect("should have type parameters");
        let ast_span = type_params.params[0].span;
        let relative_start = ast_span.start - result.ast_offset;
        let relative_end = ast_span.end - result.ast_offset;
        assert_eq!(relative_start, 0);
        assert_eq!(relative_end, 1);
    }

    #[test]
    fn test_span_content_extraction() {
        let source = "T extends Foo, U = Bar";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        // Test full parameter content extraction
        let p0_content = result.params[0].content(source.as_bytes());
        assert_eq!(p0_content, b"T extends Foo");

        let p1_content = result.params[1].content(source.as_bytes());
        assert_eq!(p1_content, b"U = Bar");
    }

    #[test]
    fn test_real_world_vue_generic() {
        // Real-world example from Vue SFC
        let source = "T extends Record<string, any>, Props = {}";
        let result = parse(source);
        assert!(result.is_ok(), "should parse: {:?}", result.errors);

        assert_eq!(result.param_count(), 2);

        let names = result.get_all_param_names(source.as_bytes());
        assert_eq!(names.as_slice(), &[b"T" as &[u8], b"Props"]);

        assert_eq!(
            result.params[0].constraint(source.as_bytes()),
            Some(b"Record<string, any>" as &[u8])
        );
        assert_eq!(
            result.params[1].default_type(source.as_bytes()),
            Some(b"{}" as &[u8])
        );
    }

    #[test]
    fn test_errors_contain_diagnostic_info() {
        let result = parse("T extends");
        assert!(result.has_errors());
        assert!(!result.errors.is_empty());
        // Errors should contain diagnostic information from OXC
    }

    #[test]
    fn test_error_preserves_position() {
        let result = parse_with_offset("T extends", 50);
        assert!(result.has_errors());
        // Position should still be set even on error
        assert_eq!(result.position.start, 50);
        assert_eq!(result.position.end, 59); // 50 + "T extends".len()
    }
}
