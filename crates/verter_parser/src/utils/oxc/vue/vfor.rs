//! Vue v-for expression parsing.
//!
//! Parses Vue v-for expressions like `item of items` or `(item, key, index) in obj`.
//!
//! The parser splits on ` of ` or ` in ` and parses left and right sides separately,
//! which properly handles Vue's multi-variable syntax.

use memchr::memmem::find;
use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKey};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::span::{adjust_diagnostics_spans, adjust_expression_spans};
use crate::common::Span;
use crate::utils::oxc::bindings::{
    collect_expression_free_refs, collect_expression_reference_spans,
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
    ///
    /// Drops global-named identifiers (`Date`, `Map`) — the runtime
    /// `_ctx`-prefixing set. Use [`liveness_reference_names`](Self::liveness_reference_names)
    /// for unused-binding liveness, where a setup binding may shadow a global.
    pub references: Vec<Span>,

    /// Free-reference NAMES for UNUSED-BINDING LIVENESS, collected by the COMPLETE
    /// `Visit` walker over the v-for source expression.
    ///
    /// Unlike [`references`](Self::references) (the runtime `_ctx`-prefixing span
    /// set, collected by the partial walker that drops globals and does not
    /// recurse into callback bodies), this set:
    /// - is collected by the complete `Visit` walker, so a binding referenced ONLY
    ///   inside a nested callback in the source (`v-for="x in rows.map(r => fmt(r))"`)
    ///   is recorded;
    /// - RETAINS global-named identifiers (a `<script setup>` binding may shadow a
    ///   JS global, so `v-for="x in Date"` is a real use of a `const Date` binding);
    /// - carries NAMES (not spans), so liveness never depends on the partial
    ///   wrapped→file-relative span shift.
    ///
    /// Feeds ONLY the liveness usage union, never runtime codegen.
    pub liveness_reference_names: Vec<String>,
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

/// Extract bindings from a VForParseResult.
///
/// This is an internal function used by `parse_vfor_with_bindings`. Returns the
/// local spans, the runtime reference spans, and the liveness reference NAMES.
/// The runtime references are spans (a self-referential-struct workaround);
/// liveness carries owned names so it never depends on the partial span shift.
fn extract_vfor_bindings_internal(
    result: &VForParseResult<'_>,
    input: &str,
    ignored_extra: &[&str],
) -> (Vec<Span>, Vec<Span>, Vec<String>) {
    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();

    // Extract local spans from the left side (iteration variables)
    if let Some(left) = &result.left {
        collect_vfor_left_local_spans(left, &mut locals);
    }

    // Build ignored set from local names (need the actual strings to filter
    // references). The result's spans are file-relative, so they slice `input`.
    let mut ignored: FxHashSet<&[u8]> = locals
        .iter()
        .map(|span| span.slice(input).as_bytes())
        .collect();

    if !ignored_extra.is_empty() {
        for name in ignored_extra {
            ignored.insert(name.as_bytes());
        }
    }

    // Extract reference spans from the right side (the iterable).
    // Note: we only collect runtime references, NOT TypeScript type references.
    // TS type assertions (e.g. `as Foo`) are preserved in SSR output (stripped later
    // by the bundler), so type identifiers must not be added to the reference set.
    //
    // `references` is the RUNTIME `_ctx`-prefixing set (drops globals via the
    // partial `is_global`-filtering walker). `liveness_reference_names` is the
    // unused-binding LIVENESS set: it routes through the COMPLETE `Visit` name
    // collector, so a setup binding referenced ONLY inside a nested callback in the
    // source (`v-for="x in rows.map(r => fmt(r))"`) is recorded, and global-named
    // references (`v-for="x in Date"` over a `const Date` binding) are retained.
    let mut liveness_reference_names: Vec<String> = Vec::new();
    if let Some(right) = &result.right {
        collect_expression_reference_spans(right, &ignored, &mut references_set);
        // The complete `Visit` walker has no `ignored` parameter (it suppresses
        // only lexically-shadowed names); the v-for LEFT locals are declared
        // outside the source expression, so exclude them here by name.
        for name in collect_expression_free_refs(right) {
            if !ignored.contains(name.as_bytes()) {
                liveness_reference_names.push(name.to_string());
            }
        }
    }

    let mut references: Vec<Span> = references_set.into_iter().collect();
    // Sort by start position — downstream consumers (prefix_vfor_references_into)
    // use a forward-scanning cursor that assumes ascending order.
    references.sort_unstable_by_key(|s| s.start);
    (locals, references, liveness_reference_names)
}

/// Parse a Vue v-for expression from a span within a larger source string.
///
/// This is the primary implementation. All AST spans, diagnostic spans, and
/// offset fields in the result are adjusted to be relative to the full `input`,
/// not the extracted substring.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `span` - The byte range within `input` containing the v-for expression
/// * `input` - The full source string (e.g., the entire SFC file)
/// * `source_type` - The source type (e.g., TSX, JavaScript)
/// * `ignored` - Identifiers to ignore when collecting references
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// //                0         1         2         3
/// //                0123456789012345678901234567890123456
/// let input = r#"<div v-for="item of items"></div>"#;
/// // The v-for value "item of items" spans bytes 12..25
/// let result = parse_vfor_sliced(&allocator, Span::new(12, 25), input, SourceType::tsx());
/// assert!(result.is_ok());
/// // All spans are file-relative:
/// // result.left_offset == 12
/// // result.right_offset == 20 (12 + 8)
/// ```
pub fn parse_vfor_sliced<'a>(
    allocator: &'a Allocator,
    span: Span,
    input: &'a str,
    source_type: SourceType,
) -> VForParseResult<'a> {
    if span.start >= span.end {
        return VForParseResult {
            left: None,
            right: None,
            is_of: false,
            left_offset: span.start,
            right_offset: span.start,
            left_errors: vec![],
            right_errors: vec![],
        };
    }

    let source = &input[span.start as usize..span.end as usize];
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
                left_offset: span.start,
                right_offset: span.start,
                left_errors: vec![OxcDiagnostic::error(
                    "Invalid v-for expression: missing 'in' or 'of' keyword",
                )],
                right_errors: vec![],
            };
        }
    };

    // Split into left and right parts. Both are borrowed slices of `input`
    // (lifetime `'a`), so they feed the parser directly — no arena copy.
    let left_str = &source[..separator_pos];
    let right_start = separator_pos + 4; // " of " or " in " is 4 bytes
    let right_str = &source[right_start..];

    // Parse left side as expression
    let left_parser = Parser::new(allocator, left_str, source_type);
    let left_result = left_parser.parse_expression();

    // Parse right side as expression
    let right_parser = Parser::new(allocator, right_str, source_type);
    let right_result = right_parser.parse_expression();

    // The right expression offset within the v-for substring
    let right_offset_in_substring = right_start as u32;

    // Extract left and errors
    let (left, left_errors) = match left_result {
        Ok(mut expr) => {
            // Adjust left expression spans to be input-relative
            adjust_expression_spans(&mut expr, span.start);
            (Some(expr), vec![])
        }
        Err(mut errors) => {
            adjust_diagnostics_spans(&mut errors, span.start);
            (None, errors)
        }
    };

    // Extract right and errors, adjusting spans to be input-relative
    let (right, right_errors) = match right_result {
        Ok(mut expr) => {
            // Adjust by right_offset_in_substring + span.start to get file-relative
            adjust_expression_spans(&mut expr, right_offset_in_substring + span.start);
            (Some(expr), vec![])
        }
        Err(mut errors) => {
            adjust_diagnostics_spans(&mut errors, right_offset_in_substring + span.start);
            (None, errors)
        }
    };

    VForParseResult {
        left,
        right,
        is_of,
        left_offset: span.start,
        right_offset: right_offset_in_substring + span.start,
        left_errors,
        right_errors,
    }
}

/// Parse a Vue v-for expression from a raw string.
///
/// Convenience wrapper around [`parse_vfor_sliced`] that treats the entire
/// `source` string as the expression. All spans in the result are relative
/// to `source` (starting from 0).
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vfor(&allocator, "item of items", SourceType::tsx());
/// assert!(result.is_ok());
/// assert!(result.is_of);
/// ```
pub fn parse_vfor<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
) -> VForParseResult<'a> {
    parse_vfor_sliced(
        allocator,
        Span::new(0, source.len() as u32),
        source,
        source_type,
    )
}

pub fn extract_vfor_positions(bytes: &[u8], start: u32, end: u32) -> Option<(u32, u32, u32, bool)> {
    let source_bytes = &bytes[start as usize..end as usize];

    if let Some(pos) = find(source_bytes, b" in ") {
        Some((start, start + pos as u32, start + pos as u32 + 4, false))
    } else {
        find(source_bytes, b" of ")
            .map(|pos| (start, start + pos as u32, start + pos as u32 + 4, true))
    }
}

/// Parse a Vue v-for expression from a span and extract bindings in one pass.
///
/// This is the preferred function when you need both the parsed AST and the
/// extracted bindings with file-relative spans. Binding extraction happens on
/// the substring-relative AST, then all spans are adjusted to be input-relative.
///
/// # Arguments
/// * `allocator` - The OXC allocator for AST memory
/// * `span` - The byte range within `input` containing the v-for expression
/// * `input` - The full source string (e.g., the entire SFC file)
/// * `source_type` - The source type (e.g., TSX, JavaScript)
///
/// # Returns
/// A `VForWithBindings` with all spans (AST, locals, references) file-relative.
pub fn parse_vfor_with_bindings_sliced<'a>(
    allocator: &'a Allocator,
    span: Span,
    input: &'a str,
    source_type: SourceType,
    ignored: &[&str],
) -> VForWithBindings<'a> {
    // `parse_vfor_sliced` returns file-relative spans for the left and right
    // expressions, so bindings are collected against `input` directly in a single
    // pass — no re-slice, no re-parse, no per-span shift.
    let result = parse_vfor_sliced(allocator, span, input, source_type);

    let (locals, references, liveness_reference_names) =
        if result.has_left_errors() || result.has_right_errors() {
            (Vec::new(), Vec::new(), Vec::new())
        } else {
            extract_vfor_bindings_internal(&result, input, ignored)
        };

    VForWithBindings {
        result,
        locals,
        references,
        liveness_reference_names,
    }
}

/// Parse a Vue v-for expression from a raw string and extract bindings.
///
/// Convenience wrapper around [`parse_vfor_with_bindings_sliced`] that treats
/// the entire `source` string as the expression.
///
/// # Example
/// ```ignore
/// let allocator = Allocator::default();
/// let result = parse_vfor_with_bindings(&allocator, "(item, index) of items", SourceType::tsx(), &[]);
/// assert!(result.is_ok());
/// ```
pub fn parse_vfor_with_bindings<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
    ignored: &[&str],
) -> VForWithBindings<'a> {
    parse_vfor_with_bindings_sliced(
        allocator,
        Span::new(0, source.len() as u32),
        source,
        source_type,
        ignored,
    )
}

#[cfg(test)]
#[path = "vfor_tests.rs"]
mod vfor_tests;
