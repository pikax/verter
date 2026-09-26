//! Vue v-slot expression parsing.
//!
//! Parses Vue v-slot expressions which use function parameter syntax.
//! Examples: `{ data }`, `{ item, index = 0 }`, `{ rowData: role }: { rowData: ProjectRole }`
//!
//! The slot content is wrapped as arrow function parameters and parsed:
//! `{ foo, bar }` → `({ foo, bar })=>{}`

use oxc_allocator::{Allocator, StringBuilder};
use oxc_ast::ast::{Expression, FormalParameters};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::span::adjust_diagnostics_spans;
use super::span_shift::shift_formal_parameters_spans;
use crate::common::Span;
use crate::utils::oxc::bindings::{
    collect_expression_free_refs, collect_expression_reference_spans,
    collect_pattern_default_free_ref_names, collect_pattern_local_spans,
    collect_pattern_reference_spans, collect_type_free_ref_names, collect_type_reference_spans,
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
    ///
    /// Default-value references drop global-named identifiers (`Date`, `Map`) — the
    /// runtime `_ctx`-prefixing set. Use [`liveness_reference_names`](Self::liveness_reference_names)
    /// for unused-binding liveness, where a setup binding may shadow a global.
    pub references: Vec<Span>,

    /// Free-reference NAMES for UNUSED-BINDING LIVENESS, collected by the COMPLETE
    /// `Visit` walker over the slot's default-value expressions plus its
    /// type-annotation references.
    ///
    /// Unlike [`references`](Self::references) (the runtime `_ctx`-prefixing span
    /// set, collected by the partial walker that drops globals and does not recurse
    /// into callback bodies), this set:
    /// - is collected by the complete `Visit` walker for BOTH domains — value
    ///   defaults route through `collect_expression_free_refs` and type annotations
    ///   through `collect_type_free_ref_names`, the same `SetupRefCollector` Visit
    ///   driven over a `TSType` — so a binding referenced ONLY inside a nested
    ///   callback in a default (`#default="{ row = list.map(r => fmt(r)) }"`) OR
    ///   ONLY via a `typeof` query buried in a function-type parameter
    ///   (`#default="{ cb }: { cb: (x: typeof Helper) => void }"`) is recorded;
    /// - RETAINS global-named identifiers (a `const Map` binding shadowing the JS
    ///   global, used as `#default="{ row = Map }"`, is a real use);
    /// - carries NAMES (not spans), so liveness never depends on the partial
    ///   wrapped→file-relative span shift (which does not recurse into arrow
    ///   bodies, so a callback-body span would slice the wrong source bytes).
    ///
    /// Feeds ONLY the liveness usage union, never runtime codegen.
    pub liveness_reference_names: Vec<String>,

    /// Free-reference NAMES from the slot's default-value expressions that
    /// resolve to TEMPLATE-SCOPE locals (this slot's own params or an
    /// enclosing v-for/v-slot scope's locals passed via `ignored`).
    /// Complements [`references`] / [`liveness_reference_names`], which
    /// both EXCLUDE scope locals. Consumed by the official-parity
    /// `hasScopeRef` slot-flag decision.
    ///
    /// [`references`]: Self::references
    pub scope_local_reference_names: Vec<String>,
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

/// Extract bindings from the file-relative FormalParameters.
///
/// Returns the local spans, the runtime reference spans, and the liveness
/// reference NAMES. The params tree has already been shifted to file-relative for
/// VALUE positions, so runtime locals/default-value reference spans slice `input`
/// directly; type-annotation reference spans keep the synthetic arrow-wrapper
/// prefix (display-only positions that are never source-mapped), so they take
/// only the `file_offset` shift. LIVENESS carries owned names so it never depends
/// on that partial span shift (which does not recurse into callback bodies).
fn extract_slot_bindings_internal(
    params: &FormalParameters<'_>,
    input: &str,
    file_offset: u32,
    ignored_extra: &[&str],
) -> (Vec<Span>, Vec<Span>, Vec<String>, Vec<String>) {
    let mut locals = Vec::new();
    let mut references_set = FxHashSet::default();
    // LIVENESS reference NAMES, collected by the complete `Visit` walker over BOTH
    // domains — value defaults via `collect_expression_free_refs` and type
    // annotations via `collect_type_free_ref_names` (the same Visit driven over a
    // `TSType`). Globals are RETAINED, a binding referenced only inside a nested
    // callback in a default is recorded, and a `typeof` query nested in any type
    // position (function/method/constructor params, signatures, mapped constraints)
    // is recorded — none of which is possible through the partial runtime span
    // collectors.
    let mut liveness_names: FxHashSet<&str> = FxHashSet::default();

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
        .map(|span| span.slice(input).as_bytes())
        .collect();

    if !ignored_extra.is_empty() {
        for name in ignored_extra {
            ignored.insert(name.as_bytes());
        }
    }

    // Type-annotation reference spans keep the wrapper prefix; collect them apart
    // and apply only the file shift so they land at their published position.
    let mut type_refs: FxHashSet<Span> = FxHashSet::default();

    // Extract references from type annotations and default values.
    //
    // RUNTIME `references_set` (partial `is_global`-filtering walker → spans) and
    // LIVENESS `liveness_names` (complete `Visit` walker → names) are collected
    // independently. The liveness VALUE walker descends into nested callback bodies
    // in a default (`#default="{ row = list.map(r => fmt(r)) }"`), records `fmt`,
    // and retains global-named references. Type-space references feed liveness via
    // the complete `Visit`-over-`TSType` collector (`collect_type_free_ref_names`),
    // whose default `walk::*` traversal reaches EVERY nested type position —
    // function/constructor/method-signature/call/index/construct-signature params,
    // mapped-type constraints, infer, import, template-literal, predicate, and
    // qualified-name roots — so a `typeof Helper` buried in `(x: typeof Helper) =>
    // void` is recorded and never falsely demoted.
    for param in &params.items {
        // Default value (initializer) — already file-relative.
        if let Some(init) = &param.initializer {
            collect_expression_reference_spans(init, &ignored, &mut references_set);
            liveness_names.extend(collect_expression_free_refs(init));
        }
        // Type annotation on the parameter (on FormalParameter, not BindingPattern)
        if let Some(annotation) = &param.type_annotation {
            collect_type_reference_spans(&annotation.type_annotation, &mut type_refs);
            liveness_names.extend(collect_type_free_ref_names(&annotation.type_annotation));
        }
        // References in default values within the pattern — already file-relative.
        collect_pattern_reference_spans(&param.pattern, &ignored, &mut references_set);
        collect_pattern_default_free_ref_names(&param.pattern, &mut liveness_names);
    }
    if let Some(rest) = &params.rest {
        if let Some(annotation) = &rest.type_annotation {
            collect_type_reference_spans(&annotation.type_annotation, &mut type_refs);
            liveness_names.extend(collect_type_free_ref_names(&annotation.type_annotation));
        }
    }

    for mut span in type_refs {
        span.start += file_offset;
        span.end += file_offset;
        references_set.insert(span);
    }

    let references: Vec<Span> = references_set.into_iter().collect();
    // The complete `Visit`/type-reference walkers have no `ignored` parameter
    // (they suppress only lexically-shadowed names); the slot's own param locals
    // are declared outside the default expressions, so partition here by name:
    // non-ignored names feed liveness, ignored (template-scope) names feed the
    // scope-local reference set for the slot-flag `hasScopeRef` decision.
    let mut liveness_reference_names: Vec<String> = Vec::new();
    let mut scope_local_reference_names: Vec<String> = Vec::new();
    for name in liveness_names {
        if ignored.contains(name.as_bytes()) {
            scope_local_reference_names.push(name.to_string());
        } else {
            liveness_reference_names.push(name.to_string());
        }
    }
    (
        locals,
        references,
        liveness_reference_names,
        scope_local_reference_names,
    )
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

    // Parse the wrapped slice, then shift every span straight to file-relative
    // in a single walk (strip the arrow-wrapper prefix and add the file offset).
    let mut result = parse_vslot_internal(allocator, source, source_type);

    if let Some(params) = &mut result.params {
        shift_formal_parameters_spans(params, result.offset, span.start);
    }
    if let Some(errors) = &mut result.errors {
        adjust_diagnostics_spans(errors, span.start);
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
    let mut result = parse_vslot_internal(allocator, source, source_type);
    // No file offset: shifting by the wrapper offset alone strips the synthetic
    // prefix and yields source-relative spans.
    if let Some(params) = &mut result.params {
        shift_formal_parameters_spans(params, result.offset, 0);
    }
    result
}

/// Internal v-slot parsing logic. Returns wrapped-relative spans (positions in
/// the synthetic `({content})=>{}` wrapper); callers shift them to source- or
/// file-relative via [`shift_formal_parameters_spans`].
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

    // Wrap as arrow-function parameters — `({content})=>{}` — concatenated
    // straight into the parser arena in a single allocation, with no transient
    // heap `String`. The leading `(` shifts every parsed span by one byte.
    const WRAPPER_OFFSET: u32 = 1;
    let wrapped = StringBuilder::from_strs_array_in(["(", source, ")=>{}"], allocator).into_str();

    // Parse as expression
    let parser = Parser::new(allocator, wrapped, source_type);
    let result = parser.parse_expression();

    match result {
        Ok(expr) => {
            // Extract FormalParameters from ArrowFunctionExpression
            if let Expression::ArrowFunctionExpression(arrow) = expr {
                let params = arrow.unbox().params.unbox();
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

    // Parse the wrapped slice, then shift every AST span straight to file-relative
    // in one walk; binding extraction reads that file-relative tree directly.
    let mut result = parse_vslot_internal(allocator, source, source_type);
    if let Some(params) = &mut result.params {
        shift_formal_parameters_spans(params, result.offset, offset);
    }
    if let Some(errors) = &mut result.errors {
        adjust_diagnostics_spans(errors, offset);
    }

    let (locals, references, liveness_reference_names, scope_local_reference_names) =
        if result.errors.is_some() {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        } else if let Some(params) = &result.params {
            extract_slot_bindings_internal(params, input, offset, ignored)
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };

    VSlotWithBindings {
        result,
        locals,
        references,
        liveness_reference_names,
        scope_local_reference_names,
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

        // "data" binding lands at its exact file-relative position 22..26.
        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            assert_eq!((obj.span.start, obj.span.end), (20, 28));
            if let BindingPattern::BindingIdentifier(id) = &obj.properties[0].value {
                assert_eq!(id.name.as_str(), "data");
                assert_eq!(id.span.start, 22);
                assert_eq!(id.span.end, 26);
                assert_eq!(&input[id.span.start as usize..id.span.end as usize], "data");
            } else {
                panic!("Expected BindingIdentifier");
            }
        } else {
            panic!("Expected ObjectPattern");
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

        // Local "data" lands at its exact file-relative position 22..26.
        assert_eq!(wb.locals.len(), 1);
        assert_eq!((wb.locals[0].start, wb.locals[0].end), (22, 26));
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

        // Local "data" lands at its exact file-relative position 9..13.
        assert_eq!(wb.locals.len(), 1);
        assert_eq!((wb.locals[0].start, wb.locals[0].end), (9, 13));
        assert_eq!(wb.locals[0].slice(input), "data");

        // The lone reference is the `MyType` type-annotation identifier. Its span
        // keeps the one-byte arrow-wrapper prefix (a display-only position), so it
        // sits one byte after the textual `MyType` start — pinned exactly here.
        assert_eq!(wb.references.len(), 1);
        assert_eq!((wb.references[0].start, wb.references[0].end), (26, 32));
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

    // ── exact single-pass span characterization ────────────────────
    //
    // These pin absolute byte positions so a one-byte placement drift in the
    // wrapped→file-relative shift fails. They lock in the published positions
    // of every v-slot output category: structural param spans, the saturating
    // synthetic-prefix mapping, default-value references, display-only
    // type-annotation references, and malformed-diagnostic label offsets.

    /// @ai-generated - Structural param spans land at exact absolute positions.
    #[test]
    fn exact_sliced_structural_param_spans_at_offset() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let input = r#"<template #default="{ data }"></template>"#;
        // "{ data }" occupies bytes 20..28.
        let result = parse_vslot_sliced(allocator, Span::new(20, 28), input, SourceType::tsx());
        assert!(result.is_ok());
        let params = result.params.unwrap();

        // The FormalParameters span covers the synthetic `(...)` wrapper, so its
        // end maps one byte past the slice; its start saturates to the slice start.
        assert_eq!((params.span.start, params.span.end), (20, 29));
        assert_eq!(
            (params.items[0].span.start, params.items[0].span.end),
            (20, 28)
        );
        if let BindingPattern::ObjectPattern(obj) = &params.items[0].pattern {
            assert_eq!((obj.span.start, obj.span.end), (20, 28));
            assert_eq!(
                (obj.properties[0].span.start, obj.properties[0].span.end),
                (22, 26)
            );
        } else {
            panic!("Expected ObjectPattern");
        }
    }

    /// @ai-generated - The synthetic arrow-wrapper `(` maps to no source byte: its
    /// position saturates to the content start rather than underflowing below it.
    #[test]
    fn exact_negative_synthetic_prefix_saturates_to_content_start() {
        let allocator = Box::leak(Box::new(Allocator::default()));

        // Raw parse: the wrapper paren at wrapped offset 0 saturates to 0, it does
        // not wrap around to u32::MAX. "data" itself starts at byte 0.
        let raw = parse_vslot(allocator, "data", SourceType::tsx());
        let rp = raw.params.unwrap();
        assert_eq!(rp.span.start, 0);
        if let BindingPattern::BindingIdentifier(id) = &rp.items[0].pattern {
            assert_eq!((id.span.start, id.span.end), (0, 4));
        } else {
            panic!("Expected BindingIdentifier");
        }

        // Sliced parse: params.span.start lands on the slice start (the content),
        // never one byte earlier where the synthetic `(` would otherwise map.
        let input = r#"<div #s="data">"#;
        let sliced = parse_vslot_sliced(allocator, Span::new(9, 13), input, SourceType::tsx());
        let sp = sliced.params.unwrap();
        assert_eq!(sp.span.start, 9);
        if let BindingPattern::BindingIdentifier(id) = &sp.items[0].pattern {
            assert_eq!((id.span.start, id.span.end), (9, 13));
            assert_eq!(&input[id.span.start as usize..id.span.end as usize], "data");
        } else {
            panic!("Expected BindingIdentifier");
        }
    }

    /// @ai-generated - Default-value references are unwrapped to their true source
    /// position; type-annotation references keep the display-only wrapper prefix.
    #[test]
    fn exact_default_and_type_references_at_offset() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let input = "prefix { data = fallback }: { data: MyType } suffix";
        let start = 7u32;
        let end = start + "{ data = fallback }: { data: MyType }".len() as u32;
        let wb = parse_vslot_with_bindings_sliced(
            allocator,
            Some(Span::new(start, end)),
            input,
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());

        let params = wb.result.params.as_ref().unwrap();
        assert_eq!((params.span.start, params.span.end), (7, 45));
        // The type-annotation span keeps the wrapper prefix (display-only).
        let ta = params.items[0].type_annotation.as_ref().unwrap();
        assert_eq!((ta.span.start, ta.span.end), (27, 45));

        // Exactly one local, at its true source position.
        assert_eq!(wb.locals.len(), 1);
        assert_eq!((wb.locals[0].start, wb.locals[0].end), (9, 13));
        assert_eq!(wb.locals[0].slice(input), "data");

        let mut refs: Vec<(u32, u32)> = wb.references.iter().map(|s| (s.start, s.end)).collect();
        refs.sort_unstable();
        // `fallback` (default value) is unwrapped to its true source span; the
        // `MyType` type reference keeps the one-byte wrapper prefix.
        assert_eq!(refs, vec![(16, 24), (37, 43)]);
        assert_eq!(input.get(16..24), Some("fallback"));
    }

    /// @ai-generated - Default-value references inside compound expressions are
    /// value-position references, so they are unwrapped to their TRUE source byte
    /// — never left one byte high carrying the synthetic arrow-wrapper prefix.
    ///
    /// Every kind exercised here (binary, conditional, logical, unary, and a nested
    /// mix whose binary parent contains a call + member access) reaches the
    /// reference collector, so each operand must land on its exact source span. A
    /// walker that lets any compound kind fall through to a file-offset-only
    /// adjustment leaves these one byte too high and fails here.
    #[test]
    fn exact_complex_default_value_references() {
        let allocator = Box::leak(Box::new(Allocator::default()));

        // `{ x = a + b }` — `a` is source byte 6, `b` is byte 10.
        let binary = parse_vslot_with_bindings(allocator, "{ x = a + b }", SourceType::tsx(), &[]);
        let mut br: Vec<(u32, u32)> = binary.references.iter().map(|s| (s.start, s.end)).collect();
        br.sort_unstable();
        assert_eq!(br, vec![(6, 7), (10, 11)]);
        assert_eq!("{ x = a + b }".get(6..7), Some("a"));
        assert_eq!("{ x = a + b }".get(10..11), Some("b"));

        // `{ x = c ? d : e }` — `c` byte 6, `d` byte 10, `e` byte 14.
        let cond =
            parse_vslot_with_bindings(allocator, "{ x = c ? d : e }", SourceType::tsx(), &[]);
        let mut cr: Vec<(u32, u32)> = cond.references.iter().map(|s| (s.start, s.end)).collect();
        cr.sort_unstable();
        assert_eq!(cr, vec![(6, 7), (10, 11), (14, 15)]);
        assert_eq!("{ x = c ? d : e }".get(6..7), Some("c"));
        assert_eq!("{ x = c ? d : e }".get(14..15), Some("e"));

        // `{ x = a && b }` — logical operands at bytes 6 and 11.
        let logical =
            parse_vslot_with_bindings(allocator, "{ x = a && b }", SourceType::tsx(), &[]);
        let mut lr: Vec<(u32, u32)> = logical
            .references
            .iter()
            .map(|s| (s.start, s.end))
            .collect();
        lr.sort_unstable();
        assert_eq!(lr, vec![(6, 7), (11, 12)]);
        assert_eq!("{ x = a && b }".get(11..12), Some("b"));

        // `{ x = !a }` — unary operand at byte 7.
        let unary = parse_vslot_with_bindings(allocator, "{ x = !a }", SourceType::tsx(), &[]);
        let ur: Vec<(u32, u32)> = unary.references.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ur, vec![(7, 8)]);
        assert_eq!("{ x = !a }".get(7..8), Some("a"));

        // `{ x = f(a) + b.c }` — the binary parent (a kind the old whitelist dropped
        // to offset-only) must not strand its call + member operands one byte high:
        // `f` byte 6, `a` byte 8, `b` byte 13 (`c` is a property, not a reference).
        let nested =
            parse_vslot_with_bindings(allocator, "{ x = f(a) + b.c }", SourceType::tsx(), &[]);
        let mut nr: Vec<(u32, u32)> = nested.references.iter().map(|s| (s.start, s.end)).collect();
        nr.sort_unstable();
        assert_eq!(nr, vec![(6, 7), (8, 9), (13, 14)]);
        assert_eq!("{ x = f(a) + b.c }".get(6..7), Some("f"));
        assert_eq!("{ x = f(a) + b.c }".get(13..14), Some("b"));
    }

    /// @ai-generated - A shorthand-object default (`{ x = { foo } }`) collects the
    /// shorthand key `foo` as its reference; that key is a value-position binding
    /// site and must be unwrapped to its true source byte, not left wrapper-high.
    #[test]
    fn exact_shorthand_object_default_reference_unwrapped() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        // `{ x = { foo } }` — `foo` is source byte 8.
        let wb = parse_vslot_with_bindings(allocator, "{ x = { foo } }", SourceType::tsx(), &[]);
        assert!(wb.is_ok());
        let refs: Vec<(u32, u32)> = wb.references.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(refs, vec![(8, 11)]);
        assert_eq!("{ x = { foo } }".get(8..11), Some("foo"));
    }

    /// @ai-generated - Raw type-annotation reference keeps the wrapper prefix.
    #[test]
    fn exact_raw_type_reference_retains_wrapper_prefix() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ data }: { data: MyType }",
            SourceType::tsx(),
            &[],
        );
        assert_eq!(wb.references.len(), 1);
        // Textual `MyType` starts at byte 18; the published span keeps the wrapper
        // prefix and so begins one byte later, at 19.
        assert_eq!((wb.references[0].start, wb.references[0].end), (19, 25));
    }

    /// @ai-generated - Malformed slot diagnostic label offset equals the raw label
    /// offset plus the slice start (a single uniform file shift, no wrapper unwrap).
    #[test]
    fn exact_malformed_diagnostic_label_offset_is_raw_plus_slice_start() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let malformed = "{ invalid: }";
        let raw = parse_vslot(allocator, malformed, SourceType::tsx());
        let raw_off = raw
            .errors
            .as_ref()
            .and_then(|e| e.first())
            .and_then(|d| d.labels.as_ref())
            .and_then(|l| l.first())
            .map(|l| l.offset())
            .expect("raw malformed parse must produce a labelled diagnostic");

        let slice_start = 13u32;
        let prefix = "x".repeat(slice_start as usize);
        let input = format!("{prefix}{malformed}");
        let sliced = parse_vslot_sliced(
            allocator,
            Span::new(slice_start, slice_start + malformed.len() as u32),
            &input,
            SourceType::tsx(),
        );
        let sliced_off = sliced
            .errors
            .as_ref()
            .and_then(|e| e.first())
            .and_then(|d| d.labels.as_ref())
            .and_then(|l| l.first())
            .map(|l| l.offset())
            .expect("sliced malformed parse must produce a labelled diagnostic");

        assert_eq!(sliced_off, raw_off + slice_start as usize);
    }

    /// A global-named identifier in a v-slot default-value (`{ row = Map }`) is
    /// DROPPED from the runtime `references` set (so it is not `_ctx`-prefixed) but
    /// RETAINED in `liveness_reference_names` (so a `const Map` setup binding
    /// shadowing the global is not falsely reported unused). Discriminating: would
    /// FAIL if liveness reused the runtime `is_global` filter.
    #[test]
    fn global_named_default_value_dropped_from_runtime_kept_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(allocator, "{ row = Map }", SourceType::tsx(), &[]);

        assert!(wb.is_ok());
        assert!(
            wb.references.is_empty(),
            "runtime references must DROP the global-named default `Map`, got: {:?}",
            wb.references
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            wb.liveness_reference_names,
            vec!["Map".to_string()],
            "liveness names must RETAIN the global-named default `Map`"
        );
    }

    /// A `typeof X` reference nested inside a `TSFunctionType` parameter of a
    /// v-slot type annotation (`{ cb }: { cb: (x: typeof Helper) => void }`) is
    /// recorded in `liveness_reference_names`. The retired partial type walker
    /// visited only a function type's `return_type`, dropping its params, so the
    /// `typeof Helper` query was never reached — `Helper` would demote to a
    /// type-only read and false-positive TS6133. Discriminating: FAILS on the
    /// partial type walker.
    #[test]
    fn typeof_in_function_type_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ cb }: { cb: (x: typeof Helper) => void }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside a function-type PARAM must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in a `TSMethodSignature` parameter of a type-literal
    /// annotation. The retired walker visited only a method signature's
    /// `return_type`, dropping its params. Discriminating: FAILS on the partial
    /// walker.
    #[test]
    fn typeof_in_method_signature_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ api }: { api: { run(x: typeof Helper): void } }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside a method-signature PARAM must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in a call signature of a type-literal annotation. A call
    /// signature (`{ (x: typeof Helper): void }`) was entirely behind the retired
    /// walker's `_ => {}` arm. Discriminating: FAILS on the partial walker.
    #[test]
    fn typeof_in_call_signature_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ cb }: { cb: { (x: typeof Helper): void } }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside a CALL signature must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in an index signature of a type-literal annotation. An
    /// index signature (`{ [k: string]: typeof Helper }`) was behind the retired
    /// walker's `_ => {}` arm. Discriminating: FAILS on the partial walker.
    #[test]
    fn typeof_in_index_signature_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ map }: { map: { [k: string]: typeof Helper } }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside an INDEX signature must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in a construct signature of a type-literal annotation. A
    /// construct signature (`{ new (x: typeof Helper): Foo }`) was behind the
    /// retired walker's `_ => {}` arm. Discriminating: FAILS on the partial walker.
    #[test]
    fn typeof_in_construct_signature_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ ctor }: { ctor: { new (x: typeof Helper): Foo } }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside a CONSTRUCT signature must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in a `TSConstructorType` parameter. A constructor type
    /// (`new (x: typeof Helper) => Foo`) was entirely behind the retired walker's
    /// `_ => {}` arm. Discriminating: FAILS on the partial walker.
    #[test]
    fn typeof_in_constructor_type_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ ctor }: { ctor: new (x: typeof Helper) => Foo }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` inside a CONSTRUCTOR type param must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// `typeof X` nested in a mapped-type constraint. A mapped type's constraint
    /// (`{ [K in keyof typeof Helper]: V }`) was dropped — the retired walker
    /// followed only the mapped value `type_annotation`. Discriminating: FAILS on
    /// the partial walker.
    #[test]
    fn typeof_in_mapped_type_constraint_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ rec }: { rec: { [K in keyof typeof Helper]: string } }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` in a MAPPED-TYPE constraint must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// A nested function-type RETURN position whose return is itself a function
    /// type carrying `typeof X` in ITS param (`() => (x: typeof Helper) => void`).
    /// The retired walker recursed return types but never params, so the inner
    /// param's `typeof Helper` was missed. Discriminating: FAILS on the partial
    /// walker.
    #[test]
    fn typeof_in_nested_function_type_return_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ make }: { make: () => (x: typeof Helper) => void }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"Helper".to_string()),
            "`typeof Helper` in a nested function-type's PARAM must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// A bare named type in a `TSFunctionType` param (no `typeof`) is also
    /// recorded — a v-slot default-slot binding can reference a setup VALUE binding
    /// from a type position via `typeof`, but a setup-declared TYPE name used as a
    /// param annotation must keep its declaration live too. Routing through the
    /// complete `Visit` over the type collects every type-name leaf in param
    /// position. Discriminating: FAILS on the partial walker (function-type params
    /// were skipped entirely).
    #[test]
    fn named_type_in_function_type_param_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ cb }: { cb: (x: HelperType) => void }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names
                .contains(&"HelperType".to_string()),
            "a named type in a function-type PARAM must be recorded; got {:?}",
            wb.liveness_reference_names
        );
    }

    /// A binding referenced ONLY inside a nested callback in a v-slot default
    /// (`{ row = list.map(r => fmt(r)) }`) is recorded in `liveness_reference_names`.
    /// The retired partial liveness span walker dropped the arrow-function argument
    /// body, missing `fmt`. Discriminating: would FAIL on the partial walker.
    #[test]
    fn nested_callback_default_value_recorded_for_liveness() {
        let allocator = Box::leak(Box::new(Allocator::default()));
        let wb = parse_vslot_with_bindings(
            allocator,
            "{ row = list.map(r => fmt(r)) }",
            SourceType::tsx(),
            &[],
        );
        assert!(wb.is_ok());
        assert!(
            wb.liveness_reference_names.contains(&"list".to_string()),
            "default-value receiver `list` recorded; got {:?}",
            wb.liveness_reference_names
        );
        assert!(
            wb.liveness_reference_names.contains(&"fmt".to_string()),
            "a reference inside the `.map(..)` callback BODY must be recorded; got {:?}",
            wb.liveness_reference_names
        );
        // `r` is the callback param (inner-scope local) and must NOT leak.
        assert!(
            !wb.liveness_reference_names.contains(&"r".to_string()),
            "callback param `r` stays excluded; got {:?}",
            wb.liveness_reference_names
        );
    }
}
